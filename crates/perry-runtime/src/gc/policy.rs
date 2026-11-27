use super::heap_budget::*;
use super::trigger_watermark::TriggerWatermark;
use super::*;

mod knobs;
pub(in crate::gc) use knobs::gc_trace_enabled;
#[cfg(test)]
pub(crate) use knobs::moving_loop_polls_enabled_from_env;
#[cfg(test)]
pub(in crate::gc) use knobs::{
    force_alloc_point_minor_pacing, force_legacy_gc_pacing, force_moving_gc_pacing,
    MovingLoopPollsGuard, TestGcTraceCaptureGuard,
};
pub(crate) use knobs::{
    gc_incremental_enabled, gc_moving_loop_polls_enabled, gc_moving_safepoint_enabled,
};

mod external_side;
#[cfg(test)]
pub(in crate::gc) use external_side::external_side_parse_pressure_due_with;
#[cfg(test)]
use external_side::GC_EXTERNAL_SIDE_LIVE_BYTES;
pub(in crate::gc) use external_side::{
    external_side_live_bytes, external_side_old_reclaim_pressure_bytes,
    external_side_parse_band_bytes, external_side_parse_pressure_due,
    GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL, GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES,
};
pub(crate) use external_side::{gc_note_external_side_alloc, gc_note_external_side_free};

mod malloc_trigger;
pub use malloc_trigger::gc_bump_malloc_trigger;
pub(crate) use malloc_trigger::{
    gc_bump_json_malloc_trigger_deferred, gc_schedule_malloc_sweep_after_json_output,
    gc_service_json_output_sweep, JsonOutputSweep,
};
pub(in crate::gc) use malloc_trigger::{
    GC_MALLOC_COUNT_STEP, GC_MALLOC_COUNT_STEP_MAX, GC_MALLOC_COUNT_STEP_MIN,
};

mod parse_pressure;
#[cfg(test)]
pub(in crate::gc) use parse_pressure::force_tiny_parse_pressure_due_for_test;
use parse_pressure::{diag_tiny_parse_forced_collection, tiny_parse_in_use_trigger_for_mode};
pub(in crate::gc) use parse_pressure::{
    gc_bump_arena_trigger_target, gc_schedule_json_construction_grace, gc_suppressed_parse_is_tiny,
    tiny_parse_generational_collection_due, tiny_parse_pressure_due,
    GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES,
    GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES, GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL,
};
pub use parse_pressure::{
    gc_collect_pending_suppressed_parse, gc_schedule_parse_boundary_collection_if_pressure,
    gc_schedule_tiny_parse_boundary_collection_if_pressure,
};
#[cfg(test)]
pub(in crate::gc) use parse_pressure::{
    tiny_parse_boundary_poll_next, tiny_parse_pressure_due_with,
    tiny_parse_pressure_headroom_bytes, GC_SUPPRESSED_TINY_PARSE_BYTES,
};

mod suppress;
#[cfg(test)]
pub(crate) use suppress::window_exit_probe;
pub(crate) use suppress::{gc_is_suppressed, GcSuppressScope};
pub use suppress::{gc_suppress, gc_unsuppress};

mod promotion_handoff;
#[cfg(test)]
pub(in crate::gc) use promotion_handoff::survivor_promotion_handoff_suppressions;
#[cfg(test)]
pub(in crate::gc) use promotion_handoff::{
    copied_minor_promotable_active_survivor_bytes, copied_minor_promotion_handoff_pressure_due,
    survivor_promotion_handoff_awaiting_minor,
};
pub(in crate::gc) use promotion_handoff::{
    copied_minor_promotion_handoff_due, credit_promoted_bytes_to_old_baseline,
    note_copying_minor_completed, note_copying_minor_young_survival,
    note_survivor_promotion_handoff_full, GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
};

mod old_reclaim;
pub use old_reclaim::js_gc_old_reclaim_valve_test_override;
pub(in crate::gc) use old_reclaim::{
    finish_full_old_reclaim_baseline, gc_old_reclaim_growth_band_bytes,
    maybe_schedule_old_reclaim_after_copied_minor, old_gen_reclaimable_pressure_bytes,
    old_reclaim_pressure_due, request_old_reclaim_for_untraced_promotions,
    GC_OLD_GEN_RECLAIM_GROWTH_BYTES, GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
};
use old_reclaim::{old_reclaim_at_allocation_point, OldReclaimReentryGuard};
#[cfg(test)]
pub(in crate::gc) use old_reclaim::{
    old_reclaim_defer_within_slack, old_reclaim_pressure_due_inner,
};
#[cfg(test)]
pub(in crate::gc) use old_reclaim::{
    test_clear_old_reclaim_pre_in_use_bytes, test_old_reclaim_backoff_shift,
    test_old_reclaim_pre_in_use_bytes, test_price_old_reclaim_full,
    test_set_old_reclaim_backoff_shift, test_set_old_reclaim_last_post_in_use,
};

mod rebaseline;
pub(in crate::gc) use rebaseline::gc_bump_malloc_trigger_with_snapshot;
#[cfg(test)]
pub(in crate::gc) use rebaseline::ArenaRebaselineAllTestGuard;
use rebaseline::{gc_finish_arena_trigger_collection, gc_finish_malloc_trigger_collection};

mod major_pacing;
pub(in crate::gc) use major_pacing::{
    arena_growth_full_escalation_due, major_pacing_live_growth_multiplier,
    note_collection_finished_arena_occupancy, old_gen_growth_since_last_full_bytes,
    pacing_arena_in_use_bytes, pacing_escalation_reading_bytes,
};
#[cfg(test)]
pub(in crate::gc) use major_pacing::{
    major_pacing_backoff_shift, major_pacing_config, major_pacing_escalation_threshold_for,
};
#[cfg(any(perry_diagnostics, test))]
pub(in crate::gc) use major_pacing::{major_pacing_retaining, major_pacing_snapshot};
use major_pacing::{
    pacing_old_in_use_bytes, update_major_pacing_backoff, MAJOR_PACING_PRODUCTIVE_YIELD_PCT,
    MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER, MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE,
};
#[cfg(test)]
pub(in crate::gc) use major_pacing::{
    test_major_pacing_pre_in_use_bytes, test_note_full_cycle_reclaimed,
    test_reset_major_pacing_backoff, test_set_collection_post_in_use_bytes,
    test_set_major_pacing_baseline, test_set_pacing_arena_in_use,
};

mod safepoint;
pub use safepoint::js_gc_loop_safepoint;
pub(in crate::gc) use safepoint::{
    contract_scan_heal_guard, moving_defer_within_slack, GC_MOVING_DEFER_SLACK_BYTES,
};
pub(crate) use safepoint::{
    gc_safepoint_moving_minor, gc_safepoint_moving_minor_nothing_due, set_safepoint_pending,
};
#[cfg(test)]
pub(in crate::gc) use safepoint::{
    run_promoted_cohort_full_if_due, safepoint_only_contract_from_value, SafepointOnlyContract,
};

mod budgeted;
#[cfg(test)]
pub(crate) use budgeted::gc_runtime_safepoint;
#[cfg(test)]
pub(in crate::gc) use budgeted::test_start_budgeted_minor_fallback_state_with_trace;
#[cfg(test)]
use budgeted::GC_BUDGETED_CYCLE_ACTIVE;
use budgeted::{
    defer_nursery_cap_to_precise_safepoint, gc_mutator_assist_step_work_units_inner_with_progress,
    GC_BUDGETED_STEP_ACTIVE,
};
pub(crate) use budgeted::{
    gc_boundary_safepoint_poll, gc_budgeted_cycle_active, gc_runtime_safepoint_poll,
};
pub(in crate::gc) use budgeted::{
    gc_drain_active_budgeted_cycle, gc_idle_reclaim_step, gc_idle_reclaim_try_start,
    BudgetedGcTrigger,
};
pub use budgeted::{
    js_gc_safepoint, js_gc_step_status, js_gc_step_us, js_gc_step_work_units, JsGcStepResult,
    JS_GC_STEP_STATUS_ACTIVE, JS_GC_STEP_STATUS_COMPLETED, JS_GC_STEP_STATUS_IDLE,
    JS_GC_STEP_STATUS_SKIPPED,
};

mod manual_collect;
use manual_collect::manual_gc_collect_now;
#[cfg(test)]
pub(in crate::gc) use manual_collect::unsafe_zone_test_override;
#[cfg(test)]
pub(in crate::gc) use manual_collect::GC_UNSAFE_WARNED;
pub(in crate::gc) use manual_collect::{
    gc_blocked_by_unsafe_zone, manual_gc_blocked_by_unsafe_zone,
};
pub use manual_collect::{
    js_gc_collect, js_gc_enter_unsafe_zone, js_gc_exit_unsafe_zone, js_gc_module_collect,
    js_gc_module_idle_hint, js_gc_module_minor, GC_UNSAFE_ZONES,
};

mod leaf_window;
use leaf_window::leaf_memory_due_at_cap;
pub(in crate::gc) use leaf_window::leaf_service_precedes_copying_cap;
#[cfg(test)]
pub(in crate::gc) use leaf_window::leaf_window_due;

