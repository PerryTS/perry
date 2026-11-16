// R6NEXT1 (2026-11-15): RED fixtures for the two largest no-loader reason
// groups of r1/r2/r3/r6/r9a (/root/PEELM/WORKLIST-NOLOADER.md):
// "Throws" (a nullish-only receiver under a guard that excludes it) and
// "key not a definite birth field". Small JS, no three.js. Proven = a verdict
// that is neither No(..) nor Throws (Dead and OwnDataOrAbsent count).
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

/// Every verdict of the reads (calls: `call`) of `key` in a.ts.
fn verdicts(src: &str, key: &str, call: bool) -> Vec<crate::Verdict> {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    let pf = crate::compute(&list, &cfg);
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.is_call == call)
        .map(|s| s.verdict.clone())
        .collect();
    // A body the solve never activates has no site: Dead.
    if v.is_empty() {
        vec![crate::Verdict::Dead]
    } else {
        v
    }
}
fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}
fn all_proven(src: &str, key: &str, call: bool) {
    let v = verdicts(src, key, call);
    assert!(v.iter().all(proven), ".{key}: {v:?}");
}
/// Every read of `key` keeps `null` among its receivers (the guard must not
/// drop the null a later store, getter or iteration gives): a receiver set of
/// objects and `null` is proven with its TypeError arm, so the verdict alone
/// cannot show a wrong narrowing.
fn keeps_null(src: &str, key: &str) {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    let pf = crate::compute(&list, &cfg);
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let r: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.is_call)
        .map(|s| (s.verdict.clone(), s.receivers.clone()))
        .collect();
    assert!(
        !r.is_empty() && r.iter().all(|(_, rs)| rs.iter().any(|x| x.starts_with("primitive:null"))),
        ".{key} must keep null: {r:?}"
    );
}
fn some_unproven(src: &str, key: &str, call: bool) {
    let v = verdicts(src, key, call);
    assert!(v.iter().any(|v| !proven(v)), ".{key} must stay unproven: {v:?}");
}

// ---- group T: Throws (receiver holds only null/undefined) ----
// T1 (Object3D.updateMatrix `const pivot = this.pivot; if (pivot !== null) pivot.x`):
// a strict `!== null` test of a local (7.2.16 IsStrictlyEqual: only null is `=== null`).
const T1: &str = "class P { constructor() { this.pivot = null; this.k = 0; }
  upd() { const pivot = this.pivot; if (pivot !== null) { this.k = pivot.px; } return this.k; } }
const p = new P(); console.log(p.upd());";
#[test]
fn r6next_t1_strict_not_null_local_guard_is_dead() {
    all_proven(T1, "px", false);
}
// keep: `!== undefined` does not exclude null: the read runs and throws.
const T1K: &str = "class P { constructor() { this.pivot = null; this.k = 0; }
  upd() { const pivot = this.pivot; if (pivot !== undefined) { this.k = pivot.px; } return this.k; } }
const p = new P(); console.log(p.upd());";
#[test]
fn r6next_t1_keep_not_undefined_on_null_throws() {
    some_unproven(T1K, "px", false);
}
// keep: `!== null` does not exclude undefined (7.2.16: different types, not
// equal): an undefined-only local still throws on the read.
const T1U: &str = "class P { constructor() { this.pivot = undefined; this.k = 0; }
  upd() { const pivot = this.pivot; if (pivot !== null) { this.k = pivot.px; } return this.k; } }
const p = new P(); console.log(p.upd());";
#[test]
fn r6next_t1_keep_not_null_on_undefined_throws() {
    some_unproven(T1U, "px", false);
}
// `!= null` excludes both null and undefined (7.2.15 steps 2-3).
const T1L: &str = "class P { constructor() { this.pivot = undefined; this.q = null; this.k = 0; }
  upd() { const pivot = this.pivot; const q = this.q; if (pivot != null) { this.k = pivot.px; }
    if (q != null) { this.k = q.px; } return this.k; } }
