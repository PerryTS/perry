//! Rule PSETTLE (ECMA-262 27.2.4.1-5): the resolve / reject element
//! functions of a Promise combinator run (`all`, `allSettled`, `race`,
//! `any`) are born with the run's result promise (PerformPromiseAll etc.
//! create them with NewPromiseCapability's promise, per evaluation, per
//! item index). They are internal fields of the promise token, so the
//! `<resolver>` / `<rejecter>` pair is one per promise token, never one per
//! solver-made run. F1 (red before): `Promise.all` reached through `.call`,
//! `.apply` and an unknown callee minted a fresh pair on every native run.

use super::*;
use crate::model::{TmplKind, TokKind};

/// (resolver functions, rejecter functions, Promise tokens born by the
/// combinator calls) after the whole pipeline's solve.
fn counts(src: &str) -> (usize, usize, usize) {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    assert!(!g.m.aborted, "the solve ends within the bound");
    let m = &g.m;
    let named = |n: &str| m.funcs.iter().filter(|f| &*f.name == n).count();
    let promises = m
        .toks
        .iter()
        .filter(|t| match t {
            TokKind::Obj(tm, _) => matches!(
                m.tmpls[*tm as usize].kind,
                TmplKind::BObj { what: "Promise", .. }
            ),
            _ => false,
        })
        .count();
    (named("<resolver>"), named("<rejecter>"), promises)
}

const REACH: &str = r#"
    const u: any = (globalThis as any)[process.argv[2]];
    const all: any = Promise.all;
    const xs = [Promise.resolve(1)];
    try { all.call(Promise, xs); all.apply(Promise, [xs]); u(all); } catch (e) {}
"#;

/// F1: one pair per promise token (plus one element function per literal
/// index), however the combinator is reached.
#[test]
fn one_settle_pair_per_promise_token() {
    let (res, rej, promises) = counts(REACH);
    assert!(promises > 0, "the combinator runs");
    assert!(res > 0, "a resolver exists");
    assert!(
        res <= 2 * promises && rej <= 2 * promises,
        "resolvers {res}, rejecters {rej}, promise tokens {promises}"
    );
}

/// The settled value still flows to the result promise.
#[test]
fn the_settled_value_reaches_the_result() {
    let v = site(
        &facts(
            r#"
            class Sym { sv: number; constructor(sv: number) { this.sv = sv; } }
            function g(o: any) { return o.sv; }
            const s = new Sym(1);
            Promise.all([Promise.resolve(s)]).then((a: any) => { console.log(g(a[0])); });
            "#,
        ),
        "sv",
    )
    .verdict
    .clone();
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// The probe shape of the R1+R2 blowup (pl2): the unknown callee is handed
/// every global and the globals are stored into one another, so the combinator
/// runs again for each new function value. One pair per promise token.
#[test]
fn an_unknown_callee_loop_mints_no_pair_per_run() {
    let (res, rej, promises) = counts(
        r#"
        const top: any = (globalThis as any)[process.argv[2]];
        top[String(Math.random())] = top;
        const all: any = Promise.all;
        const xs = [Promise.resolve(1)];
        top.run = (f: any) => f.call(Promise, xs);
        try { all.call(Promise, xs); top(all); top(top.run); } catch (e) {}
        "#,
    );
    assert!(
        res <= 2 * promises && rej <= 2 * promises,
        "resolvers {res}, rejecters {rej}, promise tokens {promises}"
    );
}
