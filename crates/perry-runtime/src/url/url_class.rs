//! `URL` class FFI surface — constructors, getters, setters, static methods.

use super::*;

use super::parse::{
    create_url_object, is_valid_absolute_url, rebuild_url_host, rebuild_url_href, resolve_url,
    throw_invalid_url, URL_FIELD_COUNT, URL_HASH, URL_HOST, URL_HOSTNAME, URL_HREF, URL_ORIGIN,
    URL_PASSWORD, URL_PATHNAME, URL_PORT, URL_PROTOCOL, URL_SEARCH, URL_SEARCH_PARAMS,
    URL_USERNAME,
};
use super::search_params::{
    create_url_search_params_object, parse_query_string, URL_SEARCH_PARAMS_OWNER,
};

fn is_ascii_hex_digit(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn should_percent_encode_userinfo_byte(b: u8) -> bool {
    b <= 0x1F
        || b > 0x7E
        || matches!(
            b,
            b' ' | b'"'
                | b'#'
                | b'/'
                | b':'
                | b';'
                | b'<'
                | b'='
                | b'>'
                | b'?'
                | b'@'
                | b'['
                | b'\\'
                | b']'
                | b'^'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
        )
}

fn percent_encode_userinfo(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%'
            && i + 2 < bytes.len()
            && is_ascii_hex_digit(bytes[i + 1])
            && is_ascii_hex_digit(bytes[i + 2])
        {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
            continue;
        }
        if should_percent_encode_userinfo_byte(b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
        i += 1;
    }
    out
}

fn should_strip_url_ascii_whitespace(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\r')
}

fn is_special_url_scheme(protocol: &str) -> bool {
    matches!(
        protocol,
        "ftp:" | "file:" | "http:" | "https:" | "ws:" | "wss:"
    )
}

fn should_percent_encode_search_byte(b: u8, special: bool) -> bool {
    b <= 0x1F
        || b >= 0x7F
        || matches!(b, b' ' | b'"' | b'#' | b'<' | b'>')
        || (special && b == b'\'')
}

fn should_percent_encode_path_byte(b: u8) -> bool {
    b <= 0x1F
        || b >= 0x7F
        || matches!(
            b,
            b' ' | b'"' | b'#' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}'
        )
}

fn should_percent_encode_fragment_byte(b: u8) -> bool {
    b <= 0x1F || b >= 0x7F || matches!(b, b' ' | b'"' | b'<' | b'>' | b'`')
}

fn percent_encode_url_component(raw: &str, should_encode: impl Fn(u8) -> bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for &b in raw.as_bytes() {
        if should_strip_url_ascii_whitespace(b) {
            continue;
        }
        if should_encode(b) {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        } else {
            out.push(b as char);
        }
    }
    out
}

fn percent_encode_search(raw: &str, special: bool) -> String {
    percent_encode_url_component(raw, |b| should_percent_encode_search_byte(b, special))
}

fn url_can_have_credentials(url: *mut ObjectHeader) -> bool {
    let host = get_string_content(crate::object::js_object_get_field_f64(url, URL_HOST));
    let protocol = get_string_content(crate::object::js_object_get_field_f64(url, URL_PROTOCOL));
    !host.is_empty() && protocol != "file:"
}

fn normalize_hostname_value(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.chars().any(|c| {
            c.is_ascii_control()
                || matches!(
                    c,
                    ' ' | '#' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
                )
        })
    {
        return None;
    }
    match idna::domain_to_ascii(raw) {
        Ok(ascii) if !ascii.is_empty() => Some(ascii),
        _ => None,
    }
}

fn percent_encode_path(raw: &str) -> String {
    percent_encode_url_component(raw, should_percent_encode_path_byte)
}

fn percent_encode_fragment(raw: &str) -> String {
    percent_encode_url_component(raw, should_percent_encode_fragment_byte)
}

