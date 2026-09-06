use super::*;

unsafe fn check_accepted(bytes: &[u8]) -> bool {
    let source = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let Some(result) = try_heap_string(crate::JSValue::string_ptr(source).bits()) else {
        return false;
    };
    let mut expected = Vec::with_capacity(bytes.len() + 2);
    expected.push(b'"');
    expected.extend_from_slice(bytes);
    expected.push(b'"');
    assert_eq!(
        std::slice::from_raw_parts(string_data(result), (*result).byte_len as usize),
        expected
    );
    assert_eq!(
        (*result).utf16_len,
        crate::string::compute_utf16_len(expected.as_ptr(), expected.len() as u32)
    );
    assert_eq!((*result).flags, 0);
    assert_eq!((*result).capacity, (*result).byte_len);
    true
}

#[test]
fn direct_quoted_strings_preserve_bytes_and_utf16_lengths() {
    unsafe {
        for n in [64, 65, 127, 128, 129, 4095, 4096, 4097] {
            for unit in ["a", "é", "東京", "🙂", "\u{2028}\u{2029}"] {
                assert!(check_accepted(unit.repeat(n).as_bytes()));
            }
        }
        assert!(!check_accepted(b"short"));
        assert!(try_heap_string(crate::value::TAG_NULL).is_none());
        assert!(try_heap_string(0.0f64.to_bits()).is_none());
    }
}

#[test]
fn direct_quoted_strings_decline_escapes_and_surrogates() {
    unsafe {
        for special in [b'"', b'\\', b'\n', 0, 0x1f, 0xed] {
            for offset in [0, 15, 16, 31, 32, 63, 64, 127] {
                let mut bytes = vec![b'a'; 128];
                bytes[offset] = special;
                assert!(!check_accepted(&bytes));
            }
        }
    }
}

#[test]
fn direct_quoted_raw_strings_preserve_fallback_length_semantics() {
    unsafe {
        // Any accepted raw tail must have exactly the metadata the existing
        // output factory computes. Incomplete sequences fall back instead.
        for a in 0x80..=255u8 {
            for b in 0x80..=255u8 {
                let mut bytes = vec![b'a'; 64];
                bytes.extend_from_slice(&[a, b]);
                check_accepted(&bytes);
                bytes.extend_from_slice(b"abc");
                if a != 0xed && b != 0xed {
                    assert!(check_accepted(&bytes));
                }
            }
        }
        for tail in [b"\xc2".as_slice(), b"\xe2\x82", b"\xf0\x9f\x99"] {
            let mut bytes = vec![b'a'; 64];
            bytes.extend_from_slice(tail);
            assert!(!check_accepted(&bytes));
        }
    }
}
