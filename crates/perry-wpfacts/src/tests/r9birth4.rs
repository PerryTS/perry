// R9BIRTH4 (2026-11-15): known-object stores. A named store whose
// receiver set is ONE program object (a class's prototype object or its
// class constructor) and whose effect ECMA-262 10.1.9.2 OrdinarySet
// decides exactly: an own data property is added or overwritten (no
// accessor of the key on the receiver or its chain, no builtin member of
// the key, the object never frozen / sealed / non-extensible, never given
// to unknown code). Owner 2026-11-15: counted proven in its own column;
// the emitted store stays the generic one.
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

/// The verdicts (Debug text) and receivers of the store sites of `key`.
fn stores(pf: &crate::ProgramFacts, key: &str) -> Vec<(String, Vec<String>)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.access.is_store())
        .map(|s| (format!("{:?}", s.verdict), s.receivers.clone()))
        .collect();
    for (verdict, r) in &v {
        eprintln!("RB4 .{key} store verdict={verdict} receivers={r:?}");
    }
    v
}

/// Every store of `key` is / no store of `key` is a known-object store.
fn known(pf: &crate::ProgramFacts, key: &str, want: bool) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|(v, _)| (v == "KnownObjectStore") == want)
}

const CLASSES: &str = "class A { constructor(v: any) { this.v = v; } get2() { return this.v; } }
class B extends A { constructor(v: any) { super(v); this.w = 1; } }
";

const USE: &str = "const a = new A(process.argv[2] ?? \"1\"); const b = new B(String(Math.random()));
console.log(a.get2(), b.get2(), (a as any).k, (b as any).k, (A as any).K, (B as any).K);
";

fn src(mid: &str) -> String {
    format!("{CLASSES}{mid}{USE}")
}

// P1: a store onto a base class's prototype object.
#[test]
fn r9birth4_p1_base_prototype() {
    let pf = facts(&src("(A.prototype as any).k = 'a';\n"));
    assert!(known(&pf, "k", true), "P1 A.prototype.k not a known-object store");
}

// P2: a subclass's prototype object: `B.prototype` reads B's own
// non-writable, non-configurable `prototype` (15.7.14 ClassDefinitionEvaluation,
// MakeConstructor(F, false, proto)), never A's.
#[test]
fn r9birth4_p2_subclass_prototype() {
    let pf = facts(&src("(B.prototype as any).k = 'b';\n"));
    let s = stores(&pf, "k");
    assert!(s.iter().all(|(_, r)| r.len() == 1), "P2 receivers not one object: {s:?}");
    assert!(known(&pf, "k", true), "P2 B.prototype.k not a known-object store");
}

// S1: a class static store `A.K = v`.
#[test]
fn r9birth4_s1_static() {
    let pf = facts(&src("(A as any).K = 5;\n"));
    assert!(known(&pf, "K", true), "S1 A.K not a known-object store");
}

// S2: a subclass static: the chain is A's constructor, Function.prototype,
// Object.prototype.
#[test]
fn r9birth4_s2_subclass_static() {
    let pf = facts(&src("(B as any).K = 6;\n"));
    assert!(known(&pf, "K", true), "S2 B.K not a known-object store");
}

// KEEP k1: a base class with a static setter `K`: the store calls it.
#[test]
fn r9birth4_k1_static_setter_on_parent() {
    let s = "class P { static set K(x: any) { console.log(\"set\", x); } }
class A extends P { constructor(v: any) { super(); this.v = v; } get2() { return this.v; } }
class B extends A { constructor(v: any) { super(v); this.w = 1; } }
(B as any).K = 6;
"
    .to_string()
        + USE;
    let pf = facts(&s);
    assert!(known(&pf, "K", false), "k1 store through a static setter proven");
}

