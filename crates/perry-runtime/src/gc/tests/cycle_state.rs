use super::super::*;
use super::barrier::assert_heap_child_marked;
use super::support::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

static SYNC_ONLY_SCANNER_CALLS: AtomicUsize = AtomicUsize::new(0);

fn sync_only_test_mutable_root_scanner(_visitor: &mut RuntimeRootVisitor<'_>) {
    SYNC_ONLY_SCANNER_CALLS.fetch_add(1, Ordering::Relaxed);
}

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
    let mut hist = std::collections::HashMap::new();
    for ph in &phases {
        *hist.entry(format!("{ph:?}")).or_insert(0usize) += 1;
    }
    panic!(
        "GC cycle did not complete within step limit; histogram: {hist:?}; tail: {:?}",
        &phases[phases.len().saturating_sub(12)..]
    );
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
        trigger.kind.progress_kind(GcCollectionKind::Minor),
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        EVACUATION_POLICY_DISABLED_REASON,
        old_page_selection,
        old_page_source_blocks,
    )
}

fn alloc_tracked_test_closure() -> *mut u8 {
    let child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(child);
    }
    child
}

fn alloc_tracked_test_object() -> *mut crate::object::ObjectHeader {
    let header_size = std::mem::size_of::<crate::object::ObjectHeader>();
    let fields_size = 8 * std::mem::size_of::<crate::JSValue>();
    let child =
        gc_malloc(header_size + fields_size, GC_TYPE_OBJECT) as *mut crate::object::ObjectHeader;
    unsafe {
        (*child).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*child).class_id = 0;
        (*child).parent_class_id = 0;
        (*child).field_count = 0;
        (*child).keys_array = std::ptr::null_mut();
        (*child).meta = std::ptr::null_mut();
        let fields_ptr = (child as *mut u8).add(header_size) as *mut crate::JSValue;
        for i in 0..8 {
            std::ptr::write(fields_ptr.add(i), crate::JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(child as *mut u8);
    }
    child
}

const VALID_POINTER_TEST_OBJECT_FIELDS: u32 = 1000;

fn alloc_large_nursery_objects(count: usize) -> Vec<usize> {
    (0..count)
        .map(|_| unsafe {
            let (object, _fields) = alloc_nursery_test_object(VALID_POINTER_TEST_OBJECT_FIELDS);
            object as usize
        })
        .collect()
}

#[test]
fn build_valid_pointer_set_slices_large_multi_block_arena_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let objects = alloc_large_nursery_objects(320);
    assert!(crate::arena::arena_block_count() > 1);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    let mut build_steps = 0usize;
    while state.phase() == GcCyclePhase::BuildValidPointerSet {
        let result = state.step(GcWorkBudget::bounded(1));
        assert_eq!(result.phase, GcCyclePhase::BuildValidPointerSet);
        build_steps += 1;
        assert!(
            build_steps < 100_000,
            "valid pointer set build did not finish"
        );
    }

    assert_eq!(state.phase(), GcCyclePhase::RootScan);
    assert!(
        build_steps > crate::arena::arena_block_count(),
        "arena setup, object walk, and finalization should span multiple build steps"
    );

    drop(objects);
    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");
}

#[test]
fn build_valid_pointer_set_first_tiny_step_only_inspects_one_arena_block() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _objects = alloc_large_nursery_objects(260);
    assert!(crate::arena::arena_block_count() > 1);

    let mut builder = ValidPointerSetBuilder::new();
    let initial = builder.snapshot_for_tests();
    assert_eq!(initial.phase, ValidPointerSetBuildPhase::ArenaCursorSetup);
    assert_eq!(initial.arena_setup_blocks, 0);

    assert!(!builder.step(1));
    let after = builder.snapshot_for_tests();
    assert_eq!(after.phase, ValidPointerSetBuildPhase::ArenaCursorSetup);
    assert_eq!(after.arena_setup_blocks, 1);
    assert_eq!(after.lookup_count, 0);

    let _ = builder.finish();
}

#[test]
fn build_valid_pointer_set_tiny_setup_step_does_not_bulk_order_blocks() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _objects = alloc_large_nursery_objects(320);
    let block_count = crate::arena::arena_block_count();
    assert!(block_count > 2);

    let mut builder = ValidPointerSetBuilder::new();
    for expected_blocks in 1..block_count {
        assert!(!builder.step(1));
        let snapshot = builder.snapshot_for_tests();
        assert_eq!(snapshot.phase, ValidPointerSetBuildPhase::ArenaCursorSetup);
        assert_eq!(snapshot.arena_setup_blocks, expected_blocks);
        assert_eq!(snapshot.lookup_count, 0);
    }

    let before_order_finish = builder.snapshot_for_tests();
    assert_eq!(
        before_order_finish.phase,
        ValidPointerSetBuildPhase::ArenaCursorSetup
    );
    assert_eq!(before_order_finish.arena_setup_blocks, block_count - 1);

    assert!(!builder.step(1));
    let after_order_finish = builder.snapshot_for_tests();
    assert_eq!(
        after_order_finish.phase,
        ValidPointerSetBuildPhase::ArenaWalk
    );
    assert_eq!(after_order_finish.lookup_count, 0);

    let _ = builder.finish();
}

