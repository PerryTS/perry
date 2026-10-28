use perry_hir::{types::Type, Expr, Function, Module, Stmt};

fn literal(captures: u32) -> Expr {
    Expr::Closure {
        func_id: 100,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Array(
            (0..captures).map(Expr::LocalGet).collect(),
            None,
        )))],
        captures: (0..captures).collect(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

fn compile(captures: u32, target: &str) -> String {
    let mut module = Module::new("fixed_birth_contract");
    let mut body: Vec<_> = (0..captures)
        .map(|id| Stmt::Let {
            id,
            name: format!("cap{id}"),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Object(Vec::new())),
        })
        .collect();
    body.push(Stmt::Return(Some(literal(captures))));
    module.functions.push(Function {
        id: 1,
        name: "make".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let options = crate::CompileOptions {
        emit_ir_only: true,
        target: Some(target.into()),
        ..Default::default()
    };
    String::from_utf8(crate::compile_module(&module, options).unwrap()).unwrap()
}

#[test]
fn closure_fast_birth_is_leaf_and_retains_collecting_runtime_fallbacks() {
    for (count, helper) in [
        (0, "js_closure_alloc_empty"),
        (1, "js_closure_alloc_small"),
        (5, "js_closure_alloc_fixed"),
    ] {
        let ir = compile(count, "x86_64-unknown-linux-gnu");
        let fast = ir.find("fixed_birth.fast.").expect("inline birth");
        let fast = ir[fast..]
            .find("\nfixed_birth.fast.")
            .map(|i| fast + i + 1)
            .unwrap();
        let end = ir[fast..].find("\nfixed_birth.runtime.").unwrap() + fast;
        let fast_body = &ir[fast..end];
        assert!(
            !fast_body
                .lines()
                .any(|line| line.contains("call ") && !line.contains(" asm ")),
            "{fast_body}"
        );
        let size = 32 + u64::from(count) * 8;
        let reserved = if count == 0 { 0x4000 } else { 0 };
        let header = 4u64 | (2 << 8) | (reserved << 16) | (size << 32);
        assert!(
            fast_body.contains(&format!("store i64 {header},")),
            "{fast_body}"
        );
        assert!(ir.contains(&format!("call i64 @{helper}")), "{helper}");
        assert!(
            ir.contains("icmp ule i64"),
            "must consume the live permission bound"
        );
        assert!(
            !fast_body.contains("load volatile i8"),
            "idle birth needs only its bound proof"
        );
    }
}

#[test]
fn fixed_births_decline_ilp32_and_large_cells() {
    use super::fixed_alloc::eligible;
    assert!(!eligible("armv7-unknown-linux-gnueabihf", 40));
    assert!(!eligible("wasm32-wasi", 40));
    assert!(!eligible("aarch64_be-unknown-linux-gnu", 40));
    assert!(!eligible("powerpc64-unknown-linux-gnu", 40));
    assert!(!eligible("x86_64-unknown-linux-gnux32", 40));
    assert!(eligible("x86_64-unknown-linux-gnu", 16 * 1024));
    assert!(!eligible("x86_64-unknown-linux-gnu", 16 * 1024 + 8));
}

#[test]
fn layout_birth_initializes_exact_capacity_and_checks_registration() {
    for slots in [2u32, 12] {
        let mut module = Module::new("fixed_layout_contract");
        let mut props = vec![("method".into(), literal(0))];
        props.extend((1..slots).map(|i| (format!("key{i}"), Expr::Number(i as f64))));
        module.init.push(Stmt::Expr(Expr::Object(props)));
        let mut options = crate::CompileOptions {
            emit_ir_only: true,
            output_type: "executable".into(),
            disable_constfn_shapes: false,
            ..Default::default()
        };
        let births = crate::module_birth_shapes(&module, options.clone()).unwrap();
        assert!(
            !births.is_empty(),
            "the layout fixture must have a carried descriptor"
        );
        options.static_shape_ids = crate::codegen::static_shape_ids::assign_static_shape_ids(
            births.iter().map(|b| &b.shape),
        )
        .into_iter()
        .collect();
        let ir = String::from_utf8(crate::compile_module(&module, options).unwrap()).unwrap();
        assert!(ir.contains("call ptr @perry_alloc_layout"));
        let header = 2u64 | (2 << 8) | (0x200 << 16) | ((24 + u64::from(slots) * 8) << 32);
        assert!(ir.contains(&format!("store i64 {header},")), "{ir}");
        assert!(ir.contains("icmp ne i32") && ir.contains(", -1"));
        assert_eq!(ir.contains("fixed_birth.holes"), slots > 8);
    }
}

#[test]
fn windows_lp64_birth_keeps_the_same_header_and_slow_abi() {
    let ir = compile(1, "x86_64-pc-windows-msvc");
    assert!(ir.contains("fixed_birth.fast"));
    let header = 4u64 | (2 << 8) | (40 << 32);
    assert!(ir.contains(&format!("store i64 {header},")));
    assert!(ir.contains("call i64 @js_closure_alloc_small"));
}
