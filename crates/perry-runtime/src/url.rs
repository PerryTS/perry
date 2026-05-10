//! URL operations runtime support
//!
//! Provides JavaScript URL functionality for parsing and working with URLs.
//! URLs are represented as regular JavaScript objects with string fields.

use crate::array::{js_array_alloc, js_array_push_f64};
use crate::object::{js_object_set_field_f64, js_object_set_keys};
use crate::{js_object_alloc, js_string_from_bytes, ArrayHeader, ObjectHeader, StringHeader};

/// Create a string from a Rust str (returns a StringHeader pointer as f64)
/// Uses proper NaN-boxing with STRING_TAG so is_string() will return true
fn create_string_f64(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    // Use js_nanbox_string to properly tag the string pointer
    crate::value::js_nanbox_string(ptr as i64)
}

/// Get string content from a NaN-boxed StringHeader pointer (passed as f64)
fn get_string_content(ptr_f64: f64) -> String {
    // Extract the pointer from NaN-boxed value using proper unboxing
    let ptr_i64 = crate::value::js_nanbox_get_string_pointer(ptr_f64);
    let ptr: *mut StringHeader = ptr_i64 as *mut StringHeader;
    if ptr.is_null() || ptr_i64 == 0 {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    }
}

/// Simple URL parser
/// Returns (protocol, host, hostname, port, pathname, search, hash)
fn parse_url(url_str: &str) -> (String, String, String, String, String, String, String) {
    let mut protocol = String::new();
    let mut host = String::new();
    let mut hostname = String::new();
    let mut port = String::new();
    let mut pathname = String::from("/");
    let mut search = String::new();
    let mut hash = String::new();

    let mut remaining = url_str;

    // Extract hash (fragment)
    if let Some(hash_idx) = remaining.find('#') {
        hash = remaining[hash_idx..].to_string();
        remaining = &remaining[..hash_idx];
    }

    // Extract search (query string)
    if let Some(query_idx) = remaining.find('?') {
        search = remaining[query_idx..].to_string();
        remaining = &remaining[..query_idx];
    }

    // Extract protocol
    if let Some(proto_idx) = remaining.find("://") {
        protocol = format!("{}:", &remaining[..proto_idx]);
        remaining = &remaining[proto_idx + 3..];
    } else if remaining.starts_with("file:") {
        protocol = "file:".to_string();
        remaining = remaining.strip_prefix("file:").unwrap_or(remaining);
        // Handle file:/// paths
        if remaining.starts_with("//") {
            remaining = remaining.strip_prefix("//").unwrap_or(remaining);
        }
    }

    // For file: URLs, the rest is the pathname
    if protocol == "file:" {
        pathname = if remaining.is_empty() {
            "/".to_string()
        } else if remaining.starts_with('/') {
            remaining.to_string()
        } else {
            format!("/{}", remaining)
        };
        host = String::new();
        hostname = String::new();
    } else {
        // Extract host and pathname
        if let Some(path_idx) = remaining.find('/') {
            host = remaining[..path_idx].to_string();
            pathname = remaining[path_idx..].to_string();
        } else {
            host = remaining.to_string();
            pathname = "/".to_string();
        }

        // Extract hostname and port from host
        if let Some(port_idx) = host.rfind(':') {
            // Check if this is actually a port (not part of IPv6)
            let potential_port = &host[port_idx + 1..];
            if potential_port.chars().all(|c| c.is_ascii_digit()) && !potential_port.is_empty() {
                hostname = host[..port_idx].to_string();
                port = potential_port.to_string();
            } else {
                hostname = host.clone();
            }
        } else {
            hostname = host.clone();
        }
    }

    (protocol, host, hostname, port, pathname, search, hash)
}

/// Resolve a relative URL against a base URL
fn resolve_url(url_str: &str, base_str: &str) -> String {
    // If url_str is already absolute, return it
    if url_str.contains("://") || url_str.starts_with("file:") {
        return url_str.to_string();
    }

    let (base_protocol, base_host, _, _, base_pathname, _, _) = parse_url(base_str);

    if url_str.starts_with("//") {
        // Protocol-relative URL
        return format!("{}{}", base_protocol, url_str);
    }

    if url_str.starts_with('/') {
        // Absolute path
        if base_protocol == "file:" {
            return format!("{}{}", base_protocol, url_str);
        }
        return format!("{}//{}{}", base_protocol, base_host, url_str);
    }

    // Relative path - resolve against base pathname
    let base_dir = if base_pathname.ends_with('/') {
        base_pathname.clone()
    } else {
        // Get directory part of base pathname
        match base_pathname.rfind('/') {
            Some(idx) => base_pathname[..=idx].to_string(),
            None => "/".to_string(),
        }
    };

    // Handle . and .. in relative path
    let mut segments: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();

    for part in url_str.split('/') {
        match part {
            "." | "" => continue,
            ".." => {
                segments.pop();
            }
            _ => segments.push(part),
        }
    }

    let resolved_path = format!("/{}", segments.join("/"));

    if base_protocol == "file:" {
        format!("{}{}", base_protocol, resolved_path)
    } else {
        format!("{}//{}{}", base_protocol, base_host, resolved_path)
    }
}

