Fixed a cross-mode derivation error in the exact live-byte census (#7879/#7886's
replacement metric): a copied minor subtracted a **block high-water** from an **exact
object census**, charging the same dead bytes twice.

A full or non-moving sweep publishes `sweep.arena_live_bytes`, an object-walk census that
excludes the dead objects left as holes beside a survivor. It cannot reset those
survivors' blocks, so the holes stay inside the block offsets that
`copying_from_space_in_use_bytes()` sums. The next copied minor computed

    pre_collection_live_bytes − from_space_high_water + copied + promoted

which removes those holes a second time. `saturating_sub` then hides the arithmetic
failure: when the young high-water exceeds unrelated old live bytes it erases
old-generation occupancy from `heapUsed` and from major-GC pacing altogether, and the
error persists into the next census.

**Fix.** The sweep now publishes the *live* from-space share alongside the total
(`SweepTraceStats::arena_live_from_space_bytes`, accumulated in
`ArenaSweepObjectsState::keep_live_object` — the one funnel the census-publishing sweep
routes live objects through, gated on a block-index test against Eden plus the active
survivor). `record_arena_live_census` stores that share together with the from-space
high-water at the same instant, and `arena::arena_live_from_space_bytes()` derives the
current from-space contribution as `census.from_space_live + (high_water_now −
census.high_water)`, clamped to the current high-water. The copied minor subtracts that.

A copied minor publishes `None`, meaning "from-space is compacted by construction" — after
the flip Eden is empty and the new active survivor holds only the copies, so live equals
high-water there and the old and new derivations agree. That is why
`copying_minors_preserve_prior_promotions_in_the_live_census` is unchanged.

Two `debug_assert`s guard the subtraction site: live from-space bytes cannot exceed the
from-space high-water, and cannot exceed the whole live census. A high-water subtraction
can no longer silently consume unrelated generations.

**Regression test**
(`heap_accounting::copied_minor_after_a_non_moving_sweep_does_not_subtract_dead_holes_twice`)
seeds an old live cohort, fills Eden with garbage plus one rooted survivor, runs a
non-moving full sweep, then a copied minor. It asserts the setup is non-degenerate — the
from-space high-water must exceed the live from-space bytes by at least half a block, or
the two formulas coincide and a green run would prove nothing — that the old high-water
derivation would have produced a strictly smaller figure on this exact state, that the old
cohort is still present in `heapUsed`, and that major-GC pacing reads the same corrected
number.

Two comments were reclaimed to stay under the 2000-line-per-file gate: an
allocate-black paragraph duplicated verbatim in `GcCycleState::new_full` and
`new_minor_fallback`, and an 18-line description in the legacy `sweep_arena_objects` of a
"two-phase probe-then-track" strategy the code no longer implements — the very next line
already contradicted it.
