Bounded the untraced whole-block promotion path's worst-case retained garbage, and
stopped it reporting bytes nobody has looked at as measured-live.

Untraced promotion (#7888) skips the trace when the *previous* cycle measured a
near-fully-live young generation, and then promotes everything as
`PromotionLiveness::AssumeAllLive`. Four things made the resulting exposure much larger
than the 32 MiB footprint the comments claimed, and invisible to the pacing that would
otherwise have noticed:

**1. The dead-byte charge was zero on exactly the workloads that reach the path.**
`note_untraced_promotion()` extrapolated dead bytes from `LAST_YOUNG_SURVIVAL_PERMILLE`
verbatim, so a stationary 1000‰ reading implied `1000 − 1000 = 0` and
`PROMOTED_DEAD_BUDGET_BYTES` was never charged. The predictor is by construction the
*previous* cycle's answer and says nothing about the cohort being promoted now — charging
zero is not "no garbage", it is "no answer". The extrapolation is now clamped at
`UNTRACED_PROMOTION_SURVIVAL_PERMILLE`, the worst ratio the decision itself admits. That
is also the figure `UNTRACED_PROMOTION_SURVIVAL_PERMILLE`'s own doc already derived its
1.28 MB bound from: the doc described a bound the code did not enforce.

**2. The remaining bound was unbounded above.** `untraced_promotion_budget_bytes()` was
`max(128 MiB, old-gen-at-last-measurement)`, so on a large live old heap an abrupt
live→dead phase change could park an old-heap-sized cohort of assumed-live garbage before
anything re-measured. It is now
`min(max(floor, old-gen-at-last-measurement), ceiling)` with an explicit
`UNTRACED_PROMOTION_CEILING_BYTES`. The budget *is* the worst-case retained-garbage bound
— every byte it admits is assumed live — and it is now statable.

**3. The 128 MiB floor ignored a configured heap budget.** Both floor and ceiling now run
through `budget_scaled_with`, giving a quarter and a half of `PERRY_GC_HEAP_LIMIT`
respectively. A device heap smaller than 128 MiB no longer carries a 128 MiB
assumed-live allowance.

**4. Assumed-live bytes were credited to the *clean* old-reclaim baseline.**
`credit_promoted_bytes_to_old_baseline()` exists because promoted bytes are "live by
construction" — a marked-liveness claim an untraced cycle does not make. Crediting them
told old-reclaim pacing that an unexamined cohort was clean, deferring the very collection
that could decide it. Untraced promotions no longer feed that baseline.

**Recovery on contradiction.** When the forced measuring cycle lands and measures *below*
the untraced threshold while an untraced run is outstanding, `note_young_survival()` now
sets `GC_OLD_RECLAIM_PENDING`. Nothing else would: the traced minor measures only its own
young generation, so it can neither identify nor reclaim a cohort the preceding untraced
cycles already moved into old-gen, and a phase-changed program's heap has stopped growing
so growth pressure may never fire.

**Tests** (`gc::tests::promote_in_place`): a stationary 1000‰ predictor still charges the
threshold's implied dead bytes and still closes the composite decision once the footprint
cap is spent; the budget is asserted in both the unconstrained arm (floor, proportional
middle, ceiling) and the constrained arm (a quarter of the budget, never more than half
even against an old generation that fills it) through a pure
`untraced_promotion_budget_with` so neither arm needs the process environment; and the
contradiction path is asserted in all three states — a confirming measurement schedules
nothing, a contradicting one with an outstanding untraced run schedules the reclaim, and
a low measurement with no untraced run behind it schedules nothing.

Still open from the issue: the end-to-end `1000‰ live phase → dead churn phase` ratchet
asserting old-gen growth, full-GC timing, RSS and `heapUsed` against the documented bound.
That is a benchmark-host artifact rather than a unit test.
