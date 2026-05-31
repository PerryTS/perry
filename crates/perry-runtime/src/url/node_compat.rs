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

fn resolve_path_to_file_url_posix(path: &str) -> String {
    let mut resolved = crate::path::resolve_posix_str(path);
    if !path.is_empty() && path.ends_with('/') && !resolved.ends_with('/') {
        resolved.push('/');
    }
    resolved
}

/// Read the `windows` option from a `{ windows }` options argument (#2975).
/// Node treats a `true` value as force-Windows, anything else (including a
/// missing/undefined options arg or `{ windows: false }`) as POSIX. Returns
/// `false` for non-object / undefined options.
fn options_windows_flag(options: f64) -> bool {
    match object_from_f64(options) {
        Some(opts) => crate::value::js_is_truthy(object_prop_f64(opts, "windows")) != 0,
        None => false,
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-decode a POSIX file-URL pathname to its raw byte sequence. Bytes
/// are returned verbatim (no UTF-8 validation) so `fileURLToPathBuffer` can
/// preserve paths whose decoded bytes are not valid UTF-8. Mirrors Node's
/// `getPathFromURLPosix`: an encoded `/` (`%2f`/`%2F`) is rejected, but an
/// encoded `\` is decoded through as an ordinary byte.
fn decode_file_url_pathname_bytes_posix(pathname: &str) -> Vec<u8> {
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

/// Percent-decode a Windows file-URL pathname to raw bytes, converting `/`
/// separators to `\`. Mirrors Node's `getPathFromURLWin32`: encoded `/`
/// (`%2f`) AND encoded `\` (`%5c`) are both rejected; the rest decodes through.
fn decode_file_url_pathname_bytes_win32(pathname: &str) -> Vec<u8> {
    let bytes = pathname.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1];
            let h2 = bytes[i + 2] | 0x20; // lowercase the second hex digit
            if (h1 == b'2' && h2 == b'f') || (h1 == b'5' && h2 == b'c') {
                throw_url_type_error_with_code(
                    "File URL path must not include encoded \\ or / characters",
                    "ERR_INVALID_FILE_URL_PATH",
                );
            }
            if let (Some(hi), Some(lo)) = (hex_nibble(h1), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        // Convert forward slashes to backslashes as Node does pre-decode.
        out.push(if bytes[i] == b'/' { b'\\' } else { bytes[i] });
        i += 1;
    }
    out
}

/// Shared `file:` URL → path parsing for both `fileURLToPath` (UTF-8 string)
/// and `fileURLToPathBuffer` (raw bytes). Returns the decoded path bytes,
/// throwing the same scheme/host/encoded-slash errors as Node. When `windows`
/// is true, applies Node's Win32 conversion (UNC hosts, drive-letter
/// validation, `/`→`\`, encoded-`\` rejection) instead of the POSIX path
/// (which still rejects non-empty/non-localhost hosts on darwin). #2975
fn file_url_to_path_bytes(url_f64: f64, windows: bool) -> Vec<u8> {
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

    let (host, pathname) = if let Some(authority_and_path) = after_scheme.strip_prefix("//") {
        let path_start = authority_and_path
            .find('/')
            .unwrap_or(authority_and_path.len());
        (
            &authority_and_path[..path_start],
            &authority_and_path[path_start..],
        )
    } else {
        ("", after_scheme)
    };
    let pathname = pathname.split(['?', '#']).next().unwrap_or_default();

    if windows {
        // Win32: a non-empty/non-localhost host is a UNC share `\\host\path`.
        let mut decoded = decode_file_url_pathname_bytes_win32(pathname);
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            let mut out = b"\\\\".to_vec();
            out.extend_from_slice(host.as_bytes());
            out.extend_from_slice(&decoded);
            return out;
        }
        // No host: pathname must be `/<drive-letter>:...`. Node validates the
        // decoded form: position 1 is an ASCII letter, position 2 is `:`.
        let letter = decoded.get(1).copied().unwrap_or(0) | 0x20;
        let sep = decoded.get(2).copied().unwrap_or(0);
        if !(b'a'..=b'z').contains(&letter) || sep != b':' {
            throw_url_type_error_with_code(
                "File URL path must be absolute",
                "ERR_INVALID_FILE_URL_PATH",
            );
        }
        // Strip the leading `\` (was `/`) so `\C:\x` → `C:\x`.
        decoded.remove(0);
        decoded
    } else {
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            throw_url_type_error_with_code(
                "File URL host must be \"localhost\" or empty on darwin",
                "ERR_INVALID_FILE_URL_HOST",
            );
        }
        decode_file_url_pathname_bytes_posix(pathname)
    }
}

