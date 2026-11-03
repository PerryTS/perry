//! DECISIONS 225: a function body contributes its births, effects and
//! escapes only once the function can run. `Host::enable` is never invoked:
//! the escape of `make` it would make never happens, so `make`'s only caller
//! is the module body and `base.mk` is proven. Every way the body can run
//! (a call, a closure, a getter, a field initializer, an escape to a global
//! or to unknown code, eval, `new Function`) keeps it refused. Per unit the
//! sites of bodies the whole program proves dead are exempt from the
//! per-unit <= whole residual, and only those.

use super::*;
use crate::SiteFacts;

const BASE: &str = r#"
function make(base: { mk(k: number): { k: number } }) { return (k: number) => base.mk(k); }
class Host {
  enable() {
    const load = (globalThis as any)["plug" + "in"];
    const o = { q: 1 };
    return load({ api: make, q: o.q });
  }
}
const f = make({ mk: (k) => ({ k }) });
const p = { r: 2 };
console.log(f(1).k, p.r);
"#;

/// Each way `Host::enable` can run, and the activation path that sees it.
const ARMS: &[(&str, &str, &str)] = &[
    ("direct", "console.log(new Host().enable());", "call"),
    (
        "closure",
        "function mkc() { return () => new Host().enable(); } const c = mkc(); console.log(c());",
        "call",
    ),
    (
        "getter",
        "class G { get x() { return new Host().enable(); } } console.log(new G().x);",
        "accessor",
    ),
    (
        "fieldinit",
        "class FI { x = new Host().enable(); } console.log(new FI().x);",
        "this",
    ),
    (
        "property",
        "(globalThis as any).box = { k: Host }; console.log(0);",
        "escape",
    ),
    (
        "unknownfn",
        "const g = (globalThis as any)[\"x\" + \"y\"]; if (g) g(Host); console.log(0);",
        "escape",
    ),
    (
        "eval",
        "function ev() { return eval(\"new Host().enable()\"); } console.log(ev());",
        "call",
    ),
    (
        "function",
        "const fn = new Function(\"H\", \"return new H().enable()\"); console.log(fn(Host));",
        "call",
    ),
];

fn proven(s: &SiteFacts) -> bool {
    !matches!(s.verdict, Verdict::No(_))
}

fn mk(pf: &ProgramFacts) -> Vec<&SiteFacts> {
    pf.sites
        .iter()
        .filter(|s| s.key == "mk" && s.is_call)
        .collect()
}

fn per_unit(src: &str) -> ProgramFacts {
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The per-unit <= whole site residual: what a view of `u` proves that no
/// view of `w` proves. A site `w`'s activation proved can never run is
/// exempt (any fact about it is vacuously true there); a site of an active
/// body never is.
pub(super) fn stronger_sites(u: &ProgramFacts, w: &ProgramFacts) -> Vec<String> {
    let mut out = Vec::new();
    for s in u.sites.iter().filter(|s| proven(s)) {
        let path = &u.module_paths[s.module as usize];
        let whole_proves = w.sites.iter().any(|x| {
            &w.module_paths[x.module as usize] == path
                && x.byte_offset == s.byte_offset
                && x.key == s.key
                && x.is_call == s.is_call
                && proven(x)
        });
        if !whole_proves && !w.dead_site(path, s) {
            out.push(format!(
                "{path}@{} .{} {:?}",
                s.byte_offset, s.key, s.verdict
            ));
        }
    }
    out
}

/// DECISIONS 225: `w`, the whole program with activation, keeps every
/// proof of the same run walking every body, but for sites of bodies it
/// proved dead; it loses no live proof and drops no live site.
pub(super) fn assert_only_dead_bodies_dropped(
    list: &[(String, &perry_hir::Module)],
    cfg: &FactsConfig,
    w: &ProgramFacts,
) {
    let all = crate::compute_act(list, cfg, None, None, false);
    for s in all.sites.iter().filter(|s| proven(s)) {
        let path = &all.module_paths[s.module as usize];
        let kept = w.sites.iter().any(|x| {
            &w.module_paths[x.module as usize] == path
                && x.byte_offset == s.byte_offset
                && x.key == s.key
                && x.is_call == s.is_call
                && proven(x)
        });
        assert!(
            kept || w.dead_site(path, s),
            "activation lost a live proof {path}@{} .{}\n{}",
            s.byte_offset,
            s.key,
            w.dump_text()
        );
    }
}

#[test]
fn an_uncalled_body_does_not_escape_what_it_names() {
    // Main-red: on main `enable`'s body escapes `make`, its parameter is ⊤.
    let pf = facts(BASE);
    let s = mk(&pf);
    assert!(
        !s.is_empty() && s.iter().all(|s| proven(s)),
        "{s:?}\n{}",
        pf.dump_text()
    );
    // The dead body's own site has no verdict (generic code).
    assert!(pf.sites.iter().all(|s| s.key != "q"), "{}", pf.dump_text());
}

#[test]
fn every_way_the_body_can_run_keeps_it_refused() {
    for (arm, tail, _) in ARMS {
        let pf = facts(&format!("{BASE}{tail}\n"));
        let s = mk(&pf);
        assert!(
            !s.is_empty() && s.iter().all(|s| !proven(s)),
            "arm {arm}: {s:?}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn sabotage_each_activation_path_proves_its_arm_wrongly() {
    // Each arm's path left out: the body that runs is not walked, so the
    // `mk` site is proven (or dropped as dead) although `make` escapes.
    for (arm, tail, path) in ARMS
        .iter()
        .filter(|a| ["direct", "getter", "fieldinit", "property"].contains(&a.0))
    {
        let cfg = FactsConfig {
            sabotage: Some(format!("activation:{path}")),
            ..FactsConfig::default()
        };
        let pf = facts_with(&format!("{BASE}{tail}\n"), cfg);
        assert!(
            mk(&pf).iter().all(|s| proven(s)),
            "arm {arm} sabotage {path}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn a_per_unit_proof_in_a_dead_body_is_not_stronger() {
    // Per unit every body is walked: `o.q` in `enable` is proven there, and
    // the whole program has no view of it (the body cannot run).
    let (u, w) = (per_unit(BASE), facts(BASE));
    let q: Vec<_> = u
        .sites
        .iter()
        .filter(|s| s.key == "q" && proven(s))
        .collect();
    assert!(!q.is_empty(), "{}", u.dump_text());
    assert!(q.iter().all(|s| w.dead_site("t.ts", s)));
    let s = stronger_sites(&u, &w);
    assert!(s.is_empty(), "{s:?}");
}

#[test]
fn a_per_unit_proof_in_an_active_body_stronger_than_whole_is_red() {
    // `p.r` is in the module body (active). A whole run that does not prove
    // it, or has no view of it, makes the per-unit proof stronger: red.
    let (u, mut w) = (per_unit(BASE), facts(BASE));
    assert!(
        u.sites.iter().any(|s| s.key == "r" && proven(s)),
        "{}",
        u.dump_text()
    );
    assert!(stronger_sites(&u, &w).is_empty());
    for s in w.sites.iter_mut().filter(|s| s.key == "r") {
        s.verdict = Verdict::No("planted".into());
    }
    let red = stronger_sites(&u, &w);
    assert!(red.iter().any(|s| s.contains(".r ")), "{red:?}");
    w.sites.retain(|s| s.key != "r");
    let red = stronger_sites(&u, &w);
    assert!(red.iter().any(|s| s.contains(".r ")), "{red:?}");
}
