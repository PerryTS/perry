//! Shape-store regressions; the module path remains `shape_store::tests`.
use super::*;
#[test]
fn function_recreated_data_key_clears_the_deletion_marker() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = fresh_test_function();
    let ptr = (owner.to_bits() & crate::value::POINTER_MASK) as usize;
    let key = crate::string::canonical_key(b"recreatedFunctionData");
    let boxed_key = crate::value::js_nanbox_string(key as i64);
    assert_eq!(unsafe { try_store(owner, boxed_key, 43.0) }, Some(43.0));
    crate::closure::closure_mark_key_deleted(ptr, "recreatedFunctionData");
    assert!(crate::closure::closure_is_key_deleted(
        ptr,
        "recreatedFunctionData"
    ));
    assert_eq!(unsafe { try_store(owner, boxed_key, 107.0) }, Some(107.0));
    assert!(!crate::closure::closure_is_key_deleted(
        ptr,
        "recreatedFunctionData"
    ));
    assert!(super::super::obj_value_has_own_key(owner, boxed_key));
    let descriptor = super::super::js_object_get_own_property_descriptor(owner, boxed_key);
    assert_ne!(descriptor.to_bits(), crate::value::TAG_UNDEFINED);
}

#[test]
fn plain_add_uses_the_existing_chain_answer_without_recording_another_fact() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    unsafe {
        let key = crate::string::canonical_key(b"plainShapeAnswerOnly");
        let boxed_key = crate::value::js_nanbox_string(key as i64);
        const ANON: u32 = 0x63C2;
        crate::object::declarations::test_declare_anonymous(ANON);
        for obj in [
            super::super::js_object_alloc(0, 0),
            super::super::js_object_alloc_null_proto(0, 0),
            super::super::js_object_alloc(ANON, 0),
        ] {
            super::super::alloc_plain::mark_object_plain_ordinary(obj);
            let before = shapes::object_shape_record(obj).unwrap();
            assert!(!inherited_store_blocked(
                before,
                obj,
                crate::value::js_nanbox_string(key as i64),
                b"plainShapeAnswerOnly"
            ));
            let owner = crate::value::js_nanbox_pointer(obj as i64);
            assert_eq!(try_store(owner, boxed_key, 17.0), Some(17.0));
            assert_eq!(
                super::super::js_object_get_field_by_name_f64(obj, key),
                17.0
            );
        }
        let proto = crate::array::object_prototype_addr() as *mut ObjectHeader;
        super::super::js_object_set_field_by_name(proto, key, 29.0);
        super::super::set_property_attrs(
            proto as usize,
            "plainShapeAnswerOnly".into(),
            super::super::PropertyAttrs::new(false, true, true),
        );
        let fresh = super::super::js_object_alloc(0, 0);
        super::super::alloc_plain::mark_object_plain_ordinary(fresh);
        let owner = crate::value::js_nanbox_pointer(fresh as i64);
        let result = try_store(owner, boxed_key, 23.0);
        let present = super::super::obj_value_has_own_key(owner, boxed_key);
        super::super::js_object_delete_dynamic_value(
            crate::value::js_nanbox_pointer(proto as i64),
            boxed_key,
        );
        assert_eq!(
            result, None,
            "the default prototype's current attributes remain authoritative"
        );
        assert!(!present);
    }
}

#[test]
fn any_own_store_preserves_string_alias_demotion() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = super::super::js_object_alloc(0, 1);
        super::super::alloc_plain::mark_object_plain_ordinary(obj);
        let key = crate::string::canonical_key(b"ownedStringValue");
        super::super::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(
            shapes::object_shape_record(obj).unwrap().rep() & 3,
            super::super::field_rep::REP_ANY
        );
        let text = b"retained string snapshot";
        let string =
            crate::string::js_string_from_bytes_with_capacity(text.as_ptr(), text.len() as u32, 64);
        (*string).refcount = 1;
        let value = crate::value::js_nanbox_string(string as i64);
        assert!(try_own_store(obj, key, value));
        assert_eq!((*string).refcount, 0, "a heap slot makes the string shared");
        let suffix = crate::string::js_string_from_bytes(b" suffix".as_ptr(), 7);
        assert_ne!(crate::string::js_string_append(string, suffix), string);
        let stored = super::super::js_object_get_field_by_name_f64(obj, key);
        assert_eq!(stored.to_bits(), value.to_bits());
        assert_eq!(
            crate::string::header_str_checked(string)
                .unwrap()
                .as_bytes(),
            text
        );
    }
}

