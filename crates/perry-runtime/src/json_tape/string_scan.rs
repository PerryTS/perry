//! Build key/string entries while retaining the escape fact from validation.

use super::{TapeEntry, STRING_NO_ESCAPES};

/// Tentatively mark this token plain; the existing backslash branch clears it.
/// The Vec cannot grow during the scan, so this native entry borrow stays valid.
/// Failed builds discard the partial tape before any consumer can observe it.
#[inline(always)]
pub(super) fn push_string<const KIND: u8>(
    bytes: &[u8],
    pos: &mut usize,
    entries: &mut Vec<TapeEntry>,
) -> bool {
    debug_assert_eq!(bytes[*pos], b'"');
    entries.push(TapeEntry {
        offset: *pos as u32,
        kind: KIND,
        link: STRING_NO_ESCAPES,
    });
    let entry = entries.last_mut().unwrap();
    *pos += 1;
    while *pos < bytes.len() {
        let Some(offset) = crate::json::simd::find_string_terminator(&bytes[*pos..]) else {
            *pos = bytes.len();
            return false;
        };
        *pos += offset;
        let c = bytes[*pos];
        if c == b'"' {
            *pos += 1;
            return true;
        }
        if c == b'\\' {
            entry.link = 0;
            *pos += 1;
            if *pos >= bytes.len() {
                return false;
            }
            match bytes[*pos] {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => *pos += 1,
                b'u' => {
                    *pos += 1;
                    if *pos + 4 > bytes.len()
                        || !bytes[*pos..*pos + 4].iter().all(u8::is_ascii_hexdigit)
                    {
                        return false;
                    }
                    *pos += 4;
                }
                _ => return false,
            }
        } else if c < 0x20 {
            return false;
        } else {
            *pos += 1;
        }
    }
    false
}
