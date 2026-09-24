use super::*;

#[test]
fn test_copying_minor_relocates_managed_set() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::set::scan_identity_roots_mut);
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
    assert_eq!(
        crate::set::test_set_side_allocation(set_after as usize),
        Some(side_allocation_before)
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

    let release_before = crate::set::test_set_side_deallocation_snapshot();
    unsafe {
        crate::set::finalize_set_side_allocation_for_gc(set_after);
    }
    let release_after = crate::set::test_set_side_deallocation_snapshot();
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
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::set::scan_identity_roots_mut);
    let (child, _) = unsafe { alloc_nursery_test_object(0) };
    let set = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(set, i as f64);
    }
    crate::set::js_set_add(set, f64::from_bits(ptr_bits(child as usize)));
    assert_eq!(crate::set::test_identity_count(), 1);
    assert_ne!(unsafe { crate::set::test_index_snapshot(set).0 }, 0);
    let release_before = crate::set::test_set_side_deallocation_snapshot();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        crate::set::test_identity_count(),
        0,
        "identity tokens must not root dead keys"
    );
    assert_eq!(crate::set::test_set_side_allocation(set as usize), None);
    assert_eq!(
        crate::set::test_set_side_deallocation_snapshot().0,
        release_before.0 + 1
    );
}

#[test]
fn indexed_array_keys_survive_repeated_evacuation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::set::scan_identity_roots_mut);
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
        assert_eq!(crate::set::test_identity_count(), 12);
    }
}
