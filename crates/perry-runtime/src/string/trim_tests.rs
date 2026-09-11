use super::*;

struct TrimTestGuard {
    _suppress: crate::gc::GcSuppressScope,
}

impl TrimTestGuard {
    fn new() -> Self {
        trim_cache::test_clear_trim_cache();
        Self {
            _suppress: crate::gc::GcSuppressScope::new(),
        }
    }
}

impl Drop for TrimTestGuard {
    fn drop(&mut self) {
        trim_cache::test_clear_trim_cache();
    }
}

fn make(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn payload(s: *const StringHeader) -> Vec<u8> {
    unsafe { OwnedStringBytes::copy_from_header(s).as_bytes().to_vec() }
}

#[test]
fn trim_ecmascript_whitespace_and_non_whitespace() {
    let _guard = TrimTestGuard::new();
    for cp in [
        0x9, 0xa, 0xb, 0xc, 0xd, 0x20, 0xa0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
        0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
        0xfeff,
    ] {
        let ws = char::from_u32(cp).unwrap();
        let source = make(format!("{ws}ä中😀Ö{ws}").as_bytes());
        for (trim, expected) in [
            (
                js_string_trim as extern "C" fn(_) -> _,
                "ä中😀Ö".to_string(),
            ),
            (js_string_trim_start, format!("ä中😀Ö{ws}")),
            (js_string_trim_end, format!("{ws}ä中😀Ö")),
        ] {
            let result = trim(source);
            assert_eq!(payload(result), expected.as_bytes(), "U+{cp:04X}");
            assert_eq!(
                unsafe { (*result).utf16_len },
                expected.encode_utf16().count() as u32
            );
        }
    }
    for source in ["", "plain", "\u{0085}keep\u{0085}", "\u{200b}keep\u{200b}"] {
        let s = make(source.as_bytes());
        assert_eq!(payload(js_string_trim(s)), source.as_bytes());
    }
    for source in [" \t\n", "\u{feff}\u{3000}\u{a0}"] {
        let empty = js_string_trim(make(source.as_bytes()));
        js_string_addref(empty);
        assert_eq!(payload(empty), b"");
    }
}

#[test]
fn trim_preserves_lone_surrogates_and_utf16_length() {
    let _guard = TrimTestGuard::new();
    let bytes = b" \t\xed\xa0\x80\xe4\xb8\xad\xf0\x9f\x98\x80\xed\xbf\xbf\n ";
    let source = js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32);
    let result = js_string_trim(source);
    assert_eq!(payload(result), &bytes[2..bytes.len() - 2]);
    assert_eq!(unsafe { (*result).utf16_len }, 5);
    assert_ne!(
        unsafe { (*result).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
        0
    );
    assert_eq!(js_string_char_code_at(result, 0), 0xd800 as f64);
    assert_eq!(js_string_char_code_at(result, 4), 0xdfff as f64);
}

// Reference the historical forward walk, including its treatment of malformed
// WTF-8. The reverse walk must agree even when a lead consumes bytes that look
// like whitespace or another lead rather than like continuation bytes.
fn forward_range(bytes: &[u8], left: bool, right: bool) -> (usize, usize) {
    let mut start = 0;
    let whitespace = |i| {
        let (advance, units, cp) = wtf8_step(bytes, i);
        (
            i + advance <= bytes.len()
                && units > 0
                && char::from_u32(cp)
                    .map(slice_ops::is_js_whitespace)
                    .unwrap_or(false),
            advance,
        )
    };
    if left {
        while start < bytes.len() {
            let (ws, advance) = whitespace(start);
            if !ws {
                break;
            }
            start += advance;
        }
    }
    let mut end = bytes.len();
    if right {
        let mut i = start;
        end = start;
        while i < bytes.len() {
            let (ws, advance) = whitespace(i);
            i = (i + advance).min(bytes.len());
            if !ws {
                end = i;
            }
        }
    }
    (start, end)
}

#[test]
fn reverse_trim_matches_forward_walk_on_malformed_payloads() {
    let alphabet = [9, 32, 65, 128, 160, 192, 194, 195, 224, 226, 239, 240, 255];
    let mut seed = 10054u64;
    for len in 0..32 {
        for _ in 0..1024 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    alphabet[seed as usize % alphabet.len()]
                })
                .collect();
            for (left, right) in [(false, true), (true, false), (true, true)] {
                assert_eq!(
                    slice_ops::js_whitespace_trim_range(&bytes, left, right),
                    forward_range(&bytes, left, right),
                    "bytes={bytes:x?}, left={left}, right={right}",
                );
            }
        }
    }
}

