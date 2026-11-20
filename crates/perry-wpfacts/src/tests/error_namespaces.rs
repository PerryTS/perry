//! `Error`, the six NativeError constructors and `AggregateError` are
//! distinct constructors with distinct prototypes (ECMA-262 20.5): each
//! NativeError's and `AggregateError`'s [[Prototype]] is `Error`, and
//! `X.prototype.constructor` is `X`.
use super::*;

/// The receivers of the `size` read on `x`, a `Svc` or the value `v`.
fn receivers_of(prelude: &str, v: &str) -> Vec<String> {
    let src = format!(
        "class Svc {{ size = 3; }}\n\
         {prelude}\n\
         const v: any = {v};\n\
         const x: any = process.argv.length > 9 ? new Svc() : v;\n\
         console.log(x.size);\n"
    );
    site(&facts(&src), "size").receivers.clone()
}

/// Red on main: one namespace stood for `Error` and every NativeError, whose
/// prototype was the collapsed `Object.prototype` (its `constructor` only
/// `Object` or `Function`), and no builtin namespace's chain reached
/// another, so `Object.getPrototypeOf(TypeError)` missed `Error` and a
/// static the program stored on `Error` was not read through `TypeError`.
#[test]
fn each_error_constructor_is_its_own_with_error_as_its_prototype() {
    let te = "builtin:NativeError(\"TypeError\")";
    let cases = [
        ("", "TypeError", te),
        ("", "(TypeError as any).prototype.constructor", te),
        (
            "",
            "(AggregateError as any).prototype.constructor",
            "builtin:AggregateError",
        ),
        ("", "Error.prototype.constructor", "builtin:Error"),
        ("", "Object.getPrototypeOf(TypeError)", "builtin:Error"),
        ("", "Object.getPrototypeOf(AggregateError)", "builtin:Error"),
        (
            "",
            "(new RangeError(\"r\")).constructor",
            "builtin:NativeError(\"RangeError\")",
        ),
        (
            "class Made { size = 1; } (Error as any).make = () => new Made();",
            "(TypeError as any).make()",
            "t.ts:Made",
        ),
        (
            "class Made { size = 1; } (Error as any).make = () => new Made();",
            "(AggregateError as any).make()",
            "t.ts:Made",
        ),
    ];
    let mut wrong = Vec::new();
    for (prelude, v, want) in cases {
        let r = receivers_of(prelude, v);
        if !r.iter().any(|t| t == want) {
            wrong.push(format!("{v}: want {want}, got {r:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    // `TypeError` is not `Error`.
    assert!(!receivers_of("", "TypeError")
        .iter()
        .any(|t| t == "builtin:Error"));
}
