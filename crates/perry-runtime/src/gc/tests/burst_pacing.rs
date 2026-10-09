use super::super::*;
use super::support::*;

fn reset_old_pressure() {
    policy::GC_OLD_RECLAIM_PENDING.with(|p| p.set(false));
    policy::GC_LAST_OLD_RECLAIM_IN_USE_BYTES
        .with(|b| b.set(policy::old_gen_reclaimable_pressure_bytes()));
}

#[test]
fn burst_pacing_arena_polls_leave_young_garbage_to_equal_minors() {
    let _placement = policy::ByteStorePolicyTestGuard::new(usize::MAX);
    let _isolation = CopyingNurseryTestGuard::new(2);
    let _moving = policy::force_moving_gc_pacing();
    let thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    js_shadow_slot_set(0, string_bits(young_leaf()));
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current();
    let baseline = policy::test_set_major_pacing_baseline(usize::MAX / 4);
    let keep = crate::buffer::js_buffer_alloc(1024 * 1024, 7);
    js_shadow_slot_set(1, ptr_bits(keep as usize));
    let mut minors = [0, 0];
    for (arm, count) in minors.iter_mut().enumerate() {
        for _ in 0..4 {
            for _ in 0..20_000 {
                let _ = young_leaf();
            }
            reset_old_pressure();
            thresholds.make_arena_trigger_due();
            if arm == 1 {
                let result = gc_runtime_safepoint();
                assert_eq!(result.status, JS_GC_STEP_STATUS_IDLE);
                assert!(
                    !policy::gc_budgeted_cycle_active(),
                    "arena capacity opened an old/budgeted cycle"
                );
            }
            let before = gc_collection_count();
            assert!(policy::gc_safepoint_moving_minor());
            assert_eq!(gc_collection_count(), before + 1);
            *count += 1;
        }
    }
    policy::test_set_major_pacing_baseline(baseline);
    assert_eq!(minors, [4, 4]);
    assert_ne!(js_shadow_slot_get(0), 0);
}

#[test]
fn burst_pacing_old_growth_still_starts_full_cycle() {
    let _placement = policy::ByteStorePolicyTestGuard::new(usize::MAX);
    let _isolation = CopyingNurseryTestGuard::new(1);
    let _moving = policy::force_moving_gc_pacing();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current();
    reset_old_pressure();
    // Large ordinary objects are born old. Unlike block rounding, this is
    // real growth, and no live root protects these allocations from reclaim.
    let band = policy::gc_old_reclaim_growth_band_bytes(
        policy::GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|b| b.get()),
    );
    for _ in 0..(band / (1024 * 1024) + 2) {
        let _ = crate::buffer::js_buffer_alloc(1024 * 1024, 1);
    }
    let result = gc_runtime_safepoint();
    assert_eq!(result.status, JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(result.collection_kind, GcCollectionKind::Full.ffi_code());
    assert_eq!(result.trigger_kind, GcTriggerKind::OldGenBytes.ffi_code());
    assert_eq!(
        complete_budgeted_gc_cycle().status,
        JS_GC_STEP_STATUS_COMPLETED
    );
}

#[test]
fn burst_pacing_large_cycle_finishes_within_bounded_automatic_polls() {
    let _isolation = CopyingNurseryTestGuard::new(1);
    let _moving = policy::force_moving_gc_pacing();
    let thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current();
    js_shadow_slot_set(0, string_bits(young_leaf()));
    gc_suppress();
    for _ in 0..300_000 {
        let _ = young_leaf();
    }
    gc_unsuppress();
    reset_old_pressure();
    // Explicit requests remain precisely sliced, providing an independent
    // control for the automatic progress budget.
    thresholds.make_arena_trigger_due();
    let mut status = JsGcStepResult::default();
    for _ in 0..4 {
        assert_eq!(
            js_gc_step_work_units(1, &mut status),
            JS_GC_STEP_STATUS_ACTIVE
        );
        assert_eq!(status.phase, GcCyclePhase::BuildValidPointerSet.ffi_code());
    }
    let mut polls = 0;
    while policy::gc_budgeted_cycle_active() && polls < usize::BITS + 8 {
        gc_runtime_safepoint_poll();
        polls += 1;
    }
    assert!(
        !policy::gc_budgeted_cycle_active(),
        "cycle still active after {polls} automatic polls"
    );
    assert!(polls > 1, "fixture must exercise sliced work");
}

#[test]
fn burst_pacing_nursery_live_census_does_not_escalate_to_old_full() {
    let _isolation = CopyingNurseryTestGuard::new(1);
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current();
    let baseline = policy::test_set_major_pacing_baseline(0);
    let (floor, _) = policy::major_pacing_config();
    // A large retained young object supplies an actual nursery census. Small
    // arena allocations are not promoted merely by publishing that census.
    gc_suppress();
    while crate::arena::arena_live_allocated_bytes() < floor + 1024 {
        let _ = young_leaf();
    }
    gc_unsuppress();
    policy::note_collection_finished_arena_occupancy(false);
    assert!(!policy::arena_growth_full_escalation_due());
    policy::test_set_major_pacing_baseline(baseline);
}

#[test]
fn burst_pacing_weak_dependency_rounds_finish_within_bounded_automatic_polls() {
    weak_dependency_cycle_finishes(false);
}

#[test]
fn burst_pacing_parked_weak_dependency_cycle_finishes_within_bounded_slices() {
    weak_dependency_cycle_finishes(true);
}

fn weak_dependency_cycle_finishes(parked: bool) {
    use super::super::idle_reclaim::test_support::*;
    let _idle = IdleReclaimTestGuard::new(0);
    set_test_max_slices(Some(1));
    set_test_slice_us(Some(0));
    let _isolation = CopyingNurseryTestGuard::new(2);
    let _moving = policy::force_moving_gc_pacing();
    let thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current();
    gc_suppress();
    let map = ptr_bits(crate::weakref::js_weakmap_new() as usize);
    let keys: Vec<_> = (0..129)
        .map(|_| ptr_bits(crate::array::js_array_alloc(0) as usize))
        .collect();
    // Reverse entry order requires another conditional marking round for
    // each key. A large work budget alone cannot finish that fixed point.
    for i in (0..128).rev() {
        crate::weakref::js_weakmap_set(
            f64::from_bits(map),
            f64::from_bits(keys[i]),
            f64::from_bits(keys[i + 1]),
        );
    }
    js_shadow_slot_set(0, map);
    js_shadow_slot_set(1, keys[0]);
    gc_unsuppress();
    reset_old_pressure();
    thresholds.make_arena_trigger_due();
    let mut status = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut status),
        JS_GC_STEP_STATUS_ACTIVE
    );
    let mut polls = 0;
    while policy::gc_budgeted_cycle_active() && polls < usize::BITS + 8 {
        if parked {
            idle_reclaim_park_hook(1000);
        } else {
            gc_runtime_safepoint_poll();
        }
        polls += 1;
    }
    assert!(
        !policy::gc_budgeted_cycle_active(),
        "conditional marking still active after {polls} automatic polls"
    );
    assert_eq!(
        crate::weakref::js_weakmap_get(f64::from_bits(map), f64::from_bits(keys[127])).to_bits(),
        keys[128],
        "the final conditional value must survive"
    );
}
