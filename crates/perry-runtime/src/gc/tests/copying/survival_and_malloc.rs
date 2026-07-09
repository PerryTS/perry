use super::*;

#[test]
fn test_copying_minor_promotes_survivor_on_fourth_survival() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let _ = gc_collect_minor();
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(survivor));

    let _ = gc_collect_minor();
    let survivor_second = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor_second, survivor);
    assert!(crate::arena::pointer_in_nursery(survivor_second));

    let _ = gc_collect_minor();
    let survivor_third = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor_third, survivor_second);
    assert!(crate::arena::pointer_in_nursery(survivor_third));

    let _ = gc_collect_minor();
    let promoted = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(promoted, survivor_third);
    assert!(crate::arena::pointer_in_old_gen(promoted));
}

#[test]
fn test_copying_minor_preserves_old_page_accounting_for_defrag_policy() {
    struct ResetGcTestState {
        pinned_header: *mut GcHeader,
    }

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
            if !self.pinned_header.is_null() {
                unsafe {
                    (*self.pinned_header).gc_flags &= !GC_FLAG_PINNED;
                }
            }
        }
    }

    let mut reset = ResetGcTestState {
        pinned_header: std::ptr::null_mut(),
    };
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let first_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&first_trace, true, CopiedMinorFallbackReason::None, false);
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor, child);
    assert!(crate::arena::pointer_in_nursery(survivor));

    let second_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&second_trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(second_trace.copying_nursery.promoted_objects, 0);
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(survivor));

    let third_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&third_trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(third_trace.copying_nursery.promoted_objects, 0);
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(survivor));

    let survivor_header = unsafe { header_from_user_ptr(survivor as *const u8) };
    let survivor_total = unsafe { (*survivor_header).size as usize };

    crate::arena::old_pages_begin_gc_cycle();
    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (live_header, live_total) = old_test_header_and_size(live);
    let (_dead_header, dead_total) = old_test_header_and_size(dead);
    let mut fragmented_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(live_header as usize, live_total) {
        fragmented_pages.insert(page);
    }
    for (page, _) in crate::arena::old_object_page_overlaps(dead - GC_HEADER_SIZE, dead_total) {
        fragmented_pages.insert(page);
    }
    let pinned =
        crate::arena::arena_alloc_gc_old_excluding_pages(40, 8, GC_TYPE_STRING, &fragmented_pages)
            as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    reset.pinned_header = pinned_header;

    unsafe {
        (*survivor_header).gc_flags |= GC_FLAG_MARKED;
        (*live_header).gc_flags |= GC_FLAG_MARKED;
        (*pinned_header).gc_flags |= GC_FLAG_PINNED;
    }

    let sweep = sweep_with_age_bump(false);
    let before_summary = crate::arena::old_page_summary();
    let before_selection = select_old_page_defrag_pages(false);

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "seeded dead old object should be observed by sweep accounting"
    );
    assert!(
        before_summary.dead_bytes >= dead_total,
        "old-page summary should include seeded dead bytes before copied minor"
    );
    assert!(
        before_summary.pinned_bytes >= pinned_total,
        "old-page summary should include seeded pinned bytes before copied minor"
    );
    assert!(
        before_selection.selected_pages > 0,
        "seeded unpinned live/dead old page should be selected for defrag"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let promoted = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let promoted_header = unsafe { header_from_user_ptr(promoted as *const u8) };
    let promoted_total = unsafe { (*promoted_header).size as usize };
    let promoted_page_count =
        crate::arena::old_object_page_overlaps(promoted_header as usize, promoted_total).len();
    let post_summary = crate::arena::old_page_summary();
    let after_selection = select_old_page_defrag_pages(false);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(promoted, survivor);
    assert!(crate::arena::pointer_in_old_gen(promoted));
    assert_eq!(promoted_total, survivor_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 1);
    assert_eq!(trace.copying_nursery.promoted_bytes, survivor_total);
    assert_eq!(trace.old_pages, post_summary);
    assert_eq!(trace.old_pages.dead_bytes, before_summary.dead_bytes);
    assert_eq!(
        trace.old_pages.dead_object_count,
        before_summary.dead_object_count
    );
    assert_eq!(trace.old_pages.pinned_bytes, before_summary.pinned_bytes);
    assert_eq!(
        trace.old_pages.pinned_object_count,
        before_summary.pinned_object_count
    );
    assert_eq!(
        post_summary.live_bytes,
        before_summary.live_bytes + survivor_total
    );
    assert_eq!(
        post_summary.live_object_count,
        before_summary.live_object_count + promoted_page_count
    );
    assert!(
        after_selection.selected_pages > 0,
        "copied minor must leave old-page defrag candidates selectable"
    );
}

