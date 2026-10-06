//! #10057: indexed operations must preserve weak identity and moving-GC safety.
use super::*;
use crate::weakref::{
    js_weakmap_delete, js_weakmap_get, js_weakmap_has, js_weakmap_new, js_weakmap_set,
    js_weakset_add, js_weakset_new,
};

/// The copying-nursery guard takes the thread's scanner registry away. A weak
/// collection is branded through the shape interner. Register its mutable
/// metadata scanners so shape keys/prototypes keep current addresses.
fn guard(slot_count: u32) -> CopyingNurseryTestGuard {
    let guard = CopyingNurseryTestGuard::new(slot_count);
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    guard
}

fn rooted(slot: u32) -> f64 {
    f64::from_bits(js_shadow_slot_get(slot))
}

fn root_object(slot: u32) {
    let object = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(slot, ptr_bits(object as usize));
}

fn root_map() {
    let map = js_weakmap_new();
    js_shadow_slot_set(0, ptr_bits(map as usize));
}

fn entry_count() -> usize {
    let map = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
    crate::weakref::weak_collection_entries(map).len()
}

#[test]
fn weakmap_index_identity_overwrite_delete_and_reuse() {
    let _guard = guard(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    // Distinct objects with identical (empty) fields must be distinct keys.
    root_object(1);
    root_object(2);
    root_object(3);
    assert_eq!(
        js_weakmap_set(rooted(0), rooted(1), 11.0).to_bits(),
        rooted(0).to_bits()
    );
    js_weakmap_set(rooted(0), rooted(2), 22.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 11.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(2)), 22.0);
    js_weakmap_set(rooted(0), rooted(1), 33.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 33.0);
    assert_eq!(entry_count(), 2);
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    let extent = crate::weakref::test_support::weak_entry_extent(rooted(0));
    js_weakmap_set(rooted(0), rooted(3), 44.0);
    assert_eq!(
        crate::weakref::test_support::weak_entry_extent(rooted(0)),
        extent
    );
    assert_eq!(js_weakmap_get(rooted(0), rooted(3)), 44.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(2)), 22.0);
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
}

