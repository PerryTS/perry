//! SKINROOT24: the property descriptor `Object.getOwnPropertyDescriptor`
//! returns (ECMA-262 20.1.2.8, 6.2.6.4 FromPropertyDescriptor) is an
//! ordinary object whose own properties are exactly `value`/`writable`/
//! `enumerable`/`configurable` (a data property) or `get`/`set`/
//! `enumerable`/`configurable` (an accessor): its values are the property's
//! value or its getter and setter, all program values. `own_descriptor`
//! (builtins/object_statics.rs) adds ⊤ to the descriptor's values whatever
//! the object holds, so a ToPrimitive of the descriptor (`${d}`, an array
//! join or toLocaleString over it: three.js r8, call c7832 -> c7845) reads a
//! ⊤ `toString` and calls it with the descriptor as receiver: the realm is
//! held, and every unlocked builtin call (here `console.log(o)`) may call
//! unknown code with `o`. Asserted by value: `o.m()` stays the program body.
//! CLOSED-WORLD PREMISE (owner 2026-11-14): no eval or dynamic code.
use super::*;

const OBJ: &str = "const o = { m() { return 1; } };\nconst src: any = { k: 1 };\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn m_calls(src: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("SKINROOT24 {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of m");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().any(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

fn exact(use_d: &str) {
    let src = format!(
        "{OBJ}const d: any = Object.getOwnPropertyDescriptor(src, \"k\");\n{use_d};\nconsole.log(o);\nconst r = o.m();\n"
    );
    let v = m_calls(&src);
    assert!(
        mono(&v),
        "a data descriptor holds program values only: {use_d}: {v:?}"
    );
}

/// RED: the descriptor's ToPrimitive reads Object.prototype.toString.
#[test]
fn descriptor_template_string() {
    exact("const s = `${d}`")
}

/// RED: the r8 shape: an array's toLocaleString runs each element's own
/// `toLocaleString` (ECMA-402 19.5.1) and makes its result a string.
#[test]
fn descriptor_array_to_locale_string() {
    exact("const s = [d].toLocaleString()")
}

/// RED: an accessor descriptor: `get` is the program's getter.
#[test]
fn accessor_descriptor_get_called() {
    let src = format!(
        "{OBJ}const a = {{ get k() {{ return 2; }} }};\nconst d: any = Object.getOwnPropertyDescriptor(a, \"k\");\nconst s = `${{d}}` + d.get.call(a);\nconsole.log(o);\nconst r = o.m();\n"
    );
    let v = m_calls(&src);
    assert!(mono(&v), "the getter is the program's: {v:?}");
}

/// Unknown code holds the realm (a declared function's call). Re-baselined
/// under R-GLOBAL step 3 (owner 2026-11-18; closed world 2026-11-14): that
/// code is the runtime's natives, which replace no member of an intrinsic
/// namespace, so `console.log(o)` stays the realm's and `o` stays proven.
#[test]
fn keep_held_by_unknown_code() {
    let src = format!(
        "declare function sink(x: any): void;\nsink(0);\n{OBJ}const d: any = Object.getOwnPropertyDescriptor(src, \"k\");\nconst s = `${{d}}`;\nconsole.log(o);\nconst r = o.m();\n"
    );
    let v = m_calls(&src);
    assert!(mono(&v), "held realm, console.log is the realm's: {v:?}");
}

/// SKINDESC1 k2: a described object the model does not know (a declared
/// `any`): its descriptor's accessors are unknown, so a getter call on `o`
/// hands `o` to unknown code.
#[test]
fn k2_unknown_object_descriptor_stays_unknown() {
    let src = format!(
        "declare function mk(): any;\nconst w = mk();\n{OBJ}const d: any = Object.getOwnPropertyDescriptor(w, \"k\");\nconst g = d.get.call(o);\nconst r = o.m();\n"
    );
    let v = m_calls(&src);
    assert!(
        !mono(&v),
        "an unknown object's accessors are unknown: {v:?}"
    );
}

/// SKINDESC1 k3: a program getter taken out by a descriptor and called on
/// another object: its result (the other object's field) is seen.
#[test]
fn k3_getter_called_on_other_object() {
    let src = format!(
        "{OBJ}const a = {{ get k() {{ return (this as any).f; }} }};\nconst b = {{ f: o }};\nconst d: any = Object.getOwnPropertyDescriptor(a, \"k\");\nconst g = d.get.call(b);\nconst r = g.m();\n"
    );
    let v = m_calls(&src);
    assert!(mono(&v), "the getter's result is b.f = o: {v:?}");
}

/// SKINDESC1 k4: a builtin accessor handed out (`Object.prototype.__proto__`
/// getter) and called on `o`.
#[test]
fn k4_builtin_accessor_handed_out() {
    let src = format!(
        "{OBJ}const d: any = Object.getOwnPropertyDescriptor(Object.prototype, \"__proto__\");\nconst p = d.get.call(o);\nconsole.log(o);\nconst r = o.m();\n"
    );
    let v = m_calls(&src);
    eprintln!("SKINDESC1 k4 verdicts {v:?}");
    assert!(K4_MONO == mono(&v), "k4: {v:?}");
}
const K4_MONO: bool = true;

/// SKINDESC1 v1: `value` of a data descriptor holds the stored value.
#[test]
fn v1_descriptor_value_is_stored_value() {
    let src = format!(
        "{OBJ}const s2 = {{ k: o }};\nconst d: any = Object.getOwnPropertyDescriptor(s2, \"k\");\nconst r = d.value.m();\n"
    );
    let v = m_calls(&src);
    // Proven (o.m, or undefined: the key is read as any key, OPEN), never unknown.
    assert!(
        v.iter().all(|x| x.starts_with("T0Mono") || x == "T0Set"),
        "d.value = o: {v:?}"
    );
}
