### Performance

**The callee-rooting window at a call site is computed, not hardcoded — and the
measurement says it is not what `#8159` attributed it to.**

`#8084` closed a real moving-GC defect: five call arms lowered the CALLEE into a
bare register, lowered the arguments below it — each of which can allocate — and
handed the original register to the consuming call. Under the shipping
statepoint lowering that register is in no live bundle, so nothing marks it and
nothing relocates it. The fix asked `rooting/`'s existing machinery for
protection and paid for it with a hardcoded `collects = true` at every site.

`collects` is not a strategy, it is a **window**: "can anything between this
operand and its consumer collect?" Hardcoding it `true` buys a slot store, a
re-read and a release at every call site whether or not the window contains a
collection point. `operand_protection` has always been able to answer this — its
`Reuse` arm emits no push, no re-read and no truncate, keeping the pre-`#8084`
IR byte for byte. It needed the truthful window, which `any_operand_may_collect`
computes and `with_operands_rooted_window` has passed all along.

- `lower_call/early_branches.rs` (closure-typed local call) — the window is the
  argument lowering. The re-read sits above the unmask because that is where the
  value leaves the tracked domain; a collection point *below* the unmask is a
  separate exposure that rooting the box cannot repair either way.
- `expr/new_dynamic.rs`, both `js_new_function_construct` arms — the callee's
  window is every argument, argument `i`'s is the arguments after it. Neither
  reaches a collection point of its own: `lower_js_args_array` is an entry
  alloca plus stores, and `emit_call_location_at` emits nothing at all in a
  default build. `new C(a, b)` over plain locals is back to emitting no rooting,
  which is the shape `operand_needs_root`'s own doc already claims for it.
- `expr/new_dynamic.rs`'s `NewDynamicSpread` and `expr/call_spread.rs` keep
  `true`, and now say why: `bundle_args_rooted` opens with `js_array_alloc`, and
  a spread call reaches `js_array_like_to_array` unconditionally, so those
  windows allocate in every instance of the arm. Nothing to narrow, as opposed
  to nothing narrowed.

Each flag is computed **below** the operand it protects, as
`with_operands_rooted_window` computes it: the predicate reads `ctx`, so asking
before the lowering asks about a different state.

**The measurement is negative, and that is the finding.** `#8159` attributed
`pipeline`'s +3.95% to this rooting sequence. `pipeline` never reaches these
arms: its `rec = stage(rec)` lowers through `lower_dynamic_closure_call`
(`js_closure_unbox_callee_checked`), already fully rooted before `#8084`, and
the emitted IR for `gc-handoff/apps/pipeline.ts` is BYTE-IDENTICAL across this
change. Instructions retired, min-of-5, identical runtime archives on both arms,
stdout sha-identical: `pipeline` +0.02%, `interp` −0.04%, `iso_miss` −0.05%,
`asyncpipe` +0.11%, `shapes` −0.18% — all inside noise. What it does move is the
population that has these shapes: zod's dep-native live bundles 36611 → 36598,
relocates 432545 → 432338. So `#8159`'s attribution to the commit stands and its
attribution to that hunk does not; the cost is elsewhere in `#8084`'s other
~2500 lines.

**Soundness, both corpora, base versus this change on the same build:**
dep-native `unrooted 2` (budget 3), `stale 0`, seeded 40/40 — identical;
curated-native `unrooted 0`, `stale 1`, seeded 39/40 — identical, and both
halves of that verdict are pre-existing on clean `07c8040bf` (invisible until
`#8207`, because the job aborted earlier at `--audit-poll-capable`). `#7803`'s
own shape keeps its root by construction: its arguments are freshly-allocated
object literals.

`temp_root_coverage/call_callee.rs` states the contract from both sides — two
differential pairs whose fixtures differ in exactly one thing, the argument's
kind — and lives in `src/`, so it runs in the per-PR `cargo-test` gate rather
than the nightly tier. Sabotage-checked against `#8084`'s hardcoded `true`: both
NEGATIVES go red there and both positives stay green. The callee local's
object-literal initializer is load-bearing and commented as such —
`expr_is_known_non_pointer_shadow_value` suppresses rooting for a `LocalGet`
with no reserved shadow slot whose type proof is not pointer-bearing, so a first
draft using `Expr::Undefined` had both POSITIVES failing against a correct
compiler and both negatives passing for that reason rather than the one they
claim.
