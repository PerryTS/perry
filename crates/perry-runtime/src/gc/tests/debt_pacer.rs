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

    let mut host_finalize_steps = 0usize;
    while status.phase == GcCyclePhase::AtomicFinalize.ffi_code() {
        assert_eq!(
            js_gc_step_work_units(1, &mut status),
            JS_GC_STEP_STATUS_ACTIVE
        );
        host_finalize_steps += 1;
        assert!(
            host_finalize_steps < 100_000,
            "host-driven atomic finalize did not finish"
        );
        assert!(
            status.phase == GcCyclePhase::AtomicFinalize.ffi_code()
                || status.phase == GcCyclePhase::Sweep.ffi_code(),
            "host-driven finalization should stay in atomic finalize or advance to sweep"
        );
    }
    assert!(
        host_finalize_steps > 0,
        "host step should advance through atomic finalize"
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

    let mut saw_partial_sweep = false;
    for _ in 0..500_000 {
        assert_eq!(
            js_gc_step_work_units(1, &mut status),
            JS_GC_STEP_STATUS_ACTIVE
        );
        if status.phase == GcCyclePhase::Reclaim.ffi_code() {
            break;
        }
        assert_eq!(status.phase, GcCyclePhase::Sweep.ffi_code());
        let remaining = tracked_malloc_headers_matching(&churn_headers);
        if remaining < churn_headers.len() {
            saw_partial_sweep = true;
        }
        assert_eq!(
            gc_collection_count(),
            before,
            "host-driven incremental sweep must not complete the cycle"
        );
    }
    assert!(
        saw_partial_sweep,
        "host-driven sweep should pause after reclaiming part of malloc churn"
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

fn noop_copy_only_root_scanner(_visit: &mut dyn FnMut(f64)) {}

/// Regression: the direct synchronous minor — taken whenever synchronous-only
/// root scanners block the budgeted stepper, i.e. in every compiled program —
/// must re-baseline the arming trigger on completion, exactly as the budgeted
/// finisher (`gc_finish_budgeted_cycle`) does.
///
/// The bug: this arm merely emitted the outcome, leaving `GC_NEXT_TRIGGER_BYTES`
/// at the value that armed the collection. The non-moving minor reclaims dead
/// objects into per-block free lists without lowering `arena_total`, so a
/// workload holding a large live set above the trigger kept
/// `gc_budgeted_due_trigger` reporting the trigger as due and re-armed a whole-
/// arena mark/sweep on every fresh block — O(n^2), a ~100% CPU stall with a
/// bounded live set that never made progress.
#[test]
fn direct_arena_minor_rebaselines_trigger_above_live_set() {
    let _nursery = CopyingNurseryTestGuard::new(1);
    // A registered copy-only scanner makes the budgeted stepper ineligible, so
    // gc_check_trigger takes the direct synchronous-minor arm.
    let _scanners = ScopedRootScannerRegistryGuard::new();
    gc_register_root_scanner(noop_copy_only_root_scanner);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"direct_minor_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }

    // Arm the arena trigger (sets GC_NEXT_TRIGGER_BYTES = 0) so it is due.
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();

    gc_check_trigger();

    // The direct arm runs a synchronous collection to completion (unlike the
    // budgeted stepper, which would only arm an assist here)...
    assert!(
        gc_collection_count() > before,
        "a registered synchronous-only scanner should drive gc_check_trigger \
         through the direct synchronous minor, not the budgeted stepper"
    );
    // ...and re-baselines the arena trigger above the retained live set, so the
    // next allocation does not immediately re-arm another whole-arena minor.
    let next_trigger = GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get());
    let arena_total = crate::arena::arena_total_bytes();
    assert!(
        next_trigger > arena_total,
        "direct minor must rebaseline the arena trigger above arena_total \
         (next_trigger={next_trigger}, arena_total={arena_total}); leaving it at \
         the arming value re-triggers a full minor on every block"
    );
}

/// Companion to the arena case for the `MallocCount` arm: the direct minor must
/// dispatch to `gc_finish_malloc_trigger_collection`, which sweeps malloc (its
/// `debug_assert!(outcome.malloc_swept)` is exercised here) and re-baselines
/// `GC_NEXT_MALLOC_TRIGGER` to `survivors + step`. Without the re-baseline the
/// malloc trigger stays at the arming value and every tracked allocation
/// re-arms a full synchronous minor.
#[test]
fn direct_malloc_minor_rebaselines_trigger_above_survivors() {
    let _nursery = CopyingNurseryTestGuard::new(1);
    let _scanners = ScopedRootScannerRegistryGuard::new();
    gc_register_root_scanner(noop_copy_only_root_scanner);
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

    // Arm the malloc-count trigger so the direct minor takes the MallocCount arm.
    let malloc_count = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_count.saturating_sub(1)));

    let before = gc_collection_count();
    gc_check_trigger();

    // The direct arm runs a synchronous collection to completion...
    assert!(
        gc_collection_count() > before,
        "a registered synchronous-only scanner should drive the MallocCount \
         trigger through the direct synchronous minor, not the budgeted stepper"
    );
    // ...and re-baselines the malloc trigger to survivors + step (the same
    // formula the budgeted finisher applies), leaving it strictly above the
    // surviving count so the next allocation does not immediately re-arm.
    let survivors_after = malloc_object_count();
    let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get());
    assert_eq!(
        next_malloc_trigger,
        survivors_after + malloc_step_after,
        "direct malloc minor must rebaseline GC_NEXT_MALLOC_TRIGGER to \
         survivors + step (next={next_malloc_trigger}, survivors={survivors_after}, \
         step={malloc_step_after})"
    );
    assert!(next_malloc_trigger > survivors_after);
}
