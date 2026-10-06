use super::super::allocation_pacing::{burst_boundary, due, finish_full, test_debt};
use super::super::*;
use super::support::*;

const MIB: usize = 1024 * 1024;

#[test]
fn nursery_garbage_cannot_reset_mature_reclaim_backoff() {
    let _isolation = GcTestIsolationGuard::new();
    let _moving = policy::force_moving_gc_pacing();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let frame = js_shadow_frame_push(1);
    let keep = crate::buffer::js_buffer_alloc((17 * MIB) as i32, 37);
    js_shadow_slot_set(0, crate::value::JSValue::pointer(keep.cast()).bits());
    finish_full(17 * MIB, 4 * MIB);
    let freed_before = GC_STATS.with(|stats| stats.borrow().total_freed_bytes);
    let nursery_window = policy::GcSuppressScope::new();
    for _ in 0..(64 * MIB / (GC_HEADER_SIZE + 128)) {
        let young = crate::arena::arena_alloc_gc(128, 8, GC_TYPE_BUFFER);
        assert!(crate::arena::pointer_in_nursery(young as usize));
        unsafe {
            std::ptr::write_bytes(young, 0, 128);
        }
    }
    drop(nursery_window);
    assert!(!policy::gc_budgeted_cycle_active());
    {
        let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    }
    assert!(
        GC_STATS.with(|stats| stats.borrow().total_freed_bytes) - freed_before > (56 * MIB) as u64
    );
    for _ in 0..3 {
        crate::buffer::js_buffer_alloc((4 * MIB) as i32, 13);
    }
    assert!(
        !due(crate::arena::old_gen_in_use_bytes(), true),
        "nursery frees must not reset backoff for an unchanged mature live set"
    );
    let keep = crate::value::JSValue::from_bits(js_shadow_slot_get(0))
        .as_pointer::<crate::buffer::BufferHeader>();
    {
        assert_eq!(crate::buffer::js_buffer_get(keep, 0), 37);
    }
    js_shadow_frame_pop(frame);
}

#[test]
fn an_owed_boundary_full_does_not_start_a_redundant_budgeted_minor() {
    let _isolation = GcTestIsolationGuard::new();
    let _moving = policy::force_moving_gc_pacing();
    let thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let frame = js_shadow_frame_push(2);
    js_shadow_slot_set(0, ptr_bits(young_leaf()));
    let keep = crate::buffer::js_buffer_alloc(MIB as i32, 37);
    js_shadow_slot_set(1, crate::value::JSValue::pointer(keep.cast()).bits());
    for _ in 0..5 {
        crate::buffer::js_buffer_alloc((4 * MIB) as i32, 13);
    }
    thresholds.make_arena_trigger_due();
    let before = GC_STATS.with(|stats| stats.borrow().collection_count);
    let old_before = crate::arena::old_gen_in_use_bytes();
    crate::promise::js_promise_run_microtasks_event_loop();
    assert_eq!(
        GC_STATS.with(|stats| stats.borrow().collection_count) - before,
        1,
        "an owed full must not first pay for a nursery-only trace"
    );
    assert!(!policy::gc_budgeted_cycle_active());
    assert!(crate::arena::old_gen_in_use_bytes() < old_before - 16 * MIB);
    let keep = crate::value::JSValue::from_bits(js_shadow_slot_get(1))
        .as_pointer::<crate::buffer::BufferHeader>();
    {
        assert_eq!(crate::buffer::js_buffer_get(keep, 0), 37);
    }
    assert_eq!(test_debt(), (0, 0));
    js_shadow_frame_pop(frame);
}

#[test]
fn a_parked_budgeted_minor_cannot_block_a_dead_large_buffer_burst() {
    let _isolation = GcTestIsolationGuard::new();
    let _moving = policy::force_moving_gc_pacing();
    let thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let frame = js_shadow_frame_push(2);
    js_shadow_slot_set(0, ptr_bits(young_leaf()));
    let keep = crate::buffer::js_buffer_alloc(MIB as i32, 37);
    js_shadow_slot_set(1, crate::value::JSValue::pointer(keep.cast()).bits());
    for _ in 0..5 {
        crate::buffer::js_buffer_alloc((4 * MIB) as i32, 13);
    }
    thresholds.make_arena_trigger_due();
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(result.collection_kind, GcCollectionKind::Minor.ffi_code());
    let old_before = crate::arena::old_gen_in_use_bytes();
    let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
    burst_boundary();
    assert!(
        !policy::gc_budgeted_cycle_active(),
        "a parking heap must finish the cycle that blocks its owed reclaim"
    );
    assert!(gc_safepoint_moving_minor());
    assert!(crate::arena::old_gen_in_use_bytes() < old_before - 16 * MIB);
    let keep = crate::value::JSValue::from_bits(js_shadow_slot_get(1))
        .as_pointer::<crate::buffer::BufferHeader>();
    {
        assert_eq!(crate::buffer::js_buffer_get(keep, 0), 37);
    }
    assert_eq!(test_debt(), (0, 0));
    js_shadow_frame_pop(frame);
}

