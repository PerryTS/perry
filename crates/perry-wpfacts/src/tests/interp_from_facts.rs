//! DECISIONS 235: "interpreted code may run" (E14) comes from the solved
//! facts. The link decision (the dyn-eval interpreter is kept because some
//! member is named `Function`, as in `SymbolFlags.Function`) runs no code by
//! itself. Interpreted code runs from a real eval-like source site, or from a
//! solved call or construct whose callee may be the Function constructor: the
//! constructor itself, reached through `.constructor` of a function value,
//! passed to a function that calls it, read under a computed key, or given
//! to unknown code (then a ⊤ callee may be it).
//! Once it may run, it catches what compiled code throws: a thrown `Thr`
//! escapes, and `t.code` is unproven.

use super::*;

/// The interpreter is linked; `{body}` runs after `thr` may throw.
fn verdict(body: &str, sources: bool) -> Verdict {
    let src = format!(
        r#"
        const SymbolFlags = {{ Function: 16, Class: 32 }};
        class Thr {{ code = 4; }}
        function thr(n: number) {{ if (n > 1) throw new Thr(); return n; }}
        try {{ thr(SymbolFlags.Function); }} catch {{}}
        {body}
        const t = new Thr();
        console.log(SymbolFlags.Function, t.code);
        "#
    );
    let cfg = FactsConfig {
        dynamic_code: Some("E14 test: a member named Function".into()),
        interpreted_sources: sources,
        ..Default::default()
    };
    site(&facts_with(&src, cfg), "code").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: prints `16 4`; nothing builds a function from source.
#[test]
fn the_link_decision_alone_runs_no_interpreted_code() {
    let v = verdict("", false);
    assert!(proven(&v), "{v:?}");
}

/// node: `(function () {}).constructor` is `Function`; prints `16 4`.
#[test]
fn the_constructor_of_a_function_value_runs_interpreted_code() {
    let v = verdict(r#"(function () {}).constructor("return 1")();"#, false);
    assert!(!proven(&v), "{v:?}");
}

/// node: `mk(Function)` builds the function; prints `16 4`.
#[test]
fn the_constructor_called_through_a_parameter_runs_interpreted_code() {
    let v = verdict(
        r#"function mk(c: any) { return c("return 1"); } mk(Function)();"#,
        false,
    );
    assert!(!proven(&v), "{v:?}");
}

/// A real source site (the driver found one) runs it from the start.
#[test]
fn a_real_source_site_runs_interpreted_code() {
    let v = verdict("", true);
    assert!(!proven(&v), "{v:?}");
}

/// A value from unmodeled code that no one gave the Function constructor is
/// not it (closed world): `hostFn` is never assigned, so in node the call
/// throws a TypeError before any source is built.
#[test]
fn a_top_callee_without_the_constructor_runs_no_interpreted_code() {
    let v = verdict(
        r#"const u: any = (globalThis as any).hostFn; if (u) u("return 1")();"#,
        false,
    );
    assert!(proven(&v), "{v:?}");
}

/// node: `F` is `Function` for `k = "Function"`; prints `16 4`.
#[test]
fn a_computed_global_read_may_be_the_constructor() {
    let v = verdict(
        r#"const k = String(SymbolFlags.Class).length > 1 ? "Function" : "Object"; const F: any = (globalThis as any)[k]; F("return 1")();"#,
        false,
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: `f[k]` is `Function` for `k = "constructor"`; prints `16 4`.
#[test]
fn a_computed_constructor_key_on_a_function_may_be_the_constructor() {
    let v = verdict(
        r#"function f() {} const k = String(SymbolFlags.Class).length > 1 ? "constructor" : "name"; (f as any)[k]("return 1")();"#,
        false,
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: unknown code given `Function` may call it; prints `16 4`.
#[test]
fn the_constructor_given_to_unknown_code_runs_interpreted_code() {
    let v = verdict(
        r#"const sink: any = (globalThis as any).hostSink; if (sink) sink(Function);"#,
        false,
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: `F` is `Function`; prints `16 4`.
#[test]
fn a_direct_call_of_the_constructor_runs_interpreted_code() {
    let v = verdict(r#"const F = Function; F("return 1")();"#, false);
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `16 4`.
#[test]
fn a_direct_construct_of_the_constructor_runs_interpreted_code() {
    let v = verdict(r#"const F: any = Function; new F("return 1")();"#, false);
    assert!(!proven(&v), "{v:?}");
}

/// node: the global holds `Function` once stored; prints `16 4`.
#[test]
fn the_constructor_stored_into_a_global_runs_interpreted_code() {
    let v = verdict(
        r#"(globalThis as any).hostFn = Function; const u: any = (globalThis as any).hostFn; u("return 1")();"#,
        false,
    );
    assert!(!proven(&v), "{v:?}");
}

/// Iterating a value from unmodeled code yields its elements, never a
/// function's `constructor`: calling one runs no interpreted code (node:
/// `hostList` is undefined; prints `16 4`).
#[test]
fn an_element_of_a_top_value_is_not_the_constructor() {
    let v = verdict(
        r#"const xs: any = (globalThis as any).hostList; if (xs) for (const f of xs) f("return 1");"#,
        false,
    );
    assert!(proven(&v), "{v:?}");
}
