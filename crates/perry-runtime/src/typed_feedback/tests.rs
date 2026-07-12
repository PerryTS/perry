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

fn assert_undefined(value: f64) {
    assert_eq!(value.to_bits(), crate::value::TAG_UNDEFINED);
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
        0,
        0,
    );
}

fn plain_object_with_key(
    key_name: &'static [u8],
) -> (*mut crate::object::ObjectHeader, *const crate::StringHeader) {
    let obj = crate::object::js_object_alloc(0, 0);
    let key = crate::string::js_string_from_bytes(key_name.as_ptr(), key_name.len() as u32);
    (obj, key)
}

struct EnvGuard {
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(value: Option<&str>) -> Self {
        let previous = std::env::var_os("PERRY_TYPED_FEEDBACK_TRACE");
        match value {
            Some(value) => std::env::set_var("PERRY_TYPED_FEEDBACK_TRACE", value),
            None => std::env::remove_var("PERRY_TYPED_FEEDBACK_TRACE"),
        }
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var("PERRY_TYPED_FEEDBACK_TRACE", previous);
        } else {
            std::env::remove_var("PERRY_TYPED_FEEDBACK_TRACE");
        }
    }
}

struct CurrentDirGuard {
    previous: std::path::PathBuf,
}

impl CurrentDirGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(path).expect("set current dir");
        Self { previous }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

fn unique_temp_dir(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let unique = format!(
        "perry-typed-feedback-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    path.push(unique);
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
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
    let guard = js_typed_feedback_plain_array_index_get_guard(25, obj_box, 0, 1);
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

    let returned = js_typed_feedback_array_index_set_fallback_boxed(24, obj_box, 0.0, 99.0);
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
fn typed_feedback_array_set_guards_reject_frozen_arrays() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(70, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");
    register(71, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");

    let values = [1.0, 2.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);
    crate::object::js_object_freeze(arr_box);

    assert_eq!(
        js_typed_feedback_plain_array_index_set_guard(70, arr_box, 0, 99.0, 1),
        0
    );
    assert_eq!(
        js_typed_feedback_numeric_array_index_set_guard(71, arr_box, 0, 99.0, 1),
        0
    );

    let returned = js_typed_feedback_array_index_set_fallback_boxed(70, arr_box, 0.0, 99.0);
    assert_eq!(returned.to_bits(), arr_box.to_bits());
    assert_eq!(
        crate::array::js_array_get_f64(arr, 0).to_bits(),
        1.0f64.to_bits()
    );

    let snapshot = typed_feedback_snapshot();
    assert_eq!(snapshot.sites[0].guard_failures, 1);
    assert_eq!(snapshot.sites[1].guard_failures, 1);
}

#[test]
fn typed_feedback_array_set_boxed_fallback_preserves_original_index_value() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(72, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");

    let obj = crate::object::js_object_alloc(0, 0);
    let obj_box = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
    let key = crate::string::js_string_from_bytes(b"foo".as_ptr(), 3);
    let key_value = crate::value::js_nanbox_string(key as i64);

    let returned = js_typed_feedback_array_index_set_fallback_boxed(72, obj_box, key_value, 77.0);
    assert_eq!(returned.to_bits(), obj_box.to_bits());
    assert_eq!(
        crate::object::js_object_get_field_by_name_f64(obj, key).to_bits(),
        77.0f64.to_bits()
    );

    let zero_key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    assert_eq!(
        crate::object::js_object_get_field_by_name_f64(obj, zero_key).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn typed_feedback_boxed_fallback_preserves_fractional_keys_for_array_like_receivers() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(73, TypedFeedbackSiteKind::ArrayElement, "arr[i]");

    let buf = crate::buffer::js_buffer_alloc(3, 0);
    crate::buffer::js_buffer_set(buf, 1, 22);
    let buf_box = crate::value::js_nanbox_pointer(buf as i64);
    assert_eq!(
        js_typed_feedback_array_index_get_fallback_boxed(73, buf_box, 1.0),
        22.0
    );
    assert_undefined(js_typed_feedback_array_index_get_fallback_boxed(
        73, buf_box, 1.5,
    ));

    let ta = crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT8 as i32, 3);
    crate::typedarray::js_typed_array_set(ta, 1, 33.0);
    let ta_box = crate::value::js_nanbox_pointer(ta as i64);
    assert_eq!(
        js_typed_feedback_array_index_get_fallback_boxed(73, ta_box, 1.0),
        33.0
    );
    assert_undefined(js_typed_feedback_array_index_get_fallback_boxed(
        73, ta_box, 1.5,
    ));

    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 10.0);
    crate::set::js_set_add(set, 20.0);
    let set_box = crate::value::js_nanbox_pointer(set as i64);
    assert_eq!(
        js_typed_feedback_array_index_get_fallback_boxed(73, set_box, 1.0),
        20.0
    );
    assert_undefined(js_typed_feedback_array_index_get_fallback_boxed(
        73, set_box, 1.5,
    ));

    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 10.0, 100.0);
    crate::map::js_map_set(map, 20.0, 200.0);
    let map_box = crate::value::js_nanbox_pointer(map as i64);
    assert_eq!(
        js_typed_feedback_array_index_get_fallback_boxed(73, map_box, 1.0),
        20.0
    );
    assert_undefined(js_typed_feedback_array_index_get_fallback_boxed(
        73, map_box, 1.5,
    ));

    let site = typed_feedback_snapshot()
        .sites
        .into_iter()
        .find(|site| site.site_id == 73)
        .expect("site 73");
    assert_eq!(site.fallback_calls, 8);
}

#[test]
fn typed_feedback_boxed_set_fallback_does_not_truncate_fractional_array_like_keys() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(74, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");

    let buf = crate::buffer::js_buffer_alloc(3, 0);
    crate::buffer::js_buffer_set(buf, 1, 22);
    let buf_box = crate::value::js_nanbox_pointer(buf as i64);
    js_typed_feedback_array_index_set_fallback_boxed(74, buf_box, 1.5, 99.0);
    assert_eq!(crate::buffer::js_buffer_get(buf, 1), 22);
    js_typed_feedback_array_index_set_fallback_boxed(74, buf_box, 1.0, 99.0);
    assert_eq!(crate::buffer::js_buffer_get(buf, 1), 99);

    let ta = crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT8 as i32, 3);
    crate::typedarray::js_typed_array_set(ta, 1, 33.0);
    let ta_box = crate::value::js_nanbox_pointer(ta as i64);
    js_typed_feedback_array_index_set_fallback_boxed(74, ta_box, 1.5, 88.0);
    assert_eq!(crate::typedarray::js_typed_array_get(ta, 1), 33.0);
    js_typed_feedback_array_index_set_fallback_boxed(74, ta_box, 1.0, 88.0);
    assert_eq!(crate::typedarray::js_typed_array_get(ta, 1), 88.0);

    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 10.0);
    crate::set::js_set_add(set, 20.0);
    let set_box = crate::value::js_nanbox_pointer(set as i64);
    js_typed_feedback_array_index_set_fallback_boxed(74, set_box, 1.5, 77.0);
    assert_eq!(crate::set::js_set_size(set), 2);
    assert_eq!(crate::set::js_set_value_at(set, 1), 20.0);

    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 10.0, 100.0);
    crate::map::js_map_set(map, 20.0, 200.0);
    let map_box = crate::value::js_nanbox_pointer(map as i64);
    js_typed_feedback_array_index_set_fallback_boxed(74, map_box, 1.5, 66.0);
    assert_eq!(crate::map::js_map_size(map), 2);
    assert_eq!(crate::map::js_map_entry_key_at(map, 1), 20.0);

    let map_handle = map_box.to_bits() as i64;
    js_typed_feedback_object_set_index_polymorphic(74, map_handle, 1.5, 55.0);
    assert_eq!(crate::map::js_map_size(map), 2);
    assert_eq!(crate::map::js_map_entry_key_at(map, 1), 20.0);

    let set_handle = set_box.to_bits() as i64;
    js_typed_feedback_object_set_index_polymorphic(74, set_handle, 1.5, 44.0);
    assert_eq!(crate::set::js_set_size(set), 2);
    assert_eq!(crate::set::js_set_value_at(set, 1), 20.0);

    let site = typed_feedback_snapshot()
        .sites
        .into_iter()
        .find(|site| site.site_id == 74)
        .expect("site 74");
    assert_eq!(site.fallback_calls, 8);
}

