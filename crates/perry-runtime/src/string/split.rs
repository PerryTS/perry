//! `String.prototype.split` — by-string and by-empty-delimiter, with limit.

use super::*;
use crate::array::ArrayHeader;

/// Split a string by a delimiter
/// Returns an array of string pointers (stored as f64 bit patterns)
#[no_mangle]
pub extern "C" fn js_string_split(
    s: *const StringHeader,
    delimiter: *const StringHeader,
) -> *mut ArrayHeader {
    js_string_split_n(s, delimiter, -1)
}

/// Split a string by a delimiter, with optional limit (issue #567).
/// `limit < 0` → no limit (matches `js_string_split`).
/// `limit == 0` → empty array.
/// `limit > 0` → at most `limit` substrings.
#[no_mangle]
pub extern "C" fn js_string_split_n(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    limit: i32,
) -> *mut ArrayHeader {
    if !is_valid_string_ptr(s) {
        // Return empty array
        return crate::array::js_array_alloc(0);
    }

    // The LLVM backend can't always statically distinguish `s.split(regex)`
    // from `s.split(string)` at the call site — it uses a single decl for
    // both. Detect regex delimiters by checking whether the pointer was
    // recorded by `js_regexp_new` and delegate to `js_string_split_regex`
    // on a match. Otherwise the regex header would be read as a
    // StringHeader and segfault on the first byte of its `regex_ptr`.
    if crate::regex::is_regex_pointer(delimiter as *const u8) {
        return crate::regex::js_string_split_regex_n(
            s,
            delimiter as *const crate::regex::RegExpHeader,
            limit,
        );
    }

    if limit == 0 {
        return crate::array::js_array_alloc(0);
    }

    let str_data = string_as_str(s);
    let delim = if !is_valid_string_ptr(delimiter) {
        ""
    } else {
        string_as_str(delimiter)
    };

    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    if delim.is_empty() {
        // Empty delimiter: split into individual characters (single pass)
        let mut parts: Vec<*mut StringHeader> = str_data
            .chars()
            .map(|c| {
                let mut buf = [0u8; 4];
                let char_str = c.encode_utf8(&mut buf);
                js_string_from_bytes(char_str.as_ptr(), char_str.len() as u32)
            })
            .collect();
        if limit > 0 && (parts.len() as i64) > (limit as i64) {
            parts.truncate(limit as usize);
        }

        let arr = crate::array::js_array_alloc(parts.len() as u32);
        unsafe {
            (*arr).length = parts.len() as u32;
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for (i, p) in parts.iter().enumerate() {
                let nanboxed = STRING_TAG | (*p as u64 & POINTER_MASK);
                std::ptr::write(elements_ptr.add(i), f64::from_bits(nanboxed));
                crate::array::note_array_slot(arr, i, nanboxed);
            }
        }
        return arr;
    }

    // Non-empty delimiter: arena-allocate parts (bump-pointer, no tracking overhead)
    let mut part_slices: Vec<&str> = str_data.split(delim).collect();
    if limit > 0 && (part_slices.len() as i64) > (limit as i64) {
        part_slices.truncate(limit as usize);
    }
    let n = part_slices.len();

    let src_is_ascii = is_ascii_string(s);

    let arr = crate::array::js_array_alloc(n as u32);
    unsafe {
        (*arr).length = n as u32;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for (i, part) in part_slices.iter().enumerate() {
            let byte_len = part.len() as u32;
            let (sh, data_ptr) = string_storage_alloc(byte_len);
            let utf16_len = if src_is_ascii {
                byte_len
            } else {
                compute_utf16_len(part.as_ptr(), byte_len)
            };
            init_string_header(sh, utf16_len, byte_len, byte_len, 0, 0);
            if byte_len > 0 {
                ptr::copy_nonoverlapping(part.as_ptr(), data_ptr, byte_len as usize);
            }
            let nanboxed = STRING_TAG | (sh as u64 & POINTER_MASK);
            std::ptr::write(elements_ptr.add(i), f64::from_bits(nanboxed));
            crate::array::note_array_slot(arr, i, nanboxed);
        }
    }

    arr
}
