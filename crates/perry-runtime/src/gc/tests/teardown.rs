use super::support::GcTriggerThresholdTestGuard;

/// The Map/Set side-deallocation counters are PROCESS-global, and every test
/// in this module measures deltas of them across a spawn/join window — run
/// concurrently (default-parallel `cargo test`, sharpest under a filter that
/// leaves few other tests), the probe threads' releases land inside each
/// other's windows and the exact-delta asserts read the sum (#6965).
/// Serialize the module. Poison-tolerant so one failure doesn't cascade
/// `PoisonError`s into the siblings.
fn teardown_counter_lock() -> std::sync::MutexGuard<'static, ()> {
    static TEARDOWN_COUNTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TEARDOWN_COUNTER_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn map_set_side_allocations_release_on_thread_exit() {
    let _counters = teardown_counter_lock();
    // #7056: drives the BUDGETED stepper via `complete_budgeted_gc_cycle`,
    // which the shipped default bypasses (scavenge defers alloc-point
    // collections to a precise safepoint). Pin legacy pacing so the cycle
    // actually starts and this keeps testing what it was written for.
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let map_before = crate::map::test_map_side_deallocation_snapshot();
    let set_before = crate::set::test_set_side_deallocation_snapshot();

    std::thread::spawn(|| {
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        for i in 0..64 {
            let map = crate::map::js_map_alloc(4);
            crate::map::js_map_set(map, i as f64, (i * 2) as f64);
            let set = crate::set::js_set_alloc(4);
            crate::set::js_set_add(set, i as f64);
        }
    })
    .join()
    .expect("Map/Set teardown probe thread should not panic");

    let map_after = crate::map::test_map_side_deallocation_snapshot();
    let set_after = crate::set::test_set_side_deallocation_snapshot();

    // The deallocation counters are PROCESS-global: under default-parallel
    // `cargo test`, any other test thread exiting (or deallocating Maps/Sets)
    // inside the spawn/join window bumps them too, so an exact-delta assert
    // is order-dependent (#6965). Lower bounds still prove the property under
    // test — the probe thread's exit released ITS 64 Maps/Sets — while an
    // under-release regression still lands below them in the serial CI run.
    assert!(map_after.0 - map_before.0 >= 64);
    assert!(map_after.1 - map_before.1 >= 4096);
    assert!(set_after.0 - set_before.0 >= 64);
    assert!(set_after.1 - set_before.1 >= 2048);
}

#[test]
fn map_set_side_allocations_release_exactly_once() {
    let _counters = teardown_counter_lock();
    let map_before = crate::map::test_map_side_deallocation_snapshot();
    let set_before = crate::set::test_set_side_deallocation_snapshot();

    std::thread::spawn(|| {
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let live_bytes_before = crate::gc::policy::external_side_live_bytes();
        let finalized_map = crate::map::js_map_alloc(4);
        let finalized_set = crate::set::js_set_alloc(4);

        unsafe {
            crate::map::finalize_map_side_allocation_for_gc(finalized_map);
            crate::set::finalize_set_side_allocation_for_gc(finalized_set);
            crate::map::finalize_map_side_allocation_for_gc(finalized_map);
            crate::set::finalize_set_side_allocation_for_gc(finalized_set);
        }

        let _drained_map = crate::map::js_map_alloc(4);
        let _drained_set = crate::set::js_set_alloc(4);
        let map_drain_before = crate::map::test_map_side_deallocation_snapshot();
        let set_drain_before = crate::set::test_set_side_deallocation_snapshot();
        crate::gc::js_gc_release_current_thread_collection_side_allocations();
        let map_drain_after = crate::map::test_map_side_deallocation_snapshot();
        let set_drain_after = crate::set::test_set_side_deallocation_snapshot();
        assert_eq!(
            (
                map_drain_after.0 - map_drain_before.0,
                map_drain_after.1 - map_drain_before.1
            ),
            (1, 64)
        );
        assert_eq!(
            (
                set_drain_after.0 - set_drain_before.0,
                set_drain_after.1 - set_drain_before.1
            ),
            (1, 32)
        );
        crate::gc::js_gc_release_current_thread_collection_side_allocations();
        assert_eq!(
            crate::map::test_map_side_deallocation_snapshot(),
            map_drain_after
        );
        assert_eq!(
            crate::set::test_set_side_deallocation_snapshot(),
            set_drain_after
        );
        assert_eq!(
            crate::gc::policy::external_side_live_bytes(),
            live_bytes_before
        );
    })
    .join()
    .expect("Map/Set exactly-once probe thread should not panic");

    let map_after = crate::map::test_map_side_deallocation_snapshot();
    let set_after = crate::set::test_set_side_deallocation_snapshot();
    // Cross-thread window: lower bounds, same rationale as
    // map_set_side_allocations_release_on_thread_exit (#6965). The
    // exactly-once core property is the exact-delta pair asserted INSIDE the
    // probe thread above.
    assert!(map_after.0 - map_before.0 >= 2);
    assert!(map_after.1 - map_before.1 >= 128);
    assert!(set_after.0 - set_before.0 >= 2);
    assert!(set_after.1 - set_before.1 >= 64);
}

