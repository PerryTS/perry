use super::*;

fn proven(s: *const StringHeader) -> bool {
    unsafe { (*s).flags & STRING_FLAG_WTF8_VALIDATED != 0 }
}

fn heap(text: &str) -> *mut StringHeader {
    js_string_from_str(text)
}

#[test]
fn construction_proves_utf8_and_generalized_wtf8() {
    for text in ["", "plain ascii", "é中🙂", "a\0b"] {
        let s = heap(text);
        assert!(proven(s));
        assert_eq!(unsafe { header_str_checked(s) }, Some(text));
        assert_eq!(
            unsafe { (*s).utf16_len as usize },
            text.encode_utf16().count()
        );
    }
    let bytes = [0xed, 0xa0, 0x80, b'x', 0xed, 0xbf, 0xbf];
    let s = js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32);
    assert!(proven(s));
    assert_eq!(unsafe { (*s).utf16_len }, 3);
    assert!(unsafe { header_str_checked(s) }.is_none());
    assert_eq!(js_string_char_code_at(s, 0), 0xd800 as f64);
    assert_eq!(js_string_char_code_at(s, 2), 0xdfff as f64);
}

#[cfg(feature = "regex-engine")]
#[test]
fn raw_byte_mutations_agree_with_the_regex_encoding_contract() {
    let mut bytes = b"a\xc3\xa9\xed\xa0\x80\xf0\x9f\x99\x82".to_vec();
    for at in 0..bytes.len() {
        let original = bytes[at];
        for byte in 0..=255 {
            bytes[at] = byte;
            let (units, flags) = raw_string_metadata(bytes.as_ptr(), bytes.len() as u32);
            match perex::input::Input::wtf8(&bytes) {
                Ok(input) => {
                    assert_ne!(flags & STRING_FLAG_WTF8_VALIDATED, 0);
                    assert_eq!(units as usize, input.len_utf16());
                    assert_eq!(
                        flags & STRING_FLAG_HAS_LONE_SURROGATES != 0,
                        std::str::from_utf8(&bytes).is_err()
                    );
                }
                Err(_) => assert_eq!(flags & STRING_FLAG_WTF8_VALIDATED, 0),
            }
        }
        bytes[at] = original;
    }
}