#[test]
fn repeated_trim_reuses_result_without_aliasing_append() {
    let _guard = TrimTestGuard::new();
    let bytes = format!(" \t{}\n ", "aBcD".repeat(100));
    let source = js_string_from_bytes_with_capacity(bytes.as_ptr(), bytes.len() as u32, 1024);
    unsafe {
        (*source).refcount = 1;
    }
    let result = js_string_trim(source);
    assert_eq!(
        js_string_trim(source),
        result,
        "repeated trim must avoid another copy"
    );
    assert_eq!(
        unsafe { (*source).refcount },
        0,
        "a cached source must be immutable"
    );
    assert_eq!(unsafe { (*result).refcount }, 0);
    let suffix = make(b"tail");
    let appended = js_string_append(source, suffix);
    assert_eq!(payload(source), bytes.as_bytes());
    assert_eq!(payload(appended), format!("{bytes}tail").as_bytes());
    assert_eq!(js_string_trim(source), result);

    let identity = js_string_from_bytes_with_capacity(b"unchanged".as_ptr(), 9, 64);
    unsafe {
        (*identity).refcount = 1;
    }
    assert_eq!(js_string_trim(identity), identity);
    assert_eq!(unsafe { (*identity).refcount }, 0);
    assert_eq!(
        payload(js_string_append(identity, suffix)),
        b"unchangedtail"
    );
    assert_eq!(payload(identity), b"unchanged");
}

#[test]
fn trim_cache_distinguishes_modes_and_replaces_old_source() {
    let _guard = TrimTestGuard::new();
    let inner = "x".repeat(400);
    let source = make(format!(" {inner} ").as_bytes());
    for (trim, expected) in [
        (js_string_trim as extern "C" fn(_) -> _, inner.clone()),
        (js_string_trim_start, format!("{inner} ")),
        (js_string_trim_end, format!(" {inner}")),
    ] {
        let result = trim(source);
        assert_eq!(payload(result), expected.as_bytes());
        assert_eq!(trim(source), result);
    }
    let next_source = make(format!(" {} ", "y".repeat(400)).as_bytes());
    let next_result = js_string_trim(next_source);
    assert_eq!(
        trim_cache::test_trim_cache_pair(),
        (next_source, next_result)
    );
}

#[test]
fn trim_cache_budget_counts_spare_capacity() {
    let _guard = TrimTestGuard::new();
    let bytes = format!(" {} ", "x".repeat(400));
    let source = js_string_from_bytes_with_capacity(
        bytes.as_ptr(),
        bytes.len() as u32,
        trim_cache::MAX_RETAINED_BYTES as u32,
    );
    let result = js_string_trim(source);
    assert_eq!(payload(result), &bytes.as_bytes()[1..bytes.len() - 1]);
    assert_eq!(
        trim_cache::test_trim_cache_pair(),
        (ptr::null_mut(), ptr::null_mut())
    );
}

#[test]
fn trim_cache_does_not_retain_foreign_string_storage() {
    let _guard = TrimTestGuard::new();
    let bytes = format!(" {} ", "x".repeat(400));
    let size = std::mem::size_of::<StringHeader>() + bytes.len();
    let mut storage = vec![0u64; size.div_ceil(8)];
    let source = storage.as_mut_ptr().cast::<StringHeader>();
    unsafe {
        init_string_header(
            source,
            bytes.len() as u32,
            bytes.len() as u32,
            bytes.len() as u32,
            0,
            0,
        );
        ptr::copy_nonoverlapping(bytes.as_ptr(), string_data(source).cast_mut(), bytes.len());
    }
    assert_eq!(
        payload(js_string_trim(source)),
        &bytes.as_bytes()[1..bytes.len() - 1]
    );
    assert_eq!(
        trim_cache::test_trim_cache_pair(),
        (ptr::null_mut(), ptr::null_mut())
    );
    let mut expected = bytes.into_bytes();
    expected[0] = b'Y';
    let last = expected.len() - 1;
    expected[last] = b'Z';
    unsafe {
        ptr::copy_nonoverlapping(
            expected.as_ptr(),
            string_data(source).cast_mut(),
            expected.len(),
        );
    }
    let unchanged = js_string_trim(source);
    assert_ne!(
        unchanged, source,
        "an unchanged foreign payload must still be copied"
    );
    drop(storage);
    assert_eq!(payload(unchanged), expected);
}
