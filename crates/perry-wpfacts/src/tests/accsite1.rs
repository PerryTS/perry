// ACCSITE1 (2026-11-15): RED fixtures for STORE sites (`o.k = v`, compound,
// setter calls) and GETTER-read sites. Today a store is no site and a read
// that resolves to a class getter may be no site either. S* = expected to
// become proven sites; K* = KEEP (must stay unproven / Throws). P* = probes:
// is the accessor's EFFECT modelled by the solve today (setter body run with
// `value` bound, getter result flowing to the read)?
// Unknown values come from process.argv[2] / Math.random(), never `declare`.
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

type S = (u32, bool, crate::Verdict, Vec<String>);

fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<S> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key)
        .map(|s| (s.byte_offset, s.is_call, s.verdict.clone(), s.receivers.clone()))
        .collect()
}

/// Sites of `key` whose offset lies inside the first occurrence of `pat`.
fn at(pf: &crate::ProgramFacts, src: &str, key: &str, pat: &str) -> Vec<S> {
    let p = src.find(pat).unwrap_or_else(|| panic!("pattern {pat}")) as u32;
    let e = p + pat.len() as u32;
    sites(pf, key).into_iter().filter(|s| s.0 >= p && s.0 < e).collect()
}

fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}

fn show(tag: &str, src: &str, pf: &crate::ProgramFacts, keys: &[&str]) {
    for k in keys {
        for s in sites(pf, k) {
            let ctx: String = src.get(s.0 as usize..(s.0 as usize + 14).min(src.len())).unwrap_or("?").into();
            eprintln!("AS1 {tag} site .{k} @{} {:?} call={} verdict={:?} recv={:?}", s.0, ctx, s.1, s.2, s.3);
        }
    }
}

/// HIR shape around every mention of `"y"` (is `e.y` a PropertyGet/Set?).
fn show_hir(tag: &str, src: &str, key: &str) {
    let h = hirs(src);
    let s = format!("{:#?}", h[0].1);
    let lines: Vec<&str> = s.lines().collect();
    let pat = format!("\"{key}\"");
    for (i, l) in lines.iter().enumerate() {
        if l.contains(&pat) {
            let lo = i.saturating_sub(4);
            eprintln!("AS1 {tag} HIR {}", lines[lo..=i].iter().map(|x| x.trim()).collect::<Vec<_>>().join(" "));
        }
    }
}

// S1: plain data store on a proven mono receiver.
const S1: &str = "class V { x = 0; }
const v = new V();
v.x = Number(process.argv[2] ?? \"1\");
console.log(v.x);";
#[test]
fn accsite1_s1_data_store_mono() {
    let pf = facts(S1);
    show("S1", S1, &pf, &["x"]);
    show_hir("S1", S1, "x");
    let st = at(&pf, S1, "x", "v.x = ");
    assert!(st.len() == 1 && matches!(st[0].2, crate::Verdict::T0Mono), "S1 store v.x: {st:?}");
}

// S2: setter store, plain and compound; S3: getter read.
const S2: &str = "class E { _y = 0;
  set y(v) { this._y = v; this.cb(); }
  get y() { return this._y; }
  cb() {} }
