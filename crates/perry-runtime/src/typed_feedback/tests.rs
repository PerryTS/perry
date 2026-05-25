use super::*;

fn register(site_id: u64, kind: TypedFeedbackSiteKind, op: &'static str) {
    js_typed_feedback_register_site(
        site_id,
        kind as u32,
        b"typed_feedback_test.ts".as_ptr(),
        "typed_feedback_test.ts".len(),
        b"probe".as_ptr(),
        "probe".len(),
        op.as_ptr(),
        op.len(),
        op.as_ptr(),
        op.len(),
        b"test_guard".as_ptr(),
        "test_guard".len(),
        b"test_fallback".as_ptr(),
        "test_fallback".len(),
    );
}

#[test]
fn typed_feedback_registers_source_attribution() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(1, TypedFeedbackSiteKind::PropertyGet, "obj.x");
    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.total_sites, 1);
    assert_eq!(snapshot.by_kind["property_get"], 1);
    assert_eq!(snapshot.by_state["uninitialized"], 1);
    assert_eq!(snapshot.sites[0].module, "typed_feedback_test.ts");
    assert_eq!(snapshot.sites[0].function, "probe");
    assert_eq!(snapshot.sites[0].operation, "obj.x");
}

#[test]
fn typed_feedback_state_transitions_to_megamorphic() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(2, TypedFeedbackSiteKind::HelperReturn, "helper");
    for i in 0..POLYMORPHIC_CAP {
        observe(
            2,
            TypedFeedbackSiteKind::HelperReturn,
            Observation {
                source: ObservationSource::HelperReturn,
                object_addr: 0,
                shape_addr: 0,
                key_hash: 0,
                class_id: 0,
                heap_type: 0,
                aux: i as u64,
                value_tag: i as u16,
            },
        );
    }
    assert_eq!(typed_feedback_snapshot().sites[0].state, "polymorphic");
    observe(
        2,
        TypedFeedbackSiteKind::HelperReturn,
        Observation {
            source: ObservationSource::HelperReturn,
            object_addr: 0,
            shape_addr: 0,
            key_hash: 0,
            class_id: 0,
            heap_type: 0,
            aux: 99,
            value_tag: 99,
        },
    );
    assert_eq!(typed_feedback_snapshot().sites[0].state, "megamorphic");
}

#[test]
fn typed_feedback_invalidation_counters_are_site_attributed() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(3, TypedFeedbackSiteKind::MethodCall, "m");
    observe(
        3,
        TypedFeedbackSiteKind::MethodCall,
        Observation {
            source: ObservationSource::Method,
            object_addr: 0,
            shape_addr: 0,
            key_hash: 1,
            class_id: 42,
            heap_type: 0,
            aux: 1,
            value_tag: 0,
        },
    );
    invalidate_method_change(42);
    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.method_invalidations, 1);
    assert_eq!(snapshot.sites[0].method_invalidations, 1);
}

#[test]
fn typed_feedback_property_and_method_keys_ignore_receiver_identity() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(5, TypedFeedbackSiteKind::PropertyGet, "obj.x");
    register(6, TypedFeedbackSiteKind::MethodCall, "obj.m()");
    for object_addr in [0x1000_0000usize, 0x2000_0000usize] {
        observe(
            5,
            TypedFeedbackSiteKind::PropertyGet,
            Observation {
                source: ObservationSource::Property,
                object_addr,
                shape_addr: 0xCAFE,
                key_hash: 0xA11C_E,
                class_id: 7,
                heap_type: crate::gc::GC_TYPE_OBJECT as u16,
                aux: 0,
                value_tag: 0,
            },
        );
        observe(
            6,
            TypedFeedbackSiteKind::MethodCall,
            Observation {
                source: ObservationSource::Method,
                object_addr,
                shape_addr: 0xCAFE,
                key_hash: 0xBEE,
                class_id: 7,
                heap_type: crate::gc::GC_TYPE_OBJECT as u16,
                aux: 0,
                value_tag: value_tag(POINTER_TAG),
            },
        );
    }

    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.by_state["monomorphic"], 2);
    assert!(snapshot
        .sites
        .iter()
        .all(|site| site.observed_count == 2 && site.observation_count == 1));
}