#[test]
fn runtime_dynamic_index_fallbacks_preserve_fractional_keys_for_array_like_receivers() {
    let buf = crate::buffer::js_buffer_alloc(3, 0);
    crate::buffer::js_buffer_set(buf, 1, 22);
    let buf_box = crate::value::js_nanbox_pointer(buf as i64);
    assert_eq!(crate::value::js_dyn_index_get(buf_box, 1.0), 22.0);
    assert_undefined(crate::value::js_dyn_index_get(buf_box, 1.5));

    let ta = crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT8 as i32, 3);
    crate::typedarray::js_typed_array_set(ta, 1, 33.0);
    let ta_box = crate::value::js_nanbox_pointer(ta as i64);
    assert_eq!(crate::value::js_dyn_index_get(ta_box, 1.0), 33.0);
    assert_undefined(crate::value::js_dyn_index_get(ta_box, 1.5));

    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 10.0);
    crate::set::js_set_add(set, 20.0);
    let set_box = crate::value::js_nanbox_pointer(set as i64);
    assert_eq!(crate::value::js_dyn_index_get(set_box, 1.0), 20.0);
    assert_undefined(crate::value::js_dyn_index_get(set_box, 1.5));
    crate::value::js_dyn_index_set(set_box, 1.5, 99.0);
    assert_eq!(crate::set::js_set_size(set), 2);
    assert_eq!(crate::set::js_set_value_at(set, 1), 20.0);

    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 10.0, 100.0);
    crate::map::js_map_set(map, 20.0, 200.0);
    let map_box = crate::value::js_nanbox_pointer(map as i64);
    assert_eq!(crate::value::js_dyn_index_get(map_box, 1.0), 20.0);
    assert_undefined(crate::value::js_dyn_index_get(map_box, 1.5));
    crate::value::js_dyn_index_set(map_box, 1.5, 88.0);
    assert_eq!(crate::map::js_map_size(map), 2);
    assert_eq!(crate::map::js_map_entry_key_at(map, 1), 20.0);
}

#[test]
fn polymorphic_index_fallbacks_preserve_fractional_keys_for_array_like_receivers() {
    let buf = crate::buffer::js_buffer_alloc(3, 0);
    crate::buffer::js_buffer_set(buf, 1, 22);
    let buf_handle = crate::value::js_nanbox_pointer(buf as i64).to_bits() as i64;
    assert_eq!(
        crate::object::js_object_get_index_polymorphic(buf_handle, 1.0),
        22.0
    );
    assert_undefined(crate::object::js_object_get_index_polymorphic(
        buf_handle, 1.5,
    ));
    crate::object::js_object_set_index_polymorphic(buf_handle, 1.5, 99.0);
    assert_eq!(crate::buffer::js_buffer_get(buf, 1), 22);

    let ta = crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT8 as i32, 3);
    crate::typedarray::js_typed_array_set(ta, 1, 33.0);
    let ta_handle = crate::value::js_nanbox_pointer(ta as i64).to_bits() as i64;
    assert_eq!(
        crate::object::js_object_get_index_polymorphic(ta_handle, 1.0),
        33.0
    );
    assert_undefined(crate::object::js_object_get_index_polymorphic(
        ta_handle, 1.5,
    ));
    crate::object::js_object_set_index_polymorphic(ta_handle, 1.5, 88.0);
    assert_eq!(crate::typedarray::js_typed_array_get(ta, 1), 33.0);

    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 10.0);
    crate::set::js_set_add(set, 20.0);
    let set_handle = crate::value::js_nanbox_pointer(set as i64).to_bits() as i64;
    assert_eq!(
        crate::object::js_object_get_index_polymorphic(set_handle, 1.0),
        20.0
    );
    assert_undefined(crate::object::js_object_get_index_polymorphic(
        set_handle, 1.5,
    ));
    crate::object::js_object_set_index_polymorphic(set_handle, 1.5, 77.0);
    assert_eq!(crate::set::js_set_size(set), 2);
    assert_eq!(crate::set::js_set_value_at(set, 1), 20.0);

    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 10.0, 100.0);
    crate::map::js_map_set(map, 20.0, 200.0);
    let map_handle = crate::value::js_nanbox_pointer(map as i64).to_bits() as i64;
    assert_eq!(
        crate::object::js_object_get_index_polymorphic(map_handle, 1.0),
        20.0
    );
    assert_undefined(crate::object::js_object_get_index_polymorphic(
        map_handle, 1.5,
    ));
    crate::object::js_object_set_index_polymorphic(map_handle, 1.5, 66.0);
    assert_eq!(crate::map::js_map_size(map), 2);
    assert_eq!(crate::map::js_map_entry_key_at(map, 1), 20.0);
}

#[test]
fn typed_feedback_numeric_array_get_guard_requires_numeric_layout() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(26, TypedFeedbackSiteKind::ArrayElement, "arr[i]");

    let values = [1.0, 2.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);

    let first = js_typed_feedback_numeric_array_index_get_guard(26, arr_box, 0, 1);
    assert_eq!(first, 1);

    let payload = crate::string::js_string_from_bytes(b"downgraded".as_ptr(), 10);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    crate::array::js_array_set_f64(arr, 0, payload_value);
    assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 0);

    let second = js_typed_feedback_numeric_array_index_get_guard(26, arr_box, 0, 1);
    assert_eq!(second, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
    assert_eq!(site.fallback_calls, 0);
}

#[test]
fn typed_feedback_packed_i32_loop_guard_rejects_fractional_numeric_layout() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(70, TypedFeedbackSiteKind::ArrayElement, "packed_i32_loop");

    let ints = [1.0, 2.0, 3.0];
    let int_arr = crate::array::js_array_from_f64(ints.as_ptr(), ints.len() as u32);
    let int_box = crate::value::js_nanbox_pointer(int_arr as i64);
    assert_eq!(
        js_typed_feedback_packed_i32_array_loop_guard(70, int_box),
        1
    );

    let fractional = [1.0, 2.5, 3.0];
    let fractional_arr =
        crate::array::js_array_from_f64(fractional.as_ptr(), fractional.len() as u32);
    let fractional_box = crate::value::js_nanbox_pointer(fractional_arr as i64);
    assert_eq!(
        crate::array::js_array_is_numeric_f64_layout(fractional_arr),
        1
    );
    assert_eq!(
        js_typed_feedback_packed_i32_array_loop_guard(70, fractional_box),
        0
    );

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
}

