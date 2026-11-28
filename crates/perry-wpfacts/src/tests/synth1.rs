// SYNTH1 (2026-11-28): the entry guard's receiver (design §11.4) is a
// class's template-wide token, which has no prototype node of its own: its
// chain is that of the class's instances.
use super::*;
use perry_hir::Module;

fn facts_at(path: &str, src: &str) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(src, path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", path).expect("lowers");
    let h = vec![(path.to_string(), hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn calls(pf: &crate::ProgramFacts, key: &str) -> Vec<crate::Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && !s.access.is_store())
        .map(|s| s.verdict.clone())
        .collect()
}

// `this.h()` in C.m runs on C and on D instances; D's layout is hard (a
// delete), so the site is generic. Behind the guard class_id(this) == C the
// receivers are C's instances, whose chain holds `h` on C.prototype: E0.
const GUARDED: &str = r#"
class C { x = 1; m(): number { return this.h(); } h(): number { return 2; } }
class D extends C { y = 2; }
const c = new C();
const d = new D();
delete (d as any).y;
console.log(c.m() + d.m());
"#;

#[test]
fn synth1_guard_chain_of_instances() {
    let pf = facts_at("synth1_a.ts", GUARDED);
    let v = calls(&pf, "h");
    assert_eq!(v, vec![crate::Verdict::E0], "{v:?}");
}

// The same, but one C instance may own `h` (a computed-key store): the
// guarded receivers may hold it own, so the site stays generic.
const OWNED: &str = r#"
class C { x = 1; m(): number { return this.h(); } h(): number { return 2; } }
class D extends C { y = 2; }
const c = new C();
const d = new D();
delete (d as any).y;
const k = process.argv.length > 9 ? "h" : "z";
(c as any)[k] = () => 3;
console.log(c.m() + d.m());
"#;

#[test]
fn synth1_guard_instance_may_own_key() {
    let pf = facts_at("synth1_b.ts", OWNED);
    let v = calls(&pf, "h");
    assert!(
        v.iter().all(|x| matches!(x, crate::Verdict::No(_))),
        "{v:?}"
    );
}