/// Field indices for URL object
const URL_HREF: u32 = 0;
const URL_PROTOCOL: u32 = 1;
const URL_HOST: u32 = 2;
const URL_HOSTNAME: u32 = 3;
const URL_PORT: u32 = 4;
const URL_PATHNAME: u32 = 5;
const URL_SEARCH: u32 = 6;
const URL_HASH: u32 = 7;
const URL_ORIGIN: u32 = 8;
const URL_SEARCH_PARAMS: u32 = 9;
// Issue #650: username + password fields. Pre-fix `u.username` /
// `u.password` returned undefined for `https://user:pass@example.com`
// URLs because parse_url's authority-component decomposition didn't split
// userinfo out of `host`. Post-processing in create_url_object extracts
// userinfo before the object is built.
const URL_USERNAME: u32 = 10;
const URL_PASSWORD: u32 = 11;
const URL_FIELD_COUNT: u32 = 12;

/// Create a URL object from a string
fn create_url_object(url_string: &str) -> *mut ObjectHeader {
    let (protocol, mut host, mut hostname, port, pathname, search, hash) =
        parse_url(url_string);

    // Issue #650: extract userinfo (`user:pass@`) from `host`. parse_url
    // leaves it as a prefix on host/hostname because the WHATWG authority
    // component decomposition (userinfo@host:port) wasn't implemented;
    // pre-fix `host`/`hostname`/`origin` all carried the userinfo prefix
    // and `username`/`password` came back undefined. This post-processing
    // pulls the userinfo back out before the object is built. Done here
    // (not in parse_url itself) to keep parse_url's existing 7-tuple
    // signature stable for resolve_url and the unit tests — only the
    // create_url_object path needs the split.
    let mut username = String::new();
    let mut password = String::new();
    if let Some(at_idx) = host.rfind('@') {
        let userinfo = host[..at_idx].to_string();
        if let Some(colon_idx) = userinfo.find(':') {
            username = userinfo[..colon_idx].to_string();
            password = userinfo[colon_idx + 1..].to_string();
        } else {
            username = userinfo;
        }
        let rest = host[at_idx + 1..].to_string();
        host = rest.clone();
        // Re-derive hostname from the cleaned host (drop port if present).
        hostname = if let Some(port_idx) = rest.rfind(':') {
            let p = &rest[port_idx + 1..];
            if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() {
                rest[..port_idx].to_string()
            } else {
                rest
            }
        } else {
            rest
        };
    }

    // Construct the full href (now includes userinfo when present, per
    // WHATWG: `scheme://[user[:pass]@]host[:port][/path][?query][#frag]`).
    let userinfo_prefix = if !username.is_empty() || !password.is_empty() {
        if password.is_empty() {
            format!("{}@", username)
        } else {
            format!("{}:{}@", username, password)
        }
    } else {
        String::new()
    };
    let href = if protocol == "file:" {
        format!("{}{}{}{}", protocol, pathname, search, hash)
    } else if host.is_empty() {
        format!("{}{}{}", pathname, search, hash)
    } else {
        format!(
            "{}//{}{}{}{}{}",
            protocol, userinfo_prefix, host, pathname, search, hash
        )
    };

    // Calculate origin (intentionally excludes userinfo per WHATWG spec —
    // origin is the "registered domain" identity, not the credentials).
    let origin = if protocol == "file:" {
        "null".to_string() // file: URLs have "null" origin
    } else if host.is_empty() {
        "null".to_string()
    } else {
        format!("{}//{}", protocol, host)
    };

    unsafe {
        // Allocate object with URL_FIELD_COUNT fields
        // Using class_id 0 for now (generic object)
        let obj = js_object_alloc(0, URL_FIELD_COUNT);

        // Create the keys array with property names (order must match field indices)
        let mut keys = js_array_alloc(URL_FIELD_COUNT);
        keys = js_array_push_f64(keys, create_string_f64("href")); // 0
        keys = js_array_push_f64(keys, create_string_f64("protocol")); // 1
        keys = js_array_push_f64(keys, create_string_f64("host")); // 2
        keys = js_array_push_f64(keys, create_string_f64("hostname")); // 3
        keys = js_array_push_f64(keys, create_string_f64("port")); // 4
        keys = js_array_push_f64(keys, create_string_f64("pathname")); // 5
        keys = js_array_push_f64(keys, create_string_f64("search")); // 6
        keys = js_array_push_f64(keys, create_string_f64("hash")); // 7
        keys = js_array_push_f64(keys, create_string_f64("origin")); // 8
        keys = js_array_push_f64(keys, create_string_f64("searchParams")); // 9
        keys = js_array_push_f64(keys, create_string_f64("username")); // 10
        keys = js_array_push_f64(keys, create_string_f64("password")); // 11
        js_object_set_keys(obj, keys);

        // Set all the URL properties
        js_object_set_field_f64(obj, URL_HREF, create_string_f64(&href));
        js_object_set_field_f64(obj, URL_PROTOCOL, create_string_f64(&protocol));
        js_object_set_field_f64(obj, URL_HOST, create_string_f64(&host));
        js_object_set_field_f64(obj, URL_HOSTNAME, create_string_f64(&hostname));
        js_object_set_field_f64(obj, URL_PORT, create_string_f64(&port));
        js_object_set_field_f64(obj, URL_PATHNAME, create_string_f64(&pathname));
        js_object_set_field_f64(obj, URL_SEARCH, create_string_f64(&search));
        js_object_set_field_f64(obj, URL_HASH, create_string_f64(&hash));
        js_object_set_field_f64(obj, URL_ORIGIN, create_string_f64(&origin));
        js_object_set_field_f64(obj, URL_USERNAME, create_string_f64(&username));
        js_object_set_field_f64(obj, URL_PASSWORD, create_string_f64(&password));
        // Build a real URLSearchParams object from the search string (parsed
        // lazily below). Storing a string here would break `url.searchParams.get()`
        // because the URLSearchParams method runtime functions interpret the
        // receiver as `*mut ObjectHeader` and would deref a StringHeader
        // instead — see issue #111.
        let params_entries = parse_query_string(&search);
        let params_obj = create_url_search_params_object(params_entries);
        let params_f64 = crate::value::js_nanbox_pointer(params_obj as i64);
        js_object_set_field_f64(obj, URL_SEARCH_PARAMS, params_f64);

        obj
    }
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

/// Validate that `s` looks like a parseable absolute URL — has a scheme
/// followed by `:` and either `//` (for hierarchical schemes) or non-empty
/// scheme-specific data. Used by `URL.canParse` / `URL.parse` to mirror
/// the validation Node's WHATWG parser performs before constructing.
fn is_valid_absolute_url(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    // Scheme: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
    let mut chars = s.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let mut scheme_end = 1;
    for c in chars {
        if c == ':' {
            break;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
        scheme_end += 1;
        if scheme_end > 64 {
            // Practical cap; real schemes are short.
            return false;
        }
    }
    if scheme_end >= s.len() || s.as_bytes()[scheme_end] != b':' {
        return false;
    }
    // For hierarchical schemes (http, https, ws, wss, ftp, file) WHATWG
    // requires `//` before the authority. For others (mailto:, data:, etc.)
    // Node accepts anything non-empty after `:`. Match that for ergonomics.
    let after_scheme = &s[scheme_end + 1..];
    let scheme = &s[..scheme_end];
    let needs_authority =
        matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp" | "file");
    if needs_authority {
        if !after_scheme.starts_with("//") {
            return false;
        }
        // file:// is allowed to have an empty host, every other scheme needs one.
        if scheme != "file" && after_scheme.len() <= 2 {
            return false;
        }
    } else if after_scheme.is_empty() {
        return false;
    }
    true
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

// ============================================================================
// URLSearchParams implementation
// ============================================================================

/// Field indices for URLSearchParams object
const URL_SEARCH_PARAMS_ENTRIES: u32 = 0; // Array of [key, value] pairs
const URL_SEARCH_PARAMS_FIELD_COUNT: u32 = 1;

/// Parse a query string into key-value pairs
/// Handles formats like "?foo=bar&baz=qux" or "foo=bar&baz=qux"
fn parse_query_string(query: &str) -> Vec<(String, String)> {
    let query = query.strip_prefix('?').unwrap_or(query);
    if query.is_empty() {
        return Vec::new();
    }

    query
        .split('&')
        .filter_map(|pair| {
            if pair.is_empty() {
                return None;
            }
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            // URL decode the key and value
            Some((url_decode(key), url_decode(value)))
        })
        .collect()
}

/// Simple URL decoding (handles %XX sequences and + as space)
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '+' => result.push(' '),
            '%' => {
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                        continue;
                    }
                }
                // Invalid escape, keep as-is
                result.push('%');
                result.push_str(&hex);
            }
            _ => result.push(c),
        }
    }
    result
}

