//! Argument-coercion contract for `dispatch_string` (#5902): the reflective /
//! any-typed String-method dispatch must apply the observable spec coercions
//! (`ToString` on `replace`/`replaceAll` pattern+replacement, `ToLength` on
//! `padStart`/`padEnd` maxLength) to its arguments instead of silently
//! degrading non-string / non-number values. Every case here fails on the
//! pre-fix shape: `replace` extracted only already-string args (a number
//! pattern became a null pointer → receiver returned unchanged) and the pad
//! arms passed the raw NaN-boxed arg through (a string/object maxLength read
//! as NaN → target 0 → no padding). The user-code side of the same contract
//! (a `{ valueOf }` maxLength must execute, in spec order, before the fill
//! coercion) needs a JS closure and is covered by test262
//! `built-ins/String/prototype/{replace,padStart,padEnd}` via the parity
//! sweep, not here.

use crate::value::JSValue;

unsafe fn call_string_method(receiver: &str, method: &str, args: &[f64]) -> f64 {
    let s = crate::string::js_string_from_bytes(receiver.as_ptr(), receiver.len() as u32);
    let recv = f64::from_bits(JSValue::string_ptr(s).bits());
    super::js_native_call_method(
        recv,
        method.as_ptr() as *const i8,
        method.len(),
        if args.is_empty() {
            std::ptr::null()
        } else {
            args.as_ptr()
        },
        args.len(),
    )
}

unsafe fn assert_string_result(result: f64, expected: &str, label: &str) {
    let v = JSValue::from_bits(result.to_bits());
    if v.is_string() {
        let s = crate::object::has_own_helpers::str_from_string_header(v.as_string_ptr())
            .unwrap_or_default();
        assert_eq!(s, expected, "{label}");
    } else if v.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        assert_eq!(std::str::from_utf8(&buf[..n]).unwrap(), expected, "{label}");
    } else {
        panic!(
            "{label}: expected a string result, got bits {:#x}",
            v.bits()
        );
    }
}

fn short(bytes: &[u8]) -> f64 {
    f64::from_bits(JSValue::try_short_string(bytes).unwrap().bits())
}

#[test]
fn replace_coerces_a_non_string_pattern_via_to_string() {
    // "a1b".replace(1, "X") — ToString(1) === "1" → "aXb".
    unsafe {
        let r = call_string_method("a1b", "replace", &[1.0, short(b"X")]);
        assert_string_result(r, "aXb", "\"a1b\".replace(1, \"X\")");
    }
}

#[test]
fn replace_all_coerces_a_non_string_pattern_via_to_string() {
    // "1a1".replaceAll(1, "X") — ToString(1) === "1" → "XaX".
    unsafe {
        let r = call_string_method("1a1", "replaceAll", &[1.0, short(b"X")]);
        assert_string_result(r, "XaX", "\"1a1\".replaceAll(1, \"X\")");
    }
}

#[test]
fn replace_coerces_a_missing_replacement_to_undefined() {
    // "ab".replace("b") — §22.1.3.19 runs ToString(replaceValue) even for an
    // absent arg: ToString(undefined) === "undefined" → "aundefined" (Node
    // agrees). The old shape substituted an empty replacement.
    unsafe {
        let r = call_string_method("ab", "replace", &[short(b"b")]);
        assert_string_result(r, "aundefined", "\"ab\".replace(\"b\")");
    }
}

#[test]
fn pad_end_coerces_a_string_target_length_via_to_number() {
    // "abc".padEnd("11", "def") — ToLength(ToNumber("11")) === 11 →
    // "abcdefdefde".
    unsafe {
        let r = call_string_method("abc", "padEnd", &[short(b"11"), short(b"def")]);
        assert_string_result(r, "abcdefdefde", "\"abc\".padEnd(\"11\", \"def\")");
    }
}

#[test]
fn pad_start_coerces_target_and_defaults_the_fill() {
    // "5".padStart("3") — string target coerces to 3, absent fill defaults
    // to " " → "  5".
    unsafe {
        let r = call_string_method("5", "padStart", &[short(b"3")]);
        assert_string_result(r, "  5", "\"5\".padStart(\"3\")");
    }
}

extern "C" fn undef_replacer(
    _closure: *const crate::closure::ClosureHeader,
    _matched: f64,
    _offset: f64,
    _whole: f64,
) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

#[test]
fn replace_fn_result_is_to_string_coerced() {
    // "gnulluna".replace("null", () => undefined) — the callback result is
    // ToString'd (§22.1.3.19): undefined renders as "undefined" →
    // "gundefineduna" (test262 S15.5.4.11_A1_T5's shape; the old
    // `call_replace_callback` dropped every non-string result to "").
    unsafe {
        let func_ptr = undef_replacer as *const u8;
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        assert!(!closure.is_null());
        crate::closure::js_register_closure_arity(func_ptr, 3);
        let cb = crate::value::js_nanbox_pointer(closure as i64);
        let r = call_string_method("gnulluna", "replace", &[short(b"null"), cb]);
        assert_string_result(r, "gundefineduna", "\"gnulluna\".replace(\"null\", fn)");
    }
}