#[test]
fn typed_feedback_packed_u32_loop_guard_rejects_signed_fractional_and_overflow_layouts() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(71, TypedFeedbackSiteKind::ArrayElement, "packed_u32_loop");

    let uints = [0.0, 4_294_967_295.0];
    let uint_arr = crate::array::js_array_from_f64(uints.as_ptr(), uints.len() as u32);
    let uint_box = crate::value::js_nanbox_pointer(uint_arr as i64);
    assert_eq!(
        js_typed_feedback_packed_u32_array_loop_guard(71, uint_box),
        1
    );

    for values in [[-1.0, 2.0], [1.5, 2.0], [4_294_967_296.0, 2.0]] {
        let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
        let arr_box = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 1);
        assert_eq!(
            js_typed_feedback_packed_u32_array_loop_guard(71, arr_box),
            0
        );
    }

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 3);
}

#[test]
fn typed_feedback_numeric_array_set_guard_requires_numeric_value_and_layout() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(27, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");

    let values = [1.0, 2.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);

    let first = js_typed_feedback_numeric_array_index_set_guard(27, arr_box, 1, 3.0, 1);
    assert_eq!(first, 1);

    let payload = crate::string::js_string_from_bytes(b"not-number".as_ptr(), 10);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    let nonnumeric =
        js_typed_feedback_numeric_array_index_set_guard(27, arr_box, 1, payload_value, 1);
    assert_eq!(nonnumeric, 0);

    crate::array::js_array_set_f64(arr, 0, payload_value);
    let downgraded = js_typed_feedback_numeric_array_index_set_guard(27, arr_box, 1, 4.0, 1);
    assert_eq!(downgraded, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 2);
    assert_eq!(site.fallback_calls, 0);
}

#[test]
fn typed_feedback_numeric_array_guards_reject_registered_class_ref_bits() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(68, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");
    register(69, TypedFeedbackSiteKind::ArrayElement, "arr.push");

    let class_id = 0x00C0_DE01;
    unsafe {
        crate::object::js_register_class_id(class_id);
    }
    let class_ref = f64::from_bits(crate::value::INT32_TAG | class_id as u64);

    let values = [1.0, 2.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);

    assert_eq!(
        js_typed_feedback_numeric_array_index_set_guard(68, arr_box, 1, class_ref, 1),
        0
    );
    assert_eq!(
        js_typed_feedback_numeric_array_push_guard(69, arr_box, class_ref),
        0
    );
    assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 1);

    let snapshot = typed_feedback_snapshot();
    let set_site = snapshot
        .sites
        .iter()
        .find(|site| site.site_id == 68)
        .expect("set site");
    assert_eq!(set_site.guard_passes, 0);
    assert_eq!(set_site.guard_failures, 1);
    let push_site = snapshot
        .sites
        .iter()
        .find(|site| site.site_id == 69)
        .expect("push site");
    assert_eq!(push_site.guard_passes, 0);
    assert_eq!(push_site.guard_failures, 1);
}

#[test]
fn typed_feedback_numeric_array_push_guard_requires_room_numeric_value_and_layout() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(28, TypedFeedbackSiteKind::ArrayElement, "arr.push");

    let arr = crate::array::js_array_alloc(0);
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);

    let first = js_typed_feedback_numeric_array_push_guard(28, arr_box, 1.0);
    assert_eq!(first, 1);

    let payload = crate::string::js_string_from_bytes(b"not-number".as_ptr(), 10);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    let nonnumeric = js_typed_feedback_numeric_array_push_guard(28, arr_box, payload_value);
    assert_eq!(nonnumeric, 0);

    let capacity = unsafe { (*arr).capacity };
    for i in 0..capacity {
        crate::array::js_array_push_f64(arr, i as f64);
    }
    let full = js_typed_feedback_numeric_array_push_guard(28, arr_box, 2.0);
    assert_eq!(full, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 2);
    assert_eq!(site.fallback_calls, 0);
}

#[test]
fn typed_feedback_numeric_array_push_guard_rejects_mutability_restricted_arrays() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(72, TypedFeedbackSiteKind::ArrayElement, "arr.push");

    let assert_rejected = |site_id, arr: *mut crate::array::ArrayHeader| {
        assert_eq!(crate::array::js_array_mark_numeric_f64_layout(arr), 1);
        let arr_box = crate::value::js_nanbox_pointer(arr as i64);
        assert_eq!(
            js_typed_feedback_numeric_array_push_guard(site_id, arr_box, 4.0),
            0
        );
    };

    let frozen = crate::array::js_array_alloc(4);
    crate::object::js_object_freeze(crate::value::js_nanbox_pointer(frozen as i64));
    assert_rejected(72, frozen);

    let sealed = crate::array::js_array_alloc(4);
    crate::object::js_object_seal(crate::value::js_nanbox_pointer(sealed as i64));
    assert_rejected(72, sealed);

    let no_extend = crate::array::js_array_alloc(4);
    crate::object::js_object_prevent_extensions(crate::value::js_nanbox_pointer(no_extend as i64));
    assert_rejected(72, no_extend);

    let non_writable_length = crate::array::js_array_alloc(4);
    let descriptor = crate::object::js_object_alloc(0, 0);
    let writable_key = crate::string::js_string_from_bytes(b"writable".as_ptr(), 8);
    crate::object::js_object_set_field_by_name(
        descriptor,
        writable_key,
        f64::from_bits(crate::value::TAG_FALSE),
    );
    crate::object::js_object_define_property(
        crate::value::js_nanbox_pointer(non_writable_length as i64),
        crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(b"length".as_ptr(), 6) as i64
        ),
        crate::value::js_nanbox_pointer(descriptor as i64),
    );
    assert_rejected(72, non_writable_length);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 0);
    assert_eq!(site.guard_failures, 4);
    assert_eq!(site.fallback_calls, 0);
}

fn assert_lto_keepalive_anchor(src: &str, static_name: &str, signature: &str, target: &str) {
    let static_pos = src
        .find(static_name)
        .unwrap_or_else(|| panic!("missing keepalive static {static_name} for {target}"));
    let start = static_pos.saturating_sub(32);
    let end = (static_pos + 512).min(src.len());
    let window = &src[start..end];
    assert!(
        window.contains("#[used]"),
        "keepalive static {static_name} for {target} is not #[used]"
    );
    assert!(
        window.contains(signature),
        "missing keepalive signature for {target}"
    );
    assert!(window.contains(target), "missing keepalive target {target}");
}