/// Resolve a `node:module` "base" argument (file URL object/string or a
/// bare path string) to a filesystem path string. URL-shaped values and
/// `file:`-scheme strings go through the file-URL decoder; any other string
/// is treated as a path and returned verbatim. Used by
/// `module.findPackageJSON` (#3120). Returns `None` for non-string,
/// non-URL-object values so the caller can raise `ERR_INVALID_ARG_TYPE`.
pub(crate) fn module_base_to_path(base_f64: f64) -> Option<String> {
    if is_js_string_value(base_f64) {
        let s = string_from_js_value(base_f64);
        if s.starts_with("file:") {
            return Some(
                String::from_utf8_lossy(&file_url_to_path_bytes(base_f64, false)).into_owned(),
            );
        }
        return Some(s);
    }
    if let Some(obj) = object_from_f64(base_f64) {
        if is_url_object_shape(obj) {
            return Some(
                String::from_utf8_lossy(&file_url_to_path_bytes(base_f64, false)).into_owned(),
            );
        }
    }
    None
}

/// Convert a file:// URL to a filesystem path
/// Strips the "file://" prefix and percent-decodes the result
/// js_url_file_url_to_path(url_f64: f64, options_f64: f64) -> f64 (NaN-boxed string)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path(url_f64: f64, options_f64: f64) -> f64 {
    let windows = options_windows_flag(options_f64);
    let decoded = String::from_utf8_lossy(&file_url_to_path_bytes(url_f64, windows)).into_owned();
    create_string_f64(&decoded)
}

/// `url.fileURLToPathBuffer(url[, options])` (#2541) — the Buffer-returning
/// counterpart to `fileURLToPath`. Returns the decoded path's raw bytes as a
/// `Buffer`, preserving percent-encoded sequences that are not valid UTF-8
/// (where the string form would lossily substitute U+FFFD). Same scheme/host
/// validation as `fileURLToPath`.
/// js_url_file_url_to_path_buffer(url_f64: f64, options_f64: f64) -> f64 (NaN-boxed Buffer ptr)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path_buffer(url_f64: f64, options_f64: f64) -> f64 {
    let windows = options_windows_flag(options_f64);
    let bytes = file_url_to_path_bytes(url_f64, windows);
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

/// Percent-encode a file-URL path component (after separator normalization),
/// keeping the WHATWG path-safe set plus `/` and `:` (drive-letter colon).
fn encode_file_url_path(path: &str) -> String {
    let mut encoded = String::new();
    for b in path.bytes() {
        match b {
            b'/' | b':' => encoded.push(b as char),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char)
            }
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    encoded
}

