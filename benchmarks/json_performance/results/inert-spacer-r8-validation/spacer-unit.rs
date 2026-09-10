
#[test]
fn full_entry_zero_and_boolean_spacing_preserves_output_and_replacer() {
    unsafe fn parse(source: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(source.as_ptr(), source.len() as u32);
        f64::from_bits(crate::json::test_json_parse_direct(ptr).bits())
    }
    unsafe fn output(result: i64) -> Vec<u8> {
        let mut scratch = [0; SHORT_STRING_MAX_LEN];
        let (bytes, length) = crate::string::str_bytes_from_jsvalue(
            f64::from_bits(result as u64), &mut scratch,
        ).expect("stringify must produce a string");
        std::slice::from_raw_parts(bytes, length as usize).to_vec()
    }
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let ignored = [0.0, -0.0, f64::from_bits(TAG_FALSE), f64::from_bits(TAG_TRUE)];
    for source in ["null", "true", "0", r#""a""#, "{}", "[]", r#"{"a":1,"b":[2,3]}"#] {
        unsafe {
            let roots = crate::gc::RuntimeHandleScope::new();
            let value = roots.root_nanbox_f64(parse(source));
            for spacer in ignored {
                let actual = output(crate::json::js_json_stringify_full(value.get_nanbox_f64(), undef, spacer));
                assert_eq!(actual, source.as_bytes(), "spacer bits {:016x}", spacer.to_bits());
            }
        }
    }
    unsafe {
        let roots = crate::gc::RuntimeHandleScope::new();
        let value = roots.root_nanbox_f64(parse(r#"{"a":1,"b":2}"#));
        let keys = roots.root_nanbox_f64(parse(r#"["b"]"#));
        for spacer in ignored {
            assert_eq!(output(crate::json::js_json_stringify_full(value.get_nanbox_f64(), keys.get_nanbox_f64(), spacer)), br#"{"b":2}"#);
        }
        for (spacer, expected) in [(1.0, "{\n \"a\": 1,\n \"b\": 2\n}"), (2.0, "{\n  \"a\": 1,\n  \"b\": 2\n}")] {
            assert_eq!(output(crate::json::js_json_stringify_full(value.get_nanbox_f64(), undef, spacer)), expected.as_bytes());
        }
    }
}
