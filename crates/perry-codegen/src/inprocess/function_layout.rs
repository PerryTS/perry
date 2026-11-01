//! The function layout pass (`crate::function_order`), applied to the final
//! optimized module just before emission so that it sees exactly the
//! functions that become symbols.

use inkwell::attributes::AttributeLoc;
use inkwell::module::{Linkage, Module};
use inkwell::values::{FunctionValue, InstructionOpcode};
use inkwell::AddressSpace;

use crate::function_order::{FunctionLayout, FunctionOrder, RECORD_HOOK};

pub(crate) fn apply(module: &Module<'_>, layout: &FunctionLayout, effective_target: &str) {
    apply_order(module, layout, effective_target);
    split_js_text(module);
    if crate::codegen::target_os_of(effective_target) == Some("windows") {
        tie_coff_records(module);
    }
}

/// COFF: put each function that has a metadata record (`crate::fn_record`)
/// in a COMDAT it keys, and its record and its info in the same COMDAT, so
/// both are associative sections of the body's: `/OPT:REF` keeps them exactly
/// when it keeps the body (or, for the info, when live code names it; the
/// info names the body, so that keeps the body too). The info matters as much
/// as the record: a private global cannot lead a COFF COMDAT, so without this
/// every info of the unit shares one `.rdata` section, and one live info keeps
/// every body the others name. A body already in a COMDAT keeps that one (the
/// two then live with the COMDAT's leader, as the body does). Done here, on
/// the final module, because a COMDAT names a global the unit defines.
fn tie_coff_records(module: &Module<'_>) {
    use inkwell::comdat::ComdatSelectionKind;
    let mut global = module.get_first_global();
    while let Some(record) = global {
        global = record.get_next_global();
        let name = record.get_name().to_string_lossy().into_owned();
        let Some(body) = crate::fn_record::body_of_record_name(&name) else {
            continue;
        };
        let Some(function) = module.get_function(body) else {
            continue;
        };
        let body_global = function.as_global_value();
        let comdat = body_global.get_comdat().unwrap_or_else(|| {
            let comdat = module.get_or_insert_comdat(body);
            comdat.set_selection_kind(ComdatSelectionKind::NoDuplicates);
            body_global.set_comdat(comdat);
            comdat
        });
        record.set_comdat(comdat);
        if let Some(info) = module.get_global(&crate::fn_info::info_symbol(body)) {
            if info.get_comdat().is_none() && !info.is_declaration() {
                info.set_comdat(comdat);
            }
        }
    }
}

/// Give every JS body still in the shared JS text section its own input
/// section, `perry_js_text.fn.<symbol>`: LLVM keeps one section for an
/// explicit section name even under function sections, and the link drops
/// unreferenced code only a whole input section at a time. The link rule
/// (`JS_TEXT_LINK_RULE`) gathers them back into the `perry_js_text` output
/// section. Bodies the order list placed already have a section of their own.
fn split_js_text(module: &Module<'_>) {
    let js_text = crate::codegen::helpers::JS_TEXT_SECTION_NAME;
    for f in defined_functions(module) {
        let global = f.as_global_value();
        if global.get_section().is_some_and(|s| s.to_bytes() == js_text.as_bytes()) {
            let name = f.get_name().to_string_lossy();
            global.set_section(Some(&format!(
                "{}{name}",
                crate::function_order::JS_FN_SECTION_PREFIX
            )));
        }
    }
}

fn apply_order(module: &Module<'_>, layout: &FunctionLayout, effective_target: &str) {
    match layout {
        FunctionLayout::Default => {}
        FunctionLayout::Record => record_first_calls(module),
        FunctionLayout::Order(order) => {
            if elf_target(effective_target) {
                place_listed_functions(module, order);
                if !order.is_empty()
                    && crate::codegen::target_os_of(effective_target) == Some("linux")
                {
                    back_hot_text(module);
                }
            }
        }
    }
}

/// Mach-O links order through ld64's `-order_file` and need no sections;
/// COFF and wasm keep the default layout.
fn elf_target(triple: &str) -> bool {
    !matches!(
        crate::codegen::target_os_of(triple),
        Some("macos" | "ios" | "tvos" | "watchos" | "visionos" | "windows")
    ) && !triple.contains("wasm")
}

fn defined_functions<'ctx>(module: &Module<'ctx>) -> Vec<FunctionValue<'ctx>> {
    module
        .get_functions()
        .filter(|f| f.count_basic_blocks() > 0)
        .collect()
}