#[test]
fn string_index_own_store_uses_dense_presence_and_descriptor_proof() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    let values = [11.0, 22.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), 2);
    let index = crate::string::canonical_key(b"1");
    unsafe {
        assert!(try_own_store(arr.cast(), index, 33.0));
        assert_eq!(crate::array::js_array_get_f64(arr, 1), 33.0);
        for name in [b"01".as_slice(), b"-0", b"2", b"4294967295", b"length"] {
            assert!(!try_own_store(
                arr.cast(),
                crate::string::canonical_key(name),
                44.0
            ));
        }
        let hole = crate::array::js_array_alloc_with_length(2);
        assert!(!try_own_store(hole.cast(), index, 44.0));
        let header = (arr as *mut u8)
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        let saved = (*header)._reserved;
        (*header)._reserved |= crate::gc::OBJ_FLAG_FROZEN;
        assert!(!try_own_store(arr.cast(), index, 44.0));
        (*header)._reserved = saved;
        // An element descriptor is a key of the array's named layout: its
        // attribute summary sends every own element store to the exact path.
        crate::object::set_property_attrs(
            arr as usize,
            "0".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert!(crate::array::array_has_element_descriptors(arr));
        assert!(!try_own_store(arr.cast(), index, 44.0));
        assert_eq!(crate::array::js_array_get_f64(arr, 1), 33.0);
    }
}

#[test]
fn own_number_store_preserves_lanes_and_generalizes_a_function_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    unsafe {
        let obj = super::super::js_object_alloc(0, 4);
        super::super::alloc_plain::mark_object_plain_ordinary(obj);
        let atom = crate::string::canonical_key(b"shapeNumberStore");
        super::super::js_object_set_field_by_name(obj, atom, 1.0);
        assert!(try_own_store(obj, atom, -17.5));
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, atom),
            -17.5
        );
        assert!(try_own_store(obj, atom, f64::INFINITY));
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, atom),
            f64::INFINITY
        );
        assert!(try_own_store(
            obj,
            atom,
            f64::from_bits(crate::JSValue::int32(42).bits())
        ));
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, atom),
            42.0
        );

        extern "C" fn body(
            _c: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
        ) -> f64 {
            1.0
        }
        let info = crate::fn_info!(body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
        let function =
            crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64);
        let method = crate::string::canonical_key(b"shapeNumberMethod");
        super::super::js_object_set_field_by_name(obj, method, function);
        let record = shapes::object_shape_record(obj).unwrap();
        let slot = record.own_store_slot(method).unwrap().unwrap();
        assert_eq!(
            super::super::field_rep::slot_rep(record.rep(), slot as u32),
            super::super::field_rep::REP_SPECIAL
        );
        assert!(try_own_store(obj, method, 7.0));
        let record = shapes::object_shape_record(obj).unwrap();
        assert_eq!(
            super::super::field_rep::slot_rep(record.rep(), slot as u32),
            super::super::field_rep::REP_ANY
        );
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, method),
            7.0
        );
    }
}

#[test]
fn short_string_put_value_reuses_the_shape_transition() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    let atom = crate::string::canonical_key(b"sPos");
    let objects = [0; 2].map(|_| {
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        crate::value::js_nanbox_pointer(obj as i64)
    });
    let heap_key = crate::value::js_nanbox_string(atom as i64);
    crate::proxy::js_put_value_set(objects[0], heap_key, 1.0, objects[0], 0);
    let short_key = f64::from_bits(crate::JSValue::try_short_string(b"sPos").unwrap().bits());
    super::super::test_reset_transition_fast_hits();
    assert_eq!(
        crate::proxy::js_put_value_set(objects[1], short_key, 2.0, objects[1], 0),
        2.0
    );
    assert!(
        super::super::test_transition_fast_hits() > 0,
        "short and heap representations must take the same learned edge"
    );
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(
            (objects[1].to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader,
            atom
        ),
        2.0
    );
}

#[test]
fn function_own_store_uses_bag_attributes_and_outer_integrity() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let function = fresh_test_function();
    let owner = (function.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    let atom = crate::string::canonical_key(b"functionStorePos");
    let key = crate::value::js_nanbox_string(atom as i64);
    assert_eq!(unsafe { try_store(function, key, 1.0) }, Some(1.0));
    assert!(unsafe { try_own_store(owner, atom, 2.0) });
    let bag = unsafe { crate::closure::props::bag_of(owner as usize) };
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(bag, atom),
        2.0
    );
    unsafe {
        let header = (owner as *mut u8)
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        let saved = (*header)._reserved;
        (*header)._reserved |= crate::gc::OBJ_FLAG_FROZEN;
        assert!(!try_own_store(owner, atom, 3.0));
        (*header)._reserved = saved;
    }
    super::super::set_property_attrs(
        bag as usize,
        "functionStorePos".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    assert!(!unsafe { try_own_store(owner, atom, 4.0) });
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(bag, atom),
        2.0
    );
}

