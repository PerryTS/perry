use super::*;

#[test]
fn test_copying_minor_relocates_managed_set() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (child_obj, _child_fields) = unsafe { alloc_nursery_test_object(0) };
    let child = child_obj as usize;
    let child_bits = ptr_bits(child);
    let set = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(set, i as f64);
    }
    crate::set::js_set_add(set, f64::from_bits(child_bits));
    assert!(crate::set::is_registered_set(set as usize));
    assert!(crate::set::test_set_index_contains(set, 8.0));
    assert!(crate::set::test_set_index_contains(
        set,
        f64::from_bits(child_bits)
    ));
    let side_allocation_before = crate::set::test_set_side_allocation(set as usize)
        .expect("managed Set should own its external elements buffer");

    let text = crate::string::js_string_from_bytes(b"a moving string".as_ptr(), 15);
    crate::set::js_set_add_string(set, text);
    let index_before = unsafe { crate::set::test_index_snapshot(set) };
    assert_ne!(index_before.0, 0, "the index must exist before evacuation");
    js_shadow_slot_set(0, ptr_bits(set as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let set_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    assert_eq!(
        unsafe { crate::set::test_index_snapshot(set_after) },
        index_before,
        "moving a Set and its keys must preserve the index without hashing"
    );
    let rewritten_bits = crate::set::js_set_value_at(set_after, 9).to_bits();
    let rewritten = (rewritten_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(set_after as usize, set as usize);
    assert!(!crate::set::is_registered_set(set as usize));
    assert!(crate::set::is_registered_set(set_after as usize));
    assert_eq!(crate::set::test_set_side_allocation(set as usize), None);
    // The young store moves with its Set; the header's alias follows it.
    let side_allocation_after = crate::set::test_set_side_allocation(set_after as usize)
        .expect("a moved Set keeps its store");
    assert_eq!(side_allocation_after.1, side_allocation_before.1);
    assert_eq!(
        unsafe { (*set_after).elements } as usize,
        side_allocation_after.0
    );
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(crate::set::js_set_has(set_after, 8.0), 1);
    assert_eq!(
        crate::set::js_set_has(set_after, f64::from_bits(child_bits)),
        0
    );
    assert_eq!(
        crate::set::js_set_has(set_after, f64::from_bits(rewritten_bits)),
        1
    );
    assert!(crate::set::test_set_index_contains(
        set_after,
        f64::from_bits(rewritten_bits)
    ));

    let text_after = crate::set::js_set_value_at(set_after, 10);
    assert_ne!(text_after.to_bits() & POINTER_MASK, text as u64);
    assert_eq!(crate::set::js_set_has(set_after, text_after), 1);

    let release_before = crate::set::test_thread_set_side_deallocation_snapshot();
    unsafe {
        crate::set::finalize_set_side_allocation_for_gc(set_after);
    }
    let release_after = crate::set::test_thread_set_side_deallocation_snapshot();
    assert_eq!(
        (
            release_after.0 - release_before.0,
            release_after.1 - release_before.1
        ),
        (1, 128)
    );
}

#[test]
fn set_key_identity_is_weak_and_dead_index_is_reclaimed() {
    let _scan = ConservativeScanDisabledGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (child, _) = unsafe { alloc_nursery_test_object(0) };
    let set = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(set, i as f64);
    }
    crate::set::js_set_add(set, f64::from_bits(ptr_bits(child as usize)));
    assert!(unsafe { crate::set::test_key_identity(child as usize) }.is_some());
    assert_ne!(unsafe { crate::set::test_index_snapshot(set).0 }, 0);
    let release_before = crate::set::test_thread_set_side_deallocation_snapshot();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    js_gc_collect();
    assert_eq!(crate::set::test_set_side_allocation(set as usize), None);
    assert_eq!(
        crate::set::test_thread_set_side_deallocation_snapshot().0,
        release_before.0 + 1
    );
}

#[test]
fn indexed_array_keys_survive_repeated_evacuation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = crate::set::js_set_alloc(16);
    for _ in 0..12 {
        let array = crate::array::js_array_alloc(4);
        crate::set::js_set_add(set, f64::from_bits(ptr_bits(array as usize)));
    }
    js_shadow_slot_set(0, ptr_bits(set as usize));
    for _ in 0..2 {
        let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
        let before = unsafe { crate::set::test_index_snapshot(set) };
        let first_key = crate::set::js_set_value_at(set, 0).to_bits();
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
        assert_eq!(unsafe { crate::set::test_index_snapshot(set) }, before);
        assert_ne!(crate::set::js_set_value_at(set, 0).to_bits(), first_key);
        for i in 0..12 {
            let value = crate::set::js_set_value_at(set, i);
            assert_eq!(crate::set::js_set_has(set, value), 1);
        }
        for i in 0..12 {
            let key = crate::set::js_set_value_at(set, i);
            assert!(unsafe {
                crate::set::test_key_identity((key.to_bits() & POINTER_MASK) as usize)
            }
            .is_some());
        }
    }
}

