//! A call of the realm's `eval` function that is not a direct eval is an
//! indirect eval (`builtins::ns_method_inner`, `(Other, "eval")`): with the
//! interpreter linked it runs its argument as global code, which may call
//! what the program stored on the global object (DECISIONS 150/235). A
//! constant source the lowering does not fold reaches the runtime's eval
//! thunk the same way, so every spelling counts, the source's text aside.
//! Without the interpreter the thunk runs no program code.

use super::*;

/// `thr` is published on the global object; `FORM` may call it with an
/// object whose `x` is not `P`'s.
const PROGRAM: &str = r#"
    class P { x: number; constructor(x: number) { this.x = x; } }
    function thr(o: any) { return o.x; }
    (globalThis as any).thr = thr;
    console.log(thr(new P(1)));
    FORM
"#;

const SRC: &str = r#""globalThis.thr({ y: 5, x: 7 })""#;

/// Every indirect spelling of a call of `eval`.
fn forms() -> Vec<String> {
    [
        "console.log((0, eval)(SRC));",
        "const e: any = eval; console.log(e(SRC));",
        "console.log([eval][0](SRC));",
        "console.log((eval as any)?.(SRC));",
        "console.log(globalThis.eval(SRC));",
        "function f() { return (0, eval)(SRC); } console.log(f());",
    ]
    .iter()
    .map(|f| f.replace("SRC", SRC))
    .collect()
}

fn program(form: &str) -> String {
    PROGRAM.replace("FORM", form)
}

/// The verdicts of the reads of `o.x`.
fn x_reads(pf: &ProgramFacts) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == "x" && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn x_proven(pf: &ProgramFacts) -> bool {
    let v = x_reads(pf);
    !v.is_empty()
        && v.iter()
            .all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

/// The driver's link decision for a program that reaches `eval` as a value
/// (`has_deferred_dynamic_code_sites`), with no interpreted source site.
fn linked_cfg() -> FactsConfig {
    FactsConfig {
        dynamic_code: Some("E14 test: eval reached as a value".into()),
        ..Default::default()
    }
}

#[test]
fn an_indirect_eval_call_runs_interpreted_code() {
    for form in forms() {
        let pf = facts_with(&program(&form), linked_cfg());
        assert!(
            !x_proven(&pf),
            "{form}\n{:?}\n{}",
            x_reads(&pf),
            pf.dump_text()
        );
    }
}

/// Control: without the interpreter linked the eval thunk runs no program
/// code, so `thr` sees `P` alone. (`globalThis.eval(s)` is left out: its
/// receiver read already gives the global object, and with it `thr`, to
/// unknown code, with or without an interpreter.)
#[test]
fn without_an_interpreter_an_eval_call_runs_nothing() {
    for form in forms()
        .into_iter()
        .filter(|f| !f.contains("globalThis.eval"))
    {
        let pf = facts(&program(&form));
        assert!(
            x_proven(&pf),
            "{form}\n{:?}\n{}",
            x_reads(&pf),
            pf.dump_text()
        );
    }
}

/// Control: the interpreter linked but no call of `eval` reachable: the
/// global store alone gives `thr` to no one.
#[test]
fn a_global_store_without_an_eval_call_stays_proven() {
    for form in ["", "const e: any = eval; console.log(typeof e);"] {
        let pf = facts_with(&program(form), linked_cfg());
        assert!(
            x_proven(&pf),
            "{form}\n{:?}\n{}",
            x_reads(&pf),
            pf.dump_text()
        );
    }
}
