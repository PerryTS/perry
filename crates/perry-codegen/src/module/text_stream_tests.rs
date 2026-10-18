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
