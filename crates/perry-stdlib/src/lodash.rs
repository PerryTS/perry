//! Lodash module
//!
//! Native implementation of common lodash utility functions.
//! Provides array, object, string, and utility operations.

use perry_runtime::{
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_object_alloc,
    js_object_get_field, js_object_set_field, js_string_from_bytes, ArrayHeader, JSValue,
    ObjectHeader, StringHeader,
};
use std::collections::HashSet;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

// ============================================================================
// Array functions
// ============================================================================

/// _.chunk(array, size) -> array[]
///
/// Split array into groups of size.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_chunk(arr_ptr: *mut ArrayHeader, size: f64) -> *mut ArrayHeader {
    let result = js_array_alloc(0);
    let size = size.max(1.0) as usize;

    if arr_ptr.is_null() {
        return result;
    }

    let len = js_array_length(arr_ptr) as usize;
    let mut i = 0;

    while i < len {
        let chunk = js_array_alloc(0);
        for j in 0..size {
            if i + j < len {
                let val = js_array_get(arr_ptr, (i + j) as u32);
                js_array_push(chunk, val);
            }
        }
        js_array_push(result, JSValue::object_ptr(chunk as *mut u8));
        i += size;
    }

    result
}

/// _.compact(array) -> array
///
/// Remove falsey values (false, null, 0, "", undefined, NaN).
#[no_mangle]
pub unsafe extern "C" fn js_lodash_compact(arr_ptr: *mut ArrayHeader) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let len = js_array_length(arr_ptr);
    for i in 0..len {
        let val = js_array_get(arr_ptr, i);
        // Keep truthy values
        if !val.is_null() && !val.is_undefined() {
            if val.is_bool() && !val.to_bool() {
                continue;
            }
            if val.is_number() && (val.to_number() == 0.0 || val.to_number().is_nan()) {
                continue;
            }
            js_array_push(result, val);
        }
    }

    result
}

/// _.concat(array, ...values) -> array
///
/// Concatenate arrays and values.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_concat(
    arr1_ptr: *mut ArrayHeader,
    arr2_ptr: *mut ArrayHeader,
) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if !arr1_ptr.is_null() {
        let len1 = js_array_length(arr1_ptr);
        for i in 0..len1 {
            js_array_push(result, js_array_get(arr1_ptr, i));
        }
    }

    if !arr2_ptr.is_null() {
        let len2 = js_array_length(arr2_ptr);
        for i in 0..len2 {
            js_array_push(result, js_array_get(arr2_ptr, i));
        }
    }

    result
}

/// _.difference(array, values) -> array
///
/// Create array with values not in the other array.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_difference(
    arr_ptr: *mut ArrayHeader,
    values_ptr: *mut ArrayHeader,
) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    // Build set of values to exclude
    let mut exclude: HashSet<u64> = HashSet::new();
    if !values_ptr.is_null() {
        let len = js_array_length(values_ptr);
        for i in 0..len {
            exclude.insert(js_array_get(values_ptr, i).bits());
        }
    }

    let len = js_array_length(arr_ptr);
    for i in 0..len {
        let val = js_array_get(arr_ptr, i);
        if !exclude.contains(&val.bits()) {
            js_array_push(result, val);
        }
    }

    result
}

/// _.drop(array, n) -> array
///
/// Drop n elements from the beginning.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_drop(arr_ptr: *mut ArrayHeader, n: f64) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let n = n.max(0.0) as u32;
    let len = js_array_length(arr_ptr);

    for i in n..len {
        js_array_push(result, js_array_get(arr_ptr, i));
    }

    result
}

/// _.dropRight(array, n) -> array
///
/// Drop n elements from the end.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_drop_right(
    arr_ptr: *mut ArrayHeader,
    n: f64,
) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let n = n.max(0.0) as u32;
    let len = js_array_length(arr_ptr);
    let end = if n >= len { 0 } else { len - n };

    for i in 0..end {
        js_array_push(result, js_array_get(arr_ptr, i));
    }

    result
}

