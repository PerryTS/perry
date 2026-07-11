//! Guard-page regression tests for #6085.
//!
//! Perry heap-string payloads are EXACT-SIZED: `string_storage_alloc` reserves
//! `size_of::<StringHeader>() + byte_len` bytes — no NUL terminator, no tail
//! padding — and the bytes are NOT guaranteed to be valid UTF-8 (`Buffer`
//! `toString`, FFI blobs, and WTF-8 lone-surrogate strings all reach
//! `js_string_from_bytes` with raw bytes). Any scanner that decodes such a
//! payload through std's UTF-8-validity-assuming iterators (`str::chars()`,
//! `str::encode_utf16()` over a `from_utf8_unchecked` view) reads the
//! continuation bytes of a multi-byte lead WITHOUT a bounds check
//! (`core::str::validations::next_code_point` uses `unwrap_unchecked`). A
//! payload whose last sequence is a truncated multi-byte lead therefore reads
//! 1–3 bytes PAST the end of its own allocation.
//!
//! In production that read usually lands in mapped heap and is invisible; when
//! the allocation happens to sit flush against an unmapped page it faults
//! (`0x...FFF8`-style access violations on Windows x64 — the #6085 report).
//!
//! These tests make the fault DETERMINISTIC on any Unix host: map two pages,
//! `mprotect(PROT_NONE)` the second, and place the string so its last payload
//! byte is the last byte of the first page. Any read past `byte_len` hits the
//! guard page and raises SIGSEGV instead of silently succeeding.

use super::*;
use std::ptr;

/// A `StringHeader` + payload placed flush against a `PROT_NONE` guard page:
/// `data[byte_len - 1]` is the last readable byte before the guard.
struct GuardedString {
    base: *mut u8,
    total: usize,
    hdr: *mut StringHeader,
}

impl GuardedString {
    /// Map the string so the payload ends exactly at the page boundary.
    /// `payload.len()` must keep the header 8-byte aligned (len ≡ 4 mod 8,
    /// since `size_of::<StringHeader>() == 20`).
    fn new(payload: &[u8]) -> Self {
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let total = page * 2;
            let base = libc::mmap(
                ptr::null_mut(),
                total,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            ) as *mut u8;
            assert!(base as isize != -1, "mmap failed");
            // Second page unreadable: any over-read past the payload faults.
            assert_eq!(
                libc::mprotect(base.add(page) as *mut libc::c_void, page, libc::PROT_NONE),
                0,
                "mprotect(PROT_NONE) failed"
            );

            let hdr_size = std::mem::size_of::<StringHeader>();
            let need = hdr_size + payload.len();
            assert!(need <= page);
            let hdr = base.add(page - need) as *mut StringHeader;
            assert_eq!(
                hdr as usize % 8,
                0,
                "test payload length must keep the header 8-byte aligned"
            );

            let data = (hdr as *mut u8).add(hdr_size);
            ptr::copy_nonoverlapping(payload.as_ptr(), data, payload.len());
            // Sanity: the payload really is flush against the guard page.
            assert_eq!(data.add(payload.len()) as usize, base.add(page) as usize);

            let byte_len = payload.len() as u32;
            let u16len = compute_utf16_len(payload.as_ptr(), byte_len);
            init_string_header(hdr, u16len, byte_len, byte_len, 0, 0);
            GuardedString { base, total, hdr }
        }
    }

    fn ptr(&self) -> *const StringHeader {
        self.hdr as *const StringHeader
    }
}

impl Drop for GuardedString {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base as *mut libc::c_void, self.total);
        }
    }
}

/// Payload ending in a TRUNCATED 2-byte lead (`0xC3` with no continuation
/// byte): "é" + "A" + a dangling lead. This is what a byte-delimited blob from
/// an FFI / `Buffer` produces when a `split()` boundary lands mid-sequence, and
/// it is the shape that makes `chars()` read past the end.
///
/// Length 4 keeps the 20-byte header 8-byte aligned.
/// `utf16_len` (3) != `byte_len` (4), so scanners take their non-ASCII path.
const TRUNCATED_TAIL: [u8; 4] = [0xC3, 0xA9, 0x41, 0xC3];

