//! Equality / comparison / starts-with / ends-with / well-formedness /
//! normalization / locale-compare.

use super::*;

/// Compare two strings lexicographically.
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
#[no_mangle]
pub extern "C" fn js_string_compare(a: *const StringHeader, b: *const StringHeader) -> i32 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0;
    }
    if !a_valid {
        return -1;
    }
    if !b_valid {
        return 1;
    }

    unsafe {
        let len_a = (*a).byte_len as usize;
        let len_b = (*b).byte_len as usize;
        let data_a = string_data(a);
        let data_b = string_data(b);
        let a_bytes = std::slice::from_raw_parts(data_a, len_a);
        let b_bytes = std::slice::from_raw_parts(data_b, len_b);
        match a_bytes.cmp(b_bytes) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Compare two strings for equality
#[no_mangle]
pub extern "C" fn js_string_equals(a: *const StringHeader, b: *const StringHeader) -> i32 {
    // Pointer identity fast path
    if std::ptr::eq(a, b) {
        return 1;
    }

    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 1;
    }
    if !a_valid || !b_valid {
        return 0;
    }

    let blen_a = unsafe { (*a).byte_len };
    let blen_b = unsafe { (*b).byte_len };

    if blen_a != blen_b {
        return 0;
    }

    unsafe {
        let data_a = string_data(a);
        let data_b = string_data(b);
        let slice_a = std::slice::from_raw_parts(data_a, blen_a as usize);
        let slice_b = std::slice::from_raw_parts(data_b, blen_b as usize);
        if slice_a == slice_b {
            1
        } else {
            0
        }
    }
}

