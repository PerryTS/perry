//! Node.js URL-module compatibility helpers — `fileURLToPath`,
//! `pathToFileURL`, `domainToASCII`, `urlToHttpOptions`, legacy
//! `url.format` / `url.parse` / `url.resolve`.

use super::*;

use super::parse::{create_url_object, is_valid_absolute_url, parse_url, resolve_url};
use super::search_params::url_decode;

const QUERYSTRING_ESCAPE_HEX: &[u8; 16] = b"0123456789ABCDEF";

fn legacy_querystring_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(QUERYSTRING_ESCAPE_HEX[(b >> 4) as usize] as char);
                out.push(QUERYSTRING_ESCAPE_HEX[(b & 0x0F) as usize] as char);
            }
        }
    }
    out
}

fn throw_url_format_invalid_arg() -> ! {
    let msg = b"The \"urlObject\" argument must be of type object or string.";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_url_type_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn is_js_string_value(value: f64) -> bool {
    crate::value::JSValue::from_bits(value.to_bits()).is_any_string()
}

fn string_from_js_value(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *mut crate::StringHeader;
    string_from_header(ptr)
}

fn url_received(value: f64) -> String {
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return "an instance of Buffer".to_string();
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
        return format!(
            "type symbol ({})",
            string_from_header(ptr as *mut StringHeader)
        );
    }
    crate::fs::validate::describe_received(value)
}

fn throw_invalid_url_arg(value: f64, url_instance_allowed: bool) -> ! {
    let expected = if url_instance_allowed {
        "string or an instance of URL"
    } else {
        "string"
    };
    let message = format!(
        "The \"path\" argument must be of type {expected}. Received {}",
        url_received(value)
    );
    throw_url_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_legacy_url_arg(value: f64) -> ! {
    let message = format!(
        "The \"url\" argument must be of type string. Received {}",
        url_received(value)
    );
    throw_url_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-decode a file-URL pathname to its raw byte sequence. Bytes are
/// returned verbatim (no UTF-8 validation) so `fileURLToPathBuffer` can
/// preserve paths whose decoded bytes are not valid UTF-8.
fn decode_file_url_pathname_bytes(pathname: &str) -> Vec<u8> {
    let bytes = pathname.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if bytes[i + 1] == b'2' && (bytes[i + 2] | 0x20) == b'f' {
                throw_url_type_error_with_code(
                    "File URL path must not include encoded / characters",
                    "ERR_INVALID_FILE_URL_PATH",
                );
            }
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn decode_file_url_pathname(pathname: &str) -> String {
    String::from_utf8_lossy(&decode_file_url_pathname_bytes(pathname)).into_owned()
}

/// Shared `file:` URL → path parsing for both `fileURLToPath` (UTF-8 string)
/// and `fileURLToPathBuffer` (raw bytes). Returns the decoded path bytes,
/// throwing the same scheme/host/encoded-slash errors as Node.
fn file_url_to_path_bytes(url_f64: f64) -> Vec<u8> {
    let url_string = if is_js_string_value(url_f64) {
        string_from_js_value(url_f64)
    } else if let Some(obj) = object_from_f64(url_f64) {
        if !is_url_object_shape(obj) {
            throw_invalid_url_arg(url_f64, true);
        }
        object_prop_string(obj, "href")
    } else {
        throw_invalid_url_arg(url_f64, true);
    };

    let Some(after_scheme) = url_string.strip_prefix("file:") else {
        throw_url_type_error_with_code("The URL must be of scheme file", "ERR_INVALID_URL_SCHEME");
    };

    let pathname = if let Some(authority_and_path) = after_scheme.strip_prefix("//") {
        let path_start = authority_and_path
            .find('/')
            .unwrap_or(authority_and_path.len());
        let host = &authority_and_path[..path_start];
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            throw_url_type_error_with_code(
                "File URL host must be \"localhost\" or empty on darwin",
                "ERR_INVALID_FILE_URL_HOST",
            );
        }
        &authority_and_path[path_start..]
    } else {
        after_scheme
    };
    let pathname = pathname.split(['?', '#']).next().unwrap_or_default();
    decode_file_url_pathname_bytes(pathname)
}

/// Convert a file:// URL to a filesystem path
/// Strips the "file://" prefix and percent-decodes the result
/// js_url_file_url_to_path(url_f64: f64) -> f64 (NaN-boxed string)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path(url_f64: f64) -> f64 {
    let decoded = String::from_utf8_lossy(&file_url_to_path_bytes(url_f64)).into_owned();
    create_string_f64(&decoded)
}

/// `url.fileURLToPathBuffer(url[, options])` (#2541) — the Buffer-returning
/// counterpart to `fileURLToPath`. Returns the decoded path's raw bytes as a
/// `Buffer`, preserving percent-encoded sequences that are not valid UTF-8
/// (where the string form would lossily substitute U+FFFD). Same scheme/host
/// validation as `fileURLToPath`.
/// js_url_file_url_to_path_buffer(url_f64: f64) -> f64 (NaN-boxed Buffer ptr)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path_buffer(url_f64: f64) -> f64 {
    let bytes = file_url_to_path_bytes(url_f64);
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    crate::value::js_nanbox_pointer(buf as i64)
}