pub(super) const GC_FLAG_IN_ALLOC: u8 = 0b01;
/// Bit 1 of GC_FLAGS — suppression flag (JSON.parse).
pub(super) const GC_FLAG_SUPPRESSED: u8 = 0b10;
/// Bit 2 of GC_FLAGS: an allocation-point trigger check reached
/// [`gc_check_trigger_evaluate`] while [`GC_FLAG_SUPPRESSED`] was set and was
/// turned away. The check is owed, not answered: the outermost
/// [`GcSuppressScope`] runs it when it drops. A `gc_suppress` window
/// ([`gc_unsuppress`]) clears it instead, because that window's owner prices
/// its whole growth itself at the window's end (JSON.parse's
/// `finish_parse_gc_accounting`, `gc_bump_malloc_trigger`).
pub(super) const GC_FLAG_TRIGGER_OWED: u8 = 0b100;

crate::perry_thread_local! {
    pub(super) static GC_FLAGS: Cell<u8> = const { Cell::new(0) };
}

/// Threshold: run GC when total arena bytes exceed this.
///
/// Current app-pattern tuning: 128 MB. The earlier 64 MB setting reduced
/// peak RSS on JSON round-trip style workloads, but it also forced a
/// collection in `buffer_transcode` while the benchmark still held a large
/// live set of rows/strings/buffers. That collection could not reclaim enough
/// and pushed the benchmark past the 30s smoke timeout. Returning the initial
/// trigger to 128 MB keeps allocation-heavy transcode and ECS-style bursts out
/// of mid-run GC while JSON parse/stringify remain below the 1.5x Bun gap in
/// the app-pattern matrix. The absolute ceiling below still bounds later
/// adaptive trigger growth at 128 MB after collections have started.
pub(super) const GC_THRESHOLD_INITIAL_BYTES: usize = 128 * 1024 * 1024; // 128 MB
/// Sanity bound on the adaptive step itself. Step growth past 1 GB is
/// only theoretically possible on multi-day services where GC fires
/// rarely; we keep the cap loose here since the *real* peak-RSS
/// guardrail is `GC_TRIGGER_ABSOLUTE_CEILING` below.
pub(super) const GC_THRESHOLD_MAX_BYTES: usize = 1024 * 1024 * 1024; // 1 GB

/// Hard ceiling on the next-GC trigger (arena_total bytes), independent
/// of how productive recent sweeps have been. Without this, the
/// >90%-freed branch doubles the step on every productive collection,
/// > and `next_trigger = new_total + step` lets peak nursery occupancy
/// > grow unboundedly even when most of what we collected was garbage.
/// > On `bench_json_roundtrip` direct (50 iters × ~5 MB / iter, GC fires
/// > 3 times), the step doubled from 64 MB → 67 MB → 134 MB and the
/// > trigger followed it, so peak nursery hit 115 MB at GC #3 — the
/// > dealloc pass from C4b-δ then returned 91 MB to the OS, but the
/// > peak-RSS damage was already done. Capping the trigger at the
/// > initial threshold prevents that runaway: after GC, trigger ≤ 128 MB
/// > regardless of how much step adapted, so peak nursery stays bounded
/// > to roughly initial + one iter's allocation buffer + headroom for
/// > non-arena overhead.
///
/// Floor: even if `arena_total` is already near or past the ceiling
/// (large old-gen + longlived combined live set), keep at least the
/// 16 MB step floor as headroom — `next_trigger = max(new_total + 16 MB,
/// min(new_total + step, ceiling))`. This avoids GC thrash when the
/// non-nursery component of arena_total alone exceeds the ceiling.
///
/// 2026-05-02 raise from 64 MB → 128 MB: ECS perf-comprehensive's
/// allocation-heavy benches (10k two-comp + sync, 5k × 3 cmds) hit
/// the 64 MB cap mid-round, then the >25%-freed branch halved the
/// step to 16 MB, so the next trigger landed ~16 MB above the post-
/// GC working set — well within a single round's allocation budget.
/// Result: 1-2 mid-round GCs per bench, the worst of which spent
/// 60 ms inside `mark_block_persisting_arena_objects` force-marking
/// + tracing 40 k newly-allocated objects in the recent window.
/// Doubling the cap lets productive sweeps accumulate full
/// `step` headroom (up to 128 MB) before the next trigger, which
/// shifts those GC events out of the measured rounds entirely.
/// `bench_json_roundtrip`-class workloads still bounded — they
/// finish under 128 MB peak and fire ≤2 GCs total.
///
/// Workloads unaffected: `07_object_create` / `12_binary_trees` /
/// `bench_gc_pressure` all fit their working sets under 64 MB and
/// fire GC at most once. The cap only changes behavior when the step
/// would otherwise have pushed the trigger past the initial threshold,
/// which is exactly the bench-RSS scenario this is targeting.
pub(super) const GC_TRIGGER_ABSOLUTE_CEILING: usize = 128 * 1024 * 1024;

// Device-derived heap budget: see gc/heap_budget.rs (split out for the
// 2000-line file lint).

/// The arena-bytes trigger as the collector should compare it: the raw
/// cell while armed (explicit re-arms/bumps may legitimately exceed the
/// ceiling — headroom floor over a big live set, medium-parse bumps), the
/// device-derived ceiling while the cell still holds its desktop-default
/// const initializer.
/// The adaptive/armed arena trigger WITHOUT the scavenge nursery cap:
/// compared against `arena_total_bytes()` (all generations), as it always
/// was. The cap is deliberately not part of this value — it is
/// young-generation-scoped and lives in [`young_scavenge_cap_due`].
pub(super) fn next_arena_trigger_base() -> usize {
    if GC_TRIGGER_ARMED.with(|a| a.get()) {
        GC_NEXT_TRIGGER_BYTES.with(|c| c.get())
    } else {
        GC_NEXT_TRIGGER_BYTES
            .with(|c| c.get())
            .min(gc_trigger_absolute_ceiling_bytes())
    }
}

/// True when the young generation (Eden + active survivor space) has
/// reached the effective scavenge nursery cap.
///
/// The basis is young-gen occupancy, NOT `arena_total_bytes()`. Comparing
/// the cap against the total put every old-gen byte on the young budget:
/// once old-gen in-use crossed 16 MB, every fresh 1 MB Eden block
/// re-crossed the trigger, degenerating the scavenge cadence to
/// once-per-block on any program with a large tenured set. That cadence is
/// the actual mechanism behind the tree.ts survivor-saturation regression
/// — objects were scavenged ~1 MB of allocation after birth, so almost
/// nothing had time to die: measured 1.05 MB of survivors per 1 MB block
/// (near-zero infant mortality), a saturated survivor space, and 1427
/// collections for a run that allocates ~1.4 GB.
pub(super) fn young_scavenge_cap_due() -> bool {
    young_scavenge_cap_due_with_old_reclaimable(old_gen_reclaimable_pressure_bytes)
}

/// [`young_scavenge_cap_due`] with the old-gen reclaimable-pressure read
/// supplied by the caller. `gc_budgeted_due_trigger` has already read that
/// value for its old-reclaim arm, and nothing between that read and this one
/// touches old-gen, so it passes a closure returning the value it holds rather
/// than paying two more thread-local reads. The read happens where it always
/// did: after the census seed, which changes only the young-side factor.
fn young_scavenge_cap_due_with_old_reclaimable(old_reclaimable: impl FnOnce() -> usize) -> bool {
    young_scavenge_cap_probe(old_reclaimable).due
}

/// [`young_scavenge_cap_due_with_old_reclaimable`]'s answer plus what it read
/// to reach it — `None` when the cap is not in force and nothing was read.
struct YoungScavengeCapProbe {
    due: bool,
    reading: Option<super::trigger_watermark::YoungArmReading>,
}

fn young_scavenge_cap_probe(old_reclaimable: impl FnOnce() -> usize) -> YoungScavengeCapProbe {
    if !nursery_cap_active() {
        return YoungScavengeCapProbe {
            due: false,
            reading: None,
        };
    }
    // One read of the cached occupancy also says whether no young leaf lives.
    let (from_space_in_use, leaf_free) =
        crate::arena::copying_from_space_in_use_bytes_and_leaf_free();
    // The current Eden block's share of that reading: the inline offset,
    // whenever the inline allocator is initialized (see
    // `synced_current_eden_block_bytes`). Read here, before anything else
    // runs, so it is the value the occupancy was summed from.
    let inline_offset = {
        // SAFETY: this thread's own inline allocator state, read only.
        let inline = unsafe { &*crate::arena::hot_inline_state() };
        (!inline.data.is_null()).then_some(inline.offset)
    };
    // #8122: before the first copying minor has measured survivors, denominate
    // the FIRST cap in this program's objects too (one header walk, once per
    // process, halfway to the base cap). Not while a collection is in
    // progress or a budgeted cycle is active — the young generation is being
    // rewritten then and the walk would read forwarding stubs.
    //
    // The seeded test comes first because the seed can happen only once: in
    // steady state this is one thread-local read instead of three.
    if !super::tenuring::object_census_seeded()
        && GC_FLAGS.with(|f| f.get()) & GC_FLAG_IN_ALLOC == 0
        && !gc_budgeted_cycle_active()
    {
        super::tenuring::maybe_seed_object_census_from_allocation(from_space_in_use);
    }
    let cap = scavenge_nursery_cap_dueness_bytes_with(old_reclaimable);
    // #10698: while the census is unseeded, the first reading at or past the
    // seed point seeds it and re-denominates the cap, so "not due" only
    // holds below whichever comes first.
    let limit = if super::tenuring::object_census_seeded() {
        cap
    } else {
        cap.min(super::tenuring::object_census_seed_point_bytes())
    };
    // Growing the inline small allocation can cross the leaf window before
    // the copying cap. Publish the same residual bound to the fast shortcut.
    // With no young leaf alive this is the not-taken branch of a flag the
    // occupancy read already produced.
    let (leaf_due, limit) = if leaf_free {
        (false, limit)
    } else {
        let (due, small_limit) = leaf_memory_due_at_cap(cap, from_space_in_use);
        (due, limit.min(small_limit))
    };
    YoungScavengeCapProbe {
        due: from_space_in_use >= cap || leaf_due,
        reading: Some(super::trigger_watermark::YoungArmReading {
            from_space_in_use,
            limit,
            inline_offset,
        }),
    }
}

