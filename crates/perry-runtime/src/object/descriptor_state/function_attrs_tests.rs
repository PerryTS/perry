use super::super::*;

extern "C" fn body(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    0.0
}

#[test]
fn function_attribute_writes_are_owned_by_the_bag() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0) as usize;
    crate::object::set_bound_native_closure_name(closure as *mut _, "original");
    let bag = unsafe { crate::closure::props::bag_of(closure) };
    let before = unsafe { (*bag).shape_word() };
    set_property_attrs(closure, "name".into(), PropertyAttrs::new(true, true, true));
    assert_ne!(before, unsafe { (*bag).shape_word() });
    assert_eq!(
        get_property_attrs(closure, "name").unwrap().bits,
        PropertyAttrs::new(true, true, true).bits,
        "default attrs must replace the old key entry"
    );
    assert_eq!(
        unsafe { crate::object::key_attrs::object_key_entry(bag, b"name") },
        0
    );
    crate::closure::closure_define_dynamic_prop(closure, "name", 42.0);
    assert_eq!(
        crate::closure::closure_get_own_dynamic_prop(closure, "name"),
        Some(42.0)
    );
    set_property_attrs(
        closure,
        "name".into(),
        PropertyAttrs::new(false, true, true),
    );
    crate::closure::closure_define_dynamic_prop(closure, "name", 43.0);
    assert_eq!(
        crate::closure::closure_get_own_dynamic_prop(closure, "name"),
        Some(43.0)
    );
    set_builtin_accessor_descriptor(
        closure,
        "custom".into(),
        AccessorDescriptor { get: 0, set: 0 },
        PropertyAttrs::new(false, false, true),
    );
    assert!(get_accessor_descriptor(closure, "custom").is_some());
    assert_eq!(accessor_descriptor_keys_for_obj(closure), vec!["custom"]);
    clear_accessor_descriptor(closure, "custom");
    assert!(get_accessor_descriptor(closure, "custom").is_none());
}

/// A function with the one own slot codegen gives a constructing function's
/// `prototype`.
fn function_with_prototype_slot() -> usize {
    static INFO: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let info = *INFO.get_or_init(|| {
        let mut info = unsafe { std::ptr::read(crate::fn_info!(body, 0)) };
        info.own_slots = 1;
        Box::leak(Box::new(info)) as *const crate::closure::JsFunctionInfo as usize
    });
    crate::closure::js_closure_alloc(info as *const crate::closure::JsFunctionInfo, 0) as usize
}

#[test]
fn prototype_birth_attributes_stay_with_the_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let closure = function_with_prototype_slot();
    crate::closure::closure_define_dynamic_prop(closure, "prototype", 5.0);
    set_property_attrs(
        closure,
        "prototype".into(),
        PropertyAttrs::new(true, false, false),
    );
    unsafe {
        assert!(crate::closure::props::prototype_slot_holds_key(closure));
        assert!(!crate::closure::props::bag_holds_prototype(closure));
    }
    assert_eq!(
        crate::closure::closure_get_own_dynamic_prop(closure, "prototype"),
        Some(5.0)
    );

    set_property_attrs(
        closure,
        "prototype".into(),
        PropertyAttrs::new(false, false, false),
    );
    unsafe { assert!(crate::closure::props::bag_holds_prototype(closure)) };
    assert_eq!(
        crate::closure::closure_get_own_dynamic_prop(closure, "prototype"),
        Some(5.0)
    );
    assert_eq!(
        get_property_attrs(closure, "prototype").unwrap().bits,
        PropertyAttrs::new(false, false, false).bits
    );
}
