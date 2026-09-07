use super::*;

#[test]
fn test_copied_minor_verify_evacuation_releases_malloc_registry_before_validation() {
    std::thread::spawn(|| {
        let _guard = CopyingNurseryTestGuard::new(2);
        let _env_guard = VerifyEvacuationTestGuard::on();
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

        let malloc_child = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            GC_TYPE_CLOSURE,
        );
        let malloc_parent = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>() + 8,
            GC_TYPE_CLOSURE,
        );
        unsafe {
            init_test_closure(malloc_child);
            init_test_closure_with_one_capture(malloc_parent, ptr_bits(malloc_child as usize));
        }
        js_shadow_slot_set(0, ptr_bits(malloc_parent as usize));
        let young = young_leaf();
        js_shadow_slot_set(1, ptr_bits(young));

        // Make the exact-validation call do real registry work. The verifier's
        // malloc-parent walk must snapshot the headers and release its borrow
        // before this child lookup reaches `ensure_set_built`. Sabotage: put
        // the verifier loop back inside `MALLOC_STATE.with(...borrow())`; the
        // lookup's `borrow_mut()` then panics this worker thread.
        deactivate_malloc_registry_for_tests();
        assert!(
            MALLOC_STATE.with(|state| !state.borrow().objects.is_empty()),
            "the malloc verifier fixture must populate the side table"
        );
        assert!(
            !malloc_registry_active_for_tests(),
            "the exact-validation lookup must have a registry to rebuild"
        );
        let rebuilds_before = MALLOC_REGISTRY_REBUILD_COUNT.with(|count| count.get());
        let stats = verify_old_to_young_edges_collect();
        let rebuilds_after = MALLOC_REGISTRY_REBUILD_COUNT.with(|count| count.get());
        assert!(
            stats.checked_old_objects > 0 && stats.checked_old_to_young_edges > 0,
            "the verifier must inspect the malloc parent and its malloc child"
        );
        assert_eq!(
            rebuilds_after,
            rebuilds_before + 1,
            "exact child validation must rebuild the non-empty malloc registry"
        );

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert!(
            trace.copying_nursery.copied_objects > 0,
            "the worker-thread collection must copy a live nursery object"
        );
        assert!(
            trace.phase_us.contains_key("evacuation_verify"),
            "the copied minor must run evacuation verification"
        );
        assert!(
            trace.phase_us.contains_key("old_young_edge_verify"),
            "the copied minor must run the malloc-parent verifier"
        );
        assert_ne!((js_shadow_slot_get(1) & POINTER_MASK) as usize, young);
        assert!(malloc_user_ptr_tracked(malloc_parent));
        assert!(malloc_user_ptr_tracked(malloc_child));
    })
    .join()
    .expect("worker-thread copying minor must complete without a RefCell borrow panic");
}
