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

/// #10688: the index used to live in a fixed four-slot array that evicted
/// round-robin, so interleaving indexed access across more strings than that
/// rebuilt from scratch on every access — 1,224x, as a step function at the
/// fifth string. There is no capacity now, so this asserts the replacement
/// guarantee: **an index survives no matter how many other strings are
/// indexed alongside it.**
///
/// It also keeps the original invariant this test carried, which is unrelated
/// to capacity and still load-bearing: consuming a one-character string
/// produced by `char_at` must not disturb the source string's index.
#[test]
fn indexes_survive_any_number_of_interleaved_strings() {
    prune_dead_utf16_indexes(&|_| true);
    const STRINGS: usize = 16; // comfortably past the old four-slot capacity
    let mut sources = Vec::new();
    for i in 0..STRINGS {
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
        assert_eq!(
            test_utf16_index_entries(),
            before,
            "a short string from char_at must not disturb the source's index"
        );
        sources.push((s, 0x400 + i));
    }
    // Every index is still resident: no eviction happened at any depth.
    assert_eq!(
        test_utf16_index_entries().len(),
        STRINGS,
        "all {STRINGS} indexes must survive; the old array held only four"
    );
    // And every one still answers correctly, cheaply, in a second pass.
    for (s, expected) in &sources {
        assert_eq!(js_string_char_code_at(*s, 256), *expected as f64);
    }
    prune_dead_utf16_indexes(&|_| true);
    assert!(
        test_utf16_index_entries().is_empty(),
        "the collector's prune hook must reclaim them"
    );
}

/// #10656: `codePointAt` used to walk the WTF-8 payload from byte 0 on every
/// call, so a scan over a string holding one non-ASCII character was O(n^2).
/// These pin the spec behaviour across the cached-index path that replaced it:
/// a BMP code point, the start of a surrogate pair (the whole code point), the
/// low half (the bare trailing surrogate), and an unpaired leading surrogate.
#[test]
fn code_point_at_matches_the_spec_through_the_cached_index() {
    // Long enough to exercise the checkpoint/cursor path, not the short-string
    // fallback, and non-ASCII so it cannot take the ASCII fast path.
    let mut text = String::new();
    for _ in 0..200 {
        text.push_str("\u{e9}abcdefghij0123456789");
    }
    let astral_at = text.chars().count();
    text.push('\u{1F600}'); // surrogate pair
    text.push('z');

    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let units: Vec<u16> = text.encode_utf16().collect();

    // Walk forwards (the sequential case tsc hits) and compare every index
    // against an independent UTF-16 expansion of the same text.
    for (idx, &unit) in units.iter().enumerate() {
        let got = crate::string::js_string_code_point_at(s, idx as i32);
        let expected = if (0xD800..0xDC00).contains(&unit) && idx + 1 < units.len() {
            let second = units[idx + 1];
            if (0xDC00..0xE000).contains(&second) {
                0x10000 + (((unit as u32 - 0xD800) << 10) | (second as u32 - 0xDC00))
            } else {
                unit as u32
            }
        } else {
            unit as u32
        };
        assert_eq!(got, expected as f64, "codePointAt({idx})");
    }

    // The surrogate pair specifically: start yields the astral code point, the
    // low half yields the bare trailing surrogate.
    let pair_start = units.len() - 3;
    assert_eq!(
        crate::string::js_string_code_point_at(s, pair_start as i32),
        128512.0_f64
    );
    assert!((0xDC00..0xE000).contains(
        &(crate::string::js_string_code_point_at(s, pair_start as i32 + 1) as u32 as u16)
    ));
    let _ = astral_at;

    // Out of bounds stays undefined.
    let oob = crate::string::js_string_code_point_at(s, units.len() as i32);
    assert_eq!(oob.to_bits(), crate::value::TAG_UNDEFINED);
}

/// Random access must agree with sequential access: the cursor optimises the
/// forward case, and a backward seek must not return a stale answer.
#[test]
fn code_point_at_is_order_independent() {
    let mut text = String::new();
    for _ in 0..150 {
        text.push_str("x\u{e9}yz");
    }
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let n = text.encode_utf16().count();

    let forward: Vec<f64> = (0..n)
        .map(|i| crate::string::js_string_code_point_at(s, i as i32))
        .collect();
    let backward: Vec<f64> = (0..n)
        .rev()
        .map(|i| crate::string::js_string_code_point_at(s, i as i32))
        .collect();
    for (i, value) in backward.iter().rev().enumerate() {
        assert_eq!(*value, forward[i], "index {i} differs by traversal order");
    }
}

/// #10685: `copy_utf16_range` resolved its start boundary by walking from byte
/// 0 on every call, so slicing a non-ASCII string at increasing offsets was
/// O(n^2). `boundary_at` must agree with that walk at every index — including
/// the low half of a surrogate pair, where `low` selects the split copy path.
#[test]
fn boundary_at_matches_a_walk_from_zero() {
    let mut text = String::new();
    for _ in 0..80 {
        text.push_str("\u{e9}abcdefghij0123456789");
    }
    text.push('\u{1F600}');
    text.push_str("tail\u{e9}");
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let bytes = unsafe {
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize)
    };
    let n = text.encode_utf16().count();

    for idx in 0..n {
        let walked = crate::string::slice_range::advance(
            bytes,
            crate::string::slice_range::Boundary::default(),
            idx,
        );
        if let Some((byte, low)) = super::boundary_at(s, idx) {
            assert_eq!(byte, walked.byte, "byte offset at {idx}");
            assert_eq!(low, walked.low, "low-surrogate flag at {idx}");
        }
    }
}

/// The cursor optimises forward seeks; a backward seek must not reuse it.
#[test]
fn boundary_at_is_order_independent() {
    let mut text = String::new();
    for _ in 0..80 {
        text.push_str("x\u{e9}yz");
    }
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let n = text.encode_utf16().count();
    let forward: Vec<_> = (0..n).map(|i| super::boundary_at(s, i)).collect();
    let backward: Vec<_> = (0..n).rev().map(|i| super::boundary_at(s, i)).collect();
    for (i, value) in backward.iter().rev().enumerate() {
        assert_eq!(*value, forward[i], "index {i} differs by traversal order");
    }
}
