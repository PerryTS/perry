//! `s.charCodeAt(i)` on a non-ASCII string through a caller-owned cursor
//! (#14973). Codegen keeps one cursor in a stack slot per string parameter
//! that the function never rebinds, so every read in that invocation names
//! the same string: a sequential scan decodes each code point once instead of
//! walking from byte 0 on every read. The cursor holds byte and UTF-16
//! offsets only, never a pointer, so a collection that moves the string
//! leaves it valid.

use super::*;

/// Codegen's `[3 x i32]` stack slot, zeroed at function entry.
#[repr(C)]
pub struct Utf16Cursor {
    /// The payload length the offsets were taken from. A different length
    /// (including the zeroed slot's 0) restarts the scan.
    byte_len: u32,
    /// Start of the code point containing UTF-16 index `utf16`.
    byte: u32,
    utf16: u32,
}

/// The UTF-16 code unit at `idx`, or `-1` when the payload has no unit there
/// (the caller then takes the complete `charCodeAt`).
///
/// Caller contract: `s` is a live heap string with validated WTF-8 and
/// `idx < utf16_len`; `cursor` was only ever passed with this same string.
#[no_mangle]
pub extern "C" fn js_string_utf16_unit_at_cursor(
    s: *const StringHeader,
    idx: i32,
    cursor: *mut Utf16Cursor,
) -> i32 {
    if s.is_null() || cursor.is_null() || idx < 0 {
        return -1;
    }
    let idx = idx as usize;
    let byte_len = unsafe { (*s).byte_len };
    let bytes = unsafe { slice::from_raw_parts(string_data(s), byte_len as usize) };
    let cursor = unsafe { &mut *cursor };
    // A backward read restarts; forward reads continue from the cursor.
    if cursor.byte_len != byte_len || idx < cursor.utf16 as usize || cursor.byte > byte_len {
        *cursor = Utf16Cursor {
            byte_len,
            byte: 0,
            utf16: 0,
        };
    }
    let mut byte = cursor.byte as usize;
    let mut utf16 = cursor.utf16 as usize;
    while byte < bytes.len() {
        let (advance, units, cp) = wtf8_step(bytes, byte);
        if units > 0 && utf16 + units > idx {
            cursor.byte = byte as u32;
            cursor.utf16 = utf16 as u32;
            return utf16_index::code_unit(cp, units, idx == utf16) as i32;
        }
        // A truncated tail can step past the payload; never keep that offset.
        byte = byte.saturating_add(advance).min(bytes.len());
        utf16 += units;
    }
    -1
}

#[cfg(test)]
#[path = "utf16_cursor_tests.rs"]
mod tests;
