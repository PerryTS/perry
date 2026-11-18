use super::LlModule;
use crate::types::{I32, VOID};

fn fixture() -> LlModule {
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    module.declare_function("consume", VOID, &[I32]);
    for i in 0..8 {
        let function = module.define_function(format!("unit_fn_{i}"), I32, vec![]);
        let block = function.create_block("entry");
        block.call_void("consume", &[(I32, "42")]);
        block.ret(I32, "42");
    }
    module
}

#[test]
fn streamed_unit_text_and_objects_match_eager_emission() {
    for units in [1, 2, 4] {
        let expected = fixture().render_codegen_units(units);
        let actual: Vec<_> = fixture().into_codegen_unit_texts(units).collect();
        assert_eq!(actual, expected, "streaming preserves unit layout and IR");
        #[cfg(all(
            feature = "llvm-inprocess",
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        {
            let expected = crate::linker::compile_units_to_object(&expected, None).unwrap();
            let actual = crate::linker::compile_streamed_units_to_object(
                fixture().into_codegen_unit_texts(units),
                None,
            )
            .unwrap();
            assert_eq!(actual, expected, "streaming preserves every object byte");
        }
    }
}

#[test]
fn named_module_proofs_and_their_operands_survive_unit_splitting() {
    for units in [1, 2, 4] {
        let mut module = fixture();
        module.add_metadata_line("!17 = !{i32 1}".to_string());
        module.add_metadata_line("!perry.no_workers = !{!0}".to_string());
        module.add_metadata_line("!perry.test_proof = !{!17}".to_string());
        for text in module.render_codegen_units(units) {
            assert!(text.contains("!perry.no_workers = !{!0}"), "{text}");
            assert!(text.contains("!perry.test_proof = !{!17}"), "{text}");
            assert!(text.contains("!17 = !{i32 1}"), "{text}");
        }
    }
}

/// A direct call into another unit is satisfied by an `available_externally`
/// copy of the callee, and the layout then omits the callee's `declare`. The
/// streamed renderer (the transport of `--debug-symbols` and
/// `PERRY_LLVM_INPROCESS=0`) once dropped the copies, so the caller's unit
/// named a symbol it neither defined nor declared: clang rejected it with
/// "use of undefined value '@perry_checked_number_remainder'".
#[test]
fn streamed_units_carry_cross_unit_inline_copies() {
    use crate::types::DOUBLE;
    fn module() -> LlModule {
        let mut m = LlModule::new("x86_64-unknown-linux-gnu");
        let f = m.define_function("perry_fn_m__caller", DOUBLE, vec![]);
        let e = f.create_block("entry");
        let r = e.call(DOUBLE, "perry_fn_m__callee", &[]);
        e.ret(DOUBLE, &r);
        let f = m.define_function("perry_fn_m__callee", DOUBLE, vec![]);
        f.create_block("entry").ret(DOUBLE, "1.0");
        m
    }
    let expected = module().render_codegen_units(2);
    let streamed: Vec<_> = module().into_codegen_unit_texts(2).collect();
    assert_eq!(
        streamed, expected,
        "both text transports render the same units"
    );
    let caller = streamed
        .iter()
        .find(|u| u.contains("define double @perry_fn_m__caller("))
        .expect("caller unit");
    assert!(
        !caller.contains("define double @perry_fn_m__callee("),
        "the fixture needs the callee in another unit:\n{caller}"
    );
    assert!(
        caller.contains("define available_externally double @perry_fn_m__callee(")
            || caller.contains("declare double @perry_fn_m__callee("),
        "the caller's unit defines or declares every callee:\n{caller}"
    );
}
