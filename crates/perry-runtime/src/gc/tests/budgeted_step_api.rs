use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

fn complete_budgeted_cycle() -> JsGcStepResult {
    let mut result = JsGcStepResult::default();
    for _ in 0..500_000 {
        js_gc_step_work_units(1, &mut result);
        match result.status {
            JS_GC_STEP_STATUS_ACTIVE => continue,
            JS_GC_STEP_STATUS_COMPLETED => return result,
            other => panic!("budgeted GC cycle stopped before completion: status {other}"),
        }
    }
    panic!("budgeted GC cycle did not complete within step limit");
}

#[test]
fn no_pressure_budgeted_step_reports_idle_without_starting_cycle() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let before = gc_collection_count();
    let mut result = JsGcStepResult::default();

    assert_eq!(js_gc_step_status(&mut result), JS_GC_STEP_STATUS_IDLE);
    assert_eq!(result.active, 0);
    assert_eq!(result.completed, 0);

    assert_eq!(
        js_gc_step_work_units(0, &mut result),
        JS_GC_STEP_STATUS_IDLE
    );
    assert_eq!(js_gc_step_us(0, &mut result), JS_GC_STEP_STATUS_IDLE);
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_IDLE
    );
    assert_eq!(gc_collection_count(), before);
}

#[test]
fn arena_pressure_budgeted_step_starts_bounded_minor_cycle() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    let _dead = young_leaf();
    trigger_guard.make_arena_trigger_due();

    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(result.active, 1);
    assert_eq!(result.completed, 0);
    assert_eq!(result.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(result.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(result.phase, GcCyclePhase::BuildValidPointerSet.ffi_code());
    assert!(result.arena_debt_bytes > 0);

    assert_eq!(js_gc_step_status(&mut result), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(result.active, 1);

    let completed = complete_budgeted_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_eq!(completed.active, 0);
    assert_eq!(completed.completed, 1);

    assert_eq!(js_gc_step_status(&mut result), JS_GC_STEP_STATUS_IDLE);
    assert_eq!(result.active, 0);
    assert_eq!(js_shadow_slot_get(0) & POINTER_MASK, live as u64);
}

#[test]
fn repeated_budgeted_steps_complete_full_cycle_and_reclaim_unreachable_objects() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_child = young_leaf();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure_with_one_capture(live_malloc, ptr_bits(live_child));
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));

    let dead_malloc_headers = allocate_dead_malloc_churn_headers(8);
    let dead_old = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_STRING);
    let dead_old_size = unsafe { (*header_from_user_ptr(dead_old as *const u8)).size as u64 };
    let freed_before = GC_STATS.with(|stats| stats.borrow().total_freed_bytes);

    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(result.collection_kind, GcCollectionKind::Full.ffi_code());
    assert_eq!(result.trigger_kind, GcTriggerKind::OldGenBytes.ffi_code());

    let completed = complete_budgeted_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_eq!(
        completed.phase,
        GcCyclePhase::Complete.ffi_code(),
        "final step should report completed phase"
    );

    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should remain tracked"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&dead_malloc_headers),
        0,
        "unreachable malloc churn should be swept"
    );
    let freed_after = GC_STATS.with(|stats| stats.borrow().total_freed_bytes);
    assert!(
        freed_after.saturating_sub(freed_before) >= dead_old_size,
        "full budgeted sweep should reclaim unreachable old-arena bytes"
    );

    assert_eq!(js_gc_step_status(&mut result), JS_GC_STEP_STATUS_IDLE);
}

#[test]
fn microsecond_budget_step_remains_bounded_on_multi_slice_heap() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    for _ in 0..5_000 {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    let before = gc_collection_count();
    let mut result = JsGcStepResult::default();
    assert_eq!(js_gc_step_us(1, &mut result), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(result.active, 1);
    assert_eq!(result.completed, 0);
    assert_eq!(gc_collection_count(), before);

    let completed = complete_budgeted_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_eq!(js_shadow_slot_get(0) & POINTER_MASK, live as u64);
}
