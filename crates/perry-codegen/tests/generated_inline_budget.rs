use perry_codegen::function::LlFunction;
use perry_codegen::types::DOUBLE;

#[test]
fn large_generated_body_does_not_inherit_unconditional_or_raised_inline_budget() {
    let mut function = LlFunction::new("large", DOUBLE, vec![(DOUBLE, "%a".into())]);
    function.force_inline = true;
    let entry = function.create_block("entry");
    for index in 0..512 {
        entry.emit_raw(format!("%r{index} = call double @expensive(double %a)"));
    }
    entry.ret(DOUBLE, "%a");
    assert!(function.estimated_ir_bytes() > 8 * 1024);
    let header = function.define_header(false);
    assert!(!header.contains("alwaysinline"), "{header}");
    assert!(!header.contains("inlinehint"), "{header}");
}

#[test]
fn tiny_helpers_keep_the_existing_inline_admission() {
    let mut function = LlFunction::new("tiny", DOUBLE, vec![(DOUBLE, "%a".into())]);
    function.force_inline = true;
    function.create_block("entry").ret(DOUBLE, "%a");
    let header = function.define_header(false);
    assert!(
        header.contains("alwaysinline") || header.contains("inlinehint"),
        "{header}"
    );
}