/// URL encode a string
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                result.push(c);
            }
            ' ' => result.push('+'),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    result
}

/// Create a URLSearchParams object from entries
fn create_url_search_params_object(entries: Vec<(String, String)>) -> *mut ObjectHeader {
    unsafe {
        let obj = js_object_alloc(0, URL_SEARCH_PARAMS_FIELD_COUNT);

        // Create keys array
        let mut keys = js_array_alloc(URL_SEARCH_PARAMS_FIELD_COUNT);
        keys = js_array_push_f64(keys, create_string_f64("_entries"));
        js_object_set_keys(obj, keys);

        // Create entries array - each entry is a 2-element array [key, value]
        let mut entries_array = js_array_alloc(entries.len() as u32);
        for (key, value) in entries {
            let mut pair = js_array_alloc(2);
            pair = js_array_push_f64(pair, create_string_f64(&key));
            pair = js_array_push_f64(pair, create_string_f64(&value));
            let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
            entries_array = js_array_push_f64(entries_array, pair_f64);
        }

        let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
        js_object_set_field_f64(obj, URL_SEARCH_PARAMS_ENTRIES, entries_f64);

        obj
    }
}

/// Get entries from a URLSearchParams object
fn get_url_search_params_entries(params: *mut ObjectHeader) -> Vec<(String, String)> {
    if params.is_null() {
        return Vec::new();
    }

    let entries_f64 = crate::object::js_object_get_field_f64(params, URL_SEARCH_PARAMS_ENTRIES);
    let entries_ptr: *mut ArrayHeader =
        unsafe { f64::to_bits(entries_f64).cast_signed() as *mut ArrayHeader };

    if entries_ptr.is_null() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let len = unsafe { (*entries_ptr).length } as usize;

    for i in 0..len {
        let pair_f64 = crate::array::js_array_get_f64(entries_ptr, i as u32);
        let pair_ptr: *mut ArrayHeader =
            unsafe { f64::to_bits(pair_f64).cast_signed() as *mut ArrayHeader };

        if !pair_ptr.is_null() {
            let key_f64 = crate::array::js_array_get_f64(pair_ptr, 0);
            let value_f64 = crate::array::js_array_get_f64(pair_ptr, 1);

            let key = get_string_content(key_f64);
            let value = get_string_content(value_f64);
            result.push((key, value));
        }
    }

    result
}

/// Create a new URLSearchParams from a string
/// js_url_search_params_new(init: *mut StringHeader) -> *mut ObjectHeader
#[no_mangle]
pub extern "C" fn js_url_search_params_new(
    init_str: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    let init_string = if init_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*init_str).byte_len as usize;
            let data_ptr = (init_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let entries = parse_query_string(&init_string);
    create_url_search_params_object(entries)
}