fn old_array_key() -> f64 {
    let arr = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::array::ArrayHeader>(),
        std::mem::align_of::<crate::array::ArrayHeader>(),
        GC_TYPE_ARRAY,
    ) as *mut crate::array::ArrayHeader;
    unsafe {
        (*arr).length = 0;
        (*arr).capacity = 0;
    }
    f64::from_bits(ptr_bits(arr as usize))
}

/// #11169 follow-up: `has`/`delete` probes that MISS must not allocate a key
/// identity. Before the fix every probed object left a permanent entry.
#[test]
fn identity_probes_that_miss_do_not_allocate_metadata() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = crate::set::js_set_alloc(16);
    for _ in 0..12 {
        let array = crate::array::js_array_alloc(4);
        crate::set::js_set_add(set, f64::from_bits(ptr_bits(array as usize)));
    }
    assert_ne!(unsafe { crate::set::test_index_snapshot(set).0 }, 0);
    js_shadow_slot_set(0, ptr_bits(set as usize));
    for _ in 0..64 {
        let stranger = f64::from_bits(ptr_bits(crate::array::js_array_alloc(4) as usize));
        assert_eq!(crate::set::js_set_has(set, stranger), 0);
        assert_eq!(crate::set::js_set_delete(set, stranger), 0);
        unsafe {
            assert!(crate::array::array_property_bag(
                (stranger.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader
            )
            .is_null());
        }
    }
    // Members are still found through their identities after they move.
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    for i in 0..12 {
        let value = crate::set::js_set_value_at(set, i);
        assert!(crate::arena::pointer_in_nursery(
            (value.to_bits() & POINTER_MASK) as usize
        ));
        assert_eq!(crate::set::js_set_has(set, value), 1);
    }
    let stranger = f64::from_bits(ptr_bits(crate::array::js_array_alloc(4) as usize));
    assert_eq!(crate::set::js_set_has(set, stranger), 0);
    // A member deleted after the move is found by its (rekeyed) identity.
    let first = crate::set::js_set_value_at(set, 0);
    assert_eq!(crate::set::js_set_delete(set, first), 1);
    assert_eq!(crate::set::js_set_has(set, first), 0);
    assert_eq!(crate::set::js_set_size(set), 11);
    // Live indices: the eleven survivors are now 0..11.
    for i in 0..11 {
        let value = crate::set::js_set_value_at(set, i);
        assert_eq!(crate::set::js_set_has(set, value), 1);
    }
}

/// Old keys keep their owner-local identities through minor collection.
#[test]
fn old_keys_keep_their_private_identity_through_minor_collection() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = crate::set::js_set_alloc(16);
    let keys: Vec<f64> = (0..12).map(|_| old_array_key()).collect();
    for &key in &keys {
        crate::set::js_set_add(set, key);
    }
    js_shadow_slot_set(0, ptr_bits(set as usize));
    // Something young and rooted so the minor has real work.
    js_shadow_slot_set(1, string_bits(young_leaf()));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    for &key in &keys {
        assert_eq!(crate::set::js_set_has(set, key), 1);
    }
}

/// A survivor keeps its identity through repeated moves while a dead Set
/// and its key need no metadata registry cleanup.
#[test]
fn young_keys_follow_their_owner_local_metadata() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(set, i as f64);
    }
    let survivor = crate::array::js_array_alloc(4) as usize;
    crate::set::js_set_add(set, f64::from_bits(ptr_bits(survivor)));
    js_shadow_slot_set(0, ptr_bits(set as usize));
    // A second, unrooted Set whose only key dies with it.
    let doomed = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(doomed, i as f64);
    }
    crate::set::js_set_add(
        doomed,
        f64::from_bits(ptr_bits(crate::array::js_array_alloc(4) as usize)),
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    // A second minor still finds (and moves) the survivor via the log.
    let _ = collect_minor_trace(GcTriggerKind::Direct);
    let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    let key = crate::set::js_set_value_at(set, 9);
    assert_ne!(key.to_bits() & POINTER_MASK, survivor as u64);
    assert_eq!(crate::set::js_set_has(set, key), 1);
}

#[test]
fn set_key_identity_moves_with_the_key_metadata() {
    check_owned_key_identity(false);
}

#[test]
#[ignore = "negative control: erase the moved Set key identity"]
fn negative_control_objtables_set_key_identity() {
    check_owned_key_identity(true);
}

fn check_owned_key_identity(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(set, i as f64);
    }
    let owner = crate::object::js_object_alloc(0, 0);
    crate::set::js_set_add(set, f64::from_bits(ptr_bits(owner as usize)));
    let identity = unsafe { crate::set::test_key_identity(owner as usize) }.unwrap();
    js_shadow_slot_set(0, ptr_bits(set as usize));
    let _ = collect_minor_trace(GcTriggerKind::Direct);
    let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    let key = crate::set::js_set_value_at(set, 9);
    let moved = (key.to_bits() & POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    if erase {
        unsafe {
            crate::set::test_erase_key_identity(moved);
        }
    }
    assert_eq!(
        unsafe { crate::set::test_key_identity(moved) },
        Some(identity)
    );
    assert_eq!(crate::set::js_set_has(set, key), 1);
    let fresh = crate::object::js_object_alloc(0, 0) as usize;
    assert!(unsafe { crate::set::test_key_identity(fresh) }.is_none());
}
