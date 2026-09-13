//! Context-dependent default casing (#10116).

use super::*;

#[test]
fn lowercase_final_sigma_uses_cased_and_case_ignorable_context() {
    // Expected strings were checked against the pinned Node oracle. In
    // particular, Case_Ignorable characters must be skipped on BOTH sides;
    // U+0345 is also Cased, so testing Cased first would be incorrect.
    let cases = [
        ("ΑΣ", "ας"),
        ("ΟΔΥΣΣΕΥΣ", "οδυσσευς"),
        ("Σ", "σ"),
        ("ΣΣ", "σς"),
        ("ΑΣΑ", "ασα"),
        ("AΣ! AΣB", "aς! aσb"),
        ("1Σ", "1σ"),
        ("中Σ", "中σ"),
        ("AΣ1", "aς1"),
        ("AΣ中", "aς中"),
        ("A\u{301}Σ", "a\u{301}ς"),
        ("AΣ\u{301}", "aς\u{301}"),
        ("AΣ\u{301}B", "aσ\u{301}b"),
        ("\u{301}Σ", "\u{301}σ"),
        ("A'Σ", "a'ς"),
        ("AΣ'B", "aσ'b"),
        ("AΣ\u{200d}", "aς\u{200d}"),
        ("AΣ\u{200d}B", "aσ\u{200d}b"),
        ("\u{345}Σ", "\u{345}σ"),
        ("A\u{345}Σ", "a\u{345}ς"),
        ("AΣ\u{345}", "aς\u{345}"),
        ("AΣ\u{345}B", "aσ\u{345}b"),
        ("𐐀Σ", "𐐨ς"),
        ("AΣ𐐀", "aσ𐐨"),
        ("😀Σ", "😀σ"),
        ("AΣ😀", "aς😀"),
        ("İΣ", "i\u{307}ς"),
        ("AΣİ", "aσi\u{307}"),
    ];
    for (input, expected) in cases {
        let source = js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let result = js_string_to_lower_case(source);
        assert_eq!(string_as_str(result), expected, "input: {input:?}");
        assert_eq!(
            unsafe { (*result).utf16_len },
            expected.encode_utf16().count() as u32
        );
    }
}

#[test]
fn lowercase_final_sigma_preserves_lone_surrogate_boundaries() {
    for surrogate in [[0xED, 0xA0, 0x80], [0xED, 0xBF, 0xBF]] {
        for (before, after, lower_before, lower_after) in [
            ("AΣ", "ΣA", "aς", "σa"),
            ("A", "Σ", "a", "σ"),
            ("", "AΣ", "", "aς"),
            ("AΣ\u{301}", "B", "aς\u{301}", "b"),
        ] {
            let input = [before.as_bytes(), &surrogate, after.as_bytes()].concat();
            let expected = [lower_before.as_bytes(), &surrogate, lower_after.as_bytes()].concat();
            let source = js_string_from_wtf8_bytes(input.as_ptr(), input.len() as u32);
            let result = js_string_to_lower_case(source);
            let bytes =
                unsafe { slice::from_raw_parts(string_data(result), (*result).byte_len as usize) };
            assert_eq!(bytes, expected);
            assert_ne!(
                unsafe { (*result).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
                0
            );
            assert_eq!(
                unsafe { (*result).utf16_len },
                compute_utf16_len_wtf8(&expected)
            );
        }
    }
}
