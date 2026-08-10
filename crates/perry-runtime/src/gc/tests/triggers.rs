use super::super::*;
use super::support::*;

struct GcBumpTriggerTestGuard {
    next_arena_trigger: usize,
    arena_step: usize,
    next_malloc_trigger: usize,
    malloc_step: usize,
    trigger_bumped: bool,
    pre_suppress_bytes: usize,
}

impl GcBumpTriggerTestGuard {
    fn new(next_arena_trigger: usize, arena_step: usize) -> Self {
        let previous = Self {
            next_arena_trigger: GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                let previous = trigger.get();
                trigger.set(next_arena_trigger);
                previous
            }),
            arena_step: GC_STEP_BYTES.with(|step| {
                let previous = step.get();
                step.set(arena_step);
                previous
            }),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
                let previous = trigger.get();
                trigger.set(usize::MAX);
                previous
            }),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|step| step.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|bumped| {
                let previous = bumped.get();
                bumped.set(false);
                previous
            }),
            pre_suppress_bytes: GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.get()),
        };
        GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.set(0));
        previous
    }

    fn set_pre_suppress(bytes: usize) {
        GC_PRE_SUPPRESS_BYTES.with(|pre| pre.set(bytes));
    }

    fn next_arena_trigger() -> usize {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get())
    }

    fn trigger_bumped() -> bool {
        GC_TRIGGER_BUMPED.with(|bumped| bumped.get())
    }

    fn reset_cycle_bump() {
        GC_TRIGGER_BUMPED.with(|bumped| bumped.set(false));
    }
}

impl Drop for GcBumpTriggerTestGuard {
    fn drop(&mut self) {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(self.next_arena_trigger));
        GC_STEP_BYTES.with(|step| step.set(self.arena_step));
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(self.next_malloc_trigger));
        GC_MALLOC_COUNT_STEP.with(|step| step.set(self.malloc_step));
        GC_TRIGGER_BUMPED.with(|bumped| bumped.set(self.trigger_bumped));
        GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.set(self.pre_suppress_bytes));
    }
}

#[test]
fn test_gc_bump_tiny_parse_caps_arena_trigger_at_collector_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING - 1024;
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
    assert!(
        !GcBumpTriggerTestGuard::trigger_bumped(),
        "tiny parses must not consume the medium/large per-cycle bump"
    );
}

#[test]
fn test_gc_bump_repeated_tiny_parses_cannot_exceed_collector_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(
        GC_TRIGGER_ABSOLUTE_CEILING - (2 * 1024 * 1024),
        GC_THRESHOLD_INITIAL_BYTES,
    );

    let first_bytes_now = GC_TRIGGER_ABSOLUTE_CEILING - 1024;
    GcBumpTriggerTestGuard::set_pre_suppress(first_bytes_now);
    assert!(gc_bump_malloc_trigger_with_snapshot(0, first_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );

    let later_bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + (32 * 1024 * 1024);
    GcBumpTriggerTestGuard::set_pre_suppress(later_bytes_now);
    assert!(gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
}

#[test]
fn test_gc_bump_one_block_parse_uses_tiny_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + GC_SUPPRESSED_TINY_PARSE_BYTES;
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now - GC_SUPPRESSED_TINY_PARSE_BYTES);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
    assert!(!GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_gc_bump_medium_parse_allows_one_arena_bump_per_gc_cycle() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let first_bytes_now = 2 * GC_SUPPRESSED_TINY_PARSE_BYTES;
    let first_expected = first_bytes_now + GC_THRESHOLD_INITIAL_BYTES;

    GcBumpTriggerTestGuard::set_pre_suppress(0);
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, first_bytes_now));
    assert_eq!(GcBumpTriggerTestGuard::next_arena_trigger(), first_expected);
    assert!(GcBumpTriggerTestGuard::trigger_bumped());

    let later_bytes_now = first_expected + (16 * 1024 * 1024);
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        first_expected,
        "second medium/large bump in the same cycle must be ignored"
    );

    GcBumpTriggerTestGuard::reset_cycle_bump();
    let second_expected = later_bytes_now + GC_THRESHOLD_INITIAL_BYTES;
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        second_expected
    );
    assert!(GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_gc_bump_never_lowers_existing_arena_trigger() {
    // The "never lower" invariant is asserted against the RAW trigger cell, whose
    // relationship to the bump target only holds under legacy pacing: with moving
    // mode on, `effective_next_arena_trigger` is clamped to the small nursery cap,
    // so a bump target above that cap legitimately re-arms the cell (without ever
    // lowering the EFFECTIVE, capped trigger). Pin legacy to keep asserting the
    // raw-cell arithmetic this test was written for. (The new nursery-cap value
    // itself is asserted under the default by
    // `test_effective_arena_trigger_respects_armed_values`.)
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let existing_trigger = GC_TRIGGER_ABSOLUTE_CEILING + (32 * 1024 * 1024);
    let _guard = GcBumpTriggerTestGuard::new(existing_trigger, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + (16 * 1024 * 1024);
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        existing_trigger
    );
    assert!(!GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_old_reclaim_pressure_uses_threshold_and_growth() {
    assert!(!old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - 1,
        GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
    ));
    assert!(old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - 1,
    ));
    assert!(!old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES + 1,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    ));
    assert!(old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES + GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    ));
}

/// #7592: the handoff full fires on CURRENT old-gen pressure only. It used to
/// add `promotable_bytes` into the pressure estimate — a prediction of where
/// old-gen would land after the promotion — but promotable bytes sit in the
/// survivor space, where a full mark-sweep can neither reclaim them nor the
/// old-gen space they have not yet occupied. A handoff over a near-empty
/// old-gen is guaranteed futile (measured: 1,015 ms over 4.2 MB, 0 freed).
#[test]
fn test_copying_minor_promotion_handoff_requires_current_old_pressure() {
    // Below the handoff minimum: never due, whatever old-gen looks like.
    assert!(!copied_minor_promotion_handoff_pressure_due(
        GC_COPY_PROMOTION_HANDOFF_MIN_BYTES - 1,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        0,
    ));
    // The #7592 shape: a huge imminent promotion over a near-empty old-gen.
    // Predicted pressure said "due" here; a full over 4 MB frees nothing.
    assert!(!copied_minor_promotion_handoff_pressure_due(
        108 * 1024 * 1024,
        4 * 1024 * 1024,
        0,
    ));
    // Current old-gen pressure is real (threshold crossing): due.
    assert!(copied_minor_promotion_handoff_pressure_due(
        GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        0,
    ));
    // Current growth past the baseline exceeds the proportional band: due.
    assert!(copied_minor_promotion_handoff_pressure_due(
        26 * 1024 * 1024,
        60 * 1024 * 1024,
        8 * 1024 * 1024,
    ));
    // Same growth, but the baseline's proportional band swallows it: not due.
    assert!(!copied_minor_promotion_handoff_pressure_due(
        26 * 1024 * 1024,
        160 * 1024 * 1024,
        120 * 1024 * 1024,
    ));
}