const p = new P(); console.log(p.upd());";
#[test]
fn r6next_t1_loose_not_null_excludes_both() {
    all_proven(T1L, "px", false);
}

// T2 (OrthographicCamera.updateProjectionMatrix `this.view !== null && this.view.enabled`):
// the guard tests a member path of `this`; the read is the same member path.
const T2: &str = "class C { constructor() { this.view = null; this.w = 1; }
  upd() { let w = this.w; if (this.view !== null && this.view.enabled) { w = w * this.view.fw; } return w; } }
const c = new C(); console.log(c.upd());";
#[test]
fn r6next_t2_member_path_guard_enabled_is_dead() {
    all_proven(T2, "enabled", false);
}
#[test]
fn r6next_t2_member_path_guard_body_is_dead() {
    all_proven(T2, "fw", false);
}
// keep (i): a call between the test and the read stores null after the
// test passed (`open` made the guard pass): the read throws on that path.
const T2K: &str = "class C { constructor() { this.view = null; this.w = 1; }
  open() { this.view = { fw: 2 }; } reset() { this.view = null; }
  upd() { let w = this.w; if (this.view !== null) { this.reset(); w = this.view.fw; } return w; } }
const c = new C(); c.open(); console.log(c.upd());";
#[test]
fn r6next_t2_keep_call_between_guard_and_read() {
    keeps_null(T2K, "fw");
}
// keep (ii): `view` is an accessor: the test's read and the later read are
// two getter calls; the second gives null.
const T2A: &str = "class C { constructor() { this.n = 0; this.w = 1; }
  get view() { this.n = this.n + 1; return this.n > 1 ? null : { fw: 2 }; }
  upd() { let w = this.w; if (this.view !== null) { w = this.view.fw; } return w; } }
const c = new C(); console.log(c.upd());";
#[test]
fn r6next_t2_keep_accessor_member() {
    keeps_null(T2A, "fw");
}
// keep (iii): a closure created under the guard runs after a store of null.
const T2C: &str = "class C { constructor() { this.view = null; this.w = 1; } open() { this.view = { fw: 2 }; }
  upd() { if (this.view !== null) { const g = () => this.view.fw; this.view = null; return g(); } return 0; } }
const c = new C(); c.open(); console.log(c.upd());";
#[test]
fn r6next_t2_keep_closure_in_guarded_code() {
    keeps_null(T2C, "fw");
}
// keep (iv): in a loop, a read BEFORE the guard sees the null the end of the
// previous iteration stored.
const T2L: &str = "class C { constructor() { this.view = { fw: 2, fw2: 3 }; this.w = 0; }
  upd() { for (let i = 0; i < 2; i++) { this.w = this.w + this.view.fw;
    if (this.view !== null) { this.w = this.w + this.view.fw2; } this.view = null; } return this.w; } }
const c = new C(); console.log(c.upd());";
#[test]
fn r6next_t2_keep_loop_read_before_guard() {
    keeps_null(T2L, "fw");
}
// The same path guard, `!= null` and `!== undefined` forms (7.2.15, 7.2.16).
const T2F: &str = "class C { constructor() { this.view = null; this.u = undefined; this.w = 1; }
  upd() { let w = this.w; if (this.view != null) { w = this.view.fa; }
    if (this.u !== undefined) { w = this.u.fa; } if (this.view === null) { return w; } return this.view.fa; } }
const c = new C(); console.log(c.upd());";
#[test]
fn r6next_t2_member_path_guard_forms_are_dead() {
    all_proven(T2F, "fa", false);
}
// keep: `!== null` passes an undefined-only member: the read throws.
const T2U: &str = "class C { constructor() { this.view = undefined; this.w = 1; }
  upd() { let w = this.w; if (this.view !== null) { w = this.view.fu; } return w; } }
