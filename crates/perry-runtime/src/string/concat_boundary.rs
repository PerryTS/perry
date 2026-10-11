//! Canonical inputs can form a new surrogate pair only at their join.
use super::*;

#[inline]
pub(super) unsafe fn canonicalize_concat_boundary(
    result: *mut StringHeader,
    boundary: u32,
) -> *mut StringHeader {
    let flags = (*result).flags;
    if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 {
        return result;
    }
    if flags & STRING_FLAG_WTF8_VALIDATED != 0 {
        let length = (*result).byte_len;
        if boundary < 3 || length - boundary < 3 {
            return result;
        }
        let bytes = string_data(result);
        let join = boundary as usize;
        // Each negative byte test avoids a whole-payload scan. Canonical
        // WTF-8 proves the continuation bytes and the sequence boundaries.
        if *bytes.add(join) != 0xed
            || *bytes.add(join - 3) != 0xed
            || !(0xa0..=0xaf).contains(&*bytes.add(join - 2))
            || !(0xb0..=0xbf).contains(&*bytes.add(join + 1))
        {
            return result;
        }
    }
    // Unknown/raw payloads and a newly adjacent pair retain the full repair.
    super::concat::canonicalize_surrogate_pairs(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(bytes: &[u8]) -> *mut StringHeader {
        js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    #[test]
    fn boundary_pairs_and_lone_surrogates_match_utf16_units() {
        let parts: &[&[u8]] = &[
            b"",
            b"ascii",
            "é🙂".as_bytes(),
            &[0xed, 0xa0, 0xbd],
            &[0xed, 0xb8, 0x80],
            &[0xed, 0xa0, 0xbd, b'x'],
            &[b'x', 0xed, 0xb8, 0x80],
        ];
        for left in parts {
            for right in parts {
                let a = raw(left);
                let b = raw(right);
                let units: Vec<_> = (0..js_string_length(a))
                    .map(|i| js_string_char_code_at(a, i as i32))
                    .chain((0..js_string_length(b)).map(|i| js_string_char_code_at(b, i as i32)))
                    .collect();
                let pair =
                    left.ends_with(&[0xed, 0xa0, 0xbd]) && right.starts_with(&[0xed, 0xb8, 0x80]);
                let boxed = js_string_concat_box(
                    crate::value::js_nanbox_string(a as i64),
                    crate::value::js_nanbox_string(b as i64),
                );
                let dynamic = unsafe {
                    crate::value::js_dynamic_string_or_number_add(
                        crate::value::js_nanbox_string(a as i64),
                        crate::value::js_nanbox_string(b as i64),
                    )
                };
                for s in [
                    js_string_concat(a, b),
                    js_string_materialize_to_heap(boxed),
                    js_string_materialize_to_heap(dynamic),
                ] {
                    assert_eq!(js_string_length(s) as usize, units.len());
                    for (i, unit) in units.iter().enumerate() {
                        assert_eq!(js_string_char_code_at(s, i as i32), *unit);
                    }
                    assert_eq!(
                        unsafe { (*s).byte_len } as usize,
                        left.len() + right.len() - if pair { 2 } else { 0 }
                    );
                }
            }
        }
    }

    #[test]
    fn raw_interior_pairs_cannot_claim_the_boundary_only_proof() {
        let bytes = [b'x', 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, b'y'];
        let s = raw(&bytes);
        assert_eq!(unsafe { (*s).flags } & STRING_FLAG_WTF8_VALIDATED, 0);
        let joined = js_string_concat(s, js_string_from_str("z"));
        assert_eq!(unsafe { (*joined).byte_len }, 7);
        assert_eq!(js_string_length(joined), 5);
        assert_eq!(js_string_code_point_at(joined, 1), 0x1f600 as f64);
    }

    #[test]
    fn negative_control_forged_canonical_proof_breaks_string_equality() {
        let bytes = [b'x', 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, b'y'];
        let scope = crate::gc::RuntimeHandleScope::new();
        let suffix = scope.root_string_ptr(js_string_from_str("z"));
        let correct =
            scope.root_string_ptr(js_string_concat(raw(&bytes), suffix.get_raw_const_ptr()));
        let expected = scope.root_string_ptr(js_string_from_str("x😀yz"));
        assert_eq!(
            js_string_equals(correct.get_raw_const_ptr(), expected.get_raw_const_ptr()),
            1
        );
        let unmerged = raw(&bytes);
        // Deliberately break the admission proof. The lone-surrogate bit
        // remains set, so this cannot authorize an unchecked Rust &str.
        unsafe {
            (*unmerged).flags |= STRING_FLAG_WTF8_VALIDATED;
        }
        let wrong = js_string_concat(unmerged, suffix.get_raw_const_ptr());
        assert_eq!(
            js_string_equals(wrong, expected.get_raw_const_ptr()),
            0,
            "equality must distinguish skipped repair from the canonical literal"
        );
    }

    #[test]
    fn long_lone_surrogates_and_split_pair_keep_exact_lengths() {
        let mut bytes = vec![b'a'; 1_000_000];
        bytes.extend_from_slice(&[0xed, 0xa0, 0xbd]);
        let left = raw(&bytes);
        let right = raw(&[0xed, 0xb8, 0x80]);
        let joined = js_string_concat(left, right);
        assert_eq!(js_string_length(joined), 1_000_002);
        assert_eq!(unsafe { (*joined).byte_len }, 1_000_004);
        assert_eq!(js_string_code_point_at(joined, 1_000_000), 0x1f600 as f64);
    }
}