/// #7592: the proportional old-reclaim growth band and the promoted-bytes
/// baseline credit.
#[test]
fn test_old_reclaim_band_is_proportional_and_promotion_credits_baseline() {
    // Small baselines keep the constant floor.
    assert_eq!(
        gc_old_reclaim_growth_band_bytes(0),
        gc_old_gen_reclaim_growth_dyn_bytes()
    );
    // Large baselines scale: the band is baseline/2 once that exceeds the
    // floor, so major count is logarithmic in heap growth.
    let big = 400 * 1024 * 1024;
    assert_eq!(gc_old_reclaim_growth_band_bytes(big), big / 2);
    // Dueness and debt share the band: at exactly band-1 past the baseline,
    // not due and zero debt; at band, due and debt begins.
    let baseline = big;
    let band = gc_old_reclaim_growth_band_bytes(baseline);
    assert!(!old_reclaim_pressure_due(baseline + band - 1, baseline));
    assert!(old_reclaim_pressure_due(baseline + band, baseline));
    // Debt counts bytes strictly PAST the trigger (pre-existing convention:
    // due at the trigger with zero debt), but both must derive the trigger
    // from the same proportional band.
    assert_eq!(gc_old_reclaim_debt_bytes(baseline + band - 1, baseline), 0);
    assert_eq!(gc_old_reclaim_debt_bytes(baseline + band + 1, baseline), 1);

    // Promotion credit: promoted bytes are live by construction, so a reclaim
    // must not become due merely because promotion crossed a threshold.
    let _guard = GcTestIsolationGuard::new();
    let prev = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|b| b.get());
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|b| b.set(4 * 1024 * 1024));
    credit_promoted_bytes_to_old_baseline(270 * 1024 * 1024);
    let credited = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|b| b.get());
    assert_eq!(credited, 274 * 1024 * 1024);
    // The #7592 cycle-6 shape: old-gen jumped to 274 MB purely by promotion.
    assert!(
        !old_reclaim_pressure_due(274 * 1024 * 1024, credited),
        "a reclaim right after promotion is guaranteed to free nothing"
    );
    credit_promoted_bytes_to_old_baseline(0);
    assert_eq!(
        GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|b| b.get()),
        credited,
        "zero promotion must not move the baseline"
    );
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|b| b.set(prev));
}

/// #7592: a handoff full must not repeat without the copying minor it exists to
/// enable.
///
/// The handoff replaces a minor with a full mark-sweep to make room for
/// survivors about to be promoted, but a full mark-sweep is non-moving and
/// promotes nothing — so it cannot relieve the pressure that scheduled it, and
/// the predicate is true again at the next minor. Without the latch that is a
/// livelock: json_pipeline at 200k records ran 19 consecutive
/// `survivor_promotion_bytes` fulls, each freeing 0.0 MB at ~400 ms.
///
/// The latch short-circuits before any arena inspection, so this asserts the
/// suppression itself rather than reproducing the heap state that arms it.
#[test]
fn test_survivor_promotion_handoff_waits_for_the_copying_minor() {
    let _guard = GcTestIsolationGuard::new();

    note_copying_minor_completed();
    assert!(
        !survivor_promotion_handoff_awaiting_minor(),
        "latch must start clear"
    );

    note_survivor_promotion_handoff_full();
    assert!(survivor_promotion_handoff_awaiting_minor());
    // Assert the SUPPRESSION, not just the `false`. With an empty heap the
    // predicate returns false at the survivor-occupancy check regardless, so a
    // bare `assert!(!due)` passes with the latch deleted — it would be a test
    // that cannot fail. The counter only moves if the latch branch ran.
    for kind in [GcTriggerKind::ArenaBytes, GcTriggerKind::MallocCount] {
        let before = survivor_promotion_handoff_suppressions();
        assert!(
            !copied_minor_promotion_handoff_due(kind),
            "a second handoff must be suppressed while the first still awaits \
             its copying minor ({kind:?})"
        );
        assert_eq!(
            survivor_promotion_handoff_suppressions(),
            before + 1,
            "the latch branch must be what rejected it ({kind:?})"
        );
    }
    // A trigger kind the handoff never applies to must not be counted as a
    // latch suppression — it is rejected earlier, on its own merits.
    let before = survivor_promotion_handoff_suppressions();
    assert!(!copied_minor_promotion_handoff_due(GcTriggerKind::Direct));
    assert_eq!(survivor_promotion_handoff_suppressions(), before);

    // Only a COPYING minor clears it: a non-moving minor fallback promotes
    // nothing and would reinstate the livelock at half rate.
    note_copying_minor_completed();
    assert!(
        !survivor_promotion_handoff_awaiting_minor(),
        "the copying minor consumes the handoff"
    );
}

// 2026-07-09 audit (device-blind policy): budget-scaled threshold math.
#[test]
fn test_budget_scaled_clamps_only_under_budget() {
    use super::super::heap_budget::budget_scaled_with;
    const MB: usize = 1024 * 1024;
    // Unbudgeted (desktop/server): historical default unchanged.
    assert_eq!(budget_scaled_with(None, 128 * MB, 1, 4, 2 * MB), 128 * MB);
    // 64 MB budget (watch-class): quarter-budget trigger.
    assert_eq!(
        budget_scaled_with(Some(64 * MB), 128 * MB, 1, 4, 2 * MB),
        16 * MB
    );
    // 256 MB container: still clamped below the default.
    assert_eq!(
        budget_scaled_with(Some(256 * MB), 128 * MB, 1, 4, 2 * MB),
        64 * MB
    );
    // Big budget: fraction exceeds the default → default wins.
    assert_eq!(
        budget_scaled_with(Some(900 * MB), 128 * MB, 1, 4, 2 * MB),
        128 * MB
    );
    // Degenerate tiny budget: floor holds.
    assert_eq!(budget_scaled_with(Some(MB), 128 * MB, 1, 4, 2 * MB), 2 * MB);
}

