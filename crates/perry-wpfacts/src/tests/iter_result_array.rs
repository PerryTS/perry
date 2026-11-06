//! GetIterator's result is the iterator the protocol steps, whatever it
//! is: an `[Symbol.iterator]()` that returns an array makes that array the
//! iterator, and every consumer (for-of, destructuring, spread,
//! `Array.from`, `new Set`, `yield*`) calls the `next` its chain answers,
//! with the array as `this` (`Array.prototype.next` if one was added).
//! Only the source a consumer iterates is never the iterator:
//! `js_get_iterator` gives an array a fresh array iterator.
//!
//! node with `Array.prototype.next = function () { this[0] = { size: 7 };
//! return { done: true } }` prints `7` for every consumer below and `3` for
//! the controls.

use super::*;

/// A store of an unknown function into a value unknown code returns,
/// under the key `next` (it may add `Array.prototype.next`), then `use`
/// consumes an iterable whose `[Symbol.iterator]()` returns the array `a`
/// (or, with `direct`, `a` itself); the site is `a[0].size` after it.
fn verdict(use_src: &str, direct: bool) -> (Vec<String>, Verdict) {
    let src = if direct {
        "a".to_string()
    } else {
        "{ [Symbol.iterator]() { return a; } }".to_string()
    };
    let src = format!(
        r#"
        export {{}};
        class Svc {{ size = 3; }}
        function g(o: any) {{ return o.size; }}
        const f: any = (globalThis as any)[process.argv[2] ?? "Object"];
        const u: any = f(process.argv[3]);
        u.next = f(process.argv[4]);
        const a: any[] = [new Svc()];
        const src: any = {src};
        try {{ {use_src} }} catch {{}}
        console.log(g(a[0]));
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let s = site(&pf, "size");
    (s.receivers.clone(), s.verdict.clone())
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const FOR_OF: &str = "for (const s of src) console.log(s);";
const DESTRUCTURE: &str = "const [s] = src; console.log(s);";
const ARRAY_FROM: &str = "console.log(Array.from(src));";
const NEW_SET: &str = "console.log(new Set(src));";
const SPREAD: &str = "console.log([...src]);";
const YIELD_STAR: &str =
    "function* G() { yield* src; } for (const s of G()) console.log(s);";

#[test]
fn for_of_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(FOR_OF, false);
    assert!(!proven(&v), "{r:?} {v:?}");
}

#[test]
fn destructuring_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(DESTRUCTURE, false);
    assert!(!proven(&v), "{r:?} {v:?}");
}

#[test]
fn array_from_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(ARRAY_FROM, false);
    assert!(!proven(&v), "{r:?} {v:?}");
}

#[test]
fn new_set_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(NEW_SET, false);
    assert!(!proven(&v), "{r:?} {v:?}");
}

#[test]
fn spread_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(SPREAD, false);
    assert!(!proven(&v), "{r:?} {v:?}");
}

#[test]
fn yield_star_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(YIELD_STAR, false);
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Controls: the same consumers over the array itself never call
/// `Array.prototype.next`; `a[0]` stays a `Svc`. (`yield*` over the
/// array itself is not proven today: the delegation reaches unmodeled code,
/// a lost proof, not a false one.)
#[test]
fn consumers_of_the_array_itself_stay_final() {
    for use_src in [FOR_OF, DESTRUCTURE, ARRAY_FROM, NEW_SET, SPREAD] {
        let (r, v) = verdict(use_src, true);
        assert!(proven(&v), "{use_src}: {r:?} {v:?}");
    }
}

/// The for-of runs in a function called from two contexts (two instances):
/// in each, the array `[Symbol.iterator]()` returns is the iterator, and its
/// step is a program call of the `next` its chain answers.
#[test]
fn for_of_in_a_function_called_twice_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(
        "function run(x: any) { for (const s of x) console.log(s); } run(src); run([1]);",
        false,
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// The same with destructuring, in a method run on two receivers.
#[test]
fn destructuring_in_a_method_instance_steps_an_array_returned_by_symbol_iterator() {
    let (r, v) = verdict(
        "class R { constructor(public x: any) {} run() { const [s] = this.x; console.log(s); } }
         new R(src).run(); new R([1]).run();",
        false,
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// Control for the two above: the array itself iterated in those instances
/// is never the iterator; `a[0]` stays proven.
#[test]
fn for_of_over_the_array_itself_in_a_function_called_twice_stays_final() {
    let (r, v) = verdict(
        "function run(x: any) { for (const s of x) console.log(s); } run(src); run([1]);",
        true,
    );
    assert!(proven(&v), "{r:?} {v:?}");
}
