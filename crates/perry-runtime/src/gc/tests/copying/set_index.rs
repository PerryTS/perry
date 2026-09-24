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
fn identity_probes_that_miss_do_not_grow_the_identity_table() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::set::scan_identity_roots_mut);
    let set = crate::set::js_set_alloc(16);
    for _ in 0..12 {
        let array = crate::array::js_array_alloc(4);
        crate::set::js_set_add(set, f64::from_bits(ptr_bits(array as usize)));
    }
    assert_ne!(unsafe { crate::set::test_index_snapshot(set).0 }, 0);
    js_shadow_slot_set(0, ptr_bits(set as usize));
    let members = crate::set::test_identity_count();
    assert_eq!(members, 12);
    for _ in 0..64 {
        let stranger = f64::from_bits(ptr_bits(crate::array::js_array_alloc(4) as usize));
        assert_eq!(crate::set::js_set_has(set, stranger), 0);
        assert_eq!(crate::set::js_set_delete(set, stranger), 0);
    }
    assert_eq!(
        crate::set::test_identity_count(),
        members,
        "a missed probe must not allocate a key identity"
    );

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
    assert_eq!(crate::set::test_identity_count(), members);
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

/// #11169 follow-up: a minor must not walk identity entries whose keys are
/// old — the table's young half confines both the rekey scan and the
/// dead-key prune.
#[test]
fn old_identity_keys_are_skipped_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::set::scan_identity_roots_mut);
    let set = crate::set::js_set_alloc(16);
    let keys: Vec<f64> = (0..12).map(|_| old_array_key()).collect();
    for &key in &keys {
        crate::set::js_set_add(set, key);
    }
    assert_eq!(crate::set::test_identity_halves(), (0, 12));
    js_shadow_slot_set(0, ptr_bits(set as usize));
    // Something young and rooted so the minor has real work.
    js_shadow_slot_set(1, string_bits(young_leaf()));

    let prune_visits_before = crate::set::test_identity_prune_visits();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let row = crate::gc::young_log::last_walk("set.key_identities")
        .expect("the identity scanner must have run");
    assert!(row.partial, "a copying minor takes the young walk: {row:?}");
    assert_eq!(row.table_len, 12, "{row:?}");
    assert_eq!(
        row.visited, 0,
        "a minor must not visit identity entries with old keys: {row:?}"
    );
    assert_eq!(
        crate::set::test_identity_prune_visits(),
        prune_visits_before,
        "a minor's dead-key prune must not visit identity entries with old keys"
    );
    let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    for &key in &keys {
        assert_eq!(crate::set::js_set_has(set, key), 1);
    }
    assert_eq!(crate::set::test_identity_count(), 12);
}

/// A young key that dies is pruned by the young-only prune, and a survivor
/// that moves stays in the young half at its new address.
#[test]
fn young_identity_half_prunes_dead_keys_and_follows_moved_ones() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::set::scan_identity_roots_mut);
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
    assert_eq!(crate::set::test_identity_count(), 2);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        crate::set::test_identity_count(),
        1,
        "the dead key's identity must be pruned by the minor"
    );
    let row = crate::gc::young_log::last_walk("set.key_identities").expect("scanner ran");
    assert!(row.partial && row.kept >= 1, "{row:?}");
    assert_eq!(crate::set::test_identity_halves(), (1, 0));
    // A second minor still finds (and moves) the survivor via the log.
    let _ = collect_minor_trace(GcTriggerKind::Direct);
    let set = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    let key = crate::set::js_set_value_at(set, 9);
    assert_ne!(key.to_bits() & POINTER_MASK, survivor as u64);
    assert_eq!(crate::set::js_set_has(set, key), 1);
    assert_eq!(crate::set::test_identity_count(), 1);
}
