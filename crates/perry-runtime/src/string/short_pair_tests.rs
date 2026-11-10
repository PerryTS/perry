//! The short-pair arm answers exactly where the general pairwise concat builds
//! an inline result, with the same bits, and declines everything else.

use super::*;
use crate::gc::RuntimeHandleScope;
use crate::string::{js_string_from_str, StringHeader, STRING_FLAG_WTF8_VALIDATED};
use crate::value::{JSValue, SHORT_STRING_TAG, STRING_TAG};

fn inline(text: &[u8]) -> f64 {
    f64::from_bits(JSValue::short_string_unchecked(text).bits())
}

fn heap_bits(ptr: *const StringHeader) -> f64 {
    f64::from_bits(STRING_TAG | ptr as u64)
}

#[test]
fn two_inline_operands_join_into_the_inline_value_of_the_concatenation() {
    for (a, b) in [
        (&b"e"[..], &b"2"[..]),
        (b"e2", b"e4"),
        (b"e7e8", b"q"),
        (b"", b"abcde"),
        (b"abcde", b""),
        (b"", b""),
        (b"ab", b"c"),
    ] {
        let joined = [a, b].concat();
        assert_eq!(
            concat_short_pair(inline(a), inline(b)).map(f64::to_bits),
            Some(JSValue::short_string_unchecked(&joined).bits()),
            "{:?} + {:?}",
            a,
            b
        );
    }
}

#[test]
fn a_short_validated_heap_operand_is_read_from_its_payload() {
    let scope = RuntimeHandleScope::new();
    let q = scope.root_string_ptr(js_string_from_str("q"));
    let empty = scope.root_string_ptr(js_string_from_str(""));
    let q = heap_bits(q.get_raw_const_ptr::<StringHeader>());
    let empty = heap_bits(empty.get_raw_const_ptr::<StringHeader>());
    assert_eq!(
        concat_short_pair(inline(b"e7e8"), q).map(f64::to_bits),
        Some(JSValue::short_string_unchecked(b"e7e8q").bits())
    );
    assert_eq!(
        concat_short_pair(inline(b"e2e4"), empty).map(f64::to_bits),
        Some(JSValue::short_string_unchecked(b"e2e4").bits())
    );
    assert_eq!(
        concat_short_pair(q, q).map(f64::to_bits),
        Some(JSValue::short_string_unchecked(b"qq").bits())
    );
    // A null string pointer reads as the empty string, as in the general path.
    assert_eq!(
        concat_short_pair(f64::from_bits(STRING_TAG), inline(b"ab")).map(f64::to_bits),
        Some(JSValue::short_string_unchecked(b"ab").bits())
    );
}

#[test]
fn bytes_above_the_inline_length_do_not_leak_into_the_result() {
    // Length 1 ("a") with junk in payload bytes 1..=4.
    let dirty = f64::from_bits(SHORT_STRING_TAG | (1u64 << 40) | 0x41_4141_4100 | b'a' as u64);
    assert_eq!(
        concat_short_pair(dirty, inline(b"b")).map(f64::to_bits),
        Some(JSValue::short_string_unchecked(b"ab").bits())
    );
}

#[test]
fn negative_controls_decline() {
    let scope = RuntimeHandleScope::new();
    // Too long for an inline result.
    assert_eq!(concat_short_pair(inline(b"abc"), inline(b"def")), None);
    // Non-ASCII inline bytes ("é" is two UTF-8 bytes): the general path
    // allocates a heap string here, so must this arm's caller.
    assert_eq!(concat_short_pair(inline("é".as_bytes()), inline(b"a")), None);
    assert_eq!(concat_short_pair(inline(b"a"), inline("é".as_bytes())), None);
    // Lone surrogate bytes (WTF-8 ED A0 80) in an inline value.
    assert_eq!(
        concat_short_pair(inline(&[0xED, 0xA0, 0x80]), inline(b"a")),
        None
    );
    // A heap operand over five bytes.
    let long = scope.root_string_ptr(js_string_from_str("abcdef"));
    let long = heap_bits(long.get_raw_const_ptr::<StringHeader>());
    assert_eq!(concat_short_pair(long, inline(b"")), None);
    // A short heap operand without the header's ASCII proof.
    let unproved = scope.root_string_ptr(js_string_from_str("ab"));
    let raw = unproved.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    unsafe { (*raw).flags &= !STRING_FLAG_WTF8_VALIDATED };
    assert_eq!(concat_short_pair(heap_bits(raw), inline(b"c")), None);
    // A short heap operand that is not ASCII.
    let accent = scope.root_string_ptr(js_string_from_str("é"));
    let accent = heap_bits(accent.get_raw_const_ptr::<StringHeader>());
    assert_eq!(concat_short_pair(accent, inline(b"a")), None);
    // Non-strings: numbers, undefined, null, booleans, a malformed length byte.
    for other in [
        1.0,
        -0.0,
        f64::NAN,
        f64::from_bits(JSValue::undefined().bits()),
        f64::from_bits(JSValue::null().bits()),
        f64::from_bits(JSValue::bool(true).bits()),
        f64::from_bits(SHORT_STRING_TAG | (200u64 << 40)),
    ] {
        assert_eq!(concat_short_pair(inline(b"a"), other), None);
        assert_eq!(concat_short_pair(other, inline(b"a")), None);
    }
}

#[test]
fn the_entry_points_return_the_short_pair_value() {
    let joined = JSValue::short_string_unchecked(b"e2e4").bits();
    assert_eq!(
        crate::string::js_string_concat_box(inline(b"e2"), inline(b"e4")).to_bits(),
        joined
    );
    let dynamic =
        unsafe { crate::value::js_dynamic_string_or_number_add(inline(b"e2"), inline(b"e4")) };
    assert_eq!(dynamic.to_bits(), joined);
    // A number operand still adds or concatenates per the spec.
    let mixed = unsafe { crate::value::js_dynamic_string_or_number_add(inline(b"a"), 1.0) };
    assert_eq!(
        mixed.to_bits(),
        JSValue::short_string_unchecked(b"a1").bits()
    );
    assert_eq!(
        unsafe { crate::value::js_dynamic_string_or_number_add(2.0, 3.0) },
        5.0
    );
}

#[test]
fn the_switch_is_default_on_and_only_explicit_off_spellings_turn_it_off() {
    assert!(switch_from_value(None));
    assert!(switch_from_value(Some("")));
    assert!(switch_from_value(Some("1")));
    assert!(switch_from_value(Some("typo")));
    for off in ["0", "off", "OFF", "false", "no", " 0 "] {
        assert!(!switch_from_value(Some(off)), "{off}");
    }
}