#[test]
fn test_copying_minor_sticky_old_to_survivor_edge_promotes_on_fourth_cycle() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = young_leaf();
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));

    let _ = gc_collect_minor();
    let survivor = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(survivor));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let survivor_second = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(survivor_second));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let survivor_third = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(survivor_third));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let promoted = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_old_gen(promoted));
}

#[test]
fn test_copying_minor_resets_eden_wholesale() {
    let _guard = CopyingNurseryTestGuard::new(1);
    for _ in 0..128 {
        let _ = young_leaf();
    }
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));

    let _ = gc_collect_minor();
    let snapshot = crate::arena::arena_telemetry_snapshot();
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_eq!(snapshot.arena.in_use_bytes, 0);
    assert!(crate::arena::pointer_in_nursery(live_after));
}

#[test]
fn test_copying_minor_sweeps_malloc_when_due_on_arena_trigger() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert!(copied_minor_malloc_sweep_due(GcTriggerKind::MallocCount));
    let live_young = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live_young));
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(1, ptr_bits(live_malloc as usize));
    activate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(32);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len(),
        "malloc churn should be tracked before the collection"
    );
    let tracked_before = malloc_object_count();
    trigger_guard.make_malloc_sweep_due();
    assert!(copied_minor_malloc_sweep_due(GcTriggerKind::ArenaBytes));

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "copied-minor GC must sweep dead malloc churn when malloc pressure is due"
    );
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive copied-minor malloc sweep"
    );
    assert!(
        malloc_object_count() < tracked_before,
        "malloc sweep should reduce the tracked malloc object count"
    );
    assert!(
        outcome.freed_bytes > 0,
        "copied-minor path should report malloc reclaim"
    );
}

#[test]
fn test_gc_check_trigger_copied_minor_malloc_sweep_rebaselines_trigger() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    activate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(48);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len(),
        "malloc churn should be tracked before gc_check_trigger"
    );
    let tracked_before = malloc_object_count();
    trigger_guard.make_malloc_sweep_due();
    let collections_before = gc_collection_count();

    gc_check_trigger();

    let mut step_status = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_status(&mut step_status),
        JS_GC_STEP_STATUS_ACTIVE,
        "gc_check_trigger should schedule malloc pressure as bounded assist work"
    );
    assert_eq!(
        gc_collection_count(),
        collections_before,
        "gc_check_trigger must not complete malloc pressure synchronously"
    );
    assert_eq!(
        step_status.trigger_kind,
        GcTriggerKind::MallocCount.ffi_code()
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        gc_collection_count() > collections_before,
        "draining the budgeted malloc-pressure cycle should collect"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "copied-minor collection should reclaim dead malloc churn"
    );
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive gc_check_trigger collection"
    );
    let survivors_after = malloc_object_count();
    assert!(
        survivors_after < tracked_before,
        "malloc sweep should reduce MALLOC_STATE.objects"
    );
    let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get());
    assert_eq!(
        next_malloc_trigger,
        survivors_after + malloc_step_after,
        "gc_check_trigger should rebaseline the next malloc trigger to survivors + step"
    );
}

