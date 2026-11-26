//! `Object.assign(t, s)` (ECMA-262 20.1.2.1) with a source `s` that is a
//! builtin function value or a primitive. Step 3.a: `null` / `undefined`
//! are skipped; any other source is ToObject(s) (7.1.18) and its own
//! enumerable keys, String and Symbol, are copied by Get + Set. A String
//! wrapper's own enumerable keys are its indices, each a one-character
//! String (10.4.3.5 StringGetOwnProperty; `length` is non-enumerable); a
//! Number / Boolean / Symbol / BigInt wrapper has none. A built-in function
//! the realm declares owns `length` and `name`, both non-enumerable
//! (10.2.9 SetFunctionName, 10.2.10 SetFunctionLength; 19.2.5
//! `parseFloat`), so the copy writes only what the program stored on it.
//! No user code runs and no unknown value is made.
use super::*;

/// Solving `src` makes no ⊤ (the STOP_ON_TOP switch, captured).
fn kept(src: &str, what: &str) {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v = crate::topstop::take_captured();
    assert!(v.is_empty(), "{what}: ⊤ made:\n{}", v.join("\n"));
}

// The c09 shape (perry-web dom/elements.ts `set type`): a realm global
// function as the source.
#[test]
fn assign_from_a_realm_global_function_keeps_the_target() {
    for s in [
        "parseFloat",
        "parseInt",
        "isNaN",
        "encodeURIComponent",
        "Number.parseFloat",
        "Object.keys",
    ] {
        let src = format!(
            r#"
class T {{ pa = 1; }}
const t = new T();
Object.assign(t, {s});
console.log(t.pa + 1);
"#
        );
        kept(&src, s);
    }
}

// Primitive sources: a String copies its characters, the others nothing,
// `null` / `undefined` are skipped.
#[test]
fn assign_from_a_primitive_source_keeps_the_target() {
    for s in ["\"ab\"", "5", "true", "null", "undefined", "Symbol()", "10n"] {
        let src = format!(
            r#"
class T {{ pa = 1; }}
const t = new T();
Object.assign(t, {s});
console.log(t.pa + 1);
"#
        );
        kept(&src, s);
    }
}

// A String target (`Object.assign("type", s)`: ToObject makes the target a
// String wrapper) with a builtin function source.
#[test]
fn assign_into_a_string_from_a_realm_function_keeps_the_argument() {
    kept(
        r#"
class A { pb = 2; }
const a = new A();
const w = Object.assign("type", parseFloat);
console.log(w.length, a.pb + 1);
"#,
        "string target",
    );
}

#[test]
fn the_realm_says_which_functions_own_enumerable_keys() {
    // Declared with `length` and `name` only.
    assert!(crate::builtins::realm_function_own_props_unenumerable("parseFloat"));
    assert!(crate::builtins::realm_function_own_props_unenumerable("setTimeout"));
    // Declared with enumerable statics (`Buffer.from`, `Event.AT_TARGET`).
    assert!(!crate::builtins::realm_function_own_props_unenumerable("Buffer"));
    assert!(!crate::builtins::realm_function_own_props_unenumerable("Event"));
    // No declaration: unknown.
    assert!(!crate::builtins::realm_function_own_props_unenumerable("noSuchNative"));
}