/// Create a new URL from a string
/// js_url_new(url: *mut StringHeader) -> *mut ObjectHeader (URL object)
#[no_mangle]
pub extern "C" fn js_url_new(url_str: *mut crate::StringHeader) -> *mut ObjectHeader {
    let url_string = if url_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*url_str).byte_len as usize;
            let data_ptr = (url_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };
    if !is_valid_absolute_url(&url_string) {
        throw_invalid_url(&url_string);
    }
    create_url_object(&url_string)
}

/// Create a new URL from a string with a base URL
/// js_url_new_with_base(url: *mut StringHeader, base: *mut StringHeader) -> *mut ObjectHeader
#[no_mangle]
pub extern "C" fn js_url_new_with_base(
    url_str: *mut crate::StringHeader,
    base_str: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    let url_string = if url_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*url_str).byte_len as usize;
            let data_ptr = (url_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let base_string = if base_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*base_str).byte_len as usize;
            let data_ptr = (base_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    // Relative URLs require a parseable base; if the base is bogus and the
    // input isn't itself absolute, the constructor must throw.
    let url_is_absolute = is_valid_absolute_url(&url_string);
    let base_is_absolute = is_valid_absolute_url(&base_string);
    if !url_is_absolute && !base_is_absolute {
        throw_invalid_url(&url_string);
    }

    // Resolve the URL against the base
    let resolved = resolve_url(&url_string, &base_string);
    create_url_object(&resolved)
}

/// Get the href property from a URL (returns field value)
#[no_mangle]
pub extern "C" fn js_url_get_href(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_HREF)
}

/// Issue #650: `url.pathname = value` setter. Per WHATWG, the setter
/// normalizes a leading `/` for hierarchical schemes — but Node's actual
/// runtime behavior on `u.pathname = "changed"` produces pathname
/// `"/changed"` ONLY when the URL has an authority component, leaving
/// opaque (non-hierarchical) URLs alone. We follow Node: prepend `/`
/// when the URL has a non-empty `host`.
#[no_mangle]
pub extern "C" fn js_url_set_pathname(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let raw = if value.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*value).byte_len as usize;
            let data_ptr = (value as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };
    unsafe {
        let host = get_string_content(crate::object::js_object_get_field_f64(url, URL_HOST));
        let normalized = if !host.is_empty() && !raw.starts_with('/') {
            format!("/{}", raw)
        } else {
            raw
        };
        let encoded = percent_encode_path(&normalized);
        js_object_set_field_f64(url, URL_PATHNAME, create_string_f64(&encoded));
        rebuild_url_href(url);
    }
}

/// Issue #650: `url.search = value` setter. Stores the leading `?`
/// when the value is non-empty (matching WHATWG: empty search clears
/// the query string entirely). Also re-parses the new query into the
/// stored URLSearchParams object so `url.searchParams.get(...)` reflects
/// the new entries.
#[no_mangle]
pub extern "C" fn js_url_set_search(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let raw = if value.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*value).byte_len as usize;
            let data_ptr = (value as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };
    let normalized = if raw.is_empty() {
        String::new()
    } else if raw.starts_with('?') {
        raw
    } else {
        format!("?{}", raw)
    };
    unsafe {
        let protocol =
            get_string_content(crate::object::js_object_get_field_f64(url, URL_PROTOCOL));
        let encoded = percent_encode_search(&normalized, is_special_url_scheme(&protocol));
        js_object_set_field_f64(url, URL_SEARCH, create_string_f64(&encoded));
        // Refresh the searchParams object's entries to match the new query.
        let params_entries = parse_query_string(&encoded);
        let new_params = create_url_search_params_object(params_entries);
        js_object_set_field_f64(
            new_params,
            URL_SEARCH_PARAMS_OWNER,
            crate::value::js_nanbox_pointer(url as i64),
        );
        let params_f64 = crate::value::js_nanbox_pointer(new_params as i64);
        js_object_set_field_f64(url, URL_SEARCH_PARAMS, params_f64);
        rebuild_url_href(url);
    }
}