/// Check if a string starts with a prefix
#[no_mangle]
pub extern "C" fn js_string_starts_with(
    s: *const StringHeader,
    prefix: *const StringHeader,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let blen = unsafe { (*s).byte_len };
    let prefix_blen = unsafe { (*prefix).byte_len };

    if prefix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let prefix_data = string_data(prefix);

        for i in 0..prefix_blen as usize {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with a suffix
#[no_mangle]
pub extern "C" fn js_string_ends_with(s: *const StringHeader, suffix: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let blen = unsafe { (*s).byte_len };
    let suffix_blen = unsafe { (*suffix).byte_len };

    if suffix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let suffix_data = string_data(suffix);
        let start = blen - suffix_blen;

        for i in 0..suffix_blen as usize {
            if *data.add(start as usize + i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string starts with `prefix` at UTF-16 code-unit `position`.
/// Mirrors `String.prototype.startsWith(searchString, position)` — clamps
/// negative positions to 0 and positions past the end to length.
#[no_mangle]
pub extern "C" fn js_string_starts_with_at(
    s: *const StringHeader,
    prefix: *const StringHeader,
    position: i32,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let u16len = unsafe { (*s).utf16_len } as i32;
    let pos = position.max(0).min(u16len) as usize;

    let prefix_blen = unsafe { (*prefix).byte_len } as usize;

    let byte_start = if is_ascii_string(s) {
        pos
    } else {
        utf16_offset_to_byte_offset(string_as_str(s), pos)
    };

    let blen = unsafe { (*s).byte_len } as usize;
    if byte_start + prefix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s).add(byte_start);
        let prefix_data = string_data(prefix);
        for i in 0..prefix_blen {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with `suffix` if truncated to UTF-16 code-unit
/// `end_position`. Mirrors `String.prototype.endsWith(searchString, endPosition)`
/// — clamps negative positions to 0 and positions past the end to length.
#[no_mangle]
pub extern "C" fn js_string_ends_with_at(
    s: *const StringHeader,
    suffix: *const StringHeader,
    end_position: i32,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let u16len = unsafe { (*s).utf16_len } as i32;
    let end_u16 = end_position.max(0).min(u16len) as usize;

    let byte_end = if is_ascii_string(s) {
        end_u16
    } else {
        utf16_offset_to_byte_offset(string_as_str(s), end_u16)
    };

    let suffix_blen = unsafe { (*suffix).byte_len } as usize;
    if suffix_blen > byte_end {
        return 0;
    }

    let byte_start = byte_end - suffix_blen;

    unsafe {
        let data = string_data(s).add(byte_start);
        let suffix_data = string_data(suffix);
        for i in 0..suffix_blen {
            if *data.add(i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// String.prototype.normalize(form) — Unicode normalization.
/// `form` is one of: NFC (default), NFD, NFKC, NFKD. Pass null/empty for default NFC.
#[no_mangle]
pub extern "C" fn js_string_normalize(
    s: *const StringHeader,
    form: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let form_str = if is_valid_string_ptr(form) {
        string_as_str(form)
    } else {
        "NFC"
    };
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = match form_str {
        "NFC" => str_data.nfc().collect(),
        "NFD" => str_data.nfd().collect(),
        "NFKC" => str_data.nfkc().collect(),
        "NFKD" => str_data.nfkd().collect(),
        _ => str_data.nfc().collect(),
    };
    let bytes = normalized.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// String.prototype.localeCompare(other) — returns negative/zero/positive number.
/// We don't ship a true ICU collator. We approximate the Unicode default
/// collation with a two-pass comparison: first case-insensitive (so the
/// character class wins) and then case-sensitive with lowercase < uppercase
/// (matching V8's default ICU behavior where 'a' < 'A').
#[no_mangle]
pub extern "C" fn js_string_locale_compare(a: *const StringHeader, b: *const StringHeader) -> f64 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0.0;
    }
    if !a_valid {
        return -1.0;
    }
    if !b_valid {
        return 1.0;
    }
    let a_str = string_as_str(a);
    let b_str = string_as_str(b);
    // Case-insensitive primary comparison
    let a_lower = a_str.to_lowercase();
    let b_lower = b_str.to_lowercase();
    match a_lower.cmp(&b_lower) {
        std::cmp::Ordering::Less => return -1.0,
        std::cmp::Ordering::Greater => return 1.0,
        std::cmp::Ordering::Equal => {}
    }
    // Same letters ignoring case — order by case (lowercase < uppercase
    // per the default Unicode collation tertiary weight).
    let mut ai = a_str.chars();
    let mut bi = b_str.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return 0.0,
            (None, Some(_)) => return -1.0,
            (Some(_), None) => return 1.0,
            (Some(ca), Some(cb)) => {
                if ca == cb {
                    continue;
                }
                let a_lower = ca.is_lowercase();
                let b_lower = cb.is_lowercase();
                if a_lower && !b_lower {
                    return -1.0;
                }
                if !a_lower && b_lower {
                    return 1.0;
                }
                return if (ca as u32) < (cb as u32) { -1.0 } else { 1.0 };
            }
        }
    }
}

/// String.prototype.isWellFormed() — returns NaN-boxed boolean.
/// A string is well-formed if it contains no lone surrogates.
/// Lone-surrogate strings are marked with STRING_FLAG_HAS_LONE_SURROGATES at construction.
#[no_mangle]
pub extern "C" fn js_string_is_well_formed(s: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if !is_valid_string_ptr(s) {
        return f64::from_bits(TAG_TRUE);
    }
    let flags = unsafe { (*s).flags };
    if flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
        return f64::from_bits(TAG_FALSE);
    }
    f64::from_bits(TAG_TRUE)
}

/// String.prototype.toWellFormed() — replaces lone surrogates with U+FFFD (U+FFFD = EF BF BD).
/// Works directly on WTF-8 bytes: replaces each 3-byte surrogate sequence
/// (ED A0..BF 80..BF) with the 3-byte U+FFFD encoding.
#[no_mangle]
pub extern "C" fn js_string_to_well_formed(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let flags = unsafe { (*s).flags };
    let blen = unsafe { (*s).byte_len } as usize;
    let data = string_data(s);
    if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 {
        // Well-formed UTF-8: return a copy without scanning
        return js_string_from_bytes(data, blen as u32);
    }
    // Scan raw bytes and replace every WTF-8 lone-surrogate sequence with U+FFFD.
    // WTF-8 surrogate: first byte = 0xED, second = 0xA0..=0xBF, third = 0x80..=0xBF.
    let bytes = unsafe { slice::from_raw_parts(data, blen) };
    let mut result: Vec<u8> = Vec::with_capacity(blen);
    let mut i = 0;
    while i < blen {
        let b = bytes[i];
        if b == 0xED
            && i + 2 < blen
            && (0xA0..=0xBF).contains(&bytes[i + 1])
            && (0x80..=0xBF).contains(&bytes[i + 2])
        {
            // Lone surrogate → U+FFFD (EF BF BD)
            result.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
            i += 3;
        } else if b < 0x80 {
            result.push(b);
            i += 1;
        } else if b < 0xC0 {
            result.push(b);
            i += 1;
        } else if b < 0xE0 {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            i += 2;
        } else if b < 0xF0 {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            if i + 2 < blen {
                result.push(bytes[i + 2]);
            }
            i += 3;
        } else {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            if i + 2 < blen {
                result.push(bytes[i + 2]);
            }
            if i + 3 < blen {
                result.push(bytes[i + 3]);
            }
            i += 4;
        }
    }
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}