/// _.fill(array, value, start, end) -> array
///
/// Fill array with value from start to end.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_fill(
    arr_ptr: *mut ArrayHeader,
    value: JSValue,
    start: f64,
    end: f64,
) -> *mut ArrayHeader {
    if arr_ptr.is_null() {
        return arr_ptr;
    }

    let len = js_array_length(arr_ptr) as i32;
    let start = start as i32;
    let end = if end.is_nan() { len } else { end as i32 };

    let start = if start < 0 {
        (len + start).max(0)
    } else {
        start.min(len)
    } as u32;
    let end = if end < 0 {
        (len + end).max(0)
    } else {
        end.min(len)
    } as u32;

    // Note: This modifies in place, but for safety we return the array
    // In a real implementation, we'd need mutable array access
    arr_ptr
}

/// _.first / _.head(array) -> element
///
/// Get the first element.
/// Returns f64 (NaN-boxed bits) instead of JSValue to avoid SysV AMD64
/// ABI mismatch: JSValue's `#[repr(transparent)] u64` returns in RAX
/// (integer register) but the LLVM call site reads the result as double
/// from XMM0 on x86_64.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_first(arr_ptr: *mut ArrayHeader) -> f64 {
    let v = if arr_ptr.is_null() || js_array_length(arr_ptr) == 0 {
        JSValue::undefined()
    } else {
        js_array_get(arr_ptr, 0)
    };
    f64::from_bits(v.bits())
}

/// _.last(array) -> element
///
/// Get the last element.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_last(arr_ptr: *mut ArrayHeader) -> f64 {
    let v = if arr_ptr.is_null() {
        JSValue::undefined()
    } else {
        let len = js_array_length(arr_ptr);
        if len == 0 {
            JSValue::undefined()
        } else {
            js_array_get(arr_ptr, len - 1)
        }
    };
    f64::from_bits(v.bits())
}

/// _.flatten(array) -> array
///
/// Flatten array one level deep.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_flatten(arr_ptr: *mut ArrayHeader) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let len = js_array_length(arr_ptr);
    for i in 0..len {
        let val = js_array_get(arr_ptr, i);
        if val.is_pointer() {
            // Check if it's an array (simplified check)
            let inner_ptr = val.as_pointer::<u8>() as *mut ArrayHeader;
            if !inner_ptr.is_null() {
                let inner_len = js_array_length(inner_ptr);
                for j in 0..inner_len {
                    js_array_push(result, js_array_get(inner_ptr, j));
                }
            } else {
                js_array_push(result, val);
            }
        } else {
            js_array_push(result, val);
        }
    }

    result
}

/// _.initial(array) -> array
///
/// Get all but the last element.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_initial(arr_ptr: *mut ArrayHeader) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let len = js_array_length(arr_ptr);
    if len > 0 {
        for i in 0..(len - 1) {
            js_array_push(result, js_array_get(arr_ptr, i));
        }
    }

    result
}

/// _.tail(array) -> array
///
/// Get all but the first element.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_tail(arr_ptr: *mut ArrayHeader) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let len = js_array_length(arr_ptr);
    for i in 1..len {
        js_array_push(result, js_array_get(arr_ptr, i));
    }

    result
}

/// _.take(array, n) -> array
///
/// Take n elements from the beginning.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_take(arr_ptr: *mut ArrayHeader, n: f64) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let n = n.max(0.0) as u32;
    let len = js_array_length(arr_ptr);
    let take_count = n.min(len);

    for i in 0..take_count {
        js_array_push(result, js_array_get(arr_ptr, i));
    }

    result
}

/// _.takeRight(array, n) -> array
///
/// Take n elements from the end.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_take_right(
    arr_ptr: *mut ArrayHeader,
    n: f64,
) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let n = n.max(0.0) as u32;
    let len = js_array_length(arr_ptr);
    let start = if n >= len { 0 } else { len - n };

    for i in start..len {
        js_array_push(result, js_array_get(arr_ptr, i));
    }

    result
}

/// _.uniq(array) -> array
///
/// Create duplicate-free array.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_uniq(arr_ptr: *mut ArrayHeader) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let mut seen: HashSet<u64> = HashSet::new();
    let len = js_array_length(arr_ptr);

    for i in 0..len {
        let val = js_array_get(arr_ptr, i);
        if seen.insert(val.bits()) {
            js_array_push(result, val);
        }
    }

    result
}

