//! Node-compatible `assert` module runtime entry points.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

fn undefined_f64() -> f64 {
    f64::from_bits(crate::value::JSValue::undefined().bits())
}

fn string_f64(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

fn value_to_string(value: f64) -> String {
    unsafe {
        let ptr = crate::value::js_jsvalue_to_string(value);
        if ptr.is_null() {
            return String::new();
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn is_null_or_undefined(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    jv.is_null() || jv.is_undefined()
}

fn is_error_value(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR
    }
}

fn regex_test_value(pattern: f64, input: f64) -> Option<bool> {
    let jv = crate::value::JSValue::from_bits(pattern.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if !crate::regex::is_regex_pointer(ptr) {
        return None;
    }
    let input_string = value_to_string(input);
    let input_ptr =
        crate::string::js_string_from_bytes(input_string.as_ptr(), input_string.len() as u32);
    Some(crate::regex::js_regexp_test(ptr as *const crate::regex::RegExpHeader, input_ptr) != 0)
}

fn assertion_message(custom_message: f64, fallback: &str) -> String {
    if is_null_or_undefined(custom_message) {
        fallback.to_string()
    } else {
        value_to_string(custom_message)
    }
}

fn make_assertion_error(
    message: String,
    actual: f64,
    expected: f64,
    operator: &str,
    generated: bool,
) -> f64 {
    // One-shot registration so AssertionError instances satisfy
    // `instanceof Error` (see `instanceof.rs`: extends_builtin_error path).
    static REGISTER_ASSERTION_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_ASSERTION_ERROR.call_once(|| {
        js_register_class_extends_error(crate::error::CLASS_ID_ASSERTION_ERROR);
    });
    let obj = js_object_alloc(crate::error::CLASS_ID_ASSERTION_ERROR, 8);
    unsafe {
        let set = |key: &str, value: f64| {
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            js_object_set_field_by_name(obj, key_ptr, value);
        };
        set("name", string_f64("AssertionError"));
        set("code", string_f64("ERR_ASSERTION"));
        set("message", string_f64(&message));
        set("actual", actual);
        set("expected", expected);
        set("operator", string_f64(operator));
        set(
            "generatedMessage",
            f64::from_bits(crate::value::JSValue::bool(generated).bits()),
        );
    }
    crate::value::js_nanbox_pointer(obj as i64)
}

fn throw_assertion(
    message: String,
    actual: f64,
    expected: f64,
    operator: &str,
    generated: bool,
) -> ! {
    crate::exception::js_throw(make_assertion_error(
        message, actual, expected, operator, generated,
    ))
}

fn deep_equal_bool(actual: f64, expected: f64) -> bool {
    crate::value::js_is_truthy(crate::builtins::js_util_is_deep_strict_equal(
        actual, expected,
    )) != 0
}

fn assert_same_value(actual: f64, expected: f64) -> bool {
    #[inline(always)]
    fn numeric_value(raw: f64) -> Option<f64> {
        let bits = raw.to_bits();
        let value = crate::value::JSValue::from_bits(bits);
        if value.is_int32() {
            Some(value.as_int32() as f64)
        } else {
            let top16 = bits >> 48;
            // Plain IEEE-754 values, including the canonical raw NaN bucket
            // (0x7FF8) and all negative numbers, are numbers. Perry tagged
            // values use 0x7FF9..=0x7FFF, so do not classify them as NaN just
            // because f64::is_nan observes their NaN-box encoding.
            if !(0x7FF9..=0x7FFF).contains(&top16) {
                Some(raw)
            } else {
                None
            }
        }
    }

    // Node assert.strictEqual follows SameValue semantics: NaN equals NaN,
    // but +0 and -0 are different.
    if let (Some(actual_num), Some(expected_num)) = (numeric_value(actual), numeric_value(expected))
    {
        if actual_num.is_nan() && expected_num.is_nan() {
            return true;
        }
        if actual_num == 0.0 && expected_num == 0.0 {
            return actual_num.to_bits() == expected_num.to_bits();
        }
        return actual_num == expected_num;
    }

    crate::value::js_jsvalue_equals(actual, expected) != 0
}

#[no_mangle]
pub extern "C" fn js_assert_ok(value: f64, message: f64) -> f64 {
    if crate::value::js_is_truthy(value) != 0 {
        return undefined_f64();
    }
    if is_error_value(message) {
        crate::exception::js_throw(message);
    }
    throw_assertion(
        assertion_message(message, "The expression evaluated to a falsy value"),
        value,
        f64::from_bits(crate::value::JSValue::bool(true).bits()),
        "==",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_fail(message: f64) -> f64 {
    if is_error_value(message) {
        crate::exception::js_throw(message);
    }
    throw_assertion(
        assertion_message(message, "Failed"),
        undefined_f64(),
        undefined_f64(),
        "fail",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_strict_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if assert_same_value(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(message, "Expected values to be strictly equal"),
        actual,
        expected,
        "strictEqual",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_not_strict_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if !assert_same_value(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(
            message,
            "Expected actual to be strictly unequal to expected",
        ),
        actual,
        expected,
        "notStrictEqual",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if crate::value::js_jsvalue_loose_equals(actual, expected) != 0 {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(message, "Expected values to be loosely equal"),
        actual,
        expected,
        "==",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_not_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if crate::value::js_jsvalue_loose_equals(actual, expected) == 0 {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(message, "Expected values to be loosely unequal"),
        actual,
        expected,
        "!=",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_deep_strict_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if deep_equal_bool(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(message, "Expected values to be deeply strictly equal"),
        actual,
        expected,
        "deepStrictEqual",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_deep_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if deep_equal_bool(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(message, "Expected values to be deeply equal"),
        actual,
        expected,
        "deepEqual",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_not_deep_strict_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if !deep_equal_bool(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(
            message,
            "Expected actual not to be deeply strictly equal to expected",
        ),
        actual,
        expected,
        "notDeepStrictEqual",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_not_deep_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if !deep_equal_bool(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(
            message,
            "Expected actual not to be deeply equal to expected",
        ),
        actual,
        expected,
        "notDeepEqual",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_match(actual: f64, expected: f64, message: f64) -> f64 {
    if regex_test_value(expected, actual).unwrap_or(false) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(message, "The input did not match the regular expression"),
        actual,
        expected,
        "match",
        is_null_or_undefined(message),
    )
}

#[no_mangle]
pub extern "C" fn js_assert_does_not_match(actual: f64, expected: f64, message: f64) -> f64 {
    if !regex_test_value(expected, actual).unwrap_or(false) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(
            message,
            "The input was expected to not match the regular expression",
        ),
        actual,
        expected,
        "doesNotMatch",
        is_null_or_undefined(message),
    )
}

/// `new assert.AssertionError({actual, expected, operator, message, ...})`
/// constructor. Reuses `make_assertion_error` so the resulting object
/// carries the `CLASS_ID_ASSERTION_ERROR` class id, satisfies
/// `instanceof Error`, and has the standard `actual` / `expected` /
/// `operator` / `code` / `message` / `generatedMessage` fields Node
/// attaches. Unspecified fields default to `undefined`. When `message`
/// is missing, the operator-derived "<actual> <op> <expected>" default
/// is left to the caller (Node's behaviour computes a stringy summary
/// — we currently default to the operator string itself, which matches
/// what Perry's failing-assert helpers produce).
#[no_mangle]
pub extern "C" fn js_assert_assertion_error_ctor(options: f64) -> f64 {
    let undef = undefined_f64();
    let opts_is_obj = {
        let jv = crate::value::JSValue::from_bits(options.to_bits());
        jv.is_pointer() && !jv.as_pointer::<u8>().is_null()
    };
    let (actual, expected, operator_str, message, generated) = if opts_is_obj {
        unsafe {
            let read = |key: &str| -> f64 {
                let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let obj_ptr = crate::value::JSValue::from_bits(options.to_bits())
                    .as_pointer::<ObjectHeader>();
                let v = crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr);
                f64::from_bits(v.to_bits())
            };
            let actual = read("actual");
            let expected = read("expected");
            let operator_v = read("operator");
            let message_v = read("message");
            let operator_str = if is_null_or_undefined(operator_v) {
                String::new()
            } else {
                value_to_string(operator_v)
            };
            let (msg, generated) = if is_null_or_undefined(message_v) {
                // Default to the operator name so the resulting message is
                // non-empty; users typically pass an explicit message.
                (operator_str.clone(), true)
            } else {
                (value_to_string(message_v), false)
            };
            (actual, expected, operator_str, msg, generated)
        }
    } else {
        (undef, undef, String::new(), String::new(), true)
    };
    make_assertion_error(message, actual, expected, &operator_str, generated)
}

#[no_mangle]
pub extern "C" fn js_assert_if_error(value: f64) -> f64 {
    if is_null_or_undefined(value) {
        return undefined_f64();
    }
    throw_assertion(
        format!("ifError got unwanted exception: {}", value_to_string(value)),
        value,
        undefined_f64(),
        "ifError",
        true,
    )
}
