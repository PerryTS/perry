//! NOTUNIT (U5): a named read whose receiver set holds `JSON.parse` values.
//! A parse without a reviver makes ordinary objects whose [[Prototype]] is
//! `%Object.prototype%` and whose own keys are data properties
//! (ECMA-262 25.5.1 JSON.parse, ParseJSON: the text is evaluated as an
//! ObjectLiteral / ArrayLiteral; 13.2.5.5 PropertyDefinitionEvaluation
//! uses CreateDataPropertyOrThrow, and JSON text is not subject to
//! Annex B.3.1, so even `__proto__` is an own data key), arrays
//! (ArrayCreate, 10.4.2) and primitives that are strings, numbers,
//! booleans or null, nothing else. A read of a key `%Object.prototype%`
//! lacks (and nothing writes there) is the object's own data or
//! `undefined`: OwnDataOrAbsent, with the String/Number/Boolean families
//! by `family_absent` and `undefined`/`null` naming the TypeError arm.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn at(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call == call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?} {:?}", s.verdict, s.receivers)).collect();
    eprintln!("NOTUNIT {key} call={call}: {v:?}");
    v.into_iter().map(|v| v.split(' ').next().unwrap_or("").to_string()).collect()
}

fn all(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v == k)
}
fn none(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v != k)
}

const C: &str = "const c = Math.random() > 0.5;\nconst s = String(Math.random());\n";
const HOLD: &str = "declare function hostTake(x: any): void;\nhostTake(0);\n";

#[test]
fn u5_root_of_a_parse_absent_key() {
    let pf = one(&format!("{C}const o: any = JSON.parse(s);\nconsole.log(o.ior);\n"));
    assert!(all(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// A field of a parse: objects, arrays, strings, numbers, booleans, null,
/// and `undefined` for an absent key; the nullish values name their arm.
#[test]
fn u5_field_of_a_parse_names_the_type_error_arm() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s);\nconst e: any = o.extensions;\nconsole.log(e.ior);\n"
    ));
    assert!(all(&at(&pf, "ior", false), "OwnDataOrAbsent"));
    assert!(pf
        .sites
        .iter()
        .filter(|s| s.key == "ior")
        .all(|s| s.receivers.iter().any(|r| r.ends_with("(TypeError)"))));
}

/// Keys `%Object.prototype%` has: the read may give its member.
#[test]
fn control_object_prototype_keys() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s);\nconsole.log(o.constructor, o.toString, o.hasOwnProperty);\n"
    ));
    for k in ["constructor", "toString", "hasOwnProperty"] {
        assert!(none(&at(&pf, k, false), "OwnDataOrAbsent"), "{k}");
    }
}

#[test]
fn control_object_prototype_written() {
    let pf = one(&format!(
        "{C}(Object.prototype as any).ior = 1;\nconst o: any = JSON.parse(s);\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// A program object with an accessor under the key beside the parse.
#[test]
fn control_program_object_mixed_in() {
    let pf = one(&format!(
        "{C}const o: any = c ? JSON.parse(s) : {{ get ior() {{ return 1; }} }};\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// Unknown code holds the realm and the value: it may define an accessor
/// on the object or set its prototype, and String.prototype takes adds.
#[test]
fn control_held_realm_and_escape() {
    let pf = one(&format!(
        "{C}{HOLD}const o: any = JSON.parse(s);\nhostTake(o);\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
    let pf = one(&format!(
        "{C}{HOLD}const o: any = JSON.parse(s);\nconst e: any = o.extensions;\nconsole.log(e.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// A reviver's results are stored in the tree (25.5.1.1
/// InternalizeJSONProperty): one returning a Symbol puts a value there no
/// family claim covers. (A reviver returning its argument keeps the claim.)
#[test]
fn control_reviver_values() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s, (k: any, v: any) => Symbol());\nconst e: any = o.extensions;\nconsole.log(e.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// An accessor defined on the parsed object.
#[test]
fn control_define_property_accessor() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s);\nObject.defineProperty(o, \"ior\", {{ get() {{ return 1; }} }});\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}
