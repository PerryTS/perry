//! The function layout pass (`crate::function_order`), applied to the final
//! optimized module just before emission so that it sees exactly the
//! functions that become symbols.

use inkwell::attributes::AttributeLoc;
use inkwell::module::{Linkage, Module};
use inkwell::values::{FunctionValue, InstructionOpcode};
use inkwell::AddressSpace;

use crate::function_order::{FunctionLayout, FunctionOrder, RECORD_HOOK};

pub(crate) fn apply(module: &Module<'_>, layout: &FunctionLayout, effective_target: &str) {
    match layout {
        FunctionLayout::Default => {}
        FunctionLayout::Record => record_first_calls(module),
        FunctionLayout::Order(order) => {
            if elf_target(effective_target) {
                place_listed_functions(module, order);
            }
        }
    }
}

/// Mach-O links order through ld64's `-order_file` and need no sections;
/// COFF and wasm keep the default layout.
fn elf_target(triple: &str) -> bool {
    !(triple.contains("apple")
        || triple.contains("darwin")
        || triple.contains("windows")
        || triple.contains("wasm"))
}

fn defined_functions<'ctx>(module: &Module<'ctx>) -> Vec<FunctionValue<'ctx>> {
    module
        .get_functions()
        .filter(|f| f.count_basic_blocks() > 0)
        .collect()
}

/// Give every listed function its rank's section. A function that already
/// names a section keeps it.
fn place_listed_functions(module: &Module<'_>, order: &FunctionOrder) {
    for f in defined_functions(module) {
        let global = f.as_global_value();
        if global.get_section().is_some() {
            continue;
        }
        let name = f.get_name().to_string_lossy();
        if let Some(section) = order.section_for(&name) {
            global.set_section(Some(&section));
        }
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
    fn the_default_layout_changes_nothing() {
        let ctx = Context::create();
        let m = module(&ctx);
        let before = m.print_to_string().to_string();
        apply(&m, &FunctionLayout::Default, "x86_64-unknown-linux-gnu");
        assert_eq!(before, m.print_to_string().to_string());
        assert!(!before.contains(RECORD_HOOK));
    }
}