#[no_mangle]
pub extern "C" fn js_url_path_to_file_url(path_f64: f64) -> f64 {
    if !is_js_string_value(path_f64) {
        throw_invalid_url_arg(path_f64, false);
    }
    let path = get_string_content(path_f64);
    let path = crate::path::resolve_posix_str(&path);
    let mut encoded = String::new();
    for b in path.bytes() {
        match b {
            b'/' => encoded.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char)
            }
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    let href = if encoded.starts_with('/') {
        format!("file://{}", encoded)
    } else {
        format!("file:///{}", encoded)
    };
    let obj = create_url_object(&href);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_url_domain_to_ascii(input_f64: f64) -> f64 {
    let input = string_from_header(js_url_coerce_string(input_f64));
    if input.chars().any(|c| c.is_ascii_whitespace()) {
        return create_string_f64("");
    }
    let out = idna::domain_to_ascii(&input).unwrap_or_else(|_| String::new());
    create_string_f64(&out)
}

#[no_mangle]
pub extern "C" fn js_url_domain_to_unicode(input_f64: f64) -> f64 {
    let input = string_from_header(js_url_coerce_string(input_f64));
    if input.chars().any(|c| c.is_ascii_whitespace()) {
        return create_string_f64("");
    }
    let (out, _) = idna::domain_to_unicode(&input);
    create_string_f64(&out)
}

fn json_to_value(json: serde_json::Value) -> f64 {
    let s = json.to_string();
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    unsafe { f64::from_bits(crate::json::js_json_parse(ptr).bits()) }
}

#[no_mangle]
pub extern "C" fn js_url_to_http_options(url_f64: f64) -> f64 {
    let undef_f64 = f64::from_bits(crate::value::TAG_UNDEFINED);
    let Some(obj) = object_from_f64(url_f64) else {
        let empty = js_object_alloc(0, 0);
        return crate::value::js_nanbox_pointer(empty as i64);
    };
    let protocol = object_prop_string(obj, "protocol");
    let hostname = object_prop_string(obj, "hostname");
    let port_s = object_prop_string(obj, "port");
    let pathname = object_prop_string(obj, "pathname");
    let search = object_prop_string(obj, "search");
    let username = object_prop_string(obj, "username");
    let password = object_prop_string(obj, "password");
    let path = format!("{}{}", pathname, search);

    // Per Node, `auth` is undefined when no userinfo; `"user:"` when username
    // is set but password is empty; `"user:pass"` otherwise. `port` is the
    // numeric value when set, undefined when empty.
    let has_userinfo = !username.is_empty() || !password.is_empty();
    let auth_f64 = if !has_userinfo {
        undef_f64
    } else {
        create_string_f64(&format!("{}:{}", username, password))
    };
    let port_f64 = match port_s.parse::<u32>() {
        Ok(p) => p as f64,
        Err(_) => undef_f64,
    };

    let field_count: u32 = 5;
    let obj_out = js_object_alloc(0, field_count);
    let mut keys = js_array_alloc(field_count);
    keys = js_array_push_f64(keys, create_string_f64("protocol"));
    keys = js_array_push_f64(keys, create_string_f64("hostname"));
    keys = js_array_push_f64(keys, create_string_f64("port"));
    keys = js_array_push_f64(keys, create_string_f64("path"));
    keys = js_array_push_f64(keys, create_string_f64("auth"));
    js_object_set_keys(obj_out, keys);
    js_object_set_field_f64(obj_out, 0, create_string_f64(&protocol));
    js_object_set_field_f64(obj_out, 1, create_string_f64(&hostname));
    js_object_set_field_f64(obj_out, 2, port_f64);
    js_object_set_field_f64(obj_out, 3, create_string_f64(&path));
    js_object_set_field_f64(obj_out, 4, auth_f64);
    crate::value::js_nanbox_pointer(obj_out as i64)
}

fn legacy_format_from_object(obj: *mut ObjectHeader) -> String {
    let protocol = object_prop_string(obj, "protocol");
    let hostname = object_prop_string(obj, "hostname");
    let host = object_prop_string(obj, "host");
    let port = object_prop_string(obj, "port");
    let pathname = object_prop_string(obj, "pathname");
    let search = object_prop_string(obj, "search");
    let hash = object_prop_string(obj, "hash");
    let auth = object_prop_string(obj, "auth");
    // Legacy `format()` only emits `//` when `slashes` is truthy OR when the
    // protocol is one of the slash-bearing built-ins (http/https/ws/wss/ftp).
    let slashes_val = object_prop_f64(obj, "slashes");
    let slashes_explicit = slashes_val.to_bits() == 0x7FFC_0000_0000_0004u64;
    let proto_wants_slashes = matches!(
        protocol.trim_end_matches(':'),
        "http" | "https" | "ws" | "wss" | "ftp" | "file"
    );
    // Legacy `url.format()`: hierarchical schemes always get `//` regardless
    // of the `slashes` flag (Node ignores `slashes:false` for http/https/etc.).
    let use_slashes = slashes_explicit || proto_wants_slashes;
    let mut out = String::new();
    if !protocol.is_empty() {
        out.push_str(&protocol);
        if !protocol.ends_with(':') {
            out.push(':');
        }
    }
    let authority = if !host.is_empty() {
        host
    } else if !hostname.is_empty() && !port.is_empty() {
        format!("{hostname}:{port}")
    } else {
        hostname
    };
    if !authority.is_empty() {
        if use_slashes {
            out.push_str("//");
        }
        if !auth.is_empty() {
            out.push_str(&auth);
            out.push('@');
        }
        out.push_str(&authority);
    }
    out.push_str(&pathname);
    if !search.is_empty() {
        out.push_str(&search);
    } else {
        let query = object_prop_f64(obj, "query");
        if let Some(qobj) = object_from_f64(query) {
            let keys = crate::object::js_object_keys(qobj as *const ObjectHeader);
            let len = unsafe { (*keys).length };
            let mut parts = Vec::new();
            for i in 0..len {
                let key_f = crate::array::js_array_get_f64(keys, i);
                let key = get_string_content(key_f);
                let val_key = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let val = crate::object::js_object_get_field_by_name_f64(qobj, val_key);
                parts.push(format!(
                    "{}={}",
                    legacy_querystring_escape(&key),
                    legacy_querystring_escape(&get_string_content(val))
                ));
            }
            if !parts.is_empty() {
                out.push('?');
                out.push_str(&parts.join("&"));
            }
        } else {
            let q = get_string_content(query);
            if !q.is_empty() {
                out.push('?');
                out.push_str(&q);
            }
        }
    }
    out.push_str(&hash);
    out
}

#[no_mangle]
pub extern "C" fn js_url_format(value: f64, options: f64) -> f64 {
    let Some(obj) = object_from_f64(value) else {
        let js_value = crate::value::JSValue::from_bits(value.to_bits());
        if js_value.is_any_string() {
            let ptr =
                crate::value::js_get_string_pointer_unified(value) as *mut crate::StringHeader;
            return create_string_f64(&string_from_header(ptr));
        }
        throw_url_format_invalid_arg();
    };
    let href = object_prop_string(obj, "href");
    let mut out = if !href.is_empty() {
        href
    } else {
        legacy_format_from_object(obj)
    };
    if let Some(opts) = object_from_f64(options) {
        let false_bits = 0x7FFC_0000_0000_0003u64;
        if object_prop_f64(opts, "search").to_bits() == false_bits {
            if let Some(idx) = out.find('?') {
                out.truncate(idx);
            }
        }
        if object_prop_f64(opts, "fragment").to_bits() == false_bits {
            if let Some(idx) = out.find('#') {
                out.truncate(idx);
            }
        }
    }
    create_string_f64(&out)
}

#[no_mangle]
pub extern "C" fn js_url_legacy_parse(
    input: f64,
    parse_query_string: f64,
    slashes_denote_host: f64,
) -> f64 {
    if !is_js_string_value(input) {
        throw_invalid_legacy_url_arg(input);
    }
    let s = get_string_content(input);
    let (protocol, mut host, mut hostname, mut port, mut pathname, search, hash) = parse_url(&s);
    let slashes_host = crate::value::js_is_truthy(slashes_denote_host) != 0;
    let protocol_is_null = protocol.is_empty() && slashes_host && s.starts_with("//");

    if protocol_is_null {
        let rest = pathname.strip_prefix("//").unwrap_or(&pathname);
        let path_idx = rest.find('/').unwrap_or(rest.len());
        host = rest[..path_idx].to_string();
        pathname = if path_idx < rest.len() {
            rest[path_idx..].to_string()
        } else {
            "/".to_string()
        };
        hostname = host.clone();
        if let Some(port_idx) = host.rfind(':') {
            let potential_port = &host[port_idx + 1..];
            if !potential_port.is_empty() && potential_port.chars().all(|c| c.is_ascii_digit()) {
                hostname = host[..port_idx].to_string();
                port = potential_port.to_string();
            }
        }
    }

    if let Some(percent_idx) = host.find('%') {
        let invalid_tail = format!("{}{}", &host[percent_idx..], pathname);
        host.truncate(percent_idx);
        hostname = host.clone();
        port.clear();
        pathname = invalid_tail;
    }

    let mut auth = String::new();
    if let Some(at_idx) = host.rfind('@') {
        auth = host[..at_idx].to_string();
        let rest = host[at_idx + 1..].to_string();
        host = rest.clone();
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
    let parse_qs = crate::value::js_is_truthy(parse_query_string) != 0;
    let query = if parse_qs {
        let mut map = serde_json::Map::new();
        let raw = search.strip_prefix('?').unwrap_or(&search);
        for part in raw.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            map.insert(url_decode(k), serde_json::Value::String(url_decode(v)));
        }
        serde_json::Value::Object(map)
    } else {
        serde_json::Value::String(search.strip_prefix('?').unwrap_or(&search).to_string())
    };
    let protocol_value = if protocol_is_null {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(protocol)
    };
    json_to_value(serde_json::json!({
        "protocol": protocol_value,
        "host": host,
        "hostname": hostname,
        "port": port,
        "pathname": pathname,
        "path": format!("{}{}", pathname, search),
        "search": search,
        "query": query,
        "hash": hash,
        "auth": auth
    }))
}

#[no_mangle]
pub extern "C" fn js_url_legacy_resolve(from: f64, to: f64) -> f64 {
    if !is_js_string_value(from) {
        throw_invalid_legacy_url_arg(from);
    }
    if !is_js_string_value(to) {
        throw_invalid_legacy_url_arg(to);
    }
    let from_s = get_string_content(from);
    let to_s = get_string_content(to);
    let resolved = if to_s.starts_with('/') && !is_valid_absolute_url(&from_s) {
        to_s
    } else if let Ok(base) = url::Url::parse(&from_s) {
        base.join(&to_s)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| resolve_url(&to_s, &from_s))
    } else {
        resolve_url(&to_s, &from_s)
    };
    create_string_f64(&resolved)
}
