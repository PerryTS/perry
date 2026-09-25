//! #10762: the NaN-box-returning conversion twins (`js_number_to_string_box`,
//! `js_string_coerce_box`, `js_template_string_coerce_box`,
//! `js_jsvalue_to_string_method_box`) must print exactly what their
//! pointer-returning originals print, and differ only in representation: a
//! result of at most `SHORT_STRING_MAX_LEN` bytes is an SSO immediate.

use super::*;
use crate::value::{JSValue, SHORT_STRING_MAX_LEN};

/// The bytes of a NaN-boxed string, whichever representation it uses.
fn boxed_bytes(value: f64) -> Vec<u8> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_short_string() {
        let mut buf = [0u8; SHORT_STRING_MAX_LEN];
        let len = jv.short_string_to_buf(&mut buf);
        return buf[..len].to_vec();
    }
    assert!(jv.is_string(), "not a string: {:016x}", value.to_bits());
    heap_bytes(jv.as_string_ptr())
}

fn heap_bytes(ptr: *const StringHeader) -> Vec<u8> {
    assert!(!ptr.is_null());
    unsafe { std::slice::from_raw_parts(string_data(ptr), (*ptr).byte_len as usize).to_vec() }
}

fn boxed_heap(ptr: *mut StringHeader) -> f64 {
    f64::from_bits(crate::value::STRING_TAG | ptr as u64)
}

/// Every value the conversions are pinned on: the SSO boundary on both sides
/// (5 vs 6 bytes, with and without a sign), the small-int cache boundary the
/// pointer path special-cases, `-0`, the non-finite values, short and long
/// fractions, and the exponent thresholds.
const EDGES: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    7.0,
    42.0,
    255.0,
    256.0,
    -255.0,
    9_999.0,
    -9_999.0,
    10_000.0,
    -10_000.0,
    99_999.0,
    100_000.0,
    -99_999.0,
    2_147_483_647.0,
    -2_147_483_648.0,
    4_294_967_296.0,
    999_999_999_999_999.0,
    1e15,
    1e21,
    1e-6,
    1e-7,
    0.5,
    -0.5,
    1.25,
    -1.25,
    0.125,
    0.1,
    123.5,
    -12.5,
    1.0 / 3.0,
    f64::MAX,
    f64::MIN_POSITIVE,
    f64::EPSILON,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

fn check_number_box(value: f64, boxed: f64) {
    let expected = heap_bytes(js_number_to_string(value));
    assert_eq!(
        boxed_bytes(boxed),
        expected,
        "text differs for {:016x}",
        value.to_bits()
    );
    let jv = JSValue::from_bits(boxed.to_bits());
    if expected.len() <= SHORT_STRING_MAX_LEN {
        assert!(
            jv.is_short_string(),
            "{:?} fits an SSO immediate but came back {:016x}",
            String::from_utf8_lossy(&expected),
            boxed.to_bits()
        );
        // Canonical: the same bits every time, and the same bits the generic
        // SSO constructor produces, so bitwise key/equality caches agree.
        assert_eq!(
            boxed.to_bits(),
            JSValue::try_short_string(&expected).unwrap().bits()
        );
    } else {
        assert!(
            jv.is_string(),
            "{:?} does not fit SSO but came back {:016x}",
            String::from_utf8_lossy(&expected),
            boxed.to_bits()
        );
    }
}

#[test]
fn number_to_string_box_prints_what_number_to_string_prints() {
    for &value in EDGES {
        check_number_box(value, js_number_to_string_box(value));
    }
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    for index in 0..50_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        // Raw bit patterns (every magnitude, subnormals), then integers across
        // the SSO boundary, then short fractions that may or may not fit.
        let value = match index % 3 {
            0 => f64::from_bits(seed),
            1 => ((seed >> 40) as i64 - (1 << 23)) as f64,
            _ => ((seed >> 50) as i64 - 4096) as f64 / 8.0,
        };
        // Perry's own tag band is not a number; the conversions never see it.
        if !JSValue::from_bits(value.to_bits()).is_number() {
            continue;
        }
        check_number_box(value, js_number_to_string_box(value));
    }
}

