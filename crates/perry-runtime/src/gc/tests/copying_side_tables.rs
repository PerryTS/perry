//! Copying-GC minor-collection rewrites for the SIDE TABLES (class metadata,
//! symbol registries) — split out of `copying.rs`, which had crossed the
//! 2000-line size gate. Same fixtures and guards as its sibling.

use super::super::*;
use super::support::*;

#[test]
fn test_copying_minor_rewrites_class_side_table_values_and_function_keys() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::object::test_clear_class_side_table_roots();
    gc_register_mutable_root_scanner(crate::object::scan_class_side_table_roots_mut);

    let value = young_leaf();
    let prototype_object = crate::object::js_object_alloc(0, 0) as usize;
    let parent_closure = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as usize;
    let key = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as usize;
    unsafe {
        init_test_closure(parent_closure as *mut u8);
        init_test_closure(key as *mut u8);
    }
    js_shadow_slot_set(0, ptr_bits(key));

    crate::object::test_seed_class_dynamic_prop_root(0x5401, "dyn", string_bits(value));
    crate::object::test_seed_class_prototype_method_root(0x5401, "proto", string_bits(value));
    crate::object::test_seed_class_prototype_method_value_root(0x5401, "bound", string_bits(value));
    crate::object::test_seed_class_prototype_object_root(0x5401, prototype_object);
    crate::object::test_seed_class_parent_closure_root(0x5401, parent_closure);
    crate::object::test_seed_function_class_id_key(ptr_bits(key), 0x8200_5401);

    let _ = gc_collect_minor();

    let dynamic_bits = crate::object::test_class_dynamic_prop_root_bits(0x5401, "dyn");
    let prototype_bits = crate::object::test_class_prototype_method_root_bits(0x5401, "proto");
    let cached_bits = crate::object::test_class_prototype_method_value_root_bits(0x5401, "bound");
    let prototype_object_after = crate::object::test_class_prototype_object_root_addr(0x5401);
    let parent_closure_after = crate::object::test_class_parent_closure_root_addr(0x5401);
    let value_after = (dynamic_bits & POINTER_MASK) as usize;
    let key_after_bits = js_shadow_slot_get(0);

    assert_eq!(dynamic_bits & TAG_MASK, STRING_TAG);
    assert_eq!(prototype_bits, dynamic_bits);
    assert_eq!(cached_bits, dynamic_bits);
    assert_ne!(value_after, value);
    assert!(crate::arena::pointer_in_nursery(value_after));
    assert_ne!(prototype_object_after, prototype_object);
    assert!(crate::arena::pointer_in_nursery(prototype_object_after));
    assert_ne!(parent_closure_after, parent_closure);
    assert!(crate::arena::pointer_in_nursery(parent_closure_after));
    assert_ne!(key_after_bits, ptr_bits(key));
    assert_eq!(
        crate::object::test_function_class_id_key_for_class(0x8200_5401),
        key_after_bits
    );
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(key_after_bits)),
        0x8200_5401
    );
}

