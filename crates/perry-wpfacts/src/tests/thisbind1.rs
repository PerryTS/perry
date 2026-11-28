// THISBIND1 (2026-11-16): `this` of a method body holds exactly the
// receivers whose own lookup of the call's key can yield that body
// (ECMA-262 13.3.6 EvaluateCall: `this` is the receiver of THIS execution;
// 10.1.8.1 OrdinaryGet: the callee is that receiver's value of the key;
// 13.3.7: `super.m()` passes the current `this`). The binding is per
// receiver token (`bind_via`, solve/this_bind.rs): these fixtures pin it. Small
// JS, no three.js.
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = [(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The verdicts of the plain reads of `key` in a.ts, in source order.
fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<crate::Verdict> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let mut v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.is_call && !s.access.is_store())
        .map(|s| (s.byte_offset, s.verdict.clone()))
        .collect();
    v.sort_by_key(|x| x.0);
    v.into_iter().map(|x| x.1).collect()
}

fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}

const AB: &str = "class A { a: number; constructor() { this.a = 1; } m() { return this.a; } n() { return this.a + 1; } }
class B { b: number; constructor() { this.b = 2; } m() { return this.b; } }
const pick = (process.argv[2] ?? \"a\").length;
";

/// T1: one site `x.m()`, x = A or B: A.m's `this` is only the A instance,
/// B.m's only the B instance: both reads proven.
#[test]
fn thisbind1_t1_two_classes_one_site() {
    let src = format!(
        "{AB}const x: any = pick > 1 ? new A() : new B();
console.log(x.m());"
    );
    let pf = facts(&src);
    let (a, b) = (reads(&pf, "a"), reads(&pf, "b"));
    eprintln!("TB1 t1 a={a:?} b={b:?}");
    assert!(proven(&a[0]), "this.a in A.m: {a:?}");
    assert!(proven(&b[0]), "this.b in B.m: {b:?}");
}

/// T2 KEEP: a base-class method run on instances of two subclasses: both
/// really are `this` (a union), the base field is definite on both.
#[test]
fn thisbind1_t2_base_method_on_subclasses() {
    let src = "class Base { k: number; constructor() { this.k = 3; } m() { return this.k; } }
class S1 extends Base { s1 = 1; }
class S2 extends Base { s2 = 2; }
const pick = (process.argv[2] ?? \"a\").length;
const x: any = pick > 1 ? new S1() : new S2();
console.log(x.m());";
    let v = reads(&facts(src), "k");
    eprintln!("TB1 t2 {v:?}");
    assert!(proven(&v[0]), "this.k in Base.m over S1|S2: {v:?}");
}

/// T3 KEEP: `C.prototype.m.call(C.prototype)`: the prototype really is
/// `this`, it has no own `k`: the read is no plain proven slot (on dd74a341
/// it is E0: proven only in the typed body behind the entry guard).
#[test]
fn thisbind1_t3_prototype_as_explicit_this_keeps() {
    let src = "class C { k: number; constructor() { this.k = 4; } m() { return this.k; } }
const c = new C();
console.log(c.m());
if (String(Math.random()) === \"x\") console.log((C.prototype as any).m.call(C.prototype));";
    let v = reads(&facts(src), "k");
    eprintln!("TB1 t3 {v:?}");
    assert!(
        matches!(v[0], crate::Verdict::E0) || !proven(&v[0]),
        "Proto(C) is this of C.m: {v:?}"
    );
}

/// T3b: the same class without that call: the prototype is no `this`.
#[test]
fn thisbind1_t3b_no_prototype_receiver() {
    let src = "class C { k: number; constructor() { this.k = 4; } m() { return this.k; } }
const c = new C();
console.log(c.m());
console.log(typeof (C.prototype as any).m);";
    let v = reads(&facts(src), "k");
    eprintln!("TB1 t3b {v:?}");
    assert!(proven(&v[0]), "only the C instance is this of C.m: {v:?}");
}

/// T4 KEEP: `const f = a.m; f.call(b)`: the B instance is `this` of A.m
/// (no `a`: the read is own data or absent, or generic).
#[test]
fn thisbind1_t4_function_value_call_keeps() {
    let src = format!(
        "{AB}const a = new A(); const b = new B();
const f = a.m;
console.log(a.m());
if (pick > 1) console.log(f.call(b));"
    );
    let v = reads(&facts(&src), "a");
    eprintln!("TB1 t4 {v:?}");
    assert!(
        matches!(v[0], crate::Verdict::OwnDataOrAbsent) || !proven(&v[0]),
        "B is this of A.m through f.call: {v:?}"
    );
}

/// T5: `x[k]()` with k from argv: each receiver binds only the bodies its
/// own lookup of a possible key yields (A: m or n; B: m).
#[test]
fn thisbind1_t5_computed_key_call() {
    let src = format!(
        "{AB}const x: any = pick > 1 ? new A() : new B();
const k = pick > 2 ? \"m\" : \"n\";
console.log(x[k]());"
    );
    let pf = facts(&src);
    let (a, b) = (reads(&pf, "a"), reads(&pf, "b"));
    eprintln!("TB1 t5 a={a:?} b={b:?}");
    assert!(
        proven(&a[0]) && proven(&a[1]),
        "A.m / A.n see only A: {a:?}"
    );
    assert!(proven(&b[0]), "B.m sees only B: {b:?}");
}

/// T6 KEEP: `super.m()` passes the subclass instance: it is `this` of
/// A.m and has `a` from A's constructor.
#[test]
fn thisbind1_t6_super_call_passes_this() {
    let src = format!(
        "{AB}class S extends A {{ m() {{ return super.m() + 1; }} }}
const x: any = pick > 1 ? new S() : new B();
console.log(x.m());"
    );
    let v = reads(&facts(&src), "a");
    eprintln!("TB1 t6 {v:?}");
    assert!(proven(&v[0]), "S instance is this of A.m via super: {v:?}");
}
