//! Rule SHARED-SITE-RUN: a plain call made by a function's shared run (the
//! run no site and no object keys: a callback a builtin calls from inside a
//! summary instance, and every run below it) runs the callee's run of the
//! call's template, one per site of the program text, as the calls of an
//! unsummarized body run per site. Before it the shared run's calls all
//! ran the callee's shared run: `edges()` called by `store` and by
//! `measure` was one run with one literal, so the literal `store` puts in
//! the escaped Box was the one `measure` reads (perry-web css/layout.ts:
//! `b.margin = edges(..)` in `layoutBox` and the edge reads in
//! `layoutInline`, under the `layoutGrid` / `intrinsic` callbacks).

use super::*;

const SRC: &str = r#"import "unseen-host";
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

/// The read `e.l` in `measure` reads the literal of `measure`'s own call of
/// `edges`, which never escapes; the literal `store` stores into the Box
/// handed to `ext` is another run's.
#[test]
fn a_shared_run_call_runs_per_template_site() {
    let pf = facts(SRC);
    let s = pf
        .sites
        .iter()
        .find(|s| s.key == "l" && !s.is_call)
        .unwrap_or_else(|| panic!("site .l in\n{}", pf.dump_text()));
    assert!(
        matches!(
            s.verdict,
            crate::derive::Verdict::T0Mono | crate::derive::Verdict::T0Set
        ),
        "e.l in measure: {:?}\n{}",
        s.verdict,
        pf.dump_text()
    );
}