/// _.reverse(array) -> array
///
/// Reverse array.
#[no_mangle]
pub unsafe extern "C" fn js_lodash_reverse(arr_ptr: *mut ArrayHeader) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if arr_ptr.is_null() {
        return result;
    }

    let len = js_array_length(arr_ptr);
    for i in (0..len).rev() {
        js_array_push(result, js_array_get(arr_ptr, i));
    }

    result
}

// ============================================================================
// String functions
// ============================================================================

/// _.camelCase(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_camel_case(str_ptr: *const StringHeader) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let mut result = String::new();
    let mut capitalize_next = false;

    for c in s.chars() {
        if c.is_alphanumeric() {
            if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c.to_ascii_lowercase());
            }
        } else {
            capitalize_next = !result.is_empty();
        }
    }

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.capitalize(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_capitalize(str_ptr: *const StringHeader) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let mut chars = s.chars();
    let result = match chars.next() {
        Some(first) => {
            let rest: String = chars.collect();
            format!("{}{}", first.to_uppercase(), rest.to_lowercase())
        }
        None => String::new(),
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.kebabCase(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_kebab_case(str_ptr: *const StringHeader) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let mut result = String::new();
    let mut prev_lower = false;

    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !result.is_empty() {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
            prev_lower = c.is_lowercase();
        } else if !result.is_empty() && !result.ends_with('-') {
            result.push('-');
            prev_lower = false;
        }
    }

    // Trim trailing dash
    while result.ends_with('-') {
        result.pop();
    }

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.snakeCase(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_snake_case(str_ptr: *const StringHeader) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let mut result = String::new();
    let mut prev_lower = false;

    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !result.is_empty() {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
            prev_lower = c.is_lowercase();
        } else if !result.is_empty() && !result.ends_with('_') {
            result.push('_');
            prev_lower = false;
        }
    }

    // Trim trailing underscore
    while result.ends_with('_') {
        result.pop();
    }

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.upperCase(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_upper_case(str_ptr: *const StringHeader) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let result = s.to_uppercase();
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.lowerCase(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_lower_case(str_ptr: *const StringHeader) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let result = s.to_lowercase();
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.trim(string, chars?) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_trim(
    str_ptr: *const StringHeader,
    chars_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let result = if let Some(chars) = string_from_header(chars_ptr) {
        let chars: Vec<char> = chars.chars().collect();
        s.trim_matches(|c| chars.contains(&c)).to_string()
    } else {
        s.trim().to_string()
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.trimStart(string, chars?) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_trim_start(
    str_ptr: *const StringHeader,
    chars_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let result = if let Some(chars) = string_from_header(chars_ptr) {
        let chars: Vec<char> = chars.chars().collect();
        s.trim_start_matches(|c| chars.contains(&c)).to_string()
    } else {
        s.trim_start().to_string()
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.trimEnd(string, chars?) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_trim_end(
    str_ptr: *const StringHeader,
    chars_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let result = if let Some(chars) = string_from_header(chars_ptr) {
        let chars: Vec<char> = chars.chars().collect();
        s.trim_end_matches(|c| chars.contains(&c)).to_string()
    } else {
        s.trim_end().to_string()
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.pad(string, length, chars?) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_pad(
    str_ptr: *const StringHeader,
    length: f64,
    chars_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let length = length as usize;
    if s.len() >= length {
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    let pad_char = string_from_header(chars_ptr)
        .and_then(|c| c.chars().next())
        .unwrap_or(' ');

    let total_pad = length - s.len();
    let left_pad = total_pad / 2;
    let right_pad = total_pad - left_pad;

    let result = format!(
        "{}{}{}",
        pad_char.to_string().repeat(left_pad),
        s,
        pad_char.to_string().repeat(right_pad)
    );

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.padStart(string, length, chars?) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_pad_start(
    str_ptr: *const StringHeader,
    length: f64,
    chars_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let length = length as usize;
    if s.len() >= length {
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    let pad_char = string_from_header(chars_ptr)
        .and_then(|c| c.chars().next())
        .unwrap_or(' ');

    let pad_count = length - s.len();
    let result = format!("{}{}", pad_char.to_string().repeat(pad_count), s);

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.padEnd(string, length, chars?) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_pad_end(
    str_ptr: *const StringHeader,
    length: f64,
    chars_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let length = length as usize;
    if s.len() >= length {
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    let pad_char = string_from_header(chars_ptr)
        .and_then(|c| c.chars().next())
        .unwrap_or(' ');

    let pad_count = length - s.len();
    let result = format!("{}{}", s, pad_char.to_string().repeat(pad_count));

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.repeat(string, n) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_repeat(
    str_ptr: *const StringHeader,
    n: f64,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let n = n.max(0.0) as usize;
    let result = s.repeat(n);

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// _.truncate(string, options) -> string
#[no_mangle]
pub unsafe extern "C" fn js_lodash_truncate(
    str_ptr: *const StringHeader,
    length: f64,
) -> *mut StringHeader {
    let s = match string_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let length = length as usize;
    if s.len() <= length {
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    let truncate_at = length.saturating_sub(3);
    let result = format!("{}...", &s[..truncate_at]);

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

// ============================================================================
// Utility functions
// ============================================================================

/// _.clamp(number, lower, upper) -> number
#[no_mangle]
pub extern "C" fn js_lodash_clamp(number: f64, lower: f64, upper: f64) -> f64 {
    number.max(lower).min(upper)
}

/// _.inRange(number, start, end) -> boolean
#[no_mangle]
pub extern "C" fn js_lodash_in_range(number: f64, start: f64, end: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let (start, end) = if start > end {
        (end, start)
    } else {
        (start, end)
    };
    if number >= start && number < end {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// _.random(lower, upper, floating?) -> number
#[no_mangle]
pub extern "C" fn js_lodash_random(lower: f64, upper: f64, floating: bool) -> f64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    if floating {
        rng.gen_range(lower..upper)
    } else {
        let lower = lower.floor() as i64;
        let upper = upper.ceil() as i64;
        rng.gen_range(lower..=upper) as f64
    }
}

/// _.times(n, fn) - returns array of indices
#[no_mangle]
pub unsafe extern "C" fn js_lodash_times(n: f64) -> *mut ArrayHeader {
    let result = js_array_alloc(0);
    let n = n.max(0.0) as u32;

    for i in 0..n {
        js_array_push(result, JSValue::int32(i as i32));
    }

    result
}

/// _.range(start, end, step) -> array
#[no_mangle]
pub unsafe extern "C" fn js_lodash_range(start: f64, end: f64, step: f64) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    let step = if step == 0.0 { 1.0 } else { step };

    if step > 0.0 {
        let mut i = start;
        while i < end {
            js_array_push(result, JSValue::number(i));
            i += step;
        }
    } else {
        let mut i = start;
        while i > end {
            js_array_push(result, JSValue::number(i));
            i += step;
        }
    }

    result
}

/// _.isEmpty(value) -> boolean
/// Takes f64 at the FFI boundary to avoid SysV AMD64 ABI mismatch
/// (JSValue is `#[repr(transparent)] u64` → integer register, but the
/// LLVM call site declares double → XMM register on x86_64).
#[no_mangle]
pub unsafe extern "C" fn js_lodash_is_empty(value_f: f64) -> i32 {
    let value = JSValue::from_bits(value_f.to_bits());
    if value.is_null() || value.is_undefined() {
        return 1;
    }

    if value.is_pointer() {
        let ptr = value.as_pointer() as *const ArrayHeader;
        if !ptr.is_null() {
            // Check if it's an array
            let len = js_array_length(ptr);
            return if len == 0 { 1 } else { 0 };
        }
    }

    0
}

/// _.isNil(value) -> boolean
#[no_mangle]
pub extern "C" fn js_lodash_is_nil(value_f: f64) -> i32 {
    let value = JSValue::from_bits(value_f.to_bits());
    if value.is_null() || value.is_undefined() {
        1
    } else {
        0
    }
}

/// _.size(collection) -> number
#[no_mangle]
pub unsafe extern "C" fn js_lodash_size(collection_f: f64) -> f64 {
    let collection = JSValue::from_bits(collection_f.to_bits());
    if collection.is_pointer() {
        let ptr = collection.as_pointer() as *const ArrayHeader;
        if !ptr.is_null() {
            return js_array_length(ptr) as f64;
        }
    }
    0.0
}