/// #10169: does the young generation hold at least one BASE nursery cap of
/// bytes — a minor's worth of data, whatever the adaptive cap currently says?
/// Read by `string::json_leaf_prefers_arena` from inside a suppressed
/// construction window. The base cap rather than [`young_scavenge_cap_due`]'s
/// adaptive one on purpose: after a fully-live minor the adaptive cap scales
/// past the very tree that made it scale, and a gate keyed on it would flip
/// the next leaf back to the malloc registry, where one tracked leaf is enough
/// to veto the untraced promotion the route exists to enable.
pub(crate) fn young_generation_holds_a_nursery() -> bool {
    nursery_cap_active()
        && crate::arena::copying_from_space_in_use_bytes() >= gc_scavenge_nursery_cap_bytes()
}

/// The cap value [`young_scavenge_cap_due`] compares against.
///
/// Split out only so a test can make the cap due without allocating the real
/// 16 MB — a PER-THREAD override next to the reader, the shape support.rs
/// mandates for anything a test needs to move (never the process environment,
/// which is shared by every libtest thread; see #7946). It deliberately does
/// NOT feed `effective_next_arena_trigger`: this is about *dueness*, and a test
/// that also moved the trigger clamp would be changing two things at once.
fn scavenge_nursery_cap_dueness_bytes() -> usize {
    scavenge_nursery_cap_dueness_bytes_with(old_gen_reclaimable_pressure_bytes)
}

fn scavenge_nursery_cap_dueness_bytes_with(old_reclaimable: impl FnOnce() -> usize) -> usize {
    #[cfg(test)]
    if let Some(bytes) = GC_NURSERY_CAP_TEST_DUE_BYTES.with(TriggerInput::get) {
        return bytes;
    }
    let influx_driven = super::tenuring::ladder_nursery_cap_bytes();
    let old_reclaimable = old_reclaimable();
    debug_assert_eq!(
        old_reclaimable,
        old_gen_reclaimable_pressure_bytes(),
        "the old-gen pressure a due check reuses must still be the current value"
    );
    super::tenuring::scavenge_nursery_cap_from(influx_driven, old_reclaimable)
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`scavenge_nursery_cap_dueness_bytes`].
    static GC_NURSERY_CAP_TEST_DUE_BYTES: TriggerInput<Option<usize>> = const { TriggerInput::new(None) };
}

/// RAII override making the young-gen scavenge cap due at `bytes` of from-space
/// occupancy on this thread (#7909).
#[cfg(test)]
pub(super) struct ScavengeNurseryCapTestGuard {
    previous: Option<usize>,
}

#[cfg(test)]
impl ScavengeNurseryCapTestGuard {
    pub(super) fn due_at_bytes(bytes: usize) -> Self {
        let previous = GC_NURSERY_CAP_TEST_DUE_BYTES.with(|cell| {
            let previous = cell.get();
            cell.set(Some(bytes));
            previous
        });
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for ScavengeNurseryCapTestGuard {
    fn drop(&mut self) {
        GC_NURSERY_CAP_TEST_DUE_BYTES.with(|cell| cell.set(self.previous));
    }
}

/// Is the scavenge nursery cap in force?
///
/// **Only when the collection it schedules can EVACUATE**, which for nursery
/// pressure means only when `gc_moving_loop_polls_enabled()` routes it to a
/// precise-root safepoint. #7056's own 2x2 says the cap and the evacuating
/// minor "ship together, because either alone is a bad trade"; this is that
/// sentence made load-bearing rather than advisory, and #7682 is the bill for
/// its being advisory.
///
/// The cap's basis is `copying_from_space_in_use_bytes()`, and **a non-moving
/// minor does not reduce it** — it sweeps in place into per-block free lists
/// and from-space stays occupied. So a capped trigger that fires a non-moving
/// minor is due again the instant the next block is taken: one whole-arena
/// collection per 1 MB allocated, O(n^2) in the live set. That is not the
/// "+23% wall for -33% RSS" the cap-only cell of the 2x2 measured (every
/// collection there still evacuated) — measured on the quiet host after #7682
/// forced the alloc-point minor non-moving, `test_gap_gc_index_get_receiver_rooting`
/// went 0.66 s -> 6.6 s, and with the cap lifted it runs in 0.13 s. It is the
/// same livelock shape as #7592, whose fix was likewise to key a band on
/// something a collection actually moves.
///
/// So this restores the pre-#7056 gating, deliberately and with a different
/// argument than #7056 removed it under. #7056 decoupled the cap because both
/// gates were off in shipped builds and the cap was therefore dead — a fair
/// reading of a world in which the alloc-point minor evacuated. It no longer
/// does. When `PERRY_GC_MOVING_LOOP_POLLS` goes default-ON again the cap comes
/// back with it, automatically and in the configuration it was measured in.
fn nursery_cap_active() -> bool {
    #[cfg(test)]
    if GC_NURSERY_CAP_TEST_SUPPRESSED.with(TriggerInput::get) {
        return false;
    }
    gc_moving_loop_polls_enabled()
}

pub(super) fn effective_next_arena_trigger() -> usize {
    let base = next_arena_trigger_base();
    // A minor is O(live) — it copies ~1k live objects out of millions
    // allocated — so the 128 MB-and-doubling adaptive trigger, tuned for the
    // OLD world where a minor was an expensive O(heap) sweep and the advice was
    // "collect rarely", is exactly backwards. Capping the nursery small makes
    // collections fire often and keeps the young arena's high-water mark near
    // the cap instead of ballooning to 128–260 MB between the ~8 collections
    // the adaptive trigger otherwise allows.
    //
    // APPLIED WHEN THE COLLECTION IT SCHEDULES CAN EVACUATE — see
    // [`nursery_cap_active`], which is where that condition and its evidence
    // live. #7056 applied it unconditionally on the reading that the
    // alloc-point minor evacuated; since #7682 it does not, and a capped
    // trigger firing a non-moving minor is a livelock rather than a trade.
    //
    // Re-derived on the statepoint-default collector, 8 gc_ratchet probes,
    // as a full 2x2 rather than a single comparison — because the one-armed
    // version of this measurement says something false:
    //
    //                    no scavenge        scavenge
    //     no cap        799,604,736      799,604,736   (+0%)
    //     cap 16 MB     537,165,824      245,006,336
    //                        (-33%)           (-69%)
    //
    // Read the row and the column, not one cell. Scavenge ON ITS OWN buys
    // exactly nothing — the top row is identical to the byte — which is why an
    // isolation that only varies the cap *within* the scavenge-on world
    // concludes "the cap is the whole effect". It is not. The two INTERACT: the
    // cap makes collections fire often, and scavenge makes those collections
    // evacuating (O(live) copying) so the nursery is actually reclaimed rather
    // than merely swept.
    //
    // Both halves ship together, because either alone is a bad trade: the cap
    // alone costs +23% wall for -33% RSS, and scavenge alone moves nothing.
    // See `gc::gc_scavenge_enabled` for the full 2x2 and why they interact.
    //
    // Wall time was flat in aggregate across the same probes (2052 ms ->
    // 2032 ms) and every probe stayed byte-identical to the pinned Node
    // oracle.
    //
    // `PERRY_GC_SCAVENGE_NURSERY_MB` still tunes the value; it is a
    // measurement dial, not an on/off mode, so it needs no kill-policy arm.
    if !nursery_cap_active() {
        return base;
    }
    // The cap the clamp applies is the *effective* one: the configured base
    // times the influx-driven scale from gc/tenuring.rs, which grows it
    // (bounded, ×2 steps) on live-set-bound workloads where a fixed 16 MB
    // multiplies the per-collection fixed cost by an enormous collection
    // count. Small-live-set workloads never leave the base value.
    base.min(super::tenuring::scavenge_nursery_cap_effective_bytes())
}

/// Default base nursery high-water cap, in MiB. Named rather than inline
/// because `docs/src/internals/garbage-collector.md` documents it and
/// `scripts/check_gc_doc_claims.py` re-derives the documented number from this
/// definition.
pub(super) const SCAVENGE_NURSERY_CAP_DEFAULT_MB: usize = 16;

/// Nursery high-water cap used only when `PERRY_GC_SCAVENGE` is on (default
/// [`SCAVENGE_NURSERY_CAP_DEFAULT_MB`]; override with
/// `PERRY_GC_SCAVENGE_NURSERY_MB`). See `effective_next_arena_trigger`.
pub(super) fn gc_scavenge_nursery_cap_bytes() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        std::env::var("PERRY_GC_SCAVENGE_NURSERY_MB")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|&mb| mb > 0)
            .unwrap_or(SCAVENGE_NURSERY_CAP_DEFAULT_MB)
            .saturating_mul(1024 * 1024)
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only suppression of the nursery cap, so `force_legacy_gc_pacing`
    /// can restore genuinely legacy pacing.
    ///
    /// The cap used to hang off `gc_moving_loop_polls_enabled()`, so pinning
    /// that flag off was enough to un-cap the trigger. It is unconditional now
    /// (#7056), which silently broke that escape hatch: 22 `gc::tests` that
    /// legitimately assert raw-cell trigger arithmetic started failing against
    /// the capped value. The guard has to suppress the cap directly.
    static GC_NURSERY_CAP_TEST_SUPPRESSED: TriggerInput<bool> = const { TriggerInput::new(false) };
}

