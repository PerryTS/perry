use super::*;

fn text(value: JSValue) -> String {
    assert!(value.is_short_string());
    let mut bytes = [0; SHORT_STRING_MAX_LEN];
    let n = value.short_string_to_buf(&mut bytes);
    String::from_utf8(bytes[..n].to_vec()).unwrap()
}

#[test]
fn short_primitive_escaping_matches_json_for_all_ascii_pairs() {
    for a in 0..=127u8 {
        for b in 0..=127u8 {
            let bytes = [a, b];
            let source = std::str::from_utf8(&bytes).unwrap();
            let expected = serde_json::to_string(source).unwrap();
            let value = JSValue::short_string_unchecked(&bytes);
            let found = try_primitive(value.bits());
            assert_eq!(found.is_some(), expected.len() <= SHORT_STRING_MAX_LEN);
            if let Some(found) = found {
                assert_eq!(text(found), expected);
            }
        }
    }
    for source in ["", "a", "abc", "abcd", "a\nb", "é", "東", "🙂"] {
        let value = JSValue::short_string_unchecked(source.as_bytes());
        if let Some(found) = try_primitive(value.bits()) {
            assert_eq!(text(found), serde_json::to_string(source).unwrap());
        }
    }
}

#[test]
fn heap_and_inline_short_inputs_produce_the_same_json() {
    for source in ["", "a", "ab", "abc", "\n", "a\n", "\"", "\\"] {
        let ptr = crate::string::js_string_from_bytes(source.as_ptr(), source.len() as u32);
        let value = JSValue::string_ptr(ptr);
        assert_eq!(
            text(try_primitive(value.bits()).unwrap()),
            serde_json::to_string(source).unwrap()
        );
    }
    assert!(try_primitive(crate::value::TAG_UNDEFINED).is_none());
    assert!(try_primitive(1.0f64.to_bits()).is_none());
}

#[test]
fn full_entry_returns_boxed_inline_output_for_short_results() {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    for source in ["null", "true", "false", "0", "1.5", "{}", "[]", "\"a\""] {
        let ptr = crate::string::js_string_from_bytes(source.as_ptr(), source.len() as u32);
        unsafe {
            let value = crate::json::test_json_parse_direct(ptr);
            let result =
                crate::json::js_json_stringify_full(f64::from_bits(value.bits()), undef, undef);
            assert_eq!(text(JSValue::from_bits(result as u64)), source);
        }
    }
}

#[test]
fn full_entry_zero_and_boolean_spacing_preserves_output_and_replacer() {
    unsafe fn parse(source: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(source.as_ptr(), source.len() as u32);
        f64::from_bits(crate::json::test_json_parse_direct(ptr).bits())
    }
    unsafe fn output(result: i64) -> Vec<u8> {
        let mut scratch = [0; SHORT_STRING_MAX_LEN];
        let (bytes, length) =
            crate::string::str_bytes_from_jsvalue(f64::from_bits(result as u64), &mut scratch)
                .expect("stringify must produce a string");
        std::slice::from_raw_parts(bytes, length as usize).to_vec()
    }
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let ignored = [
        0.0,
        -0.0,
        f64::from_bits(TAG_FALSE),
        f64::from_bits(TAG_TRUE),
    ];
    for source in [
        "null",
        "true",
        "0",
        r#""a""#,
        "{}",
        "[]",
        r#"{"a":1,"b":[2,3]}"#,
    ] {
        unsafe {
            let roots = crate::gc::RuntimeHandleScope::new();
            let value = roots.root_nanbox_f64(parse(source));
            for spacer in ignored {
                let actual = output(crate::json::js_json_stringify_full(
                    value.get_nanbox_f64(),
                    undef,
                    spacer,
                ));
                assert_eq!(
                    actual,
                    source.as_bytes(),
                    "spacer bits {:016x}",
                    spacer.to_bits()
                );
            }
        }
    }
    unsafe {
        let roots = crate::gc::RuntimeHandleScope::new();
        let value = roots.root_nanbox_f64(parse(r#"{"a":1,"b":2}"#));
        let keys = roots.root_nanbox_f64(parse(r#"["b"]"#));
        for spacer in ignored {
            assert_eq!(
                output(crate::json::js_json_stringify_full(
                    value.get_nanbox_f64(),
                    keys.get_nanbox_f64(),
                    spacer
                )),
                br#"{"b":2}"#
            );
        }
        for (spacer, expected) in [
            (1.0, "{\n \"a\": 1,\n \"b\": 2\n}"),
            (2.0, "{\n  \"a\": 1,\n  \"b\": 2\n}"),
        ] {
            assert_eq!(
                output(crate::json::js_json_stringify_full(
                    value.get_nanbox_f64(),
                    undef,
                    spacer
                )),
                expected.as_bytes()
            );
        }
    }
}