// KEEP k2: a prototype setter `k` on the parent class.
#[test]
fn r9birth4_k2_prototype_setter_on_parent() {
    let s = "class A { constructor(v: any) { this.v = v; } get2() { return this.v; }
  set k(x: any) { console.log(\"set\", x); } get k() { return 1; } }
class B extends A { constructor(v: any) { super(v); this.w = 1; } }
(B.prototype as any).k = 'b';
"
    .to_string()
        + USE;
    let pf = facts(&s);
    assert!(known(&pf, "k", false), "k2 store through a prototype setter proven");
}

// KEEP k3: `Object.freeze(A.prototype)` anywhere in the program.
#[test]
fn r9birth4_k3_frozen_prototype() {
    let pf = facts(&src(
        "if (process.argv[3] === \"f\") Object.freeze(A.prototype);\n(A.prototype as any).k = 'a';\n",
    ));
    assert!(known(&pf, "k", false), "k3 store onto a freezable prototype proven");
}

// KEEP k4: the receiver is one of two classes, chosen by argv.
#[test]
fn r9birth4_k4_two_receivers() {
    let pf = facts(&src(
        "class C { constructor() {} }\nconst X: any = process.argv[2] === \"c\" ? C : A;\nX.prototype.k = 'x';\nconsole.log((new C() as any).k);\n",
    ));
    assert!(known(&pf, "k", false), "k4 store onto one of two prototypes proven");
}

// KEEP k5: `name` of a class constructor is a non-writable own data
// property (10.2.9 SetFunctionName): the store fails.
#[test]
fn r9birth4_k5_constructor_name() {
    let pf = facts(&src("(A as any).name = 'z';\nconsole.log(A.name);\n"));
    assert!(known(&pf, "name", false), "k5 store of a class's `name` proven");
}

// KEEP k6: a non-writable data property defined on the prototype.
#[test]
fn r9birth4_k6_defined_non_writable() {
    let pf = facts(&src(
        "Object.defineProperty(A.prototype, \"k\", { value: 1, writable: false });\n(A.prototype as any).k = 'a';\n",
    ));
    assert!(known(&pf, "k", false), "k6 store onto a non-writable key proven");
}

// Rule K for literals (R9BIRTH4 item 2): `A`'s constructor stores an
// object literal whose getters read `this` through an alias into
// `this.stats`, then stores `b`. `MID` runs between the two.
fn k_src(stats: &str, mid: &str) -> String {
    format!(
        "class K {{ constructor(v: any) {{ this.a = v; const scope = this; {stats} {mid} this.b = 2; }}
  sum() {{ return this.a + this.b; }} }}
function reg(x: any) {{ return x.total; }}
const k = new K(Number(process.argv[2] ?? \"1\"));
console.log(k.sum(), (k as any).stats.x.total);
"
    )
}

const K_STATS: &str = "this.stats = { x: { get total() { return scope.a; }, get inUse() { return 1; } }, y: 0 };";

/// Every store of `key` is a slot store (T0) / none is.
fn slot_stores(pf: &crate::ProgramFacts, key: &str, want: bool) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|(v, _)| v.starts_with("T0") == want)
}

// KL1: the literal of accessors over `this` is no leak by itself.
#[test]
fn r9birth4_kl1_accessor_literal_store() {
    let pf = facts(&k_src(K_STATS, ""));
    assert!(slot_stores(&pf, "stats", true), "KL1 this.stats not a slot store");
    assert!(slot_stores(&pf, "b", true), "KL1 this.b after the literal not a slot store");
}

// KEEP kl2: the literal is handed to other code before the store.
#[test]
fn r9birth4_kl2_literal_passed_out_first() {
    let pf = facts(&k_src(
        "const s = { get total() { return scope.a; } }; reg(s); this.stats = { x: s };",
        "",
    ));
    assert!(slot_stores(&pf, "b", false), "kl2 this.b proven after the literal ran");
}

// KEEP kl3: the constructor reads `this.stats` (its getter sees `this`).
#[test]
fn r9birth4_kl3_stats_read_in_ctor() {
    let pf = facts(&k_src(K_STATS, "console.log(this.stats.x.total);"));
    assert!(slot_stores(&pf, "b", false), "kl3 this.b proven after this.stats was read");
}