#[test]
fn build_valid_pointer_set_tiny_arena_walk_step_adds_one_lookup_entry() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _objects = alloc_large_nursery_objects(64);

    let mut builder = ValidPointerSetBuilder::new();
    assert!(!builder.step(100_000));
    let after_setup = builder.snapshot_for_tests();
    assert_eq!(after_setup.phase, ValidPointerSetBuildPhase::ArenaWalk);
    assert_eq!(after_setup.lookup_count, 0);

    let mut previous_lookup_count = after_setup.lookup_count;
    for _ in 0..16 {
        assert!(!builder.step(1));
        let snapshot = builder.snapshot_for_tests();
        assert_eq!(snapshot.phase, ValidPointerSetBuildPhase::ArenaWalk);
        assert_eq!(
            snapshot.lookup_count,
            previous_lookup_count + 1,
            "one tiny arena-walk step must not rebuild or bulk-fill lookup entries"
        );
        previous_lookup_count = snapshot.lookup_count;
    }

    let _ = builder.finish();
}

#[test]
fn build_valid_pointer_set_sliced_build_preserves_contains_and_enclosing_object() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (arena_object, fields) = unsafe { alloc_nursery_test_object(4) };
    let arena_object = arena_object as usize;
    let interior = fields as usize;
    let arena_strings = (0..1100).map(|_| young_leaf()).collect::<Vec<_>>();
    let malloc_objects = (0..32)
        .map(|_| alloc_tracked_test_closure() as usize)
        .collect::<Vec<_>>();

    let mut builder = ValidPointerSetBuilder::new();
    let mut steps = 0usize;
    while !builder.step(7) {
        steps += 1;
        assert!(steps < 100_000, "sliced valid pointer build did not finish");
    }
    let valid_ptrs = builder.finish();

    assert!(valid_ptrs.contains(&arena_object));
    assert_eq!(valid_ptrs.enclosing_object(interior), Some(arena_object));
    for &ptr in arena_strings.iter().take(16) {
        assert!(valid_ptrs.contains(&ptr));
    }
    for &ptr in &malloc_objects {
        assert!(valid_ptrs.contains(&ptr));
    }
}

#[test]
fn build_valid_pointer_set_finalize_is_separate_bounded_phase() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _objects = alloc_large_nursery_objects(16);
    let _malloc_objects = (0..4)
        .map(|_| alloc_tracked_test_closure())
        .collect::<Vec<_>>();

    let mut builder = ValidPointerSetBuilder::new();
    assert!(!builder.step(10_000));
    assert_eq!(
        builder.snapshot_for_tests().phase,
        ValidPointerSetBuildPhase::ArenaWalk
    );

    assert!(!builder.step(1));
    let after_tiny_arena = builder.snapshot_for_tests();
    assert_eq!(after_tiny_arena.phase, ValidPointerSetBuildPhase::ArenaWalk);
    assert!(
        after_tiny_arena.lookup_count < _objects.len(),
        "one tiny arena-walk step must not insert the whole arena"
    );

    while builder.snapshot_for_tests().phase != ValidPointerSetBuildPhase::Finalize {
        assert!(!builder.step(10_000));
    }
    let before_finalize = builder.snapshot_for_tests();
    assert_eq!(before_finalize.phase, ValidPointerSetBuildPhase::Finalize);
    assert!(before_finalize.current_arena_run_len > 0 || before_finalize.arena_run_count > 0);

    assert!(!builder.step(0));
    assert_eq!(
        builder.snapshot_for_tests().phase,
        ValidPointerSetBuildPhase::Finalize
    );
    assert!(builder.step(1));
    assert_eq!(
        builder.snapshot_for_tests().phase,
        ValidPointerSetBuildPhase::Done
    );
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
fn root_scan_slices_many_mutable_roots_with_tiny_budget() {
    let roots = 32_u32;
    let _guard = CopyingNurseryTestGuard::new(roots);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let first_live_bytes = b"root_scan_sliced_live";
    let first_live = crate::string::js_string_from_bytes(
        first_live_bytes.as_ptr(),
        first_live_bytes.len() as u32,
    ) as usize;
    js_shadow_slot_set(0, string_bits(first_live));
    for slot in 1..roots {
        js_shadow_slot_set(slot, string_bits(young_leaf()));
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 10_000, "root scan did not finish");
    }
    assert!(
        root_steps > roots as usize,
        "bounded root scan should require multiple root_scan steps"
    );

    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");
    let traced_root_steps = trace
        .pause_steps
        .iter()
        .filter(|step| step.phase_before == GcCyclePhase::RootScan)
        .count();
    assert!(
        traced_root_steps >= root_steps,
        "trace should retain repeated root_scan pause steps"
    );
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, first_live_bytes);
    }
}

