//! MADEROOT (`Model::chain_call`): a native run for a call a maker made
//! (a callback call, a protocol step, a bound target) is keyed under the
//! root of its maker chain, never made a root of its own per run. A
//! runtime-called callback (HTML timers, promise jobs) that reaches a
//! native again (a thenable whose `then` is a timer, a timer that
//! re-arms itself) stays finitely many calls: the solve terminates.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn kept(pf: &ProgramFacts) -> bool {
    pf.templates
        .iter()
        .any(|t| t.display.starts_with("t.ts") && t.fields.iter().any(|f| f.key == "keptmark"))
}

fn src(tail: &str) -> String {
    format!(
        "const kept = {{ keptmark: 1 }};
         const cb = () => kept;
         {tail}
         console.log(String(cb() === kept));"
    )
}

/// A timer callback that re-arms itself (`setTimeout(f)` inside `f`).
#[test]
fn a_recursive_timer_callback_terminates() {
    let pf = whole(&src(
        "function f(): void { setTimeout(f); setTimeout(cb, 0); } setTimeout(f);",
    ));
    assert!(kept(&pf), "{}", pf.dump_text());
}

/// A timer callback through `bind` and through `apply` of the timer.
#[test]
fn a_timer_callback_through_bind_and_apply_terminates() {
    let pf = whole(&src(
        "function f(this: any, k: any): void { setTimeout(f.bind(null, k)); }
         setTimeout(f.bind(null, cb));
         setTimeout.apply(null, [f as any, 0, cb]);
         (setTimeout as any).bind(null, cb)();",
    ));
    assert!(kept(&pf), "{}", pf.dump_text());
}

/// A thenable whose `then` is a timer: the resolver the adoption passes
/// is called back by the timer and adopts again (27.2.2.2
/// NewPromiseResolveThenableJob, HTML timers).
#[test]
fn a_thenable_whose_then_is_a_timer_terminates() {
    let pf = whole(&src(
        "const t: any = { then: setTimeout, keep: cb };
         Promise.resolve(t).then(cb);
         (async () => { await t; cb(); })();",
    ));
    assert!(kept(&pf), "{}", pf.dump_text());
}
