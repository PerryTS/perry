use super::*;

unsafe fn parse(text: &str) -> JSValue {
    let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    super::super::test_json_parse_direct(source)
}

unsafe fn check(text: &str) {
    let value = parse(text);
    let result = try_object(value.bits()).expect("bounded primitive record");
    let header = result.as_string_ptr();
    assert_eq!(
        std::slice::from_raw_parts(
            crate::string::string_data(header),
            (*header).byte_len as usize
        ),
        text.as_bytes()
    );
    assert_eq!((*header).utf16_len as usize, text.encode_utf16().count());
}

unsafe fn output_bytes(value: JSValue) -> Vec<u8> {
    let result = try_object(value.bits()).expect("bounded primitive record");
    let header = result.as_string_ptr();
    std::slice::from_raw_parts(
        crate::string::string_data(header),
        (*header).byte_len as usize,
    )
    .to_vec()
}

unsafe fn clear_key_prefix_cache() {
    KEY_PREFIX_CACHE.with(|cache| {
        *cache.get() = [EMPTY_KEY_PREFIX_PLAN; KEY_PREFIX_CACHE_SLOTS];
    });
}

#[test]
fn record_final_output_preserves_scalars_arrays_and_utf16_lengths() {
    unsafe {
        check(
            r#"{"id":42,"name":"user_42","email":"user_42@example.com","active":false,"score":63,"tags":["tag_2","tag_0"]}"#,
        );
        check(r#"{"empty":[],"a":[true,false,null],"b":[1,0,1.25],"c":"東京🙂"}"#);
        for unit in ["a", "é", "東京", "🙂"] {
            check(&format!(
                "{{\"tags\":[\"{}\",\"tail\"],\"id\":1}}",
                unit.repeat(8192)
            ));
        }
        let fields = (0..MAX_FIELDS)
            .map(|i| format!("\"field_{i}\":[1,2]"))
            .collect::<Vec<_>>()
            .join(",");
        check(&format!("{{{fields}}}"));
    }
}

#[test]
fn cached_key_prefixes_reuse_only_shape_facts() {
    unsafe {
        clear_key_prefix_cache();
        let first_text = r#"{"id":1,"name":"first","active":true,"score":2,"tags":["a"]}"#;
        let second_text =
            r#"{"id":99,"name":"second","active":false,"score":3.5,"tags":["b","c"]}"#;
        let first = parse(first_text);
        let first_shape =
            crate::object::shapes::object_shape_stamp(first.as_pointer::<crate::ObjectHeader>());
        assert_eq!(output_bytes(first), first_text.as_bytes());

        let second = parse(second_text);
        assert_eq!(
            crate::object::shapes::object_shape_stamp(second.as_pointer::<crate::ObjectHeader>()),
            first_shape,
            "equal ordered keys must reuse the same immutable shape"
        );
        assert_eq!(output_bytes(second), second_text.as_bytes());
    }
}

#[test]
fn cached_key_prefixes_follow_shape_changes_and_slot_replacement() {
    unsafe {
        clear_key_prefix_cache();
        let scope = crate::gc::RuntimeHandleScope::new();
        let original_text = r#"{"a":1,"b":2,"c":3,"d":4,"e":5}"#;
        // Reserve one extra physical slot so the shape can grow while the
        // bounded inline-record path remains eligible.
        let original = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 6));
        for (index, name) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            let key = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
            original.with_mut_ptr(|original| {
                key.with_const_ptr(|key| {
                    crate::object::js_object_set_field_by_name(original, key, (index + 1) as f64);
                })
            });
        }
        let original_bits = original.with_const_ptr(|original| make_pointer_bits(original));
        assert_eq!(
            output_bytes(JSValue::from_bits(original_bits)),
            original_text.as_bytes()
        );
        assert_eq!(
            output_bytes(JSValue::from_bits(original_bits)),
            original_text.as_bytes(),
            "the second observation installs the prefix plan"
        );
        let before = original.with_const_ptr(|original: *const crate::ObjectHeader| {
            crate::object::shapes::object_shape_stamp(original)
        });
        let key = scope.root_string_ptr(js_string_from_bytes(b"later".as_ptr(), 5));
        original.with_mut_ptr(|original| {
            key.with_const_ptr(|key| {
                crate::object::js_object_set_field_by_name(original, key, 6.0);
            })
        });
        let changed =
            original.with_const_ptr(|original| JSValue::from_bits(make_pointer_bits(original)));
        assert_ne!(
            crate::object::shapes::object_shape_stamp(changed.as_pointer::<crate::ObjectHeader>()),
            before
        );
        assert_eq!(
            output_bytes(changed),
            br#"{"a":1,"b":2,"c":3,"d":4,"e":5,"later":6}"#
        );

        // More unique five-field shapes than cache slots guarantee a direct-map
        // collision. Alternating the colliding shapes must rebuild complete
        // prefixes rather than combining either entry with stale bytes.
        let mut seen: [Option<(u32, String)>; KEY_PREFIX_CACHE_SLOTS] = Default::default();
        let mut collision = None;
        for n in 0..KEY_PREFIX_CACHE_SLOTS + 1 {
            let text = format!(r#"{{"key_{n}":{n},"b":2,"c":3,"d":4,"e":5}}"#);
            let value = parse(&text);
            let shape = crate::object::shapes::object_shape_stamp(
                value.as_pointer::<crate::ObjectHeader>(),
            );
            let cache_slot =
                (shape.wrapping_mul(0x9e37_79b9) as usize) & (KEY_PREFIX_CACHE_SLOTS - 1);
            if let Some((other_shape, other_text)) = seen[cache_slot].take() {
                if other_shape != shape {
                    collision = Some((other_text, text));
                    break;
                }
            }
            seen[cache_slot] = Some((shape, text));
        }
        let (left, right) = collision.expect("pigeonhole collision");
        for text in [&left, &left, &right, &right, &left, &left] {
            assert_eq!(output_bytes(parse(text)), text.as_bytes());
        }
    }
}

