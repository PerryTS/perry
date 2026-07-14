//! Node-compatible `assert` module runtime entry points.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use std::os::raw::c_int;

use super::*;

fn undefined_f64() -> f64 {
    f64::from_bits(crate::value::JSValue::undefined().bits())
}

fn null_f64() -> f64 {
    f64::from_bits(crate::value::JSValue::null().bits())
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

fn string_header_to_string(ptr: *const crate::StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn is_function_value(value: f64) -> bool {
    let ptr = crate::builtins::js_value_typeof(value) as *const crate::StringHeader;
    string_header_to_string(ptr) == "function"
}

fn is_null_or_undefined(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    jv.is_null() || jv.is_undefined()
}

fn gc_type_of(value: f64) -> Option<u8> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        Some((*gc_header).obj_type)
    }
}

fn is_error_value(value: f64) -> bool {
    gc_type_of(value) == Some(crate::gc::GC_TYPE_ERROR)
}

fn regexp_ptr(pattern: f64) -> Option<*const crate::regex::RegExpHeader> {
    let jv = crate::value::JSValue::from_bits(pattern.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if !crate::regex::is_regex_pointer(ptr) {
        return None;
    }
    Some(ptr as *const crate::regex::RegExpHeader)
}

#[cfg(feature = "regex-engine")]
fn regex_test_value(pattern: f64, input: f64) -> Option<bool> {
    let re = regexp_ptr(pattern)?;
    let input_string = value_to_string(input);
    let input_ptr =
        crate::string::js_string_from_bytes(input_string.as_ptr(), input_string.len() as u32);
    Some(crate::regex::js_regexp_test(re, input_ptr) != 0)
}

/// Regex engine gated off: no RegExp value can exist, so `expected` is never a
/// RegExp — report "not a RegExp matcher" so callers fall to their non-regex
/// comparison path.
#[cfg(not(feature = "regex-engine"))]
fn regex_test_value(_pattern: f64, _input: f64) -> Option<bool> {
    None
}

#[cfg(feature = "regex-engine")]
fn regex_test_string(re: *const crate::regex::RegExpHeader, input: f64) -> bool {
    let input_ptr =
        crate::value::js_get_string_pointer_unified(input) as *const crate::StringHeader;
    !input_ptr.is_null() && crate::regex::js_regexp_test(re, input_ptr) != 0
}

#[cfg(not(feature = "regex-engine"))]
fn regex_test_string(_re: *const crate::regex::RegExpHeader, _input: f64) -> bool {
    false
}

fn validate_regexp_argument(regexp: f64) -> *const crate::regex::RegExpHeader {
    if let Some(re) = regexp_ptr(regexp) {
        return re;
    }
    let message = format!(
        "The \"regexp\" argument must be an instance of RegExp. Received {}",
        crate::fs::validate::describe_received(regexp)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn validate_assert_match_string(actual: f64, expected: f64, message: f64, operator: &str) {
    if crate::value::JSValue::from_bits(actual.to_bits()).is_any_string() {
        return;
    }
    let fallback = format!(
        "The \"string\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(actual)
    );
    throw_assertion(
        assertion_message(message, &fallback),
        actual,
        expected,
        operator,
        is_null_or_undefined(message),
    )
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
    } else if expected.to_bits() == global_builtin(b"EvalError").to_bits() {
        "EvalError"
    } else if expected.to_bits() == global_builtin(b"URIError").to_bits() {
        "URIError"
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
                | "EvalError"
                | "URIError"
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
    if constructor_name_matches_builtin_error(thrown, expected) {
        return true;
    }
    // Node: a function `expected` not matched by the instanceof / constructor
    // checks above is treated as a *validation function* — it is called with
    // the thrown error and a strict `true` return means the error matched
    // (`lib/internal/assert`). Arrow and plain `function (err) {…}` validators
    // both land here. A throw from calling it (e.g. an `Error` subclass invoked
    // without `new`) counts as no-match, mirroring Node's Error-subclass fail
    // path.
    if expected_is_callable(expected) {
        if let Ok(ret) = call_validator_capturing(expected, thrown) {
            const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
            return ret.to_bits() == TAG_TRUE;
        }
    }
    false
}

/// Whether `value` is a callable closure (a NaN-boxed `POINTER_TAG` value
/// pointing at a `ClosureHeader`). Used to recognize an `assert.throws`
/// validation function.
fn expected_is_callable(value: f64) -> bool {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & !POINTER_MASK) != POINTER_TAG {
        return false;
    }
    crate::closure::is_closure_ptr((bits & POINTER_MASK) as usize)
}

/// Call an `assert.throws` validation function with the thrown error,
/// trapping any throw it raises (so a class-without-`new` or a side-effecting
/// validator can't escape as an uncaught exception). Mirrors
/// [`call_block_capturing_throw`] but passes one argument.
fn call_validator_capturing(validator: f64, arg: f64) -> Result<f64, f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    let result = if jumped == 0 {
        let args = [arg];
        let value = unsafe { crate::closure::js_native_call_value(validator, args.as_ptr(), 1) };
        Ok(value)
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    };
    crate::exception::js_try_end();
    result
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

fn invalid_function_argument(arg_name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{arg_name}\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_promise_fn_argument(value: f64) -> f64 {
    let message = format!(
        "The \"promiseFn\" argument must be of type function or an instance of Promise. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_promise_fn_return(value: f64) -> f64 {
    let message = format!(
        "Expected instance of Promise to be returned from the \"promiseFn\" function but got {}.",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_RETURN_VALUE")
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
        // An absent operator surfaces as `undefined` (matching Node), not an
        // empty string. The assert.* helpers always pass a non-empty operator,
        // so this only affects `new AssertionError({ ... })` with no operator.
        set(
            "operator",
            if operator.is_empty() {
                undefined_f64()
            } else {
                string_f64(operator)
            },
        );
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

fn if_error_value_message(value: f64) -> Option<String> {
    match gc_type_of(value) {
        Some(crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_ERROR) => {}
        _ => return None,
    }

    let message = read_property(value, "message");
    if !crate::value::JSValue::from_bits(message.to_bits()).is_any_string() {
        return None;
    }

    let message = value_to_string(message);
    if !message.is_empty() {
        return Some(message);
    }

    let name = read_property(value, "name");
    if crate::value::JSValue::from_bits(name.to_bits()).is_any_string() {
        let name = value_to_string(name);
        if !name.is_empty() {
            return Some(name);
        }
    }

    Some(message)
}

fn inspected_value_to_string(value: f64) -> String {
    let inspected = crate::builtins::js_util_inspect(value, undefined_f64());
    value_to_string(inspected)
}

fn if_error_message(value: f64) -> String {
    let value_message =
        if_error_value_message(value).unwrap_or_else(|| inspected_value_to_string(value));
    format!("ifError got unwanted exception: {value_message}")
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

struct AssertAsyncInput {
    promise: *mut crate::promise::Promise,
    validation_error: bool,
}

fn promise_from_assert_async_input(input: f64) -> AssertAsyncInput {
    if let Some(promise) = promise_ptr_from_value(input) {
        return AssertAsyncInput {
            promise,
            validation_error: false,
        };
    }
    if !is_function_value(input) {
        return AssertAsyncInput {
            promise: rejected_promise(invalid_promise_fn_argument(input)),
            validation_error: true,
        };
    }
    let (promise, validation_error) = match call_block_capturing_throw(input) {
        Ok(value) => match promise_ptr_from_value(value) {
            Some(promise) => (promise, false),
            None => (rejected_promise(invalid_promise_fn_return(value)), true),
        },
        Err(reason) => (rejected_promise(reason), false),
    };
    AssertAsyncInput {
        promise,
        validation_error,
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
    let expected = crate::closure::js_closure_get_capture_f64(closure, 2);
    // With a matcher present, a rejection that does NOT match propagates the
    // original rejection reason unchanged (Node behavior, #2971). Only a
    // matching rejection (or no matcher) becomes an AssertionError.
    if !is_null_or_undefined(expected) && !expected_error_matches(reason, expected) {
        crate::promise::js_promise_reject(result, reason);
        return undefined_f64();
    }
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

fn deep_equal_skip_prototype_bool(actual: f64, expected: f64) -> bool {
    crate::value::js_is_truthy(
        crate::builtins::js_util_is_deep_strict_equal_skip_prototype(actual, expected),
    ) != 0
}

fn heap_value_type(value: f64) -> Option<(*const u8, u8)> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        Some((ptr, (*gc_header).obj_type))
    }
}

fn partial_array_equal(
    actual: *const crate::array::ArrayHeader,
    expected: *const crate::array::ArrayHeader,
) -> bool {
    let actual_len = crate::array::js_array_length(actual);
    let expected_len = crate::array::js_array_length(expected);
    if expected_len > actual_len {
        return false;
    }

    let expected_keys = crate::object::js_object_keys(expected as *const ObjectHeader);
    let expected_key_count = crate::array::js_array_length(expected_keys);
    for key_index in 0..expected_key_count {
        let key_value = crate::array::js_array_get_f64(expected_keys, key_index);
        let Some(element_index) = array_index_key(key_value) else {
            return false;
        };
        if !array_has_index(actual, element_index) {
            return false;
        }
        let actual_value = crate::array::js_array_get_f64(actual, element_index);
        let expected_value = crate::array::js_array_get_f64(expected, element_index);
        if !partial_deep_equal_bool(actual_value, expected_value) {
            return false;
        }
    }
    true
}

fn array_has_index(arr: *const crate::array::ArrayHeader, index: u32) -> bool {
    if arr.is_null() || index >= crate::array::js_array_length(arr) {
        return false;
    }
    unsafe {
        let elements =
            (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
        std::ptr::read(elements.add(index as usize)) != crate::value::TAG_HOLE
    }
}

fn array_index_key(key_value: f64) -> Option<u32> {
    let key = crate::JSValue::from_bits(key_value.to_bits());
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = unsafe { crate::string::js_string_key_bytes(key, &mut sso_buf)? };
    if bytes.is_empty() {
        return None;
    }
    let mut index = 0u32;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        index = index.checked_mul(10)?;
        index = index.checked_add((byte - b'0') as u32)?;
    }
    Some(index)
}

fn partial_object_equal(
    actual_value: f64,
    actual: *const ObjectHeader,
    expected: *const ObjectHeader,
) -> bool {
    let expected_keys = crate::object::js_object_keys(expected);
    let expected_key_count = crate::array::js_array_length(expected_keys);
    for index in 0..expected_key_count {
        let key_value = crate::array::js_array_get_f64(expected_keys, index);
        if crate::value::js_is_truthy(crate::object::js_object_has_own(actual_value, key_value))
            == 0
        {
            return false;
        }
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key_value) as *const crate::StringHeader;
        if key_ptr.is_null() {
            return false;
        }
        let actual_prop = crate::object::js_object_get_field_by_name_f64(actual, key_ptr);
        let expected_prop = crate::object::js_object_get_field_by_name_f64(expected, key_ptr);
        if !partial_deep_equal_bool(actual_prop, expected_prop) {
            return false;
        }
    }
    true
}

fn partial_deep_equal_bool(actual: f64, expected: f64) -> bool {
    if deep_equal_bool(actual, expected) {
        return true;
    }
    let Some((actual_ptr, actual_type)) = heap_value_type(actual) else {
        return false;
    };
    let Some((expected_ptr, expected_type)) = heap_value_type(expected) else {
        return false;
    };
    if actual_type != expected_type {
        return false;
    }
    match expected_type {
        crate::gc::GC_TYPE_ARRAY => partial_array_equal(
            actual_ptr as *const crate::array::ArrayHeader,
            expected_ptr as *const crate::array::ArrayHeader,
        ),
        crate::gc::GC_TYPE_OBJECT => partial_object_equal(
            actual,
            actual_ptr as *const ObjectHeader,
            expected_ptr as *const ObjectHeader,
        ),
        _ => false,
    }
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

/// Audit #18: `node:assert` loose equality (`assert.equal` / `assert.notEqual`)
/// must use the SAME abstract equality as the `==` operator — `js_loose_eq`,
/// which performs full ToPrimitive coercion and BigInt==Number comparison. The
/// previously-used `js_jsvalue_loose_equals` is a drifted variant lacking both,
/// so `assert.equal(1n, 1)` wrongly threw (`1n == 1` is true) and
/// `assert.equal(new Number(1), 1)` etc. disagreed with `==`. (`assert.deepEqual`
/// leaf coercion is a separate, deeper path — not changed here.)
fn assert_loose_eq(actual: f64, expected: f64) -> bool {
    crate::builtins::js_loose_eq(
        crate::value::JSValue::from_bits(actual.to_bits()),
        crate::value::JSValue::from_bits(expected.to_bits()),
    )
    .as_bool()
}

#[no_mangle]
pub extern "C" fn js_assert_equal(actual: f64, expected: f64, message: f64) -> f64 {
    if assert_loose_eq(actual, expected) {
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
    if !assert_loose_eq(actual, expected) {
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
pub extern "C" fn js_assert_deep_strict_equal_skip_prototype(
    actual: f64,
    expected: f64,
    message: f64,
) -> f64 {
    if deep_equal_skip_prototype_bool(actual, expected) {
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
pub extern "C" fn js_assert_partial_deep_strict_equal(
    actual: f64,
    expected: f64,
    message: f64,
) -> f64 {
    if partial_deep_equal_bool(actual, expected) {
        return undefined_f64();
    }
    throw_assertion(
        assertion_message(
            message,
            "Expected values to be partially deeply strictly equal",
        ),
        actual,
        expected,
        "partialDeepStrictEqual",
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
pub extern "C" fn js_assert_deep_equal_skip_prototype(
    actual: f64,
    expected: f64,
    message: f64,
) -> f64 {
    if deep_equal_skip_prototype_bool(actual, expected) {
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
pub extern "C" fn js_assert_not_deep_strict_equal_skip_prototype(
    actual: f64,
    expected: f64,
    message: f64,
) -> f64 {
    if !deep_equal_skip_prototype_bool(actual, expected) {
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
pub extern "C" fn js_assert_not_deep_equal_skip_prototype(
    actual: f64,
    expected: f64,
    message: f64,
) -> f64 {
    if !deep_equal_skip_prototype_bool(actual, expected) {
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
    let re = validate_regexp_argument(expected);
    validate_assert_match_string(actual, expected, message, "match");
    if regex_test_string(re, actual) {
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
    let re = validate_regexp_argument(expected);
    validate_assert_match_string(actual, expected, message, "doesNotMatch");
    if !regex_test_string(re, actual) {
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
    if !is_function_value(block) {
        invalid_function_argument("fn", block);
    }
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
pub extern "C" fn js_assert_does_not_throw(block: f64, expected: f64, message: f64) -> f64 {
    if !is_function_value(block) {
        invalid_function_argument("fn", block);
    }
    match call_block_capturing_throw(block) {
        Ok(_) => undefined_f64(),
        // With a matcher present, a throw that does NOT match the matcher
        // propagates the original error unchanged (Node behavior, #2971). Only
        // a matching throw (or no matcher at all) becomes an AssertionError.
        Err(thrown)
            if !is_null_or_undefined(expected) && !expected_error_matches(thrown, expected) =>
        {
            crate::exception::js_throw(thrown)
        }
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
    if source.validation_error {
        return promise_value_from_ptr(source.promise);
    }
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
    crate::promise::js_promise_then(source.promise, on_fulfilled, on_rejected);
    promise_value_from_ptr(result)
}

#[no_mangle]
pub extern "C" fn js_assert_does_not_reject(input: f64, expected: f64, message: f64) -> f64 {
    let source = promise_from_assert_async_input(input);
    if source.validation_error {
        return promise_value_from_ptr(source.promise);
    }
    let result = crate::promise::js_promise_new();
    let on_fulfilled =
        crate::closure::js_closure_alloc(assert_does_not_reject_fulfilled as *const u8, 1);
    let on_rejected =
        crate::closure::js_closure_alloc(assert_does_not_reject_rejected as *const u8, 3);
    crate::closure::js_closure_set_capture_ptr(on_fulfilled, 0, result as i64);
    crate::closure::js_closure_set_capture_ptr(on_rejected, 0, result as i64);
    crate::closure::js_closure_set_capture_f64(on_rejected, 1, message);
    crate::closure::js_closure_set_capture_f64(on_rejected, 2, expected);
    crate::promise::js_promise_then(source.promise, on_fulfilled, on_rejected);
    promise_value_from_ptr(result)
}

/// `new assert.AssertionError({actual, expected, operator, message, ...})`
/// constructor. Reuses `make_assertion_error` so the resulting object
/// carries the `CLASS_ID_ASSERTION_ERROR` class id, satisfies
/// `instanceof Error`, and has the standard `actual` / `expected` /
/// `operator` / `code` / `message` / `generatedMessage` fields Node
/// attaches.
///
/// #3034 — Node requires an options object: a missing/null/primitive
/// `options` throws `TypeError [ERR_INVALID_ARG_TYPE]`. When `message` is
/// absent, Node generates a default from `actual`/`operator`/`expected`. We
/// emit the generic comparison summary `"<actual> <operator> <expected>"`
/// (the format Node uses for non-diffing operators like `===`, and the one
/// the issue's surface exercises); the elaborate per-operator diff messages
/// for `strictEqual`/`deepStrictEqual`/… are produced by the assert.* helpers
/// themselves, not by this direct-construction path.
#[no_mangle]
pub extern "C" fn js_assert_assertion_error_ctor(options: f64) -> f64 {
    let opts_is_obj = {
        let jv = crate::value::JSValue::from_bits(options.to_bits());
        jv.is_pointer() && !jv.as_pointer::<u8>().is_null()
    };
    if !opts_is_obj {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(options)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    unsafe {
        let read = |key: &str| -> f64 {
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            let obj_ptr =
                crate::value::JSValue::from_bits(options.to_bits()).as_pointer::<ObjectHeader>();
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
        let (message, generated) = if is_null_or_undefined(message_v) {
            // Node's generated default for a directly-constructed
            // AssertionError is the comparison summary
            // `<actual> <operator> <expected>`, with each absent operand and
            // the operator itself rendered via String() (`undefined` when
            // missing). E.g. `{}` -> "undefined undefined undefined",
            // `{actual:1,expected:2,operator:"==="}` -> "1 === 2".
            let summary = format!(
                "{} {} {}",
                value_to_string(actual),
                value_to_string(operator_v),
                value_to_string(expected)
            );
            (summary, true)
        } else {
            (value_to_string(message_v), false)
        };
        make_assertion_error(message, actual, expected, &operator_str, generated)
    }
}

fn assert_option_bool(options: f64, key: &str) -> Option<bool> {
    if gc_type_of(options) != Some(crate::gc::GC_TYPE_OBJECT) {
        return None;
    }
    let value = read_property(options, key);
    if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(crate::value::js_is_truthy(value) != 0)
    }
}

/// `new assert.Assert(options)` constructor. Instances are represented as
/// native-module namespace objects with private module names that encode the
/// option-sensitive dispatch mode.
#[no_mangle]
pub extern "C" fn js_assert_assert_ctor(options: f64) -> f64 {
    let strict = assert_option_bool(options, "strict").unwrap_or(true);
    let skip_prototype = assert_option_bool(options, "skipPrototype").unwrap_or(false);
    let module_name = match (strict, skip_prototype) {
        (false, false) => "assert.instance",
        (false, true) => "assert.instance.skip",
        (true, false) => "assert/strict.instance",
        (true, true) => "assert/strict.instance.skip",
    };
    let instance = js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
    let jv = crate::value::JSValue::from_bits(instance.to_bits());
    if jv.is_pointer() {
        let obj = jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        if !obj.is_null() {
            let ctor = bound_native_callable_export_value("assert", "Assert");
            let key = crate::string::js_string_from_bytes(
                b"constructor".as_ptr(),
                "constructor".len() as u32,
            );
            js_object_set_field_by_name(obj, key, ctor);
            super::set_builtin_property_attrs(
                obj as usize,
                "constructor".to_string(),
                super::PropertyAttrs::new(true, false, true),
            );
        }
    }
    instance
}

#[no_mangle]
pub extern "C" fn js_assert_if_error(value: f64) -> f64 {
    if is_null_or_undefined(value) {
        return undefined_f64();
    }
    throw_assertion(if_error_message(value), value, null_f64(), "ifError", false)
}
