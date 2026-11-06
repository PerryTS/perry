//! A class linked by name to a built-in parent (`class HttpError extends
//! Error`) runs, at `super()`, the value its `extends` evaluated, with
//! `this` = the instance. When that value is the constructor the global
//! object declares under the name, the run is the native summary and the
//! instance reaches no program code; any other value the solve finds (a
//! program-written or unseen-module-written global, a local binding of the
//! same name, `null`) runs as before: it escapes with the instance
//! (three.js `class HttpError extends Error`).
use super::*;

fn at<'a>(pf: &'a ProgramFacts, src: &str, marker: &str, key: &str) -> Vec<&'a SiteFacts> {
    let m = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == key && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `{key}` site on the {marker} line");
    s
}

fn verdicts(pf: &ProgramFacts, src: &str) -> String {
    format!(
        "{:?}",
        at(pf, src, "// SITE", "x")
            .iter()
            .map(|s| (&s.receivers, &s.verdict))
            .collect::<Vec<_>>()
    )
}

fn proven(pf: &ProgramFacts, src: &str) -> bool {
    (at(pf, src, "// SITE", "x").iter()).all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// `a` is passed to the constructor of a class whose parent is `Error`;
/// `before` runs first.
fn program(before: &str) -> String {
    format!(
        "{before}\n\
         class HttpError extends Error {{ r: any; constructor(m: string, r: any) {{ super(m); this.r = r; }} }}\n\
         class V {{ x: number; constructor(x: number) {{ this.x = x; }} add(o: V): V {{ return new V(this.x + o.x); }} }}\n\
         const a = new V(1), b = new V(2);\n\
         function f(m: string): any {{ try {{ throw new HttpError(m, a); }} catch (e: any) {{ return e.message; }} }}\n\
         console.log(a.add(b).x, f(\"q\")); // SITE\n"
    )
}

/// Red on main: the evaluated parent (the realm's `Error`) was sunk, and
/// the instance with `a` in it escaped (E11).
#[test]
fn the_realms_error_as_parent_gives_the_instance_to_no_one() {
    let src = program("");
    let pf = facts(&src);
    assert!(proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: the program replaced the global `Error`: `super()` may run it.
#[test]
fn a_program_written_global_error_runs_as_the_parent() {
    let src = program(
        "(globalThis as any).Error = class { constructor(m: any) { (globalThis as any).leak = this; } };",
    );
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: an unseen module may have written the global (rule G): ⊤.
#[test]
fn an_unseen_modules_global_error_runs_as_the_parent() {
    let src = unseen(&program(""));
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: a global `Error` the program set to `null` is no constructor
/// (no instance exists in Node); the model keeps the escape.
#[test]
fn a_null_global_error_keeps_the_escape() {
    let src = program("(globalThis as any).Error = null;");
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: an unknown `extends` value (not linked by name) still runs as
/// unknown code.
#[test]
fn an_unknown_parent_value_still_escapes() {
    let src = unseen(&program("").replace(
        "extends Error",
        "extends ((globalThis as any)[\"E\" + String(process.argv.length)] as any)",
    ));
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}
