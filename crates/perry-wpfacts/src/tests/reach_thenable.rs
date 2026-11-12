//! Activation reach of a thenable's `then`: a promise resolve function
//! adopts its argument (ECMA-262 27.2.1.3.2 steps 9-15, 27.2.2.2
//! NewPromiseResolveThenableJob): a thenable's `then` is called with fresh
//! resolve functions. The solve's resolve function copied the thenable as
//! it is, so a `then` reached only through it was called inactive while
//! Node ran it (check mode: "inactive body: body entered" on
//! promise/test_gap_promise_tick_parity and
//! promise/test_new_promise_only_thenable).
//! Each program's marker function `hit` runs only through the `then` under
//! test; the control is the same solve without the rule (`noreachthenable`,
//! what `PERRY_FACTS_REACH_THENABLE=0` gives). The rule's facts are
//! `promise_adopts_thenable`.

use super::*;

fn solved(src: &str, rule: bool) -> (ProgramFacts, u32) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let cfg = FactsConfig {
        sabotage: (!rule).then(|| "noreachthenable".to_string()),
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    (pf, id)
}

fn hit_runs(src: &str, rule: bool) -> bool {
    let (pf, id) = solved(src, rule);
    assert!(pf.body_reach.complete, "activated");
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

const HEAD: &str = "function hit(): any { return 1; }\n";

/// A thenable given to a resolve function: its `then` is inactive without
/// the rule.
const HOLES: &[(&str, &str)] = &[
    (
        "the executor's resolve",
        "new Promise((res: any) => res({ then(r: any) { hit(); r(1); } })).then((v: any) => console.log(v));",
    ),
    (
        "the executor's resolve, awaited",
        "(async () => { const v = await new Promise((res: any) => res({ then(r: any) { hit(); r(1); } })); console.log(v); })();",
    ),
    (
        "a resolve function kept and called later",
        "let keep: any; const p = new Promise((r: any) => { keep = r; }); keep({ then(r: any) { hit(); r(2); } }); p.then((v: any) => console.log(v));",
    ),
    (
        "a thenable's then resolves a thenable",
        "Promise.resolve(1).then(() => ({ then(r: any) { r({ then(r2: any) { hit(); r2(3); } }); } }));",
    ),
    (
        "what a thenable resolves flows on",
        "new Promise((res: any) => res({ then(r: any) { r(hit); } })).then((f: any) => f());",
    ),
];

/// What already reached the thenable's `then`: an adoption
/// (`builtins::adopt`).
const COVERED: &[(&str, &str)] = &[
    (
        "Promise.resolve",
        "Promise.resolve({ then(r: any) { hit(); r(1); } }).then((v: any) => console.log(v));",
    ),
    (
        "await",
        "(async () => { const v = await { then(r: any) { hit(); r(1); } }; console.log(v); })();",
    ),
    (
        "a handler's result",
        "Promise.resolve(1).then(() => ({ then(r: any) { hit(); r(1); } }));",
    ),
];

#[test]
fn a_thenable_given_to_a_resolve_function_has_its_then_called() {
    for (what, body) in HOLES {
        let src = format!("{HEAD}{body}");
        assert!(
            hit_runs(&src, true),
            "{what}: hit inactive with the rule\n{src}"
        );
        assert!(
            !hit_runs(&src, false),
            "{what}: hit active without the rule (no hole to close)\n{src}"
        );
    }
}

#[test]
fn an_adopted_thenable_was_reached_before() {
    for (what, body) in COVERED {
        let src = format!("{HEAD}{body}");
        assert!(
            hit_runs(&src, true),
            "{what}: hit inactive with the rule\n{src}"
        );
        assert!(
            hit_runs(&src, false),
            "{what}: hit inactive without the rule\n{src}"
        );
    }
}
