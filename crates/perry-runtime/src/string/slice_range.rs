//! UTF-16 boundaries over the runtime's UTF-8/WTF-8 payloads.

use super::*;

/// A boundary can lie between the two code units of one four-byte scalar.
/// In that case `byte` still names the scalar's lead byte.
#[derive(Clone, Copy, Default)]
pub(super) struct Boundary {
    pub byte: usize,
    pub low: bool,
}

pub(super) fn advance(bytes: &[u8], mut at: Boundary, mut units: usize) -> Boundary {
    while units > 0 && at.byte < bytes.len() {
        let (width, count, _) = wtf8_step(bytes, at.byte);
        let count = count.saturating_sub(usize::from(at.low));
        if units < count {
            at.low = true;
            break;
        }
        units -= count;
        at.byte = (at.byte + width).min(bytes.len());
        at.low = false;
    }
    at
}

/// Copy a normalized nonempty UTF-16 range, preserving split surrogate halves.
/// Complete scalar ranges use the rooted copy helper. Split boundaries are
/// staged in Rust-owned bytes before the destination allocation can collect.
pub(super) fn copy_utf16_range(s: *const StringHeader, start: u32, end: u32) -> *mut StringHeader {
    if is_ascii_string(s) {
        return string_copy_range(s, start as usize, end - start, end - start, 0);
    }
    let bytes = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };
    let first = advance(bytes, Boundary::default(), start as usize);
    // A suffix's end is already known: do not scan the entire remaining string.
    let last = if end == unsafe { (*s).utf16_len } {
        Boundary {
            byte: bytes.len(),
            low: false,
        }
    } else {
        advance(bytes, first, (end - start) as usize)
    };
    if !first.low && !last.low {
        let part = &bytes[first.byte..last.byte];
        let flags = if unsafe { (*s).flags } & STRING_FLAG_HAS_LONE_SURROGATES != 0
            && bytes_have_lone_surrogate(part)
        {
            STRING_FLAG_HAS_LONE_SURROGATES
        } else {
            0
        };
        return string_copy_range(s, first.byte, part.len() as u32, end - start, flags);
    }

    let mut out = Vec::with_capacity(last.byte - first.byte + 6);
    let mut byte_start = first.byte;
    if first.low {
        let (width, _, cp) = wtf8_step(bytes, first.byte);
        let low = 0xDC00 + (cp.wrapping_sub(0x10000) & 0x3FF) as u16;
        char_ops::push_code_unit_wtf8(&mut out, low);
        byte_start = (byte_start + width).min(bytes.len());
    }
    out.extend_from_slice(&bytes[byte_start..last.byte]);
    if last.low {
        let (_, _, cp) = wtf8_step(bytes, last.byte);
        let high = 0xD800 + ((cp.wrapping_sub(0x10000) >> 10) & 0x3FF) as u16;
        char_ops::push_code_unit_wtf8(&mut out, high);
    }
    js_string_from_bytes_known_utf16(
        out.as_ptr(),
        out.len() as u32,
        end - start,
        STRING_FLAG_HAS_LONE_SURROGATES,
    )
}
