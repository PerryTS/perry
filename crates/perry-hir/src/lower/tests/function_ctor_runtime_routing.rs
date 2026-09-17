//! #10422 / #10424: a `Function` constructor call the constant fold cannot
//! compile reaches the runtime from-strings constructor with its arguments
//! intact. Split from `tests.rs` for the 2000-line cap.

fn lowered_function_debug(source: &str, name: &str) -> String {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let function = hir
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("{name} is lowered"));
    format!("{function:?}")
}

const RUNTIME_CONSTRUCT: &str = r#"New { class_name: "Function""#;
const AOT_STUB: &str = "cannot run in an ahead-of-time compiled binary";

/// The call form used to lower a runtime-built body to a function that always
/// threw, and an unfoldable constant call (an array of parameter names) to a
/// plain call of the global that returned `undefined`. Both construct now,
/// like `new Function(...)`; the `.apply` / `.call` spellings keep the call of
/// the `Function` value, which the runtime routes to the same constructor.
#[test]
fn unfolded_function_calls_construct_at_runtime() {
    let source = r#"
        export function direct(body: string): any { return Function("a", "b", body); }
        export function arrayParams(): any { return Function(["a", "b"] as any, "return a + b"); }
        export function viaApply(body: string): any { return Function.apply(null, ["a", body]); }
        export function viaCall(body: string): any { return Function.call(null, "a", body); }
        export function folded(): any { return Function("a", "return a"); }
    "#;
    for name in ["direct", "arrayParams"] {
        let debug = lowered_function_debug(source, name);
        assert!(
            debug.contains(RUNTIME_CONSTRUCT),
            "{name} must construct at runtime:\n{debug}"
        );
        assert!(
            !debug.contains(AOT_STUB),
            "{name} must not throw the AOT stub:\n{debug}"
        );
    }
    for (name, method) in [("viaApply", "apply"), ("viaCall", "call")] {
        let debug = lowered_function_debug(source, name);
        assert!(
            !debug.contains(AOT_STUB),
            "{name} must not throw the AOT stub:\n{debug}"
        );
        assert!(
            debug.contains(&format!(r#"property: "{method}""#))
                && debug.contains(r#"property: "Function""#),
            "{name} must call `Function.{method}` on the global value:\n{debug}"
        );
    }
    let folded = lowered_function_debug(source, "folded");
    assert!(
        !folded.contains(RUNTIME_CONSTRUCT),
        "an all-constant call still compiles ahead of time:\n{folded}"
    );
}

/// `new Function(...parts)` passed the spread array as ONE argument, which the
/// runtime turned into an empty body. It must keep the spread positions.
#[test]
fn spread_new_function_keeps_its_spread_arguments() {
    let source = r#"
        export function spread(body: string): any { return new Function(...["a", "b"], body); }
    "#;
    let debug = lowered_function_debug(source, "spread");
    assert!(
        debug.contains(
            r#"NewDynamicSpread { callee: PropertyGet { object: GlobalGet(0), property: "Function""#
        ) && debug.contains("Spread("),
        "a spread `new Function` must construct element by element:\n{debug}"
    );
    assert!(!debug.contains(RUNTIME_CONSTRUCT), "{debug}");
}
