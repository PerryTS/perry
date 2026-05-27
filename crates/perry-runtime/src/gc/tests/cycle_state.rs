use super::super::*;
use super::support::*;
use std::time::Instant;

fn trace_snapshot(kind: GcTriggerKind) -> GcTriggerSnapshot {
    GcTriggerSnapshot {
        kind,
        steps_before: Some(GcStepSnapshot::current()),
    }
}

fn run_cycle_in_single_unit_steps(state: &mut GcCycleState) -> Vec<GcCyclePhase> {
    let mut phases = Vec::new();
    for _ in 0..100_000 {
        if state.phase() == GcCyclePhase::Complete {
            return phases;
        }
        let result = state.step(GcWorkBudget::bounded(1));
        phases.push(result.phase);
    }
    panic!("GC cycle did not complete within step limit");
}

fn run_cycle_until_phase(state: &mut GcCycleState, target: GcCyclePhase) {
    for _ in 0..100_000 {
        if state.phase() == target {
            return;
        }
        state.step(GcWorkBudget::bounded(1));
    }
    panic!("GC cycle did not reach {target:?} within step limit");
}

fn start_minor_fallback_state(trigger: GcTriggerSnapshot) -> GcCycleState {
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    clear_mark_seeds();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let evacuation_policy_allowed = gen_gc_evacuate_enabled();
    let force_evacuation = gc_force_evacuate_enabled();
    let old_page_selection = if evacuation_policy_allowed && old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    let old_page_source_blocks =
        crate::arena::old_arena_source_blocks_for_pages(&old_page_selection.pages);

    GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        old_page_selection,
        old_page_source_blocks,
    )
}

#[test]
fn full_cycle_state_steps_through_resumable_phases() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    for _ in 0..8 {
        let _ = young_leaf();
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    let phases = run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");

    for phase in [
        GcCyclePhase::BuildValidPointerSet,
        GcCyclePhase::RootScan,
        GcCyclePhase::MarkPropagation,
        GcCyclePhase::BlockPersistence,
        GcCyclePhase::AtomicFinalize,
        GcCyclePhase::Sweep,
        GcCyclePhase::Reclaim,
    ] {
        assert!(phases.contains(&phase), "missing phase {phase:?}");
    }
    assert_eq!(state.phase(), GcCyclePhase::Complete);
    assert!(trace.phase_us.contains_key("reclaim"));
}

#[test]
fn bounded_full_cycle_preserves_roots_and_reclaims_unreachable_objects() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

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

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");

    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should remain tracked"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&dead_malloc_headers),
        0,
        "unreachable malloc churn should be swept"
    );
    assert!(
        outcome.freed_bytes >= dead_old_size,
        "full sweep should count the unreachable old-arena object"
    );
}

#[test]
fn bounded_minor_fallback_preserves_age_and_trace_fields() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));

    let mut state = start_minor_fallback_state(trace_snapshot(GcTriggerKind::Direct));
    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let header = unsafe { header_from_user_ptr(live_after as *const u8) };
    let flags = unsafe { (*header).gc_flags };

    assert_eq!(live_after, live, "fallback minor should not copy the root");
    assert!(
        flags & (GC_FLAG_HAS_SURVIVED | GC_FLAG_TENURED) != 0,
        "fallback minor should apply survival metadata"
    );
    assert_eq!(trace.collection_kind.as_str(), "minor");
    assert!(trace.phase_us.contains_key("reclaim"));
    assert_eq!(
        trace.copying_nursery.fallback_reason,
        CopiedMinorFallbackReason::NotAttempted
    );
}

#[test]
fn full_cycle_drains_incremental_barrier_seed_before_sweep() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    let child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(child);
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert_eq!(
        state.phase(),
        GcCyclePhase::BlockPersistence,
        "test must store after ordinary mark propagation has drained"
    );
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep incremental barriers active until atomic finalize"
    );

    runtime_store_jsvalue_slot(
        parent as usize,
        fields as usize,
        0,
        ptr_bits(child as usize),
    );
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "child stored after mark propagation should survive via atomic barrier-seed drain"
    );
    assert!(
        !incremental_mark_barrier_active(),
        "full cycle should disable incremental barriers before completion"
    );
}

#[test]
fn full_cycle_box_root_set_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let box_ptr = crate::r#box::js_box_alloc(0.0);
    assert!(!box_ptr.is_null());
    let child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(child);
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    crate::r#box::js_box_set(box_ptr, f64::from_bits(ptr_bits(child as usize)));
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "child stored into a box root after root scan should survive via js_box_set's root barrier"
    );
}

#[test]
fn full_cycle_global_root_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let mut root_slot = 0_u64;
    js_gc_register_global_root(&mut root_slot as *mut u64 as i64);
    let child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(child);
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    root_slot = ptr_bits(child as usize);
    js_write_barrier_root_nanbox(root_slot);
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "child stored into a registered global root after root scan should survive via root barrier"
    );
}

#[test]
fn full_cycle_exception_root_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(exception_mutable_root_scanner);
    crate::exception::js_clear_exception();

    let child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(child);
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    crate::exception::test_set_exception(f64::from_bits(ptr_bits(child as usize)));
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "child stored into the exception root after root scan should survive via root barrier"
    );
    crate::exception::js_clear_exception();
}

#[test]
fn full_cycle_console_singleton_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::builtins::scan_console_log_singleton_roots_mut);
    crate::builtins::test_set_console_log_singleton(0);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    let console_log_value = crate::builtins::js_console_log_as_closure();
    let console_log_bits = console_log_value.to_bits();
    assert_eq!(console_log_bits & TAG_MASK, POINTER_TAG);
    let console_log_ptr = (console_log_bits & POINTER_MASK) as usize;
    assert_eq!(
        crate::builtins::test_console_log_singleton(),
        console_log_ptr as i64
    );
    let console_log_header = unsafe { header_from_user_ptr(console_log_ptr as *const u8) };
    unsafe {
        assert_ne!(
            (*console_log_header).gc_flags & GC_FLAG_MARKED,
            0,
            "first-use console.log singleton CAS after root scan should fire the root barrier"
        );
    }

    let replacement = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(replacement);
    }
    crate::builtins::test_set_console_log_singleton(replacement as i64);

    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(replacement),
        "console singleton test store after root scan should survive via the root barrier"
    );
    assert_eq!(
        crate::builtins::test_console_log_singleton(),
        replacement as i64
    );
    crate::builtins::test_set_console_log_singleton(0);
}
