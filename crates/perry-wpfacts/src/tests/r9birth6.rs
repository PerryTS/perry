// R9BIRTH6 (2026-11-16): module-initialization order (ECMA-262 16.2.1.5.3
// InnerModuleEvaluation; class declarations evaluate at their statement
// position, 15.7.14). A top-level store `X.prototype.k = v` has run only
// after every statement before it. A read or call site that can execute
// before the store completes must cover the absent outcome (`undefined`;
// a call throws), so it is never a single prototype constant.
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

fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RB6 .{key} verdict={verdict}");
    }
    v
}

fn none_mono(pf: &crate::ProgramFacts, key: &str) -> bool {
    let r = reads(pf, key);
    !r.is_empty() && r.iter().all(|v| !v.starts_with("T0"))
}

// P1 (RED at rb5): the call runs once before the store (throws) and once
// after: never T0Mono.
#[test]
fn r9birth6_p1_call_before_store() {
    let pf = facts(
        "class P { run() { return (this as any).m(); } }
const p = new P();
let r: any;
try { r = p.run(); } catch (e) { r = 'threw'; }
(P.prototype as any).m = function () { return 1; };
console.log(r, p.run());
",
    );
    assert!(
        none_mono(&pf, "m"),
        "P1 this.m() proven although it runs before the store"
    );
}

// P2 (RED at rb5): the read form.
#[test]
fn r9birth6_p2_read_before_store() {
    let pf = facts(
        "class P { get() { return (this as any).k; } }
const p = new P();
const a = p.get();
(P.prototype as any).k = 1;
console.log(a, p.get());
",
    );
    assert!(
        none_mono(&pf, "k"),
        "P2 this.k proven although read before the store"
    );
}

/// The holder class the facts name for the call sites of `key` (codegen's
/// direct-call input on the ONCHANGE line).
fn verdict_and_holder(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.is_call)
        .map(|s| {
            format!(
                "{:?} targets={:?} recv={:?} off={}",
                s.verdict, s.target_fns, s.receivers, s.byte_offset
            )
        })
        .collect();
    for x in &v {
        eprintln!("RB6 call .{key} {x}");
    }
    v
}

// P3 (soundness, the borrowed class-body method): `B.prototype.m =
// A.prototype.m` makes A's class-body method B's member; a call through a
// B instance before that store throws. On a head that lowers a proven call
// to a direct call to the holder's class-body method this verdict would
// reach codegen, so it must not be T0Mono.
#[test]
fn r9birth6_p3_borrowed_class_method_before_store() {
    let pf = facts(
        "class A { m() { return 1; } }
class B { run() { return (this as any).m(); } }
const b = new B();
let r: any;
try { r = b.run(); } catch (e) { r = 'threw'; }
(B.prototype as any).m = A.prototype.m;
console.log(r, b.run(), new A().m());
",
    );
    let _ = verdict_and_holder(&pf, "m");
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let bad = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == "m" && s.is_call)
        .any(|s| {
            format!("{:?}", s.verdict).starts_with("T0")
                && s.receivers.iter().any(|r| r.contains('B'))
        });
    assert!(
        !bad,
        "P3 b.run()'s this.m() proven although it runs before the store"
    );
}

fn src_k(between: &str, tail: &str) -> String {
    format!(
        "class K {{ f() {{ return (this as any).T; }} g() {{ return (this as any).V.A; }} }}
{between}
(K.prototype as any).T = Float32Array;
(K.prototype as any).V = {{ A: 0, B: 1 }};
{tail}
const k = new K();
console.log(k.f() === Float32Array, k.g(), process.argv[2]);
"
    )
}

// G1 (RED at rb5): class, then directly the stores: the reads are the
// stored values.
#[test]
fn r9birth6_g1_store_right_after_class() {
    let pf = facts(&src_k("", ""));
    let t = reads(&pf, "T");
    assert!(
        !t.is_empty() && t.iter().all(|v| v == "T0Mono"),
        "G1 this.T not T0Mono"
    );
    let v = reads(&pf, "V");
    assert!(
        !v.is_empty() && v.iter().all(|v| v == "T0Mono"),
        "G1 this.V not T0Mono"
    );
}

// k1 (KEEP): an unknown call between the class and the store.
#[test]
fn r9birth6_k1_unknown_call_between() {
    let pf = facts(&src_k(
        "const fns: any[] = [String(Math.random())]; console.log(fns[0]);",
        "",
    ));
    assert!(
        none_mono(&pf, "T"),
        "k1 proven although code runs between class and store"
    );
}

// k2 (KEEP): `new K()` at top level before the store.
#[test]
fn r9birth6_k2_new_before_store() {
    let pf = facts(&src_k("const early = new K(); console.log(early.f());", ""));
    assert!(
        none_mono(&pf, "T"),
        "k2 proven although an instance reads before the store"
    );
}

// k3 (KEEP): `delete` of the stored key later.
#[test]
fn r9birth6_k3_delete() {
    let pf = facts(&src_k(
        "",
        "if (process.argv[2] === 'x') delete (K.prototype as any).T;",
    ));
    assert!(
        none_mono(&pf, "T"),
        "k3 proven although the key may be deleted"
    );
}

// k4 (KEEP): a second store, anywhere: the holder stays the one settled
// prototype slot (T0Mono names the holder; its field holds both values).
#[test]
fn r9birth6_k4_second_store() {
    let pf = facts(&src_k(
        "",
        "if (process.argv[2] === 'x') (K.prototype as any).T = Float64Array;",
    ));
    assert!(
        reads(&pf, "T").iter().all(|v| v == "T0Mono"),
        "k4 holder lost by a later store"
    );
}

// k5 (KEEP): a subclass's own store after an instance of it read: the
// subclass's store is not settled: never its value alone.
#[test]
fn r9birth6_k5_subclass_store_late() {
    let pf = facts(&src_k(
        "",
        "class S extends K {}\nconst s0 = new S(); console.log(s0.f());\n(S.prototype as any).T = Float64Array;\nconsole.log(new S().f());",
    ));
    assert!(
        reads(&pf, "T").iter().all(|v| v != "T0Mono"),
        "k5 subclass read proven one value"
    );
}
