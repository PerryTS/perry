use super::*;

static CLASS_FIELD_SETTER_CALLS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static CLASS_FIELD_SETTER_VALUE_BITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

extern "C" fn test_class_field_setter(_this: f64, value: f64) -> f64 {
    CLASS_FIELD_SETTER_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    CLASS_FIELD_SETTER_VALUE_BITS.store(value.to_bits(), std::sync::atomic::Ordering::SeqCst);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_direct_method(_this: f64, value: f64) -> f64 {
    value
}

extern "C" fn test_direct_closure(_closure: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    arg
}

fn test_direct_closure_ptr() -> *const u8 {
    test_direct_closure as *const () as *const u8
}

fn test_direct_method_ptr() -> *const u8 {
    test_direct_method as *const () as *const u8
}

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

fn class_instance(
    class_id: u32,
    key_name: &'static [u8],
) -> (
    *mut crate::object::ObjectHeader,
    *mut crate::array::ArrayHeader,
    *const crate::StringHeader,
    f64,
) {
    let mut packed = Vec::with_capacity(key_name.len() + 1);
    packed.extend_from_slice(key_name);
    packed.push(0);
    let obj = crate::object::js_object_alloc_class_with_keys(
        class_id,
        0,
        1,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let key = crate::string::js_string_from_bytes(key_name.as_ptr(), key_name.len() as u32);
    let keys = unsafe { (*obj).keys_array };
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    (obj, keys, key, receiver)
}

unsafe fn register_test_method(class_id: u32, name: &'static [u8]) {
    crate::object::js_register_class_method(
        class_id as i64,
        name.as_ptr(),
        name.len() as i64,
        test_direct_method as *const () as usize as i64,
        1,
    );
}

fn plain_object_with_key(
    key_name: &'static [u8],
) -> (*mut crate::object::ObjectHeader, *const crate::StringHeader) {
    let obj = crate::object::js_object_alloc(0, 0);
    let key = crate::string::js_string_from_bytes(key_name.as_ptr(), key_name.len() as u32);
    (obj, key)
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
        TypedFeedbackSiteKind::ClosureCall,
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
fn typed_feedback_class_field_set_guard_fails_for_frozen_object() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(31, TypedFeedbackSiteKind::PropertySet, "obj.x=");

    let class_id = 0x7EED_0031;
    let (obj, keys, key, receiver) = class_instance(class_id, b"x");
    crate::object::js_object_set_field(obj, 0, crate::JSValue::from_bits(1.0f64.to_bits()));
    crate::object::js_object_freeze(receiver);

    let guard = js_typed_feedback_class_field_set_guard(31, receiver, class_id, keys, key, 0, 2.0);
    assert_eq!(guard, 0);
    assert_eq!(
        crate::object::js_object_get_field(obj, 0).bits(),
        1.0f64.to_bits()
    );

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 0);
}

#[test]
fn typed_feedback_class_field_set_guard_falls_back_for_class_setter() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    CLASS_FIELD_SETTER_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
    CLASS_FIELD_SETTER_VALUE_BITS.store(0, std::sync::atomic::Ordering::SeqCst);
    register(32, TypedFeedbackSiteKind::PropertySet, "obj.x=");

    let class_id = 0x7EED_0032;
    let (obj, keys, key, receiver) = class_instance(class_id, b"x");
    crate::object::js_object_set_field(obj, 0, crate::JSValue::from_bits(1.0f64.to_bits()));
    unsafe {
        crate::object::js_register_class_setter(
            class_id as i64,
            b"x".as_ptr(),
            1,
            test_class_field_setter as *const () as usize as i64,
        );
    }

    let guard = js_typed_feedback_class_field_set_guard(32, receiver, class_id, keys, key, 0, 7.0);
    assert_eq!(guard, 0);
    js_typed_feedback_record_fallback_call(32);
    crate::object::js_object_set_field_by_name(obj, key, 7.0);

    assert_eq!(
        CLASS_FIELD_SETTER_CALLS.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        CLASS_FIELD_SETTER_VALUE_BITS.load(std::sync::atomic::Ordering::SeqCst),
        7.0f64.to_bits()
    );
    assert_eq!(
        crate::object::js_object_get_field(obj, 0).bits(),
        1.0f64.to_bits()
    );

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_class_field_get_guard_falls_back_after_shape_transition() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(39, TypedFeedbackSiteKind::PropertyGet, "obj.x");

    let class_id = 0x7EED_0039;
    let (obj, expected_keys, key_x, receiver) = class_instance(class_id, b"x");
    crate::object::js_object_set_field(obj, 0, crate::JSValue::from_bits(5.0f64.to_bits()));
    let first =
        js_typed_feedback_class_field_get_guard(39, receiver, class_id, expected_keys, key_x, 0);
    assert_eq!(first, 1);

    let key_y = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, key_y, 10.0);
    assert_ne!(unsafe { (*obj).keys_array }, expected_keys);

    let second =
        js_typed_feedback_class_field_get_guard(39, receiver, class_id, expected_keys, key_x, 0);
    assert_eq!(second, 0);
    js_typed_feedback_record_fallback_call(39);
    let stored = crate::object::js_object_get_field_by_name_f64(obj, key_x);
    assert_eq!(stored.to_bits(), 5.0f64.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_object_set_fast_hits_learned_dynamic_key_transition() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(34, TypedFeedbackSiteKind::PropertySet, "obj[dyn]=");

    let (first_obj, key) = plain_object_with_key(b"dyn_fast_key_34");
    js_typed_feedback_object_set_field_by_name_fast(34, first_obj, key, 11.0);
    let first_site = &typed_feedback_snapshot().sites[0];
    assert_eq!(first_site.fallback_calls, 1);

    let second_obj = crate::object::js_object_alloc(0, 0);
    js_typed_feedback_object_set_field_by_name_fast(34, second_obj, key, 12.0);
    let stored = crate::object::js_object_get_field_by_name_f64(second_obj, key);
    assert_eq!(stored.to_bits(), 12.0f64.to_bits());

    let site = &typed_feedback_snapshot().sites[0];
    if crate::object::descriptors_in_use() {
        assert_eq!(site.fallback_calls, 2);
    } else {
        assert_eq!(site.fallback_calls, 1);
        assert!(site.guard_passes >= 1);
    }
}

