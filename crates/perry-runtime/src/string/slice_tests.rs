use super::suffix_cursor::*;
use super::*;
use crate::value::JSValue;

fn units(s: *const StringHeader) -> Vec<u16> {
    (0..unsafe { (*s).utf16_len })
        .map(|i| js_string_char_code_at(s, i as i32) as u16)
        .collect()
}

#[test]
fn slice_utf16_bounds_and_lone_surrogates() {
    for bytes in [
        b"".as_slice(),
        b"abc",
        "ä中😀Ö".as_bytes(),
        "😀😀".as_bytes(),
        b"\xed\xa0\x80A\xed\xbf\xbf",
        b"\xed\xa0\x80\xf0\x9f\x98\x80\xed\xbf\xbf",
    ] {
        let scope = crate::gc::RuntimeHandleScope::new();
        let source = js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32);
        let root = scope.root_string_ptr(source);
        let expected = units(source);
        let len = expected.len() as i32;
        for start in -len - 2..=len + 2 {
            for end in -len - 2..=len + 2 {
                let normalize = |n: i32| if n < 0 { (len + n).max(0) } else { n.min(len) };
                let a = normalize(start) as usize;
                let b = normalize(end) as usize;
                let result = root.with_const_ptr(|s| js_string_slice(s, start, end));
                assert_eq!(
                    units(result),
                    expected[a..b.max(a)],
                    "slice({start}, {end}) of {expected:?}"
                );
                let payload = unsafe {
                    slice::from_raw_parts(string_data(result), (*result).byte_len as usize)
                };
                assert_eq!(
                    unsafe { (*result).utf16_len },
                    compute_utf16_len_wtf8(payload)
                );
                assert_eq!(
                    unsafe { (*result).flags } & STRING_FLAG_HAS_LONE_SURROGATES != 0,
                    bytes_have_lone_surrogate(payload)
                );
                let a = start.clamp(0, len) as usize;
                let b = end.clamp(0, len) as usize;
                let result = root.with_const_ptr(|s| js_string_substring(s, start, end));
                assert_eq!(units(result), expected[a.min(b)..a.max(b)]);
            }
        }
        assert!(units(root.with_const_ptr(|s| js_string_slice(s, i32::MIN, i32::MAX))) == expected);
    }
}

#[test]
fn suffix_cursor_matches_code_units_and_clamps_reads_and_advances() {
    for text in ["", "abc", "ä中😀Ö", "😀😀"] {
        let source = js_string_from_str(text);
        let boxed = f64::from_bits(JSValue::string_ptr(source).bits());
        let expected: Vec<u16> = text.encode_utf16().collect();
        for step in [0, 1, 2, 3, 20] {
            let mut cursor = SuffixCursor::default();
            let mut consumed = 0;
            for _ in 0..expected.len() + 2 {
                unsafe {
                    assert_eq!(
                        js_string_suffix_length(boxed, &cursor),
                        (expected.len() - consumed) as f64
                    );
                    for index in -1..=expected.len() as i32 {
                        let got = js_string_suffix_char_code_at(boxed, &cursor, index);
                        if index < 0 || consumed + index as usize >= expected.len() {
                            assert!(got.is_nan());
                        } else {
                            assert_eq!(got, expected[consumed + index as usize] as f64);
                        }
                    }
                    js_string_suffix_advance(boxed, &mut cursor, step);
                }
                consumed = (consumed + step as usize).min(expected.len());
            }
        }
    }
    let short = f64::from_bits(JSValue::try_short_string(b"abc").unwrap().bits());
    let mut cursor = SuffixCursor::default();
    unsafe {
        js_string_suffix_advance(short, &mut cursor, 1);
        assert_eq!(js_string_suffix_length(short, &cursor), 2.0);
        assert_eq!(js_string_suffix_char_code_at(short, &cursor, 0), 98.0);
    }
}

#[test]
fn suffix_cursor_skips_unpaired_continuation_bytes_like_char_code_at() {
    let bytes = b"\x80A\x80\xf0\x9f\x98\x80\x80B";
    let source = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let boxed = f64::from_bits(JSValue::string_ptr(source).bits());
    let expected = units(source);
    assert_eq!(expected, [65, 55357, 56832, 66]);
    let mut cursor = SuffixCursor::default();
    for consumed in 0..=expected.len() {
        unsafe {
            for index in 0..expected.len() - consumed {
                assert_eq!(
                    js_string_suffix_char_code_at(boxed, &cursor, index as i32),
                    expected[consumed + index] as f64
                );
            }
            assert!(js_string_suffix_char_code_at(
                boxed,
                &cursor,
                (expected.len() - consumed) as i32
            )
            .is_nan());
            js_string_suffix_advance(boxed, &mut cursor, 1);
        }
    }
}
