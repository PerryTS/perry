//! A defaulted parameter: the calls pass to the parameter's argument node;
//! the local holds what they pass but `undefined`, and the default when a
//! call may pass `undefined` (or nothing). An `undefined` a call passes is
//! not a value of the local, so a string default with string arguments is a
//! string receiver; a `null` or a number a call passes stays a value.

use super::*;

/// The verdicts of the method-call sites of `key`.
fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn native(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

fn unproven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// chess.ts:477 under chesst: a constructor's defaulted string, passed on
/// and trimmed; one construction passes nothing.
#[test]
fn defaulted_ctor_param_trim_is_native() {
    let pf = facts(
        r#"
        class G {
            constructor(fen: string = "a b") { this.read(fen); }
            read(fen: string): void { const f = fen.trim().split(" "); console.log(f.length); }
        }
        new G("x y"); new G();
    "#,
    );
    assert!(
        native(&pf, "trim"),
        "{:?}\n{}",
        calls(&pf, "trim"),
        pf.dump_text()
    );
}

/// A defaulted string parameter called with nothing, `undefined` and a
/// string: the local is a string.
#[test]
fn defaulted_param_passed_undefined_is_native() {
    let pf = facts(
        r#"
        function k(x: string = "s"): string { return x.trim(); }
        console.log(k(), k(undefined), k("z"));
    "#,
    );
    assert!(
        native(&pf, "trim"),
        "{:?}\n{}",
        calls(&pf, "trim"),
        pf.dump_text()
    );
}

/// Soundness: `null` is not `undefined`, the default does not replace it.
#[test]
fn defaulted_param_passed_null_stays_unproven() {
    let pf = facts(
        r#"
        function f(x: any = "s"): string { return x.trim(); }
        console.log(f(), f("a"), f(null));
    "#,
    );
    assert!(
        unproven(&pf, "trim"),
        "{:?}\n{}",
        calls(&pf, "trim"),
        pf.dump_text()
    );
}

/// Soundness: a number a call passes stays the local's value.
#[test]
fn defaulted_param_passed_number_stays_unproven() {
    let pf = facts(
        r#"
        function h(x: any = "s"): string { return x.trim(); }
        console.log(h(), h(5));
    "#,
    );
    assert!(
        unproven(&pf, "trim"),
        "{:?}\n{}",
        calls(&pf, "trim"),
        pf.dump_text()
    );
}

/// A defaulted parameter's rule is program text: the function keeps its
/// summary, each call's result is what that call passes. Under a rule only
/// the solver may add, the summary is refused and both calls' arguments
/// meet in one local (an array beside the string).
#[test]
fn defaulted_param_function_keeps_its_summary() {
    let pf = facts(
        r#"
        function k(x: any = "s"): any { return x; }
        const a = k([1]);
        const b = k("t");
        console.log(a.length, b.trim());
    "#,
    );
    assert!(
        native(&pf, "trim"),
        "{:?}\n{}",
        calls(&pf, "trim"),
        pf.dump_text()
    );
}