#[test]
fn typed_feedback_object_set_fast_falls_back_for_uncached_dynamic_key() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(35, TypedFeedbackSiteKind::PropertySet, "obj[dyn_miss]=");

    let (obj, key) = plain_object_with_key(b"dyn_uncached_key_35");
    js_typed_feedback_object_set_field_by_name_fast(35, obj, key, 21.0);

    let stored = crate::object::js_object_get_field_by_name_f64(obj, key);
    assert_eq!(stored.to_bits(), 21.0f64.to_bits());
    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_method_direct_guard_passes_for_exact_registered_method() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(61, TypedFeedbackSiteKind::MethodCall, "obj.m()");

    let class_id = 0x7EED_0061;
    let (_, keys, _, receiver) = class_instance(class_id, b"x");
    unsafe { register_test_method(class_id, b"m") };

    let guard = unsafe {
        js_typed_feedback_method_direct_call_guard(
            61,
            receiver,
            class_id,
            keys,
            b"m".as_ptr() as *const i8,
            1,
            test_direct_method_ptr(),
        )
    };
    assert_eq!(guard, 1);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 0);
    assert_eq!(site.fallback_calls, 0);
    assert_eq!(site.state, "monomorphic");
}

#[test]
fn typed_feedback_method_direct_guard_fails_for_own_method_replacement() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(62, TypedFeedbackSiteKind::MethodCall, "obj.m()");

    let class_id = 0x7EED_0062;
    let (obj, keys, _, receiver) = class_instance(class_id, b"x");
    unsafe { register_test_method(class_id, b"m") };
    let key_m = crate::string::js_string_from_bytes(b"m".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, key_m, 123.0);

    let guard = unsafe {
        js_typed_feedback_method_direct_call_guard(
            62,
            receiver,
            class_id,
            keys,
            b"m".as_ptr() as *const i8,
            1,
            test_direct_method_ptr(),
        )
    };
    assert_eq!(guard, 0);
    js_typed_feedback_record_fallback_call(62);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_method_direct_guard_fails_for_prototype_method_registration() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(63, TypedFeedbackSiteKind::MethodCall, "obj.m()");

    let class_id = 0x7EED_0063;
    let (_, keys, _, receiver) = class_instance(class_id, b"x");
    unsafe {
        register_test_method(class_id, b"m");
        crate::object::js_register_prototype_method(
            class_id,
            b"m".as_ptr(),
            1,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }

    let guard = unsafe {
        js_typed_feedback_method_direct_call_guard(
            63,
            receiver,
            class_id,
            keys,
            b"m".as_ptr() as *const i8,
            1,
            test_direct_method_ptr(),
        )
    };
    assert_eq!(guard, 0);
    js_typed_feedback_record_fallback_call(63);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_method_direct_guard_fails_for_native_receiver() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(64, TypedFeedbackSiteKind::MethodCall, "native.m()");

    let native = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 0);
    let receiver = crate::value::js_nanbox_pointer(native as i64);

    let guard = unsafe {
        js_typed_feedback_method_direct_call_guard(
            64,
            receiver,
            crate::object::NATIVE_MODULE_CLASS_ID,
            std::ptr::null(),
            b"m".as_ptr() as *const i8,
            1,
            test_direct_method_ptr(),
        )
    };
    assert_eq!(guard, 0);
    js_typed_feedback_record_fallback_call(64);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 1);
}

