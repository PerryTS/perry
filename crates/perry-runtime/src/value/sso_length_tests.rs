//! #10191: inline string storage must not change JavaScript's UTF-16 length.

use super::*;

#[test]
fn non_ascii_sso_length_matches_heap_strings_through_every_runtime_path() {
    let cases: &[(&[u8], u32)] = &[
        (b"", 0),
        (b"abcde", 5),
        (b"a\0b", 3),
        ("é".as_bytes(), 1),
        ("éé".as_bytes(), 2),
        ("éabc".as_bytes(), 4),
        ("abcé".as_bytes(), 4),
        ("中".as_bytes(), 1),
        ("é中".as_bytes(), 2),
        ("😀".as_bytes(), 2),
        ("a😀".as_bytes(), 3),
        ("😀a".as_bytes(), 3),
        ("e\u{301}".as_bytes(), 2),
        (&[0xED, 0xA0, 0x80], 1),
        (&[b'a', 0xED, 0xBF, 0xBF, b'b'], 3),
    ];
    for &(bytes, expected) in cases {
        let value = JSValue::try_short_string(bytes).expect("fixture must stay inline");
        assert!(value.is_short_string());
        assert_eq!(
            value.short_string_len(),
            bytes.len(),
            "storage still counts bytes"
        );
        let boxed = f64::from_bits(value.bits());
        assert_eq!(
            js_value_length_f64(boxed),
            expected as f64,
            "bytes={bytes:?}"
        );
        assert_eq!(js_value_length_property_f64(boxed), expected as f64);

        let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let key = f64::from_bits(JSValue::string_ptr(key).bits());
        assert_eq!(
            crate::string::js_string_index_get_boxed(boxed, key),
            expected as f64
        );

        let mut cursor = crate::string::suffix_cursor::SuffixCursor::default();
        unsafe {
            assert_eq!(
                crate::string::suffix_cursor::js_string_suffix_length(boxed, &cursor),
                expected as f64
            );
            crate::string::suffix_cursor::js_string_suffix_advance(boxed, &mut cursor, 1);
            assert_eq!(
                crate::string::suffix_cursor::js_string_suffix_length(boxed, &cursor),
                expected.saturating_sub(1) as f64
            );
        }
    }
}

#[test]
fn malformed_sso_length_keeps_the_heap_counter_convention() {
    for bytes in [
        &[0x80][..],
        &[0xF0][..],
        &[0xE2, 0x82][..],
        &[0xC3, b'a', b'b'][..],
        &[b'a', 0xF0, b'b', b'c', b'd'][..],
        &[0xED, 0xA0, 0x80, 0x80, b'x'][..],
    ] {
        let heap = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let expected = crate::string::js_string_length(heap);
        let sso = JSValue::try_short_string(bytes).unwrap();
        assert_eq!(
            js_value_length_f64(f64::from_bits(sso.bits())),
            expected as f64,
            "bytes={bytes:?}"
        );
    }
}
