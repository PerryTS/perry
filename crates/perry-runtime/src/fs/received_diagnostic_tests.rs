use super::*;

fn text(value: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64)
}

fn check(value: f64, expected: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    assert_eq!(describe_received(value.get_nanbox_f64()), expected);
    let result = crate::validators::js_runtime_describe_received(value.get_nanbox_f64());
    assert!(JSValue::from_bits(result.to_bits()).is_any_string());
    assert_eq!(read_js_string_pub(result), expected);
}

#[test]
fn received_primitives_and_numbers() {
    for (bits, expected) in [
        (crate::value::TAG_UNDEFINED, "undefined"),
        (crate::value::TAG_NULL, "null"),
        (crate::value::TAG_FALSE, "type boolean (false)"),
        (crate::value::TAG_TRUE, "type boolean (true)"),
    ] {
        check(f64::from_bits(bits), expected);
    }
    for (value, expected) in [
        (f64::NAN, "NaN"),
        (-0.0, "-0"),
        (1e21, "1e+21"),
        (1e20, "100000000000000000000"),
        (1e-7, "1e-7"),
        (5e-324, "5e-324"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        check(value, &format!("type number ({expected})"));
    }
    check(
        f64::from_bits(JSValue::int32(-42).bits()),
        "type number (-42)",
    );
}

#[test]
fn received_utf16_and_quotes() {
    for len in [27, 28, 29] {
        let input = "a".repeat(len);
        let expected = if len > 28 {
            "a".repeat(25) + "..."
        } else {
            input.clone()
        };
        check(text(&input), &format!("type string ('{expected}')"));
    }
    check(text("a'\"\\\n"), "type string (\"a'\\\"\\\\\\n\")");
    check(
        text(&("'".to_owned() + "a".repeat(23).as_str() + "😀abcd")),
        &format!("type string (\"'{}\\ud83d...\")", "a".repeat(23)),
    );
    check(text("'😀"), "type string (\"'😀\")");
    for (bytes, expected) in [
        (b"'\xed\xa0\xbd".as_slice(), "type string (\"'\\ud83d\")"),
        (b"'\xed\xb1\x8d".as_slice(), "type string (\"'\\udc4d\")"),
        (
            b"'\"\\\n\xed\xb1\x8d".as_slice(),
            "type string (\"'\\\"\\\\\\n\\udc4d\")",
        ),
    ] {
        let ptr = crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32);
        check(crate::value::js_nanbox_string(ptr as i64), expected);
    }
    for prefix_len in [22, 23, 24] {
        let prefix = "'".to_owned() + "a".repeat(prefix_len).as_str();
        let suffix = match prefix_len {
            22 => "😀...",
            23 => "\\ud83d...",
            _ => "...",
        };
        check(
            text(&(prefix.clone() + "😀abcd")),
            &format!("type string (\"{prefix}{suffix}\")"),
        );
    }
}

#[test]
fn received_abi_preserves_split_surrogate() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(text(&("a".repeat(24) + "😀abcd")));
    let result = scope.root_nanbox_f64(crate::validators::js_runtime_describe_received(
        input.get_nanbox_f64(),
    ));
    let ptr =
        crate::value::js_get_string_pointer_unified(result.get_nanbox_f64()) as *const StringHeader;
    assert_eq!(crate::string::js_string_char_code_at(ptr, 38), 55357.0);
    assert_eq!(crate::string::js_string_char_code_at(ptr, 39), b'.' as f64);
    let error = crate::exception::catch_js_throw(|| {
        validate_function("cb", input.get_nanbox_f64());
    })
    .expect_err("invalid callback must throw");
    let error = scope.root_nanbox_f64(error);
    let message = scope.root_string_ptr(crate::error::js_error_get_message(
        JSValue::from_bits(error.get_nanbox_u64()).as_pointer::<crate::error::ErrorHeader>()
            as *mut crate::error::ErrorHeader,
    ));
    let ptr = message.with_const_ptr(|s: *const StringHeader| s);
    let prefix = "The \"cb\" argument must be of type function. Received type string ('";
    assert_eq!(
        crate::string::js_string_char_code_at(ptr, prefix.len() as i32 + 24),
        55357.0
    );
}