#[test]
fn a_budgeted_full_pays_only_the_burst_present_at_its_boundary_start() {
    let _isolation = GcTestIsolationGuard::new();
    let _moving = policy::force_moving_gc_pacing();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    for _ in 0..5 {
        crate::buffer::js_buffer_alloc((4 * MIB) as i32, 1);
    }
    {
        let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
        policy::GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
        let mut result = JsGcStepResult::default();
        assert_eq!(
            js_gc_step_work_units(1, &mut result),
            JS_GC_STEP_STATUS_ACTIVE
        );
        assert_eq!(result.collection_kind, GcCollectionKind::Full.ffi_code());
    }
    // These births are marked live by the active full, even though they have
    // no roots by its completion. Its earlier snapshot cannot pay this burst.
    for _ in 0..5 {
        crate::buffer::js_buffer_alloc((4 * MIB) as i32, 2);
    }
    {
        let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
        assert_eq!(
            complete_budgeted_gc_cycle().status,
            JS_GC_STEP_STATUS_COMPLETED
        );
    }
    assert_eq!(test_debt(), (0, 0));
    let old_before = crate::arena::old_gen_in_use_bytes();
    assert!(
        old_before >= 20 * MIB,
        "allocate-black births must survive this cycle"
    );
    assert!(
        due(old_before, true),
        "post-snapshot births still need an opportunity"
    );
    let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
    burst_boundary();
    assert!(gc_safepoint_moving_minor());
    assert!(crate::arena::old_gen_in_use_bytes() < old_before - 16 * MIB);
    assert!(
        !due(8 * MIB, true),
        "the later trace pays the remaining burst"
    );
}

#[test]
fn mutator_windows_in_a_budgeted_minor_still_count_allocation() {
    let _isolation = GcTestIsolationGuard::new();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let before = test_debt().0;
    // Model the cycle-wide reentrancy flag while no collector step runs.
    let previous =
        policy::GC_FLAGS.with(|flags| flags.replace(flags.get() | policy::GC_FLAG_IN_ALLOC));
    crate::buffer::js_buffer_alloc(MIB as i32, 1);
    let after = test_debt().0;
    policy::GC_FLAGS.with(|flags| flags.set(previous));
    assert!(
        after >= before + MIB,
        "mutator work between steps is pressure"
    );
}

#[test]
fn large_buffer_burst_reclaims_at_a_precise_boundary_and_keeps_roots() {
    let _isolation = GcTestIsolationGuard::new();
    let _moving = policy::force_moving_gc_pacing();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let frame = js_shadow_frame_push(1);
    let keep = crate::buffer::js_buffer_alloc(MIB as i32, 37);
    js_shadow_slot_set(0, crate::value::JSValue::pointer(keep as *mut u8).bits());
    for _ in 0..5 {
        crate::buffer::js_buffer_alloc(4 * MIB as i32, 13);
    }
    let debt = test_debt();
    assert!(debt.0 >= 21 * MIB && debt.1 >= 21 * MIB);
    assert!(
        !due(21 * MIB, false),
        "the burst has not filled the allocation band"
    );
    assert!(
        due(21 * MIB, true),
        "a completed large burst must get a full opportunity"
    );
    let old_before = crate::arena::old_gen_in_use_bytes();
    let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
    burst_boundary();
    assert!(gc_safepoint_moving_minor());
    assert!(crate::arena::old_gen_in_use_bytes() < old_before - 16 * MIB);
    let keep = crate::value::JSValue::from_bits(js_shadow_slot_get(0))
        .as_pointer::<crate::buffer::BufferHeader>();
    unsafe {
        assert_eq!((*keep).length, MIB as u32);
        assert_eq!(crate::buffer::js_buffer_get(keep, 0), 37);
    }
    assert_eq!(test_debt(), (0, 0), "only the completed full pays the debt");
    let collections = gc_total_collection_count();
    burst_boundary();
    gc_safepoint_moving_minor();
    assert_eq!(
        gc_total_collection_count(),
        collections,
        "no per-message collection"
    );
    js_shadow_frame_pop(frame);
}

#[test]
fn minors_preserve_allocation_debt_and_collector_copies_do_not_add_to_it() {
    let _isolation = GcTestIsolationGuard::new();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    crate::buffer::js_buffer_alloc(2 * MIB as i32, 1);
    let before = test_debt();
    gc_collect_minor();
    assert_eq!(test_debt(), before);
    let _guard = allocation_pacing::CollectorStepGuard::enter();
    crate::buffer::js_buffer_alloc(MIB as i32, 2);
    assert_eq!(test_debt(), before, "relocation is not mutator allocation");
}

