//! Small JSON results can use the existing boxed inline-string representation.
//! No scratch allocation or managed-heap work is needed for these primitives.

use crate::value::{JSValue, SHORT_STRING_MAX_LEN, TAG_FALSE, TAG_NULL, TAG_TRUE};

#[inline]
pub(super) fn inline_ascii_output(bytes: &[u8]) -> Option<JSValue> {
    if bytes.len() <= SHORT_STRING_MAX_LEN && bytes.is_ascii() {
        Some(JSValue::short_string_unchecked(bytes))
    } else {
        None
    }
}

/// Only call when no replacer or observable spacer coercion is required.
#[inline]
pub(super) fn try_primitive(bits: u64) -> Option<JSValue> {
    match bits {
        TAG_NULL => return Some(JSValue::short_string_unchecked(b"null")),
        TAG_TRUE => return Some(JSValue::short_string_unchecked(b"true")),
        TAG_FALSE => return Some(JSValue::short_string_unchecked(b"false")),
        _ => {}
    }
    let mut source = [0; SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut source)?;
    // Two quote bytes must also fit. Heap strings are only borrowed; there
    // are no allocations or callbacks while their bytes are being read.
    if len as usize > SHORT_STRING_MAX_LEN - 2 || ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut output = [0; SHORT_STRING_MAX_LEN];
    output[0] = b'"';
    let mut n = 1;
    for &b in bytes {
        let escaped = match b {
            b'"' | b'\\' => Some(b),
            b'\x08' => Some(b'b'),
            b'\t' => Some(b't'),
            b'\n' => Some(b'n'),
            b'\x0c' => Some(b'f'),
            b'\r' => Some(b'r'),
            // Non-ASCII, WTF-8 and six-byte control escapes use the general
            // emitter, keeping its validation/escaping behavior unchanged.
            0..=0x1f | 0x80..=0xff => return None,
            _ => None,
        };
        if let Some(ch) = escaped {
            if n + 2 >= output.len() {
                return None;
            }
            output[n] = b'\\';
            output[n + 1] = ch;
            n += 2;
        } else {
            if n + 1 >= output.len() {
                return None;
            }
            output[n] = b;
            n += 1;
        }
    }
    output[n] = b'"';
    Some(JSValue::short_string_unchecked(&output[..n + 1]))
}

/// Call only when the replacer is absent. The original spacer must reach the
/// general path unchanged if these bounded writers cannot handle the value.
#[inline(never)]
pub(super) unsafe fn try_inert_spacer(value_bits: u64, spacer_bits: u64) -> Option<JSValue> {
    if spacer_bits != TAG_TRUE && (spacer_bits & 0x7fff_ffff_ffff_ffff) != 0 {
        return None;
    }
    if let Some(result) = try_primitive(value_bits) {
        return Some(result);
    }
    if let Some(result) = super::stringify_string::try_heap_string(value_bits) {
        return Some(JSValue::string_ptr(result));
    }
    if let Some(result) = super::stringify_record_output::try_object(value_bits) {
        return Some(result);
    }
    super::stringify_flat::try_object(value_bits)
}

#[cfg(test)]
#[path = "stringify_small_tests.rs"]
mod tests;
