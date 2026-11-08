//! A global the realm declares (`JSON`, `Error`) reads as the realm's value
//! only while no write may have replaced it on the global object: a program
//! store (`globalThis.JSON = ...`), or a store into ⊤ once the global object
//! reached unmodeled code. The bare name is read off the HIR global
//! sentinel, the same object as `globalThis`: a write through either
//! spelling is seen by a read through either.
use super::*;

fn at<'a>(pf: &'a ProgramFacts, src: &str) -> Vec<&'a SiteFacts> {
    let m = src.find("// SITE").expect("marker");
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "x" && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `x` site on the SITE line");
    s
}

fn verdicts(pf: &ProgramFacts, src: &str) -> String {
    format!(
        "{:?}",
        at(pf, src)
            .iter()
            .map(|s| (&s.receivers, &s.verdict))
            .collect::<Vec<_>>()
    )
}

fn proven(pf: &ProgramFacts, src: &str) -> bool {
    at(pf, src)
        .iter()
        .all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// `p` is given to `JSON.stringify`, read as `read`; `before` runs first.
/// The replacement hands its argument to a global the program never
/// defines (unknown code).
fn program(before: &str, read: &str) -> String {
    format!(
        "class P {{ x: number; constructor() {{ this.x = 1; }} }}\n\
         const p = new P();\n\
         {before}\n\
         console.log({read}.stringify(p).length, p.x); // SITE\n"
    )
}

const REPLACE: &str = "(globalThis as any).JSON = { stringify(v: any) { (globalThis as any)[\"h\" + String(process.argv.length)](v); return \"\"; } };";

/// Red on main: the bare `JSON`, read as a value, was the realm's
/// namespace whatever the program stored on the global object. (A call
/// spelled `JSON.stringify(v)` is lowered by HIR to its own node, which
/// reads no global: not this rule.)
#[test]
fn a_program_store_replaces_the_bare_global() {
    let src = program(&format!("{REPLACE} const J: any = JSON;"), "J");
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control (green on main too): the same through `globalThis.JSON`, a
/// read whose generic load already took what the program stored.
#[test]
fn a_program_store_replaces_the_global_property() {
    let src = program(REPLACE, "(globalThis as any).JSON");
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: with no store, both spellings read the realm's `JSON`, which
/// gives `p` to no one.
#[test]
fn an_unwritten_declared_global_is_the_realms() {
    for read in ["J", "(globalThis as any).JSON"] {
        let src = program("const J: any = JSON;", read);
        let pf = facts(&src);
        assert!(proven(&pf, &src), "{read}: {}", verdicts(&pf, &src));
    }
}

/// Control: a store under another declared name leaves `JSON` the realm's.
#[test]
fn a_store_under_another_name_leaves_the_global() {
    let src = program(
        "(globalThis as any).Reflect = {}; const J: any = JSON;",
        "J",
    );
    let pf = facts(&src);
    assert!(proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// An unseen module runs at load and may write any key of the global
/// object: `JSON`, under either spelling, is no longer the realm's. (Was:
/// trusted, so tests that loaded `unseen-host` for an unknown value proved
/// sites through `Map`, `Object` or `globalThis` an unseen module may have
/// replaced; they now take their unknown value from a computed global.)
#[test]
fn an_unseen_module_may_replace_the_declared_global() {
    for read in ["J", "(globalThis as any).JSON"] {
        let src = unseen(&program(
            "const J: any = JSON; const q: any = (globalThis as any).q;",
            read,
        ));
        let pf = facts(&src);
        assert!(!proven(&pf, &src), "{read}: {}", verdicts(&pf, &src));
    }
}