#[test]
fn shape_add_retires_its_slot_proof_once_the_overflow_exists() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    let target = crate::value::js_nanbox_pointer(obj as i64);
    for i in 0..8 {
        let name = format!("dictionaryAppendWitness{i}");
        let key = crate::string::canonical_key(name.as_bytes());
        crate::proxy::js_put_value_set(
            target,
            crate::value::js_nanbox_string(key as i64),
            i as f64,
            target,
            0,
        );
    }
    // Object model step 4: no append converts a receiver; its overflow is
    // established (here directly, as a delete or a computed-key add
    // would) and from then on no layout slot proof serves its stores.
    assert!(unsafe { super::super::overflow::establish_order(obj) });
    assert!(
        !unsafe {
            try_own_store(
                obj,
                crate::string::canonical_key(b"dictionaryAppendWitness0"),
                0.0,
            )
        },
        "an overflow receiver takes no layout slot proof"
    );
    for i in 0..8 {
        let name = format!("dictionaryAppendWitness{i}");
        let key = crate::string::canonical_key(name.as_bytes());
        assert_eq!(
            super::super::js_object_get_field_by_name_f64(obj, key),
            i as f64
        );
    }
    let readonly = crate::string::canonical_key(b"dictionaryAppendWitness0");
    super::super::set_property_attrs(
        obj as usize,
        "dictionaryAppendWitness0".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    let child = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(child) };
    let child_value = crate::value::js_nanbox_pointer(child as i64);
    super::super::js_object_set_prototype_of(child_value, target);
    let readonly_value = crate::value::js_nanbox_string(readonly as i64);
    crate::proxy::js_put_value_set(child_value, readonly_value, 99.0, child_value, 0);
    assert!(
        !super::super::obj_value_has_own_key(child_value, readonly_value),
        "dictionary prototype readonly keys must remain inherited"
    );
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(obj, readonly),
        0.0
    );
    let key = crate::value::js_nanbox_string(crate::string::canonical_key(
        b"dictionaryAbsentWitness",
    ) as i64);
    let mut may_move = false;
    assert_eq!(
        unsafe { try_store_with_refresh(target, key, 9.0, &mut may_move) },
        None
    );
    assert!(!may_move);
    // A value-bearing add on a fresh receiver: object model step 4 has no
    // conversion at publication (a layout key add is a transition), and
    // the pending value is written.
    let fresh = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(fresh) };
    let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
    let atom = crate::string::canonical_key(b"dictionaryNumberPublicationWitness");
    crate::proxy::js_put_value_set(
        fresh_value,
        crate::value::js_nanbox_string(atom as i64),
        37.0,
        fresh_value,
        0,
    );
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(fresh, atom),
        37.0
    );
}

#[test]
fn cold_shape_add_writes_the_proven_append_slot_in_a_wide_object() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    for i in 0..48 {
        let name = format!("coldWideAppend{i}");
        let key = crate::string::canonical_key(name.as_bytes());
        super::super::js_object_set_field_by_name(obj, key, i as f64);
    }
    let key = crate::string::canonical_key(b"coldWideAppendWitness");
    assert_eq!(
        unsafe {
            shapes::object_shape_record(obj)
                .unwrap()
                .logical_key_count()
        },
        48
    );
    assert_eq!(
        unsafe {
            try_store(
                crate::value::js_nanbox_pointer(obj as i64),
                crate::value::js_nanbox_string(key as i64),
                91.0,
            )
        },
        Some(91.0)
    );
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(obj, key),
        91.0
    );
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(
            obj,
            crate::string::canonical_key(b"coldWideAppend0")
        ),
        0.0
    );
}