// ───────────────────────────────────────────────────────────────────────────
// #7024: the alloc-point deferral must be REACHABLE at the moment a nursery
// trigger becomes due, at every heap budget.
//
// `gc_budgeted_due_trigger()` reports `ArenaBytes` due exactly when
// `arena_total_bytes() >= effective_next_arena_trigger()`. The deferral that
// hands the collection to the precise-root safepoint (and therefore to the
// COPYING minor) used to be guarded by `arena_total_bytes() < <absolute cap>`,
// where the cap came from `budget_scaled(128 MB, 1, 4, 2 MB)` — byte-for-byte
// the trigger-ceiling formula. Under any budget small enough for the ceiling to
// sit at or below the nursery cap the two are the same number, so the two
// predicates are exact complements: the deferral is refused at precisely the
// arena size that makes the trigger due, and the copying minor never runs.
// Measured consequence: the stress matrix's `default` arm ran zero copying
// minors on all 22 corpus rows.
//
// These tests fail against the pre-#7024 predicate (`arena_total < cap`) and
// pass against the slack-from-the-deferral-point predicate.
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn test_moving_defer_reachable_when_the_arena_trigger_is_due() {
    use super::super::heap_budget::budget_scaled_with;
    use super::super::policy::{
        moving_defer_within_slack, GC_MOVING_DEFER_SLACK_BYTES, GC_TRIGGER_ABSOLUTE_CEILING,
    };
    const MB: usize = 1024 * 1024;
    // `gc_scavenge_nursery_cap_bytes()`'s default; moving mode clamps the
    // effective trigger to it (`effective_next_arena_trigger`).
    const NURSERY_CAP: usize = 16 * MB;

    for budget in [
        None,
        Some(2 * MB),
        Some(8 * MB), // the stress matrix's `--pressure 8`
        Some(16 * MB),
        Some(32 * MB),
        Some(64 * MB),
        Some(128 * MB),
        Some(512 * MB),
    ] {
        let ceiling = budget_scaled_with(budget, GC_TRIGGER_ABSOLUTE_CEILING, 1, 4, 2 * MB);
        let slack = budget_scaled_with(budget, GC_MOVING_DEFER_SLACK_BYTES, 1, 3, MB);
        // The smallest `arena_total` at which `gc_budgeted_due_trigger()`
        // reports ArenaBytes, in moving mode.
        let due_at = ceiling.min(NURSERY_CAP);

        // The collapse premise, asserted rather than assumed: whenever the
        // budget pulls the ceiling to or below the nursery cap — every device
        // budget ≤ 64 MB, and every `--pressure` setting the matrix uses — the
        // pre-#7024 absolute cap is already reached at `due_at`, so the old
        // guard `arena_total < cap` was FALSE exactly when the trigger fired.
        let legacy_cap = budget_scaled_with(budget, 128 * MB, 1, 4, 2 * MB);
        if ceiling <= NURSERY_CAP {
            assert!(
                due_at >= legacy_cap,
                "budget {budget:?}: expected the pre-#7024 absolute cap ({legacy_cap}) to be \
                 unreachable at the due point ({due_at})"
            );
        }

        // The fix: the first deferral of a cycle is unconditional, so the
        // copying minor is reachable at every budget.
        assert!(
            moving_defer_within_slack(due_at, None, slack),
            "budget {budget:?}: a nursery trigger due at {due_at} bytes must be deferrable"
        );
        // …and it stays deferrable for a whole slack of further growth, so a
        // loop back-edge poll has room to drain it.
        assert!(
            moving_defer_within_slack(due_at + slack - 1, Some(due_at), slack),
            "budget {budget:?}: deferral must survive until the slack is spent"
        );
    }
}

#[test]
fn test_moving_defer_slack_still_has_a_safety_valve() {
    use super::super::policy::moving_defer_within_slack;
    const MB: usize = 1024 * 1024;
    let slack = 4 * MB;
    let base = 2 * MB;

    // No deferral outstanding: always allowed, however large the arena. This is
    // the sound path (precise, rewritable roots at a real safepoint); the
    // alloc-point fallback exists only to bound growth when nothing drains it.
    assert!(moving_defer_within_slack(0, None, slack));
    assert!(moving_defer_within_slack(4 * 1024 * MB, None, slack));

    // Deferral outstanding: bounded overshoot, measured from the deferral point.
    assert!(moving_defer_within_slack(base, Some(base), slack));
    assert!(moving_defer_within_slack(
        base + slack - 1,
        Some(base),
        slack
    ));
    assert!(!moving_defer_within_slack(base + slack, Some(base), slack));
    assert!(!moving_defer_within_slack(
        base + slack + MB,
        Some(base),
        slack
    ));

    // The valve is relative, not absolute: a program whose live set already
    // sits far above any fixed cap still gets its slack (and therefore still
    // gets copying minors) instead of being pinned on the non-moving path.
    let big = 900 * MB;
    assert!(moving_defer_within_slack(big, None, slack));
    assert!(moving_defer_within_slack(big + slack - 1, Some(big), slack));
    assert!(!moving_defer_within_slack(big + slack, Some(big), slack));

    // Overflow-safe.
    assert!(moving_defer_within_slack(
        usize::MAX - 1,
        Some(usize::MAX),
        slack
    ));
    assert!(!moving_defer_within_slack(
        usize::MAX,
        Some(usize::MAX),
        slack
    ));
}

// The un-armed trigger cell (desktop-default const initializer) reads as
// the device ceiling; an armed trigger above the ceiling is legitimate
// (headroom floor over a big live set) and must NOT be clamped.
#[test]
fn test_effective_arena_trigger_respects_armed_values() {
    use super::super::heap_budget::gc_trigger_absolute_ceiling_bytes;
    use super::super::policy::{
        effective_next_arena_trigger, gc_moving_loop_polls_enabled, gc_scavenge_nursery_cap_bytes,
        GC_NEXT_TRIGGER_BYTES, GC_TRIGGER_ARMED,
    };
    // `effective_next_arena_trigger` additionally clamps to the small nursery cap
    // whenever the collection that clamp schedules can EVACUATE; otherwise it
    // clamps only the UN-armed cell to the device ceiling and lets an armed
    // trigger exceed it. Assert the value correct for the mode this process runs
    // in, so the nursery-cap behavior is exercised under moving pacing and the
    // ceiling behavior under the PERRY_GC_MOVING_LOOP_POLLS=0 kill switch. This
    // mirrors the gate in `policy::nursery_cap_active` exactly.
    //
    // `gc_scavenge_enabled()` used to be ORed in here, mirroring the gate as it
    // stood. It is not part of that gate since #7682: scavenge routes nursery
    // pressure to the direct alloc-point minor, and that minor is now always
    // non-moving, so a cap keyed on it schedules a collection that cannot lower
    // the cap's own basis.
    let nursery_capped = gc_moving_loop_polls_enabled();
    let ceiling = gc_trigger_absolute_ceiling_bytes();
    let nursery_cap = gc_scavenge_nursery_cap_bytes();

    let prev_trigger = GC_NEXT_TRIGGER_BYTES.with(|c| c.get());
    let prev_armed = GC_TRIGGER_ARMED.with(|c| c.get());

    GC_TRIGGER_ARMED.with(|c| c.set(false));
    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(usize::MAX / 2));
    let expected_unarmed = if nursery_capped {
        ceiling.min(nursery_cap)
    } else {
        ceiling
    };
    assert_eq!(
        effective_next_arena_trigger(),
        expected_unarmed,
        "un-armed trigger must clamp to the device ceiling (further to the nursery cap when moving)"
    );

    GC_TRIGGER_ARMED.with(|c| c.set(true));
    let above_ceiling = ceiling * 3;
    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(above_ceiling));
    let expected_armed = if nursery_capped {
        above_ceiling.min(nursery_cap)
    } else {
        above_ceiling
    };
    assert_eq!(
        effective_next_arena_trigger(),
        expected_armed,
        "armed triggers above the ceiling survive under legacy pacing and clamp to the nursery cap when moving"
    );

    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(prev_trigger));
    GC_TRIGGER_ARMED.with(|c| c.set(prev_armed));
}