/// Well-formed multi-byte payload (also length 4): "é" + "é".
const WELL_FORMED: [u8; 4] = [0xC3, 0xA9, 0xC3, 0xA9];

#[test]
fn truncated_tail_is_non_ascii_and_flush_against_guard_page() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    unsafe {
        assert_eq!((*g.ptr()).byte_len, 4);
        // Must NOT take the ASCII fast path, or the scanners never decode.
        assert_ne!(
            (*g.ptr()).utf16_len,
            (*g.ptr()).byte_len,
            "payload must exercise the non-ASCII decode path"
        );
    }
}

#[test]
fn char_code_at_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    assert_eq!(js_string_char_code_at(s, 0), 233.0); // é
    assert_eq!(js_string_char_code_at(s, 1), 65.0); // A
                                                    // Truncated lead: decoded from the bytes that EXIST (missing continuation
                                                    // bytes read as 0) — the point is that it does not touch the guard page.
    assert_eq!(js_string_char_code_at(s, 2), 192.0);
    assert!(js_string_char_code_at(s, 3).is_nan());
}

#[test]
fn code_point_at_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    assert_eq!(js_string_code_point_at(s, 0), 233.0);
    assert_eq!(js_string_code_point_at(s, 1), 65.0);
    assert_eq!(js_string_code_point_at(s, 2), 192.0);
}

#[test]
fn char_at_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    let c0 = js_string_char_at(s, 0);
    assert_eq!(string_as_str(c0), "é");
    let c1 = js_string_char_at(s, 1);
    assert_eq!(string_as_str(c1), "A");
    // Index 2 decodes the dangling lead — must not fault.
    let _ = js_string_char_at(s, 2);
}

#[test]
fn string_at_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    // `String.prototype.at` collected the whole string via `encode_utf16()`.
    let _ = js_string_at(s, 0);
    let _ = js_string_at(s, 2);
    let _ = js_string_at(s, -1);
}

#[test]
fn to_char_array_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let boxed = crate::value::js_nanbox_string(g.ptr() as i64);
    let _ = js_string_to_char_array(boxed.to_bits() as i64);
}

#[test]
fn slice_and_substring_do_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    // Non-ASCII slice routes through `utf16_offset_to_byte_offset`, which
    // walked `chars()` before #6085.
    let sliced = js_string_slice(s, 0, 2);
    assert_eq!(string_as_str(sliced), "éA");
    let _ = js_string_slice(s, 1, 3);
    let _ = js_string_substring(s, 0, 3);
}

#[test]
fn index_of_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    let needle = js_string_from_bytes(b"A".as_ptr(), 1);
    assert_eq!(js_string_index_of(s, needle), 1);
    let missing = js_string_from_bytes(b"zz".as_ptr(), 2);
    assert_eq!(js_string_index_of(s, missing), -1);
}

#[test]
fn split_by_empty_delimiter_does_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    // `"…".split("")` walked `str_data.chars()` over the raw payload.
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    let arr = js_string_split_n(g.ptr(), empty, -1);
    assert!(!arr.is_null());
    // "é", "A", dangling lead → 3 parts, each holding its raw bytes.
    assert_eq!(unsafe { (*arr).length }, 3);
}

/// Read back a split part's `(bytes, utf16_len, flags)`.
fn split_part_meta(arr: *mut crate::array::ArrayHeader, i: u32) -> (Vec<u8>, u32, u32) {
    unsafe {
        let v = crate::array::js_array_get_f64(arr, i);
        let p = crate::value::js_nanbox_get_pointer(v) as *const StringHeader;
        let bytes = std::slice::from_raw_parts(string_data(p), (*p).byte_len as usize).to_vec();
        (bytes, (*p).utf16_len, (*p).flags)
    }
}