/// `url.protocol = value` — strip trailing `:`-free input to match the
/// canonical `"scheme:"` form, write the field, then re-derive `host`
/// (default-port stripping depends on protocol) and `href`.
#[no_mangle]
pub extern "C" fn js_url_set_protocol(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let mut raw = string_header_to_string(value);
    if !raw.ends_with(':') {
        raw.push(':');
    }
    unsafe {
        js_object_set_field_f64(url, URL_PROTOCOL, create_string_f64(&raw));
        rebuild_url_host(url);
        rebuild_url_href(url);
    }
}

/// `url.hostname = value` — update hostname and reconstruct host.
#[no_mangle]
pub extern "C" fn js_url_set_hostname(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let Some(raw) = normalize_hostname_value(&string_header_to_string(value)) else {
        return;
    };
    unsafe {
        js_object_set_field_f64(url, URL_HOSTNAME, create_string_f64(&raw));
        rebuild_url_host(url);
        rebuild_url_href(url);
    }
}

/// `url.port = value` — store as a string (Node normalizes to digits-only).
/// Empty input clears the port. Reconstructs host afterwards.
#[no_mangle]
pub extern "C" fn js_url_set_port(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let raw = string_header_to_string(value);
    // Per WHATWG: parse leading digit run; anything else discards the new port.
    let parsed: String = raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    unsafe {
        js_object_set_field_f64(url, URL_PORT, create_string_f64(&parsed));
        rebuild_url_host(url);
        rebuild_url_href(url);
    }
}

/// `url.username = value` — update userinfo and rebuild href.
#[no_mangle]
pub extern "C" fn js_url_set_username(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() || !url_can_have_credentials(url) {
        return;
    }
    let raw = percent_encode_userinfo(&string_header_to_string(value));
    unsafe {
        js_object_set_field_f64(url, URL_USERNAME, create_string_f64(&raw));
        rebuild_url_href(url);
    }
}

/// `url.password = value` — update userinfo and rebuild href.
#[no_mangle]
pub extern "C" fn js_url_set_password(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() || !url_can_have_credentials(url) {
        return;
    }
    let raw = percent_encode_userinfo(&string_header_to_string(value));
    unsafe {
        js_object_set_field_f64(url, URL_PASSWORD, create_string_f64(&raw));
        rebuild_url_href(url);
    }
}

/// `url.href = value` — parse a full replacement URL or throw.
#[no_mangle]
pub extern "C" fn js_url_set_href(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let raw = string_header_to_string(value);
    if !is_valid_absolute_url(&raw) {
        throw_invalid_url(&raw);
    }
    let parsed = create_url_object(&raw);
    for field in 0..URL_FIELD_COUNT {
        let v = crate::object::js_object_get_field_f64(parsed, field);
        js_object_set_field_f64(url, field, v);
    }
    let params_f64 = crate::object::js_object_get_field_f64(url, URL_SEARCH_PARAMS);
    if let Some(params) = object_from_f64(params_f64) {
        js_object_set_field_f64(
            params,
            URL_SEARCH_PARAMS_OWNER,
            crate::value::js_nanbox_pointer(url as i64),
        );
    }
}

unsafe fn is_gc_object_header(obj: *mut ObjectHeader) -> bool {
    if obj.is_null() || !crate::object::is_valid_obj_ptr(obj as *const u8) {
        return false;
    }
    let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
}

pub(crate) fn is_url_object_shape(url: *mut ObjectHeader) -> bool {
    if url.is_null() {
        return false;
    }
    unsafe {
        if !is_gc_object_header(url) || (*url).class_id != 0 || (*url).field_count < URL_FIELD_COUNT
        {
            return false;
        }
        let href = get_string_content(crate::object::js_object_get_field_f64(url, URL_HREF));
        if !is_valid_absolute_url(&href) {
            return false;
        }
        let params_f64 = crate::object::js_object_get_field_f64(url, URL_SEARCH_PARAMS);
        let Some(params) = object_from_f64(params_f64) else {
            return false;
        };
        if !is_gc_object_header(params) {
            return false;
        }
        let owner_f64 = crate::object::js_object_get_field_f64(params, URL_SEARCH_PARAMS_OWNER);
        object_from_f64(owner_f64).is_some_and(|owner| owner == url)
    }
}

