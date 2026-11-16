//! DEC 250 on the generator prototypes: the runtime refuses a write of an
//! image member of `%GeneratorPrototype%` / `%AsyncGeneratorPrototype%`
//! (`next`, `return`, `throw`; not `constructor`), so a store into an
//! unknown value never replaces the protocol a generator object inherits.
//! What the program writes on its generator function's `prototype` (an
//! ordinary object, or the object stored there instead) and keys the lock
//! leaves open (adds) are not final.

use super::*;

/// One store into a value unknown code returns, under `key` (a source
/// expression): before the lock it may be `%GeneratorPrototype%.next`.
fn top_store(key: &str) -> String {
    top_store_of(key, "(q: any) => q")
}

/// The same store of `value` (a source expression).
fn top_store_of(key: &str, value: &str) -> String {
    format!(
        r#"
        const f: any = (globalThis as any)[process.argv[2] ?? "Object"];
        const u: any = f(process.argv[3]);
        u[{key}] = {value};
        "#
    )
}

/// A module (`export {}`): its functions are no `globalThis` members the
/// unknown lookup would hand out.
fn facts_of(key: &str, src: &str) -> ProgramFacts {
    facts_of_store(&top_store(key), src)
}

fn facts_of_store(store: &str, src: &str) -> ProgramFacts {
    let src = format!(
        r#"
        export {{}};
        class Svc {{ size = 3; }}
        class Alt {{ other = 1; size = 4; }}
        function g(a: any) {{ return a.size; }}
        function* gen() {{ yield new Svc(); }}
        {}
        {src}
        "#,
        store
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

/// node: prints `3`. A store under any key cannot replace `next`: the site
/// reads the yielded `Svc` alone (before the lock: "argument of a written
/// method next").
#[test]
fn a_store_into_an_unknown_value_leaves_next_final() {
    let (r, v) = receivers(
        COMPUTED,
        "const it = gen(); console.log(g(it.next().value));",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. `for … of` calls `next` too.
#[test]
fn a_store_of_next_into_an_unknown_value_leaves_for_of_final() {
    let (r, v) = receivers(NEXT, "for (const s of gen()) { console.log(g(s)); break; }");
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. The computed key is a string (`process.argv[4]` or
/// `"z"`): the store writes no `[Symbol.iterator]`. Where the store may have
/// written a key the loop reads, the read holds what was stored (the arrow
/// `(q) => q`, called with no argument: it returns `undefined` and the loop
/// throws) beside the original member: the loop reads the yielded `Svc`
/// alone.
#[test]
fn a_computed_store_of_a_known_arrow_leaves_for_of_final() {
    let (r, v) = receivers(
        COMPUTED,
        "for (const s of gen()) { console.log(g(s)); break; }",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// Twin: a key that may be `[Symbol.iterator]` and a stored value unknown
/// code returns: the loop may iterate what that value returns.
#[test]
fn a_computed_store_of_an_unknown_value_leaves_for_of_open() {
    let store = top_store_of(
        r#"Math.random() > 0.5 ? Symbol.iterator : "z""#,
        "f(process.argv[5])",
    );
    let pf = facts_of_store(&store, "for (const s of gen()) { console.log(g(s)); break; }");
    let s = site(&pf, "size");
    assert!(!proven(&s.verdict), "{:?} {:?}", s.receivers, s.verdict);
}

/// Twin: a stored function under `[Symbol.iterator]` runs as the loop's
/// iterator method; the iterator it returns comes from unknown code.
#[test]
fn a_stored_iterator_method_returning_an_unknown_value_leaves_for_of_open() {
    let store = top_store_of("Symbol.iterator", "function () { return f(process.argv[5]); }");
    let pf = facts_of_store(&store, "for (const s of gen()) { console.log(g(s)); break; }");
    let s = site(&pf, "size");
    assert!(!proven(&s.verdict), "{:?} {:?}", s.receivers, s.verdict);
}

/// Control: a key outside the image (`forEach`, an add on
/// `%GeneratorPrototype%`) stays open: its callback's argument is unknown.
#[test]
fn a_key_outside_the_image_stays_open() {
    let (r, v) = receivers(
        COMPUTED,
        "(gen() as any).forEach((s: any) => console.log(g(s)));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// node: prints `4`. Control: the program's own `next` on the generator
/// function's `prototype` (patchable) is still what the call runs.
#[test]
fn a_next_written_on_the_function_prototype_still_runs() {
    let (r, v) = receivers(
        COMPUTED,
        "gen.prototype.next = function () { return { value: new Alt(), done: false }; };
         console.log(g(gen().next().value));",
    );
    assert!(
        r.iter().any(|r| r.contains("Alt")) || !proven(&v),
        "{r:?} {v:?}"
    );
}

/// node: prints `4`. Control: a generator object born after
/// `gen.prototype = o` inherits from `o`; its `next` runs.
#[test]
fn a_replaced_function_prototype_still_runs() {
    let (r, v) = receivers(
        COMPUTED,
        "(gen as any).prototype = { next() { return { value: new Alt(), done: false }; } };
         console.log(g(gen().next().value));",
    );
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
}

/// node: prints `4`. The same for a generator function made per call (one
/// closure context each): the replaced `prototype` of the one called.
#[test]
fn a_replaced_prototype_of_a_capturing_generator_still_runs() {
    let (r, v) = receivers(
        COMPUTED,
        "function mk(n: number) { return function* () { yield n > 0 ? new Svc() : new Svc(); }; }
         const h: any = mk(1);
         h.prototype = { next() { return { value: new Alt(), done: false }; } };
         console.log(g(h().next().value));",
    );
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
}