#[test]
fn cold_shape_proof_mints_the_first_edge_without_complete_set() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    let key = crate::string::canonical_key(b"coldShapeMintWitness");
    let objects = [0; 2].map(|_| {
        let obj = super::super::js_object_alloc(0, 0);
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        obj
    });
    let predecessor = unsafe { shapes::object_shape_stamp(objects[0]) };
    assert!(super::super::transition_cache_lookup(predecessor, key).is_none());
    assert_eq!(
        unsafe {
            try_store(
                crate::value::js_nanbox_pointer(objects[0] as i64),
                crate::value::js_nanbox_string(key as i64),
                41.0,
            )
        },
        Some(41.0),
        "a cold, proven absent key must create its first edge"
    );
    assert_eq!(
        unsafe { super::super::field_rep_store::object_slot_rep(objects[0], 0) },
        super::super::field_rep::REP_F64,
        "a cold Number addition must preserve the value publication's lane"
    );
    super::super::test_reset_transition_fast_hits();
    let private_key = crate::string::js_string_from_bytes(
        b"coldShapeMintWitness".as_ptr(),
        b"coldShapeMintWitness".len() as u32,
    );
    assert_ne!(private_key.cast_const(), key);
    assert_eq!(
        unsafe { try_warm_transition(objects[1], private_key, 42.0, &mut None) },
        Some(42.0)
    );
    assert!(super::super::test_transition_fast_hits() > 0);
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(objects[1], key),
        42.0
    );
    assert_eq!(
        unsafe { super::super::field_rep_store::object_slot_rep(objects[1], 0) },
        super::super::field_rep::REP_F64,
        "the warm edge must retain the cold Number publication's lane"
    );
}

#[test]
fn small_store_binding_keeps_a_later_noncanonical_declaration() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let key = crate::string::canonical_key(b"pos");
    let other = crate::string::canonical_key(b"other");
    let duplicate = crate::string::js_string_from_bytes(b"pos".as_ptr(), 3);
    assert_ne!(key, duplicate);
    let mut keys = crate::array::js_array_alloc(4);
    for name in [key, other, duplicate, other] {
        keys = crate::array::js_array_push(keys, crate::JSValue::string_ptr(name));
    }
    let obj = super::super::alloc_plain::alloc_plain_record_with_keys(4, unsafe {
        super::super::ObjectKeys::owned(keys)
    });
    let record = unsafe { shapes::object_shape_record(obj).unwrap() };
    assert_eq!(unsafe { record.own_store_slot(key) }, Some(Some(2)));
    assert_eq!(unsafe { record.own_store_slot(other) }, Some(Some(3)));
}

#[test]
fn declined_store_refreshes_only_after_an_allocating_path() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let function = fresh_test_function();
    let length = crate::value::js_nanbox_string(crate::string::canonical_key(b"length") as i64);
    let mut may_move = false;
    assert_eq!(
        unsafe { try_store_with_refresh(function, length, 3.0, &mut may_move) },
        None
    );
    assert!(!may_move, "an intrinsic-key decline is a nonmoving leaf");
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    let key =
        crate::value::js_nanbox_string(crate::string::canonical_key(b"freshStoreProof") as i64);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    unsafe { try_store_with_refresh(receiver, key, 3.0, &mut may_move) };
    assert!(
        may_move,
        "learning a cold chain proof can allocate even on decline"
    );
}

#[test]
fn own_store_uses_complete_binding_after_read_position_proof_retires() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    let key = crate::string::canonical_key(b"pos");
    super::super::js_object_set_field_by_name(obj, key, 1.0);
    super::super::js_object_set_field_by_name(obj, crate::string::canonical_key(b"temp"), 0.0);
    // A semantic transition retires read guesses while retaining these
    // own data bindings, as a prototype/accessor replacement can do.
    unsafe { shapes::transition_object_shape_semantics(obj) };
    assert_eq!(shapes::test_position_bound_of(obj), Some(0));
    assert_eq!(
        unsafe {
            shapes::object_shape_record(obj)
                .unwrap()
                .own_store_slot(key)
        },
        Some(Some(0))
    );
    assert!(
        unsafe { try_own_store(obj, key, 7.0) },
        "an authoritative binding survives retirement of the read guess proof"
    );
    assert_eq!(super::super::js_object_get_field_by_name_f64(obj, key), 7.0);
}