crate::perry_thread_local! {
    /// Lower bound for the next GC trigger. Bumped after each
    /// `gc_collect_inner` based on collection effectiveness (see the
    /// adaptive logic in `gc_check_trigger`).
    ///
    /// The initial value is `GC_THRESHOLD_INITIAL_BYTES` (128MB —
    /// chosen so that the 96MB working set of a 1M-iter object_create
    /// or binary_trees benchmark fits under the threshold and pays
    /// zero GC cost). After every collection, if the sweep freed >75%
    /// of arena bytes, the per-program "step" is doubled (capped at
    /// 1GB) so subsequent allocation bursts don't pay GC overhead just
    /// because they re-cross the same line. For hot `new ClassName()`
    /// loops where every object dies between GC cycles, this means
    /// the FIRST burst pays for at most one collection and the rest
    /// run GC-free.
    ///
    /// If a sweep frees <25%, the step is halved (down to a 16MB
    /// floor) so live-set-bound programs don't grow their working
    /// set unboundedly between collections.
    pub(super) static GC_NEXT_TRIGGER_BYTES: TriggerInput<usize> =
        const { TriggerInput::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Whether GC_NEXT_TRIGGER_BYTES has been explicitly set on this thread
    /// (re-arm after a collection, parse bump, tiny-parse lowering). While
    /// false the cell still holds the desktop-default const initializer and
    /// `effective_next_arena_trigger` substitutes the device-derived ceiling
    /// instead — an ARMED trigger above the ceiling is legitimate (big live
    /// set headroom floor, medium-parse bumps) and must not be clamped.
    pub(super) static GC_TRIGGER_ARMED: TriggerInput<bool> =
        const { TriggerInput::new(false) };

    /// Per-program adaptive GC step. Doubles (up to MAX) when sweeps
    /// are mostly-garbage; halves (down to 16MB) when sweeps reclaim
    /// little. Used to compute the next trigger after each GC as
    /// `post_total + step`.
    pub(super) static GC_STEP_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Lower bound for the next malloc-count-based GC trigger. After each
    /// collection, this is reset to `survivor_count + GC_MALLOC_COUNT_STEP`
    /// so that programs with large legitimate live sets (>10k tracked
    /// malloc objects) don't GC-thrash on every subsequent allocation.
    /// See `gc_check_trigger` for the update rule.
    pub(super) static GC_NEXT_MALLOC_TRIGGER: TriggerInput<usize> =
        const { TriggerInput::new(100_000) };

    /// Issue #745: track whether a medium-or-larger parse already
    /// raised `GC_NEXT_TRIGGER_BYTES` this GC cycle. Cleared in
    /// `gc_collect_inner` whenever a real collection runs.
    pub(super) static GC_TRIGGER_BUMPED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// Issue #745: snapshot of `arena_total_bytes()` at the most
    /// recent `gc_suppress` call. Used by `gc_bump_malloc_trigger`
    /// to compute the suppressed window's arena growth.
    pub(super) static GC_PRE_SUPPRESS_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };

    /// Non-generational full GC cannot compact a block that still contains
    /// the just-returned parse result. When tiny parse churn crosses the
    /// in-use pressure guard, collect at the next parse boundary instead of
    /// immediately after the current parse, so the previous result has had a
    /// chance to fall out of the shadow roots.
    pub(super) static GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// Tiny final-object births are bounded, so polling the whole arena after
    /// every one spends more time measuring pressure than allocating. Keep a
    /// strict call-count bound on that polling delay instead.
    #[cfg(not(test))]
    static GC_TINY_PARSE_BOUNDARY_POLL_REMAINING: std::cell::Cell<u8> =
        const { std::cell::Cell::new(GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL - 1) };
}

#[derive(Clone, Copy)]
pub(super) enum GcCollectionKind {
    Minor,
    Full,
}

impl GcCollectionKind {
    #[cfg(any(perry_diagnostics, test))]
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            GcCollectionKind::Minor => "minor",
            GcCollectionKind::Full => "full",
        }
    }

    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            GcCollectionKind::Minor => 1,
            GcCollectionKind::Full => 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum GcTriggerKind {
    ArenaBytes,
    MallocCount,
    OldGenBytes,
    SurvivorPromotionBytes,
    Emergency,
    Manual,
    Direct,
    /// The idle-time reclaim (`gc/idle_reclaim.rs`): a full started at the
    /// event loop's park site because the mutator went quiet, not because any
    /// allocation threshold was crossed.
    IdleReclaim,
    /// The idle-time compaction (`gc/idle_compact.rs`): a synchronous moving
    /// minor with old-page defrag selected, started at the park site because
    /// the old-gen free list has grown past what a non-moving sweep can give
    /// back.
    IdleCompact,
}

