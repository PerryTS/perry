use super::*;

unsafe fn check_accepted(bytes: &[u8]) -> bool {
    let source = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let Some(result) = try_heap_string(crate::JSValue::string_ptr(source).bits()) else {
        return false;
    };
    let mut expected = Vec::with_capacity(bytes.len() + 2);
    expected.push(b'"');
    if let Ok(text) = std::str::from_utf8(bytes) {
        let quoted = serde_json::to_string(text).unwrap();
        expected.extend_from_slice(&quoted.as_bytes()[1..quoted.len() - 1]);
    } else {
        expected.extend_from_slice(bytes);
    }
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
fn parsed_escape_free_string_quotes_directly_at_large_output_size() {
    unsafe {
        let payload = "x".repeat(crate::string::JSON_MALLOC_OUTPUT_THRESHOLD as usize);
        let json = format!("\"{payload}\"");
        let source = crate::string::js_string_from_bytes(json.as_ptr(), json.len() as u32);
        let parsed = super::super::test_json_parse_direct(source);
        let parsed = parsed.as_string_ptr();
        assert_ne!(
            (*parsed).flags & crate::string::STRING_FLAG_JSON_ESCAPE_FREE,
            0
        );

        let result = try_heap_string(crate::JSValue::string_ptr(parsed.cast_mut()).bits())
            .expect("parsed plain string fast path");
        let output = std::slice::from_raw_parts(string_data(result), (*result).byte_len as usize);
        assert_eq!(output, json.as_bytes());
        assert_eq!((*result).utf16_len, json.len() as u32);
    }
}

#[test]
fn direct_quoted_strings_accept_escapes_but_decline_surrogates() {
    unsafe {
        for special in [b'"', b'\\', b'\n', 0, 0x1f, 0xed] {
            for offset in [0, 15, 16, 31, 32, 63, 64, 127] {
                let mut bytes = vec![b'a'; 128];
                bytes[offset] = special;
                assert_eq!(check_accepted(&bytes), special != 0xed);
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

/// #10169: a large leaf stays malloc-tracked while the young generation is
/// smaller than the leaf or was measured as mostly garbage; it is born old in
/// the arena when the young generation holds at least the leaf's own bytes
/// and is unmeasured or measured as retained — the shape of a freshly parsed
/// document whose result the caller is about to stringify.
#[test]
fn large_json_leaf_routes_by_young_generation_occupancy() {
    if !crate::gc::gen_gc_enabled() {
        return;
    }
    let previous_survival = crate::gc::last_young_survival_permille();
    let _suppress = crate::gc::GcSuppressScope::new();
    let young_before = crate::arena::copying_from_space_in_use_bytes();
    let leaf = (young_before as u32 + (1 << 20)).max(crate::string::JSON_MALLOC_OUTPUT_THRESHOLD);
    crate::gc::seed_young_survival_for_tests(999);
    let (_, _, tracked_below) = crate::string::json_output_storage_alloc(leaf);
    assert!(
        tracked_below,
        "a young generation smaller than the leaf keeps malloc tracking"
    );

    let filler = vec![b'y'; 1024];
    while crate::arena::copying_from_space_in_use_bytes() < leaf as usize {
        crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);
    }
    if !crate::gc::young_generation_holds_a_nursery() {
        let (_, _, tracked_below_nursery) = crate::string::json_output_storage_alloc(leaf);
        assert!(
            tracked_below_nursery,
            "a young generation below one nursery keeps malloc tracking"
        );
        while !crate::gc::young_generation_holds_a_nursery() {
            crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);
        }
    }
    crate::gc::clear_young_survival_for_tests();
    let (_, _, tracked_unmeasured) = crate::string::json_output_storage_alloc(leaf);
    assert!(
        !tracked_unmeasured,
        "an unmeasured young generation at or above the leaf size births the leaf in the arena"
    );
    crate::gc::seed_young_survival_for_tests(100);
    let (_, _, tracked_dying) = crate::string::json_output_storage_alloc(leaf);
    assert!(
        tracked_dying,
        "a young generation measured as mostly garbage keeps malloc tracking"
    );
    crate::gc::seed_young_survival_for_tests(999);
    let (_, _, tracked_retained) = crate::string::json_output_storage_alloc(leaf);
    assert!(
        !tracked_retained,
        "a retained young generation at or above the leaf size births the leaf in the arena"
    );

    match previous_survival {
        Some(permille) => crate::gc::seed_young_survival_for_tests(permille),
        None => crate::gc::clear_young_survival_for_tests(),
    }
}