// #7682: the moving-loop (evacuating) minor is ON by default again, and the
// SAFE fallback direction has inverted with it.
//
// Under #7161's stopgap the safe direction was "off": a garbage value selected
// the non-evacuating minor. It is now "on", and that is not a weakening. With
// polls off, nursery pressure has NO precise collection point in a compute-only
// program — neither this poll nor the microtask-pump boundary is reached — so
// every nursery collection lands at the register-imprecise allocation point,
// which #7682 established must not move. Off is the state in which the
// collector cannot do its job precisely at all; a typo in the env var should
// not select it.

// #6184: the OS memory-pressure entry must run a real collection when the
// thread is at a safe point, and must lower+arm the arena trigger.
#[test]
fn test_memory_pressure_collects_and_clamps_trigger() {
    let _guard = GcTestIsolationGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    use super::super::policy::{GC_NEXT_TRIGGER_BYTES, GC_TRIGGER_ARMED};

    let prev_trigger = GC_NEXT_TRIGGER_BYTES.with(|c| c.get());
    let prev_armed = GC_TRIGGER_ARMED.with(|c| c.get());
    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(usize::MAX / 2));
    GC_TRIGGER_ARMED.with(|c| c.set(false));

    let count_before = GC_STATS.with(|s| s.borrow().collection_count);
    let rc = js_gc_memory_pressure(2);
    assert_eq!(rc, 2, "safe point must collect synchronously");
    let count_after = GC_STATS.with(|s| s.borrow().collection_count);
    assert!(count_after > count_before, "critical pressure must collect");
    assert!(
        GC_TRIGGER_ARMED.with(|c| c.get()),
        "pressure must arm the lowered trigger"
    );
    assert!(
        GC_NEXT_TRIGGER_BYTES.with(|c| c.get()) < usize::MAX / 2,
        "pressure must pull the trigger down"
    );

    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(prev_trigger));
    GC_TRIGGER_ARMED.with(|c| c.set(prev_armed));
    assert_eq!(js_gc_memory_pressure(0), 0);
}

/// #7690 shipped its entire default-ON argument in doc comments and left the
/// predicate matching `1|on|true`. Nothing failed, because the function that was
/// factored out expressly to make the default "unit-testable without touching
/// process env" had no test. This is that test.
///
/// A default of OFF is not a slower configuration, it is a different collector:
/// with no poll there is no precise safepoint, so after #7687 made the
/// alloc-point minor non-moving the nursery is never evacuated at all. Measured
/// on the quiet bench host at `12e48edd6`, `churn_alloc` ran 13 whole-arena full
/// collections (0.477 s of pause) where the same program at `a853135aa` ran 105
/// copying minors (0.016 s), and `tree` spent 4.1 s of a 5.1 s wall in GC.
#[test]
fn polls_default_is_on() {
    assert!(
        moving_loop_polls_enabled_from_env(None),
        "unset PERRY_GC_MOVING_LOOP_POLLS must select the moving-loop path — \
         a default of OFF leaves the nursery with no precise safepoint to evacuate at"
    );
    for kill in ["0", "off", "false"] {
        assert!(
            !moving_loop_polls_enabled_from_env(Some(kill)),
            "PERRY_GC_MOVING_LOOP_POLLS={kill} must remain the kill switch"
        );
    }
    for on in ["1", "on", "true"] {
        assert!(
            moving_loop_polls_enabled_from_env(Some(on)),
            "PERRY_GC_MOVING_LOOP_POLLS={on} must stay an explicit opt-in"
        );
    }
    // An unrecognised value is ON, like every other non-kill value. Spelled out
    // because the pre-#7690 predicate answered OFF here, so this is the arm that
    // silently changes meaning if the `matches!` is ever inverted back.
    assert!(
        moving_loop_polls_enabled_from_env(Some("yes")),
        "only the documented kill-switch spellings may disable polls"
    );
}

