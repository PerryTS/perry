use super::*;

fn module() -> perry_hir::Module {
    let mut source =
        String::from("const shared = 'shared'; const proto = { method() { return 'ok'; } };\n");
    for i in 0..72 {
        source.push_str(&format!(
            "function fn{i}(x: any) {{ return [x.name, 'shared', 'str{i}', '\\ud800']; }}\n\
             const cb{i} = (x: any) => {{ const obj = {{name: 'str{i}', method() {{ return shared; }} }}; \
             const a = [x, obj]; a.push(obj); return [x.name, obj.method(), a[0], /ok/.test('ok'), '\\ud800']; }};\n"
        ));
    }
    for i in 0..72 {
        source.push_str(&format!(
            "class C{i} {{ value: number; constructor(x: number) {{ this.value = x; }} \
             get doubled() {{ return this.value * 2; }} \
             set doubled(x: number) {{ this.value = x / 2; }} \
             method(x: any) {{ return x.name + this.value; }} \
             static tag() {{ return 'shared'; }} }}\n"
        ));
    }
    source.push_str("console.log(fn0(cb0), new C0(1).method({ name: 'ok' }));");
    let parsed = perry_parser::parse_typescript(&source, "callable-workers.ts").unwrap();
    perry_hir::lower_module(&parsed, "callable-workers", "callable-workers.ts").unwrap()
}

#[test]
fn callable_workers_preserve_serial_ir() {
    let compile = |jobs| {
        with_jobs(jobs, || {
            crate::compile_module(
                &module(),
                crate::CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap()
        })
    };
    let serial = compile(1);
    let parallel = compile(4);
    assert_same(&serial, &parallel);
    assert_same(&parallel, &compile(3));
    let text = String::from_utf8(serial).unwrap();
    assert!(
        text.matches("define ").count() > 140,
        "callables must be live"
    );
    assert!(
        text.contains("perry_regexp_site_"),
        "site relocation must be live"
    );
    assert!(text.contains(".str."), "literal relocation must be live");
    assert!(!text.contains("__perry_callable_site_"));
}

#[test]
fn owned_function_roundtrip_keeps_builder_state() {
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    let f = module.define_function("worker", crate::types::DOUBLE, vec![]);
    let address = f.alloca_entry(crate::types::DOUBLE);
    let block = f.create_block("entry");
    block.store(crate::types::DOUBLE, "4.0", &address);
    let value = block.load(crate::types::DOUBLE, &address);
    block.ret(crate::types::DOUBLE, &value);
    let before = module.to_ir();
    let input = module.callable_input();
    let output = std::thread::spawn(move || {
        // Constructed on this worker; only owned fields are returned.
        let mut local = input.module();
        let f = local.define_function("worker", crate::types::DOUBLE, vec![]);
        let address = f.alloca_entry(crate::types::DOUBLE);
        let block = f.create_block("entry");
        block.store(crate::types::DOUBLE, "4.0", &address);
        let value = block.load(crate::types::DOUBLE, &address);
        block.ret(crate::types::DOUBLE, &value);
        local.into_callable_output()
    })
    .join()
    .unwrap();
    let mut restored = LlModule::new("x86_64-unknown-linux-gnu");
    restored.merge_callable(output, &restored.callable_input(), Default::default());
    assert_eq!(before, restored.to_ir());
}

#[test]
fn callable_lowering_uses_multiple_owned_workers() {
    let owners = std::sync::Mutex::new(std::collections::HashSet::new());
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    let mut strings = StringPool::new();
    let items: Vec<u32> = (0..72).collect();
    with_jobs(4, || {
        lower(
            &mut module,
            &mut strings,
            &items,
            true,
            |module, strings, id| {
                owners.lock().unwrap().insert(std::thread::current().id());
                let index = strings.intern(&format!("literal{}", id % 7));
                let global = strings.entry(index).handle_global.clone();
                let f = module.define_function(format!("f{id}"), crate::types::DOUBLE, vec![]);
                let block = f.create_block("entry");
                let value = block.load(crate::types::DOUBLE, &format!("@{global}"));
                block.ret(crate::types::DOUBLE, &value);
                Ok(())
            },
            |_| {},
        )
    })
    .unwrap();
    assert_eq!(
        owners.into_inner().unwrap().len(),
        4,
        "parallel path must be live"
    );
    assert_eq!(module.function_count(), 72);
    assert_eq!(
        strings.len(),
        7,
        "group-local literals must coalesce in serial order"
    );
    let ir = module.to_ir();
    assert!(ir.contains("define double @f71"));
}

fn assert_same(expected: &[u8], actual: &[u8]) {
    if expected == actual {
        return;
    }
    let a = String::from_utf8_lossy(expected);
    let b = String::from_utf8_lossy(actual);
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        assert_eq!(x, y, "first IR difference at line {}", i + 1);
    }
    panic!("IR length differs: {} vs {}", expected.len(), actual.len());
}

#[test]
fn callable_preinline_budget_uses_serial_symbol_lengths() {
    use crate::codegen::callable_reloc::Relocations;
    use crate::types::DOUBLE;
    for (suffix, admitted) in [("1", true), ("1000000", false)] {
        let mut f = crate::function::LlFunction::new("edge", DOUBLE, vec![]);
        let b = f.create_block("entry");
        let value = b.load(DOUBLE, "@m_.str.1.handle");
        let base = f.estimated_ir_bytes();
        let padding = 16 * 1024 - base - 3;
        let b = f.block_mut(0).unwrap();
        b.emit_raw(format!(";{}", "x".repeat(padding - 1)));
        b.ret(DOUBLE, &value);
        // Include the return's contribution in the exact boundary adjustment.
        let excess = f.estimated_ir_bytes() - 16 * 1024;
        let b = f.block_mut(0).unwrap();
        let crate::inst::LlInst::Raw(comment) = &mut b.insts_mut()[1] else {
            panic!("padding")
        };
        comment.truncate(comment.len() - excess);
        assert_eq!(f.estimated_ir_bytes(), 16 * 1024);
        f.defer_self_preinline(usize::MAX);
        f.name = "edge$generic".into();
        let mut reloc = Relocations::new("m", 0, 0, 0);
        reloc
            .symbols
            .insert("m_.str.1.handle".into(), format!("m_.str.{suffix}.handle"));
        f.relocate_callable(&reloc);
        f.resolve_callable_preinline(|_| None);
        assert_eq!(f.pre_statepoint_inline, admitted);
    }
}

#[test]
fn callable_workers_release_analysis_options_before_emission() {
    let hir = module();
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let layouts = std::sync::Arc::clone(&opts.program_layouts);
    let lowered = with_jobs(4, || super::super::lower_module(&hir, opts, None))
        .unwrap()
        .unwrap();
    assert_eq!(
        std::sync::Arc::strong_count(&layouts),
        1,
        "worker transport must release the lowering-options graph before LLVM emission"
    );
    drop(lowered);
}
