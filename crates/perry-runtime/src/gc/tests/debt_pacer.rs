use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

fn live_test_string(bytes: &'static [u8]) -> usize {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

fn budgeted_step_until_phase(target: GcCyclePhase) -> JsGcStepResult {
    let mut status = JsGcStepResult::default();
    for _ in 0..500_000 {
        let current = js_gc_step_status(&mut status);
        if current == JS_GC_STEP_STATUS_ACTIVE && status.phase == target.ffi_code() {
            return status;
        }
        let stepped = js_gc_step_work_units(1, &mut status);
        if stepped == JS_GC_STEP_STATUS_ACTIVE && status.phase == target.ffi_code() {
            return status;
        }
        assert_ne!(
            stepped, JS_GC_STEP_STATUS_COMPLETED,
            "budgeted cycle completed before reaching phase {target:?}"
        );
    }
    panic!("budgeted cycle did not reach phase {target:?}");
}

#[test]
fn arena_threshold_debt_starts_bounded_assist_without_monolithic_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"arena_debt_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    let before = gc_collection_count();
    gc_check_trigger();

    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(status.active, 1);
    assert_eq!(status.completed, 0);
    assert!(status.arena_debt_bytes > 0);
    assert_eq!(
        gc_collection_count(),
        before,
        "arena pressure should not complete a synchronous collection"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(gc_collection_count() > before);
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"arena_debt_live");
    }
    assert!(
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get()) > crate::arena::arena_total_bytes(),
        "completed arena debt cycle should rebaseline the heap goal"
    );
}

#[test]
fn malloc_threshold_debt_reclaims_dead_churn_after_host_drain() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len()
    );
    let malloc_count = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_count.saturating_sub(1)));

    let before = gc_collection_count();
    gc_check_trigger();

    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(status.trigger_kind, GcTriggerKind::MallocCount.ffi_code());
    assert!(status.malloc_debt_objects > 0);
    assert_eq!(
        gc_collection_count(),
        before,
        "malloc pressure should be assisted, not synchronously collected"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive the completed debt cycle"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "dead malloc churn should be reclaimed once debt is drained"
    );

    let survivors_after = malloc_object_count();
    let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
    assert_eq!(
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()),
        survivors_after + malloc_step_after
    );
}

#[test]
fn active_cycle_gc_check_trigger_calls_pay_bounded_assist_work() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"active_assist_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 8) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    let before = gc_collection_count();
    gc_check_trigger();
    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);

    gc_check_trigger();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(
        gc_collection_count(),
        before,
        "active-cycle assists must not start a nested synchronous collection"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(gc_collection_count() > before);
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"active_assist_live");
    }
}

#[test]
fn allocation_assists_stop_before_unsliced_finalize_and_sweep() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len()
    );
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));

    let before = gc_collection_count();
    gc_check_trigger();

    let mut status = budgeted_step_until_phase(GcCyclePhase::AtomicFinalize);
    assert_eq!(status.status, JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.phase, GcCyclePhase::AtomicFinalize.ffi_code());

    for _ in 0..8 {
        gc_check_trigger();
        assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
        assert_eq!(
            status.phase,
            GcCyclePhase::AtomicFinalize.ffi_code(),
            "allocation-side assist must not run atomic finalize"
        );
        assert_eq!(
            gc_collection_count(),
            before,
            "allocation-side assist must not complete the cycle"
        );
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            churn_headers.len(),
            "allocation-side assist must not reach malloc sweep through finalize"
        );
    }

    assert_eq!(
        js_gc_step_work_units(1, &mut status),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(status.phase, GcCyclePhase::Sweep.ffi_code());

    for _ in 0..8 {
        gc_check_trigger();
        assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
        assert_eq!(
            status.phase,
            GcCyclePhase::Sweep.ffi_code(),
            "allocation-side assist must not run the unsliced sweep"
        );
        assert_eq!(
            gc_collection_count(),
            before,
            "allocation-side assist must not complete the cycle"
        );
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            churn_headers.len(),
            "allocation-side assist must not reclaim malloc churn from sweep"
        );
    }

    assert_eq!(
        js_gc_step_work_units(1, &mut status),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(status.phase, GcCyclePhase::Reclaim.ffi_code());
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "host-driven sweep should reclaim dead malloc churn before reclaim"
    );

    for _ in 0..8 {
        gc_check_trigger();
        assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
        assert_eq!(
            status.phase,
            GcCyclePhase::Reclaim.ffi_code(),
            "allocation-side assist must not run unsliced reclaim"
        );
        assert_eq!(
            gc_collection_count(),
            before,
            "allocation-side assist must not complete the cycle from reclaim"
        );
    }

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive after host drains the cycle"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "host-drained sweep should reclaim dead malloc churn"
    );
}