/// Give every listed function its rank's section. A function in the JS text
/// section (x86_64 Linux) takes its rank there, so the JS hot run stays in
/// the range the `Error` capture classifies by (`JsTextRun`); any other
/// function that already names a section keeps it.
fn place_listed_functions(module: &Module<'_>, order: &FunctionOrder) {
    let js_text = crate::codegen::helpers::JS_TEXT_SECTION_NAME;
    for f in defined_functions(module) {
        let global = f.as_global_value();
        let name = f.get_name().to_string_lossy();
        let in_js_text = match global.get_section() {
            None => false,
            Some(s) if s.to_bytes() == js_text.as_bytes() => true,
            Some(_) => continue,
        };
        let section = match in_js_text {
            true => order.js_section_for(&name),
            false => order.section_for(&name),
        };
        if let Some(section) = section {
            global.set_section(Some(&section));
        }
    }
}

/// The linker supplies the exact rounded prefix. This call exists only in an
/// ordered Linux executable; normal IR and non-Linux IR stay unchanged.
fn back_hot_text(module: &Module<'_>) {
    let Some(main) = module.get_function("main") else {
        return;
    };
    let Some(entry) = main.get_first_basic_block() else {
        return;
    };
    let mut at = entry.get_first_instruction();
    while let Some(inst) = at {
        if inst.get_opcode() != InstructionOpcode::Alloca {
            break;
        }
        at = inst.get_next_instruction();
    }
    let Some(at) = at else { return };
    let ctx = module.get_context();
    let ptr = ctx.ptr_type(AddressSpace::default());
    let mut bounds = Vec::new();
    let bound = |name: &str| {
        let global = module
            .get_global(name)
            .unwrap_or_else(|| module.add_global(ctx.i8_type(), None, name));
        global.as_pointer_value().into()
    };
    for name in ["__perry_hot_text_start", "__perry_hot_text_limit"] {
        bounds.push(bound(name));
    }
    // The JS hot run (the ranked functions of the JS text section) is backed
    // the same way; on targets without the section it is empty.
    let js_bounds: Vec<inkwell::values::BasicMetadataValueEnum> =
        ["__perry_hot_js_text_start", "__perry_hot_js_text_limit"]
            .iter()
            .map(|n| bound(n))
            .collect();
    let hook = module.get_function("js_back_hot_text").unwrap_or_else(|| {
        module.add_function(
            "js_back_hot_text",
            ctx.void_type().fn_type(&[ptr.into(), ptr.into()], false),
            Some(Linkage::External),
        )
    });
    let builder = ctx.create_builder();
    builder.position_before(&at);
    for b in [&bounds, &js_bounds] {
        let call = builder.build_call(hook, b, "").expect("hot text call");
        call.add_attribute(
            AttributeLoc::Function,
            ctx.create_string_attribute("gc-leaf-function", ""),
        );
    }
}