#[no_mangle]
pub extern "C" fn js_url_path_to_file_url(path_f64: f64, options_f64: f64) -> f64 {
    if !is_js_string_value(path_f64) {
        throw_invalid_url_arg(path_f64, false);
    }
    let path = get_string_content(path_f64);
    let windows = options_windows_flag(options_f64);

    let href = if windows {
        // Win32 (#2975). UNC paths (`\\host\share\...`) become
        // `file://host/share/...`; everything else is a (drive-letter) path
        // with `\` separators rewritten to `/`.
        if let Some(unc) = path.strip_prefix("\\\\") {
            // First segment after `\\` is the host; the remainder is the path.
            let (host, rest) = match unc.find('\\') {
                Some(idx) => (&unc[..idx], &unc[idx..]),
                None => (unc, ""),
            };
            let rest_fwd = rest.replace('\\', "/");
            format!("file://{}{}", host, encode_file_url_path(&rest_fwd))
        } else {
            let fwd = path.replace('\\', "/");
            let encoded = encode_file_url_path(&fwd);
            if encoded.starts_with('/') {
                format!("file://{}", encoded)
            } else {
                format!("file:///{}", encoded)
            }
        }
    } else {
        let resolved = resolve_path_to_file_url_posix(&path);
        let encoded = encode_file_url_path(&resolved);
        if encoded.starts_with('/') {
            format!("file://{}", encoded)
        } else {
            format!("file:///{}", encoded)
        }
    };
    let obj = create_url_object(&href);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `url.domainToASCII(domain)` (#3059). Node Web-IDL-stringifies the argument
/// (`String(domain)`; a Symbol throws TypeError) and runs the full WHATWG host
/// parser, so a numeric / IPv4-shorthand domain canonicalizes to a dotted-quad
/// IPv4 address (`123` → `"0.0.0.123"`, `0x7f.1` → `"127.0.0.1"`) rather than
/// being treated as a literal label. Unparsable hosts yield `""`.
#[no_mangle]
pub extern "C" fn js_url_domain_to_ascii(input_f64: f64) -> f64 {
    let input = string_from_header(js_url_coerce_string(input_f64));
    if input.chars().any(|c| c.is_ascii_whitespace()) {
        return create_string_f64("");
    }
    // `whatwg_canonicalize_host` runs IDNA *and* the WHATWG numeric/IPv4 host
    // parser, matching Node's `domainToASCII` exactly (IDN → punycode, numeric
    // → IPv4, invalid → None → ""). It supersedes the bare `idna::domain_to_ascii`.
    let out = whatwg_canonicalize_host(&input).unwrap_or_default();
    create_string_f64(&out)
}

/// `url.domainToUnicode(domain)` (#3059). Mirrors `domainToASCII`'s coercion
/// and WHATWG host parsing, but returns the Unicode IDN form. For numeric /
/// IPv4-shorthand hosts Node returns the canonical IPv4 address (`123` →
/// `"0.0.0.123"`); for registrable hostnames it returns the decoded Unicode
/// (`xn--mnchen-3ya.de` → `münchen.de`); invalid hosts yield `""`.
#[no_mangle]
pub extern "C" fn js_url_domain_to_unicode(input_f64: f64) -> f64 {
    let input = string_from_header(js_url_coerce_string(input_f64));
    if input.chars().any(|c| c.is_ascii_whitespace()) {
        return create_string_f64("");
    }
    let out = match whatwg_canonicalize_host(&input) {
        // Out-of-range / unparsable host → "" (matches Node).
        None => String::new(),
        // Numeric / IPv4-shorthand → canonical IPv4 address (Node yields the IP).
        Some(canon) if is_ipv4_host(&canon) => canon,
        // Registrable hostname → Unicode IDN form.
        Some(_) => idna::domain_to_unicode(&input).0,
    };
    create_string_f64(&out)
}

fn json_to_value(json: serde_json::Value) -> f64 {
    let s = json.to_string();
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    unsafe { f64::from_bits(crate::json::js_json_parse(ptr).bits()) }
}

fn null_f64() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

fn bool_f64(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `url.urlToHttpOptions(url)` (#2976). Mirrors Node's shape exactly:
///
/// ```js
/// const options = {
///   __proto__: null,
///   ...url,                 // copy user-added enumerable own props first
///   protocol, hostname, hash, search, pathname,
///   path: `${pathname}${search}`, href,
/// };
/// if (port !== '') options.port = Number(port);
/// if (username || password) options.auth = `${decode(username)}:${decode(password)}`;
/// ```
///
/// Node throws `ERR_INVALID_ARG_TYPE` for non-object input rather than
/// returning an empty object. `auth` is percent-decoded; `port` is numeric.
#[no_mangle]
pub extern "C" fn js_url_to_http_options(url_f64: f64) -> f64 {
    let Some(obj) = object_from_f64(url_f64) else {
        // Node: `if (url == null || typeof url !== 'object')` → ERR_INVALID_ARG_TYPE.
        let message = format!(
            "The \"url\" argument must be of type object. Received {}",
            url_received(url_f64)
        );
        throw_url_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };

    // Standard URL component names. In Node these live on the URL prototype as
    // getters, so the `...url` spread copies only *user-added* own props. Perry
    // stores them as own fields, so we must skip them when replicating the
    // spread — otherwise we'd duplicate them (out of order) ahead of the fixed set.
    const URL_OWN: &[&str] = &[
        "href",
        "protocol",
        "host",
        "hostname",
        "port",
        "pathname",
        "search",
        "hash",
        "origin",
        "searchParams",
        "username",
        "password",
    ];

    let protocol = object_prop_string(obj, "protocol");
    let hostname = object_prop_string(obj, "hostname");
    let hash = object_prop_string(obj, "hash");
    let search = object_prop_string(obj, "search");
    let pathname = object_prop_string(obj, "pathname");
    let port_s = object_prop_string(obj, "port");
    let href = object_prop_string(obj, "href");
    let username = object_prop_string(obj, "username");
    let password = object_prop_string(obj, "password");
    let path = format!("{}{}", pathname, search);

    let obj_out = js_object_alloc(0, 0);

    // 1) Copy user-added enumerable own props (`...url`) first, in insertion
    //    order, skipping the standard URL component names.
    let keys = crate::object::js_object_keys(obj as *const ObjectHeader);
    let len = unsafe { (*keys).length };
    for i in 0..len {
        let key_f = crate::array::js_array_get_f64(keys, i);
        let key = get_string_content(key_f);
        if URL_OWN.contains(&key.as_str()) {
            continue;
        }
        let val = object_prop_f64(obj, &key);
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::js_object_set_field_by_name(obj_out, key_ptr, val);
    }

    // 2) Fixed fields, in Node's order.
    set_named(obj_out, "protocol", create_string_f64(&protocol));
    set_named(obj_out, "hostname", create_string_f64(&hostname));
    set_named(obj_out, "hash", create_string_f64(&hash));
    set_named(obj_out, "search", create_string_f64(&search));
    set_named(obj_out, "pathname", create_string_f64(&pathname));
    set_named(obj_out, "path", create_string_f64(&path));
    set_named(obj_out, "href", create_string_f64(&href));

    // 3) Optional numeric `port` when non-empty.
    if !port_s.is_empty() {
        if let Ok(p) = port_s.parse::<u32>() {
            set_named(obj_out, "port", p as f64);
        }
    }

    // 4) Optional decoded `auth` when userinfo present. Node uses
    //    `decodeURIComponent` on each half (`u%20ser` → `u ser`, `p%40w` → `p@w`).
    if !username.is_empty() || !password.is_empty() {
        let auth = format!("{}:{}", url_decode(&username), url_decode(&password));
        set_named(obj_out, "auth", create_string_f64(&auth));
    }

    crate::value::js_nanbox_pointer(obj_out as i64)
}

/// Set an own property by name on a dynamically-grown object (no fixed key
/// array). Used by `urlToHttpOptions` where the field set is variable.
fn set_named(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key_ptr, value);
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

const LEGACY_URL_KEYS: [&str; 12] = [
    "protocol", "slashes", "auth", "host", "port", "hostname", "hash", "search", "query",
    "pathname", "path", "href",
];

fn string_or_null(value: String) -> f64 {
    if value.is_empty() {
        null_f64()
    } else {
        create_string_f64(&value)
    }
}

fn create_legacy_url_object(values: [f64; 12]) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, LEGACY_URL_KEYS.len() as u32);
    let mut keys = js_array_alloc(LEGACY_URL_KEYS.len() as u32);
    for (index, key) in LEGACY_URL_KEYS.iter().enumerate() {
        keys = js_array_push_f64(keys, create_string_f64(key));
        js_object_set_field_f64(obj, index as u32, values[index]);
    }
    js_object_set_keys(obj, keys);
    obj
}

#[no_mangle]
pub extern "C" fn js_url_legacy_url_new() -> f64 {
    let obj = create_legacy_url_object([
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
    ]);
    crate::value::js_nanbox_pointer(obj as i64)
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

    let mut invalid_percent_host = false;
    if let Some(percent_idx) = host.find('%') {
        invalid_percent_host = true;
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
    let raw_query = search.strip_prefix('?').unwrap_or(&search).to_string();
    let query = if parse_qs {
        let mut map = serde_json::Map::new();
        for part in raw_query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            map.insert(url_decode(k), serde_json::Value::String(url_decode(v)));
        }
        json_to_value(serde_json::Value::Object(map))
    } else if raw_query.is_empty() {
        null_f64()
    } else {
        create_string_f64(&raw_query)
    };
    let protocol_value = if protocol_is_null || protocol.is_empty() {
        null_f64()
    } else {
        create_string_f64(&protocol)
    };
    let slashes = if protocol_null_or_slashes(&s, protocol_is_null, &host) {
        bool_f64(true)
    } else {
        null_f64()
    };
    let path = format!("{}{}", pathname, search);
    let path_value = string_or_null(path);
    let href_value = create_string_f64(&s);
    let host_value = if invalid_percent_host {
        create_string_f64(&host)
    } else {
        string_or_null(host)
    };
    let hostname_value = if invalid_percent_host {
        create_string_f64(&hostname)
    } else {
        string_or_null(hostname)
    };
    let obj = create_legacy_url_object([
        protocol_value,
        slashes,
        string_or_null(url_decode(&auth)),
        host_value,
        string_or_null(port),
        hostname_value,
        string_or_null(hash),
        string_or_null(search),
        query,
        string_or_null(pathname),
        path_value,
        href_value,
    ]);
    crate::value::js_nanbox_pointer(obj as i64)
}

fn protocol_null_or_slashes(input: &str, protocol_is_null: bool, host: &str) -> bool {
    protocol_is_null || input.starts_with("//") || input.contains("://") || !host.is_empty()
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

#[no_mangle]
pub extern "C" fn js_url_legacy_resolve_object(from: f64, to: f64) -> f64 {
    let resolved = js_url_legacy_resolve(from, to);
    js_url_legacy_parse(resolved, bool_f64(false), bool_f64(false))
}
