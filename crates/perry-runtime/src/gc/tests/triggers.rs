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
