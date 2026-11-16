// ACCOWN1 (2026-11-16): an own data property shadows an inherited accessor.
// OrdinaryGet (ECMA-262 10.1.8.1 step 1) and OrdinarySet (10.1.9.2 step 1)
// find an own property first: when an instance may own the key as data (a
// class field, a birth descriptor, a later data definition), the parent's
// getter / setter never runs for it, so the site is no AccessorGet /
// AccessorSet. A class without the own key keeps its proven accessor sites.
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The verdicts (Debug text) of the sites of `key`: reads (`store` false)
/// or stores.
fn verdicts(pf: &crate::ProgramFacts, key: &str, store: bool) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.access.is_store() == store)
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    eprintln!("ACCOWN1 .{key} store={store} verdicts={v:?}");
    v
}

/// Some site of `key` exists and none has a verdict starting with `bad`.
fn none_is(pf: &crate::ProgramFacts, key: &str, store: bool, bad: &str) -> bool {
    let v = verdicts(pf, key, store);
    !v.is_empty() && v.iter().all(|x| !x.starts_with(bad))
}

/// Every site of `key` has a verdict starting with `want`.
fn all_are(pf: &crate::ProgramFacts, key: &str, store: bool, want: &str) -> bool {
    let v = verdicts(pf, key, store);
    !v.is_empty() && v.iter().all(|x| x.starts_with(want))
}

const TAIL: &str = "console.log(JSON.stringify(f(process.argv[2] ?? \"x\")));\n";

const MAT: &str = "\"use strict\";\n\
class Mat { v = 0; set k(x: any) { this.v = this.v + 1; } get k(): any { return \"acc\"; } }\n\
function r(o: any) { return o.k; }\nfunction g(o: any, x: any) { o.k = x; }\n";

// R1 (RED): a subclass class field `k` (DefineField: an own data property).
#[test]
fn accown1_r1_subclass_field_read() {
    let pf = facts(&format!(
        "{MAT}class MF extends Mat {{ k: any = 1; }}\nfunction f(v: string) {{ const q = new MF(); g(q, v); return [r(q), q.v]; }}\n{TAIL}"
    ));
    assert!(none_is(&pf, "k", false, "AccessorGet"), "R1 read claimed the getter");
    assert!(none_is(&pf, "k", true, "AccessorSet"), "R1 store claimed the setter");
}

// R2 (RED): Object.defineProperty of a data `k` on one instance chosen by argv.
#[test]
fn accown1_r2_define_one_instance_read() {
    let pf = facts(&format!(
        "{MAT}class MS extends Mat {{ w = 2; }}\nfunction f(v: string) {{ const a = new MS(); const b = new MS(); \
Object.defineProperty(v === \"b\" ? b : a, \"k\", {{ value: \"own\", writable: true, enumerable: true, configurable: true }}); \
g(a, v); g(b, v); return [r(a), r(b), a.v, b.v]; }}\n{TAIL}"
    ));
    assert!(none_is(&pf, "k", false, "AccessorGet"), "R2 read claimed the getter");
    assert!(none_is(&pf, "k", true, "AccessorSet"), "R2 store claimed the setter");
}

// R3 (RED): the field is declared in a sub-subclass; the middle class's
// instance reads through the getter at the same site.
#[test]
fn accown1_r3_subsubclass_field_read() {
    let pf = facts(&format!(
        "{MAT}class Mid extends Mat {{ m = 1; }}\nclass Leaf extends Mid {{ k: any = 3; }}\n\
function f(v: string) {{ const p = new Mid(); const q = new Leaf(); g(p, v); g(q, v); return [r(p), r(q), p.v, q.v]; }}\n{TAIL}"
    ));
    assert!(none_is(&pf, "k", false, "AccessorGet"), "R3 read claimed the getter");
}

// R4 (RED): a compound assignment (GetValue then PutValue) on an own field.
// (The added value is unknown: `o.k += 1` repeated makes an unbounded
// constant chain in this unbudgeted config.)
#[test]
fn accown1_r4_compound_own_field() {
    let pf = facts(&format!(
        "{MAT}class MF extends Mat {{ k: any = 1; }}\nfunction h(o: any, v: string) {{ o.k += v; }}\n\
function f(v: string) {{ const q = new MF(); h(q, v); h(q, v); return [q.k, q.v, v]; }}\n{TAIL}"
    ));
    assert!(none_is(&pf, "k", false, "AccessorGet"), "R4 get claimed the getter");
    assert!(none_is(&pf, "k", true, "AccessorSet"), "R4 put claimed the setter");
}

// KEEP K1: a class WITHOUT the own key keeps its proven getter and setter.
#[test]
fn accown1_k1_no_own_key_keeps_accessor() {
    let pf = facts(&format!(
        "{MAT}class MS extends Mat {{ w = 2; }}\nfunction f(v: string) {{ const s = new MS(); g(s, v); return [r(s), s.v]; }}\n{TAIL}"
    ));
    assert!(all_are(&pf, "k", false, "AccessorGet"), "K1 read lost the getter");
    assert!(all_are(&pf, "k", true, "AccessorSet"), "K1 store lost the setter");
}

// KEEP K2: a sub-subclass field elsewhere does not touch the middle class's
// own sites (its template holds no `k`).
#[test]
fn accown1_k2_middle_class_keeps_accessor() {
    let pf = facts(&format!(
        "{MAT}class Mid extends Mat {{ m = 1; }}\nclass Leaf extends Mid {{ k: any = 3; }}\n\
function f(v: string) {{ const p = new Mid(); const q = new Leaf(); g(p, v); return [r(p), p.v, q.m]; }}\n{TAIL}"
    ));
    assert!(all_are(&pf, "k", false, "AccessorGet"), "K2 read lost the getter");
    // Leaf's field definition is a store site of its own; g's store is the setter.
    assert!(verdicts(&pf, "k", true).iter().any(|x| x.starts_with("AccessorSet")), "K2 store lost the setter");
}
