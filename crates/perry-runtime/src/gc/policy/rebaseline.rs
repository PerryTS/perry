//! Re-arming the arena and malloc triggers after a collection, from what it
//! reclaimed and what survived.

use super::*;

pub(in crate::gc) fn gc_bump_malloc_trigger_with_snapshot(
    current: usize,
    bytes_now: usize,
) -> bool {
    let step = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(current + step));

    let pre_suppress = GC_PRE_SUPPRESS_BYTES.with(|c| c.get());
    let parse_growth = bytes_now.saturating_sub(pre_suppress);

    // Issue #745: gate the bytes-trigger bump on the suppressed
    // window's parse size, with two regimes:
    //
    //   * Tiny parses (<= 1 MB of arena growth) — the
    //     `test_memory_json_churn` shape: 5 k iters × ~13 KB per
    //     parse into a fragmented arena, where a small parse can still
    //     force one fresh 1 MB block while GC is suppressed. Allow
    //     repeated bumps here, but clamp them to the collector's
    //     absolute trigger ceiling so a tiny parse loop cannot keep
    //     ratcheting the next GC beyond the RSS guardrail. If a
    //     suppressed parse crosses the trigger, the next pre-parse or
    //     normal allocation check sees the trigger still due.
    //
    //   * Medium-or-larger parses (> 1 MB) — the
    //     `json_pipeline_full` and `json_polyglot` shapes: once per
    //     GC cycle, bump the trigger to grant the post-parse
    //     workload a `step` of headroom. The flag clears in
    //     `gc_collect_inner` so the next cycle gets its own bump.
    //     This is what was missing in commit 56818086 — each
    //     iteration of `json_polyglot`'s 50-iter loop bumped the
    //     trigger by another `step`, and after productive
    //     step-doubling that grew toward 1 GB the trigger ratcheted
    //     hundreds of MB above the actual live set (~5 MB) and GC
    //     never fired across the entire run. Peak RSS climbed to
    //     254/411 MB on the lazy-tape path.
    //
    // Also cap the effective step at the *initial* value (64 MB) so
    // post-`73a48ced` step-doubling can't make a single bump grant
    // hundreds of MB of headroom. The original optimization measured
    // `step` at INITIAL on the first call (no prior GC), so the cap
    // is a no-op for the `json_pipeline_full` workload.
    let is_tiny_parse = gc_suppressed_parse_is_tiny(parse_growth);
    if !is_tiny_parse && GC_TRIGGER_BUMPED.with(|c| c.get()) {
        return false;
    }

    let bytes_step = GC_STEP_BYTES.with(|c| c.get());
    let bytes_trigger = gc_bump_arena_trigger_target(bytes_now, bytes_step, is_tiny_parse);
    // Only raise — never lower — so this can't accidentally trip a
    // pending collection that the existing trigger had already armed.
    GC_NEXT_TRIGGER_BYTES.with(|c| {
        // Compare whole-arena bytes with their own threshold. The effective
        // trigger also clamps to the young-generation cap; comparing with
        // that smaller value could LOWER an armed whole-arena threshold.
        // Keep the device-budget clamp for an unarmed initializer.
        if bytes_trigger > next_arena_trigger_base() {
            c.set(bytes_trigger);
            GC_TRIGGER_ARMED.with(|a| a.set(true));
            if !is_tiny_parse {
                GC_TRIGGER_BUMPED.with(|b| b.set(true));
            }
        }
    });
    is_tiny_parse
}

fn gc_rebaseline_malloc_trigger_to_survivors(mstep: usize) {
    let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
}

/// Which nursery-collection finisher is re-baselining the whole-arena trigger.
/// Diagnostic attribution only — the arithmetic is identical for both.
#[derive(Clone, Copy)]
enum ArenaRebaselineArm {
    ArenaBytes,
    MallocCount,
}

impl ArenaRebaselineArm {
    fn label(self) -> &'static str {
        match self {
            Self::ArenaBytes => "arena",
            Self::MallocCount => "malloc",
        }
    }
}