#[test]
fn weakmap_index_entry_visits_are_linear() {
    // Counts the actual entry-array reads, including index rebuilds and
    // validation. This fails deterministically for the old n(n +/- 1)/2 scans.
    for n in [1_000u32, 10_000, 100_000] {
        let _guard = guard(n + 1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        root_map();
        for slot in 1..=n {
            root_object(slot);
        }
        crate::weakref::test_support::reset_weak_entry_visits();
        for slot in 1..=n {
            js_weakmap_set(rooted(0), rooted(slot), slot as f64);
        }
        let inserts = crate::weakref::test_support::weak_entry_visits();
        assert!(
            inserts <= 4 * n as usize,
            "{n} inserts visited {inserts} entries"
        );
        crate::weakref::test_support::reset_weak_entry_visits();
        for slot in 1..=n {
            assert_eq!(js_weakmap_get(rooted(0), rooted(slot)), slot as f64);
        }
        let reads = crate::weakref::test_support::weak_entry_visits();
        assert!(reads >= n as usize, "counter must observe actual reads");
        assert!(reads <= 3 * n as usize, "{n} reads visited {reads} entries");
        crate::weakref::test_support::reset_weak_entry_visits();
        for slot in 1..=n {
            assert_eq!(
                js_weakmap_delete(rooted(0), rooted(slot)).to_bits(),
                crate::value::TAG_TRUE
            );
        }
        let deletes = crate::weakref::test_support::weak_entry_visits();
        eprintln!(
            "WeakMap n={n}: insert visits={inserts}, get visits={reads}, delete visits={deletes}"
        );
        assert!(
            deletes <= 3 * n as usize,
            "{n} deletes visited {deletes} entries"
        );
    }
}

#[test]
fn weakmap_index_rebuilds_after_moving_minors_and_full_collection() {
    let _guard = guard(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(2);
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    for cycle in 0..3 {
        let old_key = rooted(1).to_bits();
        // Populate the cache BEFORE each collection, then force its reuse.
        assert_eq!(
            js_weakmap_get(rooted(0), rooted(1)).to_bits(),
            rooted(2).to_bits()
        );
        assert!(crate::weakref::test_support::weak_entry_extent(rooted(0)) > 0);
        // Keep a young witness so even the post-promotion cycle must copy.
        root_object(3);
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        if cycle == 0 {
            assert_ne!(rooted(1).to_bits(), old_key);
        }
        assert_eq!(
            js_weakmap_get(rooted(0), rooted(1)).to_bits(),
            rooted(2).to_bits()
        );
        assert_eq!(
            js_weakmap_has(rooted(0), rooted(1)).to_bits(),
            crate::value::TAG_TRUE
        );
    }
    assert!(crate::arena::pointer_in_old_gen(
        (rooted(1).to_bits() & POINTER_MASK) as usize
    ));
    // An overwrite on the promoted entry must remember its new young value.
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    let before_value = rooted(2).to_bits();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(rooted(2).to_bits(), before_value);
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn weakmap_index_does_not_root_keys_or_alias_recycled_addresses() {
    let _guard = guard(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    // Only the map/index sees this key. Assert tombstoning directly, with no
    // dependency on when a finalization callback happens to run.
    root_object(1);
    let dead_bits = rooted(1).to_bits();
    js_weakmap_set(rooted(0), rooted(1), 91.0);
    js_shadow_slot_set(1, 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(entry_count(), 0);
    // Pointer-bit probes never dereference the old key. Even an exact reuse
    // of its address must not retrieve the dead key's value.
    assert_eq!(
        js_weakmap_get(rooted(0), f64::from_bits(dead_bits)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    let extent = crate::weakref::test_support::weak_entry_extent(rooted(0));
    for _ in 0..128 {
        root_object(1);
        assert_eq!(
            js_weakmap_has(rooted(0), rooted(1)).to_bits(),
            crate::value::TAG_FALSE
        );
        js_weakmap_set(rooted(0), rooted(1), 42.0);
        assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 42.0);
        assert_eq!(
            js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
            crate::value::TAG_TRUE
        );
    }
    assert_eq!(
        crate::weakref::test_support::weak_entry_extent(rooted(0)),
        extent
    );
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(!crate::weakref::weak_target_holders_allocated());
}

#[test]
fn weakset_uses_the_same_index_after_collection_and_reuse() {
    let _guard = guard(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = js_weakset_new();
    js_shadow_slot_set(0, ptr_bits(set as usize));
    root_object(1);
    assert_eq!(
        js_weakset_add(rooted(0), rooted(1)).to_bits(),
        rooted(0).to_bits()
    );
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_weakset_add(rooted(0), rooted(1)).to_bits(),
        rooted(0).to_bits()
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn weakmap_index_revalidates_a_cached_slot_tombstoned_between_slices() {
    let _guard = guard(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), 11.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 11.0);
    // Model a sliced weak pass publishing tombstones and rebuilding this
    // owned table. No collection registry or owner-address cache exists.
    unsafe {
        let map = (rooted(0).to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
        let table = crate::weakref::storage::owned_storage(map);
        let entry = (*table).entries();
        (*entry).key = crate::value::TAG_UNDEFINED;
        (*entry).value = crate::value::TAG_UNDEFINED;
        (*table).rebuild();
    }
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    js_weakmap_set(rooted(0), rooted(2), 22.0);
    assert_eq!(
        crate::weakref::test_support::weak_entry_extent(rooted(0)),
        1
    );
    assert_eq!(js_weakmap_get(rooted(0), rooted(2)), 22.0);
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
}

#[test]
fn weakmap_index_survives_explicit_full_gc_with_forced_movement() {
    let _guard = guard(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _schedule =
        crate::gc::schedule::ScheduleGuard::set(7, crate::gc::schedule::rate_threshold(1.0));
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    let key_before = rooted(1).to_bits();
    let value_before = rooted(2).to_bits();
    let copies_before = crate::gc::copying_minor_cycles();
    js_gc_collect();
    assert!(crate::gc::copying_minor_cycles() > copies_before);
    assert_ne!(rooted(1).to_bits(), key_before);
    assert_ne!(rooted(2).to_bits(), value_before);
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn born_old_weak_storage_recovers_unarmed_weak_slot_coverage() {
    let _guard = guard(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _schedule =
        crate::gc::schedule::ScheduleGuard::set(7, crate::gc::schedule::rate_threshold(1.0));
    struct Unarmed;
    impl Drop for Unarmed {
        fn drop(&mut self) {
            crate::gc::barrier_arming::close_barrier_arming_window_for_tests();
        }
    }
    crate::gc::barrier_arming::reset_barrier_arming_for_tests();
    let _unarmed = Unarmed;
    remembered_set_clear();
    root_map();
    crate::weakref::test_support::reserve_weak_storage_for_tests(rooted(0), 8192);
    let owner = (rooted(0).to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
    let table = unsafe { crate::weakref::storage::owned_storage(owner) };
    assert!(crate::arena::pointer_in_old_gen(table as usize));
    root_object(1);
    root_object(2);
    let old_key = rooted(1).to_bits();
    assert!(crate::arena::pointer_in_nursery(
        (old_key & POINTER_MASK) as usize
    ));
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    let slot_page = unsafe { crate::arena::generation_page_for_addr((*table).entries() as usize) };
    assert!(
        !crate::gc::barrier_arming::barrier_remembering_armed(),
        "the fixture must remain inside the unarmed window"
    );
    assert_eq!(remembered_dirty_page_count(), 0);
    assert!(
        !old_page_dirty_for(slot_page),
        "the unarmed barrier must actually skip coverage"
    );
    // Taking the first snapshot itself arms and reconstructs the barrier.
    // Assert the skipped weak entry's exact page was recovered before copy.
    let _ = remembered_dirty_snapshot();
    assert!(old_page_dirty_for(slot_page));
    assert_eq!(
        crate::gc::barrier_arming::remembered_reconstruct_census().reconstructs,
        1
    );
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(
        old_key,
        rooted(1).to_bits(),
        "the live weak key must actually move"
    );
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
}

// #12087: a value-to-key backedge is conditional, never an independent root.
#[test]
fn weakmap_ephemeron_value_backedge_should_not_root_key() {
    let _guard = guard(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    root_object(1);
    assert!(crate::arena::pointer_in_nursery(
        (rooted(1).to_bits() & POINTER_MASK) as usize
    ));
    // wm.set(key, key): the value is conditional on independent key liveness.
    js_weakmap_set(rooted(0), rooted(1), rooted(1));
    assert_eq!(entry_count(), 1, "the entry must exist before collection");
    js_shadow_slot_set(1, 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        entry_count(),
        0,
        "the value must not keep its own key alive"
    );
}

/// #12087: the independent C witness's movement/control verdict, run as an
/// always-active runtime regression so it needs no prebuilt static archive.
#[test]
fn weakmap_ephemeron_backedge_and_weak_only_control_die_after_explicit_gc() {
    let _guard = guard(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _schedule =
        crate::gc::schedule::ScheduleGuard::set(7, crate::gc::schedule::rate_threshold(1.0));
    root_map();
    root_object(1);
    js_weakmap_set(rooted(0), rooted(1), rooted(1));
    let reference = crate::weakref::js_weakref_new(rooted(1));
    js_shadow_slot_set(2, ptr_bits(reference as usize));
    root_object(1);
    let control = crate::weakref::js_weakref_new(rooted(1));
    js_shadow_slot_set(3, ptr_bits(control as usize));
    js_shadow_slot_set(1, 0);
    let old_ref = rooted(2).to_bits();
    let copies_before = crate::gc::copying_minor_cycles();
    js_gc_collect();
    assert!(crate::gc::copying_minor_cycles() > copies_before);
    assert_ne!(old_ref, rooted(2).to_bits(), "the rooted holder must move");
    assert_eq!(
        crate::weakref::js_weakref_deref(rooted(3)).to_bits(),
        crate::value::TAG_UNDEFINED,
        "the weak-only control must die before the verdict is meaningful"
    );
    assert_eq!(
        crate::weakref::js_weakref_deref(rooted(2)).to_bits(),
        crate::value::TAG_UNDEFINED,
        "the conditional value must not keep its own key alive"
    );
    assert_eq!(entry_count(), 0);
}

#[test]
fn weakmap_ephemeron_indirect_cycles_and_cross_map_fixed_point() {
    let _guard = guard(6);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    root_object(1);
    root_object(2);
    let second = js_weakmap_new();
    js_shadow_slot_set(3, ptr_bits(second as usize));
    root_object(4);
    // Map A's independently rooted key reaches B's key, which reaches a value
    // held only by map B. Neither hash/mark visitation order may lose it.
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    js_weakmap_set(rooted(3), rooted(2), rooted(4));
    let value_ref = crate::weakref::js_weakref_new(rooted(4));
    js_shadow_slot_set(5, ptr_bits(value_ref as usize));
    js_shadow_slot_set(2, 0);
    js_shadow_slot_set(4, 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let key_b = js_weakmap_get(rooted(0), rooted(1));
    assert_ne!(key_b.to_bits(), crate::value::TAG_UNDEFINED);
    assert_eq!(
        js_weakmap_get(rooted(3), key_b).to_bits(),
        crate::weakref::js_weakref_deref(rooted(5)).to_bits()
    );
    assert_ne!(
        crate::weakref::js_weakref_deref(rooted(5)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    // With no independently live key, even an indirect cycle is collectable.
    // Keep young objects: old generation is conservatively live in a minor.
    root_object(1);
    root_object(2);
    let back = crate::string::js_string_from_bytes(b"back".as_ptr(), 4);
    crate::object::js_object_set_field_by_name(
        (rooted(2).to_bits() & POINTER_MASK) as *mut crate::ObjectHeader,
        back,
        rooted(1),
    );
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    js_shadow_slot_set(1, 0);
    js_shadow_slot_set(2, 0);
    age_ephemeron_fixture();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(entry_count(), 0);
}

#[test]
fn weakmap_ephemeron_proxy_chain_and_dead_proxy_backedge() {
    let _guard = guard(6);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    root_map();
    root_object(1);
    let second = js_weakmap_new();
    js_shadow_slot_set(2, ptr_bits(second as usize));
    root_object(3);
    root_object(4);
    root_object(5);
    // Strong endpoints make observing this proxy add NO new heap marks.
    // Its newly live handle must still enable B's conditional value.
    let proxy = crate::proxy::js_proxy_new(rooted(4), rooted(5));
    js_weakmap_set(rooted(0), proxy, rooted(3));
    js_weakmap_set(rooted(2), rooted(1), proxy);
    js_shadow_slot_set(3, 0);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let proxy = js_weakmap_get(rooted(2), rooted(1));
    assert!(crate::proxy::test_proxy_slot_is_live(proxy));
    assert_ne!(
        js_weakmap_get(rooted(0), proxy).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    // Keep only wm.set(proxy, proxy). Neither endpoint nor this value is an
    // independent reference to the proxy handle.
    js_weakmap_set(rooted(0), proxy, proxy);
    assert_eq!(js_weakmap_get(rooted(0), proxy).to_bits(), proxy.to_bits());
    js_shadow_slot_set(1, 0);
    js_shadow_slot_set(2, 0);
    age_ephemeron_fixture();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
    assert_eq!(entry_count(), 0);
}

// Full cycles conservatively retain recent general blocks. Move the negative
// witnesses out of that window so only their actual graph determines liveness.
fn age_ephemeron_fixture() {
    let start = crate::arena::general_block_count();
    while crate::arena::general_block_count().saturating_sub(start) < 7 {
        for _ in 0..64 {
            let _ = crate::arena::arena_alloc_gc(4096, 8, GC_TYPE_STRING);
        }
    }
}

#[test]
fn weakmap_mark_verifier_checks_only_enabled_values() {
    let _guard = guard(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    unsafe {
        let map = (rooted(0).to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
        let table = crate::weakref::storage::owned_storage(map);
        let header = header_from_user_ptr(table.cast());
        let key = header_from_user_ptr((rooted(1).to_bits() & POINTER_MASK) as *const u8);
        let value = header_from_user_ptr((rooted(2).to_bits() & POINTER_MASK) as *const u8);
        (*header).gc_flags |= GC_FLAG_MARKED;
        (*key).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_PINNED);
        (*value).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_PINNED);
        let mut stats = MarkInvariantVerifyStats::default();
        verify_marked_object_child_marks(&mut stats, header);
        assert_eq!(
            stats.checked_edges, 0,
            "a white key must not enable its value"
        );
        (*key).gc_flags |= GC_FLAG_MARKED;
        let mut stats = MarkInvariantVerifyStats::default();
        verify_marked_object_child_marks(&mut stats, header);
        assert_eq!(
            stats.missing_edges, 1,
            "a live key makes a white value an error"
        );
        (*value).gc_flags |= GC_FLAG_MARKED;
        let mut stats = MarkInvariantVerifyStats::default();
        verify_marked_object_child_marks(&mut stats, header);
        assert_eq!(stats.checked_edges, 1);
        assert_eq!(stats.missing_edges, 0);
        (*header).gc_flags &= !GC_FLAG_MARKED;
        (*key).gc_flags &= !GC_FLAG_MARKED;
        (*value).gc_flags &= !GC_FLAG_MARKED;
    }
}
