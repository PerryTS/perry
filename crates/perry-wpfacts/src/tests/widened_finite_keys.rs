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

/// The KIND9 fixture's solved model, unbounded.
fn solved(src: &str) -> crate::model::Model {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

/// A closure token of `g` in a context of lexical context `lay` and value
/// chain `vc`, as `closure_tok` makes one.
fn ctx_tok(m: &mut crate::model::Model, g: crate::model::FnIdx, lay: u32, vc: u32) -> crate::model::Tok {
    let c = m.sum.ctx_nodes.len() as crate::model::CallId;
    m.sum.ctx_nodes.push(Vec::new());
    m.sum.ctx_lay.push(lay);
    m.sum.ctx_vc.push(vc);
    m.tok(crate::model::TokKind::Func(g, c))
}

/// NXMEM2 (nextjs app-page-turbo `U`: 444 kind-9 runs at one template,
/// one per closure token the solve made a context for): a per-site key is
/// kept past the bound only when every part of it is bounded by program
/// text, and a receiver-layout key never is. A
/// kind-9 key at a template with a closure token made in a context of a
/// call the solve made (a nested-site chain copy), or under a value chain
/// holding a string the solve made, merges into the function's shared
/// run; the same key with a token made in a program site's context stays.
#[test]
fn widened_key_merges_a_token_the_solve_made_a_context_for() {
    use crate::summary::{LAY_NONE, LAY_SITE, NO_VC};
    let mut m = solved(KIND9);
    let n_prog = m.sum.n_prog as u32;
    let (site, ft) = m
        .sum
        .inst_key
        .keys()
        .find(|k| k.0 == 9)
        .map(|k| (k.1, k.2))
        .expect("a kind-9 run");
    assert!(site < n_prog, "a kind-9 run is keyed by a template");
    // Every key the unbounded solve made is text-bounded.
    let keys: Vec<_> = m.sum.inst_key.keys().copied().collect();
    for k in keys.iter().filter(|k| matches!(k.0, 0 | 7 | 9) && k.1 < n_prog) {
        assert!(m.text_key((k.0, k.1, k.2)), "text key {k:?}");
    }
    let g = m.tok_fn(ft);
    // A program site's context: kept.
    let text = ctx_tok(&mut m, g, LAY_SITE | site, NO_VC);
    assert_eq!(m.widened_key((9, site, text)), (9, site, text));
    // A context of a call the solve made: merged.
    let made = ctx_tok(&mut m, g, LAY_SITE | (n_prog + 1), NO_VC);
    assert_eq!(m.widened_key((9, site, made)), (1, LAY_NONE, made));
    // A context under a chain keyed by a string the solve made: merged.
    let atom = m.sum.n_prog_atoms as u32 + 7;
    let vc = m.sum.vctx.len() as u32;
    m.sum.vctx.push((NO_VC, g, atom));
    let made_val = ctx_tok(&mut m, g, LAY_SITE | site, vc);
    assert_eq!(m.widened_key((9, site, made_val)), (1, LAY_NONE, made_val));
    // The part decides, not the kind: the same token keeps a rule-C key
    // apart no more than a kind-9 one, nor does a chain-copy site.
    assert!(!m.text_key((7, site, made)));
    assert!(!m.text_key((0, n_prog + 1, text)));
    // A receiver layout is a set the solve computes: kind 1 merges.
    let lay = m.layout_of(ft);
    assert_eq!(m.widened_key((1, lay, text)), (1, LAY_NONE, text));
}