#[test]
fn test_copying_minor_rewrites_symbol_side_table_roots_and_lookups() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::symbol::test_clear_symbol_side_table_roots();
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);

    let owner = crate::object::js_object_alloc(0, 0) as usize;
    let sym_key = unsafe { alloc_nursery_test_symbol() };
    let value = young_leaf();
    let static_sym_key = unsafe { alloc_nursery_test_symbol() };
    let static_value = young_leaf();
    js_shadow_slot_set(0, ptr_bits(owner));

    crate::symbol::test_seed_symbol_pointer_root(sym_key);
    crate::symbol::test_seed_symbol_pointer_root(static_sym_key);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            f64::from_bits(ptr_bits(owner)),
            f64::from_bits(ptr_bits(sym_key)),
            f64::from_bits(string_bits(value)),
        );
        crate::symbol::js_class_register_static_symbol(
            0x5402,
            f64::from_bits(ptr_bits(static_sym_key)),
            f64::from_bits(string_bits(static_value)),
        );
    }

    let _ = gc_collect_minor();

    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let entries = crate::symbol::test_symbol_property_roots(owner_after);
    assert_eq!(entries.len(), 1);
    let (sym_key_after, value_bits_after) = entries[0];
    let value_after = (value_bits_after & POINTER_MASK) as usize;
    let static_entries = crate::symbol::test_class_static_symbol_roots_for_class(0x5402);
    assert_eq!(static_entries.len(), 1);
    let (static_sym_key_after, static_value_bits_after) = static_entries[0];
    let static_value_after = (static_value_bits_after & POINTER_MASK) as usize;

    assert_ne!(owner_after, owner);
    assert_ne!(sym_key_after, sym_key);
    assert_ne!(value_after, value);
    assert_ne!(static_sym_key_after, static_sym_key);
    assert_ne!(static_value_after, static_value);
    assert!(crate::arena::pointer_in_nursery(owner_after));
    assert!(crate::arena::pointer_in_nursery(sym_key_after));
    assert!(crate::arena::pointer_in_nursery(value_after));
    assert!(crate::arena::pointer_in_nursery(static_sym_key_after));
    assert!(crate::arena::pointer_in_nursery(static_value_after));
    assert!(
        !crate::symbol::test_symbol_property_owner_exists(owner),
        "symbol side table should not keep the stale owner key after moving"
    );
    assert!(crate::symbol::test_symbol_pointer_root_contains(
        sym_key_after
    ));
    assert!(crate::symbol::test_symbol_pointer_root_contains(
        static_sym_key_after
    ));
    assert!(!crate::symbol::test_symbol_pointer_root_contains(sym_key));
    assert!(!crate::symbol::test_symbol_pointer_root_contains(
        static_sym_key
    ));

    let moved_owner = f64::from_bits(ptr_bits(owner_after));
    let moved_sym = f64::from_bits(ptr_bits(sym_key_after));
    let moved_static_sym = f64::from_bits(ptr_bits(static_sym_key_after));
    let class_ref = f64::from_bits(crate::value::INT32_TAG | 0x5402);
    unsafe {
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(moved_owner, moved_sym).to_bits(),
            value_bits_after
        );
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(class_ref, moved_static_sym).to_bits(),
            static_value_bits_after
        );
    }
}

#[test]
fn test_copying_minor_rewrites_old_overflow_object_child_without_reentrant_borrow() {
    struct OverflowFieldsRootGuard;

    impl Drop for OverflowFieldsRootGuard {
        fn drop(&mut self) {
            crate::object::test_clear_overflow_fields_root();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _overflow_guard = OverflowFieldsRootGuard;
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::object::test_clear_overflow_fields_root();

    let (owner, _) = unsafe { alloc_old_test_object(8) };
    let owner_addr = owner as usize;
    assert!(crate::arena::pointer_in_old_gen(owner_addr));
    js_shadow_slot_set(0, ptr_bits(owner_addr));

    for i in 0..8 {
        let name = format!("k{i}");
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(owner, key, i as f64);
    }

    let child = crate::object::js_object_alloc(0, 0) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    unsafe {
        assert_eq!((*child_header).obj_type, GC_TYPE_OBJECT);
    }
    assert!(crate::arena::pointer_in_nursery(child));

    let overflow_key = crate::string::js_string_from_bytes(b"k8".as_ptr(), 2);
    crate::object::js_object_set_field_by_name(
        owner,
        overflow_key,
        f64::from_bits(ptr_bits(child)),
    );
    assert_eq!(
        crate::object::test_overflow_field_bits(owner_addr, 8) & POINTER_MASK,
        child as u64
    );
    assert!(
        remembered_set_size() > 0,
        "old overflow slot write must enter remembered metadata"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after =
        (crate::object::test_overflow_field_bits(owner_addr, 8) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(owner_after, owner_addr);
    assert_ne!(child_after, child);
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert!(trace.copying_nursery.copied_objects >= 1);
    assert_eq!(trace.remembered_set.dirty_objects_scanned, 1);
    assert!(
        trace.remembered_set.dirty_pages_scanned <= 2,
        "old owner page plus overflow Vec page should bound copied-minor scanning"
    );
    assert!(
        trace.remembered_set.dirty_slots_scanned <= 32,
        "overflow regression should scan only the dirty owner slots"
    );

    for _ in 0..3 {
        let _ = gc_collect_minor();
    }
    let promoted = (crate::object::test_overflow_field_bits(owner_addr, 8) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_old_gen(promoted));
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(
        stats.checked_old_to_young_edges, 0,
        "old overflow edge should stop being old-to-young once the child promotes"
    );
    assert_eq!(stats.missing_edges, 0);
}