/// The runtime decides whether to DEFER a nursery collection; codegen decides
/// whether to emit the poll that DRAINS it. They read the same env var from two
/// crates that cannot share a symbol, and a disagreement is silent in both
/// directions — polls nothing consumes, or a deferral nothing drains. #7690 left
/// them agreeing at OFF while documenting ON; this pins them as a pair against a
/// copy of codegen's table.
#[test]
fn polls_default_matches_codegen_mirror() {
    // Mirrors `perry_codegen::stmt::loops::moving_safepoint_polls_enabled_from_env`.
    fn codegen_mirror(value: Option<&str>) -> bool {
        !matches!(value, Some("0") | Some("off") | Some("false"))
    }
    for value in [
        None,
        Some("0"),
        Some("off"),
        Some("false"),
        Some("1"),
        Some("on"),
        Some("true"),
        Some(""),
        Some("yes"),
    ] {
        assert_eq!(
            moving_loop_polls_enabled_from_env(value),
            codegen_mirror(value),
            "runtime and codegen must agree on PERRY_GC_MOVING_LOOP_POLLS={value:?}"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Yield-adaptive major-GC pacing (#7726).
//
// `arena_growth_full_escalation_due` escalates a minor to a FULL once arena
// live-bytes pass K× the last full's live set. On a monotonically growing
// all-live heap that gate fires on the growth itself and reclaims ~nothing:
// measured on `gc-handoff/bench/retain.ts`, the two escalated fulls cost 644 ms
// of a 1.31 s run and moved arena in-use by 4 MB total — the second by zero.
// So price each full by what it reclaimed and push the next escalation out when
// the answer is "almost nothing".
//
// These assert the DECISION FUNCTION over recorded (pre, post) pairs, which is
// the part a benchmark cannot pin: a green retain time proves the backoff fired
// on that one shape, not that a productive full still resets it.
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn major_pacing_backoff_defaults_to_zero_and_needs_a_recorded_cycle_start() {
    use super::super::policy::{
        major_pacing_backoff_shift, test_note_full_cycle_reclaimed, test_reset_major_pacing_backoff,
    };
    test_reset_major_pacing_backoff();
    assert_eq!(
        major_pacing_backoff_shift(),
        0,
        "default pacing must be exactly today's: escalate at K× the baseline"
    );
    // A full with no recorded start (`note_full_cycle_started` never ran — an
    // explicit `gc()`, not an arena-growth escalation) must not move the shift:
    // its yield says nothing about the gate this backoff paces.
    test_note_full_cycle_reclaimed(0, 0);
    assert_eq!(major_pacing_backoff_shift(), 0);
    test_reset_major_pacing_backoff();
}

#[test]
fn major_pacing_backs_off_on_futile_fulls_and_resets_on_a_productive_one() {
    use super::super::policy::{
        major_pacing_backoff_shift, test_note_full_cycle_reclaimed, test_reset_major_pacing_backoff,
    };
    const MB: usize = 1024 * 1024;
    test_reset_major_pacing_backoff();

    // retain.ts's own two fulls: 67 MB → 63 MB (5.9%) and 204 MB → 204 MB (0%).
    test_note_full_cycle_reclaimed(67 * MB, 63 * MB);
    assert_eq!(
        major_pacing_backoff_shift(),
        1,
        "a 5.9%-yield full backs off"
    );
    test_note_full_cycle_reclaimed(204 * MB, 204 * MB);
    assert_eq!(
        major_pacing_backoff_shift(),
        2,
        "a 0%-yield full backs off again"
    );

    // Capped: a long run of futile fulls must not disable pacing outright.
    test_note_full_cycle_reclaimed(400 * MB, 400 * MB);
    test_note_full_cycle_reclaimed(800 * MB, 800 * MB);
    assert_eq!(major_pacing_backoff_shift(), 2, "the shift is capped");

    // A churn-shaped full reclaims most of the heap and restores the original
    // pacing immediately — the backoff is not a ratchet.
    test_note_full_cycle_reclaimed(800 * MB, 200 * MB);
    assert_eq!(
        major_pacing_backoff_shift(),
        0,
        "a productive full resets the backoff in one step"
    );

    // Exactly at the threshold counts as productive (>=, not >).
    test_note_full_cycle_reclaimed(100 * MB, 80 * MB);
    assert_eq!(major_pacing_backoff_shift(), 0);
    // One percent under it does not.
    test_note_full_cycle_reclaimed(100 * MB, 81 * MB);
    assert_eq!(major_pacing_backoff_shift(), 1);
    test_reset_major_pacing_backoff();
}

/// The escalation and the pricing of its result must not be separable.
///
/// #7726's first cut recorded the pre-full reading at the two
/// `gc_start_budgeted_cycle_for_pressure` call sites and missed the one in
/// `gc::gc_collect_minor_with_trigger_inner` — the site the shipped safepoint
/// path actually takes. Every test still passed, the decision function was
/// correct in isolation, and the change measured as a 30 ms no-op on a
/// benchmark it should have taken 480 ms off. The recording now lives INSIDE
/// `arena_growth_full_escalation_due`; this pins that coupling, in the one
/// direction a unit test can force without a 32 MB heap.
#[test]
fn declining_to_escalate_records_no_pre_full_reading() {
    use super::super::policy::{
        arena_growth_full_escalation_due, test_major_pacing_pre_in_use_bytes,
        test_reset_major_pacing_backoff, test_set_major_pacing_baseline,
    };
    test_reset_major_pacing_backoff();
    // A baseline no live arena can exceed forces the verdict to `false`.
    let previous = test_set_major_pacing_baseline(usize::MAX / 4);
    let due = arena_growth_full_escalation_due();
    let recorded = test_major_pacing_pre_in_use_bytes();
    test_set_major_pacing_baseline(previous);
    test_reset_major_pacing_backoff();
    assert!(!due, "an unreachable baseline must not escalate");
    assert_eq!(
        recorded, 0,
        "a declined escalation must leave no pre-full reading behind — a stale \
         one would price the NEXT full against the wrong heap"
    );
}

/// #7737 item 2: the POSITIVE direction of the same recording.
///
/// `declining_to_escalate_records_no_pre_full_reading` proves only that a
/// declined escalation leaves `pre_in_use == 0`. Nothing asserted that a `true`
/// verdict from the REAL `arena_growth_full_escalation_due()` — not the
/// `test_note_full_cycle_reclaimed` bypass, which sets the reading itself —
/// leaves a non-zero one behind.
///
/// That gap is exactly the shape of the first-cut bug #7733's changelog
/// describes: the recording wired at the wrong call sites,
/// `update_major_pacing_backoff` silently no-op'ing on `pre_in_use == 0`, and
/// every test still green. A negative-only test cannot see it, because the
/// no-op and the correct decline produce the same `0`.
///
/// Forcing `due == true` needs the arena reading above the floor
/// (`PERRY_GC_MAJOR_PACING_FLOOR_MB`, 32 MB by default), and the floor is not
/// lowerable per-test — `major_pacing_config` is a process-wide `OnceLock`. So
/// the reading is injected through `pacing_arena_in_use_bytes`, the one
/// accessor both the predicate and the recording now share.
#[test]
fn escalating_records_the_pre_full_arena_reading() {
    use super::super::policy::{
        arena_growth_full_escalation_due, major_pacing_config, test_major_pacing_pre_in_use_bytes,
        test_reset_major_pacing_backoff, test_set_major_pacing_baseline,
        test_set_pacing_arena_in_use,
    };

    let (floor_bytes, _growth) = major_pacing_config();
    if floor_bytes == 0 {
        // `PERRY_GC_MAJOR_PACING_FLOOR_MB=0` disables pacing outright: no
        // reading escalates, so there is no positive direction to assert.
        return;
    }

    test_reset_major_pacing_backoff();
    // Baseline 0 ("no full yet") makes the floor itself the boundary, so the
    // reading below is unambiguously over it.
    let previous_baseline = test_set_major_pacing_baseline(0);
    let reading = floor_bytes + 1;
    let previous_reading = test_set_pacing_arena_in_use(Some(reading));

    let due = arena_growth_full_escalation_due();
    let recorded = test_major_pacing_pre_in_use_bytes();

    test_set_pacing_arena_in_use(previous_reading);
    test_set_major_pacing_baseline(previous_baseline);
    test_reset_major_pacing_backoff();

    assert!(
        due,
        "an arena reading one byte over the floor ({floor_bytes}) must escalate \
         with no prior full — `major_pacing_escalation_threshold_for` returns \
         the floor verbatim when the baseline is 0"
    );
    assert_eq!(
        recorded, reading,
        "an ESCALATED full must leave the pre-full reading behind, and it must \
         be the same reading the predicate decided on. A 0 here is the #7726 \
         no-op: `update_major_pacing_backoff` returns early on a zero pre \
         reading, so the backoff never fires and pacing silently reverts to the \
         unconditional K× rule — with every test still green"
    );
}

/// The predicate and the pre-full recording must read the SAME metric.
///
/// `update_major_pacing_backoff`'s doc asserts this in prose ("deliberately
/// measured on the SAME metric the escalation gate reads"). Before #7737 both
/// sites called `arena_in_use_bytes()` independently, so the guarantee was a
/// convention two call sites happened to honour. Routing an injected reading
/// through and requiring it to come back out the other side is what makes it
/// checkable: if either site is re-pointed at a different metric, the recorded
/// value stops matching what the decision was taken on.
#[test]
fn the_recorded_reading_is_the_one_the_predicate_decided_on() {
    use super::super::policy::{
        arena_growth_full_escalation_due, major_pacing_config, test_major_pacing_pre_in_use_bytes,
        test_reset_major_pacing_backoff, test_set_major_pacing_baseline,
        test_set_pacing_arena_in_use,
    };

    let (floor_bytes, _growth) = major_pacing_config();
    if floor_bytes == 0 {
        return;
    }

    for over in [1usize, 7, 4096] {
        test_reset_major_pacing_backoff();
        let previous_baseline = test_set_major_pacing_baseline(0);
        let reading = floor_bytes + over;
        let previous_reading = test_set_pacing_arena_in_use(Some(reading));

        let due = arena_growth_full_escalation_due();
        let recorded = test_major_pacing_pre_in_use_bytes();

        test_set_pacing_arena_in_use(previous_reading);
        test_set_major_pacing_baseline(previous_baseline);
        test_reset_major_pacing_backoff();

        assert!(due, "reading {reading} is over the floor {floor_bytes}");
        assert_eq!(
            recorded, reading,
            "the recording must observe the same arena reading the predicate did"
        );
    }
}

/// A reading exactly ON the floor escalates (the clause is `>=`), and one below
/// it does not — the boundary the positive test sits above.
#[test]
fn the_floor_is_inclusive_and_below_it_declines() {
    use super::super::policy::{
        arena_growth_full_escalation_due, major_pacing_config, test_major_pacing_pre_in_use_bytes,
        test_reset_major_pacing_backoff, test_set_major_pacing_baseline,
        test_set_pacing_arena_in_use,
    };

    let (floor_bytes, _growth) = major_pacing_config();
    if floor_bytes == 0 {
        return;
    }

    let mut verdicts = Vec::new();
    for reading in [floor_bytes - 1, floor_bytes] {
        test_reset_major_pacing_backoff();
        let previous_baseline = test_set_major_pacing_baseline(0);
        let previous_reading = test_set_pacing_arena_in_use(Some(reading));

        let due = arena_growth_full_escalation_due();
        let recorded = test_major_pacing_pre_in_use_bytes();

        test_set_pacing_arena_in_use(previous_reading);
        test_set_major_pacing_baseline(previous_baseline);
        test_reset_major_pacing_backoff();
        verdicts.push((due, recorded));
    }
    assert_eq!(
        verdicts[0],
        (false, 0),
        "one byte under the floor must decline, and record nothing"
    );
    assert_eq!(
        verdicts[1],
        (true, floor_bytes),
        "exactly on the floor must escalate — the clause is `>=`, which is why \
         `major_pacing_escalation_threshold_for` adds the `+1` to the growth \
         boundary and not to the floor"
    );
}

/// The escalation boundary the GC trace reports must be the boundary the
/// predicate takes its decision on.
///
/// #7733 added `major_pacing_snapshot` for exactly one purpose: so a gate could
/// assert the pacing subject was LIVE in the trace rather than merely that
/// nothing threw. It then recomputed the boundary as `baseline × growth` with
/// the floor dropped on the floor of the function (`let (_floor, growth_num)`),
/// while `arena_growth_full_escalation_due` rejects everything under that same
/// floor. Wherever the floor dominates the two disagreed — most starkly before
/// the first full, where the trace reported `0` ("escalates at any size") for a
/// collector that escalates at 32 MB. A probe that misreports its own subject
/// is worse than no probe, because it reads green.
///
/// `escalates_reference` below is a deliberate INDEPENDENT transcription of the
/// four clauses the predicate used to spell out inline — not a call into the
/// code under test. The point is to pin the boundary against a second statement
/// of the rule, so collapsing both onto one helper cannot quietly redefine it.
#[test]
fn the_reported_escalation_boundary_is_the_one_the_predicate_decides_on() {
    use super::super::policy::major_pacing_escalation_threshold_for;
    const MB: usize = 1024 * 1024;

    fn escalates_reference(
        floor: usize,
        growth_num: usize,
        baseline: usize,
        shift: u32,
        in_use: usize,
    ) -> bool {
        if floor == 0 {
            return false; // pacing disabled
        }
        if in_use < floor {
            return false; // under the absolute floor: small heaps never pay
        }
        if baseline == 0 {
            return true; // no full yet
        }
        in_use > baseline.saturating_mul(growth_num.saturating_mul(1usize << shift))
    }

    // The named cases first: each is a different bug if it breaks.
    assert_eq!(
        major_pacing_escalation_threshold_for(32 * MB, 2, 4 * MB, 0),
        Some(32 * MB),
        "FLOOR DOMINATES (4 MB baseline × 2 = 8 MB, under the 32 MB floor): the \
         boundary is the floor. Reporting 8 MB names a collection the collector \
         will decline to run."
    );
    assert_eq!(
        major_pacing_escalation_threshold_for(32 * MB, 2, 64 * MB, 0),
        Some(128 * MB + 1),
        "GROWTH DOMINATES: strictly ABOVE K× the baseline, per the `>` clause"
    );
    assert_eq!(
        major_pacing_escalation_threshold_for(32 * MB, 2, 0, 0),
        Some(32 * MB),
        "no full yet: the floor is the boundary — the old snapshot reported 0 \
         here, i.e. 'always', for a collector that escalates at 32 MB"
    );
    assert_eq!(
        major_pacing_escalation_threshold_for(32 * MB, 2, 64 * MB, 2),
        Some(512 * MB + 1),
        "the backoff shift multiplies the GROWTH term, not the floor"
    );
    assert_eq!(
        major_pacing_escalation_threshold_for(0, 2, 64 * MB, 0),
        None,
        "PERRY_GC_MAJOR_PACING_FLOOR_MB=0 disables the pacing outright: no \
         arena reading escalates, so there is no boundary to report"
    );

    // ...then exhaustively against the reference, probing each boundary's own
    // ±1 neighbourhood so an off-by-one cannot hide between the named cases.
    for &floor in &[0, 1, 8 * MB, 32 * MB] {
        for &growth_num in &[1, 2, 3] {
            for &baseline in &[0, 1, 4 * MB, 32 * MB, usize::MAX / 4, usize::MAX] {
                for shift in 0..=2u32 {
                    let threshold =
                        major_pacing_escalation_threshold_for(floor, growth_num, baseline, shift);
                    let mut probes =
                        vec![0usize, 1, 8 * MB, 32 * MB, 64 * MB, 512 * MB, usize::MAX];
                    if let Some(boundary) = threshold {
                        probes.extend([
                            boundary.saturating_sub(1),
                            boundary,
                            boundary.saturating_add(1),
                        ]);
                    }
                    for in_use in probes {
                        assert_eq!(
                            threshold.is_some_and(|boundary| in_use >= boundary),
                            escalates_reference(floor, growth_num, baseline, shift, in_use),
                            "floor={floor} growth={growth_num} baseline={baseline} \
                             shift={shift} in_use={in_use}: the reported boundary \
                             disagrees with the escalation rule"
                        );
                    }
                }
            }
        }
    }
}

/// ...and the SHIPPED pair must agree, not just the pure helper.
///
/// The pure test above proves the formula; this proves both production callers
/// are actually reading it. It drives the real `arena_growth_full_escalation_due`
/// against the real `major_pacing_snapshot` on this thread's real arena, so a
/// future re-split (a snapshot that recomputes "the same" boundary inline)
/// fails here even if the pure helper stays correct.
///
/// `baseline = 0` is the discriminating row and it needs no particular heap
/// size: the pre-#7740 snapshot reported `0` there, i.e. `in_use >= 0`, which
/// is true of every arena including an empty one, while the predicate declines
/// under the floor.
#[test]
fn the_shipped_predicate_and_the_shipped_snapshot_read_one_boundary() {
    use super::super::policy::{
        arena_growth_full_escalation_due, major_pacing_config, major_pacing_snapshot,
        test_reset_major_pacing_backoff, test_set_major_pacing_baseline,
    };
    let (floor_bytes, _growth_num) = major_pacing_config();
    test_reset_major_pacing_backoff();
    let saved = test_set_major_pacing_baseline(0);

    for baseline in [0usize, 1, floor_bytes / 4, usize::MAX / 4] {
        test_set_major_pacing_baseline(baseline);
        let (reported_baseline, _shift, threshold) = major_pacing_snapshot();
        let in_use = crate::arena::arena_in_use_bytes();
        let due = arena_growth_full_escalation_due();
        // `due == true` records a pre-full reading; drop it so the next row and
        // every later test on this thread start from a clean pacing state.
        test_reset_major_pacing_backoff();

        assert_eq!(
            reported_baseline, baseline,
            "the snapshot must report the live baseline"
        );
        assert_eq!(
            due,
            threshold.is_some_and(|boundary| in_use >= boundary),
            "baseline={baseline}: the trace's escalate_at_or_above_bytes \
             ({threshold:?}) disagrees with the verdict the collector actually \
             took on this arena ({in_use} bytes)"
        );
    }

    test_set_major_pacing_baseline(saved);
    test_reset_major_pacing_backoff();
}

// ---------------------------------------------- the poll's arming word -----

/// The deferral flag and `PERRY_GC_POLL_ARMED` are one piece of state with two
/// representations, and only the second one is visible to the code that decides
/// whether to call the poll at all. This pins the transition in both directions.
///
/// The unsound direction is a `set` that bypasses `set_safepoint_pending`: the
/// word stays zero, codegen's inline guard branches around the call, and the
/// deferred collection is stranded until the next event-loop boundary — which a
/// compute-only program never reaches. That is #7690's failure mode (a collector
/// with no nursery evacuation) arriving through a different door, and it is
/// invisible to every existing test, because a program with no collections still
/// produces the right answer.
#[test]
fn a_deferral_arms_the_poll_word_and_draining_disarms_it() {
    let _isolation = GcTestIsolationGuard::new();
    crate::gc::set_safepoint_pending(false);
    let base = crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed);

    crate::gc::set_safepoint_pending(true);
    assert_eq!(
        crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
        base + 1,
        "arming a deferral must make the poll's global word non-zero — it is \
         the ONLY thing a codegen-emitted back-edge consults"
    );

    // Idempotent: the flag is a bool, so a second set is not a second arm.
    crate::gc::set_safepoint_pending(true);
    assert_eq!(
        crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
        base + 1,
        "only TRANSITIONS may move the counter, or a thread that defers twice \
         leaks an arm and pins the poll on for the life of the process"
    );

    crate::gc::set_safepoint_pending(false);
    assert_eq!(
        crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
        base,
        "draining must give the arm back"
    );
}

/// A poll whose word reads zero must do NOTHING — not even the bookkeeping.
///
/// This is the assertion that makes the optimisation real rather than
/// decorative. `note_loop_poll_reached` is an unconditional atomic RMW on a
/// process-shared line; leaving it above the gate would keep the most expensive
/// single instruction of the old fast path on every back-edge while looking, in
/// every other test, exactly like this one.
#[test]
fn an_unarmed_poll_touches_nothing() {
    let _isolation = GcTestIsolationGuard::new();
    let _zeal = super::super::zeal::ZealGuard::set(false);
    crate::gc::set_safepoint_pending(false);
    let restore = crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed);
    crate::gc::PERRY_GC_POLL_ARMED.store(0, std::sync::atomic::Ordering::Relaxed);

    let polls_before = crate::gc::loop_polls_reached();
    let collections_before = gc_collection_count();
    js_gc_loop_safepoint();
    js_gc_loop_safepoint();
    let polls_after = crate::gc::loop_polls_reached();
    let collections_after = gc_collection_count();

    crate::gc::PERRY_GC_POLL_ARMED.store(restore, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        polls_after, polls_before,
        "an unarmed poll must not reach the counter — reaching it means the \
         atomic increment is still on every back-edge"
    );
    assert_eq!(
        collections_after, collections_before,
        "and it certainly must not collect"
    );
}

/// Zeal's contract is a collection at EVERY safepoint, not only at ones an
/// alloc-point trigger already deferred. That is expressible only if the word
/// stays armed with nothing pending, so zeal owns a permanent arm.
///
/// Without this, `PERRY_GC_ZEAL=1` would silently become a no-op on the poll
/// path: every back-edge would read zero, skip the call, and force nothing —
/// and `zeal_liveness_report` would be left to report the vacuity after the
/// fact instead of the instrument simply working.
///
/// The release half is asserted, not narrated. `ZealGuard::set(true)` arms the
/// process-global word; if its `Drop` ever stopped giving that arm back, the
/// word would stay non-zero for the life of the test binary and every later
/// test would silently take the poll's slow path — with this test still green,
/// because it only ever looked INSIDE the scope. A test that cannot fail is not
/// a test.
#[test]
fn zeal_holds_the_poll_word_armed_with_nothing_pending() {
    let _isolation = GcTestIsolationGuard::new();
    crate::gc::set_safepoint_pending(false);
    let base = crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed);
    {
        let _zeal = super::super::zeal::ZealGuard::set(true);
        assert_eq!(
            crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
            base + 1,
            "zeal must keep the poll reachable even with no deferral outstanding"
        );
    }
    // And it gives the arm back, so one zeal test does not leave every later
    // test in this binary paying for the slow path.
    assert_eq!(
        crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
        base,
        "dropping the ZealGuard must release the arm it took — a leaked arm \
         pins the poll on for the rest of the process, and nothing else in this \
         binary would notice"
    );
    crate::gc::set_safepoint_pending(false);
}

/// #7781: the schedule's mirror of the zeal test above, and the regression
/// test for the gap that made `PERRY_GC_SCHEDULE_RATE=1` an event-loop-only
/// instrument: on #7606's reproduction it saw SIX safepoints against zeal's
/// 9,648 loop polls, because nothing armed the poll word for the mode whose
/// decision lives inside the safepoint the word gates.
#[test]
fn the_schedule_holds_the_poll_word_armed_like_zeal() {
    let _isolation = GcTestIsolationGuard::new();
    crate::gc::set_safepoint_pending(false);
    let base = crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed);
    {
        let _sched = super::super::schedule::ScheduleGuard::set(7, u64::MAX);
        assert_eq!(
            crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
            base + 1,
            "a live schedule must keep the poll reachable — its collection \
             decision happens inside the safepoint the word gates"
        );
    }
    assert_eq!(
        crate::gc::PERRY_GC_POLL_ARMED.load(std::sync::atomic::Ordering::Relaxed),
        base,
        "dropping the ScheduleGuard must release the arm it took"
    );
}

