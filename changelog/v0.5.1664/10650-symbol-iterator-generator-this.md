Fixed: a class generator method keyed by `[Symbol.iterator]`, when a `for…of`
loop inside it iterated a method call on `this` (`for (const x of
this.gen()) yield x;`), saw `this === undefined` and threw `Cannot read
properties of undefined (reading 'gen')`. Every consumer that dispatches
through the class's iterator protocol (`for…of`, spread, `Array.from`) hit it
identically; the identical body under an ordinary method name worked.

Root cause: lifting a `*[Symbol.iterator]()` method to its top-level
generator (`synthesize_symbol_iterator_wrapper`) rewrites `this` to an
explicit parameter via `replace_this_in_stmts`/`replace_this_in_expr`
(`crates/perry-hir/src/analysis.rs`). That rewrite had no arm for
`Expr::GetIterator`/`GetAsyncIterator`/`MapEntries`/`SetValues` — the wrapper
expressions a `for…of` iterable lowers to when it can't be proven a plain
Array/Map/Set — so a `this` buried inside one of those wrappers fell through
to the catch-all and was never rewritten.

Fix: added the four missing arms, recursing into the wrapped expression the
same way the existing `Await`/`TypeOf`/`Void` arms do.

Validation: new gap test (`test_gap_10445_symbol_iterator_generator_this.ts`)
covering the issue repro plus a two-level generator chain, a
`Symbol.iterator` generator on a class expression, and `yield*` delegation
alongside a `for…of` over `this.method()` — proven to fail on the pre-fix
tree and pass on this one, byte-identical to Node 26.5.1. `cargo test
--release -p perry-hir --tests`: 748 passed. Lint: 76/77 gates (the one red
is the pre-existing, repo-wide benchmark-freshness check).
