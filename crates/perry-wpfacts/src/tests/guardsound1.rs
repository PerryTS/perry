// GUARDSOUND1 (2026-11-28): the entry guard `class_id(this) == C` (design
// §11.4) admits every instance of exactly C. A delete on one of them keeps
// its class id while it drops the slot, so the guarded judgment takes the
// marks of C's instances, not the template-wide token's alone.
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

fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<crate::Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && !s.access.is_store() && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

// `this.k` in C.m runs on c, c2 and d; d is hard (a delete), so the site is
// refused and judged again behind the guard. c2, an instance of exactly C,
// lost `k`: the guard admits it, so the read is no T0 over the layout of C.
// node prints `1 undefined 1`.
const HARD_INSTANCE: &str = r#"
class C { k = 1; m(): any { return this.k; } }
class D extends C { y = 2; }
const c = new C();
const c2 = new C();
const d = new D();
delete (d as any).y;
delete (c2 as any).k;
console.log(c.m(), c2.m(), d.m());
"#;

#[test]
fn guardsound1_hard_instance_of_exactly_c() {
    let pf = facts_at("guardsound1_a.ts", HARD_INSTANCE);
    let v = reads(&pf, "k");
    assert!(!v.is_empty(), "{}", pf.dump_text());
    assert!(
        v.iter().all(|x| matches!(x, crate::Verdict::No(_))),
        "{v:?}"
    );
}

// The control: no instance of exactly C is hard, only d: behind the guard
// the read is T0 over the layout of C (E0).
const SOFT_INSTANCES: &str = r#"
class C { k = 1; m(): any { return this.k; } }
class D extends C { y = 2; }
const c = new C();
const c2 = new C();
const d = new D();
delete (d as any).y;
console.log(c.m(), c2.m(), d.m());
"#;

#[test]
fn guardsound1_soft_instances_stay_e0() {
    let pf = facts_at("guardsound1_b.ts", SOFT_INSTANCES);
    let v = reads(&pf, "k");
    assert_eq!(v, vec![crate::Verdict::E0], "{v:?}");
}