#[test]
fn numeric_array_helpers_have_lto_keepalive_anchors() {
    let header = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/array/header.rs"));
    let indexing = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/array/indexing.rs"
    ));
    let push_pop = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/array/push_pop.rs"
    ));

    for (src, static_name, signature, target) in [
        (
            header,
            "KEEP_JS_ARRAY_NUMERIC_VALUE_TO_RAW_F64",
            "static KEEP_JS_ARRAY_NUMERIC_VALUE_TO_RAW_F64: extern \"C\" fn(f64) -> f64",
            "js_array_numeric_value_to_raw_f64",
        ),
        (
            header,
            "KEEP_JS_ARRAY_MARK_NUMERIC_F64_LAYOUT",
            "static KEEP_JS_ARRAY_MARK_NUMERIC_F64_LAYOUT: extern \"C\" fn(*mut ArrayHeader) -> i32",
            "js_array_mark_numeric_f64_layout",
        ),
        (
            header,
            "KEEP_JS_ARRAY_CLEAR_NUMERIC_LAYOUT",
            "static KEEP_JS_ARRAY_CLEAR_NUMERIC_LAYOUT: extern \"C\" fn(*mut ArrayHeader)",
            "js_array_clear_numeric_layout",
        ),
        (
            header,
            "KEEP_JS_ARRAY_NOTE_NUMERIC_WRITE",
            "static KEEP_JS_ARRAY_NOTE_NUMERIC_WRITE: extern \"C\" fn(*mut ArrayHeader, u64)",
            "js_array_note_numeric_write",
        ),
        (
            header,
            "KEEP_JS_ARRAY_IS_NUMERIC_F64_LAYOUT",
            "static KEEP_JS_ARRAY_IS_NUMERIC_F64_LAYOUT: extern \"C\" fn(*const ArrayHeader) -> i32",
            "js_array_is_numeric_f64_layout",
        ),
        (
            indexing,
            "KEEP_JS_ARRAY_NUMERIC_GET_F64_UNBOXED",
            "static KEEP_JS_ARRAY_NUMERIC_GET_F64_UNBOXED: extern \"C\" fn(*mut ArrayHeader, u32) -> f64",
            "js_array_numeric_get_f64_unboxed",
        ),
        (
            indexing,
            "KEEP_JS_ARRAY_NUMERIC_SET_F64_UNBOXED",
            "static KEEP_JS_ARRAY_NUMERIC_SET_F64_UNBOXED: extern \"C\" fn(*mut ArrayHeader, u32, f64) -> i32",
            "js_array_numeric_set_f64_unboxed",
        ),
        (
            push_pop,
            "KEEP_JS_ARRAY_NUMERIC_PUSH_F64_UNBOXED",
            "static KEEP_JS_ARRAY_NUMERIC_PUSH_F64_UNBOXED: extern \"C\" fn(",
            "js_array_numeric_push_f64_unboxed",
        ),
    ] {
        assert_lto_keepalive_anchor(src, static_name, signature, target);
    }
}

#[test]
fn typed_feedback_array_loop_helpers_have_lto_keepalive_anchors() {
    let typed_feedback = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/typed_feedback.rs"
    ));

    assert_lto_keepalive_anchor(
        typed_feedback,
        "KEEP_JS_TYPED_FEEDBACK_PACKED_I32_ARRAY_LOOP_GUARD",
        "static KEEP_JS_TYPED_FEEDBACK_PACKED_I32_ARRAY_LOOP_GUARD: extern \"C\" fn(u64, f64) -> i32",
        "js_typed_feedback_packed_i32_array_loop_guard",
    );
    assert_lto_keepalive_anchor(
        typed_feedback,
        "KEEP_JS_TYPED_FEEDBACK_PACKED_U32_ARRAY_LOOP_GUARD",
        "static KEEP_JS_TYPED_FEEDBACK_PACKED_U32_ARRAY_LOOP_GUARD: extern \"C\" fn(u64, f64) -> i32",
        "js_typed_feedback_packed_u32_array_loop_guard",
    );
    // #6011: hole-tolerant range guard (rustfmt wraps the fn-pointer type, so
    // anchor on the static declaration prefix only).
    assert_lto_keepalive_anchor(
        typed_feedback,
        "KEEP_JS_TYPED_FEEDBACK_PACKED_F64_RANGE_LOOP_GUARD",
        "static KEEP_JS_TYPED_FEEDBACK_PACKED_F64_RANGE_LOOP_GUARD: extern \"C\" fn(",
        "js_typed_feedback_packed_f64_range_loop_guard",
    );
}

