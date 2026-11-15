// BIRTHDEF1 (2026-11-15): RED fixtures for "key not a definite birth field"
// on keys a constructor creates with Object.defineProperty/defineProperties
// (three.js Object3D: position/rotation/quaternion/scale, `id`), and for
// stores that run a class setter (Euler `set y`). Small JS, no three.js.
// Proven = a verdict that is neither No(..) nor Throws.
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// (offset, is_call, verdict, receivers) of every site of `key` in a.ts.
/// The read and call sites of `key` (no store sites, ACCSITE1).
fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<(u32, bool, crate::Verdict, Vec<String>)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| (s.byte_offset, s.is_call, s.verdict.clone(), s.receivers.clone()))
        .collect()
}

fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<(u32, bool, crate::Verdict, Vec<String>)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key)
        .map(|s| (s.byte_offset, s.is_call, s.verdict.clone(), s.receivers.clone()))
        .collect()
}

fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}

fn show(tag: &str, pf: &crate::ProgramFacts, keys: &[&str]) {
    for k in keys {
        for s in sites(pf, k) {
            eprintln!("BD1 {tag} site .{k} @{} call={} verdict={:?} recv={:?}", s.0, s.1, s.2, s.3);
        }
    }
}

fn show_class(tag: &str, pf: &crate::ProgramFacts, class: &str) {
    for t in pf.templates.iter().filter(|t| t.display.ends_with(class)) {
        eprintln!(
            "BD1 {tag} tmpl {} fixed_layout={} adds_keys={} birth_keys={:?}",
            t.display, t.fixed_layout, t.adds_keys, t.birth_keys
        );
        for f in &t.fields {
            eprintln!(
                "BD1 {tag}   field {} slot={:?} definite={} data={} readonly={} wc={} wp={}",
                f.key, f.slot, f.definite, f.data_field, f.readonly, f.writers_ctor, f.writers_post
            );
        }
    }
}

// F1 (Object3D:12051): a literal data descriptor map on `this` in the
// constructor, every path, before `this` escapes. Expected: `.p` proven (a
// slot read: attributes never change [[Get]] of a data property, 10.1.8.1),
// and `.x` on it has the receiver V alone.
const F1: &str = "class V { constructor() { this.x = Number(process.argv[2] ?? \"1\"); } }
class O { constructor() { const position = new V();
  Object.defineProperties(this, { p: { configurable: true, enumerable: true, value: position } }); this.k = 0; }
  upd() { return this.p.x; } }
const o = new O(); console.log(o.upd() + o.p.x);";
#[test]
fn birthdef1_f1_define_properties_literal_is_birth_field() {
    let pf = facts(F1);
    show("F1", &pf, &["p", "x"]);
    show_class("F1", &pf, "O");
    let p = reads(&pf, "p");
    assert!(!p.is_empty() && p.iter().all(|s| proven(&s.2)), "F1 .p: {p:?}");
}

