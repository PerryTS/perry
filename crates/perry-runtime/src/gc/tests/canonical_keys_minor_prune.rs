//! I3: canonical backings are mortal old metadata, never nursery objects.
use super::super::*;
use super::support::*;
use crate::object::canonical_keys::{self, CanonicalKeys, SharedLayout};

fn scanners() {
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
}

unsafe fn list(name: &[u8]) -> CanonicalKeys {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    canonical_keys::extend_key(
        &SharedLayout::shape_cache_entry(),
        CanonicalKeys::EMPTY,
        key,
    )
}

#[test]
fn old_backings_survive_both_minor_paths_and_die_at_full_trace() {
    for copying in [true, false] {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force = ForcedEvacuationTestGuard::on();
        scanners();
        canonical_keys::reset_for_test();
        let keys = unsafe { list(b"mortal_old_metadata_key") };
        assert!(crate::arena::pointer_in_old_gen(keys.addr()));
        if !copying {
            js_gc_write_barriers_emitted(0);
        }
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(matches!(trace.collection_kind, GcCollectionKind::Minor));
        assert_eq!(trace.copying_nursery.eligible, copying);
        assert_eq!(
            canonical_keys::published_lists_for_test(),
            vec![keys.as_ptr()]
        );
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        assert!(
            canonical_keys::published_lists_for_test().is_empty(),
            "movement pin must not pin liveness"
        );
        canonical_keys::reset_for_test();
    }
}

#[test]
fn stationary_backing_rewrites_young_keys_through_remembered_slots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    // An explicitly installed young child keeps the four-evacuation control.
    // Production publication requests first-copy tenure for these leaves.
    let keys_array = crate::array::js_array_alloc_canonical_keys(1, true, false);
    let key = crate::string::js_string_from_bytes(
        b"a_young_heap_key_longer_than_sso".as_ptr(),
        b"a_young_heap_key_longer_than_sso".len() as u32,
    );
    unsafe {
        crate::array::note_array_slot(keys_array, 0, crate::JSValue::string_ptr(key).bits());
        (*keys_array).length = 1;
    }
    let keys = CanonicalKeys::unlinked_for_test(keys_array, 1);
    let root = scope.root_raw_mut_ptr(keys.as_ptr());
    let mut before = crate::array::js_array_get(keys.as_ptr(), 0).as_string_ptr();
    for _ in 0..4 {
        assert!(crate::arena::pointer_in_nursery(before as usize));
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            trace.copying_nursery.copied_objects + trace.copying_nursery.promoted_objects > 0,
            "the child really must move"
        );
        root.with_const_ptr(|p: *const crate::ArrayHeader| unsafe {
            assert_eq!(p as usize, keys.addr());
            let word = crate::array::js_array_get(p, 0);
            let after = word.as_string_ptr();
            assert_ne!(before, after);
            before = after;
            let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
            assert_eq!(
                crate::string::js_string_key_bytes(word, &mut sso),
                Some(b"a_young_heap_key_longer_than_sso".as_slice())
            );
        });
    }
    assert!(crate::arena::pointer_in_old_gen(before as usize));
    canonical_keys::reset_for_test();
}

#[test]
fn published_key_moves_to_old_on_its_first_minor_without_pinning_liveness() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    let keys = unsafe { list(b"first_copy_tenured_key") };
    let root = scope.root_raw_mut_ptr(keys.as_ptr());
    let before = crate::array::js_array_get(keys.as_ptr(), 0).as_string_ptr();
    assert!(crate::arena::pointer_in_nursery(before as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.promoted_objects > 0);
    root.with_const_ptr(|p: *const crate::ArrayHeader| unsafe {
        assert_eq!(p as usize, keys.addr());
        let after = crate::array::js_array_get(p, 0).as_string_ptr();
        assert_ne!(after, before, "the key must really relocate");
        assert!(crate::arena::pointer_in_old_gen(after as usize));
        assert_eq!(
            (*header_from_user_ptr(after.cast())).gc_flags & GC_FLAG_PINNED,
            0
        );
    });
    assert_eq!(
        unsafe { list(b"first_copy_tenured_key").addr() },
        keys.addr()
    );
    drop(scope);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(canonical_keys::published_lists_for_test().is_empty());
    canonical_keys::reset_for_test();
}

/// Force the production relocation funnel directly: selection is an
/// optimization and may decline a sparse fixture. Clear the movement bit for
/// the negative control, proving the same block is otherwise movable.
fn defrag_moves_backing(remove_pin: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    canonical_keys::reset_for_test();
    let capacity = (crate::arena::BLOCK_SIZE / std::mem::size_of::<f64>() + 1) as u32;
    let backing = crate::array::js_array_alloc_canonical_keys(capacity, true, false);
    let keys = CanonicalKeys::unlinked_for_test(backing, 0);
    unsafe {
        let header = header_from_user_ptr(keys.as_ptr().cast());
        assert_ne!((*header).gc_flags & GC_FLAG_SHAPE_SHARED, 0);
        if remove_pin {
            (*header).gc_flags &= !GC_FLAG_SHAPE_SHARED;
        }
    }
    let mut selected = crate::fast_hash::new_ptr_hash_set();
    selected.insert(crate::arena::generation_page_for_addr(keys.addr()));
    let mut occupants = 0;
    crate::arena::old_arena_walk_objects_on_pages(&selected, |_| occupants += 1);
    assert_eq!(occupants, 1, "the pin witness must have a private block");
    let mut new = Vec::new();
    let mut old = Vec::new();
    let trace =
        super::super::oldgen::evacuate_selected_old_pages_collecting(&selected, &mut new, &mut old);
    let moved = old
        .iter()
        .any(|&header| unsafe { header.add(1) as usize == keys.addr() });
    if remove_pin {
        assert!(
            trace.old_page_moved_objects > 0,
            "negative control must exercise the mover"
        );
    }
    canonical_keys::reset_for_test();
    moved
}

#[test]
fn canonical_movement_pin_blocks_defrag_and_idle_compaction_funnel() {
    assert!(!defrag_moves_backing(false));
    assert!(
        defrag_moves_backing(true),
        "removing the pin must turn the stationary-address witness red"
    );
}
