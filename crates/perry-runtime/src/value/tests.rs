//! Unit tests for the NaN-boxing primitives in this module tree.

#![cfg(test)]

use super::*;

#[test]
fn test_undefined() {
    let v = JSValue::undefined();
    assert!(v.is_undefined());
    assert!(!v.is_null());
    assert!(!v.is_number());
}

#[test]
fn test_null() {
    let v = JSValue::null();
    assert!(v.is_null());
    assert!(!v.is_undefined());
}

#[test]
fn test_bool() {
    let t = JSValue::bool(true);
    let f = JSValue::bool(false);
    assert!(t.is_bool());
    assert!(f.is_bool());
    assert!(t.as_bool());
    assert!(!f.as_bool());
}

#[test]
fn test_number() {
    let v = JSValue::number(42.5);
    assert!(v.is_number());
    assert_eq!(v.as_number(), 42.5);

    let zero = JSValue::number(0.0);
    assert!(zero.is_number());
    assert_eq!(zero.as_number(), 0.0);

    let neg = JSValue::number(-123.456);
    assert!(neg.is_number());
    assert_eq!(neg.as_number(), -123.456);
}

#[test]
fn test_int32() {
    let v = JSValue::int32(42);
    assert!(v.is_int32());
    assert_eq!(v.as_int32(), 42);

    let neg = JSValue::int32(-100);
    assert!(neg.is_int32());
    assert_eq!(neg.as_int32(), -100);
}

#[test]
fn test_truthiness() {
    assert!(!JSValue::undefined().to_bool());
    assert!(!JSValue::null().to_bool());
    assert!(!JSValue::bool(false).to_bool());
    assert!(JSValue::bool(true).to_bool());
    assert!(!JSValue::number(0.0).to_bool());
    assert!(JSValue::number(1.0).to_bool());
    assert!(JSValue::number(-1.0).to_bool());
    assert!(!JSValue::number(f64::NAN).to_bool());
}

#[test]
fn test_jsvalue_equals_booleans() {
    let t = f64::from_bits(TAG_TRUE);
    let f = f64::from_bits(TAG_FALSE);
    // Same boolean values
    assert_eq!(js_jsvalue_equals(t, t), 1);
    assert_eq!(js_jsvalue_equals(f, f), 1);
    // Different boolean values
    assert_eq!(js_jsvalue_equals(t, f), 0);
    assert_eq!(js_jsvalue_equals(f, t), 0);
    // Boolean vs number (strict equality: different types)
    assert_eq!(js_jsvalue_equals(t, 1.0), 0);
    assert_eq!(js_jsvalue_equals(f, 0.0), 0);
}

#[test]
fn test_jsvalue_equals_int32() {
    let int5 = f64::from_bits(INT32_TAG | 5);
    let float5 = 5.0f64;
    let int0 = f64::from_bits(INT32_TAG | 0);
    let float0 = 0.0f64;
    let int_neg = f64::from_bits(INT32_TAG | ((-3i32 as u32) as u64));
    let float_neg = -3.0f64;
    // INT32 vs f64 with same numeric value
    assert_eq!(js_jsvalue_equals(int5, float5), 1);
    assert_eq!(js_jsvalue_equals(float5, int5), 1);
    assert_eq!(js_jsvalue_equals(int0, float0), 1);
    assert_eq!(js_jsvalue_equals(int_neg, float_neg), 1);
    // INT32 vs INT32
    assert_eq!(js_jsvalue_equals(int5, int5), 1);
    // INT32 vs different f64
    assert_eq!(js_jsvalue_equals(int5, 6.0), 0);
    assert_eq!(js_jsvalue_equals(int5, 4.0), 0);
}