#[test]
fn warm_array_transition_uses_the_projected_shape_and_retires_after_edit() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let first = crate::string::canonical_key(b"warmArrayPos");
    let key = crate::string::canonical_key(b"warmArrayEnd");
    let key_value = crate::value::js_nanbox_string(key as i64);
    let arrays = [0; 3].map(|_| crate::array::js_array_alloc(4));
    for arr in arrays {
        let receiver = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(
            unsafe { try_store(receiver, crate::value::js_nanbox_string(first as i64), 1.0) },
            Some(1.0)
        );
    }
    assert_eq!(
        unsafe {
            try_store(
                crate::value::js_nanbox_pointer(arrays[0] as i64),
                key_value,
                2.0,
            )
        },
        Some(2.0)
    );
    super::super::test_reset_transition_fast_hits();
    assert_eq!(
        unsafe { try_warm_array_transition(arrays[1], key, 7.0, &mut None) },
        Some(7.0),
        "the projected bag proof must serve another array"
    );
    assert!(super::super::test_transition_fast_hits() > 0);
    let proto = crate::array::array_prototype_addr() as *const crate::ArrayHeader;
    let bag = unsafe { crate::array::array_property_bag(proto) };
    super::super::js_object_set_field_by_name(bag, key, 9.0);
    super::super::set_property_attrs(
        bag as usize,
        "warmArrayEnd".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    let answer = unsafe { try_warm_array_transition(arrays[2], key, 8.0, &mut None) };
    super::super::js_object_delete_dynamic_value(
        crate::value::js_nanbox_pointer(bag as i64),
        key_value,
    );
    assert_eq!(answer, None, "a descriptor edit must retire the bag proof");
}

fn fresh_test_function() -> f64 {
    extern "C" fn body(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        1.0
    }
    static INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
        body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
    );
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(&INFO, 0) as i64)
}

#[test]
fn own_slot_plan_cannot_hide_a_later_declaration() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let key = crate::string::canonical_key(b"pos");
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, crate::JSValue::string_ptr(key as *mut _));
    let keys = crate::array::js_array_push(keys, crate::JSValue::string_ptr(key as *mut _));
    let obj = super::super::alloc_plain::alloc_plain_record_with_keys(2, unsafe {
        super::super::ObjectKeys::owned(keys)
    });
    let record = unsafe { shapes::object_shape_record(obj).unwrap() };
    let actual_keys = record.keys() as usize;
    super::super::prop_plan::read_plan_record(
        unsafe { (*obj).shape_word() },
        actual_keys,
        key as usize,
        0,
    );
    assert_eq!(
        super::super::prop_plan::read_plan_lookup(
            unsafe { (*obj).shape_word() },
            actual_keys,
            key as usize,
            2
        ),
        Some(0),
        "the stale prefix hint must be present before testing the guard"
    );
    assert_eq!(
        unsafe {
            shapes::object_shape_record(obj)
                .unwrap()
                .own_store_slot(key)
        },
        Some(Some(1)),
        "the earlier prefix plan must not hide the subclass declaration"
    );
}

#[test]
fn function_dictionary_adds_reuse_the_named_bag_transition() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let a = fresh_test_function();
    let b = fresh_test_function();
    let prototype = super::super::js_object_alloc(0, 0);
    for owner in [a, b] {
        super::super::js_object_set_prototype_of(
            owner,
            crate::value::js_nanbox_pointer(prototype as i64),
        );
    }
    let first = crate::value::js_nanbox_string(crate::string::canonical_key(b"rangeEnd") as i64);
    let second = crate::value::js_nanbox_string(crate::string::canonical_key(b"rangePos") as i64);
    for owner in [a, b] {
        assert_eq!(unsafe { try_store(owner, first, 1.0) }, Some(1.0));
    }
    super::super::test_reset_transition_fast_hits();
    for owner in [a, b] {
        assert_eq!(unsafe { try_store(owner, second, 2.0) }, Some(2.0));
    }
    assert!(
        super::super::test_transition_fast_hits() > 0,
        "a second named add must reuse the edge after the FunctionDictionary transition"
    );
    unsafe {
        let bags = [a, b].map(|v| {
            crate::closure::props::bag_of((v.to_bits() & crate::value::POINTER_MASK) as usize)
        });
        assert_eq!(
            shapes::object_shape_stamp(bags[0]),
            shapes::object_shape_stamp(bags[1])
        );
        assert_eq!(
            super::super::js_object_get_prototype_of(crate::value::js_nanbox_pointer(
                bags[0] as i64
            ))
            .to_bits(),
            crate::value::TAG_NULL
        );
    }
}

