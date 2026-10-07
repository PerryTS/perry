//! Descriptor facts are holder-shape facts, including array exotic holders.
use super::*;

extern "C" fn get_41(
    _c: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    41.0
}
fn getter() -> AccessorDescriptor {
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(get_41, 0), 0);
    AccessorDescriptor {
        get: crate::value::js_nanbox_pointer(closure as i64).to_bits(),
        set: 0,
    }
}
fn key(text: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

#[test]
fn array_accessor_facts_and_pair_are_in_the_holder_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let array = crate::array::js_array_alloc(0);
        set_builtin_property_attrs(
            array as usize,
            "x".into(),
            PropertyAttrs::new(true, true, true),
        );
        // An uncustomized array has no descriptor carrier to allocate.
        let acc = getter();
        set_accessor_descriptor(array as usize, "x".into(), acc);
        let bag = crate::array::array_property_bag(array);
        assert!(!bag.is_null(), "the array must own its descriptor holder");
        assert!(super::super::key_attrs::object_key_is_accessor(bag, b"x"));
        let shape = super::super::shapes::object_shape_stamp(bag);
        assert_eq!(
            get_accessor_descriptor(array as usize, "x").unwrap().get,
            acc.get
        );
        clear_accessor_descriptor(array as usize, "x");
        assert_ne!(shape, super::super::shapes::object_shape_stamp(bag));
        assert!(get_accessor_descriptor(array as usize, "x").is_none());
    }
}

#[test]
fn accessor_installed_after_a_holder_memo_retires_that_memo() {
    if !super::super::method_site::run_with_fresh_worker_gate(
        "accessor_installed_after_a_holder_memo_retires_that_memo",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let holder = crate::object::js_object_alloc(0, 1);
        crate::object::js_object_set_field_by_name(holder, key("laneAccessor"), 10.0);
        let receiver =
            crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
        let receiver = crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(receiver, key("pad"), 0.0);
        // Sites retain their PIC words in the registered root list.
        let site = Box::leak(Box::new(
            crate::object::field_get_set::runtime_read_site::RuntimeReadSite::new(),
        ));
        assert_eq!(site.read(receiver, b"laneAccessor"), 10.0);
        assert_eq!(
            site.read_leaf(receiver),
            Some(10.0),
            "subject: the holder memo must have primed"
        );
        let shape = super::super::shapes::object_shape_stamp(holder);
        set_accessor_descriptor(holder as usize, "laneAccessor".into(), getter());
        assert_ne!(shape, super::super::shapes::object_shape_stamp(holder));
        assert_ne!(
            site.read_leaf(receiver),
            Some(10.0),
            "an old holder shape cannot serve the data memo"
        );
        assert_eq!(site.read(receiver, b"laneAccessor"), 41.0);
    }
}

#[test]
fn array_descriptor_holder_survives_growth_without_rekeying() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let array = crate::array::js_array_alloc(0);
        set_accessor_descriptor(array as usize, "x".into(), getter());
        set_property_attrs(
            array as usize,
            "x".into(),
            PropertyAttrs::new(false, false, true),
        );
        let bag = crate::array::array_property_bag(array);
        let shape = super::super::shapes::object_shape_stamp(bag);
        let grown = crate::array::js_array_grow(crate::array::clean_arr_ptr_mut(array), 100);
        assert_ne!(array, grown, "subject: growth must replace the allocation");
        assert_eq!(crate::array::array_property_bag(grown), bag);
        assert_eq!(super::super::shapes::object_shape_stamp(bag), shape);
        assert!(get_accessor_descriptor(grown as usize, "x").is_some());
        assert!(!get_property_attrs(grown as usize, "x")
            .unwrap()
            .enumerable());
        assert!(
            get_accessor_descriptor(array as usize, "x").is_some(),
            "retained growth alias resolves to the same holder"
        );
    }
}
