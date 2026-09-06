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
