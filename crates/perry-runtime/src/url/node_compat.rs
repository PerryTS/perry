//! Node.js URL-module compatibility helpers — `fileURLToPath`,
//! `pathToFileURL`, `domainToASCII`, `urlToHttpOptions`, legacy
//! `url.format` / `url.parse` / `url.resolve`.

use super::*;

use super::parse::{create_url_object, is_valid_absolute_url, parse_url, resolve_url};
use super::search_params::{url_decode, url_encode};

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

#[no_mangle]
pub extern "C" fn js_url_path_to_file_url(path_f64: f64) -> f64 {
    let path = get_string_content(path_f64);
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
    let input = get_string_content(input_f64);
    let out = idna::domain_to_ascii(&input).unwrap_or_else(|_| String::new());
    create_string_f64(&out)
}

#[no_mangle]
pub extern "C" fn js_url_domain_to_unicode(input_f64: f64) -> f64 {
    let input = get_string_content(input_f64);
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
                    url_encode(&key),
                    url_encode(&get_string_content(val))
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
        return create_string_f64("");
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
pub extern "C" fn js_url_legacy_parse(input: f64, parse_query_string: f64) -> f64 {
    let s = get_string_content(input);
    let (protocol, mut host, mut hostname, port, pathname, search, hash) = parse_url(&s);
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
    let parse_qs = parse_query_string.to_bits() == 0x7FFC_0000_0000_0004u64;
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
    json_to_value(serde_json::json!({
        "protocol": protocol,
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