impl GcTriggerKind {
    #[cfg(any(perry_diagnostics, test))]
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            GcTriggerKind::ArenaBytes => "arena_bytes",
            GcTriggerKind::MallocCount => "malloc_count",
            GcTriggerKind::OldGenBytes => "old_gen_bytes",
            GcTriggerKind::SurvivorPromotionBytes => "survivor_promotion_bytes",
            GcTriggerKind::Emergency => "emergency",
            GcTriggerKind::Manual => "manual",
            GcTriggerKind::Direct => "direct",
            GcTriggerKind::IdleReclaim => "idle_reclaim",
            GcTriggerKind::IdleCompact => "idle_compact",
        }
    }

    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            GcTriggerKind::ArenaBytes => 1,
            GcTriggerKind::MallocCount => 2,
            GcTriggerKind::OldGenBytes => 3,
            GcTriggerKind::SurvivorPromotionBytes => 4,
            GcTriggerKind::Manual => 5,
            GcTriggerKind::Direct => 6,
            GcTriggerKind::Emergency => 7,
            GcTriggerKind::IdleReclaim => 8,
            GcTriggerKind::IdleCompact => 9,
        }
    }

    #[inline]
    pub(super) const fn progress_kind(self, collection_kind: GcCollectionKind) -> GcProgressKind {
        match (self, collection_kind) {
            (GcTriggerKind::Emergency, GcCollectionKind::Full) => GcProgressKind::EmergencyFull,
            (GcTriggerKind::Manual, GcCollectionKind::Full) => GcProgressKind::ExplicitFull,
            (GcTriggerKind::Manual, GcCollectionKind::Minor) => GcProgressKind::ExplicitSynchronous,
            (
                GcTriggerKind::ArenaBytes
                | GcTriggerKind::MallocCount
                | GcTriggerKind::OldGenBytes
                | GcTriggerKind::SurvivorPromotionBytes
                | GcTriggerKind::Emergency
                | GcTriggerKind::Direct
                | GcTriggerKind::IdleReclaim
                | GcTriggerKind::IdleCompact,
                _,
            ) => GcProgressKind::LegacySynchronous,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DeferredGcRequest {
    None,
    CheckTrigger,
    DirectMinor,
    Collect(GcTriggerKind),
}

impl DeferredGcRequest {
    #[inline]
    pub(super) fn merge(self, next: DeferredGcRequest) -> DeferredGcRequest {
        use DeferredGcRequest::*;
        match (self, next) {
            (None, request) => request,
            (request, None) => request,
            (Collect(GcTriggerKind::Manual), _) | (_, Collect(GcTriggerKind::Manual)) => {
                Collect(GcTriggerKind::Manual)
            }
            (Collect(kind), _) => Collect(kind),
            (_, Collect(kind)) => Collect(kind),
            (DirectMinor, _) | (_, DirectMinor) => DirectMinor,
            (CheckTrigger, CheckTrigger) => CheckTrigger,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(perry_diagnostics), allow(dead_code))]
pub(super) struct GcStepSnapshot {
    pub(super) arena_step_bytes: usize,
    pub(super) next_arena_trigger_bytes: usize,
    pub(super) malloc_step: usize,
    pub(super) next_malloc_trigger: usize,
    pub(super) trigger_bumped: bool,
}

impl GcStepSnapshot {
    #[inline]
    pub(super) fn current() -> Self {
        Self {
            arena_step_bytes: GC_STEP_BYTES.with(|c| c.get()),
            next_arena_trigger_bytes: GC_NEXT_TRIGGER_BYTES.with(|c| c.get()),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|c| c.get()),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|c| c.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|c| c.get()),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GcTriggerSnapshot {
    pub(super) kind: GcTriggerKind,
    pub(super) steps_before: Option<GcStepSnapshot>,
}

impl GcTriggerSnapshot {
    #[inline]
    pub(super) fn capture(kind: GcTriggerKind) -> Self {
        Self {
            kind,
            steps_before: gc_trace_enabled().then(GcStepSnapshot::current),
        }
    }
}

crate::perry_thread_local! {
    pub(super) static GC_DEFERRED_REQUEST: Cell<DeferredGcRequest> =
        const { Cell::new(DeferredGcRequest::None) };
    pub(super) static GC_OLD_RECLAIM_PENDING: TriggerInput<bool> = const { TriggerInput::new(false) };
    /// #10169: a document-sized JSON leaf was born old under young pressure
    /// since the last trigger decision (`note_young_leaf_born_old`).
    pub(super) static GC_YOUNG_LEAF_BORN_OLD: TriggerInput<bool> = const { TriggerInput::new(false) };
    pub(super) static GC_LAST_OLD_RECLAIM_IN_USE_BYTES: TriggerInput<usize> = const { TriggerInput::new(0) };
    /// Reclaimable old bytes measured right after the last FULL
    /// mark-sweep — the baseline for major-GC pacing
    /// (`arena_growth_full_escalation_due`).
    pub(super) static GC_LAST_FULL_OLD_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Reclaimable old bytes measured when the last FULL mark-sweep STARTED.
    /// Paired with `GC_LAST_FULL_OLD_IN_USE_BYTES` to price what that full
    /// actually reclaimed — see `GC_MAJOR_PACING_BACKOFF_SHIFT`.
    pub(super) static GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Reclaimable old bytes at the end of the most recent collection.
    /// Retained nursery cohorts still belong to minors; neither their live
    /// census nor their block-rounded capacity can escalate an old trace.
    pub(super) static GC_LAST_COLLECTION_POST_OLD_IN_USE_BYTES: Cell<usize> =
        const { Cell::new(0) };
    /// #9831: `arena_in_use_bytes()` as the most recent collection of ANY kind
    /// ended — the base the tiny-parse pressure guard measures growth from.
    /// The bump-offset reading rather than the live census above, because the
    /// guard compares against `arena_in_use_bytes()` at every parse boundary,
    /// and mixing the two would count every swept hole as growth.
    pub(super) static GC_TINY_PARSE_PRESSURE_BASE_BYTES: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static GC_TINY_PARSE_PRESSURE_TEST_FORCE: Cell<bool> = const { Cell::new(false) };
    /// Yield-adaptive backoff for major-GC pacing (#7726).
    ///
    /// `arena_growth_full_escalation_due` escalates a minor to a full once the
    /// arena's live bytes pass K× the last full's live set. On a workload whose
    /// live set genuinely GROWS — every record retained, nothing to reclaim —
    /// that gate fires on the growth itself and buys nothing: measured on
    /// `gc-handoff/bench/retain.ts`, the two escalated fulls cost 644 ms of a
    /// 1.31 s run and moved arena in-use by 4 MB total, the second one by zero.
    ///
    /// So price each full by what it reclaimed and shift K left when the answer
    /// is "almost nothing". A churn workload's fulls reclaim most of the heap,
    /// keep the shift at 0, and pace exactly as before. The shift is capped and
    /// resets on the first productive full, and it does not touch the
    /// `OldReclaim` escalation — old-gen garbage still forces a full through
    /// `old_reclaim_pressure_due` regardless of this backoff.
    pub(super) static GC_MAJOR_PACING_BACKOFF_SHIFT: Cell<u32> = const { Cell::new(0) };
    /// #10928: pre-full LIVE reading for the old-reclaim full that is about to
    /// run, recorded by `old_reclaim_pressure_due` itself so a future call site
    /// cannot forget to. Zero means "no start reading was taken", exactly as
    /// `GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES` uses it.
    pub(super) static GC_OLD_RECLAIM_PRE_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// #10928: how far left to shift the old-reclaim growth band because the
    /// preceding old-reclaim fulls were unproductive. Reset to 0 by the first
    /// productive one. Capped at `OLD_RECLAIM_BACKOFF_SHIFT_MAX`.
    pub(super) static GC_OLD_RECLAIM_BACKOFF_SHIFT: TriggerInput<u32> = const { TriggerInput::new(0) };
    /// #10960: live bytes the PREVIOUS priced full left behind, so
    /// `update_old_reclaim_backoff` can tell a futile full (it re-traced the
    /// same live set) from one that found the live set GROWING. Zero before
    /// the first full, which reads as "everything this full found is new".
    pub(super) static GC_OLD_RECLAIM_LAST_POST_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Survival-adaptive arm of major-GC pacing: `true` once a copying minor
    /// has measured a young-survival ratio at or above
    /// `MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE`, cleared by any minor that
    /// measures less. See `MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER`.
    ///
    /// Deliberately the LAST minor's verdict rather than a running maximum: a
    /// heap that stops retaining must pace tightly again on its very next
    /// collection, not after a decay window.
    pub(super) static GC_MAJOR_PACING_RETAINING: TriggerInput<bool> = const { TriggerInput::new(false) };
    /// Re-entrancy guard for the #5476 direct old-gen reclaim driven from
    /// `gc_check_trigger`: the full collection must not recursively trigger
    /// another reclaim if a hook it runs allocates.
    pub(super) static GC_OLD_RECLAIM_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
    /// #7592: a survivor-promotion handoff full has run and the copying minor
    /// it was scheduled for has not. See
    /// `survivor_promotion_handoff_awaiting_minor`.
    static SURVIVOR_HANDOFF_AWAITING_MINOR: Cell<bool> = const { Cell::new(false) };
    /// Count of handoffs the latch has suppressed — see
    /// `survivor_promotion_handoff_suppressions`.
    static SURVIVOR_HANDOFF_SUPPRESSIONS: Cell<u64> = const { Cell::new(0) };
    /// Phase 2/3 of the moving-GC project: set when an alloc-point nursery
    /// trigger fires while moving mode is on, deferring the collection to the
    /// next precise-root safepoint (event-loop boundary or a codegen loop
    /// back-edge poll) so the copying minor can MOVE survivors instead of the
    /// conservative non-moving minor running mid-expression.
    ///
    /// **Write it only through [`super::set_safepoint_pending`].** The poll's
    /// fast path cannot afford to read a thread-local (on Darwin that is a call
    /// to `_tlv_get_addr`), so this `Cell` has a process-global shadow —
    /// `gc::poll_arm::PERRY_GC_POLL_ARMED` — that codegen loads inline to decide
    /// whether the poll is worth calling. A `set` that bypasses the helper
    /// leaves the shadow reading zero, and a deferred collection whose drain
    /// point has been optimised away is stranded until the next event-loop
    /// boundary.
    pub(super) static GC_SAFEPOINT_PENDING: Cell<bool> = const { Cell::new(false) };
    /// `arena_total_bytes()` sampled at the moment `GC_SAFEPOINT_PENDING` was
    /// last set — the baseline the deferral slack is measured from (#7024).
    /// Meaningless while `GC_SAFEPOINT_PENDING` is false.
    pub(super) static GC_SAFEPOINT_DEFER_ARENA_BASE: Cell<usize> = const { Cell::new(0) };
    /// #11873: the old-reclaim pressure (`old_reclaim_in_use_bytes`) at the
    /// allocation point that deferred an owed old-generation reclaim to the next
    /// precise safepoint, or `None` when no such deferral is outstanding. The
    /// slack the allocation point grants before it collects itself is measured
    /// from here. Cleared by every completed full (`finish_full_old_reclaim_baseline`).
    pub(super) static GC_OLD_RECLAIM_DEFER_BASE: Cell<Option<usize>> = const { Cell::new(None) };
    /// Set by [`js_gc_old_reclaim_valve_test_override`]: the next allocation
    /// point that finds an old reclaim owed takes the valve. Consumed there.
    static GC_OLD_RECLAIM_VALVE_TEST_OVERRIDE: Cell<bool> = const { Cell::new(false) };
    /// True while a DECLARED safepoint drain is running: a loop back-edge
    /// poll, the outermost microtask-pump moving minor, or an explicit
    /// `gc()`. Consumed by the `PERRY_GC_SAFEPOINT_ONLY` contract assert in
    /// the root-scan subphase.
    pub(super) static GC_AT_DECLARED_SAFEPOINT: Cell<bool> = const { Cell::new(false) };
}

#[inline]
pub(super) fn defer_gc_request(request: DeferredGcRequest) -> bool {
    let locked = GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0);
    if locked {
        GC_DEFERRED_REQUEST.with(|pending| {
            pending.set(pending.get().merge(request));
        });
    }
    locked
}

/// Whether a root lock is held on this thread, i.e. a collection started now
/// would have to be deferred. The idle compaction asks before it runs one: it
/// is synchronous and moving, so there is no deferral path for it to take.
pub(super) fn gc_root_lock_held() -> bool {
    GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
}

pub(super) fn take_deferred_gc_request() -> DeferredGcRequest {
    GC_DEFERRED_REQUEST.with(|pending| {
        let request = pending.get();
        pending.set(DeferredGcRequest::None);
        request
    })
}

pub(super) fn flush_deferred_gc_request() {
    if std::thread::panicking() {
        let _ = take_deferred_gc_request();
        return;
    }
    match take_deferred_gc_request() {
        DeferredGcRequest::None => {}
        DeferredGcRequest::CheckTrigger => gc_check_trigger(),
        DeferredGcRequest::DirectMinor => {
            if gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
                .emit_after_current();
        }
        DeferredGcRequest::Collect(GcTriggerKind::Manual) => {
            if manual_gc_blocked_by_unsafe_zone() {
                return;
            }
            manual_gc_collect_now();
        }
        DeferredGcRequest::Collect(kind) => {
            if gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(kind)).emit_after_current();
        }
    }
}

/// Check if automatic GC pressure should pay a bounded assist step.
///
/// Arena and malloc thresholds are heap goals. Crossing them starts or resumes
/// a budgeted cycle. Allocation-side assists spend at most
/// `GC_MUTATOR_ASSIST_WORK_UNITS` and only enter phases that already consume
/// that budget; unsliced phases stay active for host-driven budgeted steps.
///
/// #10698: this runs on every `gc_malloc`, and nearly every call finds
/// nothing due. When the last full evaluation found nothing due and none of
/// its inputs has changed since, two loads against the watermark it
/// published answer the question; see `gc/trigger_watermark.rs` for why that
/// answer is the ladder's own.
///
/// Out of line, as it always was in practice: the arena slow path calls it
/// from `arena_cell_alloc`, which is inlined into every arena allocation, and
/// the fast path buys nothing there. `gc_malloc` inlines
/// [`gc_check_trigger_inlined`] instead.
#[inline(never)]
pub fn gc_check_trigger() {
    gc_check_trigger_inlined();
}

/// A budgeted minor holds IN_ALLOC for the whole cycle, including mutator
/// windows. Exclude only actual collector work from allocation debt.
#[inline(always)]
pub(super) fn allocation_pacing_collector_active() -> bool {
    GC_BUDGETED_STEP_ACTIVE.with(Cell::get) || super::allocation_pacing::collector_step_active()
}

/// [`gc_check_trigger`] with its fast path inlined into the caller, for
/// `gc_malloc` — the one caller that runs it once per allocation.
#[inline(always)]
pub(super) fn gc_check_trigger_inlined() {
    if super::trigger_watermark::nothing_due() {
        #[cfg(test)]
        super::trigger_watermark::verify_nothing_due();
        return;
    }
    gc_check_trigger_evaluate();
}

#[inline(never)]
fn gc_check_trigger_evaluate() {
    if GC_BUDGETED_STEP_ACTIVE.with(Cell::get) {
        return;
    }
    // Issue #62: single TLS access covers both `in_alloc` and `suppressed`.
    let flags = GC_FLAGS.with(|f| f.get());
    if flags & GC_FLAG_SUPPRESSED != 0 {
        // Owed, not answered: the outermost `GcSuppressScope` runs it on drop.
        GC_FLAGS.with(|f| f.set(flags | GC_FLAG_TRIGGER_OWED));
        return;
    }
    if !gc_budgeted_cycle_active() && flags & GC_FLAG_IN_ALLOC != 0 {
        return;
    }
    if gc_blocked_by_unsafe_zone() {
        return;
    }
    if defer_gc_request(DeferredGcRequest::CheckTrigger) {
        return;
    }

    // This function asks "what is due?" up to three times on the path where
    // nothing is (the old-reclaim arm, the nursery arm, the assist gate). Every
    // arm that acts returns, so the state each later question sees is the
    // state the first one saw, and a repeatable answer can be reused. An
    // answer from the #10169 flag branch is re-evaluated, exactly as before.
    //
    // #10698: an evaluation that finds nothing due also says how long that
    // answer holds. It is published only on the path below that returns
    // without acting, which is where the next call's fast path stands in.
    let nothing_due_watermark = Cell::new(TriggerWatermark::RETIRED);
    let mut due_memo = DueTriggerMemo::new();
    let mut due = || {
        due_memo.get(|| {
            let (due, repeatable, watermark) = gc_budgeted_due_trigger_probe();
            nothing_due_watermark.set(watermark);
            (due, repeatable)
        })
    };

    // #5476: a workload that churns *large* temporaries (>16 KB, born directly
    // in the old arena) grows the old generation without ever exercising the
    // nursery, and a compute-only loop never reaches the event loop. So when
    // old-gen reclaim pressure is due at an allocation point, the reclaim must
    // complete without waiting for event-loop IO.
    //
    // #11873: it no longer runs HERE. An allocation point is not a precise
    // point: native runtime frames between the compiled caller and the
    // allocation hold GC values in Rust locals (a half-built error object, a
    // stream's options, the arguments the compiled caller passed), and nothing
    // roots them. Running the full here was sound only because it forced a
    // conservative scan of the whole native stack, which cost ~1.7% of tsc's
    // instructions (235 of 236 fulls came through here) and made the number
    // of fulls depend on stack layout and ASLR. Measured with the poisoning
    // audit (#11873): collecting here without the scan reclaims objects still
    // named by native frames in 39 of 496 gap tests.
    //
    // So the reclaim is deferred to the next precise safepoint, exactly as the
    // nursery arm below defers its minor: `GC_OLD_RECLAIM_PENDING` stays set,
    // the poll is armed, and `gc_safepoint_moving_minor` runs the full with
    // precise roots at the next loop back-edge poll or microtask-pump
    // boundary. A compute-only loop reaches a back-edge poll, so its reclaim
    // still completes without the event loop. Native frames live at that
    // point are the ones that called user code, which already must keep their
    // values in handles across it (a moving minor runs at the same points).
    //
    // The allocation point collects itself only as a bounded valve: once old
    // pressure has grown another growth band past the deferral point without
    // a safepoint (a synchronous recursion or a single mega-call that reaches
    // no poll). There is no precise point to defer to then, and the native
    // frames are exactly as described above, so the valve keeps the
    // conservative scan; `ConservativeScanSite::OldReclaimAllocPoint` counts
    // it. With loop polls off (`PERRY_GC_MOVING_LOOP_POLLS=0`) nothing drains
    // a deferral, and every reclaim takes the valve, as before.
    if !gc_budgeted_cycle_active()
        && matches!(due(), Some(BudgetedGcTrigger::OldReclaim))
        && !GC_OLD_RECLAIM_IN_PROGRESS.with(Cell::get)
        && !super::schedule::budgeted_old_reclaim_forced()
    {
        old_reclaim_at_allocation_point();
        return;
    }

    // The NURSERY-churn triggers (ArenaBytes / MallocCount) have the same
    // hole #5476 patched for OldReclaim: whenever synchronous-only root
    // scanners are registered — every compiled program, since codegen
    // registers sync scanners at startup — `gc_budgeted_start_blocked()`
    // holds for the life of the process, the mutator-assist step below can
    // never START a cycle, and allocation pressure accumulates without
    // bound (probe: 4M small allocations → 1.9 GB RSS with ZERO collection
    // cycles; the 64 MB arena trigger was due after the first ~64 blocks
    // and simply never fired). When the budgeted machinery is structurally
    // unavailable, run the direct synchronous minor the pre-budgeted
    // block-alloc trigger used to run, then re-baseline the arming trigger
    // below (the budgeted finisher never runs on this arm).
    // `gc_collect_minor_with_trigger` carries its own re-entrancy guard
    // (GC_FLAG_IN_ALLOC). `force_full_scan` mirrors the OldReclaim
    // arm: at an arbitrary allocation point a value mid-construction may
    // live only in registers, so the conservative native scan retains it —
    // which also makes copied-minor ineligible for THIS cycle, so the
    // non-moving minor runs (no relocation hazards at alloc points).
    // PERRY_GC_SCAVENGE (Phase-1 de-risking, OFF by default): when the budgeted
    // stepper is NOT blocked (all scanners budgeted), the nursery-churn triggers
    // fall through to the budgeted mutator-assist step below, which is
    // deliberately non-moving (`low_pause_non_moving = is_budgeted()`), so a
    // reallocation-heavy loop's minors free nothing. Route those triggers to the
    // direct (non-budgeted, atomic) minor here instead, so the collection is an
    // atomic minor that actually reclaims rather than a budgeted step that
    // does not. It is NOT an evacuating one: since #7682 the guard below is
    // unconditional, so a collection that happens here is always non-moving.
    // Scavenge is a PACING knob and nothing more; it used to also skip that
    // guard, which is the bug.
    // `gc_moving_loop_polls_enabled()`: the SOUND moving-nursery path. When loop
    // polls are on, entering this block routes nursery pressure AWAY from the
    // budgeted non-moving stepper (which would otherwise own it and free nothing
    // on reallocation loops) and into the defer arm below, which sets
    // GC_SAFEPOINT_PENDING and returns — the collection then runs as an
    // evacuating MOVING minor at the next precise loop back-edge safepoint
    // (`js_gc_loop_safepoint` → `gc_safepoint_moving_minor`), NOT here at the
    // register-imprecise alloc point. That deferral is the ONLY route by which
    // nursery pressure becomes a moving collection, and it is why the polls
    // flag and the scavenge flag are not interchangeable.
    //
    // #7280: that used to read "so it is sound by construction". IT IS NOT, and
    // the overclaim is the kind that stops the next person looking. What
    // deferring to `js_gc_loop_safepoint` buys is precise *codegen* roots — the
    // loop body has completed, so every live value the COMPILED frame holds is a
    // named local on the shadow stack. It buys nothing for a value parked in a
    // RUNTIME (Rust) frame, which the precise walk does not visit at all: no
    // shadow slot, no temp root, no registered scanner. A back-edge poll that
    // fires while `js_new_function_construct` is midway through a user
    // constructor body relocates the instance that helper is holding in a plain
    // `let`, and no safepoint's root set covers it. That was measured, not
    // argued — four reproducers in
    // `tests/parity/language/class/test_gap_gc_dynamic_construct_receiver_rooting.ts`, 200/200
    // iterations wrong per route before the `RuntimeHandleScope` routing in
    // `object/class_registry/construct.rs`. The correct statement is: the
    // loop-polls route makes the COLLECTION POINT precise; keeping runtime
    // frames rooted across it is a separate obligation, discharged by
    // `RuntimeHandleScope`, and every runtime helper that calls back into user
    // JS owes it.
    if !gc_budgeted_cycle_active()
        && (super::gc_scavenge_enabled()
            || gc_moving_loop_polls_enabled()
            || super::roots::registered_root_scanners_block_budgeted_gc())
    {
        let direct_kind = match due() {
            // #7909: `YoungScavengeCap` is a nursery-churn trigger exactly like
            // `ArenaBytes` here — this arm's whole job is to route nursery
            // pressure to a collection that can actually reclaim it, so the two
            // must not diverge at THIS site. They diverge only at the budgeted
            // stepper's start decision.
            Some(BudgetedGcTrigger::ArenaBytes | BudgetedGcTrigger::YoungScavengeCap) => {
                Some(GcTriggerKind::ArenaBytes)
            }
            Some(BudgetedGcTrigger::MallocCount) => Some(GcTriggerKind::MallocCount),
            _ => None,
        };
        if let Some(kind) = direct_kind {
            if matches!(kind, GcTriggerKind::ArenaBytes) && super::json_defer::should_defer(false) {
                defer_nursery_cap_to_precise_safepoint();
                return;
            }
            // Phase 2/3: with moving mode on, DEFER this alloc-point collection
            // to the next precise-root safepoint (event-loop boundary or a
            // codegen loop back-edge poll) so the copying minor MOVES survivors
            // instead of the conservative non-moving minor running here at a
            // register-imprecise point. Safety valve: once the arena has grown
            // `gc_moving_defer_slack_dyn_bytes()` PAST the point at which the
            // collection was deferred (a mega-expression that reached no poll),
            // fall through and collect non-moving here so growth stays bounded.
            //
            // #7024: the allowance is measured from the deferral point, not
            // against an absolute arena size. The absolute cap shared
            // `budget_scaled(_, 1, 4, 2 MB)` with the trigger ceiling, so under
            // an explicit PERRY_GC_HEAP_LIMIT "a trigger is due" and "the
            // deferral is allowed" became exact complements and this branch was
            // dead — see `GC_MOVING_DEFER_SLACK_BYTES`.
            if gc_moving_loop_polls_enabled() {
                let arena_total = crate::arena::arena_total_bytes();
                let already_deferred = GC_SAFEPOINT_PENDING.with(Cell::get);
                let deferred_at =
                    already_deferred.then(|| GC_SAFEPOINT_DEFER_ARENA_BASE.with(Cell::get));
                if moving_defer_within_slack(
                    arena_total,
                    deferred_at,
                    gc_moving_defer_slack_dyn_bytes(),
                ) {
                    if !already_deferred {
                        GC_SAFEPOINT_DEFER_ARENA_BASE.with(|base| base.set(arena_total));
                        set_safepoint_pending(true);
                    }
                    return;
                }
                // The deferral never drained. The direct minor below IS the
                // collection that was owed, so retire the request — leaving it
                // pending would pin `GC_SAFEPOINT_DEFER_ARENA_BASE` at a stale,
                // already-exceeded baseline and disable deferral for the rest of
                // the process (the same "the branch is dead" shape as #7024).
                set_safepoint_pending(false);
            }
            let pre_in_use = crate::arena::arena_in_use_bytes();
            let pre_malloc_count = malloc_object_count();
            super::diagnostic_hooks::site_trigger_decision("alloc_point_slack", "nursery");
            let probe = super::diagnostic_hooks::CollectionCharge::begin();
            // THE ALLOC POINT IS REGISTER-IMPRECISE, SO THIS MINOR MUST NOT
            // MOVE. Unconditional, and the unconditionality is the fix for
            // #7682.
            //
            // Reaching this line means the collection is happening HERE, at an
            // arbitrary allocation point inside a half-built expression — not
            // at a declared safepoint. Neither root lowering describes that
            // point: the shadow stack only names values codegen has already
            // stored to a slot, and RS4GC only relocates values it can type as
            // `ptr addrspace(1)`, which a NaN-boxed `double` operand in an SSA
            // register is not. A value that exists ONLY in a register here is
            // therefore invisible to both, so an evacuating minor relocates the
            // object and leaves the register naming the pre-move address. The
            // conservative native-stack scan is what covers exactly that gap:
            // it retains such values AND makes the copying minor ineligible
            // (`CopiedMinorFallbackReason::ConservativeStack`), so the
            // non-moving in-place minor runs and nothing relocates.
            //
            // ★ #7148 disposition: **keep as the bounded valve, now counted.**
            // The deferral above is the primary path and makes the collection
            // point precise; reaching here means the slack expired without the
            // program touching a single loop back-edge poll or microtask-pump
            // boundary — a mega-expression, or a synchronous recursion, that
            // allocated `gc_moving_defer_slack_dyn_bytes()` past the deferral
            // point. There is no safepoint to defer to in that state, so the
            // answer to "what if pressure spikes before a safepoint is
            // reached" is: this arm runs, and it is the reason RSS stays
            // bounded. Making it *imprecise* instead (collecting without the
            // scan) is the one thing #7148 rules out — it would trade a cost
            // problem for a soundness problem.
            //
            // #7682 is that trade, shipped. `PERRY_GC_SCAVENGE` used to gate
            // this guard off, on the strength of a doc comment claiming the
            // flag was "OFF by default … for measurement only" and a body
            // comment saying it also "defers alloc-point collections to a
            // precise safepoint". Neither held in the shipped configuration:
            // the flag has been ON by default since #7056, and the deferral
            // above is gated on `gc_moving_loop_polls_enabled()`, which is OFF
            // by default since #7161. So the default build collected — and
            // EVACUATED — right here, with no scan and no deferral. A
            // tree-walking interpreter (`test_gap_gc_alloc_point_no_move.ts`)
            // then read a relocated heap string out of a stale register and
            // silently returned the wrong number.
            //
            // The scan-skip cannot be recovered by asking "is scavenge on?":
            // that question is about pacing, and the precondition being
            // asserted here is about the PRECISION OF THIS PROGRAM POINT,
            // which no pacing knob can change. Scavenge keeps its other job —
            // routing nursery-churn triggers to this direct minor instead of
            // the budgeted non-moving stepper — and the moving minor keeps
            // running at the precise safepoints, where the root set is real.
            let _scan = super::roots::ManualGcScanGuard::force_full_scan(
                super::ConservativeScanSite::NurseryChurnSlackValve,
            );
            let outcome = super::gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(kind));
            // Re-baseline the arming trigger after the direct minor, mirroring
            // `gc_finish_budgeted_cycle`. This arm is taken whenever
            // synchronous-only root scanners block the budgeted stepper — i.e.
            // every compiled program — so it, not the budgeted finisher, is the
            // completion path for nursery collections there. Emitting the
            // outcome without re-baselining left `GC_NEXT_TRIGGER_BYTES` /
            // `GC_NEXT_MALLOC_TRIGGER` at the value that armed THIS collection.
            // The non-moving minor reclaims dead objects into per-block free
            // lists but does not lower `arena_total` (committed blocks), so a
            // workload holding a large live set above the trigger — e.g.
            // building an object graph that stays reachable while churning
            // transient allocations — keeps `gc_budgeted_due_trigger` reporting
            // the same trigger as due, and every fresh block re-arms a whole-
            // arena mark/sweep. That is one O(arena) collection per block
            // allocated: O(n^2) in the graph size, a ~100% CPU stall with a
            // bounded live set that never makes progress. The finish helpers
            // raise the trigger past the retained set (adapting the step),
            // exactly as the budgeted and full-GC paths do on completion.
            match kind {
                GcTriggerKind::MallocCount => {
                    gc_finish_malloc_trigger_collection(pre_malloc_count, pre_in_use, outcome);
                }
                _ => {
                    gc_finish_arena_trigger_collection(pre_in_use, outcome);
                }
            }
            probe.end(0, super::diag_sites::ChargeKind::DirectMinor);
            return;
        }
    }

    if !gc_budgeted_cycle_active() && due().is_none() {
        super::trigger_watermark::publish_trigger_watermark(nothing_due_watermark.get());
        return;
    }

    let units = gc_mutator_assist_scaled_work_units();
    let probe = super::diagnostic_hooks::CollectionCharge::begin();
    let _ =
        gc_mutator_assist_step_work_units_inner_with_progress(units, GcProgressKind::MutatorAssist);
    probe.end(units, super::diag_sites::ChargeKind::Assist);
}

/// Debt-proportional assist pacing (#6180 Stage 2, measured 2026-07-10).
///
/// A FIXED per-assist budget lets a tight allocation loop outrun the
/// collector: on a 10M-allocation ring benchmark the budgeted cycle NEVER
/// completed (0 collections vs the synchronous default's 7) and RSS grew
/// unbounded (6-22× the synchronous collector's) — the cycle crawled at 256
/// units per arena-block allocation against a heap growing by ~16k objects
/// per block. `GcDebtSnapshot` already measured exactly this shortfall but
/// fed telemetry only.
///
/// Scale the budget linearly with the measured debt instead. Debt is how far
/// allocation has run past the armed triggers, so between two block-alloc
/// assists it grows by ~one block while the budget grows with total debt —
/// the controller self-stabilizes at the equilibrium where collection keeps
/// pace with allocation, instead of falling behind forever.
///
/// No explicit cap is needed: the budget is a CEILING on work, not a pause
/// floor — `GcCycleState::step` stops the moment the cycle completes, and a
/// cycle's remaining work is bounded by the heap. The worst case is therefore
/// finishing the whole cycle in one assist: exactly the pause the synchronous
/// collector takes on every collection today. Under extreme allocation
/// pressure incremental degrades gracefully toward synchronous behavior
/// rather than toward unbounded memory.
pub(super) fn gc_mutator_assist_scaled_work_units() -> usize {
    let debt = GcDebtSnapshot::current();
    let arena_units = (debt.arena_debt_bytes / GC_ASSIST_DEBT_BYTES_PER_WORK_UNIT) as usize;
    // Malloc-registry work is per-object (mark/sweep touches each header
    // once), so malloc debt converts 1:1.
    let malloc_units = debt.malloc_debt_objects as usize;
    GC_MUTATOR_ASSIST_WORK_UNITS
        .saturating_add(arena_units)
        .saturating_add(malloc_units)
}
pub(super) fn gc_old_reclaim_debt_bytes(old_in_use: usize, baseline: usize) -> u64 {
    // #10928: one trigger, shared with `old_reclaim_pressure_due` -- the
    // absolute first-crossing arm it used to mirror no longer exists, and a
    // debt computed against a trigger the predicate does not use is exactly
    // the #7024 two-predicates-collapse the shared helper exists to prevent.
    let trigger = baseline.saturating_add(gc_old_reclaim_growth_band_bytes(baseline));
    old_in_use.saturating_sub(trigger) as u64
}

/// The thread-locals [`gc_budgeted_due_trigger`] reads on its fast path, with
/// the hot-cache slot each one claimed.
///
/// Enumerating them in the module that reads them is what makes the coverage
/// test able to fail: reverting any declaration below to a raw
/// `thread_local!` removes `slot_index` and breaks this function's build at
/// the declaration's own name, rather than leaving a silently slow path — the
/// failure mode `tls_hot.rs` was written to abolish and that this path
/// nonetheless kept for three years.
#[cfg(test)]
pub(crate) fn trigger_path_hot_slot_indices() -> Vec<(&'static str, u32)> {
    // Touch each one first: a slot is claimed on first read, not at
    // declaration, so an unread declaration reports the unassigned sentinel.
    //
    // `GC_DEFERRED_REQUEST` is deliberately absent: `defer_gc_request` reads it
    // only when `GC_ROOT_LOCK_DEPTH` is non-zero, so on the fast path it is
    // never touched and never claims a slot. Listing it made this function's
    // own test fail with `index 4294967295` on the first run — which is the
    // evidence that the test can fail, and the reason the list is "what the
    // fast path reads" rather than "what the module declares".
    let _ = gc_budgeted_due_trigger();
    let _ = super::trigger_watermark::nothing_due();
    vec![
        (
            "GC_OLD_RECLAIM_PENDING",
            GC_OLD_RECLAIM_PENDING.slot_index(),
        ),
        (
            "GC_LAST_OLD_RECLAIM_IN_USE_BYTES",
            GC_LAST_OLD_RECLAIM_IN_USE_BYTES.slot_index(),
        ),
        (
            "GC_NEXT_MALLOC_TRIGGER",
            GC_NEXT_MALLOC_TRIGGER.slot_index(),
        ),
        ("GC_NEXT_TRIGGER_BYTES", GC_NEXT_TRIGGER_BYTES.slot_index()),
        ("GC_TRIGGER_ARMED", GC_TRIGGER_ARMED.slot_index()),
        (
            "GC_EXTERNAL_SIDE_LIVE_BYTES",
            GC_EXTERNAL_SIDE_LIVE_BYTES.slot_index(),
        ),
        (
            "GC_MAJOR_PACING_RETAINING",
            GC_MAJOR_PACING_RETAINING.slot_index(),
        ),
        ("GC_FLAGS", GC_FLAGS.slot_index()),
        (
            "GC_TRIGGER_WATERMARK",
            super::trigger_watermark::trigger_watermark_slot_index(),
        ),
        (
            "GC_BUDGETED_CYCLE_ACTIVE",
            GC_BUDGETED_CYCLE_ACTIVE.slot_index(),
        ),
        (
            "GC_BUDGETED_STEP_ACTIVE",
            GC_BUDGETED_STEP_ACTIVE.slot_index(),
        ),
        (
            "OLD_GEN_IN_USE_BYTES",
            crate::arena::old_gen_in_use_bytes_slot_index(),
        ),
        ("OLD_FREE_BYTES", super::old_free_bytes_slot_index()),
        ("MALLOC_STATE", super::malloc_state_slot_index()),
    ]
}

/// #10169: record that a document-sized JSON leaf was just born old in the
/// arena because the young generation is at least that large and has not been
/// measured as dying (`string::json_leaf_prefers_arena`). Read once by the
/// next trigger decision.
pub(crate) fn note_young_leaf_born_old() {
    GC_YOUNG_LEAF_BORN_OLD.with(|flag| flag.set(true));
}

pub(super) fn gc_budgeted_due_trigger() -> Option<BudgetedGcTrigger> {
    gc_budgeted_due_trigger_eval().0
}

/// Reuse of a repeatable [`gc_budgeted_due_trigger_eval`] answer across the
/// questions one `gc_check_trigger` call asks. An unrepeatable answer is
/// returned once and the next question evaluates again.
pub(super) struct DueTriggerMemo(Option<Option<BudgetedGcTrigger>>);

impl DueTriggerMemo {
    pub(super) const fn new() -> Self {
        Self(None)
    }

    pub(super) fn get(
        &mut self,
        eval: impl FnOnce() -> (Option<BudgetedGcTrigger>, bool),
    ) -> Option<BudgetedGcTrigger> {
        if let Some(due) = self.0 {
            return due;
        }
        let (due, repeatable) = eval();
        if repeatable {
            self.0 = Some(due);
        }
        due
    }
}

/// [`gc_budgeted_due_trigger`], plus whether evaluating it again with no state
/// changed in between is guaranteed to give the same answer.
///
/// Only the #10169 flag branch below breaks that: it consumes the flag and
/// returns early, so a second evaluation takes the ordinary path and may
/// answer differently (for example `OldReclaim`). The #8122 census seed does
/// not break it. The seed runs inside the young-cap arm before that arm's
/// comparison, so the first evaluation already compared against the seeded
/// cap; no earlier arm reads the census and the malloc arm after it does not
/// either, and a second evaluation cannot seed again.
pub(super) fn gc_budgeted_due_trigger_eval() -> (Option<BudgetedGcTrigger>, bool) {
    let (due, repeatable, _) = gc_budgeted_due_trigger_probe();
    (due, repeatable)
}

/// [`gc_budgeted_due_trigger_eval`], plus — when nothing is due — the
/// watermark under which that answer keeps holding (#10698). Every other
/// answer carries [`TriggerWatermark::RETIRED`].
fn gc_budgeted_due_trigger_probe() -> (Option<BudgetedGcTrigger>, bool, TriggerWatermark) {
    const RETIRED: TriggerWatermark = TriggerWatermark::RETIRED;
    // #10169: a leaf born old under young pressure gives the nursery minor
    // ONE-TIME priority over old-reclaim, and only while the young generation
    // is still unmeasured. A young generation that a minor has already
    // measured as retained wholesale is either promoted (so the next leaf
    // finds it small) or, when it dies at every loop edge, best left to the
    // old-reclaim full that sweeps it in Eden together with the leaf. The one
    // case that must not fall through is an unmeasured young generation that
    // stays live: old-reclaim would re-mark it in place at every full, so it
    // is promoted by a minor first. The flag is consumed here whatever the
    // decision, so it can never starve old-reclaim.
    if GC_YOUNG_LEAF_BORN_OLD.with(TriggerInput::get) {
        GC_YOUNG_LEAF_BORN_OLD.with(|flag| flag.set(false));
        if !super::young_generation_measured_retained() && young_scavenge_cap_due() {
            return (Some(BudgetedGcTrigger::YoungScavengeCap), false, RETIRED);
        }
    }
    let old_pending = GC_OLD_RECLAIM_PENDING.with(TriggerInput::get);
    // #6010: external Map/Set side-buffer bytes escalate to OldReclaim too.
    let old_reclaimable = old_gen_reclaimable_pressure_bytes();
    let old_in_use = old_reclaimable.saturating_add(external_side_old_reclaim_pressure_bytes());
    let old_baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    let old_reclaim_why = if old_pending {
        Some("pending")
    } else if super::allocation_pacing::due(old_in_use, false) {
        Some("pacing")
    } else if old_reclaim_pressure_due(old_in_use, old_baseline) {
        Some("growth_band")
    } else {
        None
    };
    if let Some(why) = old_reclaim_why {
        if why != "pending" {
            super::diagnostic_hooks::note_reclaim_reason(why);
        }
        return (Some(BudgetedGcTrigger::OldReclaim), true, RETIRED);
    }

    // Two separately-scoped arena arms (see `young_scavenge_cap_due` for why
    // they must not share a basis): the adaptive base trigger against the
    // whole arena, and the scavenge nursery cap against the young generation
    // only.
    // Copying capacity never exceeds the gross arena bytes, so the leaf
    // exclusion (a block walk) is owed only once the gross figure is at the base.
    let base = next_arena_trigger_base();
    if crate::arena::arena_total_bytes() >= base && crate::arena::copying_capacity_bytes() >= base {
        return (Some(BudgetedGcTrigger::ArenaBytes), true, RETIRED);
    }
    // Old-gen is untouched since the read above: the arms in between only
    // read, and the census seed walks the young generation.
    let young = young_scavenge_cap_probe(|| old_reclaimable);
    if young.due {
        return (Some(BudgetedGcTrigger::YoungScavengeCap), true, RETIRED);
    }

    let malloc_count = malloc_object_count();
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
    if malloc_count >= next_malloc_trigger {
        return (Some(BudgetedGcTrigger::MallocCount), true, RETIRED);
    }

    (
        None,
        true,
        TriggerWatermark::after_nothing_due(next_malloc_trigger, young.reading),
    )
}

/// Threshold-based GC trigger (safe for use from the event loop).
/// Only runs collection if arena or malloc thresholds are exceeded.
#[no_mangle]
pub extern "C" fn gc_check_trigger_export() {
    gc_check_trigger();
}

pub(crate) use super::byte_store_placement::{byte_store_placement, ByteStorePlacement};
#[cfg(test)]
pub(crate) use super::byte_store_placement::{
    byte_store_test_collection_count, byte_store_test_external_live_bytes, ByteStorePolicyTestGuard,
};