#[test]
fn test_gc_check_trigger_copied_minor_without_malloc_sweep_preserves_malloc_trigger() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    deactivate_malloc_registry_for_tests();

    let live_young = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live_young));
    let churn_headers = allocate_dead_malloc_churn_headers(48);
    let tracked_before = tracked_malloc_headers_matching(&churn_headers);
    assert_eq!(
        tracked_before,
        churn_headers.len(),
        "malloc churn should be tracked before gc_check_trigger"
    );

    let malloc_count_before = malloc_object_count();
    let next_malloc_trigger = malloc_count_before + 1;
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(next_malloc_trigger));
    trigger_guard.make_arena_trigger_due();
    assert!(
        !copied_minor_malloc_sweep_due(GcTriggerKind::ArenaBytes),
        "arena-triggered copied-minor should not sweep malloc while below malloc pressure"
    );

    let collections_before = gc_collection_count();
    gc_check_trigger();

    let mut step_status = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_status(&mut step_status),
        JS_GC_STEP_STATUS_ACTIVE,
        "gc_check_trigger should schedule arena pressure as bounded assist work"
    );
    assert_eq!(
        gc_collection_count(),
        collections_before,
        "gc_check_trigger must not complete arena pressure synchronously"
    );
    assert_eq!(
        step_status.trigger_kind,
        GcTriggerKind::ArenaBytes.ffi_code()
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        gc_collection_count() > collections_before,
        "draining the budgeted arena-pressure cycle should collect"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        tracked_before,
        "malloc sweep was not due, so dead churn should remain tracked"
    );
    assert_eq!(
        malloc_object_count(),
        malloc_count_before,
        "copied-minor collection should not sweep malloc while below malloc pressure"
    );
    assert_eq!(
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()),
        next_malloc_trigger,
        "arena-triggered copied-minor without malloc sweep must preserve the existing malloc trigger"
    );
}

#[test]
fn test_copied_minor_malloc_scaling_no_roots_skips_registry_walk() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    deactivate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(512);
    let tracked_before = tracked_malloc_headers_matching(&churn_headers);
    assert_eq!(tracked_before, churn_headers.len());
    let live_young = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live_young));

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        trace.copying_nursery.malloc_validation_lookups, 0,
        "copied-minor should not probe malloc entries when no roots mention malloc"
    );
    assert_eq!(
        trace.copying_nursery.malloc_registry_rebuilds, 0,
        "copied-minor must not rebuild the malloc registry"
    );
    assert!(
        !malloc_registry_active_for_tests(),
        "copied-minor should leave an inactive malloc registry inactive"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        tracked_before,
        "malloc sweep was not due, so dead churn should remain tracked without being walked"
    );
}

#[test]
fn test_copied_minor_malloc_scaling_live_root_with_active_registry() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_child = young_leaf();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>(),
        GC_TYPE_CLOSURE,
    );
    let capture_slot =
        unsafe { init_test_closure_with_one_capture(live_malloc, ptr_bits(live_child)) };
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    activate_malloc_registry_for_tests();
    assert!(malloc_registry_active_for_tests());

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    trigger_guard.make_malloc_sweep_due();
    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
    assert!(
        trace.copying_nursery.malloc_validation_lookups > 0,
        "active registry should validate the live malloc root"
    );
    assert!(
        trace.copying_nursery.malloc_validation_lookups < churn_headers.len(),
        "malloc validation should scale with reachable candidates, not dead churn"
    );
    assert_eq!(
        trace.copying_nursery.malloc_registry_rebuilds, 0,
        "copied-minor should use the active registry without rebuilding it"
    );
    assert_eq!(tracked_malloc_headers_matching(&churn_headers), 0);
    assert!(malloc_user_ptr_tracked(live_malloc));
    let capture_after = unsafe { (*capture_slot & POINTER_MASK) as usize };
    assert_ne!(capture_after, live_child);
    assert!(crate::arena::pointer_in_nursery(capture_after));
}

#[test]
fn test_copied_minor_malloc_scaling_falls_back_when_registry_unavailable() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    let mut raw_root = live_malloc as u64;
    js_gc_register_global_root(&mut raw_root as *mut u64 as i64);
    deactivate_malloc_registry_for_tests();

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::MallocRegistryUnavailable,
        false,
    );
    assert_eq!(
        trace.copying_nursery.malloc_registry_rebuilds, 0,
        "copied-minor fallback must not rebuild the malloc registry"
    );
    assert!(malloc_user_ptr_tracked(live_malloc));
    assert_eq!(raw_root as usize, live_malloc as usize);
    assert!(
        !malloc_registry_active_for_tests(),
        "fallback mark-sweep should not activate the copied-minor malloc registry"
    );
}