/// Create an empty URLSearchParams
/// js_url_search_params_new_empty() -> *mut ObjectHeader
#[no_mangle]
pub extern "C" fn js_url_search_params_new_empty() -> *mut ObjectHeader {
    create_url_search_params_object(Vec::new())
}

/// Create a URLSearchParams from any NaN-boxed init value.
///
/// Spec init shapes (`new URLSearchParams(init)`):
/// - `undefined` / `null`        → empty
/// - `string` (with or without `?`)→ parse as query string
/// - record `{ k: v, ... }`      → use property names + stringified values
/// - another `URLSearchParams`   → copy entries
/// - array of `[k, v]` pairs     → use as-is (spec-conformant; rarely used)
///
/// Pre-fix, codegen routed every init through `js_url_search_params_new`
/// (which only handles strings) — object inits got `js_get_string_pointer_unified`'d
/// into an interpret-pointer-as-string read of garbage bytes (typed-local
/// repro printed `"%00="`). Refs #575.
#[no_mangle]
pub extern "C" fn js_url_search_params_new_any(init: f64) -> *mut ObjectHeader {
    let bits = init.to_bits();
    let jsval = crate::value::JSValue::from_bits(bits);

    if jsval.is_undefined() || jsval.is_null() {
        return create_url_search_params_object(Vec::new());
    }

    // String — common path. Includes both STRING_TAG and SHORT_STRING (SSO).
    if jsval.is_string() || jsval.is_short_string() {
        let s = get_string_content(init);
        return create_url_search_params_object(parse_query_string(&s));
    }

    if jsval.is_pointer() {
        let ptr_i64 = crate::value::js_nanbox_get_pointer(init);
        if ptr_i64 == 0 {
            return create_url_search_params_object(Vec::new());
        }
        let raw_ptr = ptr_i64 as *const u8;

        // Issue #650 sub-issue: iterable form `new URLSearchParams([['a','1'], ['b','2']])`.
        // The init is an Array (GC_TYPE_ARRAY), each element a 2-element
        // pair array. Pre-fix this fell through to `read_record_entries`
        // which read the array's numeric "keys" (`"0"`, `"1"`) as if they
        // were string keys and stringified the inner pair-array values
        // via `[object Object]` — and on some shapes silently exited
        // mid-construction. Detect via the GC header's obj_type tag and
        // walk pair-by-pair.
        unsafe {
            if !raw_ptr.is_null() && (raw_ptr as usize) >= 0x1000 {
                let gc_obj_type =
                    *raw_ptr.sub(crate::gc::GC_HEADER_SIZE);
                if gc_obj_type == crate::gc::GC_TYPE_ARRAY {
                    return create_url_search_params_object(
                        read_iterable_pair_entries(raw_ptr as *const ArrayHeader),
                    );
                }
            }
        }

        let obj_ptr = ptr_i64 as *mut ObjectHeader;

        // Detect another URLSearchParams: its `_entries` field holds the
        // ArrayHeader of [k, v] pair arrays. We can't tell apart by class
        // (both are class_id 0), so peek at the keys array's first entry.
        // Simpler heuristic: try to read it as an entries-table; fall back
        // to record enumeration if shape doesn't match.
        let copied = try_read_as_search_params(obj_ptr);
        if let Some(entries) = copied {
            return create_url_search_params_object(entries);
        }

        // Treat as record `{ k: v }`. Iterate keys and read each field.
        let entries = read_record_entries(obj_ptr);
        return create_url_search_params_object(entries);
    }

    // Numbers / booleans / etc. — coerce to string via the unified string-ptr
    // helper, then parse. Matches Node which `String(init)`-coerces unknown
    // init values before parsing.
    let s = get_string_content(init);
    create_url_search_params_object(parse_query_string(&s))
}

/// Issue #650: iterable URLSearchParams init — each element is itself
/// a 2-element array `[key, value]`. Strings (key + value) are
/// extracted via `get_string_content`, which handles SSO + heap
/// strings + INT32 / number coercion so `new URLSearchParams([["n", 1]])`
/// silently stringifies the value to `"1"` to match Node. Non-array
/// entries are skipped (Node would throw, but Perry's loose-iter
/// stance is to accept what we can read and drop garbage rather than
/// abort the whole construction).
fn read_iterable_pair_entries(arr: *const ArrayHeader) -> Vec<(String, String)> {
    if arr.is_null() {
        return Vec::new();
    }
    let len = unsafe { (*arr).length } as usize;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let pair_f64 = crate::array::js_array_get_f64(arr, i as u32);
        let pair_bits = pair_f64.to_bits();
        let pair_jsval = crate::value::JSValue::from_bits(pair_bits);
        if !pair_jsval.is_pointer() {
            continue;
        }
        let pair_ptr_i64 = crate::value::js_nanbox_get_pointer(pair_f64);
        if pair_ptr_i64 == 0 {
            continue;
        }
        let pair_raw = pair_ptr_i64 as *const u8;
        unsafe {
            if (pair_raw as usize) < 0x1000 {
                continue;
            }
            let gc_type = *pair_raw.sub(crate::gc::GC_HEADER_SIZE);
            if gc_type != crate::gc::GC_TYPE_ARRAY {
                continue;
            }
        }
        let pair = pair_ptr_i64 as *const ArrayHeader;
        let pair_len = unsafe { (*pair).length };
        if pair_len < 2 {
            continue;
        }
        let k = get_string_content(crate::array::js_array_get_f64(pair, 0));
        let v = get_string_content(crate::array::js_array_get_f64(pair, 1));
        out.push((k, v));
    }
    out
}