/// The survival-adaptive arm must CHANGE a verdict, in both directions, from
/// the same arena reading.
///
/// A test that only armed the retaining flag and checked the boundary grew
/// would pass on a build where the flag never reaches the predicate — the
/// #7024/#6942 shape this repo keeps paying for. So the assertion is on
/// `arena_growth_full_escalation_due()` itself, at a reading chosen to sit
/// strictly between the un-retained boundary and the retained one: today's
/// policy escalates there, and the retaining arm is the only thing that can
/// make it decline.
#[test]
fn retaining_survival_widens_the_escalation_band_and_low_survival_restores_it() {
    use super::super::policy::{
        arena_growth_full_escalation_due, major_pacing_config, major_pacing_retaining,
        note_copying_minor_young_survival, test_major_pacing_pre_in_use_bytes,
        test_reset_major_pacing_backoff, test_set_major_pacing_baseline,
        test_set_pacing_arena_in_use,
    };

    let (floor_bytes, growth_num) = major_pacing_config();
    if floor_bytes == 0 {
        return; // pacing disabled outright: no boundary to widen
    }

    test_reset_major_pacing_backoff();
    // Baseline high enough that the growth clause, not the floor, is the
    // boundary — the floor would mask the multiplier entirely.
    let baseline = floor_bytes;
    let previous_baseline = test_set_major_pacing_baseline(baseline);
    // Above `growth_num × baseline` (today's boundary), below `4 ×` that.
    let reading = baseline * growth_num + 1;
    let previous_reading = test_set_pacing_arena_in_use(Some(reading));

    // 1. Not retaining (the OFF state): this reading escalates, as before.
    note_copying_minor_young_survival(0);
    let off_retaining = major_pacing_retaining();
    let off_due = arena_growth_full_escalation_due();
    test_reset_major_pacing_backoff();
    test_set_major_pacing_baseline(baseline);

    // 2. Retaining: the same reading no longer escalates. Re-arm the reading
    //    first — the retaining path re-baselines from it, and `max` keeps the
    //    baseline where it is here (reading > baseline would raise it, which is
    //    itself part of the effect being asserted).
    note_copying_minor_young_survival(1000);
    let on_retaining = major_pacing_retaining();
    let on_due = arena_growth_full_escalation_due();
    let on_recorded = test_major_pacing_pre_in_use_bytes();

    // 3. A single low-survival minor disarms it again, same reading.
    note_copying_minor_young_survival(0);
    let back_retaining = major_pacing_retaining();

    test_set_pacing_arena_in_use(previous_reading);
    test_set_major_pacing_baseline(previous_baseline);
    test_reset_major_pacing_backoff();

    assert!(!off_retaining, "survival 0 must not arm the retaining arm");
    assert!(
        off_due,
        "the reading must escalate WITHOUT the retaining arm, or this test \
         proves nothing about the arm"
    );
    assert!(on_retaining, "survival 1000 must arm the retaining arm");
    assert!(
        !on_due,
        "a retaining heap must not escalate at a reading only the un-widened \
         growth band rejects"
    );
    assert_eq!(
        on_recorded, 0,
        "a declined escalation must leave no pre-full reading behind"
    );
    assert!(
        !back_retaining,
        "one non-retaining minor must disarm the band immediately — a decayed \
         window would keep pacing a churning heap as if it were retaining"
    );
}

