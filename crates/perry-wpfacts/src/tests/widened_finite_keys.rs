//! WIDEN9 (owner yes 11-26): past the work bound (widened) a call keeps
//! its run key when that key's count is fixed by the program text before
//! the solve: a program call or a summary template (`< n_prog`) times a
//! function token, kinds 0 at a program site, 7 and 9. Only a key built
//! from the solve's own products (a nested-site chain copy, a receiver or
//! trap context) still merges into the function's shared run. Before,
//! every new kind-0/1/7/8/9 key merged, so a past-the-bound solve lost the
//! kind-9 runs of SHARED-SITE-RUN (perry-web css/layout.ts layoutFlex
//! 850..867 on c09: work 1.112e9 for the bound 1e9).
use super::*;

/// The SHARED-SITE-RUN fixture (`shared_run_site.rs`): `store` and
/// `measure` run from a map callback inside `run`'s instance, so their
/// `edges()` calls are kind-9 runs of two templates.
const KIND9: &str = r#"import "unseen-host";
declare const ext: any;
type E = { t: number; l: number };
function edges(n: number): E { return { t: n, l: n + 1 }; }
class Box { m: E | null = null; }
function store(b: Box, n: number): number { b.m = edges(1); ext(b); return n > 0 ? measure(b, n - 1) : 0; }
function measure(b: Box, n: number): number { const e = edges(n); return n > 0 ? store(b, n - 1) + e.l : e.l; }
function run(xs: number[]): number[] {
  return xs.map((x) => store(new Box(), x));
}
const out = run(JSON.parse(process.argv[2] ?? "[1]"));
console.log(out.length);
"#;

/// Control: the same pair called from a program site, and both reach
/// `edges()` through one call in `edgesOf`. What tells the two `edgesOf`
/// runs apart is the nested-site chain (kind 0 keyed by a copy the solve
/// made, no pre-solve bound): past the bound those merge into the shared
/// run, whose one `edges()` template run (kind 9) both callers then share.
const CHAIN: &str = r#"import "unseen-host";
declare const ext: any;
type E = { t: number; l: number };
function edges(n: number): E { return { t: n, l: n + 1 }; }
function edgesOf(n: number): E { return n > 9 ? edgesOf(n - 1) : edges(n); }
class Box { m: E | null = null; }
function store(b: Box, n: number): number { b.m = edgesOf(1); ext(b); return n > 0 ? measure(b, n - 1) : 0; }
function measure(b: Box, n: number): number { const e = edgesOf(n); return n > 0 ? store(b, n - 1) + e.l : e.l; }
const out = store(new Box(), Number(process.argv[2] ?? "1"));
console.log(out);
"#;

fn run(src: &str, bound: u64) -> ProgramFacts {
    let mut cfg = FactsConfig::default();
    cfg.max_work = Some(bound);
    facts_with(src, cfg)
}

/// Is `e.l` in `measure` proven (its own literal, never escaped)?
fn el_proven(pf: &ProgramFacts) -> bool {
    let s = pf
        .sites
        .iter()
        .find(|s| s.key == "l" && !s.is_call)
        .unwrap_or_else(|| panic!("site .l in\n{}", pf.dump_text()));
    matches!(
        s.verdict,
        crate::derive::Verdict::T0Mono | crate::derive::Verdict::T0Set
    )
}

/// Every bound that widens the solve without stopping it, smallest first.
fn widened_bounds(src: &str) -> (ProgramFacts, Vec<u64>) {
    let full = run(src, 50_000_000);
    assert!(
        !full.stats.widened && !full.stats.aborted,
        "unbounded solve"
    );
    let mut v = Vec::new();
    for b in 1..=full.stats.work {
        let pf = run(src, b);
        if pf.stats.widened && !pf.stats.aborted {
            v.push(b);
        }
        if b > 64 && v.len() >= 8 {
            break;
        }
    }
    eprintln!("work {} widened bounds {v:?}", full.stats.work);
    assert!(!v.is_empty(), "some bound widens without a stop");
    (full, v)
}

/// RED before WIDEN9: past the bound the kind-9 runs of `store`'s and
/// `measure`'s `edges()` merged into edges' shared run, so `e.l` read the
/// literal `store` hands to `ext`. Now the two template runs stay apart.
#[test]
fn widened_solve_keeps_template_runs_apart() {
    let (full, bounds) = widened_bounds(KIND9);
    assert!(
        el_proven(&full),
        "unbounded: e.l proven\n{}",
        full.dump_text()
    );
    for b in bounds {
        let pf = run(KIND9, b);
        assert!(el_proven(&pf), "bound {b}: e.l proven\n{}", pf.dump_text());
    }
}

/// Control: a nested-site chain key (a copy the solve made) still merges
/// into the shared run past the bound, and so do the template runs below
/// it.
#[test]
fn widened_solve_still_merges_chain_runs() {
    let (full, bounds) = widened_bounds(CHAIN);
    assert!(
        el_proven(&full),
        "unbounded: e.l proven\n{}",
        full.dump_text()
    );
    let pf = run(CHAIN, bounds[0]);
    assert!(
        !el_proven(&pf),
        "bound {}: chain runs merge\n{}",
        bounds[0],
        pf.dump_text()
    );
}