#[test]
fn function_dictionary_asks_its_actual_prototype_own_attributes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let a = fresh_test_function();
    let b = fresh_test_function();
    let first = crate::value::js_nanbox_string(crate::string::canonical_key(b"rangeEnd") as i64);
    let key = crate::string::canonical_key(b"rangePos");
    let key_value = crate::value::js_nanbox_string(key as i64);
    for owner in [a, b] {
        assert_eq!(unsafe { try_store(owner, first, 1.0) }, Some(1.0));
    }
    let proto = super::super::js_object_alloc(0, 0);
    super::super::js_object_set_field_by_name(proto, key, 9.0);
    super::super::set_property_attrs(
        proto as usize,
        "rangePos".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    super::super::js_object_set_prototype_of(a, crate::value::js_nanbox_pointer(proto as i64));
    let other_proto = super::super::js_object_alloc(0, 0);
    super::super::js_object_set_prototype_of(
        b,
        crate::value::js_nanbox_pointer(other_proto as i64),
    );
    unsafe {
        let id = |v: f64| {
            (*((v.to_bits() & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader))
                .shape_id
        };
        assert_eq!(
            id(a),
            id(b),
            "FunctionDictionary is shared across these distinct prototype chains"
        );
    }
    assert_eq!(
        unsafe { try_store(a, key_value, 2.0) },
        None,
        "the actual prototype's own readonly key must decline"
    );
    assert!(!super::super::obj_value_has_own_key(a, key_value));
    assert_eq!(
        unsafe { try_store(b, key_value, 2.0) },
        Some(2.0),
        "the same FunctionDictionary id with a different prototype must remain writable"
    );
}

#[test]
fn warm_transition_uses_the_shape_proof_before_put_value_roots() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = super::super::js_object_alloc(0, 0);
    let a = super::super::js_object_alloc(0, 0);
    let b = super::super::js_object_alloc(0, 0);
    let c = super::super::js_object_alloc(0, 0);
    for obj in [a, b, c] {
        unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
        super::super::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(obj as i64),
            crate::value::js_nanbox_pointer(proto as i64),
        );
    }
    let key = crate::string::canonical_key(b"rangePos");
    let a_value = crate::value::js_nanbox_pointer(a as i64);
    crate::proxy::js_put_value_set(
        a_value,
        crate::value::js_nanbox_string(key as i64),
        1.0,
        a_value,
        0,
    );
    super::super::test_reset_transition_fast_hits();
    assert_eq!(
        unsafe { try_warm_transition(b, key, 7.0, &mut None) },
        Some(7.0)
    );
    assert!(super::super::test_transition_fast_hits() > 0);
    super::super::js_object_set_field_by_name(proto, key, 9.0);
    super::super::set_property_attrs(
        proto as usize,
        "rangePos".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    assert_eq!(unsafe { try_warm_transition(c, key, 8.0, &mut None) }, None);
    let c_value = crate::value::js_nanbox_pointer(c as i64);
    assert!(!super::super::obj_value_has_own_key(
        c_value,
        crate::value::js_nanbox_string(key as i64)
    ));
}

#[test]
fn own_slot_lookup_answers_empty_and_wide_layouts() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    let key = crate::string::canonical_key(b"pos");
    let absent = crate::string::canonical_key(b"absent");
    unsafe {
        assert_eq!(
            shapes::object_shape_record(obj)
                .unwrap()
                .own_store_slot(key),
            Some(None)
        );
    }
    super::super::js_object_set_field_by_name(obj, key, 1.0);
    unsafe {
        let record = shapes::object_shape_record(obj).unwrap();
        assert_eq!(record.own_store_slot(key), Some(Some(0)));
        assert_eq!(record.own_store_slot(absent), Some(None));
    }
    for i in 0..super::super::KEYS_INDEX_THRESHOLD {
        let name = format!("field{i}");
        super::super::js_object_set_field_by_name(
            obj,
            crate::string::canonical_key(name.as_bytes()),
            1.0,
        );
    }
    unsafe {
        let record = shapes::object_shape_record(obj).unwrap();
        assert_eq!(record.own_store_slot(key), Some(Some(0)));
        assert_eq!(record.own_store_slot(absent), Some(None));
    }
    super::super::set_property_attrs(
        obj as usize,
        "pos".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    unsafe {
        assert_eq!(
            shapes::object_shape_record(obj)
                .unwrap()
                .own_store_slot(key),
            Some(Some(0))
        );
        assert!(!try_own_store(obj, key, 2.0));
    }
}

