use super::*;

pub(super) unsafe fn call_replace_callback(callback: f64, args: &[f64]) -> String {
    let prev = crate::object::js_implicit_this_set(f64::from_bits(crate::value::TAG_UNDEFINED));
    let ret = crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
    crate::object::js_implicit_this_set(prev);
    let ptr = crate::value::js_get_string_pointer_unified(ret) as *const StringHeader;
    if is_valid_ptr(ptr) {
        string_as_str(ptr).to_string()
    } else {
        String::new()
    }
}

unsafe fn call_string_replace_callback(
    callback: f64,
    matched: &str,
    offset: usize,
    whole: &str,
) -> String {
    let scope = crate::gc::RuntimeHandleScope::new();
    let matched_value = js_nanbox_string(js_string_from_str(matched) as i64);
    let matched_handle = scope.root_nanbox_f64(matched_value);
    let offset_handle = scope.root_nanbox_f64(offset as f64);
    let whole_value = js_nanbox_string(js_string_from_str(whole) as i64);
    let whole_handle = scope.root_nanbox_f64(whole_value);
    let args = [
        matched_handle.get_nanbox_f64(),
        offset_handle.get_nanbox_f64(),
        whole_handle.get_nanbox_f64(),
    ];
    call_replace_callback(callback, &args)
}

/// string.replace(pattern, replacerFn) for a non-regex string pattern.
#[no_mangle]
pub extern "C" fn js_string_replace_string_fn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    callback: f64,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };

    unsafe {
        if pattern_str.is_empty() {
            let replacement = call_string_replace_callback(callback, "", 0, str_data);
            let mut result = String::with_capacity(replacement.len() + str_data.len());
            result.push_str(&replacement);
            result.push_str(str_data);
            return js_string_from_str(&result);
        }

        let Some(byte_idx) = str_data.find(pattern_str) else {
            return js_string_from_str(str_data);
        };
        let char_offset = str_data[..byte_idx].chars().count();
        let replacement =
            call_string_replace_callback(callback, pattern_str, char_offset, str_data);
        let mut result = String::with_capacity(str_data.len() + replacement.len());
        result.push_str(&str_data[..byte_idx]);
        result.push_str(&replacement);
        result.push_str(&str_data[byte_idx + pattern_str.len()..]);
        js_string_from_str(&result)
    }
}

/// string.replaceAll(pattern, replacerFn) for a non-regex string pattern.
#[no_mangle]
pub extern "C" fn js_string_replace_all_string_fn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    callback: f64,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };

    unsafe {
        if pattern_str.is_empty() {
            let mut result = String::new();
            result.push_str(&call_string_replace_callback(callback, "", 0, str_data));
            let mut offset = 0usize;
            for ch in str_data.chars() {
                result.push(ch);
                offset += 1;
                result.push_str(&call_string_replace_callback(
                    callback, "", offset, str_data,
                ));
            }
            return js_string_from_str(&result);
        }

        let mut result = String::new();
        let mut last_end = 0usize;
        for (byte_idx, matched) in str_data.match_indices(pattern_str) {
            result.push_str(&str_data[last_end..byte_idx]);
            let char_offset = str_data[..byte_idx].chars().count();
            result.push_str(&call_string_replace_callback(
                callback,
                matched,
                char_offset,
                str_data,
            ));
            last_end = byte_idx + matched.len();
        }
        if last_end == 0 {
            return js_string_from_str(str_data);
        }
        result.push_str(&str_data[last_end..]);
        js_string_from_str(&result)
    }
}