/// Walk an object as if it were a URLSearchParams, returning `Some(entries)`
/// only if the shape matches: a single field whose value is a top-level array
/// of 2-element [string, string] pair arrays. Returns None for any other shape
/// (the caller falls back to record enumeration).
fn try_read_as_search_params(params: *mut ObjectHeader) -> Option<Vec<(String, String)>> {
    if params.is_null() {
        return None;
    }
    unsafe {
        // URLSearchParams stores entries in field index 0 (URL_SEARCH_PARAMS_ENTRIES).
        // If this isn't a URLSearchParams, that slot likely holds a string or
        // is missing — we detect by checking the keys array shape.
        let keys_arr = (*params).keys_array;
        if keys_arr.is_null() {
            return None;
        }
        let keys_len = (*keys_arr).length;
        if keys_len != 1 {
            return None;
        }
        let key0 = crate::array::js_array_get_f64(keys_arr, 0);
        let key0_str = get_string_content(key0);
        if key0_str != "_entries" {
            return None;
        }
    }
    Some(get_url_search_params_entries(params))
}

/// Enumerate an object's own enumerable keys as `(name, String(value))` pairs.
/// Used for `new URLSearchParams({ a: "1", b: "2" })` — order matches the
/// keys array (insertion order, like Node).
fn read_record_entries(obj: *mut ObjectHeader) -> Vec<(String, String)> {
    if obj.is_null() {
        return Vec::new();
    }
    unsafe {
        let keys_arr = (*obj).keys_array;
        if keys_arr.is_null() {
            return Vec::new();
        }
        let len = (*keys_arr).length as usize;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let key_f64 = crate::array::js_array_get_f64(keys_arr, i as u32);
            let key = get_string_content(key_f64);
            if key.is_empty() {
                continue;
            }
            let val_f64 = crate::object::js_object_get_field_f64(obj, i as u32);
            let val = stringify_field_value(val_f64);
            out.push((key, val));
        }
        out
    }
}

/// Coerce a NaN-boxed field value to a String the way Node's
/// `URLSearchParams` does — values are passed through `String(...)`. Strings
/// pass through; numbers / booleans use their textual form; null/undefined
/// stringify literally.
fn stringify_field_value(v: f64) -> String {
    let bits = v.to_bits();
    let jsval = crate::value::JSValue::from_bits(bits);
    if jsval.is_string() || jsval.is_short_string() {
        return get_string_content(v);
    }
    if jsval.is_undefined() {
        return "undefined".to_string();
    }
    if jsval.is_null() {
        return "null".to_string();
    }
    if !v.is_nan() {
        // Plain double — format without trailing ".0" for integers.
        if v == v.trunc() && v.is_finite() && v.abs() < 1e21 {
            return format!("{}", v as i64);
        }
        return format!("{}", v);
    }
    // Booleans land in the NaN-tag space.
    if bits == 0x7FFC_0000_0000_0004 {
        return "true".to_string();
    }
    if bits == 0x7FFC_0000_0000_0003 {
        return "false".to_string();
    }
    // Pointer / unknown: stringify via the unified helper.
    get_string_content(v)
}

/// Get a value by name
/// js_url_search_params_get(params: *mut ObjectHeader, name: *mut StringHeader) -> *mut StringHeader (string or null)
#[no_mangle]
pub extern "C" fn js_url_search_params_get(
    params: *mut ObjectHeader,
    name_str: *mut crate::StringHeader,
) -> *mut crate::StringHeader {
    let name = if name_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*name_str).byte_len as usize;
            let data_ptr = (name_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let entries = get_url_search_params_entries(params);
    for (key, value) in entries {
        if key == name {
            let bytes = value.as_bytes();
            return js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        }
    }

    // Return null pointer
    std::ptr::null_mut()
}

/// Check if a name exists
/// js_url_search_params_has(params: *mut ObjectHeader, name: *mut StringHeader) -> f64 (boolean)
#[no_mangle]
pub extern "C" fn js_url_search_params_has(
    params: *mut ObjectHeader,
    name_str: *mut crate::StringHeader,
) -> f64 {
    let name = if name_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*name_str).byte_len as usize;
            let data_ptr = (name_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let entries = get_url_search_params_entries(params);
    let found = entries.iter().any(|(key, _)| key == &name);
    if found {
        1.0
    } else {
        0.0
    }
}

/// Set a value (replaces existing or adds new)
/// js_url_search_params_set(params: *mut ObjectHeader, name: *mut StringHeader, value: *mut StringHeader) -> void
#[no_mangle]
pub extern "C" fn js_url_search_params_set(
    params: *mut ObjectHeader,
    name_str: *mut crate::StringHeader,
    value_str: *mut crate::StringHeader,
) {
    let name = if name_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*name_str).byte_len as usize;
            let data_ptr = (name_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let value = if value_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*value_str).byte_len as usize;
            let data_ptr = (value_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let mut entries = get_url_search_params_entries(params);

    // Remove all existing entries with this name, then add the new one
    entries.retain(|(key, _)| key != &name);
    entries.push((name, value));

    // Update the object with new entries
    unsafe {
        let mut entries_array = js_array_alloc(entries.len() as u32);
        for (key, val) in entries {
            let mut pair = js_array_alloc(2);
            pair = js_array_push_f64(pair, create_string_f64(&key));
            pair = js_array_push_f64(pair, create_string_f64(&val));
            let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
            entries_array = js_array_push_f64(entries_array, pair_f64);
        }
        let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
        js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    }
}