// F2 (Object3D:11980): `Object.defineProperty(this, "id", {value})` first,
// then ordinary `this.a = ..` fields. Expected: `a`, `b` stay definite birth
// fields with a slot, as without the define line (F2N), and `.id` is proven.
const F2: &str = "let nid = 0;
class V { constructor() { this.x = 1; } }
class O { constructor() { Object.defineProperty(this, \"id\", { value: nid++ });
  this.a = Number(process.argv[2] ?? \"1\"); this.b = new V(); }
  f() { return this.a + this.b.x + this.id; } }
const o = new O(); console.log(o.f() + o.a + o.id);";
const F2N: &str = "let nid = 0;
class V { constructor() { this.x = 1; } }
class O { constructor() { nid++;
  this.a = Number(process.argv[2] ?? \"1\"); this.b = new V(); }
  f() { return this.a + this.b.x; } }
const o = new O(); console.log(o.f() + o.a);";
#[test]
fn birthdef1_f2_id_define_keeps_ordinary_slots() {
    let pn = facts(F2N);
    show("F2N", &pn, &["a", "b"]);
    show_class("F2N", &pn, "O");
    let pf = facts(F2);
    show("F2", &pf, &["a", "b", "id"]);
    show_class("F2", &pf, "O");
    let t = pf.templates.iter().find(|t| t.display.ends_with("O")).expect("O");
    for k in ["a", "b"] {
        let f = t.fields.iter().find(|f| f.key == k).expect("field");
        assert!(f.definite && f.slot.is_some(), "F2 field {k}: {f:?}");
    }
    let ab: Vec<_> = sites(&pf, "a").into_iter().chain(sites(&pf, "b")).collect();
    assert!(ab.iter().all(|s| proven(&s.2)), "F2 .a/.b: {ab:?}");
    let id = sites(&pf, "id");
    assert!(!id.is_empty() && id.iter().all(|s| proven(&s.2)), "F2 .id: {id:?}");
}

// F3 KEEP: a store to the non-writable defined key (writable absent = false,
// 20.1.2.3.1 -> ToPropertyDescriptor; 10.1.9.2 OrdinarySetWithOwnDescriptor
// step 2.a returns false, strict code throws TypeError). The value set of
// `o.p` stays {V}: `.x` must never see W.
const F3: &str = "class V { constructor() { this.x = 1; } }
class W { constructor() { this.x = 2; this.w = 3; } }
class O { constructor() {
  Object.defineProperties(this, { p: { configurable: true, enumerable: true, value: new V() } }); this.k = 0; } }
const o = new O();
try { o.p = new W(); } catch (e) { console.log(\"TypeError\"); }
console.log(o.p.x);";
#[test]
fn birthdef1_f3_keep_nonwritable_store_does_not_store() {
    let pf = facts(F3);
    show("F3", &pf, &["p", "x"]);
    let x = reads(&pf, "x");
    let leaked: Vec<_> = x.iter().filter(|s| s.3.iter().any(|r| r.ends_with(":W"))).collect();
    assert!(leaked.is_empty(), "F3 .x sees W: {leaked:?}");
    let p = reads(&pf, "p");
    assert!(!p.is_empty() && p.iter().all(|s| proven(&s.2)), "F3 .p: {p:?}");
}

// F4 KEEP: a descriptor map from data: unknown keys and attributes.
const F4: &str = "class O { constructor() { Object.defineProperties(this, JSON.parse(process.argv[2] ?? \"{}\")); this.k = 0; } }
const o = new O(); console.log(o.p);";
#[test]
fn birthdef1_f4_keep_unknown_descriptor_map() {
    let pf = facts(F4);
    show("F4", &pf, &["p"]);
    let p = reads(&pf, "p");
    assert!(p.iter().any(|s| !proven(&s.2)), "F4 .p must stay unproven: {p:?}");
}

// F5 KEEP: a defined ACCESSOR: the read runs the getter, never a slot read.
const F5: &str = "class V { constructor() { this.x = 1; } }
class O { constructor() { Object.defineProperties(this, { p: { configurable: true, get() { return new V(); } } }); this.k = 0; } }
const o = new O(); console.log(o.p.x);";
#[test]
fn birthdef1_f5_keep_defined_accessor() {
    let pf = facts(F5);
    show("F5", &pf, &["p", "x"]);
    show_class("F5", &pf, "O");
    let p = reads(&pf, "p");
    assert!(
        p.iter().all(|s| !matches!(s.2, crate::Verdict::T0Mono | crate::Verdict::T0Set)),
        "F5 .p must not be a slot read: {p:?}"
    );
}

// F6 (Euler `set y`): a store that runs a class setter, plain and compound.
// Expected: the two stores are sites ("calls exactly setter E.y"), proven,
// beside the two reads (getter calls).
const F6: &str = "class E { _y = 0;
  set y(v) { this._y = v; this.cb(); }
  get y() { return this._y; }
  cb() {} }
const e = new E();
e.y = Number(process.argv[2] ?? \"1\");
e.y += 2;
console.log(e.y);";
#[test]
fn birthdef1_f6_setter_store_is_a_site() {
    let pf = facts(F6);
    show("F6", &pf, &["y", "_y", "cb"]);
    let y = sites(&pf, "y");
    assert!(y.len() >= 4 && y.iter().all(|s| proven(&s.2)), "F6 .y (2 reads + 2 setter stores): {y:?}");
}

// F7 (BIRTHBUILD1): a `delete` of a configurable defined key: the key may be
// absent after it (10.1.10.1 OrdinaryDelete), so no read of it is a plain
// definite-slot read (the existing delete handling, not a new rule).
const F7: &str = "class V { constructor() { this.x = 1; } }
class O { constructor() {
  Object.defineProperties(this, { p: { configurable: true, enumerable: true, value: new V() } }); this.k = 0; } }
const o = new O();
if (process.argv[2] === \"d\") delete o.p;
console.log(o.p === undefined ? 0 : o.p.x);";
#[test]
fn birthdef1_f7_delete_of_configurable_defined_key() {
    let pf = facts(F7);
    show("F7", &pf, &["p", "x"]);
    show_class("F7", &pf, "O");
    let p = reads(&pf, "p");
    assert!(
        p.iter().all(|s| !matches!(s.2, crate::Verdict::T0Mono | crate::Verdict::T0Set)),
        "F7 .p after a delete must not be a definite slot read: {p:?}"
    );
}

// F8 (BIRTHBUILD1): a subclass constructor redefines the parent's
// configurable defined key with another literal descriptor (10.1.6.3
// ValidateAndApplyPropertyDescriptor on an existing configurable key: value
// and attributes replaced). Implemented: the redefinition is not a birth
// define (the key is an ancestor's birth key) but the general define, which
// makes the subclass instance HARD: W is never lost.
const F8: &str = "class V { constructor() { this.x = 1; } }
class W { constructor() { this.x = 2; this.w = 3; } }
class A { constructor() { Object.defineProperty(this, \"p\", { configurable: true, enumerable: true, value: new V() }); } }
class B extends A { constructor() { super();
  Object.defineProperty(this, \"p\", { configurable: true, enumerable: false, writable: true, value: new W() }); } }
const b = new B();
console.log(b.p.x);";
#[test]
fn birthdef1_f8_subclass_redefine_keeps_its_value() {
    let pf = facts(F8);
    show("F8", &pf, &["p", "x"]);
    show_class("F8", &pf, "A");
    show_class("F8", &pf, "B");
    let x = reads(&pf, "x");
    assert!(
        x.iter().all(|s| !proven(&s.2) || s.3.iter().any(|r| r.ends_with(":W"))),
        "F8 b.p.x must see W (or stay unproven): {x:?}"
    );
}

// F9 (BIRTHBUILD1): for-in lists the enumerable defined key `p` and the
// stored `k`, never the non-enumerable `id` (14.7.5.10 EnumerateObject
// Properties: own [[Enumerable]] String keys): `o[k]` in the loop never
// reads id's W.
const F9: &str = "class V { constructor() { this.x = 1; } }
class W { constructor() { this.x = 2; this.w = 3; } }
class O { constructor() { Object.defineProperty(this, \"id\", { value: new W() });
  Object.defineProperties(this, { p: { configurable: true, enumerable: true, value: new V() } }); this.k = new V(); } }
const o = new O(); let s = 0;
for (const k in o) { s += o[k].x; }
console.log(s + o.id.x);";
#[test]
fn birthdef1_f9_for_in_skips_non_enumerable_defined_key() {
    let pf = facts(F9);
    show("F9", &pf, &["p", "id", "k", "x"]);
    show_class("F9", &pf, "O");
    let mut x = sites(&pf, "x");
    x.sort_by_key(|s| s.0);
    let first = x.first().expect("loop .x");
    assert!(
        first.3.iter().any(|r| r.ends_with(":V")) && !first.3.iter().any(|r| r.ends_with(":W")),
        "F9 loop o[k].x: {first:?}"
    );
}

// F3b (BIRTHBUILD1): F3 with a NON-configurable key: no delete or
// redefinition can ever make it writable, so the store never stores
// (10.1.9.2 step 2.a) and `.x` never sees W. (F3's configurable key may be
// redefined writable or deleted by code it escapes to: its store flows.)
const F3B: &str = "class V { constructor() { this.x = 1; } }
class W { constructor() { this.x = 2; this.w = 3; } }
class O { constructor() {
  Object.defineProperties(this, { p: { enumerable: true, value: new V() } }); this.k = 0; } }
const o = new O();
try { o.p = new W(); } catch (e) { console.log(\"TypeError\"); }
console.log(o.p.x);";
#[test]
fn birthdef1_f3b_nonconfigurable_nonwritable_store_does_not_store() {
    let pf = facts(F3B);
    show("F3b", &pf, &["p", "x"]);
    let x = reads(&pf, "x");
    assert!(x.iter().all(|s| !s.3.iter().any(|r| r.ends_with(":W"))), "F3b .x sees W: {x:?}");
    let p = reads(&pf, "p");
    assert!(!p.is_empty() && p.iter().all(|s| proven(&s.2)), "F3b .p: {p:?}");
}