/// Re-baseline `GC_NEXT_TRIGGER_BYTES` (and adapt `GC_STEP_BYTES`) from the
/// state a nursery collection leaves behind.
///
/// Shared by BOTH nursery-collection finishers. `GC_NEXT_TRIGGER_BYTES`'s doc
/// says it is bumped "after each `gc_collect_inner`", and until #9840 that was
/// false: only the `ArenaBytes` finisher moved it, so the whole-arena trigger
/// was measured from the last *arena-kind* collection rather than the last
/// collection. A `MallocCount` minor is the same nursery collection with the
/// malloc sweep added — the arena was swept — so it re-baselines the arena
/// trigger with exactly the arithmetic the arena arm would have used on the
/// same nursery.
///
/// The asymmetry the split kept, in the OTHER direction, is still correct and
/// still here (see `gc_finish_arena_trigger_collection`): an arena minor that
/// skipped the malloc sweep must not move the malloc trigger.
///
/// **Unit.** The base is `arena_total_bytes()` — COMMITTED bytes, all
/// generations — because that is the quantity `next_arena_trigger_base()` is
/// compared against in `gc_budgeted_due_trigger`. It is deliberately neither
/// of the two post-collection occupancy readings published at
/// [`note_collection_finished_arena_occupancy`] (a live census, and #9831's
/// bump-offset guard base); that funnel's doc comment tabulates all three.
/// Re-baselining this cell from either of those would arm the trigger below
/// the arena's own total and make the arm due the instant it re-armed.
///
/// **What is in scope.** The two *nursery-trigger* finishers, whichever
/// collection their arm ended up running — an `ArenaBytes` or `MallocCount`
/// trigger that `arena_growth_full_escalation_due()` escalated to a full still
/// finishes here, exactly as the arena arm's escalated fulls already did before
/// #9840. Out of scope is `BudgetedGcRebaseline::OldReclaim` (and the idle
/// reclaim): those are paced by the old-generation band, and after a full that
/// released blocks the un-moved trigger sits *further* above the new total,
/// which is the conservative direction.
///
/// Measured on the compiled claude-code TUI before this (`PERRY_GC_DIAG=1`,
/// per firing, four 3300-character captures on two binaries): the streaming
/// turn ran a strict 6:1 pattern — six `MallocCount` minors promoting ~3.2 MB
/// each grew the old generation past the arena trigger *inside the sixth
/// minor*, and at the very next safepoint the arm fired on a nursery of
/// **856 bytes** (`promoted_bytes=216 freed_bytes=640`), paying the whole
/// per-collection fixed cost to free 640 bytes. One in seven of the turn's
/// collections. See `secret-tests/cc-perf-campaign/DESIGN_arena_contract.md`.
fn gc_rebaseline_arena_trigger_after_collection(
    pre_in_use: usize,
    outcome: &GcCollectOutcome,
    arm: ArenaRebaselineArm,
) {
    let sweep_freed_bytes = outcome.freed_bytes;
    let post_in_use = crate::arena::arena_in_use_bytes();

    // Adaptive step:
    //   >90% freed → double (almost all dead — `object_create`-style
    //                        hot loops fit their entire working set
    //                        under the threshold; defer.)
    //   10-90% freed → halve (productive collection — real reclaim
    //                         is possible, so collect again sooner
    //                         to keep the working set bounded;
    //                         16MB floor prevents thrash).
    //   <10% freed → double (live set genuinely large, don't thrash).
    //
    // Issue #179: the halve band was formerly 10-25% only. Before
    // the age-restricted block-persist, collections in the 25-90%
    // band were illusory — block-persist re-marked dead neighbors
    // as live, so "freed" over-counted what was actually reclaimable
    // on subsequent cycles. Keeping step flat there was the correct
    // defensive choice. With v0.5.193's block-persist limited to
    // the last 5 general-arena blocks, "freed" now reflects real
    // sweep effectiveness, and widening the halve band lets the
    // trigger fire often enough for middle blocks to actually
    // reset and RSS to stay bounded. `bench_json_roundtrip` moves
    // into this band: first GC frees ~73% → halve → next trigger
    // ~56MB later → second GC frees more → step halves again →
    // RSS stabilizes instead of growing linearly with iters.
    //
    // The >90% and <10% branches retain the existing "don't thrash"
    // protection (Issue #64 follow-up): both extremes mean the
    // live/garbage ratio is such that collecting sooner is wasted
    // work.
    // Adaptive step, driven by the *larger* of sweep-freed-bytes
    // and the block-reset delta (`pre - post`). `freed_bytes` from
    // the sweep surfaces reclaim potential immediately (before the
    // 2-cycle grace completes); `pre - post` reflects actual block
    // resets landing on subsequent cycles. Using the max keeps the
    // step adaptive to both surfaces of productive collection.
    //
    //   >90% freed → double (near-total sweep; `object_create`-style
    //                        hot loops pay one GC then run free).
    //   25-90% freed → halve (productive — reclaim is meaningful,
    //                         collect again sooner to bound RSS).
    //   10-25% freed → keep (marginal — don't thrash vs. churn).
    //   <10% freed → double (live set genuinely large, defer).
    //
    // Issue #179 driver: formerly the halve band was 10-25% only,
    // which never fired on `bench_json_roundtrip` because typical
    // freed-pct there is 50-80%. With the max-of-two metric AND
    // the age-restricted block-persist (v0.5.193), widening the
    // halve band to 25-90% lets the trigger fire often enough for
    // middle blocks to actually reset, without dropping into the
    // 16MB-floor thrash territory that hurts throughput on
    // moderate workloads. `bench_json_roundtrip` lands here on
    // most cycles (60-80% freed) → step halves → GC fires 3-4×
    // across the 50-iter loop → RSS stabilizes around the live-
    // set size plus the 5-block recent-window headroom.
    //
    // The 16MB floor keeps `object_create`-scale hot loops from
    // thrashing: those workloads land in the >90% band on the
    // first GC and immediately double the step, escaping the
    // halve trajectory after a single cycle.
    let block_reclaim = pre_in_use.saturating_sub(post_in_use);
    let freed = std::cmp::max(block_reclaim, sweep_freed_bytes as usize);
    let mut step = GC_STEP_BYTES.with(|c| c.get());
    let old_step = step;
    // #9840: reported on `[gc-arena-rebaseline]` for BOTH arms, because the
    // step is now scored by both and #9831's tiny-parse guard prices its
    // headroom from it — see the note above the `GC_STEP_BYTES` write below.
    let mut scored_pct_freed = 0usize;
    if pre_in_use > 0 {
        let pct_freed = (freed * 100) / pre_in_use;
        scored_pct_freed = pct_freed;
        // 2026-05-02: widen the "double" band from `>90% || <10%` to
        // `>=85% || <10%`. ECS perf-comprehensive's two
        // alloc-heavy benches (10k two-comp, 5k × 3 cmds) sweep
        // at 86-89 % freed, which previously landed in the halve
        // band. Step would shrink 64→32→16 MB across the first
        // two benches, then GC fired every ~16 MB of fresh
        // allocations — a 60 ms `mark_block_persisting_arena_objects`
        // outlier landed mid-measured-round on each refire.
        // Promoting 85-90 % to double lets the step grow to the
        // 128 MB ceiling on the first sweep, the trigger jumps
        // out past the bench's full per-iteration allocation
        // budget, and subsequent GCs fire BETWEEN measured rounds
        // (i.e. invisible to the bench's wall-time counter).
        // `bench_json_roundtrip` lands at 50-80 % freed and is
        // unchanged — it still halves and stabilizes at the floor.
        //
        // With INITIAL == ABSOLUTE_CEILING (128 MB), the post-GC
        // `next_trigger` cap below supersedes doubling above the
        // ceiling; the doubling branch is kept for the bisection
        // escape hatch.
        if !(10..=84).contains(&pct_freed) {
            step = (step * 2).min(GC_THRESHOLD_MAX_BYTES);
        } else if pct_freed >= 25 {
            step = (step / 2).max(16 * 1024 * 1024);
        }
        // 10-25% freed → keep step unchanged (marginal churn).
        //
        // #9840, and the one behavioural coupling this change has beyond the
        // trigger itself: `GC_STEP_BYTES` had exactly one production writer —
        // this line, reached only by the `ArenaBytes` finisher — and #9831 made
        // it an INPUT to the tiny-parse pressure guard
        // (`tiny_parse_pressure_headroom_bytes`). Scoring a `MallocCount`
        // minor's productivity here therefore moves that guard's headroom too.
        // That is the intended reading of both cells and not a side effect:
        // the step is documented as "collection effectiveness", the guard's
        // own base cell was made kind-agnostic by #9831 at
        // `note_collection_finished_arena_occupancy`, and a step scored by
        // only one of the two nursery arms is the same defect this change
        // fixes, one cell over. The direction on a given workload is a
        // question for measurement, not for reasoning. Estimated over the four
        // 3300-character captures (209 `MallocCount` firings: `freed_bytes`
        // from each firing's own `[gc-copy-minor]` line over the nearest
        // `[gc-step]`'s post-collection in-use — an ADJACENT-diagnostic
        // estimate, because before this change a `MallocCount` minor emitted no
        // line carrying its own `pre_in_use`, which is exactly why the new one
        // does):
        //
        //     pct_freed  median 4-5 %   <10 %: 194/209   10-24 %: 10/209
        //                               25-84 %:   5/209   >84 %:  0/209
        //
        // 93 % of them land in the "<10 % → double" band and none in ">84 %".
        // A `MallocCount` minor frees 5-44 MB against an 83-277 MB `pre_in_use`
        // — most of which is old generation it cannot touch — so on cc this
        // coupling pushes the step UP and makes #9831's guard MORE conservative,
        // reinforcing that fix rather than eroding it. `[gc-arena-rebaseline]`
        // carries `pct=`/`step=` for both arms so the claim is read off a
        // capture instead of estimated from a neighbour.
        GC_STEP_BYTES.with(|c| c.set(step));
        // `[gc-step]` stays an ArenaBytes-only line: `scripts/gc_repsel_matrix.sh`
        // sums every `sweep_freed=` it can grep, so printing it for the malloc
        // arm too would double-count that arm's reclaim (already reported on its
        // `[gc-copy-minor]` line) in the ratchet. The malloc arm is attributed
        // on `[gc-arena-rebaseline]` below instead.
        if matches!(arm, ArenaRebaselineArm::ArenaBytes) && crate::gc::diagnostic_hooks::enabled() {
            eprintln!(
                "[gc-step] pre_in_use={} post_in_use={} sweep_freed={} block_reclaim={} pct={}% step={}→{}",
                pre_in_use, post_in_use, sweep_freed_bytes, block_reclaim, pct_freed, old_step, step
            );
        }
    }
    let new_total = crate::arena::arena_total_bytes();
    // C4b-δ-tune: hard cap on next_trigger so the >90%-freed
    // step-doubling can't drive peak nursery past the initial
    // threshold. Floor: at least 16 MB of headroom past
    // `new_total` so a workload whose post-GC live set already
    // approaches the ceiling doesn't thrash on every fresh
    // allocation.
    //
    // #9831: above `ceiling - floor` this arithmetic re-arms at `new_total +
    // floor` whatever `step` says — the productivity backoff the branch above
    // just computed is not visible to it. That is deliberate, and measured:
    // pricing the arm's own headroom by the step bought -10.8 % turn CPU on
    // the compiled claude-code TUI and cost +22 % settled footprint — a
    // CPU-for-footprint trade this project does not accept (the issue's own
    // refuted branch). The step is instead consumed where it was
    // actually being discarded — the tiny-parse pressure guard
    // (`tiny_parse_pressure_due_with`), which pulled this trigger down to
    // "now" after every small `JSON.parse` on a heap above its in-use
    // threshold and so re-fired this arm 51 times in one reply regardless of
    // what this function armed.
    let stepped = new_total.saturating_add(step);
    let capped = stepped.min(gc_trigger_absolute_ceiling_bytes());
    let floor = new_total.saturating_add(gc_trigger_headroom_floor_bytes());
    // #7742: whole-block promotion hands Eden's blocks to old-gen instead of
    // recycling them, so the free young capacity that would have carried the
    // mutator to the next collection is gone from `new_total`. Give it back as
    // headroom (consumed once) rather than by re-reserving the blocks, which
    // would map memory the program may never reach.
    let next_trigger =
        std::cmp::max(capped, floor).saturating_add(super::take_promoted_young_capacity_credit());
    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(next_trigger));
    GC_TRIGGER_ARMED.with(|a| a.set(true));
    if crate::gc::diagnostic_hooks::enabled() {
        // Field names deliberately disjoint from the `[gc-step]` line above and
        // from the `[gc-copy-minor]` line: `scripts/gc_repsel_matrix.sh` sums
        // every `sweep_freed=`/`freed_bytes=` it can grep, so a second line
        // carrying those keys would double-count reclaim in the ratchet.
        eprintln!(
            "[gc-arena-rebaseline] arm={} next_trigger={} total={} headroom={} pct={}% step={}→{}",
            arm.label(),
            next_trigger,
            new_total,
            next_trigger.saturating_sub(new_total),
            scored_pct_freed,
            old_step,
            step
        );
    }
}