#[test]
fn root_scan_slices_many_registered_promise_roots_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_budgeted_mutable_root_scanner_with_source(
        promise_mutable_root_scanner,
        crate::promise::scan_promise_roots_mut_step,
        crate::promise::new_promise_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );

    const ROOTS: usize = 32;
    let children = (0..ROOTS).map(|_| young_leaf()).collect::<Vec<_>>();
    let values = children
        .iter()
        .map(|&child| f64::from_bits(string_bits(child)))
        .collect::<Vec<_>>();
    crate::promise::test_seed_many_promise_task_roots(&values);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 10_000, "root scan did not finish");
    }
    assert!(
        root_steps > ROOTS,
        "promise task roots should require multiple tiny root_scan steps"
    );
    for &child in &children {
        let header = unsafe { header_from_user_ptr(child as *const u8) };
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_MARKED,
                0,
                "promise task value should be marked by the sliced scanner"
            );
        }
    }
}

#[test]
fn root_scan_slices_many_registered_timer_roots_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_budgeted_mutable_root_scanner_with_source(
        timer_mutable_root_scanner,
        crate::timer::scan_timer_roots_mut_step,
        crate::timer::new_timer_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );

    const ROOTS: usize = 32;
    let children = (0..ROOTS).map(|_| young_leaf()).collect::<Vec<_>>();
    let values = children
        .iter()
        .map(|&child| f64::from_bits(string_bits(child)))
        .collect::<Vec<_>>();
    crate::timer::test_seed_many_timeout_roots(&values);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 10_000, "root scan did not finish");
    }
    assert!(
        root_steps > ROOTS,
        "timeout roots should require multiple tiny root_scan steps"
    );
    for &child in &children {
        let header = unsafe { header_from_user_ptr(child as *const u8) };
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_MARKED,
                0,
                "timer value should be marked by the sliced scanner"
            );
        }
    }
}

#[test]
fn root_scan_slices_many_registered_class_side_table_roots_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::object::test_clear_class_side_table_roots();
    gc_register_budgeted_mutable_root_scanner_with_source(
        crate::object::scan_class_side_table_roots_mut,
        crate::object::scan_class_side_table_roots_mut_step,
        crate::object::new_class_side_table_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );

    const ROOTS: usize = 32;
    let children = (0..ROOTS).map(|_| young_leaf()).collect::<Vec<_>>();
    for (idx, &child) in children.iter().enumerate() {
        crate::object::test_seed_class_dynamic_prop_root(
            0x5300 + idx as u32,
            "root",
            string_bits(child),
        );
    }
    let prototype_object = crate::object::js_object_alloc(0, 0) as usize;
    let parent_closure = alloc_tracked_test_closure() as usize;
    crate::object::test_seed_class_prototype_object_root(0x53f0, prototype_object);
    crate::object::test_seed_class_parent_closure_root(0x53f1, parent_closure);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 10_000, "root scan did not finish");
    }
    assert!(
        root_steps > ROOTS,
        "class side-table roots should require multiple tiny root_scan steps"
    );
    for &child in &children {
        let header = unsafe { header_from_user_ptr(child as *const u8) };
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_MARKED,
                0,
                "class side-table value should be marked by the sliced scanner"
            );
        }
    }
    assert_marked_user_ptr(
        prototype_object,
        "prototype-object side-table value in sliced scanner",
    );
    assert_marked_user_ptr(
        parent_closure,
        "parent-closure side-table value in sliced scanner",
    );
    crate::object::test_clear_class_side_table_roots();
}

