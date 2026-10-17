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
    if is_utf8(bytes) {
        // SAFETY: `is_utf8` just validated these bytes.
        let text = unsafe { std::str::from_utf8_unchecked(bytes) };
        return (utf16_count::count(text) as u32, STRING_FLAG_WTF8_VALIDATED);
    }
    match valid_wtf8_metadata(bytes) {
        Some((units, lone, canonical)) => (
            units,
            (if canonical {
                STRING_FLAG_WTF8_VALIDATED
            } else {
                0
            }) | if lone {
                STRING_FLAG_HAS_LONE_SURROGATES
            } else {
                0
            },
        ),
        None => (compute_utf16_len_wtf8(bytes), 0),
    }
}

/// Whether `bytes` is well-formed UTF-8: the runtime's one SIMD validation
/// entry. On x86_64 it selects the AVX2 or SSE4.2 validator with std's cached
/// feature bits and calls it DIRECTLY. `simdutf8::basic::from_utf8` instead
/// calls through a function pointer it stores on first use; once that is
/// inlined into a runtime helper (prod's codegen units do it), the GC
/// call-effects analyzer sees an indirect call that may reach JS, and every
/// string-making entry that validates (`js_nanbox_string`, the URL getters,
/// `js_string_builder_new`, ...) turns Reenters.
#[inline]
pub(super) fn is_utf8(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        // simdutf8's own cut-over: below one SIMD chunk the scalar check wins.
        if bytes.len() >= 64 {
            if std::is_x86_feature_detected!("avx2") {
                // SAFETY: AVX2 is present on this CPU.
                return unsafe { simdutf8::basic::imp::x86::avx2::validate_utf8(bytes) }.is_ok();
            }
            if std::is_x86_feature_detected!("sse4.2") {
                // SAFETY: SSE4.2 is present on this CPU.
                return unsafe { simdutf8::basic::imp::x86::sse42::validate_utf8(bytes) }.is_ok();
            }
        }
        std::str::from_utf8(bytes).is_ok()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        simdutf8::basic::from_utf8(bytes).is_ok()
    }
}

/// Generalized WTF-8 permits surrogate code points, but still rejects bad
/// continuations, overlong encodings, truncation, and points above U+10FFFF.
fn valid_wtf8_metadata(bytes: &[u8]) -> Option<(u32, bool, bool)> {
    let mut at = 0;
    let mut units = 0;
    let mut lone = false;
    let mut previous_lead = false;
    let mut canonical = true;
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
        let surrogate = width == 3 && lead == 0xed && point[1] >= 0xa0;
        let trail = surrogate && point[1] >= 0xb0;
        canonical &= !(previous_lead && trail);
        previous_lead = surrogate && !trail;
        units += if width == 4 { 2 } else { 1 };
        at += width;
    }
    Some((units, lone, canonical))
}

/// Surrogate metadata unions; encoding/JSON proofs intersect. One proven operand cannot
/// certify an unknown operand. Empty strings use the proven identity flag.
#[inline]
pub(super) fn combine_string_flags(a: u32, b: u32) -> u32 {
    let proofs = STRING_FLAG_WTF8_VALIDATED | STRING_FLAG_JSON_ESCAPE_FREE;
    ((a | b) & !proofs) | (a & b & proofs)
}
