//! Client factory overload normalization for `http.request` /
//! `http.get` / `https.request` / `https.get` (#3226 / #3227 / #3228).
//!
//! Codegen packs every user argument into a single JS array
//! (`NA_VARARGS`); these helpers resolve `(url, options, callback)` by
//! value type so all Node overloads work: `(url[, cb])`,
//! `(options[, cb])`, and `(url, options[, cb])`. Extracted from
//! `lib.rs` to keep that file under the 2000-line lint cap.

use std::collections::HashMap;

use perry_ffi::{ArrayHeader, Handle, JsValue};

use super::{
    agent, extract_string_value, headers_from_options, is_string_value, method_from_options,
    parse_options_object, timeout_from_options, url_from_options, PTR_MASK, TAG_UNDEFINED,
};

// ------------------------------------------------------------------
// Client factory overload normalization (#3226 / #3227 / #3228)
// ------------------------------------------------------------------

/// Resolved positional arguments for `request()` / `get()` after Node's
/// type-directed overload handling. Mirrors `parse_listen_args` in
/// `perry-ext-http-server`: codegen packs every user argument into a
/// single JS array (`NA_VARARGS`) and we resolve each slot by value
/// type so the callback is picked up wherever it floats.
pub(crate) struct ClientArgs {
    /// First string argument — the URL (`request(url, ...)` /
    /// `get(url, ...)`). `undefined` when the caller passed only an
    /// options object.
    pub(crate) url: f64,
    /// First object argument — the options bag (`request(options, ...)`
    /// or the options half of `request(url, options, ...)`). `undefined`
    /// when the caller passed only a URL.
    pub(crate) opts: f64,
    /// Raw `*const ClosureHeader` pointer for the response callback, or
    /// `0` when no function argument was supplied.
    pub(crate) callback: i64,
}

/// Normalize the JS-side `http.request(...)` / `http.get(...)` argument
/// array into `(url, options, callback)`. Accepts every Node overload:
/// `(url[, cb])`, `(options[, cb])`, and `(url, options[, cb])`. The
/// single function argument is the response callback regardless of
/// position; the first string is the URL; the first object is the
/// options bag.
///
/// # Safety
/// `args_array` must be `0`/null or a valid Perry-runtime `ArrayHeader`.
pub(crate) unsafe fn parse_client_args(args_array: i64) -> ClientArgs {
    let mut out = ClientArgs {
        url: f64::from_bits(TAG_UNDEFINED),
        opts: f64::from_bits(TAG_UNDEFINED),
        callback: 0,
    };
    let arr_ptr = args_array as *const ArrayHeader;
    if arr_ptr.is_null() {
        return out;
    }
    // Codegen passes a clean raw pointer; reject a stray NaN-boxed value
    // rather than dereferencing tag bits as an address.
    if (args_array as u64) >> 48 != 0 {
        return out;
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    for i in 0..len {
        let bits = *elements.add(i);
        // The response callback is the (single) function argument — match
        // it by value type, not position.
        if js_value_is_closure(bits as i64) != 0 {
            out.callback = (bits & PTR_MASK) as i64;
            continue;
        }
        let f = f64::from_bits(bits);
        let v = JsValue::from_bits(bits);
        if is_string_value(f) {
            if out.url.to_bits() == TAG_UNDEFINED {
                out.url = f;
            }
        } else if !v.is_undefined() && !v.is_null() && v.is_pointer() {
            // First non-string, non-callback pointer → options object.
            // (A bare `URL` instance also lands here; `url_from_options`
            // reads its `protocol`/`host`/`pathname` via JSON-stringify.)
            if out.opts.to_bits() == TAG_UNDEFINED {
                out.opts = f;
            }
        }
    }
    out
}

extern "C" {
    /// `crates/perry-runtime/src/closure/dynamic_props.rs::js_value_is_closure`.
    fn js_value_is_closure(value_bits: i64) -> i32;
}

/// Merge a URL string with an options object into the request fields.
/// The URL supplies protocol / host / port / path; the options bag
/// overrides method, headers, timeout, agent, and any explicitly-set
/// protocol / host / port / path. When `url_f64` is `undefined`, this
/// degenerates to the options-only path; when `opts_f64` is
/// `undefined`, to the URL-only path.
pub(crate) unsafe fn merge_url_and_options(
    url_f64: f64,
    opts_f64: f64,
    default_protocol: &str,
) -> (String, HashMap<String, String>, Option<u64>, Handle) {
    let opts = parse_options_object(opts_f64);
    let has_opts = opts.is_some();
    let opts = opts.unwrap_or(serde_json::Value::Null);

    let url = if is_string_value(url_f64) {
        let raw = extract_string_value(url_f64).unwrap_or_default();
        let base = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw
        } else if !raw.is_empty() {
            format!("{}://{}", default_protocol, raw)
        } else {
            String::new()
        };
        // When both a URL and an options object are given, options fields
        // (protocol/host/port/path) override the URL-derived parts. Node
        // merges this way; we mirror it by re-parsing the URL and letting
        // present option keys win.
        if has_opts {
            merge_options_onto_url(&base, &opts, default_protocol)
        } else {
            base
        }
    } else {
        url_from_options(&opts, default_protocol)
    };

    let headers = if has_opts {
        headers_from_options(&opts)
    } else {
        HashMap::new()
    };
    let timeout = if has_opts {
        timeout_from_options(&opts)
    } else {
        None
    };
    let agent_handle = if has_opts {
        agent::agent_handle_from_options(opts_f64).unwrap_or(0)
    } else {
        0
    };

    (url, headers, timeout, agent_handle)
}