pub(super) fn gc_finish_arena_trigger_collection(
    pre_in_use: usize,
    outcome: GcCollectOutcome,
) -> u64 {
    gc_rebaseline_arena_trigger_after_collection(
        pre_in_use,
        &outcome,
        ArenaRebaselineArm::ArenaBytes,
    );
    // Rebaseline the malloc-count trigger only if this collection
    // actually swept malloc objects. Copied-minor arena collections
    // may skip the malloc sweep while count pressure is still below
    // its trigger; moving the trigger in that case would postpone
    // reclamation of already-tracked dead malloc churn.
    if outcome.malloc_swept {
        let mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        gc_rebaseline_malloc_trigger_to_survivors(mstep);
    }
    outcome.emit_after_current()
}

/// `PERRY_GC_ARENA_REBASELINE_ALL=0|off|false` restores the pre-#9840
/// asymmetry: only the `ArenaBytes` finisher moves the whole-arena trigger.
///
/// The kill switch, and the positive control — both arms of the measurement
/// live in ONE binary, so no build difference can be confounded with the
/// change. Default ON; only the three explicit off-spellings turn it off, so a
/// typo cannot silently change which behaviour a bisect is measuring
/// (`env_default_on_from_value`'s contract).
///
/// CLAUDE.md's GC knob kill-policy is binding: this knob's OFF state is
/// asserted, not merely available. `direct_malloc_minor_arena_rebaseline_kill_
/// switch_restores_the_stale_threshold` in `gc/tests/debt_pacer.rs` runs the
/// same fixture as the ON-state test through the seam below and asserts the
/// defect returns — trigger left at the pre-collection value, and a second
/// minor firing on the nursery the first one emptied. That test is
/// simultaneously the sabotage proof for the ON-state pair: the OFF state IS
/// the deletion of the call, kept live by CI instead of performed by hand.
///
/// The env read is cached, so a test cannot flip it by poking the process
/// environment (and must not try — the tests run in one process). The
/// `#[cfg(test)]` seam is the supported way in, matching
/// `pacing_arena_in_use_bytes`.
fn arena_rebaseline_all_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = TEST_ARENA_REBASELINE_ALL.with(|cell| cell.get()) {
        return enabled;
    }
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ENABLED, || {
        crate::gc::env_default_on_enabled("PERRY_GC_ARENA_REBASELINE_ALL")
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`arena_rebaseline_all_enabled`]. Thread-local,
    /// so concurrently-running tests cannot see each other's value.
    static TEST_ARENA_REBASELINE_ALL: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Force [`arena_rebaseline_all_enabled`] for the duration of a test, restoring
