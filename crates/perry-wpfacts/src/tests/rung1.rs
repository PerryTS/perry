//! RUNG1 (uncommitted): `Object.defineProperty(this, "id", { value })` in a
//! class constructor, a data descriptor under a NAMED key (rule D1: it
//! creates that key alone, ECMA-262 20.1.2.4 -> 7.3.8 -> 10.1.6.3), marks
//! the class instance HARD, and every read of every other birth key of every
//! instance of the class (and its subclasses) goes unproven. three.js:
//! Object3D, Material, Texture, BufferGeometry, Source all do this.
use super::*;
fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}
fn reads(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites.iter().filter(|s| s.key == key && !s.is_call).map(|s| s.verdict.clone()).collect()
}
fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let v = reads(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}
fn open(pf: &ProgramFacts, key: &str) -> bool {
    let v = reads(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}
fn src(define: &str) -> String {
    format!(
        "let _id = 0;
         class Base {{
             geometry: number; zoom: number;
             constructor() {{ {define} this.geometry = 1; this.zoom = 2; }}
         }}
         class Cam extends Base {{
             right: number;
             constructor() {{ super(); this.right = 3; }}
             upd(): number {{ return (this.right - this.geometry) / this.zoom; }}
         }}
         const c = new Cam();
         console.log(c.upd());"
    )
}
/// RED: the named data define writes `id` alone; `right`, `geometry` and
/// `zoom` are birth fields of one fixed layout either way.
#[test]
fn rung1_named_data_define_keeps_other_birth_keys_proven() {
    let pf = whole(&src("Object.defineProperty(this, \"id\", { value: _id++ });"));
    for k in ["right", "geometry", "zoom"] {
        assert!(proven(&pf, k), "{k}: {:?}", reads(&pf, k));
    }
}
/// Control (green): without the define the same reads are proven, so the
/// harness can see a proof.
#[test]
fn rung1_control_no_define_is_proven() {
    let pf = whole(&src(""));
    for k in ["right", "geometry", "zoom"] {
        assert!(proven(&pf, k), "{k}: {:?}", reads(&pf, k));
    }
}
/// Control (green, must stay green): an accessor descriptor under the key
/// read by the method installs a getter there: that read stays open.
#[test]
fn rung1_control_accessor_define_stays_open() {
    let s = src("Object.defineProperty(this, \"right\", { get() { return (globalThis as any)[process.argv[2]]; }, configurable: true });")
        .replace("this.right = 3;", "");
    let pf = whole(&s);
    assert!(open(&pf, "right"), "{:?}", reads(&pf, "right"));
}
/// Control (ii): an unknown computed key may be any key of the instance,
/// and the descriptor may make it non-writable: the other keys are NOT
/// claimed.
#[test]
fn rung1_control_computed_key_define_keeps_others_open() {
    let pf = whole(&src("Object.defineProperty(this, process.argv[2], { value: 1 });"));
    for k in ["right", "geometry", "zoom"] {
        assert!(!proven(&pf, k), "{k}: {:?}", reads(&pf, k));
    }
}
/// Control (iii): a data define of an EXISTING birth key with
/// `writable: false`, then a store to that key elsewhere. The store fails
/// (strict [[Set]] throws, ECMA-262 10.1.9.2 step 2.a); the solver does not
/// model that failure, so the key's sites stay open; the other keys stay
/// proven.
#[test]
fn rung1_existing_key_nonwritable_define_key_open_others_proven() {
    let s = src("")
        .replace("this.zoom = 2; }", "this.zoom = 2; Object.defineProperty(this, \"zoom\", { value: 5, writable: false }); }")
        .replace("const c = new Cam();", "const c = new Cam(); try { (c as any).zoom = 7; } catch (e) {}");
    let pf = whole(&s);
    assert!(!proven(&pf, "zoom"), "zoom: {:?}", reads(&pf, "zoom"));
    for k in ["right", "geometry"] {
        assert!(proven(&pf, k), "{k}: {:?}", reads(&pf, k));
    }
}
/// Control (iv): a descriptor object built elsewhere that may hold `get`:
/// the read of the defined key stays open.
#[test]
fn rung1_control_unknown_descriptor_may_hold_get_stays_open() {
    let s = src("const d: any = process.argv[2] ? { get() { return (globalThis as any)[process.argv[3]]; } } : { value: 3 }; Object.defineProperty(this, \"right\", d);")
        .replace("this.right = 3;", "");
    let pf = whole(&s);
    assert!(!proven(&pf, "right"), "{:?}", reads(&pf, "right"));
}
/// Probe (second group, r2-r7 `toJSON`/`copy`): `super.m()` calls the
/// parent method with the CURRENT `this` (ECMA-262 13.3.7.1 SuperProperty,
/// MakeSuperPropertyReference(actualThis)); the parent body's `this.v`
/// reads the instance, never a prototype.
#[test]
fn rung1_super_call_this_is_the_instance() {
    let pf = whole(
        "class A { v: number; constructor() { this.v = 1; } m(): number { return this.v; } }
         class B extends A { w: number; constructor() { super(); this.w = 2; } m(): number { return super.m() + this.w; } }
         console.log(new B().m());",
    );
    assert!(proven(&pf, "v"), "v: {:?}", reads(&pf, "v"));
}