#[test]
fn root_scan_slices_many_registered_tui_state_roots_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::tui::state::test_reset_state_slots();
    gc_register_budgeted_mutable_root_scanner_with_source(
        crate::tui::state::scan_state_slot_roots_mut,
        crate::tui::state::scan_state_slot_roots_mut_step,
        crate::tui::state::new_state_slot_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );

    const ROOTS: usize = 32;
    let children = (0..ROOTS).map(|_| young_leaf()).collect::<Vec<_>>();
    for &child in &children {
        crate::tui::state::js_perry_tui_state_alloc(f64::from_bits(string_bits(child)));
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::ArenaBytes));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 10_000, "root scan did not finish");
    }
    assert!(
        root_steps > ROOTS,
        "tui state roots should require multiple tiny root_scan steps"
    );
    for &child in &children {
        let header = unsafe { header_from_user_ptr(child as *const u8) };
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_MARKED,
                0,
                "tui state value should be marked by the sliced scanner"
            );
        }
    }

    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");
    assert!(
        trace
            .pause_steps
            .iter()
            .filter(|step| step.phase_before == GcCyclePhase::RootScan)
            .count()
            >= root_steps,
        "trace should report repeated root_scan pause steps"
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn normal_incremental_root_scan_runs_synchronous_only_scanner_inline() {
    // #6180 flip: with incremental as the default, unbudgeted mutable
    // scanners run synchronously inside the initial root-scan step instead
    // of pausing the cycle before them.
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    SYNC_ONLY_SCANNER_CALLS.store(0, Ordering::Relaxed);
    gc_register_mutable_root_scanner(sync_only_test_mutable_root_scanner);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::ArenaBytes));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan && steps < 500_000 {
        state.step(GcWorkBudget::bounded(1));
        steps += 1;
    }

    assert!(
        SYNC_ONLY_SCANNER_CALLS.load(Ordering::Relaxed) >= 1,
        "default-incremental root scan must run synchronous-only scanners inline"
    );
    incremental_mark_barrier_disable();
    clear_mark_seeds();
}

#[test]
fn root_scan_slices_remembered_set_dirty_slots_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    const SLOTS: usize = 48;
    let (old_obj, fields) = unsafe { alloc_old_test_object(SLOTS as u32) };
    let mut children = Vec::with_capacity(SLOTS);
    for slot in 0..SLOTS {
        let child = young_leaf();
        children.push(child);
        unsafe {
            runtime_store_jsvalue_slot(
                old_obj as usize,
                fields.add(slot) as usize,
                slot,
                string_bits(child),
            );
        }
    }
    assert!(remembered_set_size() > 0);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 10_000, "root scan did not finish");
    }
    assert!(
        root_steps > SLOTS,
        "dirty remembered slots should be scanned across multiple root_scan steps"
    );
    for &child in &children {
        let header = unsafe { header_from_user_ptr(child as *const u8) };
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_MARKED,
                0,
                "remembered-set root scan should mark every dirty young child"
            );
        }
    }

    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");
    assert!(
        trace.remembered_set.dirty_slots_scanned >= SLOTS,
        "remembered-set telemetry should include the sliced dirty slots"
    );
}

#[test]
fn root_scan_slices_remembered_set_dirty_old_pages_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    const OBJECTS: usize = 24;
    const FIELDS_PER_OBJECT: u32 = 512;
    let mut children = Vec::with_capacity(OBJECTS);
    for _ in 0..OBJECTS {
        let (old_obj, fields) = unsafe { alloc_old_test_object(FIELDS_PER_OBJECT) };
        let child = young_leaf();
        children.push(child);
        runtime_store_jsvalue_slot(old_obj as usize, fields as usize, 0, string_bits(child));
    }
    assert!(remembered_set_size() > 0);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);

    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 100_000, "root scan did not finish");
    }
    assert!(
        root_steps > OBJECTS,
        "dirty old-page header discovery should require multiple tiny root_scan steps"
    );
    for &child in &children {
        let header = unsafe { header_from_user_ptr(child as *const u8) };
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_MARKED,
                0,
                "remembered-set old-page scan should mark every dirty young child"
            );
        }
    }
}

