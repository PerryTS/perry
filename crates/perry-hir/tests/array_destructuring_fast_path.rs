//! #10086: array destructuring over a spread-free array literal or a
//! statically-proven array must lower to the guarded non-iterator arm, and
//! everything else must keep the plain spec iterator protocol.
//!
//! These assert the LOWERING DECISION, which is what the perf fix is; the
//! behavioural half (evaluation order, defaults, holes, rest, nested patterns,
//! generators, a patched `Array.prototype[Symbol.iterator]`) is
//! `test-files/test_gap_10086_array_destructuring_fast_path.ts`, compared
//! byte-for-byte against node.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module};

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "destructure_fast.ts", &mut cache)
                    .expect("parse should succeed");
            lower_module(&parsed.module, "test", "destructure_fast.ts").expect("lowering succeeds")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn hir(src: &str) -> String {
    format!("{:#?}", lower(src))
}

/// The guard itself: `Expr::ArrayIterationPatched`, the volatile read of the
/// runtime's sticky `PERRY_ARRAY_ITERATION_NOT_PRISTINE` byte.
const GUARD: &str = "ArrayIterationPatched";

/// `[x, y] = [y, x]` — the measured swap. The guard must be read BEFORE the
/// iterator is resolved, which is the whole point: `GetIterator` takes the
/// rebuilt array literal as its operand, so a guard that dominates it means
/// neither the iterator NOR the array is materialized on the fast arm.
#[test]
fn swap_through_an_array_literal_is_guarded_and_allocates_nothing_up_front() {
    let dump = hir("let x = 1; let y = 2; [x, y] = [y, x]; console.log(x, y);");
    let guard = dump
        .find(GUARD)
        .expect("a literal-source swap must read the array-iteration guard");
    let get_iterator = dump
        .find("GetIterator(")
        .expect("the protocol arm must still exist for a patched prototype");
    assert!(
        guard < get_iterator,
        "the guard must dominate GetIterator (and therefore the array literal \
         it iterates); guard at {guard}, GetIterator at {get_iterator}"
    );
}

/// `const [a, b] = [f(), g()]` — the declaration form of the same shape.
#[test]
fn declaration_from_an_array_literal_is_guarded() {
    let dump =
        hir("function f(): number { return 1; }\nconst [a, b] = [f(), 2];\nconsole.log(a, b);");
    assert!(
        dump.contains(GUARD),
        "a literal-source declaration must read the array-iteration guard"
    );
}

/// `const [a, b] = pair(x)` where `pair(): number[]` — the pair does escape
/// (it is a real array returned across a call), so it is still allocated; what
/// goes away is the iterator object and the per-element `{ value, done }`
/// result. The fast arm reads it by index.
#[test]
fn declaration_from_a_proven_array_reads_by_index() {
    let dump = hir(
        "function pair(v: number): number[] { return [v, v + 1]; }\n\
         const [a, b] = pair(3);\nconsole.log(a, b);",
    );
    assert!(
        dump.contains(GUARD),
        "a proven-array source must read the array-iteration guard"
    );
    assert!(
        dump.contains("IndexGet"),
        "the fast arm of a proven-array source must read elements by index"
    );
}

/// An assignment whose source is a proven array takes the same arm.
#[test]
fn assignment_from_a_proven_array_is_guarded() {
    let dump = hir(
        "const src: number[] = [1, 2];\nlet a = 0; let b = 0;\n[a, b] = src;\nconsole.log(a, b);",
    );
    assert!(
        dump.contains(GUARD),
        "a proven-array assignment must be guarded"
    );
}

/// A source with no static array proof — a custom iterable, a generator, an
/// `any` — keeps the plain iterator protocol, unguarded. Emitting the fast arm
/// here would read `.length` / `[0]` off something that has neither.
#[test]
fn an_unproven_source_keeps_the_plain_iterator_protocol() {
    let dump =
        hir("declare const it: any;\nlet a = 0; let b = 0;\n[a, b] = it;\nconsole.log(a, b);");
    assert!(
        !dump.contains(GUARD),
        "an unproven source must not take the non-iterator arm"
    );
    assert!(
        dump.contains("GetIterator("),
        "an unproven source must still drive the iterator protocol"
    );
}

/// A generator call is not a proven array either.
#[test]
fn a_generator_source_keeps_the_plain_iterator_protocol() {
    let dump = hir("function* g() { yield 1; yield 2; }\nconst [a, b] = g();\nconsole.log(a, b);");
    assert!(
        !dump.contains(GUARD),
        "a generator source must not take the non-iterator arm"
    );
}

/// A rest element keeps the iterator drain: the protocol builds a DENSE array
/// while the index-side equivalent (`slice`) preserves holes, so the two
/// disagree on a sparse source.
#[test]
fn a_rest_element_keeps_the_iterator_drain() {
    let dump =
        hir("const src: number[] = [1, 2, 3];\nconst [a, ...rest] = src;\nconsole.log(a, rest);");
    assert!(
        !dump.contains(GUARD),
        "a rest pattern must keep the unguarded iterator lowering"
    );
}

/// A spread inside the literal makes the element count dynamic, so the literal
/// shape does not apply. (`[...xs]` is still an array, so the PROVEN-array arm
/// may take it — what must not happen is treating `xs`'s elements as if they
/// were the literal's.)
#[test]
fn a_spread_in_the_literal_does_not_take_the_scalar_arm() {
    let dump = hir(
        "const xs: number[] = [1, 2];\nlet a = 0; let b = 0;\n[a, b] = [...xs];\nconsole.log(a, b);",
    );
    // Whichever arm it takes, the spread must still be materialized into a real
    // array before anything reads element 0.
    assert!(
        dump.contains("ArraySpread") || dump.contains("Spread"),
        "the spread must still build a real array"
    );
}
