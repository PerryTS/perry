//! Object.create must keep its prototype alive only through its owner.
use super::super::*;
use super::support::*;
use crate::object::*;

#[test]
fn object_create_does_not_register_classes_or_invalidate_existing_caches() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = js_object_alloc(0, 0);
    let bits = ptr_bits(proto as usize);
    let next_id = NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed);
    let class_generation = class_lookup_surface_generation();
    let plan_epoch = prop_plan::prop_plan_semantic_epoch();
    let element_epoch = crate::array::js_array_element_shape_epoch();
    for _ in 0..1024 {
        let obj = js_object_create(f64::from_bits(bits));
        assert_eq!(js_object_get_prototype_of(obj).to_bits(), bits);
        let ptr = crate::value::js_nanbox_get_pointer(obj) as *const ObjectHeader;
        assert_eq!(unsafe { (*ptr).class_id }, 0);
    }
    assert_eq!(
        NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed),
        next_id
    );
    assert_eq!(class_lookup_surface_generation(), class_generation);
    assert_eq!(prop_plan::prop_plan_semantic_epoch(), plan_epoch);
    assert_eq!(crate::array::js_array_element_shape_epoch(), element_epoch);
    let mut retained = false;
    scan_class_side_table_roots(&mut |value| {
        retained |= value.to_bits() == bits;
    });
    assert!(
        !retained,
        "the prototype must not become a permanent class root"
    );
}

#[test]
fn object_create_prototype_edge_survives_copying_without_a_separate_root() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = js_object_alloc(0, 1);
    js_object_set_field(proto, 0, crate::value::JSValue::number(42.0));
    let obj = js_object_create(f64::from_bits(ptr_bits(proto as usize)));
    js_shadow_slot_set(0, obj.to_bits());
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let moved_obj = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(moved_obj.to_bits(), obj.to_bits(), "owner must move");
    let moved_proto = js_object_get_prototype_of(moved_obj);
    assert_ne!(
        moved_proto.to_bits(),
        ptr_bits(proto as usize),
        "prototype must move"
    );
    let ptr = crate::value::js_nanbox_get_pointer(moved_proto) as *const ObjectHeader;
    assert_eq!(js_object_get_field(ptr, 0).to_number(), 42.0);
}

#[test]
fn object_create_inherits_live_properties_and_preserves_null() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let proto = js_object_alloc(0, 0);
    let obj = js_object_create(f64::from_bits(ptr_bits(proto as usize)));
    let child = js_object_create(obj);
    let ptr = crate::value::js_nanbox_get_pointer(child) as *const ObjectHeader;
    for value in [42.0, 73.0] {
        js_object_set_field_by_name(proto, key, value);
        assert_eq!(js_object_get_field_by_name(ptr, key).to_number(), value);
        assert_eq!(
            js_object_has_own(child, crate::value::js_nanbox_string(key as i64)).to_bits(),
            crate::value::TAG_FALSE
        );
    }
    let null_obj = js_object_create(f64::from_bits(crate::value::TAG_NULL));
    assert_eq!(
        js_object_get_prototype_of(null_obj).to_bits(),
        crate::value::TAG_NULL
    );
}

#[test]
fn object_create_keeps_distinct_prototypes_and_instanceof_chains() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    const CLASS: u32 = 0x5106;
    unsafe {
        js_register_class_id(CLASS);
        js_register_class_name(CLASS, b"CreatedBase".as_ptr(), 11);
    }
    let proto = js_object_alloc(0, 0);
    class_decl_prototype_object_root_store(CLASS, proto);
    let obj = js_object_create(f64::from_bits(ptr_bits(proto as usize)));
    let child = js_object_create(obj);
    let other = js_object_create(f64::from_bits(ptr_bits(js_object_alloc(0, 0) as usize)));
    assert_eq!(js_instanceof(obj, CLASS).to_bits(), crate::value::TAG_TRUE);
    assert_eq!(
        js_instanceof(child, CLASS).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_instanceof(other, CLASS).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_ne!(
        js_object_get_prototype_of(obj).to_bits(),
        js_object_get_prototype_of(other).to_bits()
    );
}

#[test]
fn object_create_prototype_is_reclaimed_once_its_owner_dies() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = super::cycle_state::alloc_tracked_test_object();
    let obj = js_object_create(f64::from_bits(ptr_bits(proto as usize)));
    // Positive control: while the owner is rooted, its prototype survives a
    // full collection through the owner's meta record alone.
    js_shadow_slot_set(0, obj.to_bits());
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(
        malloc_user_ptr_tracked(proto as *mut u8),
        "a live owner must keep its prototype"
    );
    let obj = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        js_object_get_prototype_of(obj).to_bits(),
        ptr_bits(proto as usize)
    );
    // Once the owner is unreachable nothing else may hold the prototype.
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(
        !malloc_user_ptr_tracked(proto as *mut u8),
        "an unreachable Object.create prototype must be collected"
    );
}