const e = new E();
e.y = Number(process.argv[2] ?? \"1\");
e.y += 2;
console.log(e._y);";
#[test]
fn accsite1_s2_setter_store_exact() {
    let pf = facts(S2);
    show("S2", S2, &pf, &["y", "_y", "cb"]);
    show_hir("S2", S2, "y");
    let a = at(&pf, S2, "y", "e.y = ");
    let c = at(&pf, S2, "y", "e.y += ");
    assert!(a.len() == 1 && proven(&a[0].2), "S2 e.y = : {a:?}");
    // compound: the getter read and the setter store, both proven
    assert!(c.len() == 2 && c.iter().all(|s| proven(&s.2)), "S2 e.y += : {c:?}");
}

const S3: &str = "class G { g = 1; }
class E { _g = new G();
  get y() { return this._g; } }
const e = new E();
console.log(e.y.g);";
#[test]
fn accsite1_s3_getter_read_exact() {
    let pf = facts(S3);
    show("S3", S3, &pf, &["y", "g", "_g"]);
    show_hir("S3", S3, "y");
    let y = at(&pf, S3, "y", "e.y.g");
    assert!(y.len() == 1 && proven(&y[0].2), "S3 e.y read: {y:?}");
}

// S4: store on a receiver set of two classes, same field, same slot.
const S4: &str = "class A { x = 0; t = 1; }
class B { x = 0; u = 2; }
const o = Math.random() < 0.5 ? new A() : new B();
o.x = 5;
console.log(o.x);";
#[test]
fn accsite1_s4_data_store_set() {
    let pf = facts(S4);
    show("S4", S4, &pf, &["x"]);
    let st = at(&pf, S4, "x", "o.x = ");
    assert!(st.len() == 1 && matches!(st[0].2, crate::Verdict::T0Set), "S4 store o.x: {st:?}");
}

// S5: inherited setter (accessor on the base, receiver a subclass instance).
const S5: &str = "class B { _y = 0;
  set y(v) { this._y = v; }
  get y() { return this._y; } }
class D extends B { constructor() { super(); this.z = 1; } }
const d = new D();
d.y = Number(process.argv[2] ?? \"1\");
console.log(d.y);";
#[test]
fn accsite1_s5_inherited_setter() {
    let pf = facts(S5);
    show("S5", S5, &pf, &["y", "_y"]);
    let st = at(&pf, S5, "y", "d.y = ");
    let rd = at(&pf, S5, "y", "d.y);");
    assert!(st.len() == 1 && proven(&st[0].2), "S5 store d.y: {st:?}");
    assert!(rd.len() == 1 && proven(&rd[0].2), "S5 read d.y (getter): {rd:?}");
}

// S6: an own data property shadows the prototype accessor on ONE instance.
const S6: &str = "class E { _y = 0;
  set y(v) { this._y = v; }
  get y() { return this._y; } }
const a = new E();
const b = new E();
Object.defineProperty(a, 'y', { value: 1, writable: true });
const o = Math.random() < 0.5 ? a : b;
o.y = 3;
console.log(o.y);";
#[test]
fn accsite1_s6_own_data_shadows_accessor() {
    let pf = facts(S6);
    show("S6", S6, &pf, &["y", "_y"]);
    let st = at(&pf, S6, "y", "o.y = ");
    // a site exists, and it is not a single-kind (mono / exact-setter) claim
    assert!(
        st.len() == 1 && !matches!(st[0].2, crate::Verdict::T0Mono | crate::Verdict::T0Set),
        "S6 store o.y: {st:?}"
    );
}

// K1 KEEP: receiver from JSON.parse: unproven.
const K1: &str = "const o = JSON.parse(process.argv[2] ?? \"{}\");
o.y = 1;
console.log(o.y);";
#[test]
fn accsite1_k1_keep_json_receiver() {
    let pf = facts(K1);
    show("K1", K1, &pf, &["y"]);
    let st = at(&pf, K1, "y", "o.y = ");
    assert!(st.len() == 1 && !proven(&st[0].2), "K1 store must be a site, unproven: {st:?}");
}