/// Each split part's metadata must come from THAT PART's own bytes.
///
/// `is_ascii_string(s)` only compares `byte_len == utf16_len` over the whole
/// source, and malformed bytes can satisfy that aggregate while the individual
/// parts cannot. For `[0x80, '|', 0xF0]` the source is 3 == 3, so the old ASCII
/// fast path stamped `utf16_len = byte_len = 1` onto BOTH parts — but a stray
/// continuation byte is 0 code units and a 4-byte lead is 2. Wrong `.length`
/// then propagates into every downstream index/length operation.
#[test]
fn split_parts_get_metadata_from_their_own_bytes() {
    let src = [0x80u8, b'|', 0xF0];
    let s = js_string_from_bytes(src.as_ptr(), 3);
    // The aggregate check really does lie here — that is the whole bug.
    assert!(
        is_ascii_string(s),
        "precondition: the aggregate byte_len == utf16_len check misfires"
    );

    let delim = js_string_from_bytes(b"|".as_ptr(), 1);
    let arr = js_string_split_n(s, delim, -1);
    assert_eq!(unsafe { (*arr).length }, 2);

    // [0x80] — a stray continuation byte contributes ZERO UTF-16 units.
    let (b0, u0, f0) = split_part_meta(arr, 0);
    assert_eq!(b0, vec![0x80]);
    assert_eq!(u0, 0, "stray continuation byte is 0 UTF-16 units, not 1");
    assert_eq!(f0, 0);

    // [0xF0] — a 4-byte lead counts as an astral code point: TWO units.
    let (b1, u1, f1) = split_part_meta(arr, 1);
    assert_eq!(b1, vec![0xF0]);
    assert_eq!(u1, 2, "4-byte lead is 2 UTF-16 units, not 1");
    assert_eq!(f1, 0);
}

/// A lone surrogate carved out of a WTF-8 source must keep its
/// `HAS_LONE_SURROGATES` flag, or `isWellFormed()` on the part wrongly says
/// true. (`js_string_from_bytes` hardcodes `flags = 0`, so the flag has to be
/// derived from the part's own bytes.)
#[test]
fn split_parts_preserve_lone_surrogate_flag() {
    // "\uD800" (ED A0 80) + '|' + 'B'
    let src = [0xEDu8, 0xA0, 0x80, b'|', b'B'];
    let s = js_string_from_wtf8_bytes(src.as_ptr(), 5);
    let delim = js_string_from_bytes(b"|".as_ptr(), 1);

    let arr = js_string_split_n(s, delim, -1);
    assert_eq!(unsafe { (*arr).length }, 2);

    let (b0, u0, f0) = split_part_meta(arr, 0);
    assert_eq!(b0, vec![0xED, 0xA0, 0x80]);
    assert_eq!(u0, 1);
    assert_eq!(
        f0, STRING_FLAG_HAS_LONE_SURROGATES,
        "the part holding the lone surrogate must stay flagged"
    );
    let part0 = unsafe {
        let v = crate::array::js_array_get_f64(arr, 0);
        crate::value::js_nanbox_get_pointer(v) as *const StringHeader
    };
    assert_eq!(
        js_string_is_well_formed(part0).to_bits(),
        crate::value::TAG_FALSE,
        "isWellFormed() must be false for a lone-surrogate part"
    );

    // The clean part must NOT inherit the flag.
    let (b1, _, f1) = split_part_meta(arr, 1);
    assert_eq!(b1, vec![b'B']);
    assert_eq!(f1, 0, "a part with no surrogate must not carry the flag");

    // Same for the empty-delimiter path.
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    let chars = js_string_split_n(s, empty, -1);
    let (cb0, _, cf0) = split_part_meta(chars, 0);
    assert_eq!(cb0, vec![0xED, 0xA0, 0x80]);
    assert_eq!(cf0, STRING_FLAG_HAS_LONE_SURROGATES);
}