/// Regression: a born-black (allocate-black) object must also be TRACED.
/// The budgeted BuildValidPointerSet phase runs mutator windows BEFORE the
/// insertion barrier enables; an object born there is MARKED at birth, so
/// the later trace treats it as already-visited. Pre-fix its children —
/// linked before barrier-enable, hence never shaded — were unreachable to
/// the whole mark and swept live (the compiled TUI's React fiber tree,
/// reachable only through a build-window fiber's `alternate` back-edge,
/// lost its side-table fields this way). `gc_note_black_birth` seeds every
/// black birth so the trace descends into it.
#[test]
fn born_black_build_phase_object_is_traced() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    // Budgeted kind: production build-phase mutator windows only exist in
    // budgeted cycles, whose valid-pointer set is the live page classifier
    // (mid-cycle births classify as valid). A census-mode cycle with mutator
    // windows would be an artificial hybrid production never runs.
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    // One bounded step: parked inside BuildValidPointerSet, barrier not yet
    // enabled, allocate-black active since cycle construction.
    state.step(GcWorkBudget::bounded(1));
    assert_eq!(state.phase(), GcCyclePhase::BuildValidPointerSet);

    // Mutator window: a runtime-path allocation is born black...
    let (parent, fields) = unsafe { alloc_nursery_test_object(1) };
    unsafe {
        let header = header_from_user_ptr(parent as *const u8);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "runtime allocation during an active budgeted cycle is born marked"
        );
    }
    // ...and links a white child with the barrier still off. The child is
    // reachable ONLY through the born-black parent; the parent is not
    // rooted anywhere (its birth mark alone keeps it alive this cycle).
    let (child, _child_fields) = unsafe { alloc_nursery_test_object(1) };
    unsafe {
        let ch = header_from_user_ptr(child as *const u8);
        (*ch).gc_flags &= !GC_FLAG_MARKED;
    }
    crate::object::test_seed_overflow_fields_root(child as usize, 7f64.to_bits());
    unsafe {
        runtime_store_jsvalue_slot(
            parent as usize,
            fields as usize,
            0,
            ptr_bits(child as usize),
        );
    }

    // Age the parent/child block out of the block-persistence window
    // (BLOCK_PERSIST_WINDOW = 5 recent general blocks get their objects
    // force-marked as register-holding candidates — that pass would
    // otherwise resurrect the child and mask the missing trace).
    let aged_from = crate::arena::general_block_count();
    let mut filler_blocks = 0usize;
    while filler_blocks < 7 {
        for _ in 0..64 {
            let _ = unsafe { crate::arena::arena_alloc_gc(4096, 8, GC_TYPE_STRING) };
        }
        filler_blocks = crate::arena::general_block_count().saturating_sub(aged_from);
    }

    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");

    assert!(
        crate::object::debug_overflow_entry_len(child as usize).is_some(),
        "child reachable only through a born-black object was swept live: \
         black births must be seeded into the trace"
    );
    crate::object::test_clear_overflow_fields_root();

    // The filler blocks above were born black (and seeded) inside the active
    // cycle, so they survived it as floating garbage. Reclaim them so later
    // tests' bounded cycles don't walk seven extra blocks of dead strings.
    let mut cleanup = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_in_single_unit_steps(&mut cleanup);
    let _ = cleanup.take_outcome();
}

/// Regression: an object born in a mutator window BETWEEN AtomicFinalize
/// completion and the first sweep slice ("the finalize->sweep gap"), then
/// linked from a live parent, must survive the sweep. Pre-fix the barrier
/// was disabled at finalize-end while the sweep's block/fill snapshot was
/// only taken one or more mutator windows later: a gap-born object (codegen
/// bump allocations carry no allocate-black birth flag) was WHITE yet inside
/// the snapshot and freed live — observed as React fibers losing their
/// overflow side-table fields in the compiled TUI. The fix keeps the barrier
/// active across the gap and disables it in the same slice that takes the
/// snapshot.
#[test]
fn gap_born_child_stored_between_finalize_and_sweep_survives() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    js_shadow_slot_set(0, ptr_bits(parent as usize));

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    // Stop exactly in the gap: AtomicFinalize is done (marks final), but the
    // first step_sweep slice (which builds the sweep state's block snapshot)
    // has not run.
    run_cycle_until_phase(&mut state, GcCyclePhase::Sweep);

    // Mutator window in the gap: birth a young object and link it from the
    // live parent. Codegen's inline bump allocator does not stamp the
    // allocate-black birth flag, so emulate a codegen birth by clearing the
    // runtime-path birth mark before the store.
    let (child, _child_fields) = unsafe { alloc_nursery_test_object(1) };
    unsafe {
        let ch = header_from_user_ptr(child as *const u8);
        (*ch).gc_flags &= !GC_FLAG_MARKED;
    }
    // Give the child a side-table entry: its survival is then observable the
    // same way the production failure was (a swept child has its
    // OVERFLOW_FIELDS entry cleared by the dead-payload sweep arm).
    crate::object::test_seed_overflow_fields_root(child as usize, 42f64.to_bits());
    unsafe {
        runtime_store_jsvalue_slot(
            parent as usize,
            fields as usize,
            0,
            ptr_bits(child as usize),
        );
    }

    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");

    assert!(
        crate::object::debug_overflow_entry_len(child as usize).is_some(),
        "gap-born child was swept live: its overflow side-table entry was cleared"
    );
    unsafe {
        let obj = child as *mut crate::object::ObjectHeader;
        assert_eq!(
            (*obj).object_type,
            1,
            "gap-born child payload clobbered after sweep"
        );
    }
    crate::object::test_clear_overflow_fields_root();
}

