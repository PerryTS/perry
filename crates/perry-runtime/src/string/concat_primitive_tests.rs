use super::*;
use crate::value::{JSValue, STRING_TAG};

fn inline(text: &[u8]) -> f64 {
    f64::from_bits(JSValue::short_string_unchecked(text).bits())
}

fn chain(parts: &[f64]) -> *mut StringHeader {
    js_string_concat_chain(parts.as_ptr(), parts.len() as i32)
}

fn bytes(s: *const StringHeader) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(string_data(s), (*s).byte_len as usize).to_vec() }
}

#[test]
fn numeric_and_inline_operands_reach_no_collection_point() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let prefix = scope.root_string_ptr(js_string_from_str("number="));
    let suffix = scope.root_string_ptr(js_string_from_str("é"));
    let parts = || {
        [
            f64::from_bits(STRING_TAG | prefix.get_raw_const_ptr::<StringHeader>() as u64),
            123456.0,
            inline(b";"),
            f64::from_bits(STRING_TAG | suffix.get_raw_const_ptr::<StringHeader>() as u64),
        ]
    };
    // Warm the allocator using exactly the result size, then reload roots.
    let _ = chain(&parts());
    let parts = parts();
    let hits = CONCAT_CHAIN_NO_COLLECT_HITS.with(|c| c.get());
    crate::arena::reset_gc_trigger_arena_probe();
    let before = crate::arena::gc_trigger_arena_calls();
    let result = concat_chain_primitives::<4>(parts.as_ptr(), 4)
        .expect("test premise: the warmed nursery must serve the result");
    assert_eq!(bytes(result), "number=123456;é".as_bytes());
    assert_eq!(crate::arena::gc_trigger_arena_calls(), before);
    assert_eq!(CONCAT_CHAIN_NO_COLLECT_HITS.with(|c| c.get()), hits + 1);
}

#[test]
fn primitive_chains_preserve_number_to_string_edges_and_dispatch_sizes() {
    for (value, expected_number) in [
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (-1.0, "-1"),
        (99999.0, "99999"),
        (100000.0, "100000"),
        (2147483647.0, "2147483647"),
        (-2147483648.0, "-2147483648"),
        (4294967296.0, "4294967296"),
        (999999999999999.0, "999999999999999"),
        (1e15, "1000000000000000"),
        (1e20, "100000000000000000000"),
        (1e21, "1e+21"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (0.1, "0.1"),
        (-0.5, "-0.5"),
        (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
        (f64::from_bits(1), "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        for count in [3, 7, 15, 32] {
            let mut parts = vec![inline(b"x"); count];
            parts[1] = value;
            let mut expected = vec![b'x'];
            expected.extend_from_slice(expected_number.as_bytes());
            expected.extend(std::iter::repeat_n(b'x', count - 2));
            let result = chain(&parts);
            assert_eq!(bytes(result), expected, "{value} / {count}");
            assert_eq!(js_string_length(result) as usize, expected.len());
        }
    }
}

#[test]
fn primitive_chains_keep_wtf8_repair_and_raw_provenance() {
    let cases: &[&[u8]] = &[
        b"",
        "é🙂".as_bytes(),
        &[0xed, 0xa0, 0xbd],
        &[0xed, 0xb8, 0x80],
        &[0xc3],
        &[0x80],
        &[b'x', 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, b'y'],
    ];
    for left in cases {
        for right in cases {
            let scope = crate::gc::RuntimeHandleScope::new();
            let a =
                scope.root_string_ptr(js_string_from_wtf8_bytes(left.as_ptr(), left.len() as u32));
            let b = scope.root_string_ptr(js_string_from_wtf8_bytes(
                right.as_ptr(),
                right.len() as u32,
            ));
            let expected = js_string_concat(a.get_raw_const_ptr(), b.get_raw_const_ptr());
            let expected_bytes = bytes(expected);
            let expected_len = js_string_length(expected);
            let parts = [
                f64::from_bits(STRING_TAG | a.get_raw_const_ptr::<StringHeader>() as u64),
                inline(b""),
                f64::from_bits(STRING_TAG | b.get_raw_const_ptr::<StringHeader>() as u64),
            ];
            let result = chain(&parts);
            assert_eq!(bytes(result), expected_bytes, "{left:?} / {right:?}");
            assert_eq!(js_string_length(result), expected_len);
        }
    }
}

#[test]
fn unsupported_values_are_refused_before_allocating_or_converting() {
    let parts = [inline(b"x"), f64::from_bits(crate::value::TAG_UNDEFINED)];
    crate::arena::reset_gc_trigger_arena_probe();
    let before = crate::arena::gc_trigger_arena_calls();
    assert!(concat_chain_primitives::<4>(parts.as_ptr(), 2).is_none());
    assert_eq!(crate::arena::gc_trigger_arena_calls(), before);
    assert_eq!(bytes(chain(&parts)), b"xundefined");
}

#[test]
fn inline_non_ascii_and_raw_bytes_keep_utf16_lengths_and_flags() {
    for source in [
        "é".as_bytes(),
        "🙂".as_bytes(),
        &[0xed, 0xa0, 0xbd],
        &[0xed, 0xb8, 0x80],
        &[0xc3],
        &[0x80],
        &[b'a', 0, b'b'],
    ] {
        let scope = crate::gc::RuntimeHandleScope::new();
        let suffix = scope.root_string_ptr(js_string_from_char_code(0xde00 as f64));
        let materialized = scope.root_string_ptr(js_string_from_wtf8_bytes(
            source.as_ptr(),
            source.len() as u32,
        ));
        let expected =
            js_string_concat(materialized.get_raw_const_ptr(), suffix.get_raw_const_ptr());
        let expected_bytes = bytes(expected);
        let expected_len = js_string_length(expected);
        let parts = [
            inline(source),
            inline(b""),
            f64::from_bits(STRING_TAG | suffix.get_raw_const_ptr::<StringHeader>() as u64),
        ];
        let result = chain(&parts);
        assert_eq!(bytes(result), expected_bytes, "{source:?}");
        assert_eq!(js_string_length(result), expected_len, "{source:?}");
        let (units, flags) = raw_string_metadata(source.as_ptr(), source.len() as u32);
        let result = chain(&[inline(source), 7.0]);
        assert_eq!(js_string_length(result), units + 1);
        assert_eq!(
            unsafe { (*result).flags } & STRING_FLAG_WTF8_VALIDATED,
            flags & STRING_FLAG_WTF8_VALIDATED
        );
    }
}

#[test]
fn boxed_numeric_concat_preserves_the_inline_capacity_boundary() {
    for prefix in ["", "k", "ab", "abc", "abcd", "abcde", "é", "🙂"] {
        let scope = crate::gc::RuntimeHandleScope::new();
        let prefix = scope.root_string_ptr(js_string_from_str(prefix));
        for value in [
            9999.0,
            10000.0,
            99999.0,
            100000.0,
            999999.0,
            1e9,
            u32::MAX as f64 - 1.0,
            u32::MAX as f64,
            u32::MAX as f64 + 0.5,
            u32::MAX as f64 + 1.0,
            -0.0,
            -100000.0,
            0.5,
            f64::NAN,
        ] {
            let expected = bytes(js_string_concat_value(prefix.get_raw_const_ptr(), value));
            let result = js_string_concat_value_box(prefix.get_raw_const_ptr(), value);
            if expected.len() > crate::value::SHORT_STRING_MAX_LEN {
                assert!(!JSValue::from_bits(result.to_bits()).is_short_string());
            }
            assert_eq!(bytes(js_string_materialize_to_heap(result)), expected);
        }
    }
}
