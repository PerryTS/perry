# What the incremental collector's step budget actually bounds

`js_gc_step_us(budget_us)` and the mutator-assist paths advertise a *time*
budget. They implement it like this:

```rust
let mut result = gc_budgeted_step_work_units_inner(1);
while result.status == ACTIVE && start.elapsed().as_micros() < budget_us {
    result = gc_budgeted_step_work_units_inner(1);
}
```

The clock is consulted **between** work units and never during one. So the
advertised budget is only as strong as the most expensive single unit, and any
unit whose cost scales with the heap makes the budget a statement of intent
rather than a bound. This page records which phases are bounded, which are
deliberately not, and how to see the difference in a real run.

## The three regimes

| phase | per-unit cost | bounded by the step budget? |
|---|---|---|
| marking / trace drain | one object per unit | **yes** |
| weak processing | one holder **or one FinalizationRegistry record** per unit | **yes**, since #7903 |
| final root remark | root-set scan **plus** the transitive drain of everything it newly reaches | **no — deliberately atomic** |

### Weak processing was unbounded until #7903

A `FinalizationRegistry` is one registered weak *holder*, and weak processing
used to charge one work unit per holder while
`process_finreg_after_mark` walked that holder's entire record array inside the
unit. One registry with a million registrations was therefore one atomic,
heap-sized "work unit" sitting behind a time-budgeted API.

`crates/perry-runtime/src/weakref/sliced.rs` now keeps a cursor *into* the
record array and charges one unit per record. The module docs carry the full
argument; the part worth repeating here is why the cursor cannot simply be an
index.

Between two steps the mutator runs.
`FinalizationRegistry.prototype.unregister` **rebuilds** the entries array
without the matching records, so every index after a removed element shifts
down. A resumed index-only cursor would skip exactly as many records as were
removed before it — and a skipped record is a weak slot that never gets
tombstoned. On a non-moving budgeted cycle its target is then swept and the slot
is left dangling. That hazard is precisely why the array was atomic in the first
place; the old code said so in a comment.

So the cursor carries the **identity** of the array it indexes: the value word
of the registry's `entries` field plus that array's length. Both mutation paths
change one of the two (`unregister` installs a freshly built array; `register`
pushes). On resume the identity is re-read and compared, and a mismatch restarts
that registry's scan from index 0 against the new array. Restarting is safe
because a rescan is idempotent — the first pass writes `undefined` into a
collected record's target and `false` into its pending flag after enqueueing, so
a second pass over the same record enqueues nothing and clears nothing twice.

Restart-on-mutation alone is livelock-shaped, so restarts are capped
(`MAX_REGISTRY_RESTARTS`); past the cap the registry is finished in one atomic
pass and **charged as such** in `weak_registry_atomic_finishes`. The bound is
therefore explicit: per-step weak work is at most the requested budget, plus at
most one forced atomic registry pass per registry per cycle.

### Final root remark is atomic on purpose

`AtomicFinalizeSubphase::FinalRootRemark` re-scans every root with the marks
nearly final, then drains everything that re-scan newly discovered, both with
`usize::MAX`. The root scan really is bounded by root-set size. **The drain is
not** — a root installed after the initial scan can anchor an arbitrarily large
graph, and the code's older inline claim that the phase was "bounded by
root-set size, not heap size" did not cover it.

Two ways to make it bounded were considered and rejected:

- **Yield mid-drain.** Returning to the mutator between the remark scan and weak
  processing invalidates the remark itself: the mutator can install new roots,
  so the scan would have to be repeated, and repeating it to a fixpoint is not
  guaranteed to terminate under an adversarial mutator.
- **Yield after the liveness decision.** This is the correctness race tracked in
  #7900. Weak processing must observe a *complete* mark set; handing control
  back once liveness has been decided but before the weak slots are tombstoned
  lets the mutator observe a target that the collector has already condemned.

So the phase stays atomic, and the project's obligation is to **measure it
rather than claim it**. `final_remark_max_us` is that measurement.

## Seeing it in a run

`PERRY_GC_DIAG=1` (no `PERRY_GC_TRACE` needed, and no new env knob) prints:

```
[gc-step-bounds] step_max_us= final_remark_max_us= final_remarks= \
                 weak_records= weak_max_records_per_step= weak_steps_sliced= \
                 weak_registry_restarts= weak_registry_atomic_finishes=
```

- `step_max_us` is the honest answer to "how long is a step". Compare it against
  `GC_NORMAL_INCREMENTAL_SOFT_PAUSE_US` (2 000) and
  `GC_MUTATOR_ASSIST_SOFT_PAUSE_US` (500).
- `final_remark_max_us` is reported **separately and on purpose**. Folding an
  intentionally-atomic phase into the general maximum would let a heap-sized
  pause hide behind "the collector's worst step".
- `weak_steps_sliced` is the **subject-was-live counter**. A run reporting `0`
  has not exercised the sliced path at all, however green everything else looks.
  Do not read a zero here as "slicing works"; read it as "slicing did not
  happen". Most programs register no weak holders and will legitimately report
  zeros across this whole line — which is exactly why the acceptance tests drive
  the counters directly rather than inferring them from a corpus run.