/// Append a value (adds even if name already exists)
/// js_url_search_params_append(params: *mut ObjectHeader, name: *mut StringHeader, value: *mut StringHeader) -> void
#[no_mangle]
pub extern "C" fn js_url_search_params_append(
    params: *mut ObjectHeader,
    name_str: *mut crate::StringHeader,
    value_str: *mut crate::StringHeader,
) {
    let name = if name_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*name_str).byte_len as usize;
            let data_ptr = (name_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let value = if value_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*value_str).byte_len as usize;
            let data_ptr = (value_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let mut entries = get_url_search_params_entries(params);
    entries.push((name, value));

    // Update the object with new entries
    unsafe {
        let mut entries_array = js_array_alloc(entries.len() as u32);
        for (key, val) in entries {
            let mut pair = js_array_alloc(2);
            pair = js_array_push_f64(pair, create_string_f64(&key));
            pair = js_array_push_f64(pair, create_string_f64(&val));
            let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
            entries_array = js_array_push_f64(entries_array, pair_f64);
        }
        let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
        js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    }
}

/// Delete all entries with a name
/// js_url_search_params_delete(params: *mut ObjectHeader, name: *mut StringHeader) -> void
#[no_mangle]
pub extern "C" fn js_url_search_params_delete(
    params: *mut ObjectHeader,
    name_str: *mut crate::StringHeader,
) {
    let name = if name_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*name_str).byte_len as usize;
            let data_ptr = (name_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let mut entries = get_url_search_params_entries(params);
    entries.retain(|(key, _)| key != &name);

    // Update the object with new entries
    unsafe {
        let mut entries_array = js_array_alloc(entries.len() as u32);
        for (key, val) in entries {
            let mut pair = js_array_alloc(2);
            pair = js_array_push_f64(pair, create_string_f64(&key));
            pair = js_array_push_f64(pair, create_string_f64(&val));
            let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
            entries_array = js_array_push_f64(entries_array, pair_f64);
        }
        let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
        js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    }
}

/// Issue #650: `URLSearchParams.size` getter — returns the number of
/// entries (key/value pairs) currently stored. Reads the length of the
/// `_entries` ArrayHeader directly; null receiver / missing array
/// returns 0.
#[no_mangle]
pub extern "C" fn js_url_search_params_size(params: *mut ObjectHeader) -> i32 {
    if params.is_null() {
        return 0;
    }
    let entries_f64 = crate::object::js_object_get_field_f64(params, URL_SEARCH_PARAMS_ENTRIES);
    let entries_ptr: *const ArrayHeader =
        unsafe { f64::to_bits(entries_f64).cast_signed() as *const ArrayHeader };
    if entries_ptr.is_null() {
        return 0;
    }
    unsafe { (*entries_ptr).length as i32 }
}

/// Convert to query string
/// js_url_search_params_to_string(params: *mut ObjectHeader) -> *mut StringHeader (raw string pointer)
#[no_mangle]
pub extern "C" fn js_url_search_params_to_string(
    params: *mut ObjectHeader,
) -> *mut crate::StringHeader {
    let entries = get_url_search_params_entries(params);

    if entries.is_empty() {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }

    let result: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect();

    let joined = result.join("&");
    let bytes = joined.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// `params.entries()` — returns an array of `[key, value]` pair arrays. Used
/// to lower direct iteration `for (const [k, v] of params)` (refs #575). The
/// pair arrays expose strings, so the destructure `[k, v]` reads them with
/// the standard array-element path.
#[no_mangle]
pub extern "C" fn js_url_search_params_entries_arr(params: *mut ObjectHeader) -> f64 {
    let entries = get_url_search_params_entries(params);
    unsafe {
        let mut arr = js_array_alloc(entries.len() as u32);
        for (k, v) in entries {
            let mut pair = js_array_alloc(2);
            pair = js_array_push_f64(pair, create_string_f64(&k));
            pair = js_array_push_f64(pair, create_string_f64(&v));
            // Inline NaN-box the pair pointer with POINTER_TAG so for-of
            // destructure reads the array via `js_array_get_f64` correctly.
            let pair_bits = 0x7FFD_0000_0000_0000u64 | ((pair as u64) & 0x0000_FFFF_FFFF_FFFF);
            arr = js_array_push_f64(arr, f64::from_bits(pair_bits));
        }
        f64::from_bits(0x7FFD_0000_0000_0000u64 | ((arr as u64) & 0x0000_FFFF_FFFF_FFFF))
    }
}

/// Get all values for a name
/// js_url_search_params_get_all(params: *mut ObjectHeader, name: *mut StringHeader) -> f64 (array)
#[no_mangle]
pub extern "C" fn js_url_search_params_get_all(
    params: *mut ObjectHeader,
    name_str: *mut crate::StringHeader,
) -> f64 {
    let name = if name_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*name_str).byte_len as usize;
            let data_ptr = (name_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let entries = get_url_search_params_entries(params);
    let values: Vec<String> = entries
        .iter()
        .filter(|(key, _)| key == &name)
        .map(|(_, value)| value.clone())
        .collect();

    unsafe {
        let mut result = js_array_alloc(values.len() as u32);
        for value in values {
            result = js_array_push_f64(result, create_string_f64(&value));
        }
        f64::from_bits(i64::cast_unsigned(result as i64))
    }
}

// =========================================================================
// AbortController implementation
// =========================================================================

/// AbortController object structure (matches ObjectHeader layout)
/// Field 0: signal (object-ptr NaN-boxed)
/// Field 1: aborted flag (NaN-boxed bool)
const ABORT_CONTROLLER_FIELD_COUNT: u32 = 2;
const ABORT_SIGNAL_FIELD: u32 = 0;
const ABORT_ABORTED_FIELD: u32 = 1;

// AbortSignal object layout (all fields NaN-boxed):
//   field 0: aborted (bool)
//   field 1: reason (any)
//   field 2: listeners (array of closure f64 values; may be null/undefined if empty)
const ABORT_SIGNAL_FIELD_COUNT: u32 = 3;

const TAG_UNDEFINED_AC: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE_AC: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE_AC: u64 = 0x7FFC_0000_0000_0003;
const POINTER_TAG_AC: u64 = 0x7FFD_0000_0000_0000;

#[inline]
fn nanbox_pointer_ac(ptr: *mut ObjectHeader) -> f64 {
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED_AC);
    }
    let bits = POINTER_TAG_AC | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF);
    f64::from_bits(bits)
}