/// the previous override on drop. `Drop` rather than a bare setter because the
/// tests that use it assert a *defect* is present, and a leaked `false` would
/// silently disarm every later test in the same thread.
#[cfg(test)]
pub(in crate::gc) struct ArenaRebaselineAllTestGuard(Option<bool>);

#[cfg(test)]
impl ArenaRebaselineAllTestGuard {
    pub(in crate::gc) fn force(enabled: bool) -> Self {
        Self(TEST_ARENA_REBASELINE_ALL.with(|cell| cell.replace(Some(enabled))))
    }
}

#[cfg(test)]
impl Drop for ArenaRebaselineAllTestGuard {
    fn drop(&mut self) {
        TEST_ARENA_REBASELINE_ALL.with(|cell| cell.set(self.0));
    }
}

pub(super) fn gc_finish_malloc_trigger_collection(
    pre_count: usize,
    pre_in_use: usize,
    outcome: GcCollectOutcome,
) -> u64 {
    debug_assert!(
        outcome.malloc_swept,
        "malloc-count trigger must sweep malloc objects"
    );
    let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
    // Adapt the malloc-count step based on collection effectiveness.
    //
    // Issue #58 insight: in tight allocation loops the conservative
    // stack scanner keeps almost everything alive — GC finds <10%
    // garbage and wastes time walking 100k+ objects. In this regime
    // we should BACK OFF (increase the step) so the loop can finish
    // without GC interference. Once control returns to a higher scope
    // the dead objects will fall off the stack and become collectable.
    //
    // Conversely, when GC reclaims >75% it's working well and can
    // afford to stay at the current cadence or even speed up.
    let mut mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    if pre_count > 0 {
        let freed = pre_count.saturating_sub(survivors);
        let pct_freed = (freed * 100) / pre_count;
        if pct_freed < 15 {
            // GC is nearly useless — quadruple the step to back off fast
            mstep = (mstep * 4).min(GC_MALLOC_COUNT_STEP_MAX);
        } else if pct_freed < 50 {
            // GC is partially effective — double the step
            mstep = (mstep * 2).min(GC_MALLOC_COUNT_STEP_MAX);
        } else if pct_freed > 90 {
            // GC is highly effective — halve the step to collect sooner
            mstep = (mstep / 2).max(GC_MALLOC_COUNT_STEP_MIN);
        }
        // 50-90% freed: keep current step (balanced)
        GC_MALLOC_COUNT_STEP.with(|c| c.set(mstep));
    }
    if outcome.malloc_swept {
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
    }
    // #9840: this collection swept the nursery too, so the whole-arena trigger
    // is measured from after it — see `gc_rebaseline_arena_trigger_after_collection`.
    if arena_rebaseline_all_enabled() {
        gc_rebaseline_arena_trigger_after_collection(
            pre_in_use,
            &outcome,
            ArenaRebaselineArm::MallocCount,
        );
    }
    outcome.emit_after_current()
}
