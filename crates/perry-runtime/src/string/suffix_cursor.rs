//! Allocation-free operations on a compiler-proven, non-escaping suffix.
//!
//! The source remains a normal, rooted string. The stack cursor contains only
//! offsets, so moving GC cannot invalidate it. No substring or backing store
//! escapes this representation; ordinary string consumers still get flat data.

use super::slice_range::{advance, Boundary};
use super::*;

/// Mirrors the code generator's zero-initialized `[3 x i32]` stack allocation.
#[repr(C)]
#[derive(Default)]
pub struct SuffixCursor {
    byte: u32,
    consumed: u32,
    low: u32,
}

fn length(source: f64) -> u32 {
    let value = crate::value::JSValue::from_bits(source.to_bits());
    if value.is_short_string() {
        value.short_string_len() as u32
    } else {
        unsafe { (*value.as_string_ptr()).utf16_len }
    }
}

/// The compiler guards `source` as a string and supplies live stack storage.
#[no_mangle]
pub unsafe extern "C" fn js_string_suffix_length(source: f64, cursor: *const SuffixCursor) -> f64 {
    length(source).saturating_sub((*cursor).consumed) as f64
}

/// Advance by a nonnegative, already-coerced UTF-16 count, clamped to the tail.
#[no_mangle]
pub unsafe extern "C" fn js_string_suffix_advance(
    source: f64,
    cursor: *mut SuffixCursor,
    count: i32,
) {
    let count = (count.max(0) as u32).min(length(source).saturating_sub((*cursor).consumed));
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, len) = str_bytes_from_jsvalue(source, &mut scratch).unwrap();
    let bytes = if len == 0 {
        &[]
    } else {
        slice::from_raw_parts(data, len as usize)
    };
    let at = advance(
        bytes,
        Boundary {
            byte: (*cursor).byte as usize,
            low: (*cursor).low != 0,
        },
        count as usize,
    );
    (*cursor).byte = at.byte as u32;
    (*cursor).low = u32::from(at.low);
    (*cursor).consumed += count;
}

/// Read a UTF-16 code unit relative to the current suffix without materializing it.
#[no_mangle]
pub unsafe extern "C" fn js_string_suffix_char_code_at(
    source: f64,
    cursor: *const SuffixCursor,
    index: i32,
) -> f64 {
    if index < 0 || index as u32 >= length(source).saturating_sub((*cursor).consumed) {
        return f64::NAN;
    }
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, len) = str_bytes_from_jsvalue(source, &mut scratch).unwrap();
    let bytes = slice::from_raw_parts(data, len as usize);
    let mut at = advance(
        bytes,
        Boundary {
            byte: (*cursor).byte as usize,
            low: (*cursor).low != 0,
        },
        index as usize,
    );
    if at.byte >= bytes.len() {
        return f64::NAN;
    }
    // Invalid FFI/binary payloads can contain stray continuation bytes, which
    // the runtime's UTF-16 counter and charCodeAt skip rather than count.
    while at.byte < bytes.len() && wtf8_step(bytes, at.byte).1 == 0 {
        at.byte += 1;
    }
    if at.byte >= bytes.len() {
        return f64::NAN;
    }
    let (_, units, cp) = wtf8_step(bytes, at.byte);
    if units == 2 {
        let v = cp.wrapping_sub(0x10000);
        if at.low {
            (0xDC00 + (v & 0x3FF)) as f64
        } else {
            (0xD800 + ((v >> 10) & 0x3FF)) as f64
        }
    } else {
        cp as f64
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_LENGTH: unsafe extern "C" fn(f64, *const SuffixCursor) -> f64 = js_string_suffix_length;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ADVANCE: unsafe extern "C" fn(f64, *mut SuffixCursor, i32) = js_string_suffix_advance;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_CHAR_CODE: unsafe extern "C" fn(f64, *const SuffixCursor, i32) -> f64 =
    js_string_suffix_char_code_at;
