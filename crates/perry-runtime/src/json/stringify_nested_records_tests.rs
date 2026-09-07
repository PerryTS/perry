use super::*;

unsafe fn with_array(text: &str, f: impl FnOnce(*const crate::ArrayHeader)) {
    let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let value = crate::json::test_json_parse_direct(source);
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(value.as_pointer::<crate::ArrayHeader>());
    let first = crate::array::js_array_get(input.get_raw_const_ptr(), 0);
    let first = scope.root_nanbox_u64(first.bits());
    invalidate_object_proto_tojson_state();
    assert!(
        super::super::stringify_tojson_probe::to_json_definitely_absent(
            (first.get_nanbox_u64() & POINTER_MASK) as *const u8
        )
    );
    f(input.get_raw_const_ptr());
}

fn collections() -> u64 {
    let mut value = 0;
    crate::gc::js_gc_stats(&mut value, std::ptr::null_mut(), std::ptr::null_mut());
    value
}

#[test]
fn json_nested_records_reuses_multiple_shapes_without_managed_intermediates() {
    unsafe {
        let text = format!("[{}]", (0..160).map(|i| {
            format!("{{\"id\":{i},\"field_{}\":{},\"value\":\"東京🙂\\n\",\"nested\":{{\"yes\":true,\"escaped\\\"\":null}}}}", i % 32, i * 2)
        }).collect::<Vec<_>>().join(","));
        with_array(&text, |arr| {
            let bytes = crate::arena::arena_total_bytes();
            let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
            let count = collections();
            let stack = STRINGIFY_STACK.with(|s| s.borrow().len());
            set_to_json_key_str("unchanged key");
            let mut output = String::with_capacity(text.len());
            for _ in 0..20 {
                output.clear();
                assert!(try_emit(arr, &mut output, 0));
                assert_eq!(output, text);
            }
            assert_eq!(crate::arena::arena_total_bytes(), bytes);
            assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
            assert_eq!(collections(), count);
            assert_eq!(STRINGIFY_STACK.with(|s| s.borrow().len()), stack);
            assert_eq!(TO_JSON_KEY.with(|s| s.borrow().clone()), "unchanged key");
        });
    }
}

#[test]
fn json_nested_records_declines_unsupported_late_values_and_restores_prefix() {
    unsafe {
        for last in [
            r#"{"id":1,"nested":{"deeper":{"yes":true}}}"#,
            r#"{"id":1,"nested":[true]}"#,
            r#"{"id":1,"nested":{"2":true,"1":false}}"#,
            "null",
        ] {
            let text = format!(r#"[{{"id":0,"nested":{{"yes":true}}}},{last}]"#);
            with_array(&text, |arr| {
                let before = crate::arena::arena_total_bytes();
                let mut output = String::from("already emitted:");
                assert!(!try_emit(arr, &mut output, 0), "{text}");
                assert_eq!(output, "already emitted:");
                assert_eq!(crate::arena::arena_total_bytes(), before);
            });
        }
    }
}

#[test]
fn json_nested_records_observes_mutation_between_attempts() {
    unsafe {
        with_array(
            r#"[{"id":0,"nested":{"yes":true}},{"id":1,"nested":{"yes":true}}]"#,
            |arr| {
                let mut output = String::new();
                assert!(try_emit(arr, &mut output, 0));
                let second = crate::array::js_array_get(arr, 1).as_pointer::<crate::ObjectHeader>();
                crate::object::js_object_set_field(
                    second.cast_mut(),
                    0,
                    JSValue::from_bits(TAG_UNDEFINED),
                );
                output.clear();
                assert!(!try_emit(arr, &mut output, 0));
                assert!(output.is_empty());
                crate::object::js_object_set_field(second.cast_mut(), 0, JSValue::number(19.0));
                assert!(try_emit(arr, &mut output, 0));
                assert!(output.contains("\"id\":19"));
            },
        );
    }
}

#[test]
fn json_nested_records_declines_cold_prototype_lookup_without_collection() {
    unsafe {
        with_array(r#"[{"nested":{"x":1}},{"nested":{"x":2}}]"#, |arr| {
            let cached = CACHED_OBJECT_PROTO_BITS.with(|c| c.replace(0));
            let state = OBJECT_PROTO_TOJSON_STATE
                .with(|c| c.replace(super::super::stringify_tojson_probe::PROTO_TOJSON_DIRTY));
            let bytes = crate::arena::arena_total_bytes();
            let count = collections();
            let mut output = String::from("prefix");
            let result = try_emit(arr, &mut output, 0);
            CACHED_OBJECT_PROTO_BITS.with(|c| c.set(cached));
            OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(state));
            assert!(!result);
            assert_eq!(output, "prefix");
            assert_eq!(crate::arena::arena_total_bytes(), bytes);
            assert_eq!(collections(), count);
        });
    }
}

#[test]
fn json_nested_records_caps_native_shape_and_prefix_storage() {
    unsafe {
        let text = format!(
            "[{}]",
            (0..MAX_SHAPES + 1)
                .map(|i| format!("{{\"field_{i}\":1,\"nested\":{{\"yes\":true}}}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        with_array(&text, |arr| {
            let mut output = String::from("prefix");
            assert!(!try_emit(arr, &mut output, 0));
            assert_eq!(output, "prefix");
        });
        let huge_key = "x".repeat(MAX_PREFIX_BYTES);
        let text = format!("[{{\"nested\":{{\"yes\":true}}}},{{\"{huge_key}\":1}}]");
        with_array(&text, |arr| {
            let mut output = String::from("prefix");
            assert!(!try_emit(arr, &mut output, 0));
            assert_eq!(output, "prefix");
        });
    }
}

#[test]
fn json_nested_records_preserves_empty_leaves_and_depth_fallback() {
    unsafe {
        let text = r#"[{"a":{}},{"b":{"s":"a","n":0,"b":false,"z":null}}]"#;
        with_array(text, |arr| {
            let mut output = String::new();
            assert!(try_emit(arr, &mut output, 0));
            assert_eq!(output, text);
            output.clear();
            assert!(!try_emit(
                arr,
                &mut output,
                super::super::stringify::MAX_STRINGIFY_NESTING_DEPTH as u32 - 2
            ));
            assert!(output.is_empty());
        });
    }
}
