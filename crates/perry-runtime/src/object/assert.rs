//! Node-compatible `assert` module runtime entry points.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use std::os::raw::c_int;

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

fn read_property(value: f64, key: &str) -> f64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return undefined_f64();
    }
    let ptr = jv.as_pointer::<ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return undefined_f64();
    }
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_get_field_by_name_f64(ptr, key_ptr)
}

fn is_plain_matcher_object(value: f64) -> bool {
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
        (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
    }
}

fn object_matcher_matches(actual: f64, expected: f64) -> bool {
    if !is_plain_matcher_object(expected) {
        return false;
    }

    let mut saw_expected_key = false;
    for key in ["name", "message", "code", "errno"] {
        let expected_prop = read_property(expected, key);
        if is_null_or_undefined(expected_prop) {
            continue;
        }
        saw_expected_key = true;
        let actual_prop = read_property(actual, key);
        // Node compares every key the validator object specifies: if it
        // names `code`/`errno`/`name`/`message`, the thrown error must carry
        // an equal value. A missing or mismatching value on the thrown error
        // is a mismatch (#2014) — there is no "optional code" carve-out.
        // A RegExp validator value (e.g. `{ message: /bad/ }`) is tested
        // against the thrown error's prop instead of being deep-equal-ed,
        // matching Node's `deepStrictEqual`-with-RegExp-special-case shape.
        if let Some(re_matches) = regex_test_value(expected_prop, actual_prop) {
            if !re_matches {
                return false;
            }
            continue;
        }
        if !assert_same_value(actual_prop, expected_prop)
            && crate::value::js_jsvalue_loose_equals(actual_prop, expected_prop) == 0
        {
            return false;
        }
    }

    saw_expected_key
}

fn constructor_name_matches_builtin_error(thrown: f64, expected: f64) -> bool {
    fn global_builtin(name: &'static [u8]) -> f64 {
        crate::object::js_get_global_this_builtin_value(name.as_ptr(), name.len())
    }

    let expected_name = if expected.to_bits() == global_builtin(b"Error").to_bits() {
        "Error"
    } else if expected.to_bits() == global_builtin(b"TypeError").to_bits() {
        "TypeError"
    } else if expected.to_bits() == global_builtin(b"RangeError").to_bits() {
        "RangeError"
    } else if expected.to_bits() == global_builtin(b"ReferenceError").to_bits() {
        "ReferenceError"
    } else if expected.to_bits() == global_builtin(b"SyntaxError").to_bits() {
        "SyntaxError"
    } else if expected.to_bits() == global_builtin(b"AggregateError").to_bits() {
        "AggregateError"
    } else {
        let expected_name = read_property(expected, "name");
        if is_null_or_undefined(expected_name) {
            return false;
        }
        let expected_name = value_to_string(expected_name);
        if !matches!(
            expected_name.as_str(),
            "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "AggregateError"
        ) {
            return false;
        }
        let thrown_name = read_property(thrown, "name");
        return !is_null_or_undefined(thrown_name) && value_to_string(thrown_name) == expected_name;
    };
    if expected_name == "Error" && is_error_value(thrown) {
        return true;
    }
    let thrown_name = read_property(thrown, "name");
    !is_null_or_undefined(thrown_name) && value_to_string(thrown_name) == expected_name
}

fn expected_error_matches(thrown: f64, expected: f64) -> bool {
    if is_null_or_undefined(expected) {
        return true;
    }
    if let Some(matches_thrown) = regex_test_value(expected, thrown) {
        if matches_thrown {
            return true;
        }
        let message = read_property(thrown, "message");
        if !is_null_or_undefined(message) && regex_test_value(expected, message).unwrap_or(false) {
            return true;
        }
    }
    // A plain object validator (e.g. `{ code: "ERR_X" }`) is a property-bag
    // matcher, never a constructor — its own enumerable keys must each equal
    // the thrown error's. Do NOT fall through to the instanceof /
    // builtin-constructor checks below: those can spuriously accept *any*
    // error (e.g. `js_instanceof_dynamic` against a plain object, or a
    // validator carrying `name: "Error"`) and would mask a wrong `code`
    // (#2014).
    if is_plain_matcher_object(expected) {
        return object_matcher_matches(thrown, expected);
    }
    if crate::value::js_is_truthy(crate::object::js_instanceof_dynamic(thrown, expected)) != 0 {
        return true;
    }
    constructor_name_matches_builtin_error(thrown, expected)
}

fn call_block_capturing_throw(block: f64) -> Result<f64, f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    let result = if jumped == 0 {
        let value = unsafe { crate::closure::js_native_call_value(block, std::ptr::null(), 0) };
        Ok(value)
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    };
    crate::exception::js_try_end();
    result
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

fn promise_ptr_from_value(value: f64) -> Option<*mut crate::promise::Promise> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return None;
    }
    let ptr = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    (!ptr.is_null()).then_some(ptr)
}

fn promise_value_from_ptr(promise: *mut crate::promise::Promise) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

fn fulfilled_promise(value: f64) -> *mut crate::promise::Promise {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_resolve(promise, value);
    promise
}

