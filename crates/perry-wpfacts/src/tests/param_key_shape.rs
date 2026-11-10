//! A computed key that is a parameter of a call-only function is the union
//! of what its calls pass (`gen/param_key.rs`). tsc's checker.ts stores
//! `(type as IterableOrIteratorType)[cacheKey] = cachedTypes` in
//! `setCachedIterationTypes`, whose callers pass string literals; `type`
//! may be the result of an any-key read of a plain object, so it may hold
//! `Object` (Object.prototype.constructor) and Object.prototype's methods.
//! As an any-key store it might name `__proto__` (ECMA-262 B.2.2.1.2): the
//! stored value became the prototype of those builtin objects (E6), and
//! with that value's keys unknown every key of `Object` became unknown
//! (`settle_native_given`). With the arguments' shape the store names only
//! the strings passed.
//!
//! Each fixture stores `cached` (which takes a computed store: its keys are
//! unknown) through such a function, then calls whatever `Object` holds
//! under `someMember` with `obj`: when `Object` may hold anything there, `obj`
//! escapes and `obj.len()` is unproven.

use super::call_shadow::{call_of, proven};
use super::*;

fn program(decls: &str, calls: &str) -> String {
    format!(
        r#"
        class V {{ k = 1; len() {{ return this.k; }} }}
        const obj = new V();
        const plain: any = {{ x: 1 }};
        const got: any = plain[String(Math.random())];
        const cached: any = {{ y: 1 }};
        cached[String(Math.random())] = 2;
        function setCached(type: any, key: string, v: any) {{ type[key] = v; }}
        function viaParam(type: any, key: string, v: any) {{ setCached(type, key, v); }}
        {decls}
        {calls}
        const O: any = Object;
        const f = O.someMember;
        if (typeof f === "function") f(obj);
        console.log(obj.len());
        "#
    )
}

/// Is `obj.len()` proven?
fn len_proven(program: &str) -> bool {
    proven(&call_of(&facts(program), "len").verdict)
}

fn proven_with(decls: &str, calls: &str) -> bool {
    len_proven(&program(decls, calls))
}

#[test]
fn a_parameter_key_of_literal_arguments_gives_no_prototype() {
    assert!(proven_with("", "setCached(got, \"iterB\", cached);"));
}

/// An argument that is a parameter of another call-only function is that
/// parameter's union; a constant binding is its string.
#[test]
fn a_parameter_passed_on_keeps_its_arguments_shape() {
    assert!(proven_with(
        "const KEY = \"iterD\";",
        "setCached(got, \"iterB\", cached); viaParam(got, \"iter\" + \"C\", cached); \
         viaParam(got, KEY, cached);"
    ));
}

/// tsc's shape: the functions are declared inside another function, and
/// one calls itself.
#[test]
fn a_local_function_parameter_key_is_shaped_too() {
    let decls = r#"
        function checker() {
            function setC(type: any, key: string, v: any) { type[key] = v; }
            function via(type: any, key: string, v: any) {
                if (Math.random() > 2) via(type, key, v);
                setC(type, key, v);
            }
            setC(got, "iterB", cached);
            via(got, "iterC", cached);
        }
        checker();
    "#;
    assert!(proven_with(decls, ""));
}

/// A default is what a missing argument stands for.
#[test]
fn a_defaulted_parameter_key_adds_its_default() {
    let decls = "function setD(type: any, v: any, key: string = \"iterD\") { type[key] = v; }";
    assert!(proven_with(
        decls,
        "setD(got, cached); setD(got, cached, \"iterE\");"
    ));
}

/// Control: one call passes `"__proto__"`.
#[test]
fn a_call_passing_proto_gives_the_prototype() {
    assert!(!proven_with(
        "",
        "setCached(got, \"iterB\", cached); setCached(got, \"__proto__\", cached);"
    ));
}

/// Control: one call passes an unknown string.
#[test]
fn a_call_passing_an_unknown_string_gives_the_prototype() {
    assert!(!proven_with(
        "",
        "setCached(got, \"iterB\", cached); viaParam(got, String(Math.random()), cached);"
    ));
}

/// Control: the function escapes; unknown code may call it with any key.
#[test]
fn an_escaped_function_has_no_parameter_shape() {
    let p = program(
        "(globalThis as any).hook = setCached;",
        "setCached(got, \"iterB\", cached);",
    );
    assert!(!len_proven(&unseen(&p)));
}

/// Control: passed as an argument, the function is called by its callee.
#[test]
fn a_function_passed_as_an_argument_has_no_parameter_shape() {
    let decls = "function apply(g: any) { g(got, String(Math.random()), cached); }";
    assert!(!proven_with(
        decls,
        "apply(setCached); setCached(got, \"iterB\", cached);"
    ));
}

/// Control: a value the body assigns to the parameter is one of its keys.
#[test]
fn an_assigned_unknown_string_has_no_shape() {
    let decls = "function setW(type: any, key: string, v: any) { \
                 if (Math.random() > 0.5) key = String(Math.random()); type[key] = v; }";
    assert!(!proven_with(decls, "setW(got, \"iterB\", cached);"));
}

/// Control: a shaped key that admits `__proto__`.
#[test]
fn a_shape_admitting_proto_gives_the_prototype() {
    let decls = "function setP(type: any, x: string, v: any) { type[\"__pro\" + x] = v; }";
    assert!(!proven_with(decls, "setP(got, \"to__\", cached);"));
}
