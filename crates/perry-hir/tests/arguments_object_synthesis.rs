//! Every non-arrow function that references `arguments` must get the synthetic
//! arguments binding (a `Param` carrying `arguments_object: Some(..)`);
//! otherwise the reference lowers to an unresolved global read and throws
//! `ReferenceError: arguments is not defined` at run time.
//!
//! Shapes found in bundled third-party libraries:
//! pako's `utils.assign` reads `arguments` only in a `for` init clause (jszip
//! throws at module evaluation), object-literal accessors, and parameter
//! defaults of function expressions / nested function declarations.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn synthesized_bindings(src: &str) -> usize {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "arguments_object_synthesis.js", &mut cache)
        .expect("fixture parses");
    let module = lower_module(&parsed.module, "test", "arguments_object_synthesis.js")
        .expect("fixture lowers");
    format!("{module:#?}")
        .matches("arguments_object: Some(")
        .count()
}

fn assert_binds(label: &str, src: &str) {
    assert!(
        synthesized_bindings(src) >= 1,
        "{label}: the function reading `arguments` must get its own arguments object"
    );
}

#[test]
fn for_init_clause_var() {
    assert_binds(
        "for (var ...; ...) init",
        "var r = {}; r.assign = function (e) { for (var t = Array.prototype.slice.call(arguments, 1); t.length; ) { var s = t.shift(); for (var n in s) e[n] = s[n]; } return e; }; r.assign({}, {a: 1});",
    );
}

#[test]
fn for_init_clause_let_and_expr() {
    assert_binds(
        "for (let ...) init",
        "function f() { for (let t = arguments.length; t > 0; ) return t; return 0; } f(1);",
    );
    assert_binds(
        "for (expr; ...) init",
        "function f() { var t; for (t = arguments[0]; t; ) return t; return 0; } f(1);",
    );
}

#[test]
fn object_literal_getter() {
    assert_binds(
        "object getter",
        "var o = { get g() { return arguments.length; } }; o.g;",
    );
}

#[test]
fn object_literal_setter() {
    assert_binds(
        "object setter",
        "var o = { set s(v) { this.v = arguments[0]; } }; o.s = 9;",
    );
}

#[test]
fn function_expression_parameter_default() {
    assert_binds(
        "function expression default",
        "var f = function (a, b = arguments.length) { return b; }; f(1);",
    );
}

#[test]
fn nested_function_declaration_parameter_default() {
    assert_binds(
        "nested function default",
        "function outer() { function f(a, b = arguments.length) { return b; } return f(1); } outer();",
    );
}

#[test]
fn functions_without_arguments_get_no_binding() {
    assert_eq!(
        synthesized_bindings("var o = { get g() { return 1; }, set s(v) { this.v = v; } }; function f(a, b = 2) { for (var i = 0; i < a; i++) {} return b; } var e = function (x = 1) { return x; };"),
        0,
        "no synthetic arguments object for functions that never read it"
    );
}
