use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
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

fn complete_incremental_sweep(sweep: &mut IncrementalSweepState) -> SweepTraceStats {
    for _ in 0..500_000 {
        if sweep.step(1) {
            return sweep.stats();
        }
    }
    panic!("incremental sweep did not complete within step limit");
}

fn realloc_until_header_moves(mut ptr: *mut u8) -> *mut u8 {
    let original_header = unsafe { header_from_user_ptr(ptr as *const u8) };
    for payload_size in [
        1024 * 1024,
        4 * 1024 * 1024,
        16 * 1024 * 1024,
        64 * 1024 * 1024,
    ] {
        ptr = gc_realloc(ptr, payload_size);
        let current_header = unsafe { header_from_user_ptr(ptr as *const u8) };
        if current_header != original_header {
            return ptr;
        }
    }
    panic!("test requires gc_realloc to move the malloc header");
}

#[test]
fn malloc_sweep_pauses_mid_list_and_eventually_frees_dead_malloc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
        (*header_from_user_ptr(live_malloc as *const u8)).gc_flags |= GC_FLAG_MARKED;
    }
    let dead_headers = allocate_dead_malloc_churn_headers(32);

    let mut sweep = IncrementalSweepState::new(false, false, None, true);
    assert!(!sweep.step(1));
    assert!(
        tracked_malloc_headers_matching(&dead_headers) > 0,
        "tiny malloc sweep budget should pause before freeing all dead objects"
    );

    for _ in 0..500_000 {
        if sweep.step(1) {
            break;
        }
    }

    assert!(malloc_user_ptr_tracked(live_malloc));
    assert_eq!(tracked_malloc_headers_matching(&dead_headers), 0);
    assert!(sweep.stats().freed_bytes > 0);
}

#[test]
fn budgeted_malloc_sweep_revalidates_live_malloc_moved_by_realloc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let dead_headers = allocate_dead_malloc_churn_headers(32);
    let mut live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();

    let mut status = budgeted_step_until_phase(GcCyclePhase::Sweep);
    assert_eq!(status.phase, GcCyclePhase::Sweep.ffi_code());
    assert_eq!(
        js_gc_step_work_units(1, &mut status),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(
        status.phase,
        GcCyclePhase::Sweep.ffi_code(),
        "one host work unit should leave malloc sweep paused"
    );

    let old_header = unsafe { header_from_user_ptr(live_malloc as *const u8) };
    unsafe {
        assert_ne!(
            (*old_header).gc_flags & GC_FLAG_MARKED,
            0,
            "live malloc root should be marked before sweep reaches it"
        );
    }
    live_malloc = realloc_until_header_moves(live_malloc);
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    let new_header = unsafe { header_from_user_ptr(live_malloc as *const u8) };
    assert_ne!(new_header, old_header);
    unsafe {
        assert_ne!(
            (*new_header).gc_flags & GC_FLAG_MARKED,
            0,
            "gc_realloc should preserve the mark bit until sweep clears it"
        );
    }

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(malloc_user_ptr_tracked(live_malloc));
    assert_eq!(tracked_malloc_headers_matching(&dead_headers), 0);
    assert_eq!(js_shadow_slot_get(0) & POINTER_MASK, live_malloc as u64);
    unsafe {
        assert_eq!(
            (*new_header).gc_flags & GC_FLAG_MARKED,
            0,
            "resumed sweep must clear the moved malloc header's mark bit"
        );
    }
}

