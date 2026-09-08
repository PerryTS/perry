use perry_codegen::module::LlModule;
use perry_codegen::types::DOUBLE;

// Run in separate processes for default, O0/O2/O3, Os and Oz. This avoids
// changing process-global compiler settings while another test is lowering.
#[test]
fn size_attributes_survive_text_and_native_construction() {
    let expected = std::env::var("PERRY_TEST_SIZE_ATTRIBUTES").unwrap_or_else(|_| "optsize".into());
    let mut module = LlModule::new("arm64-apple-macosx15.0.0");
    for (name, forced, hinted, noinline) in [
        ("plain", false, false, false),
        ("forced", true, false, false),
        ("hinted", false, true, false),
        ("boundary", false, false, true),
    ] {
        let function = module.define_function(name, DOUBLE, vec![(DOUBLE, "%value".to_string())]);
        function.force_inline = forced;
        function.inline_hint = hinted;
        function.no_inline = noinline;
        function.create_block("entry").ret(DOUBLE, "%value");
        for external in [false, true] {
            let header = function.define_header(external);
            assert_eq!(
                header.contains(" optsize"),
                expected.contains("optsize"),
                "{header}"
            );
            assert_eq!(
                header.contains(" minsize"),
                expected.contains("minsize"),
                "{header}"
            );
        }
    }
    let text = perry_codegen::linker::compile_ll_to_object(
        &module.to_ir(),
        Some("arm64-apple-macosx15.0.0"),
    )
    .unwrap();
    let native = perry_codegen::native_emit::compile_module_native(
        &mut module,
        Some("arm64-apple-macosx15.0.0"),
        "size_attribute_fixture",
    )
    .unwrap();
    assert!(!native.is_empty());
    assert_eq!(
        native, text,
        "native/text size policy must produce the same object"
    );
}
