//! PROTORECV: a member read or method call whose receiver is a program
//! class's prototype object (`X.prototype.m`).
//! ECMA-262 15.7.14 ClassDefinitionEvaluation defines each method of the
//! class body (15.4.4 DefineMethodProperty) and `constructor`
//! (MakeConstructor) as own, writable, non-enumerable data properties of
//! the prototype object when the class is evaluated; the object exists only
//! from then on. A read of such a key is one own data lookup
//! (10.1.8.1 OrdinaryGet) when no accessor of that key, no
//! defineProperty/delete (HARD), no computed member or computed store, and
//! no unknown code can reach the object. A program store of the key is an
//! ordinary [[Set]] on an own writable data property (10.1.9.2): the key
//! stays a data property and the store widens the value set the read
//! gives. A key the prototype inherits (found on a parent class's
//! prototype) is not claimed.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the sites of `key` (calls when `call`), in source order.
fn at(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call == call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?} {:?}", s.verdict, s.receivers)).collect();
    eprintln!("PROTORECV {key} call={call}: {v:?}");
    v.into_iter().map(|v| v.split(' ').next().unwrap_or("").to_string()).collect()
}

fn all(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v == k)
}
fn none_proven(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|v| v.starts_with("No"))
}

const A: &str = "class A { m() { return 1; } }\n";

#[test]
fn p1_own_method_read_in_a_table() {
    let pf = one(&format!("{A}const t = [A.prototype.m];\nconsole.log(typeof t[0]);\n"));
    assert!(all(&at(&pf, "m", false), "T0Mono"));
}

#[test]
fn p1_own_method_call() {
    let pf = one(&format!("{A}console.log(A.prototype.m());\n"));
    assert!(all(&at(&pf, "m", true), "T0Mono"));
}

#[test]
fn p1_constructor_read() {
    let pf = one(&format!("{A}console.log(A.prototype.constructor === A);\n"));
    assert!(all(&at(&pf, "constructor", false), "T0Mono"));
}

#[test]
fn c1_written_method_gives_the_set_of_both() {
    let pf = one(&format!(
        "{A}const c = Math.random() > 0.5;\nif (c) A.prototype.m = function () {{ return 2; }};\nconsole.log(A.prototype.m());\n"
    ));
    assert!(all(&at(&pf, "m", true), "T0Set"));
}

#[test]
fn c2_escaped_prototype_has_no_claim() {
    let pf = one(&format!(
        "import \"unseen-host\";\n{A}(globalThis as any).out = A.prototype;\nconsole.log(A.prototype.m());\n"
    ));
    assert!(none_proven(&at(&pf, "m", true)));
}

#[test]
fn c3_escaped_class_has_no_claim() {
    let pf = one(&format!(
        "import \"unseen-host\";\n{A}(globalThis as any).out = A;\nconsole.log(A.prototype.m());\n"
    ));
    assert!(none_proven(&at(&pf, "m", true)));
}

#[test]
fn c4_getter_is_the_accessor_path() {
    let pf = one("class A { get m() { return 1; } }\nconsole.log(A.prototype.m);\n");
    assert!(none_proven(&at(&pf, "m", false)));
}

#[test]
fn c5_inherited_method_stays_open() {
    let pf = one(&format!("{A}class B extends A {{}}\nconsole.log(typeof B.prototype.m);\n"));
    assert!(none_proven(&at(&pf, "m", false)));
}

#[test]
fn c6_computed_store_has_no_claim() {
    let pf = one(&format!(
        "{A}const k: string = Math.random() > 0.5 ? \"m\" : \"n\";\n(A.prototype as any)[k] = 5;\nconsole.log(A.prototype.m());\n"
    ));
    assert!(none_proven(&at(&pf, "m", true)));
}

#[test]
fn c7_define_property_has_no_claim() {
    let pf = one(&format!(
        "{A}Object.defineProperty(A.prototype, \"m\", {{ get() {{ return () => 3; }} }});\nconsole.log(A.prototype.m());\n"
    ));
    assert!(none_proven(&at(&pf, "m", true)));
}

#[test]
fn c8_object_assign_onto_the_prototype_is_not_the_declaration_alone() {
    let pf = one(&format!(
        "{A}Object.assign(A.prototype, {{ m() {{ return 4; }} }});\nconsole.log(A.prototype.m());\n"
    ));
    let v = at(&pf, "m", true);
    assert!(!v.is_empty() && v.iter().all(|v| v != "T0Mono"));
}

#[test]
fn c9_delete_has_no_claim() {
    let pf = one(&format!("{A}delete (A.prototype as any).m;\nconsole.log(typeof A.prototype.m);\n"));
    assert!(none_proven(&at(&pf, "m", false)));
}