#[test]
fn representation_lowering_helpers_have_lto_keepalive_anchors() {
    let native_abi = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/native_abi.rs"));
    let native_module = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/object/native_module.rs"
    ));
    let guards = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/typed_feedback/guards.rs"
    ));
    let trace = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/typed_feedback/trace.rs"
    ));
    let map = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/map.rs"));
    let set = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/set.rs"));
    let boxes = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/box.rs"));
    let closure_alloc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/closure/alloc.rs"));
    let promise = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/promise/mod.rs"));

    for (src, static_name, signature, target) in [
        (
            native_abi,
            "KEEP_JS_TYPED_F64_ARG_GUARD",
            "static KEEP_JS_TYPED_F64_ARG_GUARD: extern \"C\" fn(f64) -> i32",
            "js_typed_f64_arg_guard",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_F64_ARG_TO_RAW",
            "static KEEP_JS_TYPED_F64_ARG_TO_RAW: extern \"C\" fn(f64) -> f64",
            "js_typed_f64_arg_to_raw",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_I32_ARG_GUARD",
            "static KEEP_JS_TYPED_I32_ARG_GUARD: extern \"C\" fn(f64) -> i32",
            "js_typed_i32_arg_guard",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_I32_ARG_TO_RAW",
            "static KEEP_JS_TYPED_I32_ARG_TO_RAW: extern \"C\" fn(f64) -> i32",
            "js_typed_i32_arg_to_raw",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_I1_ARG_GUARD",
            "static KEEP_JS_TYPED_I1_ARG_GUARD: extern \"C\" fn(f64) -> i32",
            "js_typed_i1_arg_guard",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_I1_ARG_TO_RAW",
            "static KEEP_JS_TYPED_I1_ARG_TO_RAW: extern \"C\" fn(f64) -> i32",
            "js_typed_i1_arg_to_raw",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_STRING_ARG_GUARD",
            "static KEEP_JS_TYPED_STRING_ARG_GUARD: extern \"C\" fn(f64) -> i32",
            "js_typed_string_arg_guard",
        ),
        (
            native_abi,
            "KEEP_JS_TYPED_STRING_ARG_TO_RAW",
            "static KEEP_JS_TYPED_STRING_ARG_TO_RAW: extern \"C\" fn(f64) -> i64",
            "js_typed_string_arg_to_raw",
        ),
        (
            boxes,
            "KEEP_JS_BOX_ALLOC_BITS",
            "static KEEP_JS_BOX_ALLOC_BITS: extern \"C\" fn(i64) -> *mut Box",
            "js_box_alloc_bits",
        ),
        (
            boxes,
            "KEEP_JS_BOX_GET_BITS",
            "static KEEP_JS_BOX_GET_BITS: extern \"C\" fn(*mut Box) -> i64",
            "js_box_get_bits",
        ),
        (
            boxes,
            "KEEP_JS_BOX_SET_BITS",
            "static KEEP_JS_BOX_SET_BITS: extern \"C\" fn(*mut Box, i64)",
            "js_box_set_bits",
        ),
        (
            closure_alloc,
            "KEEP_JS_CLOSURE_GET_CAPTURE_BITS",
            "static KEEP_JS_CLOSURE_GET_CAPTURE_BITS: extern \"C\" fn(*const ClosureHeader, u32) -> u64",
            "js_closure_get_capture_bits",
        ),
        (
            closure_alloc,
            "KEEP_JS_CLOSURE_SET_CAPTURE_BITS",
            "static KEEP_JS_CLOSURE_SET_CAPTURE_BITS: extern \"C\" fn(*mut ClosureHeader, u32, u64)",
            "js_closure_set_capture_bits",
        ),
        (
            native_abi,
            "KEEP_JS_OBJECT_GET_FIELD_BY_PROPERTY_ID_F64",
            "static KEEP_JS_OBJECT_GET_FIELD_BY_PROPERTY_ID_F64: extern \"C\" fn(*const ObjectHeader, i64) -> f64",
            "js_object_get_field_by_property_id_f64",
        ),
        (
            native_abi,
            "KEEP_JS_OBJECT_SET_FIELD_BY_PROPERTY_ID",
            "static KEEP_JS_OBJECT_SET_FIELD_BY_PROPERTY_ID: extern \"C\" fn(*mut ObjectHeader, i64, f64)",
            "js_object_set_field_by_property_id",
        ),
        (
            native_abi,
            "KEEP_JS_NATIVE_CALL_METHOD_BY_ID",
            "static KEEP_JS_NATIVE_CALL_METHOD_BY_ID: unsafe extern \"C\" fn(f64, i64, *const f64, usize) -> f64",
            "js_native_call_method_by_id",
        ),
        (
            native_abi,
            "KEEP_JS_NATIVE_CALL_METHOD_APPLY_BY_ID",
            "static KEEP_JS_NATIVE_CALL_METHOD_APPLY_BY_ID: unsafe extern \"C\" fn(f64, i64, i64) -> f64",
            "js_native_call_method_apply_by_id",
        ),
        (
            native_module,
            "KEEP_CLASS_METHOD_BIND_BY_ID",
            "static KEEP_CLASS_METHOD_BIND_BY_ID: extern \"C\" fn(f64, i64) -> f64",
            "js_class_method_bind_by_id",
        ),
        (
            guards,
            "static G0",
            "static G0: extern \"C\" fn(u64, f64, u32, *const ArrayHeader, *const crate::StringHeader, u32, i32) -> i32",
            "js_typed_feedback_class_field_get_guard",
        ),
        (
            guards,
            "static G1",
            "static G1: extern \"C\" fn(u64, f64, u32, *const ArrayHeader, *const crate::StringHeader, u32, f64, i32) -> i32",
            "js_typed_feedback_class_field_set_guard",
        ),
        (
            guards,
            "static G2",
            "static G2: unsafe extern \"C\" fn(u64, f64, u32, *const ArrayHeader, *const i8, usize, *const u8) -> i32",
            "js_typed_feedback_method_direct_call_guard",
        ),
        (
            guards,
            "static G3",
            "static G3: extern \"C\" fn(u64, f64, *const u8, u32, u32) -> i32",
            "js_typed_feedback_closure_direct_call_guard",
        ),
        (
            guards,
            "static G4",
            "static G4: unsafe extern \"C\" fn(f64, u32, *const ArrayHeader) -> i32",
            "js_method_direct_shape_guard",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_NUMBER",
            "static KEEP_JS_MAP_SET_STRING_NUMBER: extern \"C\" fn(",
            "js_map_set_string_number",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_KEY",
            "static KEEP_JS_MAP_SET_STRING_KEY: extern \"C\" fn(",
            "js_map_set_string_key",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_I32",
            "static KEEP_JS_MAP_SET_STRING_I32: extern \"C\" fn(",
            "js_map_set_string_i32",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_U32",
            "static KEEP_JS_MAP_SET_STRING_U32: extern \"C\" fn(",
            "js_map_set_string_u32",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_F32",
            "static KEEP_JS_MAP_SET_STRING_F32: extern \"C\" fn(",
            "js_map_set_string_f32",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_BOOL",
            "static KEEP_JS_MAP_SET_STRING_BOOL: extern \"C\" fn(",
            "js_map_set_string_bool",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_STRING_STRING",
            "static KEEP_JS_MAP_SET_STRING_STRING: extern \"C\" fn(",
            "js_map_set_string_string",
        ),
        (
            map,
            "KEEP_JS_MAP_SET_NUMBER_KEY",
            "static KEEP_JS_MAP_SET_NUMBER_KEY: extern \"C\" fn(*mut MapHeader, f64, f64) -> *mut MapHeader",
            "js_map_set_number_key",
        ),
        (
            map,
            "KEEP_JS_MAP_HAS_STRING_KEY",
            "static KEEP_JS_MAP_HAS_STRING_KEY: extern \"C\" fn(*const MapHeader, *const StringHeader) -> i32",
            "js_map_has_string_key",
        ),
        (
            map,
            "KEEP_JS_MAP_HAS_NUMBER_KEY",
            "static KEEP_JS_MAP_HAS_NUMBER_KEY: extern \"C\" fn(*const MapHeader, f64) -> i32",
            "js_map_has_number_key",
        ),
        (
            map,
            "KEEP_JS_MAP_GET_STRING_KEY",
            "static KEEP_JS_MAP_GET_STRING_KEY: extern \"C\" fn(*const MapHeader, *const StringHeader) -> f64",
            "js_map_get_string_key",
        ),
        (
            map,
            "KEEP_JS_MAP_GET_NUMBER_KEY",
            "static KEEP_JS_MAP_GET_NUMBER_KEY: extern \"C\" fn(*const MapHeader, f64) -> f64",
            "js_map_get_number_key",
        ),
        (
            map,
            "KEEP_JS_MAP_DELETE_STRING_KEY",
            "static KEEP_JS_MAP_DELETE_STRING_KEY: extern \"C\" fn(*mut MapHeader, *const StringHeader) -> i32",
            "js_map_delete_string_key",
        ),
        (
            map,
            "KEEP_JS_MAP_DELETE_NUMBER_KEY",
            "static KEEP_JS_MAP_DELETE_NUMBER_KEY: extern \"C\" fn(*mut MapHeader, f64) -> i32",
            "js_map_delete_number_key",
        ),
        (
            set,
            "KEEP_JS_SET_ADD_STRING",
            "static KEEP_JS_SET_ADD_STRING: extern \"C\" fn(",
            "js_set_add_string",
        ),
        (
            set,
            "KEEP_JS_SET_ADD_NUMBER",
            "static KEEP_JS_SET_ADD_NUMBER: extern \"C\" fn(*mut SetHeader, f64) -> *mut SetHeader",
            "js_set_add_number",
        ),
        (
            set,
            "KEEP_JS_SET_HAS_STRING",
            "static KEEP_JS_SET_HAS_STRING: extern \"C\" fn(*const SetHeader, *const StringHeader) -> i32",
            "js_set_has_string",
        ),
        (
            set,
            "KEEP_JS_SET_HAS_NUMBER",
            "static KEEP_JS_SET_HAS_NUMBER: extern \"C\" fn(*const SetHeader, f64) -> i32",
            "js_set_has_number",
        ),
        (
            set,
            "KEEP_JS_SET_DELETE_STRING",
            "static KEEP_JS_SET_DELETE_STRING: extern \"C\" fn(*mut SetHeader, *const StringHeader) -> i32",
            "js_set_delete_string",
        ),
        (
            set,
            "KEEP_JS_SET_DELETE_NUMBER",
            "static KEEP_JS_SET_DELETE_NUMBER: extern \"C\" fn(*mut SetHeader, f64) -> i32",
            "js_set_delete_number",
        ),
        (
            set,
            "KEEP_JS_SET_ADD_I32",
            "static KEEP_JS_SET_ADD_I32: extern \"C\" fn(*mut SetHeader, i32) -> *mut SetHeader",
            "js_set_add_i32",
        ),
        (
            set,
            "KEEP_JS_SET_HAS_I32",
            "static KEEP_JS_SET_HAS_I32: extern \"C\" fn(*const SetHeader, i32) -> i32",
            "js_set_has_i32",
        ),
        (
            set,
            "KEEP_JS_SET_DELETE_I32",
            "static KEEP_JS_SET_DELETE_I32: extern \"C\" fn(*mut SetHeader, i32) -> i32",
            "js_set_delete_i32",
        ),
        (
            set,
            "KEEP_JS_SET_ADD_U32",
            "static KEEP_JS_SET_ADD_U32: extern \"C\" fn(*mut SetHeader, u32) -> *mut SetHeader",
            "js_set_add_u32",
        ),
        (
            set,
            "KEEP_JS_SET_HAS_U32",
            "static KEEP_JS_SET_HAS_U32: extern \"C\" fn(*const SetHeader, u32) -> i32",
            "js_set_has_u32",
        ),
        (
            set,
            "KEEP_JS_SET_DELETE_U32",
            "static KEEP_JS_SET_DELETE_U32: extern \"C\" fn(*mut SetHeader, u32) -> i32",
            "js_set_delete_u32",
        ),
        (
            set,
            "KEEP_JS_SET_ADD_F32",
            "static KEEP_JS_SET_ADD_F32: extern \"C\" fn(*mut SetHeader, f32) -> *mut SetHeader",
            "js_set_add_f32",
        ),
        (
            set,
            "KEEP_JS_SET_HAS_F32",
            "static KEEP_JS_SET_HAS_F32: extern \"C\" fn(*const SetHeader, f32) -> i32",
            "js_set_has_f32",
        ),
        (
            set,
            "KEEP_JS_SET_DELETE_F32",
            "static KEEP_JS_SET_DELETE_F32: extern \"C\" fn(*mut SetHeader, f32) -> i32",
            "js_set_delete_f32",
        ),
        (
            set,
            "KEEP_JS_SET_ADD_BOOL",
            "static KEEP_JS_SET_ADD_BOOL: extern \"C\" fn(*mut SetHeader, i32) -> *mut SetHeader",
            "js_set_add_bool",
        ),
        (
            set,
            "KEEP_JS_SET_HAS_BOOL",
            "static KEEP_JS_SET_HAS_BOOL: extern \"C\" fn(*const SetHeader, i32) -> i32",
            "js_set_has_bool",
        ),
        (
            set,
            "KEEP_JS_SET_DELETE_BOOL",
            "static KEEP_JS_SET_DELETE_BOOL: extern \"C\" fn(*mut SetHeader, i32) -> i32",
            "js_set_delete_bool",
        ),
        (
            boxes,
            "KEEP_JS_I32_BOX_ALLOC",
            "static KEEP_JS_I32_BOX_ALLOC: extern \"C\" fn(i32) -> *mut I32Box",
            "js_i32_box_alloc",
        ),
        (
            boxes,
            "KEEP_JS_I32_BOX_GET",
            "static KEEP_JS_I32_BOX_GET: extern \"C\" fn(*mut I32Box) -> i32",
            "js_i32_box_get",
        ),
        (
            boxes,
            "KEEP_JS_I32_BOX_SET",
            "static KEEP_JS_I32_BOX_SET: extern \"C\" fn(*mut I32Box, i32)",
            "js_i32_box_set",
        ),
        (
            boxes,
            "KEEP_JS_BOOL_BOX_ALLOC",
            "static KEEP_JS_BOOL_BOX_ALLOC: extern \"C\" fn(i32) -> *mut BoolBox",
            "js_bool_box_alloc",
        ),
        (
            boxes,
            "KEEP_JS_BOOL_BOX_GET",
            "static KEEP_JS_BOOL_BOX_GET: extern \"C\" fn(*mut BoolBox) -> i32",
            "js_bool_box_get",
        ),
        (
            boxes,
            "KEEP_JS_BOOL_BOX_SET",
            "static KEEP_JS_BOOL_BOX_SET: extern \"C\" fn(*mut BoolBox, i32)",
            "js_bool_box_set",
        ),
        (
            promise,
            "KEEP_JS_ITER_RESULT_SET_I32",
            "static KEEP_JS_ITER_RESULT_SET_I32: extern \"C\" fn(i32, i32) -> f64",
            "js_iter_result_set_i32",
        ),
        (
            promise,
            "KEEP_JS_ITER_RESULT_SET_I1",
            "static KEEP_JS_ITER_RESULT_SET_I1: extern \"C\" fn(i32, i32) -> f64",
            "js_iter_result_set_i1",
        ),
        (
            promise,
            "KEEP_JS_ITER_RESULT_GET_VALUE_I32",
            "static KEEP_JS_ITER_RESULT_GET_VALUE_I32: extern \"C\" fn() -> i32",
            "js_iter_result_get_value_i32",
        ),
        (
            promise,
            "KEEP_JS_ITER_RESULT_GET_VALUE_I1",
            "static KEEP_JS_ITER_RESULT_GET_VALUE_I1: extern \"C\" fn() -> i32",
            "js_iter_result_get_value_i1",
        ),
        (
            trace,
            "static K30",
            "static K30: unsafe extern \"C\" fn(u64, f64, i64, *const f64, usize) -> f64",
            "js_typed_feedback_native_call_method_by_id",
        ),
        (
            trace,
            "static K31",
            "static K31: unsafe extern \"C\" fn(u64, f64, i64, i64) -> f64",
            "js_typed_feedback_native_call_method_apply_by_id",
        ),
    ] {
        assert_lto_keepalive_anchor(src, static_name, signature, target);
    }
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

    let guard =
        js_typed_feedback_class_field_set_guard(31, receiver, class_id, keys, key, 0, 2.0, 0);
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

    let guard =
        js_typed_feedback_class_field_set_guard(32, receiver, class_id, keys, key, 0, 7.0, 0);
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
        js_typed_feedback_class_field_get_guard(39, receiver, class_id, expected_keys, key_x, 0, 0);
    assert_eq!(first, 1);

    let key_y = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, key_y, 10.0);
    assert_ne!(unsafe { (*obj).keys_array }, expected_keys);

    let second =
        js_typed_feedback_class_field_get_guard(39, receiver, class_id, expected_keys, key_x, 0, 0);
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
fn typed_feedback_class_field_get_guard_requires_raw_f64_layout_when_requested() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(43, TypedFeedbackSiteKind::PropertyGet, "obj.x");

    let class_id = 0x7EED_0043;
    let (obj, expected_keys, key_x, receiver) = class_instance(class_id, b"x");
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 5.0);
    let raw_mask = [0b1u64];
    crate::gc::js_gc_init_typed_shape_layout(
        obj as u64,
        1,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    let first =
        js_typed_feedback_class_field_get_guard(43, receiver, class_id, expected_keys, key_x, 0, 1);
    assert_eq!(first, 1);

    let payload = crate::string::js_string_from_bytes(b"boxed".as_ptr(), 5);
    crate::object::js_object_set_field(obj, 0, crate::JSValue::string_ptr(payload));

    let second =
        js_typed_feedback_class_field_get_guard(43, receiver, class_id, expected_keys, key_x, 0, 1);
    assert_eq!(second, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 1);
    assert!(site.representation_invalidations >= 1);
}