#[test]
fn malloc_growth_and_external_releases_keep_byte_pressure() {
    let _isolation = GcTestIsolationGuard::new();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let p = gc_malloc(MIB, GC_TYPE_BUFFER);
    let allocated = test_debt().0;
    let p = gc_realloc(p, 3 * MIB);
    assert_eq!(test_debt().0, allocated + 2 * MIB);
    gc_realloc(p, MIB);
    assert_eq!(
        test_debt().0,
        allocated + 2 * MIB,
        "shrinking must not pay full debt"
    );
    policy::gc_note_external_side_alloc(5 * MIB);
    let before_free = test_debt();
    policy::gc_note_external_side_free(5 * MIB);
    assert_eq!(
        test_debt(),
        before_free,
        "explicit external release must not hide churn"
    );
}

#[test]
fn process_pressure_combines_heaps_and_thread_exit_removes_their_debt() {
    let _isolation = GcTestIsolationGuard::new();
    finish_full(0, 4 * MIB);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (exit_tx, exit_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        allocation_pacing::note_allocation(128 * MIB, false);
        ready_tx.send(()).unwrap();
        exit_rx.recv().unwrap();
    });
    ready_rx.recv().unwrap();
    allocation_pacing::note_allocation(12 * MIB, false);
    assert!(
        !due(8 * MIB, true),
        "shared pressure must not turn nursery-only churn into a full"
    );
    allocation_pacing::note_allocation(4 * MIB, true);
    assert!(due(8 * MIB, true), "another heap's pressure must count");
    exit_tx.send(()).unwrap();
    worker.join().unwrap();
    assert!(
        !due(8 * MIB, true),
        "retired heaps must not leave process pressure behind"
    );
}

#[test]
fn generated_bumps_and_runtime_bumps_are_counted_once() {
    let _isolation = GcTestIsolationGuard::new();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let before = test_debt().0;
    crate::arena::arena_alloc_gc(64, 8, GC_TYPE_BUFFER);
    assert_eq!(test_debt().0, before + GC_HEADER_SIZE + 64);
    unsafe {
        let state = &mut *crate::arena::js_inline_arena_state();
        let raw = state.data.add(state.offset);
        std::ptr::write_bytes(raw, 0, 32);
        std::ptr::write(
            raw as *mut GcHeader,
            GcHeader {
                obj_type: GC_TYPE_BUFFER,
                gc_flags: GC_FLAG_ARENA,
                _reserved: 0,
                size: 32,
            },
        );
        state.offset += 32;
        crate::arena::record_arena_object_start(raw as usize, GC_TYPE_BUFFER);
    }
    let after = test_debt();
    assert_eq!(after.0, before + GC_HEADER_SIZE + 64 + 32);
    assert_eq!(
        test_debt(),
        after,
        "a second offset sync must not double count"
    );
    assert_eq!(
        after.1, 0,
        "a synchronized run of small bumps is not a large buffer"
    );
}

#[test]
fn old_hole_reuse_counts_bytes_even_when_the_bump_pointer_stands_still() {
    let _isolation = GcTestIsolationGuard::new();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    crate::buffer::js_buffer_alloc((MIB / 2) as i32, 3);
    let keep = crate::arena::arena_alloc_gc_old_born_tenured(8, 8, GC_TYPE_BUFFER);
    unsafe {
        std::ptr::write_bytes(keep, 0, 8);
    }
    let frame = js_shadow_frame_push(1);
    js_shadow_slot_set(0, crate::value::JSValue::pointer(keep).bits());
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let high_water = crate::arena::old_gen_in_use_bytes();
    let holes = old_free_bytes();
    assert!(holes >= MIB / 2, "the full must create a reusable hole");
    let before = test_debt().0;
    crate::buffer::js_buffer_alloc((MIB / 2) as i32, 4);
    assert_eq!(crate::arena::old_gen_in_use_bytes(), high_water);
    assert!(old_free_bytes() < holes);
    assert!(test_debt().0 >= before + MIB / 2);
    js_shadow_frame_pop(frame);
}

#[test]
fn a_mid_task_full_does_not_erase_the_later_dead_buffer_opportunity() {
    let _isolation = GcTestIsolationGuard::new();
    let _moving = policy::force_moving_gc_pacing();
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    finish_full(0, 4 * MIB);
    let frame = js_shadow_frame_push(5);
    for slot in 0..5 {
        let buffer = crate::buffer::js_buffer_alloc((4 * MIB) as i32, slot as i32);
        js_shadow_slot_set(slot, crate::value::JSValue::pointer(buffer.cast()).bits());
    }
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(test_debt(), (0, 0));
    for slot in 0..5 {
        js_shadow_slot_set(slot, 0);
    }
    let old_before = crate::arena::old_gen_in_use_bytes();
    assert!(due(old_before, true), "a mid-task trace found these buffers live; their later boundary still needs an opportunity");
    let _boundary = allocation_pacing::BurstBoundaryGuard::enter();
    burst_boundary();
    assert!(gc_safepoint_moving_minor());
    assert!(crate::arena::old_gen_in_use_bytes() < old_before - 16 * MIB);
    assert!(
        !due(8 * MIB, true),
        "one completed boundary reclaim pays the burst"
    );
    js_shadow_frame_pop(frame);
}
