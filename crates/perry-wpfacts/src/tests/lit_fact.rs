//! LITFACT: an object literal with constant keys is a fact unit like a
//! class instance. Its own keys at birth are its source keys in source
//! order (ECMA-262 13.2.5.4 PropertyDefinitionEvaluation,
//! CreateDataPropertyOrThrow), so a read of a birth key is a slot read of
//! the literal layout, as a class instance birth field is.
//! (A) one literal site; (B) several literal sites with the key at the same
//! slot; controls: (C) two layouts, a later added key, a getter, a literal
//! passed to unknown code.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the reads of `key`, in source order.
fn reads(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && !s.is_call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("LITFACT {key}: {v:?}");
    v
}

/// A read proven as one layout slot (one receiver) or the same slot over
/// the receiver set.
fn slot(v: &str) -> bool {
    v == "T0Mono" || v == "T0Set"
}

#[test]
fn a_one_literal_site_birth_key_is_a_slot() {
    let pf = one("function f(x: any) { return x.b; }\n\
                  const o = { a: 1, b: 2 };\n\
                  console.log(f(o));\n");
    let v = reads(&pf, "b");
    assert!(!v.is_empty() && v.iter().all(|v| slot(v)), "{v:?}");
}

#[test]
fn b_two_literal_sites_same_slot_is_one_slot() {
    let pf = one("function f(x: any) { return x.b; }\n\
                  const o = { a: 1, b: 2 };\n\
                  const p = { a: 3, b: 4 };\n\
                  console.log(f(o) + f(p));\n");
    let v = reads(&pf, "b");
    assert!(!v.is_empty() && v.iter().all(|v| slot(v)), "{v:?}");
}

#[test]
fn c_two_layouts_are_not_one_slot() {
    let pf = one("function f(x: any) { return x.b; }\n\
                  const o = { a: 1, b: 2 };\n\
                  const p = { b: 3, a: 4 };\n\
                  console.log(f(o) + f(p));\n");
    let v = reads(&pf, "b");
    assert!(!v.is_empty() && v.iter().all(|v| !slot(v)), "{v:?}");
}

/// The class-instance rule: a key stored after birth on a fixed layout is
/// one slot as well (the template field row, `writers_post`); a literal
/// gets the same answer.
#[test]
fn a_key_added_after_birth_is_one_slot_as_on_a_class_instance() {
    let pf = one("function f(x: any) { return x.z; }\n\
                  const o: any = { a: 1, b: 2 };\n\
                  o.z = 3;\n\
                  console.log(f(o));\n");
    let v = reads(&pf, "z");
    assert!(!v.is_empty() && v.iter().all(|v| slot(v)), "{v:?}");
}

#[test]
fn a_getter_literal_is_open() {
    let pf = one("function f(x: any) { return x.a; }\n\
                  const o = { get a() { return 1; }, b: 2 };\n\
                  console.log(f(o));\n");
    let v = reads(&pf, "a");
    assert!(!v.is_empty() && v.iter().all(|v| !slot(v)), "{v:?}");
}

#[test]
fn a_literal_passed_to_unknown_code_is_open() {
    let pf = one("declare function ext(o: any): void;\n\
                  function f(x: any) { return x.b; }\n\
                  const o = { a: 1, b: 2 };\n\
                  ext(o);\n\
                  console.log(f(o));\n");
    let v = reads(&pf, "b");
    assert!(!v.is_empty() && v.iter().all(|v| !slot(v)), "{v:?}");
}
