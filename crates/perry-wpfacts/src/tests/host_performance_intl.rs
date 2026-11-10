//! HOST seed group (tssrc performanceCore.ts, organizeImports.ts): the
//! global host objects `Intl` and `performance` (also
//! `require("perf_hooks").performance`, the same object) are read as their
//! realm declarations, and their members run their declared effects
//! (H-A: Intl.Collator, H-B: Performance.prototype). F-tests are red
//! without the batch; C-tests are the named controls.
use super::*;

fn esc(pf: &ProgramFacts, class: &str) -> bool {
    tmpl(pf, class).escape.is_some()
}

fn site_top(pf: &ProgramFacts, key: &str) -> bool {
    pf.sites.iter().filter(|s| s.key == key).any(|s| s.top)
}

fn no_host_top(pf: &ProgramFacts) {
    let d = pf.dump_text();
    for t in [
        "global Intl",
        "global performance",
        "native module ref node:perf_hooks.performance",
    ] {
        assert!(!d.contains(t), "`{t}` still read as ⊤:\n{d}");
    }
}

/// F-A (PR 1 head: `global Intl` ⊤, P is an argument of `new` of ⊤):
/// `Intl.Collator` reads its locale list and options (ToString of the
/// string options), gives them to no one, and returns a collator whose
/// `compare` getter returns its bound compare function.
#[test]
fn intl_collator_reads_its_options_and_gives_them_to_no_one() {
    let pf = facts(
        r#"
        class P { usage: string; numeric: boolean; constructor() { this.usage = "sort"; this.numeric = true; } }
        const p = new P();
        const l = Intl.Collator.supportedLocalesOf("en");
        const c = new Intl.Collator(l.length ? l[0] : "en", p as any);
        const cmp = c.compare;
        const r = cmp("a", "b");
        console.log(r + 1, p.usage);
        export const keep = 1;
    "#,
    );
    no_host_top(&pf);
    assert!(!esc(&pf, "P"), "{}", pf.dump_text());
    assert!(!site_top(&pf, "usage"), "{}", pf.dump_text());
}

/// F-B (PR 1 head: `global performance` and `native module ref
/// node:perf_hooks.performance` ⊤): both reads are the one declared
/// object; `timeOrigin` is its inherited getter's number, `mark` /
/// `measure` / `clearMarks` run ToPrimitive on their arguments (o.toString
/// runs as a program call) and keep nothing.
#[test]
fn performance_is_one_declared_object() {
    let pf = facts(
        r#"
        class O { x = 1; toString() { return "m"; } }
        const { performance: ph } = require("perf_hooks");
        const perf = typeof performance === "object" ? performance : ph;
        const o = new O();
        if (typeof perf.timeOrigin === "number" && typeof perf.now === "function") {
            perf.mark(o as any);
            perf.measure("n", "m");
            perf.clearMarks(o as any);
        }
        const t = perf.now();
        console.log(t + 1, o.x);
        export const keep = 1;
    "#,
    );
    no_host_top(&pf);
    assert!(!esc(&pf, "O"), "{}", pf.dump_text());
    assert!(!site_top(&pf, "x"), "{}", pf.dump_text());
}

/// C1: an Intl member the realm does not declare (`Intl.Segmenter`) is
/// the unknown constructor it was: its options escape.
#[test]
fn an_undeclared_intl_member_keeps_todays_path() {
    let pf = facts(
        r#"
        class P { granularity = "word"; }
        const p = new P();
        const s = new (Intl as any).Segmenter("en", p);
        console.log(s, p.granularity);
        export const keep = 1;
    "#,
    );
    assert!(esc(&pf, "P"), "{}", pf.dump_text());
}

/// C2: the `performance` members the realm holds as identities of
/// undeclared natives (`timerify`, `createHistogram`, on the object and on
/// the module) stay unknown calls: their arguments escape.
#[test]
fn an_undeclared_performance_member_keeps_todays_path() {
    for src in [
        "(performance as any).timerify(q);",
        "require(\"perf_hooks\").performance.timerify(q);",
        "require(\"perf_hooks\").timerify(q);",
        "require(\"perf_hooks\").createHistogram(q);",
    ] {
        let pf = facts(&format!(
            r#"
            class Q {{ v = 1; }}
            const q = new Q();
            {src}
            console.log(q.v);
            export const keep = 1;
        "#
        ));
        assert!(esc(&pf, "Q"), "{src}\n{}", pf.dump_text());
    }
}

/// C3: `mark` structured-clones `detail` (declared `escapes`): it escapes.
#[test]
fn mark_detail_still_escapes() {
    let pf = facts(
        r#"
        class Q { v = 1; }
        const q = new Q();
        performance.mark("m", { detail: q });
        console.log(q.v);
        export const keep = 1;
    "#,
    );
    assert!(esc(&pf, "Q"), "{}", pf.dump_text());
}

/// C4: a program write of the global reads what was written: the
/// program's `mark` runs (it hands its argument to unknown code), not the
/// declaration.
#[test]
fn a_written_global_reads_what_was_written() {
    let pf = facts(
        r#"
        class O { x = 1; }
        (globalThis as any).performance = { mark(v: any) { (globalThis as any).zzz(v); } };
        const o = new O();
        performance.mark(o as any);
        console.log(o.x);
        export const keep = 1;
    "#,
    );
    assert!(esc(&pf, "O"), "{}", pf.dump_text());
}