#[test]
fn typed_feedback_array_keys_use_element_facts_not_sample_identity() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(7, TypedFeedbackSiteKind::ArrayElement, "arr[i]");

    let values1 = [1.0, 1.5];
    let values2 = [2.0, 2.5, 3.0, 3.5];
    let arr1 = crate::array::js_array_from_f64(values1.as_ptr(), values1.len() as u32);
    let arr2 = crate::array::js_array_from_f64(values2.as_ptr(), values2.len() as u32);

    js_typed_feedback_observe_array_element(7, arr1, 0);
    js_typed_feedback_observe_array_element(7, arr2, 3);

    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.sites[0].state, "monomorphic");
    assert_eq!(snapshot.sites[0].observed_count, 2);
    assert_eq!(snapshot.sites[0].observation_count, 1);

    let reg = registry();
    let observation = reg.sites.get(&7).unwrap().observations[0];
    assert_eq!(observation.object_addr, 0);
    assert_eq!(observation.heap_type, crate::gc::GC_TYPE_ARRAY as u16);
    assert_eq!(observation.value_tag, STABLE_VALUE_NUMBER);
}

#[test]
fn typed_feedback_helper_return_keys_use_shape_facts_not_sample_identity() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(8, TypedFeedbackSiteKind::HelperReturn, "helper()");

    let packed = b"x\0";
    let obj1 = crate::object::js_object_alloc_with_shape(
        0x7EED_0008,
        1,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let obj2 = crate::object::js_object_alloc_with_shape(
        0x7EED_0008,
        1,
        packed.as_ptr(),
        packed.len() as u32,
    );

    js_typed_feedback_observe_helper_return(8, crate::value::js_nanbox_pointer(obj1 as i64));
    js_typed_feedback_observe_helper_return(8, crate::value::js_nanbox_pointer(obj2 as i64));

    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.sites[0].state, "monomorphic");
    assert_eq!(snapshot.sites[0].observed_count, 2);
    assert_eq!(snapshot.sites[0].observation_count, 1);

    let reg = registry();
    let observation = reg.sites.get(&8).unwrap().observations[0];
    assert_eq!(observation.object_addr, 0);
    assert_eq!(observation.heap_type, crate::gc::GC_TYPE_OBJECT as u16);
    assert_ne!(observation.shape_addr, 0);
}

#[test]
fn typed_feedback_tracks_all_site_categories() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    let kinds = [
        TypedFeedbackSiteKind::PropertyGet,
        TypedFeedbackSiteKind::PropertySet,
        TypedFeedbackSiteKind::MethodCall,
        TypedFeedbackSiteKind::ArrayElement,
        TypedFeedbackSiteKind::NumericFieldWrite,
        TypedFeedbackSiteKind::HelperReturn,
    ];
    for (idx, kind) in kinds.iter().copied().enumerate() {
        register(10 + idx as u64, kind, kind.as_str());
    }

    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.total_sites, kinds.len());
    for kind in kinds {
        assert_eq!(snapshot.by_kind[kind.as_str()], 1);
    }
}