#[test]
fn own_spill_store_answers_from_the_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    for name in [b"pos".as_slice(), b"end", b"parent", b"transformFlags"] {
        super::super::js_object_set_field_by_name(obj, crate::string::canonical_key(name), 1.0);
    }
    let key = crate::string::canonical_key(b"transformFlags");
    assert!(
        unsafe { try_own_store(obj, key, 37.0) },
        "an existing spill slot must take the shape answer"
    );
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(obj, key),
        37.0
    );
    super::super::set_property_attrs(
        obj as usize,
        "transformFlags".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    assert!(!unsafe { try_own_store(obj, key, 38.0) });
    assert_eq!(
        super::super::js_object_get_field_by_name_f64(obj, key),
        37.0
    );
}

#[test]
fn function_named_add_reuses_the_bag_transition() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    extern "C" fn body(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        1.0
    }
    static INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
        body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
    );
    let key = crate::string::canonical_key(b"rangeEnd");
    let key_value = crate::value::js_nanbox_string(key as i64);
    let a = crate::closure::js_closure_alloc(&INFO, 0);
    let b = crate::closure::js_closure_alloc(&INFO, 0);
    assert_eq!(
        unsafe { try_store(crate::value::js_nanbox_pointer(a as i64), key_value, 1.0) },
        Some(1.0)
    );
    super::super::test_reset_transition_fast_hits();
    assert_eq!(
        unsafe { try_store(crate::value::js_nanbox_pointer(b as i64), key_value, 2.0) },
        Some(2.0)
    );
    assert!(
        super::super::test_transition_fast_hits() > 0,
        "the second function must take the learned edge"
    );
}

#[test]
fn generic_set_uses_indexed_prototype_attributes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = super::super::js_object_alloc(0, 0);
    let obj = super::super::js_object_alloc(0, 0);
    let key = crate::string::canonical_key(b"rangeEnd");
    super::super::js_object_set_field_by_name(proto, key, 9.0);
    super::super::set_property_attrs(
        proto as usize,
        "rangeEnd".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    let target = crate::value::js_nanbox_pointer(obj as i64);
    super::super::js_object_set_prototype_of(target, crate::value::js_nanbox_pointer(proto as i64));
    assert!(unsafe { super::super::proto_validity::object_is_marked_prototype(proto as usize) });
    let key_value = crate::value::js_nanbox_string(key as i64);
    let result = crate::proxy::reflect_ordinary_set_with_receiver(target, key_value, 3.0, target);
    assert_eq!(crate::value::js_is_truthy(result), 0);
    assert!(!super::super::obj_value_has_own_key(target, key_value));
}

#[test]
fn own_store_uses_the_shape_and_never_accepts_a_frozen_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let obj = super::super::js_object_alloc(0, 0);
    unsafe { super::super::alloc_plain::mark_object_plain_ordinary(obj) };
    let key = crate::string::canonical_key(b"pos");
    super::super::js_object_set_field_by_name(obj, key, 1.0);
    for name in [b"end".as_slice(), b"parent", b"transformFlags"] {
        super::super::js_object_set_field_by_name(obj, crate::string::canonical_key(name), 0.0);
    }
    assert!(
        unsafe { try_own_store(obj, key, 2.0) },
        "shape store must be live"
    );
    assert_eq!(super::super::js_object_get_field_by_name_f64(obj, key), 2.0);
    super::super::set_property_attrs(
        obj as usize,
        "pos".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    assert!(!unsafe { try_own_store(obj, key, 3.0) });
    assert_eq!(super::super::js_object_get_field_by_name_f64(obj, key), 2.0);
}

#[test]
fn array_named_chain_includes_object_prototype_after_warmup() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = super::super::js_get_global_this();
    let key = crate::string::canonical_key(b"arrayGrandparentRange");
    let key_value = crate::value::js_nanbox_string(key as i64);
    let arr = crate::array::js_array_alloc(4);
    let receiver = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(unsafe { try_store(receiver, key_value, 1.0) }, Some(1.0));
    let proto = crate::array::object_prototype_addr() as *mut ObjectHeader;
    super::super::js_object_set_field_by_name(proto, key, 9.0);
    super::super::set_property_attrs(
        proto as usize,
        "arrayGrandparentRange".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    let fresh = crate::array::js_array_alloc(4);
    let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
    let answer = unsafe { try_store(fresh_value, key_value, 2.0) };
    super::super::js_object_delete_dynamic_value(
        crate::value::js_nanbox_pointer(proto as i64),
        key_value,
    );
    assert_eq!(answer, None, "Object.prototype is part of an array's chain");
    assert!(!super::super::obj_value_has_own_key(fresh_value, key_value));
}