/// Regression (#6495): the trace must visit EVERY overflow slot of a live
/// object, not the subset its layout mask claims. The per-object slot mask
/// is maintained by `layout_note_slot` at store time, but not every
/// overflow write path notes (GC owner moves merge entries via
/// `merge_overflow_fields` with no notes) — a stale SIDE_MASK then hides
/// pointer-bearing slots from the trace, and their referents are swept
/// while referenced. Observed at bundle scale as masks capped at bit 48
/// with live NaN-boxed pointers sitting at slots 49..63.
#[test]
fn overflow_slots_beyond_layout_mask_are_traced() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (owner, _fields) = unsafe { alloc_nursery_test_object(1) };
    js_shadow_slot_set(0, ptr_bits(owner as usize));

    // Build a usable SIDE_MASK layout claiming slots 0..=48 as the complete
    // pointer set. A pointer note only creates a side mask from the
    // POINTER_FREE state (notes on an UNKNOWN object leave it UNKNOWN =
    // conservative full visit), so first rebuild the layout from the
    // object's single non-pointer inline slot.
    unsafe {
        let zero: u64 = 0;
        crate::gc::layout_rebuild_from_slots(owner as *mut u8, &zero as *const u64, 1);
    }
    let dummy = young_leaf();
    for i in 0..49 {
        crate::gc::layout_note_slot(owner as usize, i, string_bits(dummy));
    }
    // The bug path: an overflow write that never runs `layout_note_slot`.
    // Slot 50 holds the ONLY reference to a live string; the mask does not
    // know about it.
    let child = young_leaf();
    let mut values = vec![crate::value::TAG_UNDEFINED; 51];
    values[50] = string_bits(child);
    crate::object::test_seed_overflow_fields_vec(owner as usize, values);

    // Age the owner/child block out of the block-persistence window (that
    // pass would otherwise force-mark the child as a register-holding
    // candidate and mask the missing trace).
    let aged_from = crate::arena::general_block_count();
    let mut filler_blocks = 0usize;
    while filler_blocks < 7 {
        for _ in 0..64 {
            let _ = unsafe { crate::arena::arena_alloc_gc(4096, 8, GC_TYPE_STRING) };
        }
        filler_blocks = crate::arena::general_block_count().saturating_sub(aged_from);
    }

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::Sweep);
    unsafe {
        let header = header_from_user_ptr(child as *const u8);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "a pointer in an overflow slot beyond the layout mask was not \
             traced: its referent is about to be swept live"
        );
    }
    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");
    crate::object::test_clear_overflow_fields_root();

    // Reclaim the filler blocks so later bounded-step tests aren't slowed.
    let mut cleanup = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_in_single_unit_steps(&mut cleanup);
    let _ = cleanup.take_outcome();
}