fn rejected_promise(reason: f64) -> *mut crate::promise::Promise {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_reject(promise, reason);
    promise
}

fn promise_from_assert_async_input(input: f64) -> *mut crate::promise::Promise {
    if let Some(promise) = promise_ptr_from_value(input) {
        return promise;
    }
    match call_block_capturing_throw(input) {
        Ok(value) => promise_ptr_from_value(value).unwrap_or_else(|| fulfilled_promise(value)),
        Err(reason) => rejected_promise(reason),
    }
}

extern "C" fn assert_rejects_fulfilled(
    closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    let result =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    let expected = crate::closure::js_closure_get_capture_f64(closure, 1);
    let message = crate::closure::js_closure_get_capture_f64(closure, 2);
    let err = make_assertion_error(
        assertion_message(message, "Missing expected rejection"),
        undefined_f64(),
        expected,
        "rejects",
        is_null_or_undefined(message),
    );
    crate::promise::js_promise_reject(result, err);
    undefined_f64()
}

extern "C" fn assert_rejects_rejected(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let result =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    let expected = crate::closure::js_closure_get_capture_f64(closure, 1);
    let message = crate::closure::js_closure_get_capture_f64(closure, 2);
    if expected_error_matches(reason, expected) {
        crate::promise::js_promise_resolve(result, undefined_f64());
    } else {
        let err = make_assertion_error(
            assertion_message(message, "The rejection did not match the expected matcher"),
            reason,
            expected,
            "rejects",
            is_null_or_undefined(message),
        );
        crate::promise::js_promise_reject(result, err);
    }
    undefined_f64()
}

extern "C" fn assert_does_not_reject_fulfilled(
    closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    let result =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    crate::promise::js_promise_resolve(result, undefined_f64());
    undefined_f64()
}

extern "C" fn assert_does_not_reject_rejected(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let result =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    let message = crate::closure::js_closure_get_capture_f64(closure, 1);
    let err = make_assertion_error(
        assertion_message(message, "Got unwanted rejection"),
        reason,
        undefined_f64(),
        "doesNotReject",
        is_null_or_undefined(message),
    );
    crate::promise::js_promise_reject(result, err);
    undefined_f64()
}

fn closure3(
    func: *const u8,
    result: *mut crate::promise::Promise,
    expected: f64,
    message: f64,
) -> *const crate::closure::ClosureHeader {
    let closure = crate::closure::js_closure_alloc(func, 3);
    crate::closure::js_closure_set_capture_ptr(closure, 0, result as i64);
    crate::closure::js_closure_set_capture_f64(closure, 1, expected);
    crate::closure::js_closure_set_capture_f64(closure, 2, message);
    closure
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

#[no_mangle]
pub extern "C" fn js_assert_throws(block: f64, expected: f64, message: f64) -> f64 {
    match call_block_capturing_throw(block) {
        Err(thrown) if expected_error_matches(thrown, expected) => undefined_f64(),
        Err(thrown) => throw_assertion(
            assertion_message(
                message,
                "The thrown error did not match the expected matcher",
            ),
            thrown,
            expected,
            "throws",
            false,
        ),
        Ok(_) => throw_assertion(
            assertion_message(message, "Missing expected exception"),
            undefined_f64(),
            expected,
            "throws",
            false,
        ),
    }
}

#[no_mangle]
pub extern "C" fn js_assert_does_not_throw(block: f64, _expected: f64, message: f64) -> f64 {
    match call_block_capturing_throw(block) {
        Ok(_) => undefined_f64(),
        Err(thrown) => throw_assertion(
            assertion_message(message, "Got unwanted exception"),
            thrown,
            undefined_f64(),
            "doesNotThrow",
            false,
        ),
    }
}

#[no_mangle]
pub extern "C" fn js_assert_rejects(input: f64, expected: f64, message: f64) -> f64 {
    let source = promise_from_assert_async_input(input);
    let result = crate::promise::js_promise_new();
    let on_fulfilled = closure3(
        assert_rejects_fulfilled as *const u8,
        result,
        expected,
        message,
    );
    let on_rejected = closure3(
        assert_rejects_rejected as *const u8,
        result,
        expected,
        message,
    );
    crate::promise::js_promise_then(source, on_fulfilled, on_rejected);
    promise_value_from_ptr(result)
}

#[no_mangle]
pub extern "C" fn js_assert_does_not_reject(input: f64, _expected: f64, message: f64) -> f64 {
    let source = promise_from_assert_async_input(input);
    let result = crate::promise::js_promise_new();
    let on_fulfilled =
        crate::closure::js_closure_alloc(assert_does_not_reject_fulfilled as *const u8, 1);
    let on_rejected =
        crate::closure::js_closure_alloc(assert_does_not_reject_rejected as *const u8, 2);
    crate::closure::js_closure_set_capture_ptr(on_fulfilled, 0, result as i64);
    crate::closure::js_closure_set_capture_ptr(on_rejected, 0, result as i64);
    crate::closure::js_closure_set_capture_f64(on_rejected, 1, message);
    crate::promise::js_promise_then(source, on_fulfilled, on_rejected);
    promise_value_from_ptr(result)
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
