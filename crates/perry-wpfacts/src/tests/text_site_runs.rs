//! Rules C and E2 run a function once per call site of the program text.
//! A call the solver makes (`apply`, `call`, a callback) has no text of its
//! own: its site is the site of the call its chain started at
//! (`Model::text_site`). A wrapper that replaced a method and calls the
//! one it replaced through `orig.apply(this, args)` reaches itself (the
//! field holds both functions); keyed by the made call's own id, every
//! level of that recursion was a new run without end (next.js
//! `node-crypto.js`, `randomBytes`). The bound below is the test's, computed from the
//! program; the solver has none.
use super::*;

fn facts(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(20_000_000),
        ..FactsConfig::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// A run per (site of the text, function value, receiver layout): `(`
/// counts the program's calls from above, `function` and `=>` its
/// function values.
fn check(src: &str) {
    let pf = facts(src);
    assert!(!pf.stats.aborted, "the solve finishes: {:?}", pf.stats);
    assert!(
        !pf.stats.widened,
        "the solve is not widened: {:?}",
        pf.stats
    );
    let calls = src.matches('(').count() as u64;
    let fns = (src.matches("function").count() + src.matches("=>").count()) as u64 + 1;
    let b = 4 * calls * fns;
    assert!(
        pf.stats.instances <= b,
        "instances {} > {b}: {:?}",
        pf.stats.instances,
        pf.stats
    );
}

#[test]
fn a_rest_wrapper_applying_what_it_replaced_runs_once_per_site() {
    check(
        r#"
const o: any = { f: function (...xs: any[]) { return xs.length; } };
const orig = o.f;
o.f = function f(...xs: any[]) {
  return orig.apply(this, xs);
};
const g = o.f;
console.log(g(1, 2));
console.log(g(3));
"#,
    );
}
