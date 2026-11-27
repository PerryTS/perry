//! Rule NOT-ERROR: a failed `x instanceof Error` (the bare global name, the
//! intrinsic constructor) drops from `x` every value whose prototype chain
//! reaches `%Error.prototype%`: an instance of a program class over
//! `extends Error` never reaches the false side (`Cons::NotError`). A
//! `Symbol.hasInstance` installed on `Error`, or a [[Prototype]] write on
//! the class's prototype, keeps it.
use super::*;

/// The receivers of `g`'s `v.k` (`:E`, `:P`) and its verdict.
fn read(prelude: &str, test: &str) -> (Vec<String>, Verdict) {
    let src = format!(
        r#"
        class E extends Error {{ k = 1; }}
        class P {{ k = 2; }}
        {prelude}
        function g(v: any) {{ return v.k; }}
        function main() {{
        const v: any = process.argv.length > 9 ? new E("x") : new P();
        {test}
        }}
        main();
        "#
    );
    let pf = facts(&src);
    let s = site(&pf, "k");
    (s.receivers.clone(), s.verdict.clone())
}

/// Only `P` reaches `g`: the `E` instance is dropped (and the read, `P`'s
/// definite birth field, is monomorphic; `E`'s `k` is not definite).
fn assert_dropped((recv, v): (Vec<String>, Verdict)) {
    assert!(!recv.iter().any(|r| r.ends_with(":E")), "E kept: {recv:?}");
    assert!(recv.iter().any(|r| r.ends_with(":P")), "{recv:?}");
    assert_eq!(v, Verdict::T0Mono, "{recv:?} {v:?}");
}

/// Both instances reach `g`.
fn assert_kept((recv, v): (Vec<String>, Verdict)) {
    assert!(
        recv.iter().any(|r| r.ends_with(":E")),
        "E dropped: {recv:?}"
    );
    assert!(recv.iter().any(|r| r.ends_with(":P")), "{recv:?}");
    assert_ne!(v, Verdict::T0Mono, "{recv:?} {v:?}");
}

/// `if (!(v instanceof Error))`: `g` sees `P` only.
#[test]
fn not_error_if_drops_the_error_subclass() {
    assert_dropped(read("", "if (!(v instanceof Error)) console.log(g(v));"));
}

/// The else arm of `?:` (the INSPECT stop's shape).
#[test]
fn not_error_conditional_else_arm() {
    assert_dropped(read("", "console.log(v instanceof Error ? 0 : g(v));"));
}

/// Control: the true side keeps both (no narrowing there).
#[test]
fn not_error_true_side_keeps_both() {
    assert_kept(read(
        "",
        "if (v instanceof Error) console.log(g(v)); console.log(g(v));",
    ));
}

/// KEEP: a program `@@hasInstance` on `Error` decides instead.
#[test]
fn not_error_has_instance_on_error_keeps() {
    assert_kept(read(
        "Object.defineProperty(Error, Symbol.hasInstance, { value: (x: any) => false });",
        "if (!(v instanceof Error)) console.log(g(v));",
    ));
}

/// KEEP: the class prototype's chain is changed.
#[test]
fn not_error_set_prototype_of_keeps() {
    assert_kept(read(
        "Object.setPrototypeOf(E.prototype, Object.prototype);",
        "if (!(v instanceof Error)) console.log(g(v));",
    ));
}

/// KEEP: the global `Error` replaced: the guard lets every value through.
#[test]
fn not_error_replaced_global_keeps() {
    assert_kept(read(
        "(globalThis as any).Error = P;",
        "if (!(v instanceof Error)) console.log(g(v));",
    ));
}