#[test]
fn typed_feedback_class_field_set_guard_requires_raw_f64_value_and_layout() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(44, TypedFeedbackSiteKind::PropertySet, "obj.x=");

    let class_id = 0x7EED_0044;
    let (obj, expected_keys, key_x, receiver) = class_instance(class_id, b"x");
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.0);
    let raw_mask = [0b1u64];
    crate::gc::js_gc_init_typed_shape_layout(
        obj as u64,
        1,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    let first = js_typed_feedback_class_field_set_guard(
        44,
        receiver,
        class_id,
        expected_keys,
        key_x,
        0,
        2.0,
        1,
    );
    assert_eq!(first, 1);

    let payload = crate::string::js_string_from_bytes(b"boxed".as_ptr(), 5);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    let second = js_typed_feedback_class_field_set_guard(
        44,
        receiver,
        class_id,
        expected_keys,
        key_x,
        0,
        payload_value,
        1,
    );
    assert_eq!(second, 0);

    let short = crate::value::JSValue::try_short_string(b"abc").unwrap();
    let third = js_typed_feedback_class_field_set_guard(
        44,
        receiver,
        class_id,
        expected_keys,
        key_x,
        0,
        f64::from_bits(short.bits()),
        1,
    );
    assert_eq!(third, 0);

    let handle_value = f64::from_bits(crate::value::JS_HANDLE_TAG | 0x1234);
    let fourth = js_typed_feedback_class_field_set_guard(
        44,
        receiver,
        class_id,
        expected_keys,
        key_x,
        0,
        handle_value,
        1,
    );
    assert_eq!(fourth, 0);

    let site = &typed_feedback_snapshot().sites[0];
    assert_eq!(site.guard_passes, 1);
    assert_eq!(site.guard_failures, 3);
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

#[test]
fn typed_feedback_trace_json_includes_observed_kinds() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(67, TypedFeedbackSiteKind::ArrayElement, "arr[i]=");

    let values = [1.0, 2.0];
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(
        js_typed_feedback_numeric_array_index_set_guard(67, arr_box, 1, 3.0, 1),
        1
    );

    let payload = crate::string::js_string_from_bytes(b"not-number".as_ptr(), 10);
    let payload_value = crate::value::js_nanbox_string(payload as i64);
    assert_eq!(
        js_typed_feedback_numeric_array_index_set_guard(67, arr_box, 1, payload_value, 1),
        0
    );

    let json = typed_feedback_trace_json();
    let observed = json["sites"][0]["observed_kinds"]
        .as_array()
        .expect("observed_kinds array");
    assert!(
        observed.iter().any(|kind| {
            kind["source"].as_str() == Some("array")
                && kind["heap_type"].as_str() == Some("array")
                && kind["array_layout"].as_str() == Some("pointer_free")
                && kind["array_element_kind"].as_str() == Some("number")
                && kind["value_kind"].as_str() == Some("number")
        }),
        "expected numeric array observation in {observed:?}"
    );
    assert!(
        observed
            .iter()
            .any(|kind| kind["value_kind"].as_str() == Some("string")),
        "expected fallback value kind in {observed:?}"
    );
    for kind in observed {
        let obj = kind.as_object().expect("kind object");
        assert!(!obj.contains_key("object_addr"));
        assert!(!obj.contains_key("shape_addr"));
    }
}

