//! Without the interpreter linked, the Function-constructor token runs no
//! program code (`builtins::function_ctor_runs_code`): the Function
//! constructor converts its arguments to strings and throws, the async and
//! generator function constructors run nothing. A function's `constructor`
//! stored in a field and called as a method gives its receiver and
//! arguments to no one; their `toString` still runs.
//! With the interpreter linked (the driver's decision for a program with a
//! `new Function`/`eval` source site) the call is dynamic code, as before.

use super::*;

/// `g.constructor` is the Function constructor; `holder.m(d)` calls it
/// with `d`.
const PROGRAM: &str = r#"
    class D { y = 2; }
    const d = new D();
    const g: any = function () {};
    const holder: any = { m: g.constructor };
    BODY
    holder.m(ARG);
    console.log(d.y);
"#;

fn program(body: &str, arg: &str) -> String {
    PROGRAM.replace("BODY", body).replace("ARG", arg)
}

/// The verdicts of the reads of `d.y`.
fn y_reads(pf: &ProgramFacts) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == "y" && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn y_proven(pf: &ProgramFacts) -> bool {
    let v = y_reads(pf);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

/// What the driver passes for a program with an interpreted source site
/// (`run_pipeline`: the `dyn-eval` install condition and DECISIONS 235).
fn linked_cfg() -> FactsConfig {
    FactsConfig {
        dynamic_code: Some("E14 test: new Function(...) @ t.ts".into()),
        interpreted_sources: true,
        ..Default::default()
    }
}

#[test]
fn without_an_interpreter_the_function_constructor_runs_nothing() {
    let pf = facts(&program("", "d"));
    assert!(y_proven(&pf), "{:?}\n{}", y_reads(&pf), pf.dump_text());
    assert!(pf.interp_guard.is_none(), "{:?}", pf.interp_guard);
    // `new` of it, and `Function.call` on the rest, likewise.
    let pf = facts(&program("new holder.m(d);", "d"));
    assert!(y_proven(&pf), "{:?}\n{}", y_reads(&pf), pf.dump_text());
    let pf = facts(&program("Function.call(null, d);", "d"));
    assert!(y_proven(&pf), "{:?}\n{}", y_reads(&pf), pf.dump_text());
}

/// Sabotage: with the interpreter linked the call runs interpreted code,
/// which receives `d`.
#[test]
fn with_an_interpreter_linked_it_is_dynamic_code() {
    for body in [
        "const src = String(Math.random()); new Function(src);",
        "const src = String(Math.random()); eval(src);",
    ] {
        let pf = facts_with(&program(body, "d"), linked_cfg());
        assert!(!y_proven(&pf), "{body}\n{:?}", y_reads(&pf));
    }
}

/// Sabotage: a genuinely unknown function as the callee receives `d`.
#[test]
fn an_unknown_callee_still_receives_its_arguments() {
    let src = program("", "d").replace(
        "const holder: any = { m: g.constructor };",
        "declare const unknown: any; const holder: any = { m: unknown.f };",
    );
    let pf = facts(&src);
    assert!(!y_proven(&pf), "{:?}", y_reads(&pf));
}

/// Sabotage: the constructor converts its argument to a string, which runs
/// the argument's `toString`; here that gives `d` to unknown code.
#[test]
fn the_arguments_tostring_still_runs() {
    let pf = facts(&program(
        "declare const unknown: any; const a = { toString() { unknown.take(d); return ''; } };",
        "a",
    ));
    assert!(!y_proven(&pf), "{:?}", y_reads(&pf));
}

/// Sabotage: six arguments may be the runtime's depd `wrapfunction` shape,
/// which makes a function: that call stays dynamic code.
#[test]
fn six_arguments_stay_dynamic_code() {
    let pf = facts(&program("", "d, 1, 2, 3, 4, 5"));
    assert!(!y_proven(&pf), "{:?}", y_reads(&pf));
}