#[test]
fn typed_feedback_unboxed_numeric_write_falls_back_for_string_values() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(21, TypedFeedbackSiteKind::NumericFieldWrite, "obj.x=");

    let packed = b"x\0";
    let obj = crate::object::js_object_alloc_with_shape(
        0x7EED_0021,
        1,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);

    js_typed_feedback_object_set_unboxed_f64_field(21, obj, 0, key, 1.0);
    let payload = crate::string::js_string_from_bytes(b"fallback".as_ptr(), 8);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    js_typed_feedback_object_set_unboxed_f64_field(21, obj, 0, key, payload_value);

    let stored = crate::object::js_object_get_field_by_name_f64(obj, key);
    assert_eq!(stored.to_bits(), payload_value.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_helper_return_guard_failure_returns_original_value() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(22, TypedFeedbackSiteKind::HelperReturn, "helper()");

    let first = js_typed_feedback_observe_helper_return(22, 42.0);
    assert_eq!(first.to_bits(), 42.0f64.to_bits());

    let payload = crate::string::js_string_from_bytes(b"shape-change".as_ptr(), 12);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    let second = js_typed_feedback_observe_helper_return(22, payload_value);
    assert_eq!(second.to_bits(), payload_value.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_array_guard_failure_matches_jsvalue_fallback() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(23, TypedFeedbackSiteKind::ArrayElement, "arr[i]");

    let values = [1.0, 2.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let expected = crate::array::js_array_get_f64(arr, 5);
    let actual = js_typed_feedback_array_get_f64(23, arr, 5);
    assert_eq!(actual.to_bits(), expected.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_array_get_guard_failure_uses_jsvalue_object_fallback() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(25, TypedFeedbackSiteKind::ArrayElement, "arr[i]");

    let obj = crate::object::js_object_alloc(0, 0);
    let obj_box = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
    let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, key, 42.0);

    // Models an array-typed compiled read whose receiver was replaced by
    // a dynamic object at a JS boundary. The guard must reject it before
    // codegen reads ArrayHeader fields; fallback then performs obj["0"].
    let guard = js_typed_feedback_plain_array_index_get_guard(25, obj_box, 0.0, 0, 1);
    assert_eq!(guard, 0);

    let actual = js_typed_feedback_array_index_get_fallback_boxed(25, obj_box, 0.0);
    assert_eq!(actual.to_bits(), 42.0f64.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_non_bounded_array_set_guard_failure_uses_jsvalue_object_fallback() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(24, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");

    let obj = crate::object::js_object_alloc(0, 0);
    let obj_box = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());

    // Models an array-typed compiled local slot that receives an object
    // from a dynamic boundary: the non-bounded set guard must fail before
    // codegen can read ArrayHeader fields or raw-store an element.
    let guard = js_typed_feedback_plain_array_index_set_guard(24, obj_box, 0, 99.0, 0);
    assert_eq!(guard, 0);

    let returned = js_typed_feedback_array_index_set_fallback_boxed(24, obj_box, 0, 99.0);
    assert_eq!(returned.to_bits(), obj_box.to_bits());

    let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    let stored = crate::object::js_object_get_field_by_name_f64(obj, key);
    assert_eq!(stored.to_bits(), 99.0f64.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_trace_json_reports_counts() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(4, TypedFeedbackSiteKind::ArrayElement, "arr[i]");
    js_typed_feedback_record_guard_pass(4);
    js_typed_feedback_record_guard_fail(4);
    js_typed_feedback_record_fallback_call(4);
    let json = typed_feedback_trace_json();
    assert_eq!(json["total_sites"].as_u64(), Some(1));
    assert_eq!(json["by_kind"]["array_element"].as_u64(), Some(1));
    assert_eq!(json["by_state"]["uninitialized"].as_u64(), Some(1));
    assert_eq!(json["guards"]["passes"].as_u64(), Some(1));
    assert_eq!(json["guards"]["failures"].as_u64(), Some(1));
    assert_eq!(json["guards"]["fallback_calls"].as_u64(), Some(1));
    assert_eq!(
        json["guards"]["by_guard"]["test_guard"]["fallback_calls"].as_u64(),
        Some(1)
    );
    assert_eq!(json["sites"][0]["guard_name"].as_str(), Some("test_guard"));
    assert_eq!(
        json["sites"][0]["fallback_name"].as_str(),
        Some("test_fallback")
    );
    assert_eq!(json["sites"][0]["guard_passes"].as_u64(), Some(1));
    assert_eq!(json["sites"][0]["guard_failures"].as_u64(), Some(1));
    assert_eq!(json["sites"][0]["fallback_calls"].as_u64(), Some(1));
}