const c = new C(); console.log(c.upd());";
#[test]
fn r6next_t2_keep_not_null_passes_undefined() {
    some_unproven(T2U, "fu", false);
}

// T3 (PerspectiveCamera.updateProjectionMatrix: `const view = this.view;
// if (this.view !== null && this.view.enabled) { view.fullWidth }`): alias read before the guard.
const T3: &str = "class C { constructor() { this.view = null; this.w = 1; }
  upd() { let w = this.w; const view = this.view; if (this.view !== null && this.view.enabled) { w = view.fw2; } return w; } }
const c = new C(); console.log(c.upd());";
#[test]
#[ignore = "rule T3 not built"]
fn r6next_t3_alias_read_before_guard_is_dead() {
    all_proven(T3, "fw2", false);
}

// T4 (Material.setValues: for-in, `this[key]`, `=== undefined` continue, `c && c.isColor`).
const T4: &str = "class M { constructor() { this.map = null; this.n = 1; }
  setValues(values) { for (const key in values) { const nv = values[key]; if (nv === undefined) continue;
    const cv = this[key]; if (cv === undefined) continue;
    if (cv && cv.isColor) { cv.n = nv; } else { this[key] = nv; } } } }
const m = new M(); m.setValues({ map: null, n: 2 }); console.log(m.n);";
#[test]
fn r6next_t4_truthy_guard_in_for_in_is_dead() {
    all_proven(T4, "isColor", false);
}

