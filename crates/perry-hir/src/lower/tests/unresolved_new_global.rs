//! #8882/#8730: an unresolved `new` names the identifier and defers to a
//! runtime global lookup. Split from `tests.rs` for the 2000-line cap.

use super::*;

/// #8882 / #8730: a constructor name that resolves to nothing in the module
/// is read off `globalThis` when the `new` executes — exactly like a bare
/// identifier read — so a runtime-created global constructs and a true miss
/// throws `ReferenceError: <name> is not defined` WITH the identifier. The
/// `typeof`-guarded browser-API shape is the one Next's `app-page` runtime
/// carries; it previously lowered to the nameless throw even though the guard
/// makes the branch dead on a server.
#[test]
fn unresolved_new_names_the_identifier_and_defers_to_a_runtime_global_lookup() {
    let source = r#"
        function observe(cb: any): any {
            return typeof IntersectionObserver === "function"
                ? new IntersectionObserver(cb)
                : null;
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let observe = hir
        .functions
        .iter()
        .find(|function| function.name == "observe")
        .expect("observe is lowered");
    let debug = format!("{observe:?}");

    assert!(
        !debug.contains("js_throw_reference_error_unresolved_get"),
        "the nameless ReferenceError helper must not be emitted for `new <unknown>()`:\n{debug}"
    );
    assert!(
        debug.contains(
            r#"NewDynamic { callee: Call { callee: ExternFuncRef { name: "js_global_get_or_throw_unresolved", param_types: [Any], return_type: Any }, args: [String("IntersectionObserver")]"#
        ),
        "an unresolved constructor must be a runtime globalThis lookup carrying its name:\n{debug}"
    );
}