#[test]
fn record_final_output_declines_before_allocating_on_ineligible_fields() {
    unsafe {
        for text in [
            "[]",
            "{}",
            "{\"x\":{}}",
            "{\"x\":[{}]}",
            "{\"x\":[[1]]}",
            "{\"x\":\"\\ud800\"}",
            "{\"2\":2,\"1\":1}",
        ] {
            let value = parse(text);
            let before = crate::arena::arena_total_bytes();
            assert!(try_object(value.bits()).is_none(), "{text}");
            assert_eq!(crate::arena::arena_total_bytes(), before);
        }
        let fields = (0..MAX_FIELDS + 1)
            .map(|i| format!("\"f{i}\":1"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(try_object(parse(&format!("{{{fields}}}")).bits()).is_none());
        for len in [MAX_ELEMENTS + 1, 32] {
            let values = vec!["1"; len].join(",");
            assert!(try_object(parse(&format!("{{\"a\":[{values}]}}")).bits()).is_none());
        }
        // The element budget is shared across all arrays, not per field.
        let values = vec!["1"; 9].join(",");
        assert!(
            try_object(parse(&format!("{{\"a\":[{values}],\"b\":[{values}]}}")).bits()).is_none()
        );
    }
}

#[test]
fn record_final_output_declines_array_expandos_and_undefined() {
    unsafe {
        let value = parse("{\"id\":1,\"tags\":[1,2]}");
        let obj = value.as_pointer::<crate::ObjectHeader>();
        let arr = crate::object::js_object_get_field(obj, 1).as_pointer::<crate::ArrayHeader>()
            as *mut crate::ArrayHeader;
        crate::array::js_array_set(arr, 1, JSValue::undefined());
        assert!(try_object(value.bits()).is_none());
        crate::array::js_array_set(arr, 1, JSValue::number(2.0));
        let key = js_string_from_bytes(b"toJSON".as_ptr(), 6);
        crate::array::array_named_property_set(arr, key, 1.0);
        assert!(try_object(value.bits()).is_none());
    }
}

#[test]
fn fused_key_checks_preserve_numeric_order_and_native_forwarding_fallbacks() {
    unsafe {
        let mut keys = vec!["0", "1", "4294967294", "toJSON", "__module__"];
        keys.push(std::str::from_utf8(crate::object::FETCH_SUBCLASS_HANDLE_FIELD).unwrap());
        #[cfg(feature = "temporal")]
        keys.push(std::str::from_utf8(crate::object::TEMPORAL_SUBCLASS_CELL_FIELD).unwrap());
        for key in keys {
            let text = format!("{{\"z\":2,{}:1}}", serde_json::to_string(key).unwrap());
            let value = parse(&text);
            let before = crate::arena::arena_total_bytes();
            assert!(try_object(value.bits()).is_none(), "{key}");
            assert!(
                super::super::stringify_flat::try_object(value.bits()).is_none(),
                "{key}"
            );
            assert_eq!(crate::arena::arena_total_bytes(), before, "{key}");
        }
        // Eligibility uses decoded property names, including escaped markers.
        let value = parse(r#"{"to\u004aSON":1}"#);
        assert!(try_object(value.bits()).is_none());
        assert!(super::super::stringify_flat::try_object(value.bits()).is_none());
    }
}

#[test]
fn fused_key_checks_accept_nonindices_and_marker_neighbours() {
    unsafe {
        for key in [
            "",
            "00",
            "01",
            "-0",
            "1e0",
            "4294967295",
            "4294967296",
            "18446744073709551616",
            "toJson",
            "toJSONx",
            "__module___",
            "東京",
            "a\n\"b",
        ] {
            let text = format!("{{\"z\":2,{}:1}}", serde_json::to_string(key).unwrap());
            check(&text);
            let value = parse(&text);
            let output = super::super::stringify_flat::try_object(value.bits()).expect(key);
            let header = output.as_string_ptr();
            assert_eq!(
                std::slice::from_raw_parts(
                    crate::string::string_data(header),
                    (*header).byte_len as usize
                ),
                text.as_bytes(),
                "{key}"
            );
        }
    }
}