/// Call the recording hook at the top of every defined function, after its
/// static allocas, with the function's private `{flag, name}` record. The
/// hook never reaches the collector; the call is marked a GC leaf.
fn record_first_calls(module: &Module<'_>) {
    let ctx = module.get_context();
    let ptr = ctx.ptr_type(AddressSpace::default());
    let functions = defined_functions(module);
    if functions.is_empty() {
        return;
    }
    let hook = module.get_function(RECORD_HOOK).unwrap_or_else(|| {
        module.add_function(
            RECORD_HOOK,
            ctx.void_type().fn_type(&[ptr.into()], false),
            Some(Linkage::External),
        )
    });
    let builder = ctx.create_builder();
    for (index, f) in functions.into_iter().enumerate() {
        let Some(entry) = f.get_first_basic_block() else {
            continue;
        };
        let mut at = entry.get_first_instruction();
        while let Some(inst) = at {
            if inst.get_opcode() != InstructionOpcode::Alloca {
                break;
            }
            at = inst.get_next_instruction();
        }
        let Some(at) = at else {
            continue;
        };
        let mut bytes = vec![0u8];
        bytes.extend_from_slice(f.get_name().to_bytes());
        bytes.push(0);
        let init = ctx.const_string(&bytes, false);
        let record = module.add_global(init.get_type(), None, &format!("__perry_fnorder.{index}"));
        record.set_linkage(Linkage::Private);
        record.set_initializer(&init);
        builder.position_before(&at);
        let call = builder
            .build_call(hook, &[record.as_pointer_value().into()], "")
            .expect("record call builds at a positioned builder");
        call.add_attribute(
            AttributeLoc::Function,
            ctx.create_string_attribute("gc-leaf-function", ""),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;
    use std::sync::Arc;

    const IR: &str = "define void @first() {\nentry:\n  %a = alloca i64\n  store i64 1, ptr %a\n  call void @second()\n  ret void\n}\ndefine void @second() {\nentry:\n  ret void\n}\ndefine void @third() {\nentry:\n  ret void\n}\ndeclare void @external()\n";

    fn module(ctx: &Context) -> Module<'_> {
        crate::inprocess::parse_ir_text(ctx, IR, "layout").expect("parses")
    }

    fn section(m: &Module<'_>, name: &str) -> Option<String> {
        m.get_function(name)?
            .as_global_value()
            .get_section()
            .map(|s| s.to_string_lossy().into_owned())
    }

    #[test]
    fn listed_functions_get_their_rank_and_the_rest_stay_put() {
        let ctx = Context::create();
        let m = module(&ctx);
        let order = Arc::new(FunctionOrder::parse("third\nstale_name\nfirst\nexternal\n"));
        apply(
            &m,
            &FunctionLayout::Order(order),
            "x86_64-unknown-linux-gnu",
        );
        m.verify().expect("verifies");
        assert_eq!(
            section(&m, "third").as_deref(),
            Some(".text.sorted.00000000")
        );
        assert_eq!(
            section(&m, "first").as_deref(),
            Some(".text.sorted.00000002")
        );
        assert_eq!(
            section(&m, "second"),
            None,
            "unlisted functions keep the default text"
        );
        assert_eq!(
            section(&m, "external"),
            None,
            "declarations are never placed"
        );
    }

    #[test]
    fn mach_o_and_coff_keep_the_default_sections() {
        for triple in ["arm64-apple-macosx15.0.0", "x86_64-pc-windows-msvc"] {
            let ctx = Context::create();
            let m = module(&ctx);
            let order = Arc::new(FunctionOrder::parse("first\nsecond\nthird\n"));
            apply(&m, &FunctionLayout::Order(order), triple);
            for name in ["first", "second", "third"] {
                assert_eq!(section(&m, name), None, "{triple} {name}");
            }
        }
    }

    #[test]
    fn recording_calls_the_hook_once_at_every_defined_entry() {
        let ctx = Context::create();
        let m = module(&ctx);
        apply(&m, &FunctionLayout::Record, "x86_64-unknown-linux-gnu");
        m.verify().expect("verifies");
        let ir = m.print_to_string().to_string();
        assert_eq!(
            ir.matches(&format!("call void @{RECORD_HOOK}(")).count(),
            3,
            "one hook call per defined function:\n{ir}"
        );
        // The call follows the allocas, so they stay static.
        let first = ir.split("define void @first()").nth(1).unwrap();
        let alloca = first.find("alloca").unwrap();
        let hook = first.find(RECORD_HOOK).unwrap();
        assert!(alloca < hook, "{first}");
        assert!(
            ir.contains("c\"\\00first\\00\""),
            "the record carries flag + name:\n{ir}"
        );
        assert!(ir.contains("\"gc-leaf-function\""), "{ir}");
    }

    #[test]
    fn huge_text_startup_is_only_in_nonempty_linux_ordered_main() {
        let ctx = Context::create();
        for (triple, names, want) in [
            ("x86_64-unknown-linux-gnu", "main", true),
            ("x86_64-unknown-linux-gnu", "", false),
            ("arm64-apple-macosx15.0.0", "main", false),
            ("x86_64-pc-windows-msvc", "main", false),
            ("aarch64-linux-android", "main", false),
        ] {
            let m = crate::inprocess::parse_ir_text(
                &ctx,
                "define i32 @main() {\nentry:\n %x = alloca i64\n ret i32 0\n}\n",
                "main",
            )
            .unwrap();
            apply(
                &m,
                &FunctionLayout::Order(Arc::new(FunctionOrder::parse(names))),
                triple,
            );
            m.verify().unwrap();
            let ir = m.print_to_string().to_string();
            assert_eq!(
                ir.contains("call void @js_back_hot_text("),
                want,
                "{triple}"
            );
            if want {
                assert!(ir.find("alloca").unwrap() < ir.find("call void").unwrap());
                assert!(ir.contains("@__perry_hot_text_start"));
                assert!(ir.contains("@__perry_hot_text_limit"));
            }
        }
    }

    #[test]
    fn the_default_layout_changes_nothing() {
        let ctx = Context::create();
        let m = module(&ctx);
        let before = m.print_to_string().to_string();
        apply(&m, &FunctionLayout::Default, "x86_64-unknown-linux-gnu");
        assert_eq!(before, m.print_to_string().to_string());
        assert!(!before.contains(RECORD_HOOK));
    }
}
