//! A `let`/`const` a hoisted function declaration reads is forward-captured
//! and its cell is seeded with the dead-zone marker (`PreallocateTdzBoxes`).
//! The marker is not a value: a program read that finds it throws a
//! ReferenceError (`js_box_get_bits`), so only initialized values flow out
//! of the read. Only a class capture snapshot (a TDZ-suppressed read) takes
//! `undefined` from a cell in its dead zone.

use super::*;

fn sites_of<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

#[test]
fn hoisted_declaration_calls_a_const_arrow() {
    // `f` is read inside the hoisted `g`: its cell is TDZ-seeded, and the
    // call's callee is `f` alone.
    let pf = facts(
        "function run() { const f = () => 1; function g() { return f() } return g(); }
        run();",
    );
    for s in sites_of(&pf, "()") {
        assert!(
            !s.targets.is_empty() && !matches!(s.verdict, Verdict::No(_)),
            "{:?} {:?}\n{}",
            s.targets,
            s.verdict,
            pf.dump_text()
        );
    }
}

#[test]
fn hoisted_declaration_reads_a_string_let() {
    // chess tui `run()`: `let pending = ""` read inside `function receive`.
    let pf = facts(
        "function run() { let pending = \"\";
           function receive(s: string) { pending += s; return pending.indexOf(\"x\") + pending.length }
           return receive(\"a\"); }
        run();",
    );
    let idx = sites_of(&pf, "indexOf");
    assert!(
        idx.iter().all(|s| s.verdict == Verdict::NativeMember),
        "{:?}\n{}",
        idx.iter().map(|s| &s.verdict).collect::<Vec<_>>(),
        pf.dump_text()
    );
    let len = sites_of(&pf, "length");
    assert!(
        len.iter().all(|s| s.verdict == Verdict::OwnLength),
        "{:?}\n{}",
        len.iter().map(|s| &s.verdict).collect::<Vec<_>>(),
        pf.dump_text()
    );
}

#[test]
fn class_capture_snapshot_keeps_the_dead_zone_undefined() {
    // The class is evaluated and constructed while `x` is in its dead zone:
    // the capture snapshot takes `undefined` (no ReferenceError), and the
    // refresh after the initializer republishes the string. A read through
    // the snapshot sees both.
    let pf = facts(
        "function run() {
           class C { n() { return x.length } static s() { return x.length } }
           const c = new C();
           let x = \"abc\";
           return c.n() + C.s(); }
        run();",
    );
    for s in sites_of(&pf, "length") {
        assert!(
            matches!(s.verdict, Verdict::No(_)) || s.top,
            "{:?}\n{}",
            s.verdict,
            pf.dump_text()
        );
    }
}
