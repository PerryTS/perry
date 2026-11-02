perf(hir, runtime): `const [a, b] = value` on an untyped value reads the elements
by index when the runtime proves `value` is an ordinary Array (#10524).

#10086 gave array destructuring a guarded index-read arm, but only when the
source is an array literal or statically typed `Array`. In compiled JavaScript
almost nothing is, so `const [a, b] = f()` still allocated an array iterator, a
`{ value, done }` result object per element, read both fields by name and ran
`IteratorClose` — about 35 k instructions per destructure against 2 k for
`r[0]`, `r[1]`.

`array_fast::plan_for_source` now has a third arm for a source with no static
proof (any type that could still be an Array at runtime: `any`, `unknown`,
unions, tuples, named/interface types, type variables…). The source is spilled
into an `Any` local; the guard is a new non-allocating runtime predicate,
`js_array_destructure_needs_iterator(src)`, which is the negation of the proof
`[...value]` already uses (`array::dense_spread_source`): a real
`GC_TYPE_ARRAY`, no index accessors or prototype-inherited elements, pristine
array iteration (which subsumes #10086's `PERRY_ARRAY_ITERATION_NOT_PRISTINE`
byte), no re-parented prototype, and no own `[Symbol.iterator]`. On a pass, the
fast arm reads `i < length ? src[i] : undefined` per element through an
`Array(Any)`-typed alias of the same value — the same element read the
statically-proven arm uses — and never creates an iterator; on a fail the
unchanged protocol arm runs, `GetIterator` exactly once. Strings, Maps, Sets,
generators, typed arrays, `arguments`, Proxies, `class X extends Array`
instances and custom iterables all fail the check. Types that provably cannot
be an Array (primitives, functions, promises, `Map`/`Set`/other non-Array
generics) keep the unguarded protocol with no fast arm. Rest elements, empty
patterns and nested patterns are unchanged.

Measured with the issue's benchmark (N = 500,000, median of 3, same machine and
runtime, compiler with vs without this change):

| variant | before | after | index-read control |
|---|---:|---:|---:|
| `const [a, b] = pairAny` (`any`) | 2,152 ms | 33 ms | 13.5 ms |
| `const [a, b] = mapped(i)` (untyped call result) | 2,464 ms | 114 ms | 86 ms |
| `const [a, b] = pair` (`number[]`, #10086 arm) | 18.8 ms | 18.8 ms | 13.1 ms |

Tests: `test-files/test_gap_10524_untyped_array_destructuring.ts` (holes,
defaults, a default that shrinks or grows the source, grown arrays, nested
patterns, strings/Sets/Maps/generators/typed arrays/`arguments`, custom
iterators with `return()`, non-iterables throwing `TypeError`, Array
subclasses, Proxies, index accessors, prototype-inherited elements, a patched
`Array.prototype[Symbol.iterator]`, and an async function), byte-identical to
node; the lowering decision in
`crates/perry-hir/tests/array_destructuring_fast_path.rs`; the predicate in
`array::spread_dense_tests::destructure_guard_is_the_negation_of_the_dense_proof`.
Every `test-files/*.ts` containing array destructuring (103 files) produces
byte-identical output under the compiler with and without this change.
