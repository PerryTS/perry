// D1BUILD1 (2026-11-15): `Object.defineProperty( this, "id", { value: n ++ } )`
// in a constructor is no leak of `this` (cause D1, /root/PEELM/NOTE.md
// `## R6WHY1`): the keys stored after it stay definite from birth, on the
// class and on its subclasses. KEEP fixtures: forms where `this` may be
// observed (a value that holds it, a closure, a non-literal descriptor, an
// accessor descriptor, a spread, a computed key, `__proto__`).
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn verdicts(src: &str, key: &str) -> Vec<crate::Verdict> {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    let pf = crate::compute(&list, &cfg);
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect();
    if v.is_empty() {
        vec![crate::Verdict::Dead]
    } else {
        v
    }
}
fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}
fn all_proven(src: &str, key: &str) {
    let v = verdicts(src, key);
    assert!(v.iter().all(proven), ".{key}: {v:?}");
}
fn some_unproven(src: &str, key: &str) {
    let v = verdicts(src, key);
    assert!(
        v.iter().any(|v| !proven(v)),
        ".{key} must stay unproven: {v:?}"
    );
}

/// The constructor with statement `def` first, then the plain stores; a
/// subclass; reads of the stored keys on both.
fn prog(def: &str, pre: &str) -> String {
    format!(
        r#"{pre}
let _n = 0;
class V {{ x: number; constructor() {{ this.x = 1; }} }}
class B {{
  constructor() {{
    {def}
    this.pivot = null;
    this.view = null;
    this.name = 'b';
  }}
}}
class S extends B {{ constructor() {{ super(); this.k = 2; }} }}
function use(o: any) {{
  if (o.pivot !== null) console.log(o.pivot.x);
  console.log(o.name.length);
}}
use(new B());
use(new S());
"#
    )
}

const DEF1: &str = "Object.defineProperty(this, 'id', { value: _n++ });";
const DEFS: &str =
    "Object.defineProperties(this, { a: { value: 1 }, b: { enumerable: true, value: new V() } });";

#[test]
fn d1_define_property_value_is_no_leak() {
    let p = prog(DEF1, "");
    all_proven(&p, "length");
    all_proven(&p, "x");
}

#[test]
fn d1_define_property_all_data_keys() {
    let p = prog(
        "Object.defineProperty(this, 'id', { value: _n++, writable: false, enumerable: false, configurable: true });",
        "",
    );
    all_proven(&p, "length");
    all_proven(&p, "x");
}

#[test]
fn d1_define_properties_literal_is_no_leak() {
    let p = prog(DEFS, "");
    all_proven(&p, "length");
    all_proven(&p, "x");
}

// KEEP: the descriptor is not a literal.
#[test]
fn d1_keep_descriptor_variable() {
    let p = prog(
        "const d = { value: 1 }; Object.defineProperty(this, 'id', d);",
        "",
    );
    some_unproven(&p, "x");
}

// KEEP: an accessor descriptor.
#[test]
fn d1_keep_accessor_descriptor() {
    let p = prog(
        "Object.defineProperty(this, 'id', { get() { return 1; } });",
        "",
    );
    some_unproven(&p, "x");
    let p = prog("Object.defineProperty(this, 'id', { set(v: any) {} });", "");
    some_unproven(&p, "x");
}

// KEEP: a spread in the descriptor.
#[test]
fn d1_keep_descriptor_spread() {
    let p = prog(
        "Object.defineProperty(this, 'id', { ...ex, value: 1 });",
        "const ex: any = { enumerable: true };",
    );
    some_unproven(&p, "x");
}

// KEEP: a computed descriptor key.
#[test]
fn d1_keep_descriptor_computed_key() {
    let p = prog(
        "Object.defineProperty(this, 'id', { [dk]: 1 });",
        "let dk = 'va' + 'lue';",
    );
    some_unproven(&p, "x");
}

// KEEP: the key is not a constant string.
#[test]
fn d1_keep_key_not_constant() {
    let p = prog(
        "Object.defineProperty(this, kk, { value: 1 });",
        "let kk = 'i' + 'd';",
    );
    some_unproven(&p, "x");
}

// KEEP: the value holds `this` (an accessor on Object.prototype under a
// descriptor field name runs with the descriptor as receiver, 6.2.6.5
// ToPropertyDescriptor -> Get) or passes it to code.
#[test]
fn d1_keep_value_holds_this() {
    let p = prog("Object.defineProperty(this, 'id', { value: this });", "");
    some_unproven(&p, "x");
    let p = prog(
        "Object.defineProperty(this, 'id', { value: f(this) });",
        "let seen: any = null; function f(o: any) { seen = o; return 0; }",
    );
    some_unproven(&p, "x");
    let p = prog(DEF1.replace("_n++", "() => this").as_str(), "");
    some_unproven(&p, "x");
}

// KEEP: `__proto__` in the descriptor literal gives it another prototype.
#[test]
fn d1_keep_descriptor_proto() {
    let p = prog(
        "Object.defineProperty(this, 'id', { __proto__: pp, value: 1 });",
        "const pp: any = {};",
    );
    some_unproven(&p, "x");
}