#[test]
fn typed_feedback_method_direct_guard_fails_after_megamorphic_site() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(65, TypedFeedbackSiteKind::MethodCall, "obj.m()");
    for i in 0..=POLYMORPHIC_CAP {
        observe(
            65,
            TypedFeedbackSiteKind::MethodCall,
            Observation {
                source: ObservationSource::Method,
                object_addr: 0,
                shape_addr: 0x1000 + i,
                key_hash: i as u64,
                class_id: i as u32 + 1,
                heap_type: crate::gc::GC_TYPE_OBJECT as u16,
                aux: i as u64,
                value_tag: STABLE_VALUE_POINTER,
            },
        );
    }

    let class_id = 0x7EED_0065;
    let (_, keys, _, receiver) = class_instance(class_id, b"x");
    unsafe { register_test_method(class_id, b"m") };
    let guard = unsafe {
        js_typed_feedback_method_direct_call_guard(
            65,
            receiver,
            class_id,
            keys,
            b"m".as_ptr() as *const i8,
            1,
            test_direct_method_ptr(),
        )
    };
    assert_eq!(guard, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.state, "megamorphic");
    assert_eq!(site.guard_failures, 1);
}

#[test]
fn typed_feedback_closure_direct_guard_passes_and_rejects_bound_sentinel() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(66, TypedFeedbackSiteKind::ClosureCall, "cb()");

    let fn_ptr = test_direct_closure_ptr();
    crate::closure::js_register_closure_arity(fn_ptr, 1);
    let closure = crate::closure::js_closure_alloc_singleton(fn_ptr);
    let closure_value = crate::value::js_nanbox_pointer(closure as i64);
    let pass = js_typed_feedback_closure_direct_call_guard(66, closure_value, fn_ptr, 1, 1);
    assert_eq!(pass, 1);

    let bound = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 0);
    let bound_value = crate::value::js_nanbox_pointer(bound as i64);
    let fail = js_typed_feedback_closure_direct_call_guard(66, bound_value, fn_ptr, 1, 1);
    assert_eq!(fail, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
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
