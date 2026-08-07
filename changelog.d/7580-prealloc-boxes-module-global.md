**fix(codegen): `PreallocateBoxes` no longer shadows a module-level global (#7521)**

Every ES module with a top-level `{ … }` block containing a `function`
declaration that read a sibling `let`/`const` compiled to a silently-empty
binding:

```ts
import { anything } from "node:path";   // any import — it just makes the file strict
{
  const events: string[] = [];
  function t(a: any) { events.push("fn:" + a); }
  t("A");
  console.log(events.length);   // node: 1   perry: 0
}
```

Nothing threw, so the breakage was invisible except where a test happened to
print the accumulated value. Object, number and string accumulators lost the
same way — inside `t`, the captured binding simply read `undefined`, so
`acc.n++` threw `Cannot read properties of undefined`. It surfaced as
`test-files/test_gap_diagchannel_3082_3084_3085_3086.ts` printing an empty
`events` array for the `#3086` `traceCallback` block, but neither
`traceCallback`, nor closures, nor arrays were involved.

Root cause: `emit_preallocate_boxes` (`crates/perry-codegen/src/stmt/mod.rs`)
allocated a heap box — and, decisively, a `ctx.locals` slot — for every id in a
`Stmt::PreallocateBoxes` directive, including ids that
`codegen/module_globals_emit.rs` had already promoted to
`@perry_global_<mod>__<id>`. Codegen already encodes the rule that the module
global wins over the box (`ctx.boxed_vars.contains(id) &&
!ctx.module_globals.contains_key(id)`, repeated at every read/write site in
`expr/literals_vars.rs` and `stmt/let_stmt.rs`); this was the one place that did
not check. The stale `ctx.locals` entry is consulted *before*
`ctx.module_globals` on the `Stmt::Let` reuse path and the `LocalGet`/`LocalSet`
store paths, so the declaration wrote into the local box-pointer slot, the
module global was never stored, and the closure — which reads through the
global, with zero captures — saw the `undefined` the global was defined with.

The trigger was #7105's `lower_strict_block_fn_decls`, which made a bare block
at the top level of a strict module emit `PreallocateBoxes` for the block's
lexical bindings — the first time a promoted module-level id was ever handed to
`emit_preallocate_boxes`. (#6853's window, suspected on the issue, is not
involved.)

Fix: skip promoted ids. The global is statically initialized to
`TAG_UNDEFINED`, exactly what a non-TDZ prealloc box seeds. The TDZ variant is
skipped too — module-global reads are a raw `load double @g` with no
`js_box_get_bits` choke point, so seeding `TAG_TDZ` would leak the sentinel into
arithmetic instead of throwing a `ReferenceError`.

Regression coverage is an in-`src` unit module
(`crates/perry-codegen/src/stmt/prealloc_module_global_tests.rs`), not a
`crates/*/tests/*.rs` suite, so it runs in the per-PR `cargo-test` gate rather
than only nightly (#5960) — the gap test that surfaced this is in neither
`gap_snapshot.json` nor `known_failures.json`, and `parity` is tag-gated, which
is why it sat unnoticed from 2026-07-30. The two positive tests are
sabotage-checked: reverting the `ctx.module_globals` guard takes both red while
the fixture-premise test and the "#569/#6044 still gets its box" test stay
green. The store assertion counts `store … ptr @perry_global_…` lines inside
`main()` rather than using `contains` — `main` also takes the global's address
for `js_gc_register_global_root`, which is emitted whether or not the
declaration ever writes the cell, and that reference made the first draft of the
test pass under sabotage.

The stale `test-parity/known_failures.json` entry for
`test_gap_diagchannel_3082_3084_3085_3086` (added 2026-07-04 for the original
`#3082/#3084/#3085/#3086` feature cluster, long since implemented) is retired in
the same change — that list is a pure suppression, so an entry whose test has
started passing silences the regression forever rather than failing the way the
`gc_root_dominance_allowlist` ratchet does. The test now byte-matches
`node --experimental-strip-types` with exit 0 across all four blocks.
