//! `yield*` pulls its delegate through `js_iterator_delegate_step`:
//! Call(next, iterator, « value »), or the value of that result, read from
//! a generator delegate's record. No unknown code runs; a delegate from
//! unmodeled code still yields ⊤.

use super::*;

fn facts_of(outer: &str) -> ProgramFacts {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        function* inner() {{ yield new Svc(); }}
        {outer}
        for (const s of outer()) console.log(g(s));
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

fn verdict(outer: &str) -> Verdict {
    site(&facts_of(outer), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: prints `3`. A sync generator's public `next` wraps its step's raw
/// value in a fresh `{ value, done }`.
#[test]
fn a_generator_yields_its_values() {
    let v = verdict("function* outer() { yield new Svc(); }");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `3` (also through `next().value`).
#[test]
fn next_value_is_the_yielded_value() {
    let v = verdict("function* outer() { const i = inner(); yield i.next().value; }");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `3`. `yield*` re-yields the delegate's values; its inner
/// result object rides beside them (step kind 2 is returned as-is), so the
/// site sees `Svc` among its receivers, not only a primitive.
#[test]
fn a_delegated_generator_yields_its_values() {
    let pf = facts_of("function* outer() { yield* inner(); }");
    let s = site(&pf, "size");
    assert!(s.receivers.iter().any(|r| r.contains("Svc")), "{:?} {:?}", s.receivers, s.verdict);
}

/// node: prints `1`, `2`. A yielded value reaches its consumer: the site
/// holds both layouts (it is not the `A`-only site it was when the step's
/// raw value was read as its own result object).
#[test]
fn a_yielded_value_joins_the_receivers() {
    let src = r#"
        class A { k = 1; }
        class B { z = 0; k = 2; }
        function h(o: any) { return o.k; }
        console.log(h(new A()));
        function* gen() { yield new B(); }
        for (const v of gen()) console.log(h(v));
    "#;
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let s = site(&pf, "k");
    assert!(s.receivers.iter().any(|r| r.contains(":B")), "{:?} {:?}", s.receivers, s.verdict);
}

/// node (`hostIter = [{ size: 'x' }]`): prints `x`.
#[test]
fn a_delegate_from_unmodeled_code_yields_top() {
    let v = verdict("function* outer() { yield* (globalThis as any).hostIter; }");
    assert!(!proven(&v), "{v:?}");
}

