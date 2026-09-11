use super::*;

fn reset_steps() {
    DECODE_STEPS.with(|steps| steps.set(0));
}

fn steps() -> usize {
    DECODE_STEPS.with(|steps| steps.get())
}

#[test]
fn sequential_decode_work_is_linear() {
    for n in [1_000, 10_000, 100_000] {
        let text = "ä中😀Ö".repeat(n);
        let expected: Vec<u16> = text.encode_utf16().collect();
        let scope = crate::gc::RuntimeHandleScope::new();
        let s = scope.root_string_ptr(js_string_from_str(&text));
        prune_dead_utf16_indexes(&|_| true);
        reset_steps();
        s.with_const_ptr(|s_ptr: *const StringHeader| {
            for (i, &unit) in expected.iter().enumerate() {
                assert_eq!(js_string_char_code_at(s_ptr, i as i32), unit as f64);
            }
            assert!(
                steps() >= expected.len(),
                "the public API must exercise the indexed decoder"
            );
            assert!(
                steps() <= expected.len() * 2,
                "prefix decoding must not restart per index"
            );
            assert!(test_utf16_index_entries().iter().any(|&(owner, count)| {
                owner == s_ptr as usize && count > 0 && count <= text.len() / CHECKPOINT_BYTES
            }));
        });
    }
    prune_dead_utf16_indexes(&|_| true);
}

#[test]
fn reverse_and_random_reads_use_checkpoints() {
    let text = "aé中😀🦀".repeat(4096);
    let expected: Vec<u16> = text.encode_utf16().collect();
    let mut index = Index::default();
    // A first lookup at the end builds checkpoints in one bounded walk.
    assert_eq!(
        index.unit_at(text.as_bytes(), expected.len() - 1),
        expected.last().copied()
    );
    let mut seed = 0x1234_5678u32;
    for pass in 0..3 {
        reset_steps();
        for j in 0..expected.len() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let i = if pass == 0 {
                expected.len() - 1 - j
            } else {
                seed as usize % expected.len()
            };
            assert_eq!(index.unit_at(text.as_bytes(), i), Some(expected[i]));
        }
        assert!(steps() <= expected.len() * (4 * CHECKPOINT_BYTES + 1));
    }
}

#[test]
fn bounded_wtf8_reads_preserve_lone_surrogates_and_truncated_tails() {
    let mut bytes = "中😀".repeat(100).into_bytes();
    let mut expected: Vec<u16> = "中😀".repeat(100).encode_utf16().collect();
    bytes.extend_from_slice(&[0xed, 0xa0, 0x80, b'x', 0xed, 0xb0, 0x80, 0xc3]);
    expected.extend_from_slice(&[0xd800, b'x' as u16, 0xdc00, 0xc0]);
    let mut index = Index::default();
    for i in (0..expected.len()).rev().chain(0..expected.len()) {
        assert_eq!(index.unit_at(&bytes, i), Some(expected[i]));
    }
    assert_eq!(index.unit_at(&bytes, expected.len()), None);
    assert_eq!(index.unit_at(&bytes, usize::MAX), None);
    assert_eq!(index.unit_at(&bytes, 0), Some(0x4e2d));
}

#[test]
fn at_preserves_surrogate_halves_at_positive_and_negative_indexes() {
    for n in [1, 128] {
        let bytes = [0xc3, 0xa4, 0xf0, 0x9f, 0x98, 0x80, 0xed, 0xa0, 0x80].repeat(n);
        let expected = [228, 0xd83d, 0xde00, 0xd800].repeat(n);
        let scope = crate::gc::RuntimeHandleScope::new();
        let s = scope.root_string_ptr(js_string_from_wtf8_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        ));
        for (i, &unit) in expected.iter().enumerate() {
            for index in [i as i32, i as i32 - expected.len() as i32] {
                let ch = s.with_const_ptr(|s_ptr| js_string_at(s_ptr, index));
                let ptr = crate::value::js_get_string_pointer_unified(ch) as *const StringHeader;
                assert_eq!(js_string_char_code_at(ptr, 0), unit as f64);
                assert_eq!(unsafe { (*ptr).utf16_len }, 1);
            }
        }
        for index in [expected.len() as i32, -(expected.len() as i32) - 1] {
            assert_eq!(
                s.with_const_ptr(|s_ptr| js_string_at(s_ptr, index))
                    .to_bits(),
                crate::value::TAG_UNDEFINED
            );
        }
    }
    prune_dead_utf16_indexes(&|_| true);
}

#[test]
fn cache_distinguishes_strings_and_invalidates_in_place_appends() {
    prune_dead_utf16_indexes(&|_| true);
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_string_ptr(js_string_from_str(&"中😀".repeat(100)));
    let b = scope.root_string_ptr(js_string_from_str(&"ä🦀".repeat(100)));
    for _ in 0..3 {
        assert_eq!(
            a.with_const_ptr(|a_ptr| js_string_char_code_at(a_ptr, 299)),
            0xde00 as f64
        );
        assert_eq!(
            b.with_const_ptr(|b_ptr| js_string_char_code_at(b_ptr, 299)),
            0xdd80 as f64
        );
        assert_eq!(
            a.with_const_ptr(|a_ptr| js_string_char_code_at(a_ptr, 1)),
            0xd83d as f64
        );
    }
    // Model append's header update inside capacity, without a GC allocation.
    let text = "中".repeat(100);
    let (s, data) = string_storage_alloc(text.len() as u32 + 4);
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), data, text.len());
        init_string_header(s, 100, text.len() as u32, text.len() as u32 + 4, 1, 0);
    }
    assert_eq!(js_string_char_code_at(s, 99), 0x4e2d as f64);
    unsafe {
        std::ptr::copy_nonoverlapping("😀".as_ptr(), data.add(text.len()), 4);
        (*s).byte_len += 4;
        (*s).utf16_len += 2;
    }
    assert_eq!(js_string_char_code_at(s, 100), 0xd83d as f64);
    assert_eq!(js_string_char_code_at(s, 101), 0xde00 as f64);
    assert_eq!(js_string_char_code_at(s, 99), 0x4e2d as f64);
    prune_dead_utf16_indexes(&|_| true);
}

#[test]
fn cache_eviction_is_bounded_and_short_strings_do_not_evict_sources() {
    prune_dead_utf16_indexes(&|_| true);
    for i in 0..CACHE_ENTRIES * 3 {
        let text = format!(
            "{}{}",
            "中".repeat(256),
            char::from_u32(0x400 + i as u32).unwrap()
        );
        let s = js_string_from_str(&text);
        assert_eq!(js_string_char_code_at(s, 256), (0x400 + i) as f64);
        let before = test_utf16_index_entries();
        let ch = js_string_char_at(s, 256);
        assert_eq!(js_string_char_code_at(ch, 0), (0x400 + i) as f64);
        assert_eq!(test_utf16_index_entries(), before);
        assert!(before.len() <= CACHE_ENTRIES);
    }
    prune_dead_utf16_indexes(&|_| true);
}