#[test]
fn invalid_raw_bytes_cannot_gain_proof_from_matching_lengths_or_concat() {
    // Negative control: truncated leads used to pass the length-equality
    // shortcut and manufacture an invalid Rust &str.
    for bytes in [
        &[0xc3][..],
        &[0xf0, b'A'],
        &[0xc0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
    ] {
        let raw = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        assert!(!proven(raw));
        assert!(unsafe { header_str_checked(raw) }.is_none());
        assert!(!is_ascii_string(raw));
        assert_eq!(crate::value::js_is_truthy(js_string_is_well_formed(raw)), 0);
        let good = heap("ascii");
        let result = js_string_concat(good, raw);
        assert!(!proven(result), "one proven operand cannot certify both");
        assert!(unsafe { header_str_checked(result) }.is_none());
        let dest = js_string_from_bytes_with_capacity(b"ab".as_ptr(), 2, 64);
        unsafe {
            (*dest).refcount = 1;
        }
        let appended = js_string_append(dest, raw);
        assert_eq!(dest, appended, "control must reach in-place append");
        assert!(!proven(appended));
    }
}

#[test]
fn slice_concat_and_chain_preserve_exact_proof_and_surrogate_units() {
    let text = heap("a🙂éz");
    let high = js_string_slice(text, 1, 2);
    let low = js_string_slice(text, 2, 3);
    assert!(proven(high) && proven(low));
    assert_eq!(js_string_char_code_at(high, 0), 0xd83d as f64);
    assert_eq!(js_string_char_code_at(low, 0), 0xde42 as f64);
    let merged = js_string_concat(high, low);
    assert!(proven(merged));
    assert_eq!(unsafe { header_str_checked(merged) }, Some("🙂"));
    let parts = [
        crate::value::js_nanbox_string(high as i64),
        crate::value::js_nanbox_string(low as i64),
    ];
    let chain = js_string_concat_chain(parts.as_ptr(), 2);
    assert!(proven(chain));
    assert_eq!(unsafe { header_str_checked(chain) }, Some("🙂"));
    let boxed = js_string_concat_box(parts[0], parts[1]);
    let merged = crate::value::JSValue::from_bits(boxed.to_bits()).as_string_ptr();
    assert!(proven(merged));
    assert_eq!(unsafe { header_str_checked(merged) }, Some("🙂"));
    let ascii = js_string_slice(text, 4, 5);
    assert!(proven(ascii));
    assert_eq!(unsafe { header_str_checked(ascii) }, Some("z"));
}

#[test]
fn index_conversion_matches_saturation_and_numeric_coercion() {
    for (n, expected) in [
        (f64::NAN, 0),
        (f64::INFINITY, i32::MAX),
        (f64::NEG_INFINITY, i32::MIN),
        (1.9, 1),
        (-1.9, -1),
        (-0.0, 0),
        (2147483647.9, i32::MAX),
        (-2147483648.9, i32::MIN),
    ] {
        assert_eq!(js_string_index_to_i32(n), expected);
        assert_eq!(slice_ops::js_string_position_to_index(n), expected);
    }
    // Negative control for the numeric path: ToNumber still runs on strings.
    assert_eq!(
        js_string_index_to_i32(crate::value::js_nanbox_string(heap("-1.9") as i64)),
        -1
    );
    assert_eq!(
        js_string_index_to_i32(f64::from_bits(crate::value::TAG_TRUE)),
        1
    );
}

#[test]
fn search_handles_single_byte_surrogate_halves_and_overlaps() {
    let text = heap("a🙂é🙂z");
    let high = js_string_slice(text, 1, 2);
    let low = js_string_slice(text, 2, 3);
    assert_eq!(js_string_index_of_from(text, high, 0), 1);
    assert_eq!(js_string_index_of_from(text, low, 0), 2);
    assert_eq!(js_string_index_of_from(text, low, 3), 5);
    assert_eq!(js_string_last_index_of(text, low), 5);
    assert_eq!(js_string_index_of_from(text, heap("🙂"), 2), 4);
    assert_eq!(js_string_index_of_from(text, heap("z"), 0), 6);
    // Negative controls: non-ASCII one-scalar and multi-byte needles retain
    // substring semantics; an overlapping match must not be skipped.
    assert_eq!(js_string_index_of_from(text, heap("é"), 0), 3);
    assert_eq!(js_string_index_of_from(text, heap("🙂é"), 0), 1);
    assert_eq!(
        js_string_last_index_of_from(heap("aaaa"), heap("aa"), 1.0, 1),
        1
    );
    assert_eq!(js_string_index_of_from(text, heap(""), i32::MAX), 7);
}

#[test]
fn very_long_flat_payloads_keep_proof_and_correct_boundaries() {
    // Perry uses flat payloads, not ropes. Exercise both the large allocation
    // path and the substring's byte/UTF-16 offset conversion.
    let text = "aé🙂".repeat(150_000);
    let s = heap(&text);
    assert!(proven(s));
    assert_eq!(unsafe { (*s).utf16_len }, 600_000);
    let tail = js_string_slice(s, 599_996, 600_000);
    assert!(proven(tail));
    assert_eq!(unsafe { header_str_checked(tail) }, Some("aé🙂"));
    assert_eq!(js_string_index_of_from(s, heap("é🙂"), 599_996), 599_997);
}

#[test]
fn prehashed_intern_probe_checks_bytes_on_a_hash_collision() {
    let a = b"strops-first";
    let b = b"strops-other";
    let hash = crate::object::key_bytes_hash(a.as_ptr(), a.len());
    let first = intern::intern_prehashed_bytes(a.as_ptr(), a.len(), hash, false);
    assert_eq!(
        first,
        intern::intern_prehashed_bytes(a.as_ptr(), a.len(), hash, false)
    );
    // Negative control: deliberately identical hashes cannot merge unequal
    // text. Atoms are a separate identity system and never become interned.
    let other = intern::intern_prehashed_bytes(b.as_ptr(), b.len(), hash, false);
    assert_ne!(first, other);
    assert_eq!(unsafe { header_str_checked(other) }, Some("strops-other"));
    let atom = js_string_pool_atom(a.as_ptr(), a.len() as u32, hash, 0);
    assert_ne!(atom, first as *mut StringHeader);
    assert_eq!(unsafe { header_str_checked(atom) }, Some("strops-first"));
}
