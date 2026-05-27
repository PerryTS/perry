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
