//! `m.call(thisArg, …)` on a local bound to a built-in prototype method
//! folds to `thisArg.m(…)`, except for `Function.prototype`'s `bind`,
//! `call` and `apply`, which run as values through their own thunks.

fn lowered_body(source: &str, function: &str) -> String {
    let module = perry_parser::parse_typescript(source, "alias-call.ts").expect("source parses");
    let hir =
        crate::lower::lower_module(&module, "alias-call", "alias-call.ts").expect("source lowers");
    let body = &hir
        .functions
        .iter()
        .find(|f| f.name == function)
        .unwrap_or_else(|| panic!("`{function}` must be lowered: {hir:?}"))
        .body;
    format!("{body:?}")
}

/// A wrapper installed over `Function.prototype.bind` that calls the saved
/// original through `.call` must call that value: if the call were folded to
/// `thisArg.bind(…)`, it would read the wrapper again and recurse. The Array
/// fold is unaffected.
#[test]
fn a_function_prototype_alias_call_is_not_folded_to_a_member_call() {
    let source = r#"
        const origBind = Function.prototype.bind;
        const origPush = Array.prototype.push;
        export function viaBind(f: any, t: any) { return origBind.call(f, t); }
        export function viaPush(a: any) { return origPush.call(a, 1); }
    "#;
    let via_bind = lowered_body(source, "viaBind");
    let via_push = lowered_body(source, "viaPush");
    assert_eq!(
        (
            via_bind.contains("property: \"call\""),
            via_bind.contains("property: \"bind\""),
            via_push.contains("property: \"call\""),
        ),
        (true, false, false),
        "viaBind: {via_bind}\nviaPush: {via_push}"
    );
}