// T5 (`typeof __THREE_DEVTOOLS__ !== 'undefined'` then `__THREE_DEVTOOLS__.dispatchEvent(..)`):
// an undeclared global behind a typeof guard (13.5.3 step 2: unresolvable -> "undefined").
const T5: &str = "if (typeof __DEVT__ !== \"undefined\") { __DEVT__.ping(1); }
console.log(1);";
#[test]
fn r6next_t5_typeof_guarded_global_is_dead() {
    all_proven(T5, "ping", true);
    assert!(verdicts(T5, "ping", true).iter().all(|v| matches!(v, crate::Verdict::Dead)));
}
// The three.js form: the guard in a constructor, `this` passed.
const T5C: &str = "class A { constructor() { this.k = 1;
  if (typeof __DEVT__ !== \"undefined\") { __DEVT__.ping(this); } } }
const a = new A(); console.log(a.k);";
#[test]
fn r6next_t5_typeof_guard_in_constructor_is_dead() {
    let v = verdicts(T5C, "ping", true);
    assert!(v.iter().all(|v| matches!(v, crate::Verdict::Dead)), "{v:?}");
}
// `typeof NAME === "undefined"` false: the else branch is the dead one.
const T5E: &str = "if (typeof __DEVT__ === \"undefined\") { console.log(0); } else { __DEVT__.ping(1); }";
#[test]
fn r6next_t5_typeof_eq_undefined_else_is_dead() {
    let v = verdicts(T5E, "ping", true);
    assert!(v.iter().all(|v| matches!(v, crate::Verdict::Dead)), "{v:?}");
}
// A top-level `var` of another module: an ES module's `var` is bound in the
// module environment record (16.2.1.6.4 InitializeEnvironment step 21), not
// on the global object; perry lowers it to a module global, so `typeof` in
// a.ts still reads the global object: dead.
#[test]
fn r6next_t5_other_module_var_is_not_a_global() {
    let b = "export var __DEVT__ = { ping(x) { return x; } }; console.log(__DEVT__.ping(2));";
    let mut h = hirs(T5);
    let bp = "b.ts".to_string();
    let bm = perry_parser::parse_typescript(b, &bp).expect("parses");
    h.push((bp.clone(), perry_hir::lower_module(&bm, "b", &bp).expect("lowers")));
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig { unit_io: true, ..Default::default() };
    let pf = crate::compute(&list, &cfg);
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf.sites.iter().filter(|s| s.module == mi && s.key == "ping" && s.is_call).map(|s| s.verdict.clone()).collect();
    assert!(v.iter().all(|v| matches!(v, crate::Verdict::Dead)), "{v:?}");
}
/// The guarded call keeps a site that is not Dead (rule T5 must not fire).
fn live(src: &str, key: &str) {
    let v = verdicts(src, key, true);
    assert!(v.iter().any(|v| !matches!(v, crate::Verdict::Dead)), ".{key} must stay live: {v:?}");
}
const T5G: &str = "if (typeof __DEVT__ !== \"undefined\") { __DEVT__.ping(1); }";
// keep: `globalThis.NAME = v` elsewhere.
#[test]
fn r6next_t5_keep_global_this_store() {
    live(&format!("function inst() {{ globalThis.__DEVT__ = {{ ping(x) {{ return x; }} }}; }}\n{T5G}\ninst();"), "ping");
}
// keep: `globalThis[k] = v`, k unknown.
#[test]
fn r6next_t5_keep_global_this_computed_store() {
    live(&format!("const k = String(Math.random()); globalThis[k] = {{ ping(x) {{ return x; }} }};\n{T5G}"), "ping");
}
// keep: `Object.prototype[k] = v`, k unknown (the global object inherits it).
#[test]
fn r6next_t5_keep_object_prototype_computed_store() {
    live(&format!("const k = String(Math.random()); Object.prototype[k] = {{ ping(x) {{ return x; }} }};\n{T5G}"), "ping");
}
// keep: `Object.defineProperty(globalThis, "NAME", ..)`.
#[test]
fn r6next_t5_keep_define_property_global() {
    live(&format!("Object.defineProperty(globalThis, \"__DEVT__\", {{ value: {{ ping(x) {{ return x; }} }} }});\n{T5G}"), "ping");
}
// keep: an assignment to the undeclared NAME (strict: writes an existing
// global, else throws; it is a store into the global object under NAME).
#[test]
fn r6next_t5_keep_undeclared_assignment() {
    live(&format!("function inst() {{ __DEVT__ = {{ ping(x) {{ return x; }} }}; }}\n{T5G}\ntry {{ inst(); }} catch (e) {{}}"), "ping");
}
// keep: a host global (`typeof console`, `typeof process`) stays live.
#[test]
fn r6next_t5_keep_host_global() {
    live("if (typeof console !== \"undefined\") { console.log(1); }", "log");
    let p = "if (typeof process !== \"undefined\") { const e = process.env; console.log(e.HOME); }";
    let v = verdicts(p, "HOME", false);
    assert!(v.iter().any(|v| !matches!(v, crate::Verdict::Dead)), ".HOME must stay live: {v:?}");
}
// keep: a declared `let` in its TDZ: `typeof` throws (13.5.3 step 2 with a
// resolvable reference: GetValue throws ReferenceError), not this rule.
#[test]
fn r6next_t5_keep_tdz_let() {
    live("function f() { if (typeof dv !== \"undefined\") { dv.ping(1); } }\nlet dv = { ping(x) { return x; } }; f();", "ping");
}

// ---- group B: key not a definite birth field ----
// B1 (Object3D constructor: Object.defineProperties(this, { position: { configurable,
// enumerable, value } })): 20.1.2.3.1 ObjectDefineProperties, each named data descriptor.
const B1: &str = "class V { constructor() { this.x = 1; } }
class O { constructor() { const position = new V();
  Object.defineProperties(this, { position: { configurable: true, enumerable: true, value: position } }); this.k = 0; }
  upd() { return this.position.x; } }
const o = new O(); console.log(o.upd() + o.position.x);";
#[test]
#[ignore = "rule B1 (DESIGN-R6NEXT) not built"]
fn r6next_b1_define_properties_named_data_is_birth_field() {
    all_proven(B1, "position", false);
}
// keep: a defineProperties accessor key is not a data field.
const B1K: &str = "class O { constructor() { Object.defineProperties(this, { pos: { get() { return 3; } } }); this.k = 0; } }
const o = new O(); console.log(o.pos);";
#[test]
fn r6next_b1_keep_define_properties_accessor() {
    some_unproven(B1K, "pos", false);
}
// keep: a defineProperties on one path only is not definite.
const B1C: &str = "class V { constructor() { this.x = 1; } }
class O { constructor(f) { if (f) { Object.defineProperties(this, { position: { value: new V() } }); } this.k = 0; } }
const o = new O(Math.random() > 2); console.log(o.position.x);";
#[test]
fn r6next_b1_keep_conditional_define_properties() {
    some_unproven(B1C, "position", false);
}

// B2 (BufferAttribute: `if (Array.isArray(array)) throw ..;` and a defineProperty
// before the stores; subclass through super).
const B2: &str = "let nid = 0;
class E { constructor() {} }
class A extends E { constructor(array, s) { super(); if (Array.isArray(array)) { throw new TypeError(\"t\"); }
  Object.defineProperty(this, \"id\", { value: nid++ });
  this.array = array; this.itemSize = s; }
  getX(i) { return this.array[i * this.itemSize]; } }
class F extends A { constructor(a, s) { super(new Float32Array(a), s); } }
const a = new F([1, 2], 1); const b = new A(new Uint16Array(2), 1); console.log(a.getX(0) + b.getX(1));";
#[test]
fn r6next_b2_store_after_throwing_branch_is_birth_field() {
    all_proven(B2, "itemSize", false);
}

// B3 (Vector2.clone `new this.constructor(this.x, this.y)`): the class prototype's own
// `constructor` (15.7.14 ClassDefinitionEvaluation, 10.2.5 MakeConstructor).
const B3: &str = "class V { constructor(x) { this.x = x; } clone() { return new this.constructor(this.x); } }
const v = new V(1); console.log(v.clone().x);";
#[test]
fn r6next_b3_prototype_constructor_read() {
    all_proven(B3, "constructor", false);
}
// keep: an instance that may get an own `constructor` by a named store.
const B3N: &str = "class V { constructor(x) { this.x = x; } clone() { return new this.constructor(this.x); } }
const v = new V(1); const w = new V(2); w.constructor = V; console.log(v.clone().x + w.clone().x);";
#[test]
fn r6next_b3_keep_named_own_constructor_store() {
    some_unproven(B3N, "constructor", false);
}
// keep: an unknown-key store on an instance may name `constructor`.
const B3C: &str = "class V { constructor(x) { this.x = x; } clone() { return new this.constructor(this.x); } }
const v = new V(1); const k = String(Math.random()); v[k] = V; console.log(v.clone().x);";
#[test]
fn r6next_b3_keep_computed_own_store() {
    some_unproven(B3C, "constructor", false);
}
// keep: an unknown-key store on the prototype may replace `constructor`.
const B3P: &str = "class V { constructor(x) { this.x = x; } clone() { return new this.constructor(this.x); } }
const v = new V(1); const k = String(Math.random()); V.prototype[k] = function () {}; console.log(v.clone().x);";
#[test]
fn r6next_b3_keep_computed_proto_store() {
    some_unproven(B3P, "constructor", false);
}

// B4 keep (Quaternion._onChangeCallback: a proto method for some instances, an own key
// stored after birth by `_onChange(cb)` on others): two outcomes, stays unproven.
const B4K: &str = "class Q { constructor() { this.x = 0; } _onChange(cb) { this._cb = cb; return this; } _cb() {}
  set(x) { this.x = x; this._cb(); return this; } }
const q1 = new Q(); const q2 = new Q()._onChange(() => console.log(1)); q1.set(1); q2.set(2);";
#[test]
fn r6next_b4_keep_post_birth_own_or_proto() {
    some_unproven(B4K, "_cb", true);
}
