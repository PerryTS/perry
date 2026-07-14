use super::support::GcTriggerThresholdTestGuard;

#[test]
fn map_set_side_allocations_release_on_thread_exit() {
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

    assert_eq!(map_after.0 - map_before.0, 64);
    assert_eq!(map_after.1 - map_before.1, 4096);
    assert_eq!(set_after.0 - set_before.0, 64);
    assert_eq!(set_after.1 - set_before.1, 2048);
}

#[test]
fn map_set_side_allocations_release_exactly_once() {
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
    assert_eq!(
        (map_after.0 - map_before.0, map_after.1 - map_before.1),
        (2, 128)
    );
    assert_eq!(
        (set_after.0 - set_before.0, set_after.1 - set_before.1),
        (2, 64)
    );
}

#[test]
fn map_set_owner_records_follow_growth() {
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
    assert_eq!(
        (map_after.0 - map_before.0, map_after.1 - map_before.1),
        (1, 128)
    );
    assert_eq!(
        (set_after.0 - set_before.0, set_after.1 - set_before.1),
        (1, 64)
    );
}