/// Rebuild a URL string, overriding its parts with any present option
/// keys (`protocol` / `hostname` / `host` / `port` / `path`). Used for
/// the `request(url, options, cb)` overload.
fn merge_options_onto_url(
    base_url: &str,
    opts: &serde_json::Value,
    default_protocol: &str,
) -> String {
    let parsed = reqwest::Url::parse(base_url).ok();

    let protocol = opts
        .get("protocol")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches(':').to_string())
        .or_else(|| parsed.as_ref().map(|p| p.scheme().to_string()))
        .unwrap_or_else(|| default_protocol.to_string());

    let raw_host = opts
        .get("hostname")
        .and_then(|v| v.as_str())
        .or_else(|| opts.get("host").and_then(|v| v.as_str()))
        .map(|s| s.split(':').next().unwrap_or(s).to_string())
        .or_else(|| parsed.as_ref().and_then(|p| p.host_str().map(String::from)))
        .unwrap_or_else(|| "localhost".to_string());

    let port = opts
        .get("port")
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .or_else(|| v.as_u64().map(|n| n.to_string()))
                .or_else(|| v.as_f64().map(|n| (n as u64).to_string()))
        })
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|p| p.port().map(|n| n.to_string()))
        });

    let path = opts
        .get("path")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            parsed.as_ref().map(|p| {
                let mut s = p.path().to_string();
                if let Some(q) = p.query() {
                    s.push('?');
                    s.push_str(q);
                }
                if s.is_empty() {
                    s.push('/');
                }
                s
            })
        })
        .unwrap_or_else(|| "/".to_string());

    match port {
        Some(p) if !p.is_empty() => format!("{}://{}:{}{}", protocol, raw_host, p, path),
        _ => format!("{}://{}{}", protocol, raw_host, path),
    }
}

/// Resolve the request method for the merged overload: `force_get`
/// (the `get()` factories) always yields `GET`; otherwise the options
/// `method` field, defaulting to `GET`.
pub(crate) unsafe fn method_for_overload(opts_f64: f64, force_get: bool) -> String {
    if force_get {
        return "GET".to_string();
    }
    match parse_options_object(opts_f64) {
        Some(opts) => method_from_options(&opts),
        None => "GET".to_string(),
    }
}
