use super::*;

#[test]
fn json_template_capture_handles_growing_and_shrinking_initialized_prefixes() {
    unsafe {
        for fields in (1..=8).chain((1..8).rev()) {
            for elements in 0..=8 {
                let members: Vec<String> = (0..fields)
                    .map(|index| {
                        let values: Vec<String> = (0..elements)
                            .map(|n| format!("\"value-{index}-{n}\""))
                            .collect();
                        format!("\"k{index}\":[{}]", values.join(","))
                    })
                    .collect();
                let expected = format!("{{{}}}", members.join(","));
                let input = " ".repeat(65) + expected.as_str();
                let scope = crate::gc::RuntimeHandleScope::new();
                let source = scope.root_string_ptr(crate::js_string_from_bytes(
                    input.as_ptr(),
                    input.len() as u32,
                ));
                let first = source.with_const_ptr(|source| crate::json::js_json_parse(source));
                let first = scope.root_nanbox_u64(first.bits());
                assert!(source.with_const_ptr(|source| {
                    test_parse_object_template_matches(source, input.len())
                }));
                let second = source.with_const_ptr(|source| crate::json::js_json_parse(source));
                assert_ne!(first.get_nanbox_u64(), second.bits());
                let output = crate::json::js_json_stringify(
                    f64::from_bits(second.bits()),
                    crate::json::TYPE_UNKNOWN,
                );
                assert_eq!(
                    crate::json::str_from_header(output),
                    Some(expected.as_str())
                );
            }
        }
    }
}

#[test]
fn json_template_rejected_partial_capture_preserves_previous_plan() {
    unsafe {
        let expected =
            r#"{"text":"previous-plan-must-survive-a-rejected-capture","items":["one","two"]}"#;
        let scope = crate::gc::RuntimeHandleScope::new();
        let source = scope.root_string_ptr(crate::js_string_from_bytes(
            expected.as_ptr(),
            expected.len() as u32,
        ));
        source.with_const_ptr(|source| crate::json::js_json_parse(source));
        for unsupported in [r#"{"nested":1}"#, "[0,1,2,3,4,5,6,7,8]", "[[1]]"] {
            let input = format!(
                "{{\"a\":\"long-initialized-prefix-before-rejection\",\"b\":[1,2],\"last\":{unsupported}}}"
            );
            let other = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
            crate::json::js_json_parse(other);
            assert!(source.with_const_ptr(|source| {
                test_parse_object_template_matches(source, expected.len())
            }));
            let value = source.with_const_ptr(|source| crate::json::js_json_parse(source));
            let output = crate::json::js_json_stringify(
                f64::from_bits(value.bits()),
                crate::json::TYPE_UNKNOWN,
            );
            assert_eq!(crate::json::str_from_header(output), Some(expected));
        }
    }
}
