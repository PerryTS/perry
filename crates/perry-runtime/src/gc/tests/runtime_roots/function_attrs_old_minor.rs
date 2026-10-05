//! #11931: both minor-collection edges of an aged function's attribute bag.
use super::*;

fn old_builtin_attributes_survive_minor(promote_bag: bool) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _verify = VerifyEvacuationTestGuard::on();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);

    let scope = RuntimeHandleScope::new();
    let owner =
        crate::closure::js_closure_alloc(crate::fn_info!(test_no_capture_singleton_func, 0), 0);
    let handle = scope.root_raw_const_ptr(owner);
    if promote_bag {
        crate::object::set_bound_native_closure_name(owner as *mut _, "before");
    }
    let (trace, owner) = handle.across_const::<crate::closure::ClosureHeader, _>(|| {
        collect_minor_trace(GcTriggerKind::Direct)
    });
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let owner = owner as usize;
    assert!(
        !crate::arena::pointer_in_nursery(owner),
        "function must be old"
    );
    let previous_bag = unsafe { crate::closure::props::bag_of(owner) };
    if promote_bag {
        assert!(!previous_bag.is_null());
        assert!(
            !crate::arena::pointer_in_nursery(previous_bag as usize),
            "bag must be old"
        );
    } else {
        assert!(
            previous_bag.is_null(),
            "exercise first bag attachment to an old closure"
        );
    }

    crate::object::set_bound_native_closure_name(owner as *mut _, "after");
    let attrs = crate::object::PropertyAttrs::new(true, true, true);
    crate::object::set_property_attrs(owner, "name".into(), attrs);
    crate::closure::closure_define_data_with_attrs(owner, "length", 17.0, attrs);
    let bag = unsafe { crate::closure::props::bag_of(owner) };
    let descriptor = unsafe { crate::object::shapes::object_shape_descriptor(bag) }.unwrap();
    let keys = descriptor.keys as usize;
    assert!(
        crate::arena::pointer_in_nursery(keys),
        "successor keys must be young"
    );
    if promote_bag {
        assert_eq!(bag, previous_bag);
        assert!(
            descriptor.old_carrier,
            "old bag must arm the successor shape's minor gate"
        );
    } else {
        assert!(
            crate::arena::pointer_in_nursery(bag as usize),
            "new bag must be young"
        );
    }

    let (trace, owner_after) = handle.across_const::<crate::closure::ClosureHeader, _>(|| {
        collect_minor_trace(GcTriggerKind::Direct)
    });
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let owner_after = owner_after as usize;
    assert_eq!(owner_after, owner, "old closure must stay in place");
    let bag_after = unsafe { crate::closure::props::bag_of(owner_after) };
    if promote_bag {
        assert_eq!(bag_after, bag);
    } else {
        assert_ne!(bag_after, bag, "closure bag slot must be rewritten");
    }
    let after = unsafe { crate::object::shapes::object_shape_descriptor(bag_after) }.unwrap();
    assert_ne!(
        after.keys as usize, keys,
        "shape table keys slot must be rewritten"
    );
    let name = crate::closure::closure_get_own_dynamic_prop(owner_after, "name").unwrap();
    assert_eq!(
        crate::string::string_as_str((name.to_bits() & POINTER_MASK) as *const crate::StringHeader),
        "after"
    );
    assert_eq!(crate::object::builtin_closure_length(owner_after), Some(17));
    for key in ["name", "length"] {
        assert_eq!(
            crate::object::get_property_attrs(owner_after, key)
                .unwrap()
                .bits,
            attrs.bits
        );
    }
}

#[test]
fn old_builtin_young_bag_survives_evacuating_minor() {
    old_builtin_attributes_survive_minor(false);
}

#[test]
fn old_builtin_old_bag_young_keys_survive_evacuating_minor() {
    old_builtin_attributes_survive_minor(true);
}
