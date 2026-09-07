use super::super::*;
use super::support::*;

const NURSERY_OBJECT_BYTES: usize = 8 * 1024;

struct TriggerArmedTestGuard(bool);

impl TriggerArmedTestGuard {
    fn new() -> Self {
        Self(GC_TRIGGER_ARMED.with(std::cell::Cell::get))
    }
}

impl Drop for TriggerArmedTestGuard {
    fn drop(&mut self) {
        GC_TRIGGER_ARMED.with(|armed| armed.set(self.0));
    }
}

struct MajorPacingReadingTestGuard(Option<usize>);

impl MajorPacingReadingTestGuard {
    fn keep_minor() -> Self {
        Self(super::super::policy::test_set_pacing_arena_in_use(Some(0)))
    }
}

impl Drop for MajorPacingReadingTestGuard {
    fn drop(&mut self) {
        super::super::policy::test_set_pacing_arena_in_use(self.0);
    }
}

fn reset_old_reclaim_pressure() {
    let old_in_use =
        old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

fn fill_dead_nursery_to(bytes: usize) {
    while crate::arena::copying_from_space_in_use_bytes() < bytes {
        let _ = crate::arena::arena_alloc_gc(NURSERY_OBJECT_BYTES, 8, crate::gc::GC_TYPE_STRING);
    }
}

fn collect_unrooted_nursery() {
    gc_collect_forced_evacuating_minor(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current();
}

/// Sabotage: compare `arena_total_bytes()` instead of
/// `arena_trigger_total_bytes()` in `gc_budgeted_due_trigger`. The filled
/// nursery then crosses `next_base`, increments the budgeted-start counter,
/// and makes the first assertion fail.
#[test]
fn full_dead_nursery_does_not_arm_budgeted_cycle_and_next_minor_reclaims_it() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _moving = force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _armed = TriggerArmedTestGuard::new();
    let _minor = MajorPacingReadingTestGuard::keep_minor();
    reset_old_reclaim_pressure();

    let old_total = arena_trigger_total_bytes();
    let next_base = old_total.saturating_add(gc_trigger_headroom_floor_bytes());
    GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(next_base));
    GC_TRIGGER_ARMED.with(|armed| armed.set(true));

    fill_dead_nursery_to(gc_scavenge_nursery_cap_bytes());
    let from_space_before = crate::arena::copying_from_space_in_use_bytes();
    assert!(
        from_space_before >= gc_scavenge_nursery_cap_bytes(),
        "fixture must fill the copying nursery to its cap"
    );
    assert_eq!(
        arena_trigger_total_bytes(),
        old_total,
        "nursery allocation must not grow the old-space trigger quantity"
    );
    assert!(
        crate::arena::arena_total_bytes() >= next_base,
        "SABOTAGE WITNESS: restoring whole-arena bytes must make the old predicate due"
    );

    let starts_before = super::super::instruments::incremental_cycle_starts();
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_IDLE,
        "a full nursery with old space below next_base must not start budgeted work"
    );
    assert_eq!(
        super::super::instruments::incremental_cycle_starts(),
        starts_before,
        "the budgeted-cycle start counter prices the work this test excludes"
    );

    let copied_before = copying_minor_cycles();
    assert!(
        gc_safepoint_moving_minor(),
        "the precise safepoint must consume the deferred nursery arm"
    );
    assert_eq!(
        copying_minor_cycles(),
        copied_before + 1,
        "the replacement copying minor must actually run"
    );
    assert!(
        crate::arena::copying_from_space_in_use_bytes() < from_space_before,
        "the next minor must reclaim the dead nursery"
    );
}

/// Sabotage: omit `nursery_reserved_bytes_sub` from
/// `finish_in_place_promotion`. The promoted blocks remain excluded, so the
/// ArenaBytes arm stays below `next_base` and the start counter does not move.
#[test]
fn promotion_past_old_space_headroom_arms_one_budgeted_cycle_with_empty_nursery() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _legacy = force_legacy_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _armed = TriggerArmedTestGuard::new();

    collect_unrooted_nursery();
    assert_eq!(crate::arena::copying_from_space_in_use_bytes(), 0);

    let old_total_before = arena_trigger_total_bytes();
    let next_base = old_total_before.saturating_add(gc_trigger_headroom_floor_bytes());
    GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(next_base));
    GC_TRIGGER_ARMED.with(|armed| armed.set(true));

    fill_dead_nursery_to(gc_trigger_headroom_floor_bytes() + crate::arena::BLOCK_SIZE);
    let total_before_promotion = crate::arena::arena_total_bytes();
    let promotion = crate::arena::retag_young_for_in_place_promotion(false);
    assert!(
        promotion.reserved_bytes() > gc_trigger_headroom_floor_bytes(),
        "fixture must promote more than the unchanged headroom"
    );
    crate::arena::finish_in_place_promotion(
        promotion,
        crate::arena::PromotionLiveness::AssumeAllLive,
    );

    assert_eq!(
        crate::arena::arena_total_bytes(),
        total_before_promotion,
        "promotion transfers capacity without reserving more"
    );
    assert_eq!(
        crate::arena::copying_from_space_in_use_bytes(),
        0,
        "the promoted nursery must be empty before the arming decision"
    );
    assert!(
        arena_trigger_total_bytes() >= next_base,
        "promoted capacity must become visible to old-space pacing"
    );
    reset_old_reclaim_pressure();

    let starts_before = super::super::instruments::incremental_cycle_starts();
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(result.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(
        super::super::instruments::incremental_cycle_starts(),
        starts_before + 1,
        "old-space growth past the headroom must arm exactly one cycle"
    );
    assert_eq!(
        complete_budgeted_gc_cycle().status,
        JS_GC_STEP_STATUS_COMPLETED
    );

    // The promoted fixture is deliberately unrooted; clean it out of old-gen
    // so this sabotage test leaves no large live-looking cohort behind.
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
        .emit_after_current();
}
