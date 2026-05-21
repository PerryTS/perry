//! Character-level access: charCodeAt, charAt, codePointAt, fromCharCode,
//! fromCodePoint, at, plus spread-into-array (`toCharArray`).

use super::*;

/// Get character code at index (returns UTF-16 code unit, or NaN if out of bounds).
/// Index is in UTF-16 code units (matches JS spec). For ASCII strings this is
/// equivalent to byte indexing; for multi-byte UTF-8 we walk codepoints without
/// allocating — the old `encode_utf16().collect()` path made hashing a 68 MB
/// string O(n²) (issue #65).
#[no_mangle]
pub extern "C" fn js_string_char_code_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::NAN;
    }

    let u16len = unsafe { (*s).utf16_len } as usize;
    let idx = index as usize;
    if idx >= u16len {
        return f64::NAN;
    }

    // ASCII fast path: byte_len == utf16_len means every byte is one
    // UTF-16 code unit. Direct byte index, no scan, no allocation.
    if is_ascii_string(s) {
        unsafe {
            return *string_data(s).add(idx) as f64;
        }
    }

    // Non-ASCII: walk codepoints counting UTF-16 units. Allocation-free.
    let str_data = string_as_str(s);
    let mut utf16_pos = 0usize;
    for ch in str_data.chars() {
        let clen = ch.len_utf16();
        if utf16_pos + clen > idx {
            if clen == 1 {
                return ch as u32 as f64;
            }
            let mut buf = [0u16; 2];
            ch.encode_utf16(&mut buf);
            return buf[idx - utf16_pos] as f64;
        }
        utf16_pos += clen;
    }
    f64::NAN
}

/// Get character at UTF-16 code unit index (returns single-character string).
/// For a BMP character this returns the character itself; for a surrogate half
/// of an astral character this returns the lone surrogate (matching JS behavior).
#[no_mangle]
pub extern "C" fn js_string_char_at(s: *const StringHeader, index: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || index < 0 {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    let u16len = unsafe { (*s).utf16_len };
    if index as u32 >= u16len {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // ASCII fast path: skip utf16_len scan
    if is_ascii_string(s) {
        unsafe {
            let data = string_data(s);
            let char_ptr = data.add(index as usize);
            return js_string_from_ascii_bytes(char_ptr, 1);
        }
    }

    // UTF-16 path: find the UTF-8 bytes for the character at this UTF-16 index
    let str_data = string_as_str(s);
    let byte_off = utf16_offset_to_byte_offset(str_data, index as usize);
    let remaining = &str_data[byte_off..];
    if let Some(ch) = remaining.chars().next() {
        let ch_len = ch.len_utf8();
        js_string_from_bytes(remaining.as_ptr(), ch_len as u32)
    } else {
        js_string_from_bytes(std::ptr::null(), 0)
    }
}

/// Split a string into an array of single-character strings.
/// Used by the spread operator: `[..."hello"]` → `["h","e","l","l","o"]`.
/// JS spread iterates by codepoints (not UTF-16 units), so "😀" → ["😀"] (1 element).
/// Returns an ArrayHeader pointer with NaN-boxed STRING_TAG elements.
#[no_mangle]
pub extern "C" fn js_string_to_char_array(s: i64) -> i64 {
    let str_ptr = (s as u64 & crate::value::POINTER_MASK) as *const StringHeader;
    if str_ptr.is_null() || !is_valid_string_ptr(str_ptr) {
        return crate::array::js_array_alloc(0) as i64;
    }
    let str_data = string_as_str(str_ptr);
    let char_count = str_data.chars().count();
    let arr = crate::array::js_array_alloc_with_length(char_count as u32);
    let elements = unsafe { (arr as *mut u8).add(8) as *mut f64 };
    for (i, ch) in str_data.chars().enumerate() {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        let ch_ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
        let nanboxed =
            f64::from_bits(crate::value::STRING_TAG | (ch_ptr as u64 & crate::value::POINTER_MASK));
        unsafe {
            *elements.add(i) = nanboxed;
            crate::array::note_array_slot(arr, i, nanboxed.to_bits());
        }
    }
    arr as i64
}

/// Create a string from a character code (String.fromCharCode)
/// Takes a single character code and returns a 1-character string
#[no_mangle]
pub extern "C" fn js_string_from_char_code(code: i32) -> *mut StringHeader {
    if !(0..=0xFFFF).contains(&code) {
        // Invalid character code, return empty string
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // For ASCII characters, create a simple 1-byte string
    if code < 128 {
        let byte = code as u8;
        return js_string_from_bytes(&byte as *const u8, 1);
    }

    // For non-ASCII, encode as UTF-8
    let ch = char::from_u32(code as u32).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// Create a string from a Unicode code point (String.fromCodePoint).
/// Supports the full Unicode range (0..0x10FFFF), unlike fromCharCode (0..0xFFFF).
#[no_mangle]
pub extern "C" fn js_string_from_code_point(code: i32) -> *mut StringHeader {
    if !(0..=0x10FFFF).contains(&code) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let ch = match char::from_u32(code as u32) {
        Some(c) => c,
        None => return js_string_from_bytes(std::ptr::null(), 0),
    };
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// String.prototype.at(index) — supports negative indices.
/// Returns NaN-boxed single-char string, or NaN-boxed undefined if out of bounds.
/// Index is in UTF-16 code units (matches JS spec).
#[no_mangle]
pub extern "C" fn js_string_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let str_data = string_as_str(s);
    let utf16: Vec<u16> = str_data.encode_utf16().collect();
    let len = utf16.len() as i32;
    let resolved = if index < 0 { len + index } else { index };
    if resolved < 0 || resolved >= len {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Decode the UTF-16 code unit at `resolved`. If it's a high surrogate followed
    // by a low surrogate, decode the pair; otherwise the unit is the code point.
    let unit = utf16[resolved as usize];
    let cp: u32 = if (0xD800..=0xDBFF).contains(&unit) && (resolved + 1) < len {
        let next = utf16[(resolved + 1) as usize];
        if (0xDC00..=0xDFFF).contains(&next) {
            0x10000 + ((unit as u32 - 0xD800) << 10) + (next as u32 - 0xDC00)
        } else {
            unit as u32
        }
    } else {
        unit as u32
    };
    let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

/// String.prototype.codePointAt(index) — returns the Unicode code point at the given
/// UTF-16 code unit position, or NaN-boxed undefined if out of bounds.
#[no_mangle]
pub extern "C" fn js_string_code_point_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let u16len = unsafe { (*s).utf16_len } as usize;
    let idx = index as usize;
    if idx >= u16len {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // ASCII fast path — identical to charCodeAt's.
    if is_ascii_string(s) {
        unsafe {
            return *string_data(s).add(idx) as f64;
        }
    }

    // Non-ASCII: walk codepoints without allocating a Vec<u16>.
    let str_data = string_as_str(s);
    let mut utf16_pos = 0usize;
    for ch in str_data.chars() {
        let clen = ch.len_utf16();
        if utf16_pos + clen > idx {
            if clen == 1 || utf16_pos == idx {
                // Either a BMP char, or the start of a surrogate pair
                // (which is the whole codepoint per the spec).
                return ch as u32 as f64;
            }
            // Index lands on the low surrogate — return the bare unit.
            let mut buf = [0u16; 2];
            ch.encode_utf16(&mut buf);
            return buf[idx - utf16_pos] as f64;
        }
        utf16_pos += clen;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
