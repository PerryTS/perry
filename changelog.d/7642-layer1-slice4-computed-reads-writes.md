### Layer 1 slice 4 — the computed read/write lowerings move onto the rooting API

Slice 4 of the Layer 1 campaign (#7615). Nine modules — `expr/index_get.rs`,
`expr/index_get/guarded_array.rs`, `expr/index_get/inline_dyn_typed_array.rs`,
`expr/index_set.rs`, `expr/index_set_typed_array.rs`, `expr/property_get.rs`,
`expr/property_get/globalget.rs`, `expr/property_get/helpers.rs` and
`expr/property_set.rs` — are migrated end to end and listed in
`MIGRATED_MODULES`. No module is left half-migrated and none names
`expr::temp_root` any more.

**One shape, no new combinator.** Every rooting decision in this slice was the
`StoreOperandGuard` family — lower the receiver, guard it, lower the rest,
re-read, store, release — and that is exactly what
`rooting::with_operands_rooted` and `rooting::with_operands_rooted_across`
already express. The `across` form is needed wherever the value is lowered by
`lower_value_for_dynamic_{index,property}_set` or
`lower_value_for_optional_barrier`, which produce native representations the
operand list cannot.

**Three live bugs, fixed here** — each the same window every sibling arm has
guarded since #7154, each on an arm whose store already routes through a runtime
helper, so the root is noise beside the call:

- **#7637** — `arr.length = f()` had no guard at all. The receiver is lowered
  first (spec order), `f()` allocates, and `js_array_set_length_strict` then
  truncates the abandoned from-space copy.
- **#7638** — two array element stores held a heap-string KEY across the value.
  `arr[k] = f()` with a non-numeric index rooted the receiver but not the key,
  on the one arm whose whole purpose is keys that are not proven numeric; and
  `arr[stringKey] = f()` guarded neither operand.
- **#7639** — the polymorphic `o[k] = f()` fallback guarded neither operand,
  and it is the arm reached precisely when nothing about receiver or key is
  known, so both are heap values by default. Separately, the dynamic-string-key
  arm derived the receiver's window from `value` alone — the half-measure #7201
  named in prose — so `o[f()] = 1` was unguarded.

The last one is the argument for the combinator rather than for a better flag:
`with_operands_rooted_window` computes each operand's window as
`across_collects || any_may_trigger_gc(exprs[i+1..])`, so "the receiver is live
across everything after it" is a property of the operand list instead of
something the author has to restate at each site. The dynamic-string-key arm's
two nested guards also collapse to one group, retiring the release-inner-to-outer
obligation that `temp_root_truncate`'s stack-cut semantics imposed.

**What the migration surfaced and did NOT fix — #7640.** Translating the guarded
arms made visible that about twenty arms in the same files make *no* rooting
decision at all: the bounded-index array store, the `#5525` inline typed-array
stores, `globalThis[k] = v`, ten read-side arms of `index_get.rs` where the base
is live across the key, an unsubstantiated "statepoint re-read" claim above the
class-field store, and a `unbox_str_handle` ordering hazard that no temp root can
express. Those sit on inline fast paths where a root is a measured cost, so they
are filed with a repro and a suggested order rather than smuggled into a
behaviour-preserving refactor.

That gap is now stated in the ledger itself, because it is the campaign's most
misreadable result: **a module listed in `MIGRATED_MODULES` is not an audited
module.** The ledger asserts that every rooting decision a module makes goes
through `crate::rooting`; a window with no decision at all is invisible to it.

**Verification.** Emitted IR byte-identical on all 149 modules of the curated
root-dominance corpus and all 81 of the dependency-scale (zod) corpus, with each
migrated arm's call sites counted in the emitted IR first so the A/B is not
green over a corpus containing none of the subject. Both gated modes green on
both corpora with an empty allowlist, `--seeded-violations 40` at 40/40. The
ledger sabotage was run per module — a real, compiling `temp_root_push_double` /
`temp_root_truncate` pair planted in each of the nine in turn, each recorded red
and naming its own file and lines.

The three repaired arms get differential HIR-built unit tests rather than gap
tests, because they are not reachable from ordinary TypeScript: an assignment
statement in those shapes lowers to `Expr::PutValueSet` and reaches the dynamic
IC, not these arms. Measured, not assumed — `js_array_set_length_strict` is
called zero times across the whole curated corpus.