// K2 KEEP: store to a getter-only accessor (10.1.9.2 step 7: [[Set]] returns
// false; 6.2.5.6 PutValue throws a TypeError only in strict code). A `.ts`
// file without import/export is a script (node agrees): its top level is
// sloppy, the store does nothing and nothing throws; a class body is strict
// code (11.2.2), and so is every part of a module.
const K2: &str = "class E { get y() { return 1; } }
const e = new E();
try { e.y = Number(process.argv[2] ?? \"2\"); } catch (x) { console.log(\"t\"); }
console.log(e.y);";
const K2B: &str = "class E { get y() { return 1; } }
class H { run(e: E) { try { e.y = Number(process.argv[2] ?? \"2\"); } catch (x) { console.log(\"t\"); } } }
const e = new E();
new H().run(e);
console.log(e.y);";
const K2C: &str = "export {};
class E { get y() { return 1; } }
const e = new E();
try { e.y = Number(process.argv[2] ?? \"2\"); } catch (x) { console.log(\"t\"); }
console.log(e.y);";
#[test]
fn accsite1_k2_keep_getter_only_throws() {
    let pf = facts(K2);
    show("K2", K2, &pf, &["y"]);
    let st = at(&pf, K2, "y", "e.y = ");
    assert!(
        st.len() == 1 && !proven(&st[0].2) && !matches!(st[0].2, crate::Verdict::Throws),
        "K2 sloppy script store: no Throws: {st:?}"
    );
    for (tag, src) in [("K2b", K2B), ("K2c", K2C)] {
        let pf = facts(src);
        show(tag, src, &pf, &["y"]);
        let st = at(&pf, src, "y", "e.y = ");
        assert!(st.len() == 1 && matches!(st[0].2, crate::Verdict::Throws), "{tag} strict store: {st:?}");
    }
}

// K3 KEEP: the setter may be replaced at run time: not "exactly setter E.y".
const K3: &str = "class W { w = 1; }
class E { _y = 0;
  set y(v) { this._y = v; } }
if (process.argv[2] === \"r\") Object.defineProperty(E.prototype, 'y', { set(v) { this.q = v; } });
const e = new E();
e.y = new W();
console.log((e as any).q);";
#[test]
fn accsite1_k3_keep_replaced_setter() {
    let pf = facts(K3);
    show("K3", K3, &pf, &["y", "q", "_y"]);
    let st = at(&pf, K3, "y", "e.y = ");
    assert!(st.len() == 1 && !proven(&st[0].2), "K3 store must be a site, not exact: {st:?}");
}

// K4 KEEP: a store of a key absent at birth adds a key: not a slot store.
const K4: &str = "class C { a = 0; }
const c: any = new C();
c.b = Number(process.argv[2] ?? \"1\");
console.log(c.b);";
#[test]
fn accsite1_k4_keep_adds_key() {
    let pf = facts(K4);
    show("K4", K4, &pf, &["b", "a"]);
    let st = at(&pf, K4, "b", "c.b = ");
    assert!(
        st.len() == 1 && !matches!(st[0].2, crate::Verdict::T0Mono | crate::Verdict::T0Set),
        "K4 store must be a site and not a slot store: {st:?}"
    );
}

// P1 probe: the setter's EFFECT. `e.y = new W()` runs `this._y = v`, so the
// later `e._y.w` has receiver W. Green = the setter body is modelled today.
const P1: &str = "class W { w = 1; }
class E { _y: any = 0;
  set y(v) { this._y = v; } }
const e = new E();
e.y = new W();
console.log(e._y.w);";
#[test]
fn accsite1_p1_setter_effect_modelled() {
    let pf = facts(P1);
    show("P1", P1, &pf, &["w", "_y", "y"]);
    let w = sites(&pf, "w");
    assert!(w.iter().any(|s| s.3.iter().any(|r| r.contains('W'))), "P1 setter effect missing: {w:?}");
}

// P2 probe: the getter's result flows to the read (`e.y.g` receiver G), and
// the compound form calls the getter: `e.y += 1` with the getter returning a
// G makes the setter's v a string/number, never G itself.
const P2: &str = "class G { g = 1; }
class E { _g: any = new G(); _s: any = 0;
  get y() { return this._g; }
  set y(v) { this._s = v; } }
const e = new E();
e.y += 1;
console.log(e.y.g, e._s.length);";
#[test]
fn accsite1_p2_getter_effect_modelled() {
    let pf = facts(P2);
    show("P2", P2, &pf, &["g", "y", "_s", "length"]);
    let g = sites(&pf, "g");
    assert!(g.iter().any(|s| s.3.iter().any(|r| r.contains('G'))), "P2 getter result missing: {g:?}");
}
