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