#[test]
fn test_copying_minor_falls_back_for_pinned_young_root() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    unsafe {
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
    }
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungRoot,
        false,
    );
    assert_eq!(after, child);
    unsafe {
        (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_copying_minor_falls_back_for_pinned_young_dirty_slot() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(child);
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let child_after = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungDirtySlot,
        false,
    );
    assert_eq!(child_after, child);
    unsafe {
        (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_copying_minor_falls_back_for_transitive_pinned_young_child() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = crate::array::js_array_alloc(1);
    let child = young_leaf();
    let elements = unsafe {
        (*arr).length = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = ptr_bits(child);
        layout_note_slot(arr as usize, 0, *elements);
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
        elements
    };
    if gc_force_evacuate_enabled() {
        // This test is about copying-preflight fallback; forced
        // evacuation would otherwise move the parent after fallback.
        let arr_header = unsafe { header_from_user_ptr(arr as *const u8) };
        CONS_PINNED.with(|s| {
            s.borrow_mut().insert(arr_header as usize);
        });
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungTransitive,
        false,
    );
    assert_eq!(
        arr_after, arr as usize,
        "copying nursery must fall back before moving the young parent"
    );
    assert_eq!(
        child_after, child,
        "pinned transitive young child must keep its raw address"
    );
    unsafe {
        let child_header = header_from_user_ptr(child as *const u8);
        assert_eq!(
            (*child_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "pinned child must not receive a forwarding pointer"
        );
        (*child_header).gc_flags &= !GC_FLAG_PINNED;
    }
}

// Regression (2026-07 GC audit, old→malloc hole): minors sweep the malloc
// registry, old parents are black leaves, and the barrier used to remember
// only old→NURSERY children — a malloc-GC child (RegExp, hook-mode Promise,
// Symbol, large-capture closure) whose sole referrer was a clean old parent
// was freed while live on the next MallocCount/ArenaBytes minor.
#[test]
fn test_copying_minor_old_to_malloc_child_survives_sweep() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();

    let malloc_child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(malloc_child);
    }
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(malloc_child as usize);
    }
    js_write_barrier_slot(
        ptr_bits(old_arr as usize),
        elements as u64,
        ptr_bits(malloc_child as usize),
    );
    assert!(
        remembered_set_size() > 0,
        "old→malloc store must dirty the remembered page"
    );

    trigger_guard.make_malloc_sweep_due();
    let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });

    assert!(
        malloc_user_ptr_tracked(malloc_child),
        "malloc child referenced only by a clean old parent must survive \
         the minor malloc sweep via the remembered old→malloc edge"
    );
}

// Same hole, scenario B: the nursery GRANDCHILD behind an unmarked malloc
// parent needs no malloc sweep to die — the malloc parent was never marked
// in a minor, so its nursery child was unmarked and the nursery reset freed
// it on the very next minor.
#[test]
fn test_copying_minor_old_to_malloc_nursery_grandchild_survives() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();

    let grandchild = young_leaf();
    let malloc_child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>() + 8,
        GC_TYPE_CLOSURE,
    );
    let capture_slot =
        unsafe { init_test_closure_with_one_capture(malloc_child, ptr_bits(grandchild)) };
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(malloc_child as usize);
    }
    js_write_barrier_slot(
        ptr_bits(old_arr as usize),
        elements as u64,
        ptr_bits(malloc_child as usize),
    );

    trigger_guard.make_malloc_sweep_due();
    let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });

    assert!(
        malloc_user_ptr_tracked(malloc_child),
        "malloc parent must survive the minor via the remembered edge"
    );
    let grandchild_after = unsafe { (*capture_slot & POINTER_MASK) as usize };
    assert!(
        crate::arena::pointer_in_nursery(grandchild_after)
            || crate::arena::pointer_in_old_gen(grandchild_after),
        "nursery grandchild behind a malloc parent must be evacuated/kept, \
         not left dangling in reset from-space"
    );
}