/// A truncated multi-byte tail must NOT be mistaken for whitespace.
///
/// `wtf8_step` zero-fills continuation bytes that don't exist, so a dangling
/// `E2 80` decodes as U+2000 (EN QUAD) — which IS JS whitespace. Without an
/// explicit "sequence is complete" gate, `trim`/`trimEnd` would silently DELETE
/// those bytes. Trimming must only consume complete whitespace sequences.
#[test]
fn trim_preserves_truncated_multibyte_tail() {
    // "A" + truncated E2 80 (would decode as U+2000 if zero-filled), length 4
    // keeps the header 8-byte aligned.
    let g = GuardedString::new(&[0x41, 0x41, 0xE2, 0x80]);
    let s = g.ptr();
    for out in [js_string_trim(s), js_string_trim_end(s)] {
        let bytes =
            unsafe { std::slice::from_raw_parts(string_data(out), (*out).byte_len as usize) };
        assert_eq!(
            bytes,
            &[0x41, 0x41, 0xE2, 0x80],
            "truncated tail must survive trimming byte-for-byte"
        );
    }
    // A COMPLETE U+2000 really is whitespace and must still be trimmed.
    let ws = GuardedString::new(&[0x41, 0xE2, 0x80, 0x80]); // "A" + U+2000
    let trimmed = js_string_trim_end(ws.ptr());
    let bytes =
        unsafe { std::slice::from_raw_parts(string_data(trimmed), (*trimmed).byte_len as usize) };
    assert_eq!(bytes, &[0x41], "complete U+2000 must still be trimmed");
}

#[test]
fn trim_and_case_conversion_do_not_read_past_payload() {
    let g = GuardedString::new(&TRUNCATED_TAIL);
    let s = g.ptr();
    let _ = js_string_trim(s);
    let _ = js_string_trim_start(s);
    let _ = js_string_trim_end(s);
    let _ = js_string_to_lower_case(s);
    let _ = js_string_to_upper_case(s);
}

/// Well-formed multi-byte input must decode EXACTLY as before the #6085 fix
/// (the bounded decoder is byte-for-byte equivalent on valid input).
#[test]
fn well_formed_multibyte_decodes_identically() {
    let g = GuardedString::new(&WELL_FORMED);
    let s = g.ptr();
    unsafe {
        assert_eq!((*s).byte_len, 4);
        assert_eq!((*s).utf16_len, 2);
    }
    assert_eq!(js_string_char_code_at(s, 0), 233.0);
    assert_eq!(js_string_char_code_at(s, 1), 233.0);
    assert!(js_string_char_code_at(s, 2).is_nan());
    assert_eq!(string_as_str(js_string_char_at(s, 1)), "é");
    assert_eq!(string_as_str(js_string_slice(s, 0, 1)), "é");
    assert_eq!(string_as_str(js_string_to_upper_case(s)), "ÉÉ");
}

/// An astral (4-byte) code point split into surrogate halves — the
/// `utf16_len != byte_len` path with `units == 2`, plus a truncated astral
/// lead flush against the guard page.
#[test]
fn astral_and_truncated_astral_lead() {
    // "😀" = F0 9F 98 80 (2 UTF-16 units, 4 bytes).
    let g = GuardedString::new(&[0xF0, 0x9F, 0x98, 0x80]);
    let s = g.ptr();
    unsafe {
        assert_eq!((*s).utf16_len, 2);
        assert_eq!((*s).byte_len, 4);
    }
    assert_eq!(js_string_char_code_at(s, 0), 55357.0); // high surrogate
    assert_eq!(js_string_char_code_at(s, 1), 56832.0); // low surrogate
    assert_eq!(js_string_code_point_at(s, 0), 128512.0);

    // Truncated astral lead: "é" + a dangling 0xF0 (len 4 keeps alignment).
    let t = GuardedString::new(&[0xC3, 0xA9, 0x41, 0xF0]);
    let ts = t.ptr();
    let _ = js_string_char_code_at(ts, 2);
    let _ = js_string_to_char_array(crate::value::js_nanbox_string(ts as i64).to_bits() as i64);
}