#[test]
fn arena_sweep_pauses_before_block_cleanup_and_preserves_live_objects() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    clear_marks();
    clear_mark_seeds();

    let live = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_STRING) as usize;
    unsafe {
        (*header_from_user_ptr(live as *const u8)).gc_flags |= GC_FLAG_MARKED;
    }
    js_shadow_slot_set(0, ptr_bits(live));

    for _ in 0..1200 {
        let _ = crate::arena::arena_alloc_gc(8 * 1024, 8, GC_TYPE_STRING);
    }
    let in_use_after_alloc = crate::arena::arena_in_use_bytes();

    let mut sweep = IncrementalSweepState::new(true, false, None, false);
    assert!(
        !sweep.step(1),
        "first tiny step should only enter arena sweep"
    );
    assert!(
        !sweep.step(1),
        "arena object sweep should pause before block cleanup"
    );
    assert_eq!(
        crate::arena::arena_in_use_bytes(),
        in_use_after_alloc,
        "partial object sweep must not reset blocks before cleanup"
    );

    let stats = complete_incremental_sweep(&mut sweep);
    assert!(stats.reset_blocks > 0);
    assert!(stats.freed_bytes > 0);
    assert!(
        crate::arena::arena_in_use_bytes() < in_use_after_alloc,
        "completed cleanup should reclaim empty general blocks"
    );
    unsafe {
        let live_header = header_from_user_ptr(live as *const u8);
        assert_eq!((*live_header).obj_type, GC_TYPE_STRING);
        assert_eq!((*live_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(js_shadow_slot_get(0) & POINTER_MASK, live as u64);
}

#[test]
fn old_generation_targeted_and_full_reclaim_are_bounded_and_publish_telemetry() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let targeted_dead = crate::arena::arena_alloc_gc_old(900 * 1024, 8, GC_TYPE_STRING) as usize;
    let targeted_header = unsafe { header_from_user_ptr(targeted_dead as *const u8) };
    let targeted_total = unsafe { (*targeted_header).size as usize };
    let mut targeted_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in
        crate::arena::old_object_page_overlaps(targeted_header as usize, targeted_total)
    {
        targeted_pages.insert(page);
    }
    let targeted_blocks = crate::arena::old_arena_source_blocks_for_pages(&targeted_pages);
    assert!(!targeted_blocks.block_indices.is_empty());

    let _untargeted_dead = crate::arena::arena_alloc_gc_old(900 * 1024, 8, GC_TYPE_STRING) as usize;
    let mut targeted_sweep = IncrementalSweepState::new(
        false,
        false,
        Some(targeted_blocks.block_indices.clone()),
        false,
    );
    assert!(
        !targeted_sweep.step(1),
        "targeted old reclaim should not complete in one work unit"
    );
    let targeted_stats = complete_incremental_sweep(&mut targeted_sweep);
    assert!(targeted_stats.returned_bytes > 0 || targeted_stats.reusable_bytes > 0);
    let targeted_summary = crate::arena::old_page_summary();
    assert_eq!(
        targeted_summary.returned_bytes,
        targeted_stats.returned_bytes
    );
    assert_eq!(
        targeted_summary.reusable_bytes,
        targeted_stats.reusable_bytes
    );

    let _full_dead_a = crate::arena::arena_alloc_gc_old(900 * 1024, 8, GC_TYPE_STRING) as usize;
    let _full_dead_b = crate::arena::arena_alloc_gc_old(900 * 1024, 8, GC_TYPE_STRING) as usize;
    crate::arena::old_pages_begin_gc_cycle();
    let mut full_sweep = IncrementalSweepState::new(false, true, None, false);
    assert!(
        !full_sweep.step(1),
        "full old reclaim should not complete in one work unit"
    );
    let full_stats = complete_incremental_sweep(&mut full_sweep);
    assert!(full_stats.returned_bytes > 0 || full_stats.reusable_bytes > 0);
    let full_summary = crate::arena::old_page_summary();
    assert_eq!(full_summary.returned_bytes, full_stats.returned_bytes);
    assert_eq!(full_summary.reusable_bytes, full_stats.reusable_bytes);
}

#[test]
fn budgeted_sweep_phase_requires_multiple_host_steps() {
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
    let dead_headers = allocate_dead_malloc_churn_headers(64);
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();

    let mut status = budgeted_step_until_phase(GcCyclePhase::Sweep);
    assert_eq!(status.phase, GcCyclePhase::Sweep.ffi_code());

    assert_eq!(
        js_gc_step_work_units(1, &mut status),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(
        status.phase,
        GcCyclePhase::Sweep.ffi_code(),
        "one host work unit should not finish sweep"
    );
    assert!(
        tracked_malloc_headers_matching(&dead_headers) > 0,
        "sweep should pause with dead malloc work remaining"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(malloc_user_ptr_tracked(live_malloc));
    assert_eq!(tracked_malloc_headers_matching(&dead_headers), 0);
}

#[test]
fn budgeted_reclaim_phase_is_split_from_completion() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    let dead_headers = allocate_dead_malloc_churn_headers(8);
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();

    let mut status = budgeted_step_until_phase(GcCyclePhase::Reclaim);
    let before = gc_collection_count();
    assert_eq!(
        js_gc_step_work_units(1, &mut status),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(status.phase, GcCyclePhase::Reclaim.ffi_code());
    assert_eq!(
        gc_collection_count(),
        before,
        "first reclaim substep should not publish cycle completion"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(gc_collection_count() > before);
    assert_eq!(tracked_malloc_headers_matching(&dead_headers), 0);
    assert_eq!(js_shadow_slot_get(0) & POINTER_MASK, live as u64);
}