#[test]
fn typed_feedback_trace_dump_honors_env_paths() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();

    let disabled_dir = unique_temp_dir("disabled");
    {
        let _cwd = CurrentDirGuard::set(&disabled_dir);
        for value in [None, Some(""), Some("0")] {
            reset_typed_feedback_for_tests();
            let _env = EnvGuard::set(value);
            js_typed_feedback_maybe_dump_trace();
            assert!(!disabled_dir.join("typed-feedback-trace.json").exists());
        }
    }
    let _ = std::fs::remove_dir_all(&disabled_dir);

    let explicit_dir = unique_temp_dir("explicit");
    let explicit_path = explicit_dir.join("nested").join("trace.json");
    {
        reset_typed_feedback_for_tests();
        register(68, TypedFeedbackSiteKind::PropertyGet, "obj.x");
        let _env = EnvGuard::set(Some(explicit_path.to_str().unwrap()));
        js_typed_feedback_maybe_dump_trace();
        assert!(explicit_path.exists());
        let parsed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&explicit_path).unwrap()).unwrap();
        assert_eq!(parsed["total_sites"].as_u64(), Some(1));
    }
    let _ = std::fs::remove_dir_all(&explicit_dir);

    let default_dir = unique_temp_dir("default");
    {
        reset_typed_feedback_for_tests();
        register(69, TypedFeedbackSiteKind::PropertyGet, "obj.y");
        let _cwd = CurrentDirGuard::set(&default_dir);
        let _env = EnvGuard::set(Some("1"));
        js_typed_feedback_maybe_dump_trace();
        assert!(default_dir.join("typed-feedback-trace.json").exists());
    }
    let _ = std::fs::remove_dir_all(&default_dir);
}

#[test]
fn typed_feedback_roots_rewrite_shape_observations() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();

    let shape_user = crate::arena::arena_alloc_gc(64, 8, crate::gc::GC_TYPE_ARRAY);
    register(70, TypedFeedbackSiteKind::PropertyGet, "obj.x");
    observe(
        70,
        TypedFeedbackSiteKind::PropertyGet,
        Observation {
            source: ObservationSource::Property,
            object_addr: 0,
            shape_addr: shape_user as usize,
            key_hash: 0xABCD,
            class_id: 7,
            heap_type: crate::gc::GC_TYPE_OBJECT as u16,
            aux: 0,
            value_tag: STABLE_VALUE_POINTER,
        },
    );

    let valid_ptrs = crate::gc::build_valid_pointer_set();
    let forwarded_user = crate::arena::arena_alloc_gc_old(64, 8, crate::gc::GC_TYPE_ARRAY);
    unsafe {
        let shape_header =
            (shape_user as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::set_forwarding_address(shape_header, forwarded_user);
    }

    let mut visitor = crate::gc::RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    scan_typed_feedback_roots_mut(&mut visitor);

    let reg = registry();
    let shape_addr = reg.sites.get(&70).unwrap().observations[0].shape_addr;
    assert_eq!(shape_addr, forwarded_user as usize);
    assert_ne!(shape_addr, shape_user as usize);
}