/// The retaining re-baseline is a ratchet, never a decrease: a minor must not
/// be able to pull the boundary in below what the last full established.
#[test]
fn retaining_rebaseline_never_lowers_the_pacing_baseline() {
    use super::super::policy::{
        major_pacing_snapshot, note_copying_minor_young_survival, test_reset_major_pacing_backoff,
        test_set_major_pacing_baseline, test_set_pacing_arena_in_use,
    };

    test_reset_major_pacing_backoff();
    let high = 512 * 1024 * 1024;
    let previous_baseline = test_set_major_pacing_baseline(high);
    let previous_reading = test_set_pacing_arena_in_use(Some(1024));

    note_copying_minor_young_survival(1000);
    let (after_low, _, _) = major_pacing_snapshot();

    test_set_pacing_arena_in_use(Some(high * 2));
    note_copying_minor_young_survival(1000);
    let (after_high, _, _) = major_pacing_snapshot();

    test_set_pacing_arena_in_use(previous_reading);
    test_set_major_pacing_baseline(previous_baseline);
    test_reset_major_pacing_backoff();

    assert_eq!(
        after_low, high,
        "a small post-minor occupancy must not lower the baseline"
    );
    assert_eq!(
        after_high,
        high * 2,
        "a larger post-minor occupancy must raise it"
    );
}
