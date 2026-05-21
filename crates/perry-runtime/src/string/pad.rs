//! `padStart`, `padEnd`, `repeat`, and the default-pad space allocator.

use super::*;

/// Allocate a string containing a single space character " "
/// Used as default pad string for padStart/padEnd
#[no_mangle]
pub extern "C" fn js_string_alloc_space() -> *mut StringHeader {
    js_string_from_bytes(" ".as_ptr(), 1)
}

/// ToLength coercion (ECMA-262 §7.1.21): NaN/negative → 0, +Infinity →
/// `2^53 - 1` (capped here at a sane runtime maximum so callers like
/// `padStart` can't allocate gigabytes from a single bad input). Used by
/// `js_string_pad_start` / `_pad_end` where the codegen passes the raw
/// `f64` length argument. Pre-fix the codegen `fptosi(NaN)`-then-`u32`-
/// cast path produced `0xFFFFFFFF` from a literal `-1` and filled 4 GiB
/// of padding before OOM; literal `NaN` similarly miscompiled via
/// LLVM's undefined `fptosi(NaN)`.
fn to_length_clamped(target_length: f64) -> usize {
    const MAX_PAD_LEN: usize = 1 << 20; // 1 MiB cap — saner than the spec's 2^53-1.
    if target_length.is_nan() || target_length <= 0.0 {
        0
    } else if target_length >= MAX_PAD_LEN as f64 {
        MAX_PAD_LEN
    } else {
        target_length as usize
    }
}

/// Pad the start of a string to reach target length (in UTF-16 code units).
/// str.padStart(targetLength, padString)
#[no_mangle]
pub extern "C" fn js_string_pad_start(
    s: *const StringHeader,
    target_length: f64,
    pad_string: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) {
        string_as_str(pad_string)
    } else {
        " "
    };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = to_length_clamped(target_length);

    if current_len >= target_len || pad_data.is_empty() {
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    let pad_needed = target_len - current_len;
    let _pad_u16: Vec<u16> = pad_data.encode_utf16().collect();
    let mut result = String::with_capacity(target_len * 4);

    // Build padding by UTF-16 code units
    let mut u16_added = 0;
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    while u16_added < pad_needed {
        let ch = pad_chars[pad_idx % pad_chars.len()];
        let ch_u16_len = ch.len_utf16();
        if u16_added + ch_u16_len > pad_needed {
            break;
        }
        result.push(ch);
        u16_added += ch_u16_len;
        pad_idx += 1;
    }

    result.push_str(str_data);

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Pad the end of a string to reach target length (in UTF-16 code units).
/// str.padEnd(targetLength, padString) — see `to_length_clamped` above.
#[no_mangle]
pub extern "C" fn js_string_pad_end(
    s: *const StringHeader,
    target_length: f64,
    pad_string: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) {
        string_as_str(pad_string)
    } else {
        " "
    };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = to_length_clamped(target_length);

    if current_len >= target_len || pad_data.is_empty() {
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    let pad_needed = target_len - current_len;
    let mut result = String::with_capacity(target_len * 4);

    result.push_str(str_data);

    // Build padding by UTF-16 code units
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    let mut u16_added = 0;
    while u16_added < pad_needed {
        let ch = pad_chars[pad_idx % pad_chars.len()];
        let ch_u16_len = ch.len_utf16();
        if u16_added + ch_u16_len > pad_needed {
            break;
        }
        result.push(ch);
        u16_added += ch_u16_len;
        pad_idx += 1;
    }

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Repeat a string a specified number of times
/// str.repeat(count)
#[no_mangle]
pub extern "C" fn js_string_repeat(s: *const StringHeader, count: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || count <= 0 {
        // Return empty string
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let str_data = string_as_str(s);
    if str_data.is_empty() {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let count = count as usize;
    let result = str_data.repeat(count);
    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}