#[test]
fn test_short_string_encoding_roundtrip() {
    for s in [b"" as &[u8], b"a", b"ab", b"abc", b"abcd", b"abcde"] {
        let v = JSValue::try_short_string(s).unwrap();
        assert!(v.is_short_string(), "tag mismatch for {:?}", s);
        assert!(v.is_any_string(), "is_any_string should accept SSO");
        assert!(!v.is_string(), "legacy is_string should NOT accept SSO");
        assert_eq!(v.short_string_len(), s.len(), "length mismatch for {:?}", s);
        let mut buf = [0u8; SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        assert_eq!(n, s.len());
        assert_eq!(&buf[..n], s, "bytes mismatch for {:?}", s);
    }
}

#[test]
fn test_short_string_too_long_rejects() {
    assert!(JSValue::try_short_string(b"abcdef").is_none()); // 6 bytes
    assert!(JSValue::try_short_string(b"hello world").is_none()); // 11 bytes
}

#[test]
fn test_short_string_embedded_nul_ok() {
    // Strings with embedded U+0000 work fine in SSO — length
    // is authoritative, NULs are plain data bytes.
    let s = &[b'a', 0, b'b', 0, b'c'];
    let v = JSValue::try_short_string(s).unwrap();
    assert_eq!(v.short_string_len(), 5);
    let mut buf = [0u8; SHORT_STRING_MAX_LEN];
    let n = v.short_string_to_buf(&mut buf);
    assert_eq!(&buf[..n], s);
}

#[test]
fn test_short_string_tag_distinct_from_others() {
    // Any valid SSO value must not collide with other NaN-box
    // tags. `is_short_string()` is strict — returns false for
    // everything except the SSO tag band.
    let sso = JSValue::try_short_string(b"abcde").unwrap();
    let heap_string = JSValue {
        bits: STRING_TAG | 0x1234,
    };
    let pointer = JSValue {
        bits: POINTER_TAG | 0x5678,
    };
    let int32 = JSValue::int32(42);
    let number = JSValue::number(3.14);
    let undef = JSValue::undefined();
    assert!(sso.is_short_string());
    assert!(!heap_string.is_short_string());
    assert!(!pointer.is_short_string());
    assert!(!int32.is_short_string());
    assert!(!number.is_short_string());
    assert!(!undef.is_short_string());
    // is_any_string accepts both SSO and heap string, rejects others.
    assert!(sso.is_any_string());
    assert!(heap_string.is_any_string());
    assert!(!pointer.is_any_string());
    assert!(!int32.is_any_string());
    assert!(!number.is_any_string());
}

#[test]
fn test_short_string_empty_roundtrip() {
    let v = JSValue::try_short_string(b"").unwrap();
    assert!(v.is_short_string());
    assert_eq!(v.short_string_len(), 0);
    let mut buf = [0u8; SHORT_STRING_MAX_LEN];
    assert_eq!(v.short_string_to_buf(&mut buf), 0);
}

#[test]
fn test_short_string_byte_order_stability() {
    // First byte should land in the least-significant byte of
    // the payload. This invariant is relied on by any future
    // SIMD-style decoder that bulk-reads the payload.
    let v = JSValue::try_short_string(b"abcde").unwrap();
    let payload = v.bits() & SHORT_STRING_DATA_MASK;
    assert_eq!((payload & 0xFF) as u8, b'a');
    assert_eq!(((payload >> 8) & 0xFF) as u8, b'b');
    assert_eq!(((payload >> 16) & 0xFF) as u8, b'c');
    assert_eq!(((payload >> 24) & 0xFF) as u8, b'd');
    assert_eq!(((payload >> 32) & 0xFF) as u8, b'e');
}

#[test]
fn test_jsvalue_equals_numbers() {
    // Same numbers
    assert_eq!(js_jsvalue_equals(42.0, 42.0), 1);
    assert_eq!(js_jsvalue_equals(0.0, 0.0), 1);
    // -0 === 0 is true in JS
    assert_eq!(js_jsvalue_equals(-0.0, 0.0), 1);
    // Different numbers
    assert_eq!(js_jsvalue_equals(1.0, 2.0), 0);
    // null/undefined
    let null = f64::from_bits(TAG_NULL);
    let undef = f64::from_bits(TAG_UNDEFINED);
    assert_eq!(js_jsvalue_equals(null, null), 1);
    assert_eq!(js_jsvalue_equals(undef, undef), 1);
    assert_eq!(js_jsvalue_equals(null, undef), 0); // strict: null !== undefined
    assert_eq!(js_jsvalue_equals(null, 0.0), 0);
}
