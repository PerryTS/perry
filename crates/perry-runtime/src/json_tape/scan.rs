//! Byte-only JSON tape scanner helpers.

// Helper: skip whitespace.
#[inline(always)]
pub(super) fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() {
        match bytes[*pos] {
            b' ' | b'\t' | b'\n' | b'\r' => *pos += 1,
            _ => break,
        }
    }
}

// Helper: validate and skip a JSON string in place (past the closing
// quote). Decoding remains deferred to materialization.
#[inline(always)]
pub(super) fn skip_string(bytes: &[u8], pos: &mut usize) -> bool {
    debug_assert_eq!(bytes[*pos], b'"');
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
            *pos += 1;
            if *pos >= bytes.len() {
                return false;
            }
            match bytes[*pos] {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => *pos += 1,
                b'u' => {
                    *pos += 1;
                    if *pos + 4 > bytes.len() {
                        *pos = bytes.len();
                        return false;
                    }
                    if let Some(bad) = bytes[*pos..*pos + 4]
                        .iter()
                        .position(|byte| !byte.is_ascii_hexdigit())
                    {
                        *pos += bad;
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

// Helper: validate and skip a JSON number (past its last digit/exponent).
#[inline(always)]
pub(super) fn skip_number(bytes: &[u8], pos: &mut usize) -> bool {
    if *pos < bytes.len() && bytes[*pos] == b'-' {
        *pos += 1;
    }
    match bytes.get(*pos) {
        Some(b'0') => *pos += 1,
        Some(b'1'..=b'9') => {
            *pos += 1;
            *pos += crate::json::simd::count_ascii_digits(&bytes[*pos..]);
        }
        _ => return false,
    }
    if *pos < bytes.len() && bytes[*pos] == b'.' {
        *pos += 1;
        let fraction_start = *pos;
        *pos += crate::json::simd::count_ascii_digits(&bytes[*pos..]);
        if *pos == fraction_start {
            return false;
        }
    }
    if *pos < bytes.len() && (bytes[*pos] == b'e' || bytes[*pos] == b'E') {
        *pos += 1;
        if *pos < bytes.len() && (bytes[*pos] == b'+' || bytes[*pos] == b'-') {
            *pos += 1;
        }
        let exponent_start = *pos;
        *pos += crate::json::simd::count_ascii_digits(&bytes[*pos..]);
        if *pos == exponent_start {
            return false;
        }
    }
    true
}
