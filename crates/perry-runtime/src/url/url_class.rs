//! `URL` class FFI surface: constructors, getters, setters, static methods.
//!
//! A URL object holds its href and the component boundaries inside it
//! (`super::href`): constructing parses once, a getter slices href, a setter
//! re-parses href and applies the WHATWG setter.

use super::href::{self, Component, Setter};
use super::parse::throw_invalid_url;
use super::*;

fn coerce_url_setter_value(value: f64) -> String {
    string_from_header(js_url_coerce_string(value))
}

/// The text of a constructor argument (USVString: invalid sequences read as
/// U+FFFD). A null pointer reads as "".
fn arg_text(s: *mut crate::StringHeader) -> std::borrow::Cow<'static, str> {
    if s.is_null() {
        return std::borrow::Cow::Borrowed("");
    }
    unsafe {
        let len = (*s).byte_len as usize;
        let bytes = std::slice::from_raw_parts(crate::string::string_data(s), len);
        match std::str::from_utf8(bytes) {
            // The argument string outlives the parse, which allocates nothing
            // on the JS heap.
            Ok(text) => std::borrow::Cow::Borrowed(&*(text as *const str)),
            Err(_) => std::borrow::Cow::Owned(String::from_utf8_lossy(bytes).into_owned()),
        }
    }
}

/// `new URL(input)`.
#[no_mangle]
pub extern "C" fn js_url_new(url_str: *mut crate::StringHeader) -> *mut ObjectHeader {
    let input = arg_text(url_str);
    match href::parse(&input, None) {
        Some(parsed) => href::new_url_object(&parsed),
        None => throw_invalid_url(&input, None),
    }
}

/// `new URL(input, base)`: the input is parsed once, against the parsed base.
#[no_mangle]
pub extern "C" fn js_url_new_with_base(
    url_str: *mut crate::StringHeader,
    base_str: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    let input = arg_text(url_str);
    let base = arg_text(base_str);
    match href::parse(&input, Some(&base)) {
        Some(parsed) => href::new_url_object(&parsed),
        None => throw_invalid_url(&input, Some(&base)),
    }
}

/// `URL.canParse(input)`.
#[no_mangle]
pub extern "C" fn js_url_can_parse(input: *mut crate::StringHeader) -> i32 {
    i32::from(href::parse(&arg_text(input), None).is_some())
}

/// `URL.canParse(input, base)`.
#[no_mangle]
pub extern "C" fn js_url_can_parse_with_base(
    input: *mut crate::StringHeader,
    base: *mut crate::StringHeader,
) -> i32 {
    i32::from(href::parse(&arg_text(input), Some(&arg_text(base))).is_some())
}

/// `URL.parse(input)`: the URL, or null when it does not parse.
#[no_mangle]
pub extern "C" fn js_url_parse(input: *mut crate::StringHeader) -> *mut ObjectHeader {
    href::parse(&arg_text(input), None).map_or(std::ptr::null_mut(), |p| href::new_url_object(&p))
}

/// `URL.parse(input, base)`.
#[no_mangle]
pub extern "C" fn js_url_parse_with_base(
    input: *mut crate::StringHeader,
    base: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    href::parse(&arg_text(input), Some(&arg_text(base)))
        .map_or(std::ptr::null_mut(), |p| href::new_url_object(&p))
}

macro_rules! url_component_getter {
    ($($fn_name:ident => $component:ident;)*) => {$(
        #[no_mangle]
        pub extern "C" fn $fn_name(url: *mut ObjectHeader) -> f64 {
            if url.is_null() {
                return create_string_f64("");
            }
            href::component_value(url, Component::$component)
        }
    )*};
}

url_component_getter! {
    js_url_get_href => Href;
    js_url_get_protocol => Protocol;
    js_url_get_username => Username;
    js_url_get_password => Password;
    js_url_get_host => Host;
    js_url_get_hostname => Hostname;
    js_url_get_port => Port;
    js_url_get_pathname => Pathname;
    js_url_get_search => Search;
    js_url_get_hash => Hash;
}

/// `url.origin`.
#[no_mangle]
pub extern "C" fn js_url_get_origin(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    href::origin_value(url)
}

/// `url.searchParams`: made on the first read, the same object after.
#[no_mangle]
pub extern "C" fn js_url_get_search_params(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    super::search_params::url_search_params(url)
}

macro_rules! url_setter {
    ($($fn_name:ident => $setter:ident;)*) => {$(
        /// WHATWG setter: a rejected value leaves the URL unchanged.
        #[no_mangle]
        pub extern "C" fn $fn_name(url: *mut ObjectHeader, value: f64) {
            if url.is_null() {
                return;
            }
            let value = coerce_url_setter_value(value);
            let _ = href::set(url, Setter::$setter(&value));
        }
    )*};
}

url_setter! {
    js_url_set_protocol => Protocol;
    js_url_set_username => Username;
    js_url_set_password => Password;
    js_url_set_host => Host;
    js_url_set_hostname => Hostname;
    js_url_set_port => Port;
    js_url_set_pathname => Pathname;
    js_url_set_search => Search;
    js_url_set_hash => Hash;
}

/// `url.href = value`: parse a full replacement URL or throw.
#[no_mangle]
pub extern "C" fn js_url_set_href(url: *mut ObjectHeader, value: f64) {
    if url.is_null() {
        return;
    }
    let value = coerce_url_setter_value(value);
    if href::set(url, Setter::Href(&value)).is_err() {
        throw_invalid_url(&value, None);
    }
}

/// #3880: If `value` (a NaN-boxed JSValue) is a WHATWG `URL` instance,
/// return its `href` string (NaN-boxed); otherwise return `undefined`.
///
/// `perry-ext-http` uses this to route a `URL`-*object* first argument of
/// `http.request` / `http.get` to the string-URL path. Without it, the
/// overload parser treats the URL object as the options bag and
/// `parse_options_object` JSON-stringifies it — which throws
/// `Converting circular structure to JSON` on the URL's
/// `searchParams` ↔ owner back-reference.
#[no_mangle]
pub extern "C" fn js_url_href_if_url(value: f64) -> f64 {
    // Undefined NaN-box (matches the canonical TAG_UNDEFINED).
    const UNDEFINED: f64 = f64::from_bits(0x7FFC_0000_0000_0001);
    match object_from_f64(value) {
        Some(obj) if is_url_object_shape(obj) => {
            super::href::href_value(obj)
        }
        _ => UNDEFINED,
    }
}

pub(crate) use crate::object::url_layout::is_url_object_shape;