#[test]
fn full_atomic_finalize_slices_barrier_seed_drain_with_tiny_budget() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    const SEEDS: usize = 16;
    let (parent, fields) = unsafe { alloc_old_test_object(SEEDS as u32) };
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    let children = (0..SEEDS).map(|_| young_leaf()).collect::<Vec<_>>();

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::AtomicFinalize);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep incremental barriers active until atomic finalize finishes"
    );

    for (slot, &child) in children.iter().enumerate() {
        unsafe {
            runtime_store_jsvalue_slot(
                parent as usize,
                fields.add(slot) as usize,
                slot,
                string_bits(child),
            );
        }
    }

    let mut atomic_steps = 0usize;
    while state.phase() == GcCyclePhase::AtomicFinalize {
        state.step(GcWorkBudget::bounded(1));
        atomic_steps += 1;
        assert!(atomic_steps < 100_000, "atomic finalize did not finish");
    }
    assert!(
        atomic_steps > SEEDS,
        "barrier seed drain and remembered rebuild should keep tiny steps in atomic_finalize"
    );
    // The barrier must survive the AtomicFinalize->Sweep boundary: the sweep
    // state's per-block fill snapshot is only taken on the first step_sweep
    // slice, and mutator windows in between would otherwise birth WHITE
    // objects that sit inside the snapshot and get freed live (the compiled
    // TUI's lost-fiber-field bug). The first sweep slice drains the gap's
    // shaded seeds, disables the barrier, and takes the snapshot atomically.
    assert!(
        incremental_mark_barrier_active(),
        "barrier must stay active across the finalize->sweep gap"
    );
    state.step(GcWorkBudget::bounded(1));
    assert!(
        !incremental_mark_barrier_active(),
        "first sweep slice must disable the barrier when it takes the block snapshot"
    );

    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");
    let traced_atomic_steps = trace
        .pause_steps
        .iter()
        .filter(|step| step.phase_before == GcCyclePhase::AtomicFinalize)
        .count();
    assert!(
        traced_atomic_steps >= atomic_steps,
        "trace should retain repeated atomic_finalize pause steps"
    );
    for (slot, &child) in children.iter().enumerate() {
        unsafe {
            assert_eq!(*fields.add(slot), string_bits(child));
        }
    }
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
fn budgeted_minor_fallback_ignores_forced_evacuation_and_stays_non_moving() {
    let _defrag = OldDefragTestEnable::new();
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = EnvVarGuard::set("PERRY_GC_FORCE_EVACUATE", "1");

    let _old_block_filler =
        crate::arena::arena_alloc_gc_old(2 * 1024 * 1024 - GC_HEADER_SIZE, 8, GC_TYPE_STRING);
    let (old_parent, _) = unsafe { alloc_old_test_object(0) };
    let old_parent_header = unsafe { header_from_user_ptr(old_parent as *const u8) };
    let old_parent_total = unsafe { (*old_parent_header).size as usize };
    let mut old_parent_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in
        crate::arena::old_object_page_overlaps(old_parent_header as usize, old_parent_total)
    {
        old_parent_pages.insert(page);
    }
    let _dead_old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING);
    unsafe {
        (*old_parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    let _ = sweep_with_age_bump(false);
    let selected_before = select_old_page_defrag_pages(true);
    assert!(
        old_parent_pages
            .iter()
            .any(|page| selected_before.pages.contains(page)),
        "test must seed an old-page defrag candidate"
    );

    js_shadow_slot_set(0, ptr_bits(old_parent as usize));
    let (nursery_candidate, _) = unsafe { alloc_nursery_test_object(0) };
    let nursery_candidate_user = nursery_candidate as usize;
    let nursery_candidate_header = unsafe { header_from_user_ptr(nursery_candidate as *const u8) };
    unsafe {
        (*nursery_candidate_header).gc_flags |= GC_FLAG_TENURED;
    }
    js_shadow_slot_set(1, ptr_bits(nursery_candidate_user));

    let mut state = test_start_budgeted_minor_fallback_state_with_trace(
        GcTriggerKind::ArenaBytes,
        GcProgressKind::NormalIncremental,
    );
    run_cycle_in_single_unit_steps(&mut state);
    let outcome = state.take_outcome().expect("cycle should complete");
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_eq!(
        js_shadow_slot_get(1) & POINTER_MASK,
        nursery_candidate_user as u64,
        "budgeted low-pause minor GC must not move a forced nursery candidate"
    );
    unsafe {
        assert_eq!(
            (*nursery_candidate_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "budgeted low-pause minor GC must not leave a forwarding stub"
        );
    }
    assert_eq!(trace.progress_kind, GcProgressKind::NormalIncremental);
    assert!(!trace.evacuation_policy.allowed);
    assert!(!trace.evacuation_policy.force);
    assert!(!trace.evacuation_policy.considered);
    assert!(!trace.evacuation_policy.enabled);
    assert_eq!(
        trace.evacuation_policy.reason,
        EVACUATION_POLICY_LOW_PAUSE_NON_MOVING_REASON
    );
    assert_eq!(
        trace.evacuation_policy.snapshot.old_page_selected_pages, 0,
        "budgeted low-pause startup must skip old-page defrag selection"
    );
    assert_eq!(trace.evacuation.moved_objects, 0);
    assert_eq!(trace.evacuation.moved_bytes, 0);
    assert_eq!(trace.evacuation.old_page_moved_objects, 0);
    assert_eq!(trace.evacuation.old_page_moved_bytes, 0);
    assert_eq!(trace.phase_us.get("evacuation").copied(), Some(0));
    assert_eq!(trace.phase_us.get("reference_rewrite").copied(), Some(0));
    assert_eq!(
        js_shadow_slot_get(0) & POINTER_MASK,
        old_parent as u64,
        "old root should remain valid without evacuation"
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
fn full_cycle_class_static_field_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = alloc_tracked_test_closure();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    let name = b"lateStatic";
    unsafe {
        crate::object::js_class_register_static_field(
            0x5101,
            name.as_ptr(),
            name.len(),
            f64::from_bits(ptr_bits(child as usize)),
        );
    }
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "static class field stored after root scan should survive via the side-table root barrier"
    );
}

#[test]
fn full_cycle_symbol_property_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::symbol::test_clear_symbol_side_table_roots();

    let owner = alloc_tracked_test_object();
    let sym = alloc_tracked_test_symbol();
    let child = alloc_tracked_test_closure();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    unsafe {
        crate::symbol::js_object_set_symbol_property(
            f64::from_bits(ptr_bits(owner as usize)),
            f64::from_bits(ptr_bits(sym as usize)),
            f64::from_bits(ptr_bits(child as usize)),
        );
    }
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(sym as *mut u8),
        "symbol property key stored after root scan should survive via the side-table root barrier"
    );
    assert!(
        malloc_user_ptr_tracked(child),
        "symbol property value stored after root scan should survive via the side-table root barrier"
    );
    crate::symbol::test_clear_symbol_side_table_roots();
}