#[inline]
fn unbox_pointer_ac(v: f64) -> *mut ObjectHeader {
    let bits = v.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != POINTER_TAG_AC {
        // Fallback: legacy raw bitcast path
        return (v.to_bits() as usize) as *mut ObjectHeader;
    }
    (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

fn alloc_abort_signal() -> *mut ObjectHeader {
    unsafe {
        let signal = js_object_alloc(0, ABORT_SIGNAL_FIELD_COUNT);
        let mut signal_keys = js_array_alloc(ABORT_SIGNAL_FIELD_COUNT);
        signal_keys = js_array_push_f64(signal_keys, create_string_f64("aborted"));
        signal_keys = js_array_push_f64(signal_keys, create_string_f64("reason"));
        signal_keys = js_array_push_f64(signal_keys, create_string_f64("_listeners"));
        js_object_set_keys(signal, signal_keys);
        js_object_set_field_f64(signal, 0, f64::from_bits(TAG_FALSE_AC));
        js_object_set_field_f64(signal, 1, f64::from_bits(TAG_UNDEFINED_AC));
        js_object_set_field_f64(signal, 2, f64::from_bits(TAG_UNDEFINED_AC));
        signal
    }
}

/// Create a new AbortController
#[no_mangle]
pub extern "C" fn js_abort_controller_new() -> *mut ObjectHeader {
    unsafe {
        // Allocate the AbortController object
        let controller = js_object_alloc(0, ABORT_CONTROLLER_FIELD_COUNT);

        let signal = alloc_abort_signal();

        // Set up controller keys
        let mut keys = js_array_alloc(ABORT_CONTROLLER_FIELD_COUNT);
        keys = js_array_push_f64(keys, create_string_f64("signal"));
        keys = js_array_push_f64(keys, create_string_f64("aborted"));
        js_object_set_keys(controller, keys);

        // Store signal in controller (NaN-boxed with POINTER_TAG)
        js_object_set_field_f64(controller, ABORT_SIGNAL_FIELD, nanbox_pointer_ac(signal));
        js_object_set_field_f64(
            controller,
            ABORT_ABORTED_FIELD,
            f64::from_bits(TAG_FALSE_AC),
        );

        controller
    }
}

/// Get the signal from an AbortController (returns NaN-boxed object ptr)
#[no_mangle]
pub extern "C" fn js_abort_controller_signal(controller: *mut ObjectHeader) -> *mut ObjectHeader {
    if controller.is_null() {
        return std::ptr::null_mut();
    }
    let signal_val = crate::object::js_object_get_field_f64(controller, ABORT_SIGNAL_FIELD);
    unbox_pointer_ac(signal_val)
}

fn fire_abort_listeners(signal: *mut ObjectHeader) {
    if signal.is_null() {
        return;
    }
    unsafe {
        let listeners_val = crate::object::js_object_get_field_f64(signal, 2);
        let bits = listeners_val.to_bits();
        if bits == TAG_UNDEFINED_AC || bits == TAG_FALSE_AC {
            return;
        }
        // Extract array pointer (NaN-boxed POINTER_TAG).
        let arr_ptr = if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG_AC {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader
        } else {
            return;
        };
        if arr_ptr.is_null() {
            return;
        }
        let len = crate::array::js_array_length(arr_ptr) as usize;
        for i in 0..len {
            let cb_val = crate::array::js_array_get_f64(arr_ptr, i as u32);
            let cb_bits = cb_val.to_bits();
            // Try to extract closure pointer (may be POINTER_TAG or raw bitcast).
            let cb_ptr = if (cb_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG_AC {
                (cb_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader
            } else if cb_bits > 0x10000 && (cb_bits >> 48) == 0 {
                cb_bits as *const crate::closure::ClosureHeader
            } else {
                continue;
            };
            if !cb_ptr.is_null() {
                crate::closure::js_closure_call0(cb_ptr);
            }
        }
    }
}

/// Abort the controller (sets aborted = true on signal)
#[no_mangle]
pub extern "C" fn js_abort_controller_abort(controller: *mut ObjectHeader) {
    js_abort_controller_abort_reason(controller, f64::from_bits(TAG_UNDEFINED_AC));
}

/// Abort with an optional reason (NaN-boxed value). Fires any registered listeners.
#[no_mangle]
pub extern "C" fn js_abort_controller_abort_reason(controller: *mut ObjectHeader, reason: f64) {
    if controller.is_null() {
        return;
    }
    unsafe {
        let signal_val = crate::object::js_object_get_field_f64(controller, ABORT_SIGNAL_FIELD);
        let signal = unbox_pointer_ac(signal_val);

        if !signal.is_null() {
            // Set aborted = true on signal
            js_object_set_field_f64(signal, 0, f64::from_bits(TAG_TRUE_AC));
            // Store reason (defaults to undefined); if user passes a string or other value we keep it as-is.
            js_object_set_field_f64(signal, 1, reason);
            // Fire listeners
            fire_abort_listeners(signal);
        }

        // Also set aborted on controller
        js_object_set_field_f64(controller, ABORT_ABORTED_FIELD, f64::from_bits(TAG_TRUE_AC));
    }
}

/// Register an "abort" event listener on a signal. `event_type` is the NaN-boxed
/// string name (we only act on "abort"); `listener` is a NaN-boxed closure f64.
#[no_mangle]
pub extern "C" fn js_abort_signal_add_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) {
    if signal.is_null() {
        return;
    }
    // Only handle "abort" events — ignore everything else.
    let type_str = get_string_content(event_type);
    if type_str != "abort" {
        return;
    }
    unsafe {
        let listeners_val = crate::object::js_object_get_field_f64(signal, 2);
        let bits = listeners_val.to_bits();
        let arr_ptr: *mut crate::array::ArrayHeader =
            if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG_AC {
                (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader
            } else {
                // Lazily allocate the listeners array.
                let new_arr = js_array_alloc(0);
                let new_bits = POINTER_TAG_AC | ((new_arr as u64) & 0x0000_FFFF_FFFF_FFFF);
                js_object_set_field_f64(signal, 2, f64::from_bits(new_bits));
                new_arr
            };
        if !arr_ptr.is_null() {
            js_array_push_f64(arr_ptr, listener);
        }
    }
}

/// `AbortSignal.timeout(ms)` — returns a signal that is initially not aborted.
/// Perry does not spin up a real timer for this stub (tests only check the
/// initial state), but the returned object has the full AbortSignal shape so
/// subsequent `.aborted` / `.reason` / `.addEventListener` reads work.
#[no_mangle]
pub extern "C" fn js_abort_signal_timeout(_ms: f64) -> *mut ObjectHeader {
    alloc_abort_signal()
}

/// Convert a file:// URL to a filesystem path
/// Strips the "file://" prefix and percent-decodes the result
/// js_url_file_url_to_path(url_f64: f64) -> f64 (NaN-boxed string)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path(url_f64: f64) -> f64 {
    let url_string = get_string_content(url_f64);

    // Strip file:// prefix
    let path = if url_string.starts_with("file:///") {
        // file:///path → /path (Unix)
        &url_string[7..]
    } else if url_string.starts_with("file://") {
        // file://host/path or file:///path
        &url_string[7..]
    } else if url_string.starts_with("file:") {
        &url_string[5..]
    } else {
        // Not a file URL, return as-is
        &url_string
    };

    // Percent-decode the path
    let decoded = url_decode(path);
    create_string_f64(&decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_url() {
        let (protocol, host, hostname, port, pathname, search, hash) =
            parse_url("https://example.com/path?query=1#section");
        assert_eq!(protocol, "https:");
        assert_eq!(host, "example.com");
        assert_eq!(hostname, "example.com");
        assert_eq!(port, "");
        assert_eq!(pathname, "/path");
        assert_eq!(search, "?query=1");
        assert_eq!(hash, "#section");
    }

    #[test]
    fn test_parse_url_with_port() {
        let (protocol, host, hostname, port, pathname, _, _) =
            parse_url("http://localhost:3000/api");
        assert_eq!(protocol, "http:");
        assert_eq!(host, "localhost:3000");
        assert_eq!(hostname, "localhost");
        assert_eq!(port, "3000");
        assert_eq!(pathname, "/api");
    }

    #[test]
    fn test_parse_file_url() {
        let (protocol, host, hostname, _, pathname, _, _) = parse_url("file:///Users/test/file.ts");
        assert_eq!(protocol, "file:");
        assert_eq!(host, "");
        assert_eq!(hostname, "");
        assert_eq!(pathname, "/Users/test/file.ts");
    }

    #[test]
    fn test_resolve_relative_url() {
        let resolved = resolve_url(".", "file:///Users/test/lib/file.ts");
        assert_eq!(resolved, "file:/Users/test/lib");

        let resolved = resolve_url("..", "file:///Users/test/lib/file.ts");
        assert_eq!(resolved, "file:/Users/test");
    }

    #[test]
    fn test_parse_query_string() {
        let entries = parse_query_string("foo=bar&baz=qux");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("foo".to_string(), "bar".to_string()));
        assert_eq!(entries[1], ("baz".to_string(), "qux".to_string()));
    }

    #[test]
    fn test_url_search_params_entries() {
        let entries = vec![
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ];
        let params = create_url_search_params_object(entries);

        let read_entries = get_url_search_params_entries(params);
        assert_eq!(
            read_entries.len(),
            2,
            "Expected 2 entries, got {}",
            read_entries.len()
        );
        assert_eq!(read_entries[0].0, "key1");
        assert_eq!(read_entries[0].1, "value1");
        assert_eq!(read_entries[1].0, "key2");
        assert_eq!(read_entries[1].1, "value2");
    }

    #[test]
    fn test_string_round_trip() {
        // Test that create_string_f64 and get_string_content round-trip correctly
        let original = "test string";
        let f64_val = create_string_f64(original);
        let recovered = get_string_content(f64_val);
        assert_eq!(
            recovered, original,
            "String round-trip failed: expected '{}', got '{}'",
            original, recovered
        );
    }
}
