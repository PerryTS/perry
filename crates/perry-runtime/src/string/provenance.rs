//! Encoding proof at the raw-byte boundary, carried in the existing header.
use super::*;

/// Metadata for arbitrary incoming bytes. Invalid input keeps the legacy
/// bounded decoder's count, but cannot grant any unchecked Unicode borrow.
pub(super) fn raw_string_metadata(data: *const u8, len: u32) -> (u32, u32) {
    if len == 0 {
        return (0, STRING_FLAG_WTF8_VALIDATED);
    }
    if data.is_null() {
        return (0, 0);
    }
    let bytes = unsafe { slice::from_raw_parts(data, len as usize) };
    if bytes.is_ascii() {
        return (len, STRING_FLAG_WTF8_VALIDATED);
    }
    if let Ok(text) = simdutf8::basic::from_utf8(bytes) {
        return (utf16_count::count(text) as u32, STRING_FLAG_WTF8_VALIDATED);
    }
    match valid_wtf8_metadata(bytes) {
        Some((units, lone)) => (
            units,
            STRING_FLAG_WTF8_VALIDATED
                | if lone {
                    STRING_FLAG_HAS_LONE_SURROGATES
                } else {
                    0
                },
        ),
        None => (compute_utf16_len_wtf8(bytes), 0),
    }
}

/// Generalized WTF-8 permits surrogate code points, but still rejects bad
/// continuations, overlong encodings, truncation, and points above U+10FFFF.
fn valid_wtf8_metadata(bytes: &[u8]) -> Option<(u32, bool)> {
    let mut at = 0;
    let mut units = 0;
    let mut lone = false;
    while at < bytes.len() {
        let lead = bytes[at];
        let width = match lead {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return None,
        };
        let point = bytes.get(at..at + width)?;
        if !point[1..].iter().all(|&byte| (0x80..=0xbf).contains(&byte)) {
            return None;
        }
        if width >= 3 {
            let second = point[1];
            if (lead == 0xe0 && second < 0xa0)
                || (lead == 0xf0 && second < 0x90)
                || (lead == 0xf4 && second > 0x8f)
            {
                return None;
            }
            lone |= lead == 0xed && second >= 0xa0;
        }
        units += if width == 4 { 2 } else { 1 };
        at += width;
    }
    Some((units, lone))
}

/// Surrogate metadata unions; encoding/JSON proofs intersect. One proven operand cannot
/// certify an unknown operand. Empty strings use the proven identity flag.
#[inline]
pub(super) fn combine_string_flags(a: u32, b: u32) -> u32 {
    let proofs = STRING_FLAG_WTF8_VALIDATED | STRING_FLAG_JSON_ESCAPE_FREE;
    ((a | b) & !proofs) | (a & b & proofs)
}