#[test]
fn full_cycle_class_static_symbol_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::symbol::test_clear_symbol_side_table_roots();

    let sym = alloc_tracked_test_symbol();
    let child = alloc_tracked_test_closure();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    unsafe {
        crate::symbol::js_class_register_static_symbol(
            0x5106,
            f64::from_bits(ptr_bits(sym as usize)),
            f64::from_bits(ptr_bits(child as usize)),
        );
    }
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(sym as *mut u8),
        "class static symbol key stored after root scan should survive via the side-table root barrier"
    );
    assert!(
        malloc_user_ptr_tracked(child),
        "class static symbol value stored after root scan should survive via the side-table root barrier"
    );
    crate::symbol::test_clear_symbol_side_table_roots();
}

#[test]
fn full_cycle_class_ref_dynamic_prop_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = alloc_tracked_test_closure();
    let key = crate::string::js_string_from_bytes(b"lateDynamic".as_ptr(), 11);
    let class_ref_bits = 0x7FFE_0000_0000_0000u64 | 0x5102;
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    crate::object::js_object_set_field_by_name(
        class_ref_bits as *mut crate::object::ObjectHeader,
        key,
        f64::from_bits(ptr_bits(child as usize)),
    );
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "dynamic class-ref property stored after root scan should survive via the side-table root barrier"
    );
}

#[test]
fn full_cycle_prototype_method_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = alloc_tracked_test_closure();
    let name = b"lateMethod";
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    unsafe {
        crate::object::js_register_prototype_method(
            0x5103,
            name.as_ptr(),
            name.len(),
            f64::from_bits(ptr_bits(child as usize)),
        );
    }
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "prototype method stored after root scan should survive via the side-table root barrier"
    );
}

#[test]
fn full_cycle_prototype_object_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = alloc_tracked_test_object();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    let _created = crate::object::js_object_create(f64::from_bits(ptr_bits(child as usize)));
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child as *mut u8),
        "prototype object stored after root scan should survive via the side-table root barrier"
    );
}

#[test]
fn full_cycle_parent_closure_store_after_root_scan_preserves_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = alloc_tracked_test_closure();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    crate::object::js_register_class_parent_dynamic(
        0x5105,
        f64::from_bits(ptr_bits(child as usize)),
    );
    run_cycle_in_single_unit_steps(&mut state);

    assert!(
        malloc_user_ptr_tracked(child),
        "parent closure stored after root scan should survive via the side-table root barrier"
    );
}

#[test]
fn full_cycle_bound_prototype_method_cache_after_root_scan_marks_new_value() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "full cycle should keep root barriers active after root scan"
    );

    let value = crate::object::class_prototype_method_value_for_name(0x5104, "lateBound");
    let value_bits = value.to_bits();
    assert_eq!(value_bits & TAG_MASK, POINTER_TAG);
    let value_ptr = (value_bits & POINTER_MASK) as usize;
    let value_header = unsafe { header_from_user_ptr(value_ptr as *const u8) };
    unsafe {
        assert_ne!(
            (*value_header).gc_flags & GC_FLAG_MARKED,
            0,
            "bound prototype-method cache creation after root scan should fire the root barrier"
        );
    }

    run_cycle_in_single_unit_steps(&mut state);
    assert_eq!(
        crate::object::test_class_prototype_method_value_root_bits(0x5104, "lateBound"),
        value_bits
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

// Regression (2026-07 GC audit, sync/step scanner divergence): cycle-based
// collections run ONLY the budgeted STEP scanner when one is registered, and
// the promise step machine lacked the PROMISE_OVERFLOW_REACTIONS phase its
// sync twin visits — the 2nd+ `.then()` on a still-pending promise parks its
// reaction closure and chained `next` promise ONLY in that table, so every
// full/fallback collection swept them while the promise was pending.
#[test]
fn full_cycle_step_scanner_covers_promise_overflow_reactions() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_budgeted_mutable_root_scanner_with_source(
        promise_mutable_root_scanner,
        crate::promise::scan_promise_roots_mut_step,
        crate::promise::new_promise_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );

    let promise = unsafe { alloc_old_test_promise() };
    js_shadow_slot_set(0, ptr_bits(promise as usize));
    let cb1 = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 0);
    let cb2 = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 0);
    // First reaction lands inline in the promise's own fields; the second
    // goes to PROMISE_OVERFLOW_REACTIONS — the table under test.
    let _next1 = crate::promise::js_promise_then(promise, cb1, std::ptr::null());
    let next2 = crate::promise::js_promise_then(promise, cb2, std::ptr::null());

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::RootScan);
    let mut root_steps = 0usize;
    while state.phase() == GcCyclePhase::RootScan {
        state.step(GcWorkBudget::bounded(1));
        root_steps += 1;
        assert!(root_steps < 100_000, "root scan did not finish");
    }

    // Marked during RootScan by the step scanner itself (not via promise
    // field propagation — cb2/next2 are reachable ONLY through the table).
    assert_heap_child_marked(cb2 as *const u8, "overflow on_fulfilled closure");
    assert_heap_child_marked(next2 as *const u8, "overflow next promise");

    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");
}
