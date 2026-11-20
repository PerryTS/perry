use super::*;

fn construct(args: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let args: Vec<_> = args.iter().map(|&v| scope.root_nanbox_f64(v)).collect();
    crate::event_target_hook_slots::install();
    crate::object::js_get_global_this();
    let args: Vec<_> = args.iter().map(|v| v.get_nanbox_f64()).collect();
    let constructor = crate::object::builtin_constructor_value("MessageEvent");
    unsafe { crate::object::js_new_function_construct(constructor, args.as_ptr(), args.len()) }
}

fn get(value: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let name = key(name);
    js_object_get_field_by_name_f64(
        value_as_ptr::<ObjectHeader>(value.get_nanbox_f64()).unwrap(),
        name,
    )
}

#[test]
fn message_event_constructor_and_legacy_use_canonical_class_zero_values() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let original = scope.root_nanbox_f64(construct(&[string_value(b"old")]));
    let ptr = value_as_ptr::<ObjectHeader>(original.get_nanbox_f64()).unwrap();
    assert_eq!(unsafe { (*ptr).class_id }, 0);
    assert!(is_event_instance(ptr));
    assert_eq!(get(original.get_nanbox_f64(), b"data").to_bits(), TAG_NULL);
    let ports = get(original.get_nanbox_f64(), b"ports");
    assert_eq!(
        js_array_length(value_as_ptr::<ArrayHeader>(ports).unwrap()),
        0
    );
    let args = [string_value(b"fresh")];
    let fresh = unsafe {
        crate::object::js_native_call_method(
            original.get_nanbox_f64(),
            b"initMessageEvent".as_ptr().cast(),
            16,
            args.as_ptr(),
            1,
        )
    };
    let fresh = scope.root_nanbox_f64(fresh);
    assert_ne!(original.get_nanbox_u64(), fresh.get_nanbox_u64());
    assert_eq!(
        crate::value::boxed_text::get_string_content(get(original.get_nanbox_f64(), b"type")),
        "old"
    );
    assert_eq!(
        crate::value::boxed_text::get_string_content(get(fresh.get_nanbox_f64(), b"type")),
        "fresh"
    );
    let error = crate::exception::catch_js_throw(|| unsafe {
        crate::closure::js_native_call_value(
            get(original.get_nanbox_f64(), b"initMessageEvent"),
            crate::closure::JsThis::from_f64(null_value()),
            std::ptr::null(),
            0,
        )
    })
    .unwrap_err();
    assert_eq!(
        crate::value::boxed_text::get_string_content(get(error, b"code")),
        "ERR_INVALID_THIS"
    );
}

#[test]
fn message_event_delivery_cleanup_preserves_exception_identity() {
    crate::event_target_hook_slots::install();
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(crate::event_target_hook_slots::message_event(42.0));
    let target = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    let sentinel = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    let result = crate::exception::catch_js_throw(|| {
        crate::event_target_hook_slots::with_message_event_dispatch(
            event.get_nanbox_f64(),
            boxed_ptr(target.get_raw_mut_ptr::<ObjectHeader>()),
            || {
                assert_eq!(get(event.get_nanbox_f64(), b"data"), 42.0);
                assert_eq!(get(event.get_nanbox_f64(), b"eventPhase"), 2.0);
                assert_eq!(
                    get(event.get_nanbox_f64(), b"currentTarget").to_bits(),
                    boxed_ptr(target.get_raw_mut_ptr::<ObjectHeader>()).to_bits()
                );
                assert_eq!(
                    get(event.get_nanbox_f64(), b"isTrusted").to_bits(),
                    JSValue::bool(false).bits()
                );
                unsafe {
                    crate::object::js_native_call_method(
                        event.get_nanbox_f64(),
                        b"stopImmediatePropagation".as_ptr().cast(),
                        24,
                        std::ptr::null(),
                        0,
                    );
                }
                crate::exception::test_throw(boxed_ptr(sentinel.get_raw_mut_ptr::<ObjectHeader>()));
            },
        );
    });
    assert_eq!(
        result.unwrap_err().to_bits(),
        boxed_ptr(sentinel.get_raw_mut_ptr::<ObjectHeader>()).to_bits()
    );
    assert_eq!(
        get(event.get_nanbox_f64(), b"currentTarget").to_bits(),
        TAG_NULL
    );
    assert_eq!(get(event.get_nanbox_f64(), b"eventPhase"), 0.0);
    assert_eq!(
        get(event.get_nanbox_f64(), b"target").to_bits(),
        boxed_ptr(target.get_raw_mut_ptr::<ObjectHeader>()).to_bits()
    );
    crate::event_target_hook_slots::with_message_event_dispatch(
        event.get_nanbox_f64(),
        boxed_ptr(target.get_raw_mut_ptr::<ObjectHeader>()),
        || panic!("stopped delivery must not invoke another callback"),
    );
}

#[test]
fn node_delivery_defaults_data_without_changing_lossless_factories() {
    crate::event_target_hook_slots::install();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(boxed_ptr(js_object_alloc(0, 0)));
    let ports = scope.root_nanbox_f64(boxed_ptr(js_array_alloc(0)));
    let values: Vec<_> = [
        (undefined_value(), null_value()),
        (null_value(), null_value()),
        (7.0, 7.0),
        (bool_value(false), bool_value(false)),
        (0.0, 0.0),
        (object.get_nanbox_f64(), object.get_nanbox_f64()),
    ]
    .into_iter()
    .map(|(data, expected)| (scope.root_nanbox_f64(data), scope.root_nanbox_f64(expected)))
    .collect();
    for (data, expected) in values {
        for supplied_ports in [false, true] {
            let node = scope.root_nanbox_f64(crate::event_target_hook_slots::node_message_event(
                data.get_nanbox_f64(),
                supplied_ports.then(|| ports.get_nanbox_f64()),
            ));
            assert_eq!(
                get(node.get_nanbox_f64(), b"data").to_bits(),
                expected.get_nanbox_f64().to_bits()
            );
            let actual_ports = scope.root_nanbox_f64(get(node.get_nanbox_f64(), b"ports"));
            assert!(value_as_ptr::<ArrayHeader>(actual_ports.get_nanbox_f64()).is_some());
            assert_eq!(
                crate::object::js_object_is_frozen(actual_ports.get_nanbox_f64()).to_bits(),
                JSValue::bool(true).bits()
            );
            if supplied_ports {
                assert_eq!(actual_ports.get_nanbox_u64(), ports.get_nanbox_u64());
            }
            let raw = scope.root_nanbox_f64(if supplied_ports {
                crate::event_target_hook_slots::message_event_with_ports(
                    data.get_nanbox_f64(),
                    ports.get_nanbox_f64(),
                )
            } else {
                crate::event_target_hook_slots::message_event(data.get_nanbox_f64())
            });
            assert_eq!(
                get(raw.get_nanbox_f64(), b"data").to_bits(),
                data.get_nanbox_f64().to_bits()
            );
        }
    }
}