#[test]
fn array_chain_projection_declines_a_changed_prototype_after_warmup() {
    let _latch = crate::test_support::ArrayPrototypeLatchGuard::new();
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let key = crate::string::canonical_key(b"rangeEnd");
    let key_value = crate::value::js_nanbox_string(key as i64);
    let arr = crate::array::js_array_alloc(4);
    let receiver = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(unsafe { try_store(receiver, key_value, 1.0) }, Some(1.0));
    let prototype = super::super::js_object_get_prototype_of(receiver);
    let original = super::super::js_object_get_prototype_of(prototype);
    let readonly = super::super::js_object_alloc(0, 0);
    super::super::js_object_set_field_by_name(readonly, key, 9.0);
    super::super::set_property_attrs(
        readonly as usize,
        "rangeEnd".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    super::super::js_object_set_prototype_of(
        prototype,
        crate::value::js_nanbox_pointer(readonly as i64),
    );
    let fresh = crate::array::js_array_alloc(4);
    let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
    let answer = unsafe { try_store(fresh_value, key_value, 2.0) };
    super::super::js_object_set_prototype_of(prototype, original);
    assert_eq!(answer, None, "a custom array chain needs the complete Set");
}

#[test]
fn array_named_bags_share_store_shapes_and_read_back_inline_and_spill() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let key = crate::string::canonical_key(b"pos");
    let key_value = crate::value::js_nanbox_string(key as i64);
    let mut bags = Vec::new();
    for value in [11.0, 19.0] {
        let arr = crate::array::js_array_alloc_with_length(4);
        let receiver = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(
            unsafe { try_store(receiver, key_value, value) },
            Some(value),
            "named add must take the shaped path"
        );
        let bag = unsafe { crate::array::array_property_bag(arr) };
        assert!(!bag.is_null());
        assert_eq!(
            unsafe { shapes::object_shape_record(bag).unwrap().object_kind() },
            shapes::ShapeObjectKind::Ordinary
        );
        bags.push(bag);
    }
    assert_eq!(unsafe { shapes::object_shape_stamp(bags[0]) }, unsafe {
        shapes::object_shape_stamp(bags[1])
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    // Read shaped stores back through the ordinary array getter, for
    // both inline and spill storage.
    let arr = crate::array::js_array_alloc_with_length(4);
    let receiver = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(unsafe { try_store(receiver, key_value, 23.0) }, Some(23.0));
    let arr_root = scope.root_raw_mut_ptr(arr);
    assert_eq!(
        crate::object::field_get_set::get_field_miss(
            arr_root.get_raw_mut_ptr::<crate::ArrayHeader>().cast(),
            key
        ),
        23.0
    );
    for name in [b"end".as_slice(), b"hasTrailingComma", b"transformFlags"] {
        let key = crate::string::canonical_key(name);
        let receiver = crate::value::js_nanbox_pointer(
            arr_root.get_raw_mut_ptr::<crate::ArrayHeader>() as i64,
        );
        assert_eq!(
            unsafe { try_store(receiver, crate::value::js_nanbox_string(key as i64), 29.0) },
            Some(29.0)
        );
    }
    let spill_key = crate::string::canonical_key(b"transformFlags");
    assert_eq!(
        crate::object::field_get_set::get_field_miss(
            arr_root.get_raw_mut_ptr::<crate::ArrayHeader>().cast(),
            spill_key
        ),
        29.0
    );
}
#[test]
fn chain_fact_is_shape_owned_and_rejected_after_mutation() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proto = super::super::js_object_alloc(0, 0);
    let a = super::super::js_object_alloc(0, 0);
    let b = super::super::js_object_alloc(0, 0);
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    for obj in [a, b] {
        super::super::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(obj as i64),
            proto_value,
        );
    }
    let shape = unsafe { shapes::object_shape_record(a) }.unwrap();
    let key = crate::string::js_string_from_bytes(b"rangePos".as_ptr(), 8);
    let key_value = crate::value::js_nanbox_string(key as i64);
    let a_value = crate::value::js_nanbox_pointer(a as i64);
    crate::proxy::js_put_value_set(a_value, key_value, 1.0, a_value, 0);
    assert!(super::super::obj_value_has_own_key(a_value, key_value));
    let _ = shape;
    super::super::set_property_attrs(
        proto as usize,
        "rangePos".into(),
        super::super::PropertyAttrs::new(false, true, true),
    );
    let b_value = crate::value::js_nanbox_pointer(b as i64);
    crate::proxy::js_put_value_set(b_value, key_value, 2.0, b_value, 0);
    assert!(!super::super::obj_value_has_own_key(b_value, key_value));
}