#[test]
fn every_integer_across_the_sso_boundary_is_short() {
    for n in -9_999i32..=99_999 {
        let boxed = js_number_to_string_box(n as f64);
        assert!(JSValue::from_bits(boxed.to_bits()).is_short_string(), "{n}");
        assert_eq!(boxed_bytes(boxed), n.to_string().into_bytes(), "{n}");
    }
    for n in [100_000i32, -10_000, 123_456, -99_999] {
        let boxed = js_number_to_string_box(n as f64);
        assert!(JSValue::from_bits(boxed.to_bits()).is_string(), "{n}");
        assert_eq!(boxed_bytes(boxed), n.to_string().into_bytes(), "{n}");
    }
}

#[test]
fn coercion_box_twins_match_their_pointer_originals_on_numbers() {
    for &value in EDGES {
        check_number_box(value, crate::builtins::js_string_coerce_box(value));
        check_number_box(value, crate::builtins::js_template_string_coerce_box(value));
        check_number_box(value, crate::value::js_jsvalue_to_string_method_box(value));
    }
}

#[test]
fn coercion_box_twins_hand_a_string_back_unchanged() {
    let heap = boxed_heap(js_string_from_bytes(b"not short".as_ptr(), 9));
    let sso = f64::from_bits(JSValue::try_short_string(b"abc").unwrap().bits());
    for value in [heap, sso] {
        assert_eq!(
            crate::builtins::js_string_coerce_box(value).to_bits(),
            value.to_bits()
        );
        assert_eq!(
            crate::builtins::js_template_string_coerce_box(value).to_bits(),
            value.to_bits()
        );
        // The pointer original materializes an SSO argument; the text is the
        // same either way.
        assert_eq!(
            boxed_bytes(crate::builtins::js_string_coerce_box(value)),
            heap_bytes(crate::builtins::js_string_coerce(value))
        );
    }
}

#[test]
fn coercion_box_twins_match_their_pointer_originals_on_other_primitives() {
    let values = [
        f64::from_bits(crate::value::TAG_UNDEFINED),
        f64::from_bits(crate::value::TAG_NULL),
        f64::from_bits(crate::value::TAG_TRUE),
        f64::from_bits(crate::value::TAG_FALSE),
        f64::from_bits(JSValue::int32(-42).bits()),
        f64::from_bits(JSValue::int32(123_456).bits()),
    ];
    for value in values {
        assert_eq!(
            boxed_bytes(crate::builtins::js_string_coerce_box(value)),
            heap_bytes(crate::builtins::js_string_coerce(value)),
            "String({:016x})",
            value.to_bits()
        );
        assert_eq!(
            boxed_bytes(crate::builtins::js_template_string_coerce_box(value)),
            heap_bytes(crate::builtins::js_template_string_coerce(value)),
            "`${{{:016x}}}`",
            value.to_bits()
        );
    }
    // `.toString()` on a non-nullish primitive.
    for value in [values[2], values[4], values[5]] {
        assert_eq!(
            boxed_bytes(crate::value::js_jsvalue_to_string_method_box(value)),
            heap_bytes(crate::value::js_jsvalue_to_string_method(value)),
            "({:016x}).toString()",
            value.to_bits()
        );
    }
}

#[test]
fn empty_prefix_concat_packs_short_integers_and_prints_the_rest_unchanged() {
    // `"" + n`: the integer arm now also covers negatives, which used to be
    // minted on the heap; everything else still takes the general arm. The
    // text must be the pointer path's in every case.
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    let mut values: Vec<f64> = EDGES.to_vec();
    values.extend((-10_050..=-9_950).map(|n| n as f64));
    values.extend((99_950..=100_050).map(|n| n as f64));
    for value in values {
        let boxed = super::concat::js_string_concat_value_box(empty, value);
        let expected = heap_bytes(super::concat::js_string_concat_value(empty, value));
        assert_eq!(boxed_bytes(boxed), expected, "{:016x}", value.to_bits());
        let integral_sso = value.fract() == 0.0 && (-9_999.0..=99_999.0).contains(&value);
        if integral_sso {
            assert!(
                JSValue::from_bits(boxed.to_bits()).is_short_string(),
                "\"\" + {value} should be SSO"
            );
        }
    }
}
