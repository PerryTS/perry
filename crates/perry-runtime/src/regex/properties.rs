//! Observable RegExp data properties and stringification.

use super::escape::escape_regexp_source;
use super::RegExpHeader;
use super::{is_valid_ptr, is_valid_regex_ptr, js_string_from_str, string_as_bytes, string_as_str};
use crate::string::StringHeader;

/// Get regex.source — returns the pattern string.
#[no_mangle]
pub extern "C" fn js_regexp_get_source(re: *const RegExpHeader) -> *mut StringHeader {
    if !is_valid_regex_ptr(re) {
        return js_string_from_str("(?:)");
    }
    unsafe {
        if is_valid_ptr((*re).pattern_ptr) {
            let escaped = escape_regexp_source(string_as_bytes((*re).pattern_ptr));
            crate::string::js_string_from_wtf8_bytes(escaped.as_ptr(), escaped.len() as u32)
        } else {
            js_string_from_str("(?:)")
        }
    }
}

/// `RegExp.prototype.source` for the prototype object itself (no
/// `[[OriginalSource]]`) returns the canonical empty source `"(?:)"`.
#[no_mangle]
pub extern "C" fn js_regexp_empty_source() -> *mut StringHeader {
    js_string_from_str("(?:)")
}

/// Get regex.flags — returns the flags string.
#[no_mangle]
pub extern "C" fn js_regexp_get_flags(re: *const RegExpHeader) -> *mut StringHeader {
    if !is_valid_regex_ptr(re) {
        return js_string_from_str("");
    }
    unsafe {
        if is_valid_ptr((*re).flags_ptr) {
            let flags_str = string_as_str((*re).flags_ptr);
            js_string_from_str(flags_str)
        } else {
            js_string_from_str("")
        }
    }
}

/// `RegExp.prototype.toString()` — `/source/flags`. Used by both the
/// `regex.toString()` method dispatch and ToString coercion (`String(re)`,
/// template literals). Node never produces `"[object Object]"` for a RegExp.
#[no_mangle]
pub extern "C" fn js_regexp_to_string(re: *const RegExpHeader) -> *mut StringHeader {
    let src = js_regexp_get_source(re);
    let flg = js_regexp_get_flags(re);
    let out = format!("/{}/{}", string_as_str(src), string_as_str(flg));
    js_string_from_str(&out)
}

/// Get regex.lastIndex — returns the stored value (NaN-boxed JSValue bits as
/// f64). Usually a number, but `re.lastIndex = obj` round-trips the object.
#[no_mangle]
pub extern "C" fn js_regexp_get_last_index(re: *const RegExpHeader) -> f64 {
    if !is_valid_regex_ptr(re) {
        return 0.0;
    }
    unsafe { f64::from_bits((*re).last_index) }
}

/// Set regex.lastIndex — stores the value verbatim (no coercion on write, per
/// spec `Set(R, "lastIndex", v)`).
#[no_mangle]
pub extern "C" fn js_regexp_set_last_index(re: *mut RegExpHeader, value: f64) {
    if !is_valid_regex_ptr(re) {
        return;
    }
    unsafe {
        (*re).last_index = value.to_bits();
    }
}