/// Issue #650: `url.hash = value` setter. Stores the leading `#` when
/// the value is non-empty; clears entirely when empty (matches WHATWG).
#[no_mangle]
pub extern "C" fn js_url_set_hash(url: *mut ObjectHeader, value: *mut crate::StringHeader) {
    if url.is_null() {
        return;
    }
    let raw = if value.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*value).byte_len as usize;
            let data_ptr = (value as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };
    let normalized = if raw.is_empty() {
        String::new()
    } else if raw.starts_with('#') {
        raw
    } else {
        format!("#{}", raw)
    };
    let encoded = percent_encode_fragment(&normalized);
    unsafe {
        js_object_set_field_f64(url, URL_HASH, create_string_f64(&encoded));
        rebuild_url_href(url);
    }
}

/// Issue #650: `URL.canParse(input)` static method (Node 18+).
/// Returns 1 if `input` parses as a valid URL, 0 otherwise. Treats absent
/// scheme + non-`file:` relative inputs as failures, matching Node's
/// stricter validation than `parse_url`'s liberal accept-anything path
/// (which the constructor still relies on for backwards compat with
/// pre-existing test fixtures).
#[no_mangle]
pub extern "C" fn js_url_can_parse(input: *mut crate::StringHeader) -> i32 {
    if input.is_null() {
        return 0;
    }
    let s = unsafe {
        let len = (*input).byte_len as usize;
        let data_ptr = (input as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    };
    if is_valid_absolute_url(&s) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_url_can_parse_with_base(
    input: *mut crate::StringHeader,
    base: *mut crate::StringHeader,
) -> i32 {
    let input_s = string_from_header(input);
    let base_s = string_from_header(base);
    if is_valid_absolute_url(&input_s) {
        return 1;
    }
    if !is_valid_absolute_url(&base_s) || input_s.trim().is_empty() {
        return 0;
    }
    let resolved = resolve_url(&input_s, &base_s);
    if is_valid_absolute_url(&resolved) {
        1
    } else {
        0
    }
}

/// Issue #650: `URL.parse(input)` static method (Node 22+) — non-throwing
/// counterpart to `new URL(input)`. Returns the parsed URL object on
/// success or null on failure. Wraps `js_url_new` with the same liberal
/// validation as `URL.canParse` so the two stay in lockstep.
#[no_mangle]
pub extern "C" fn js_url_parse(input: *mut crate::StringHeader) -> *mut ObjectHeader {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let s = unsafe {
        let len = (*input).byte_len as usize;
        let data_ptr = (input as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    };
    if !is_valid_absolute_url(&s) {
        return std::ptr::null_mut();
    }
    create_url_object(&s)
}

#[no_mangle]
pub extern "C" fn js_url_parse_with_base(
    input: *mut crate::StringHeader,
    base: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    let input_s = string_from_header(input);
    let base_s = string_from_header(base);
    if is_valid_absolute_url(&input_s) {
        return create_url_object(&input_s);
    }
    if !is_valid_absolute_url(&base_s) || input_s.trim().is_empty() {
        return std::ptr::null_mut();
    }
    let resolved = resolve_url(&input_s, &base_s);
    if !is_valid_absolute_url(&resolved) {
        return std::ptr::null_mut();
    }
    create_url_object(&resolved)
}

/// Get the pathname property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_pathname(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_PATHNAME)
}

/// Get the protocol property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_protocol(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_PROTOCOL)
}

/// Get the host property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_host(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_HOST)
}

/// Get the hostname property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_hostname(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_HOSTNAME)
}

/// Get the port property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_port(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_PORT)
}

/// Get the search property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_search(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_SEARCH)
}

/// Get the hash property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_hash(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_HASH)
}

/// Get the origin property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_origin(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_ORIGIN)
}

/// Get the searchParams property from a URL
#[no_mangle]
pub extern "C" fn js_url_get_search_params(url: *mut ObjectHeader) -> f64 {
    if url.is_null() {
        return create_string_f64("");
    }
    crate::object::js_object_get_field_f64(url, URL_SEARCH_PARAMS)
}