#[test]
fn map_set_owner_records_follow_growth() {
    let _counters = teardown_counter_lock();
    let map_before = crate::map::test_map_side_deallocation_snapshot();
    let set_before = crate::set::test_set_side_deallocation_snapshot();

    std::thread::spawn(|| {
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let map = crate::map::js_map_alloc(4);
        let set = crate::set::js_set_alloc(4);
        for i in 0..5 {
            crate::map::js_map_set(map, i as f64, (i * 2) as f64);
            crate::set::js_set_add(set, i as f64);
        }

        unsafe {
            assert_eq!(
                crate::map::test_map_side_allocation(map as usize),
                Some(((*map).entries as usize, (*map).capacity as usize))
            );
            assert_eq!(
                crate::set::test_set_side_allocation(set as usize),
                Some(((*set).elements as usize, (*set).capacity as usize))
            );
            assert_eq!((*map).capacity, 8);
            assert_eq!((*set).capacity, 8);

            crate::map::finalize_map_side_allocation_for_gc(map);
            crate::set::finalize_set_side_allocation_for_gc(set);
        }
    })
    .join()
    .expect("Map/Set growth ownership probe thread should not panic");

    let map_after = crate::map::test_map_side_deallocation_snapshot();
    let set_after = crate::set::test_set_side_deallocation_snapshot();
    // Cross-thread window: lower bounds, same rationale as
    // map_set_side_allocations_release_on_thread_exit (#6965). The growth
    // ownership itself is asserted exactly INSIDE the probe thread above; the
    // grown (128/64-byte) release still has to land for these to hold.
    assert!(map_after.0 - map_before.0 >= 1);
    assert!(map_after.1 - map_before.1 >= 128);
    assert!(set_after.0 - set_before.0 >= 1);
    assert!(set_after.1 - set_before.1 >= 64);
}

/// #7539: a lazy JSON array's tape is a side allocation too, so a thread that
/// exits still holding one must hand its bytes back. Runs entirely inside the
/// probe thread — `json_tape_store`'s registry and byte counter are
/// thread-local, so unlike the Map/Set counters above there is no cross-thread
/// window to widen the assertions for.
#[test]
fn lazy_tape_side_allocations_release_on_thread_exit() {
    std::thread::spawn(|| {
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        assert_eq!(crate::json_tape_store::registered_bytes(), 0);

        let input = b"[1,2,3,4,5,6,7,8]";
        let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let tape = crate::json_tape::build_tape(input).expect("valid JSON");
        let len = crate::json_tape::count_array_length(&tape.entries, 0);
        // Left live and unmaterialized: only teardown can free this.
        let _lazy = unsafe { crate::json_tape::alloc_lazy_array(&tape.entries, 0, len, text) };
        assert!(
            crate::json_tape_store::registered_bytes() > 0,
            "test premise: the thread exits owning tape bytes"
        );

        crate::gc::js_gc_release_current_thread_collection_side_allocations();
        assert_eq!(
            crate::json_tape_store::registered_bytes(),
            0,
            "thread teardown must release live tapes"
        );
        // Idempotent, like the Map/Set drains above.
        crate::gc::js_gc_release_current_thread_collection_side_allocations();
        assert_eq!(crate::json_tape_store::registered_bytes(), 0);
    })
    .join()
    .expect("lazy tape teardown probe thread should not panic");
}
