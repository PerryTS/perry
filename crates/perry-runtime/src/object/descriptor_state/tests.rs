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
fn resolved_own_slots_observe_accessor_edits_in_narrow_wide_and_dictionary_holders() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        for (width, dictionary) in [(3, false), (300, false), (3, true)] {
            let object = crate::object::js_object_alloc(0, width);
            for i in 0..width {
                crate::object::js_object_set_field_by_name(
                    object,
                    key(&format!("resolved_holder_{i}")),
                    i as f64,
                );
            }
            if dictionary {
                assert!(crate::object::dictionary::latch_object_to_dictionary(
                    object
                ));
            }
            let lane = key("resolved_holder_1");
            let neighbor = key("resolved_holder_2");
            for _ in 0..3 {
                assert_eq!(
                    crate::object::js_object_get_field_by_name(object, lane).as_number(),
                    1.0
                );
            }
            let before = super::super::shapes::object_shape_stamp(object);
            set_accessor_descriptor(object as usize, "resolved_holder_1".into(), getter());
            assert_ne!(before, super::super::shapes::object_shape_stamp(object));
            for _ in 0..3 {
                assert_eq!(
                    crate::object::js_object_get_field_by_name(object, lane).as_number(),
                    41.0
                );
                assert_eq!(
                    crate::object::js_object_get_field_by_name(object, neighbor).as_number(),
                    2.0
                );
            }
            clear_accessor_descriptor(object as usize, "resolved_holder_1");
            crate::object::js_object_set_field_by_name(object, lane, 19.0);
            assert_eq!(
                crate::object::js_object_get_field_by_name(object, lane).as_number(),
                19.0
            );
        }
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

#[test]
fn freezing_an_array_snapshots_indices_before_the_holder_reserve_grows() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let mut array = crate::array::js_array_alloc(2);
        let len = (*array).capacity;
        for i in 0..len {
            array = crate::array::js_array_push(array, crate::JSValue::number(i as f64));
        }
        assert_eq!((*array).length, (*array).capacity);
        crate::object::js_object_freeze(crate::value::js_nanbox_pointer(array as i64));
        let live = crate::array::clean_arr_ptr_mut(array);
        assert_ne!(
            array, live,
            "subject: the holder reserve must grow the full array"
        );
        assert_eq!((*live).length, len);
        for key in ["0", "1", "length"] {
            let attrs = get_property_attrs(live as usize, key).unwrap();
            assert!(!attrs.writable() && !attrs.configurable());
        }
    }
}

#[test]
fn array_index_descriptors_inside_capacity_do_not_block_holey_dense_growth() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let array = crate::array::js_array_constructor_single(2_000_000.0);
        let array = crate::array::js_array_set_f64_extend(array, 0, 7.0);
        let array = crate::array::clean_arr_ptr_mut(array);
        set_property_attrs(
            array as usize,
            "0".into(),
            PropertyAttrs::new(false, true, true),
        );
        set_accessor_descriptor(array as usize, "1".into(), getter());
        let array = crate::array::clean_arr_ptr_mut(array);
        let capacity = (*array).capacity;
        assert!(capacity < (*array).length, "subject: a logical holey tail");
        assert!(!crate::array::array_has_sparse_index_properties_resolved(
            array
        ));
        let grown = crate::array::js_array_set_f64_extend(array, capacity, 42.0);
        assert!(
            (*grown).capacity > capacity,
            "near fill must grow the dense allocation"
        );
        assert_eq!(crate::array::js_array_get_f64(grown, capacity), 42.0);
        assert_eq!(crate::array::js_array_get_f64(grown, 0), 7.0);
        assert_eq!(crate::array::js_array_get_f64(grown, 1), 41.0);
        assert!(!get_property_attrs(grown as usize, "0").unwrap().writable());

        // A genuine sparse property still blocks growth across its value.
        let far = (*grown).capacity + 500_000;
        let sparse = crate::array::js_array_set_f64_extend(grown, far, 9.0);
        assert!(crate::array::array_has_sparse_index_properties_resolved(
            sparse
        ));
        let capacity = (*sparse).capacity;
        let filled = crate::array::js_array_set_f64_extend(sparse, capacity, 43.0);
        assert_eq!((*filled).capacity, capacity);
        assert_eq!(crate::array::js_array_get_f64(filled, far), 9.0);
        assert_eq!(crate::array::js_array_get_f64(filled, capacity), 43.0);
    }
}