/// Typed-feedback method dispatch must never dereference a NaN-boxed *inline*
/// receiver (SHORT_STRING / INT32) as a heap object. Only POINTER / STRING /
/// BIGINT tags carry a real, dereferenceable heap pointer in the low 48 bits;
/// SHORT_STRING (small-string optimization) and INT32 pack their payload
/// inline, so masking that payload to an "address" and probing `addr - header`
/// in `object_shape` reads unmapped memory and faults.
///
/// Regression: a short receiver string such as `email = "jo"` reaching
/// `email.includes("@")` through native-method dispatch had its inline bytes
/// (`0x..6f6a`) treated as a pointer and dereferenced (EXC_BAD_ACCESS in
/// `object_shape`). The guard returns 0 — the "not a GC object" sentinel — for
/// every non-heap-pointer tag in the NaN-box band, so dispatch falls back to the
/// safe generic path. Heap pointers (POINTER/STRING/BIGINT) still pass through.
#[test]
fn normalize_raw_object_addr_rejects_inline_nanboxed_receivers() {
    // The inline payloads below are deliberately >= the native-handle band
    // ceiling (`addr_class::HANDLE_BAND_MAX`, 0x100000) and < 2^48, i.e. they
    // alias *plausible heap addresses*. That is the case only the tag check
    // catches: the downstream `is_handle_band` / `addr >> 48` fallbacks already
    // reject tiny or out-of-range payloads, so a small value like 0x6f6a would
    // pass this test even without the guard. A real SSO string's inline bytes
    // can land anywhere in the 48-bit space, including this dereferenceable-
    // looking range — which is exactly what faulted.

    // SHORT_STRING (SSO): inline UTF-8 bytes in the low 48 bits, NOT a heap
    // pointer, but a bit pattern that looks like a valid address. Must
    // normalize to 0 rather than be dereferenced.
    let sso = crate::value::SHORT_STRING_TAG | 0x0000_5566_7788_99aa;
    assert_eq!(
        normalize_raw_object_addr(sso),
        0,
        "short-string receiver must not be dereferenced as an object",
    );

    // INT32: inline integer payload (max i32, well above the handle band).
    let int32 = crate::value::INT32_TAG | 0x7fff_ffff;
    assert_eq!(
        normalize_raw_object_addr(int32),
        0,
        "int32 receiver must not be dereferenced as an object",
    );

    // POINTER: a genuine heap address (above every native-handle band, below
    // 2^48) is preserved so real object receivers still dispatch natively.
    let heap_addr: u64 = 0x0000_0001_0000_0000; // 4 GiB — clear of the handle bands
    let ptr = crate::value::POINTER_TAG | heap_addr;
    assert_eq!(
        normalize_raw_object_addr(ptr),
        heap_addr as usize,
        "real heap-pointer receiver must pass through unchanged",
    );

    // STRING: heap string headers are dereferenceable; the tag passes through.
    let str_addr: u64 = 0x0000_0002_0000_0000;
    let strv = crate::value::STRING_TAG | str_addr;
    assert_eq!(
        normalize_raw_object_addr(strv),
        str_addr as usize,
        "heap-string receiver must pass through unchanged",
    );
}

// ---------------------------------------------------------------------------
// #6136 / #6190 — the typed-array GcHeader invariant behind the plain-array
// index guard.
//
// `plain_array_index_guard` decides whether an element read may take the inline
// plain-`ArrayHeader` raw-slot fast path, and it decides that by back-reading a
// `GcHeader` at `addr - GC_HEADER_SIZE` (see `gc_header_for_user_addr`). That is
// only sound while EVERY array-like receiver carries a real GcHeader. Before
// #6190, typed arrays under 16 KB were raw-`alloc`'d with no header, so the
// guard read whatever heap bytes happened to precede the block; when they looked
// like `GC_TYPE_ARRAY` it wrongly admitted a typed array to the plain-array fast
// path, which then reinterpreted the element bytes as f64 (denormals ~1e-312)
// and silently summed them as zero. These tests pin the invariant.
// ---------------------------------------------------------------------------

/// Every typed array — including a SMALL one, below the 16 KB threshold that
/// used to select the deleted raw-`alloc` tier — must carry a real GcHeader
/// tagged `GC_TYPE_TYPED_ARRAY`, sized to cover header + elements.
#[test]
fn typed_array_alloc_always_carries_a_real_typed_array_gc_header() {
    // (kind, length): Uint32Array(8) is 16 B header + 32 B data = 48 B payload,
    // the exact shape that took the off-GC-heap tier and caused #6136.
    let cases = [
        (crate::typedarray::KIND_UINT32, 8u32, 4usize),
        (crate::typedarray::KIND_UINT8, 8, 1),
        (crate::typedarray::KIND_FLOAT64, 4, 8),
    ];
    for (kind, length, elem_size) in cases {
        let ta = crate::typedarray::typed_array_alloc(kind, length);
        assert!(!ta.is_null(), "typed_array_alloc returned null");

        let header = unsafe {
            &*((ta as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
        };
        assert_eq!(
            header.obj_type,
            crate::gc::GC_TYPE_TYPED_ARRAY,
            "a typed array must be a GC-heap object tagged GC_TYPE_TYPED_ARRAY \
             (kind={kind}, length={length}); an off-GC-heap tier makes the \
             plain-array index guard sniff unrelated bytes at addr-8 (#6136)",
        );
        assert_ne!(
            header.obj_type,
            crate::gc::GC_TYPE_ARRAY,
            "a typed array must never be tagged GC_TYPE_ARRAY — that is exactly \
             what admits it to the inline plain-ArrayHeader raw-slot reader",
        );

        // The header's size must cover the elements, not just the header: a
        // relocation that copied only the header would leave the elements zeroed.
        let min_total = crate::gc::GC_HEADER_SIZE
            + std::mem::size_of::<crate::typedarray::TypedArrayHeader>()
            + (length as usize) * elem_size;
        assert!(
            header.size as usize >= min_total,
            "GcHeader.size ({}) must cover header + {} elements (>= {min_total})",
            header.size,
            length,
        );
    }
}

/// The load-bearing assertion: the plain-array index guard must REJECT a typed
/// array, so the read takes the boxed fallback (which dispatches typed arrays)
/// rather than the inline plain-`ArrayHeader` raw-slot path.
#[test]
fn plain_array_index_get_guard_rejects_typed_array_receivers() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(6136, TypedFeedbackSiteKind::ArrayElement, "nd.buf[i]");

    for (kind, length) in [
        (crate::typedarray::KIND_UINT32, 8u32),
        (crate::typedarray::KIND_FLOAT64, 4),
    ] {
        let ta = crate::typedarray::typed_array_alloc(kind, length);
        let ta_box = crate::value::js_nanbox_pointer(ta as i64);

        // The internal contract check — independent of feedback-site state.
        assert!(
            !plain_array_index_guard(ta as *const ArrayHeader, 0, true),
            "plain_array_index_guard must reject a typed array (kind={kind})",
        );

        // The guard as codegen actually calls it.
        assert_eq!(
            js_typed_feedback_plain_array_index_get_guard(6136, ta_box, 0, 1),
            0,
            "the emitted guard must reject a typed-array receiver (kind={kind}) \
             — admitting it reads elements as f64 denormals and sums them as 0 (#6136)",
        );
    }
}

/// Positive control: the guard still ADMITS a genuine plain array, so the test
/// above proves rejection of typed arrays rather than a guard that never passes.
#[test]
fn plain_array_index_get_guard_still_accepts_plain_arrays() {
    let _guard = TYPED_FEEDBACK_TEST_LOCK.lock().unwrap();
    reset_typed_feedback_for_tests();
    register(6137, TypedFeedbackSiteKind::ArrayElement, "arr[i]");

    let arr = crate::array::js_array_alloc(4);
    for i in 0..4 {
        crate::array::js_array_push_f64(arr, i as f64);
    }
    let arr_box = crate::value::js_nanbox_pointer(arr as i64);

    assert!(
        plain_array_index_guard(arr as *const ArrayHeader, 0, true),
        "plain_array_index_guard must accept a plain array",
    );
    assert_eq!(
        js_typed_feedback_plain_array_index_get_guard(6137, arr_box, 0, 1),
        1,
        "the emitted guard must keep admitting plain arrays to the fast path",
    );
}
