//! DEC 250 on the array iterator: the runtime seals and add-locks
//! `%ArrayIteratorPrototype%` and `%IteratorPrototype%` (as it does
//! `Object.prototype`), so no write the program makes changes what an array
//! iterator's chain answers under any key but `constructor`. A store into an
//! unknown value never replaces the `next` that `for … of` over an array
//! calls. The Map/Set iterator prototypes lock their image members and take
//! adds; the iterator helpers' take adds; what the program writes on the
//! iterator object itself, or a prototype it gives it, is not final.

use super::*;

/// One store into a value unknown code returns, under `key` (a source
/// expression): before the lock it may be `%ArrayIteratorPrototype%.next`.
fn facts_of(key: &str, src: &str) -> ProgramFacts {
    let src = format!(
        r#"
        export {{}};
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const f: any = (globalThis as any)[process.argv[2] ?? "Object"];
        const u: any = f(process.argv[3]);
        u[{key}] = (q: any) => q;
        {src}
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

fn receivers(key: &str, src: &str) -> (Vec<String>, Verdict) {
    let pf = facts_of(key, src);
    let s = site(&pf, "size");
    (s.receivers.clone(), s.verdict.clone())
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const COMPUTED: &str = r#"process.argv[4] ?? "z""#;
const NEXT: &str = r#""next""#;

/// node: prints `3`. Before the lock: "argument of a written method next".
#[test]
fn a_store_of_next_leaves_for_of_over_an_array_final() {
    let (r, v) = receivers(NEXT, "for (const s of [new Svc()]) console.log(g(s));");
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. A computed key may name any key: the whole chain
/// (`Array.prototype[@@iterator]`, the iterator prototypes) is final.
#[test]
fn a_computed_store_leaves_for_of_over_an_array_final() {
    let (r, v) = receivers(COMPUTED, "for (const s of [new Svc()]) console.log(g(s));");
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. The iterator `[Symbol.iterator]()` returns, stepped
/// by hand.
#[test]
fn a_store_of_next_leaves_an_explicit_array_iterator_final() {
    let (r, v) = receivers(
        NEXT,
        "const a = [new Svc()]; const it = a[Symbol.iterator](); console.log(g(it.next().value));",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. `%MapIteratorPrototype%` locks its image members
/// (`next`, `@@toStringTag`; adds stay open): a store of `next` into an
/// unknown value never replaces the `next` for-of over a Map iterator calls.
#[test]
fn a_store_of_next_leaves_for_of_over_a_map_final() {
    let (r, v) = receivers(
        NEXT,
        "const m = new Map([[\"a\", new Svc()]]); for (const s of m.values()) console.log(g(s));",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// Control: `next` written on a Map iterator itself is an own key, read
/// before the chain.
#[test]
fn next_written_on_a_map_iterator_itself_is_not_final() {
    let (r, v) = receivers(
        "\"z\"",
        "const m = new Map([[\"a\", new Svc()]]); const it: any = m.values(); \
         it.next = () => ({ value: u, done: false }); console.log(g(it.next().value));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: a Set iterator whose prototype the program replaces reads that
/// prototype's `next`.
#[test]
fn a_replaced_set_iterator_prototype_is_not_final() {
    let (r, v) = receivers(
        "\"z\"",
        "const t = new Set([new Svc()]); const it: any = t.values(); \
         Object.setPrototypeOf(it, u); console.log(g(it.next().value));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: node prints `undefined`. `next` written on the iterator object
/// itself is an own key, read before the chain.
#[test]
fn next_written_on_the_iterator_itself_is_not_final() {
    let (r, v) = receivers(
        "\"z\"",
        "const a = [new Svc()]; const it: any = a[Symbol.iterator](); \
         it.next = () => ({ value: u, done: false }); console.log(g(it.next().value));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: an array iterator whose prototype the program replaces reads
/// that prototype's `next`.
#[test]
fn a_replaced_iterator_prototype_is_not_final() {
    let (r, v) = receivers(
        "\"z\"",
        "const a = [new Svc()]; const it: any = a[Symbol.iterator](); \
         Object.setPrototypeOf(it, u); console.log(g(it.next().value));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: `Array.prototype` takes named adds, so a store of `next` into
/// an unknown value may add one there; the program's own `a.next()` (not
/// the iteration protocol's) runs it.
#[test]
fn a_program_call_of_next_on_an_array_is_not_final() {
    let (r, v) = receivers(NEXT, "const a: any = [new Svc()]; console.log(g(a.next()));");
    assert!(!proven(&v), "{r:?} {v:?}");
}
