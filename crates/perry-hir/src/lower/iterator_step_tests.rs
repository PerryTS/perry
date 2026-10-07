use super::*;

#[test]
fn synchronous_consumers_capture_next_and_write_native_step_values() {
    for source in [
        "function f(it:any) { for (const x of it) console.log(x); }",
        "function* g() { yield 1; } for (const x of g()) console.log(x);",
        "function f(it:any) { const [,x,...rest] = it; console.log(x,rest); }",
        "function f(it:any) { let x,rest; [,x,...rest] = it; console.log(x,rest); }",
    ] {
        let parsed = perry_parser::parse_typescript(source, "native_step.ts").unwrap();
        let hir = crate::lower_module(&parsed, "native_step", "native_step.ts").unwrap();
        let text = format!("{hir:#?}");
        assert!(
            text.contains("iteratorNextMethod"),
            "capture once: {source}"
        );
        assert!(
            text.contains("method: \"iteratorStep\""),
            "native consumer: {source}"
        );
        assert!(
            text.contains("iteratorStepOutput"),
            "unknown output write: {source}"
        );
        assert!(
            !text.contains("iteratorNextResult"),
            "no protocol-result consumer: {source}"
        );
        assert!(
            !text.contains("property: \"value\""),
            "no result.value read: {source}"
        );
    }
}
