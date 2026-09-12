//! Public String entry points preserve arbitrary JS hook return values.
use super::perex_match_search::{self as operations, Operation};
use super::*;

#[no_mangle]
pub extern "C" fn js_string_match_js(receiver: f64, pattern: f64) -> f64 {
    perex_api::finish(operations::string(Operation::Match, receiver, pattern))
}
#[no_mangle]
pub extern "C" fn js_string_search_js(receiver: f64, pattern: f64) -> f64 {
    perex_api::finish(operations::string(Operation::Search, receiver, pattern))
}
#[no_mangle]
pub extern "C" fn js_string_match_value(s: *const StringHeader, pattern: f64) -> f64 {
    js_string_match_js(crate::value::js_nanbox_string(s as i64), pattern)
}
#[no_mangle]
pub extern "C" fn js_string_search_value(s: *const StringHeader, pattern: f64) -> f64 {
    js_string_search_js(crate::value::js_nanbox_string(s as i64), pattern)
}

// Legacy Rust test conveniences. Production consumers use boxed JS results;
// a custom symbol method can return a primitive or a non-array object.
#[cfg(test)]
pub fn js_string_match(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut crate::array::ArrayHeader {
    let result = js_string_match_value(s, crate::value::js_nanbox_pointer(re as i64));
    if result.to_bits() == crate::value::TAG_NULL {
        std::ptr::null_mut()
    } else {
        crate::value::js_nanbox_get_pointer(result) as *mut crate::array::ArrayHeader
    }
}
#[cfg(test)]
pub fn js_string_search_regex(s: *const StringHeader, re: *const RegExpHeader) -> i32 {
    js_string_search_value(s, crate::value::js_nanbox_pointer(re as i64)) as i32
}
