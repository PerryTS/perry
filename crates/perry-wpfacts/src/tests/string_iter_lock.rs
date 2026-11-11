//! DEC 250 on the String Iterator: `String.prototype[@@iterator]`
//! (ECMA-262 22.1.3.36) makes an iterator whose [[Prototype]] is
//! `%StringIteratorPrototype%` (22.1.5.1). The runtime seals that prototype
//! as it does the Map/Set iterator prototypes (`builtin_proto.rs`
//! `STRING_ITERATOR_FAMILY`): its image member `next` is final, adds stay
//! open. A store of `next` into an unknown value never replaces the `next`
//! the iteration protocol (or the program) calls on a string iterator; a
//! `return` the store may add there is what `IteratorClose` would call.

use super::*;

/// One store into a value unknown code returns, under `key` (a source
/// expression), then `src`.
fn facts_of(key: &str, src: &str) -> ProgramFacts {
    let src = format!(
        r#"
        export {{}};
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const f: any = (globalThis as any)[process.argv[2] ?? "Object"];
        const u: any = f(process.argv[3]);
        u[{key}] = (q: any) => q;
        const s: any = process.argv.length > 9 ? [1] : "ab";
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

const NEXT: &str = r#""next""#;

/// node: prints `3`. The tssrc shape: the iteration protocol over an
/// iterable whose `@@iterator` gives a string iterator calls
/// `%StringIteratorPrototype%.next` (locked), which hands the iterator to
/// no one. Before: "argument of a written method next".
#[test]
fn a_store_of_next_leaves_for_of_over_a_string_iterator_final() {
    let (r, v) = receivers(
        NEXT,
        "const it: any = s[Symbol.iterator](); it.box = new Svc(); \
         for (const c of { [Symbol.iterator]: () => it }) console.log(c); \
         console.log(g(it.box));",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. The program's own `next()` on a string iterator.
#[test]
fn a_store_of_next_leaves_an_explicit_string_iterator_final() {
    let (r, v) = receivers(
        NEXT,
        "const it: any = s[Symbol.iterator](); it.box = new Svc(); it.next(); \
         console.log(g(it.box));",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// node: prints `3`. The locked `next` ignores its argument (22.1.5.1):
/// an argument given to it reaches no one.
#[test]
fn an_argument_given_to_a_string_iterator_next_reaches_no_one() {
    let (r, v) = receivers(
        NEXT,
        "const it: any = s[Symbol.iterator](); const o = new Svc(); it.next(o); \
         console.log(g(o));",
    );
    assert!(proven(&v), "{r:?} {v:?}");
}

/// Control: `return` is absent on `%StringIteratorPrototype%`, which takes
/// adds; a store of `return` into an unknown value may add it there, and
/// `IteratorClose` on the `break` calls it with the iterator as `this`
/// (7.4.11 GetMethod(iterator, "return")): the stored function writes
/// through `this`.
#[test]
fn a_store_of_return_may_add_what_iterator_close_calls() {
    let (r, v) = receivers(
        "\"z\"",
        "u[\"return\"] = function (this: any) { this.box = u; return {}; }; \
         const it: any = s[Symbol.iterator](); it.box = new Svc(); \
         for (const c of { [Symbol.iterator]: () => it }) { console.log(c); break; } \
         console.log(g(it.box));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: `next` written on the string iterator itself is an own key,
/// read before the chain.
#[test]
fn next_written_on_a_string_iterator_itself_is_not_final() {
    let (r, v) = receivers(
        "\"z\"",
        "const it: any = s[Symbol.iterator](); it.box = new Svc(); \
         it.next = (w: any) => { w.box = u; return { done: true }; }; it.next(it); \
         console.log(g(it.box));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: a string iterator whose prototype the program replaces reads
/// that prototype's `next`.
#[test]
fn a_replaced_string_iterator_prototype_is_not_final() {
    let (r, v) = receivers(
        "\"z\"",
        "const it: any = s[Symbol.iterator](); it.box = new Svc(); \
         Object.setPrototypeOf(it, u); it.next(); console.log(g(it.box));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: a string iterator truly handed to unknown code escapes.
#[test]
fn a_string_iterator_given_to_unknown_code_escapes() {
    let (r, v) = receivers(
        NEXT,
        "const h: any = new Function(process.argv[4]); \
         const it: any = s[Symbol.iterator](); it.box = new Svc(); h(it); it.next(); \
         console.log(g(it.box));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control: a generator's `next(v)` resumes the body with `v`; the body
/// hands it to unknown code.
#[test]
fn a_value_the_program_resumes_a_generator_with_reaches_its_body() {
    let (r, v) = receivers(
        NEXT,
        "const h: any = new Function(process.argv[4]); \
         function* gen() { const w = yield 1; h(w); } \
         const it = gen(); const o = new Svc(); it.next(); it.next(o); console.log(g(o));",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

