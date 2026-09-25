//! `ServerResponse` — the Node.js Writable stream returned to a
//! `(req, res) => …` handler. The response buffers chunks until `.end()`
//! (or streams them once the head has been flushed), and the turnloop
//! connection that decoded the request encodes and writes it
//! (`turnloop_route`).

use std::collections::HashMap;

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::server::request::handle_to_pointer_f64;
use crate::server::response_end::call_closure0;
use crate::server::types::{
    js_json_stringify, js_node_setheaders_entries_json, js_value_is_closure, jsvalue_to_body_bytes,
    jsvalue_to_owned_string, read_string_header, PTR_MASK, STRING_TAG, TAG_FALSE, TAG_NULL,
    TAG_TRUE, TAG_UNDEFINED,
};

/// Node's default `highWaterMark` for an HTTP `OutgoingMessage` (16 KiB).
/// `res.write()` returns `false` once the buffered body grows past this,
/// signalling backpressure so producer loops (`while (res.write(buf))`)
/// terminate instead of spinning forever (#4909).
const DEFAULT_HIGH_WATER_MARK: usize = 16 * 1024;

// ------------------------------------------------------------------
// #4907 — Node-compatible header / argument validation.
//
// `res.setHeader` / `res.removeHeader` throw after headers are sent, and
// `setHeader` rejects non-token field names. `res.writeEarlyHints` validates
// its `hints` argument. Each throws an `ERR_*`-coded error that unwinds back
// to the JS handler frame.
// ------------------------------------------------------------------

/// Node HTTP token bytes (`tchar`, mirrored from `lib/_http_common.js`).
fn http_is_token_byte(b: u8) -> bool {
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
        | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
}

fn http_is_valid_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(http_is_token_byte)
}

/// Returns whether the response's headers have already been flushed. A throw
/// must fire even when the handle has gone away, so callers check this before
/// touching state.
fn response_headers_sent(handle: i64) -> bool {
    get_handle::<ServerResponse>(handle)
        .map(|sr| sr.headers_sent)
        .unwrap_or(false)
}

/// `lib/internal/http.js` link-header format: a `<uri>` followed by at least
/// one `;`-separated parameter. Faithful enough for the invalid cases the
/// corpus exercises (`'</>; '`, `'rel=preload; </scripts.js>'`,
/// `'invalid string'`).
fn is_valid_link_header(value: &str) -> bool {
    let s = value.trim();
    if !s.starts_with('<') {
        return false;
    }
    let close = match s.find('>') {
        Some(i) => i,
        None => return false,
    };
    let rest = s[close + 1..].trim_start();
    if !rest.starts_with(';') {
        return false;
    }
    let params = rest[1..].trim();
    if params.is_empty() {
        return false;
    }
    params.split(';').all(|p| !p.trim().is_empty())
}

/// Per-request handle backing `ServerResponse` JS-side.
pub struct ServerResponse {
    pub status_code: u16,
    pub status_message: Option<String>,
    /// Lowercase-keyed header map (the lookup table). For array-valued
    /// headers this holds Node's scalar coercion (the array's elements
    /// joined with `, `); the per-element values live in
    /// `header_value_lists` so the wire layer can emit one line each.
    pub headers: HashMap<String, String>,
    /// Lowercase-keyed multi-value header map. Populated when a header is
    /// assigned an array value (e.g. `Set-Cookie`): Node emits one header
    /// line per element rather than a single comma-joined line, so the wire
    /// serializer expands these into repeated `name: value` lines (#4826).
    pub header_value_lists: HashMap<String, Vec<String>>,
    /// Lowercase-keyed trailer map for HTTP trailers emitted after the
    /// response body, per Node's `ServerResponse.addTrailers` contract.
    pub trailers: HashMap<String, String>,
    /// Lowercase → original-case map so `getHeaderNames()` returns
    /// what the user originally set (matches Node behavior).
    pub raw_header_names: HashMap<String, String>,
    /// Lowercase header names in first-insertion order. The lookup maps above
    /// intentionally stay hash-based; this side list preserves Node's wire
    /// ordering for `writeHead({...})` and sequential `setHeader` calls.
    pub header_order: Vec<String>,
    pub raw_trailer_names: HashMap<String, String>,
    pub headers_sent: bool,
    /// True once `writeHead()` has committed the status line + headers (Node's
    /// `_header`). Distinct from `headers_sent` (the wire flush, set at
    /// `write`/`end`) so the normal deferred-send path is unaffected, but a
    /// post-`writeHead` `setHeaders()` still throws `ERR_HTTP_HEADERS_SENT`
    /// like Node (#4965).
    pub header_committed: bool,
    pub writable_ended: bool,
    pub writable_finished: bool,
    pub send_date: bool,
    pub strict_content_length: bool,
    pub req_handle: i64,
    /// True for direct `new http.OutgoingMessage()` handles. They share the
    /// outgoing header/writable surface but are not a live ServerResponse.
    pub outgoing_message_only: bool,
    /// Body chunks accumulated by `.write(chunk)` calls. Assembled
    /// + flushed when `.end()` is called.
    pub buffered_body: Vec<u8>,
    /// True after a streaming `res.write()` returned `false`; the pump
    /// fires `'drain'` (once) when the socket's queued bytes drop below the
    /// HWM.
    pub needs_drain: bool,
    /// Event-name → list of registered listener closure pointers.
    pub listeners: HashMap<String, Vec<i64>>,
    /// Event-name → one-shot (`res.once(event, cb)`) listener closure
    /// pointers. Drained by every event-consumption site after firing so a
    /// `once` listener fires exactly once — Node's `EventEmitter.once`
    /// contract. Kept separate from `listeners` so persistent `on` listeners
    /// survive the drain. The critical caller is Perry's own `createReadStream`
    /// → `.pipe(res)` pump, which re-arms `res.once('drain')` after each
    /// backpressure pause; without one-shot semantics the pump stalls after the
    /// first 64 KB chunk (static files truncate).
    pub once_listeners: HashMap<String, Vec<i64>>,
    /// #4904: true for `new http.ServerResponse(req)` instances (and any
    /// response wired through `assignSocket`) — `.end()` flushes through
    /// `standalone_socket` instead of a connection.
    pub standalone: bool,
    /// #4904: the JS Writable assigned via `res.assignSocket(socket)`.
    /// `TAG_UNDEFINED` while unassigned.
    pub standalone_socket: f64,
    /// #4904: `req.method` captured from the standalone constructor's
    /// request argument — `HEAD` suppresses the body on flush.
    pub standalone_req_method: Option<String>,
    /// #4904: `res.write(chunk, cb)` callbacks, invoked in order when the
    /// buffered body flushes on `.end()`.
    pub pending_write_callbacks: Vec<i64>,
    /// #4975: `outgoingMessage.destroy()` state. Node's `OutgoingMessage`
    /// (and its `ServerResponse` subclass) exposes a `destroyed` getter that
    /// flips `true` after `destroy()`, and a post-destroy `write(chunk, cb)`
    /// invokes `cb` with an `ERR_STREAM_DESTROYED` error instead of buffering.
    pub destroyed: bool,
    /// P5: the turnloop connection this response writes to, and the request
    /// ordinal it answers (the HTTP/2 stream id on an HTTP/2 connection). The
    /// handler, the codec and the socket are all on the same thread, so
    /// `res.end()` encodes and submits the response itself. `seq` is what
    /// keeps a late `res.end()` from writing onto the connection's *next*
    /// request after the first one was destroyed. `None` for a standalone
    /// `new http.ServerResponse(req)` and a bare `OutgoingMessage`.
    pub turnloop: Option<(i64, u64)>,
    /// P5: the head has gone out and further writes stream straight to the
    /// socket.
    pub turnloop_streaming: bool,
}

/// Owned shape produced by `.end()` (or by `begin_streaming` for the head
/// alone) and handed to the connection's encoder.
pub struct ResponseShape {
    pub status: u16,
    pub status_message: Option<String>,
    pub headers: Vec<(String, String)>,
    pub trailers: Vec<(String, String)>,
    /// The fully buffered body; empty for a head sent ahead of a streamed
    /// body.
    pub body: Vec<u8>,
    /// True when Perry synthesized Content-Length for a fully buffered body,
    /// rather than the application setting it explicitly.
    pub auto_content_length: bool,
}

impl ServerResponse {
    pub fn new() -> Self {
        Self {
            status_code: 200,
            status_message: None,
            headers: HashMap::new(),
            header_value_lists: HashMap::new(),
            trailers: HashMap::new(),
            raw_header_names: HashMap::new(),
            header_order: Vec::new(),
            header_committed: false,
            raw_trailer_names: HashMap::new(),
            headers_sent: false,
            writable_ended: false,
            writable_finished: false,
            send_date: true,
            strict_content_length: false,
            req_handle: 0,
            outgoing_message_only: false,
            buffered_body: Vec::new(),
            needs_drain: false,
            listeners: HashMap::new(),
            once_listeners: HashMap::new(),
            standalone: false,
            standalone_socket: f64::from_bits(TAG_UNDEFINED),
            standalone_req_method: None,
            pending_write_callbacks: Vec::new(),
            destroyed: false,
            turnloop: None,
            turnloop_streaming: false,
        }
    }

    pub fn outgoing_message() -> Self {
        let mut response = Self::new();
        response.send_date = false;
        response.outgoing_message_only = true;
        response
    }

    pub fn with_request_handle(mut self, req_handle: i64) -> Self {
        self.req_handle = req_handle;
        self
    }

    /// Snapshot the current header map as `Vec<(orig_name, value)>`
    /// preserving original case. Array-valued headers (tracked in
    /// `header_value_lists`) expand to one entry per element so the wire
    /// layer emits a separate header line each (#4826).
    pub fn snapshot_headers(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.headers.len());
        let mut ordered = self.header_order.clone();
        for lower in self.headers.keys() {
            if !ordered.contains(lower) {
                ordered.push(lower.clone());
            }
        }
        for lower_k in &ordered {
            let Some(v) = self.headers.get(lower_k) else {
                continue;
            };
            let orig = self
                .raw_header_names
                .get(lower_k)
                .cloned()
                .unwrap_or_else(|| lower_k.clone());
            if let Some(values) = self.header_value_lists.get(lower_k) {
                for elem in values {
                    out.push((orig.clone(), elem.clone()));
                }
            } else {
                out.push((orig, v.clone()));
            }
        }
        if self.send_date && !self.headers.contains_key("date") {
            out.push((
                "Date".to_string(),
                httpdate::fmt_http_date(std::time::SystemTime::now()),
            ));
        }
        out
    }

    fn remember_header(&mut self, lower: &str) {
        if !self.header_order.iter().any(|name| name == lower) {
            self.header_order.push(lower.to_string());
        }
    }

    pub(crate) fn snapshot_trailers(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.trailers.len());
        for (lower_k, v) in &self.trailers {
            let orig = self
                .raw_trailer_names
                .get(lower_k)
                .cloned()
                .unwrap_or_else(|| lower_k.clone());
            out.push((orig, v.clone()));
        }
        out
    }

    /// Auto-fill `Content-Length` if unset and we know the full body.
    pub(crate) fn ensure_content_length(&mut self) {
        // A response with trailers must not declare a fixed Content-Length:
        // the body length alone doesn't bound the response (trailing headers
        // still follow), and some clients/proxies treat a present
        // Content-Length as "body complete, no trailers expected".
        if !self.trailers.is_empty() {
            return;
        }
        if !self.headers.contains_key("content-length")
            && !self.headers.contains_key("transfer-encoding")
        {
            self.remember_header("content-length");
            let len = self.buffered_body.len();
            self.headers
                .insert("content-length".to_string(), len.to_string());
            self.raw_header_names
                .insert("content-length".to_string(), "Content-Length".to_string());
        }
    }
}

// ============================================================================
// FFI surface
// ============================================================================

/// `res.statusCode = N` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_status(handle: i64, code: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent && code.is_finite() && code > 0.0 {
            sr.status_code = code as u16;
        }
    }
}

/// `res.statusCode` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_status(handle: i64) -> f64 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| {
            if sr.outgoing_message_only {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                sr.status_code as f64
            }
        })
        .unwrap_or(200.0)
}

/// `res.statusMessage = "..."` setter.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_status_message(
    handle: i64,
    msg_ptr: *const StringHeader,
) {
    let msg = read_string_header(msg_ptr as *mut _);
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.status_message = msg;
        }
    }
}

/// `res.setHeader(name, value)`. `value` arrives as a raw NaN-boxed
/// JSValue (`NA_F64`) so array values (e.g. `Set-Cookie`) can be detected
/// and stored as a per-element list — Node emits one header line per array
/// element rather than a single comma-joined / JSON-stringified line
/// (#4826). Scalar values are coerced to a string as before.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) {
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    // #4907 — Node's `OutgoingMessage.setHeader` throws if headers are already
    // sent, then validates the field name, before touching any state.
    if response_headers_sent(handle) {
        perry_ffi::throw_with_code(
            "Cannot set headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    if name.is_empty() {
        return;
    }
    if !http_is_valid_token(&name) {
        perry_ffi::throw_with_code(
            &format!("Header name must be a valid HTTP token [\"{name}\"]"),
            "ERR_INVALID_HTTP_TOKEN",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    let lower = name.to_lowercase();

    // Detect an array value via JSON: an array serializes to `[ … ]` and
    // parses back to `serde_json::Value::Array`. Anything else is coerced
    // to its string form (matching the previous `NA_STR` behavior).
    let jsv = JsValue::from_bits(value.to_bits());
    let array_elems: Option<Vec<String>> = if jsv.is_pointer() {
        let ptr = js_json_stringify(value, 0);
        if ptr.is_null() {
            None
        } else {
            read_string_header(ptr).and_then(|json| {
                match serde_json::from_str::<serde_json::Value>(&json) {
                    Ok(serde_json::Value::Array(items)) => Some(
                        items
                            .into_iter()
                            .map(|item| match item {
                                serde_json::Value::String(s) => s,
                                other => other.to_string(),
                            })
                            .collect(),
                    ),
                    _ => None,
                }
            })
        }
    } else {
        None
    };

    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.remember_header(&lower);
            if let Some(elems) = array_elems {
                sr.headers.insert(lower.clone(), elems.join(", "));
                sr.header_value_lists.insert(lower.clone(), elems);
            } else {
                sr.headers.insert(
                    lower.clone(),
                    jsvalue_to_owned_string(value).unwrap_or_default(),
                );
                sr.header_value_lists.remove(&lower);
            }
            sr.raw_header_names.insert(lower, name);
        }
    }
}

/// `res.setHeader(name, value)` chainable wrapper for static dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header_self(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) -> i64 {
    js_node_http_res_set_header(handle, name_ptr, value);
    handle
}

/// `res.getHeader(name)` — case-insensitive lookup. Returns `null`
/// when the header isn't set.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_get_header(
    handle: i64,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if let Some(v) = sr.headers.get(&name) {
            let header = alloc_string(v);
            return f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK));
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `res.removeHeader(name)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_remove_header(
    handle: i64,
    name_ptr: *const StringHeader,
) {
    // #4907 — Node's `OutgoingMessage.removeHeader` throws once headers are
    // sent (distinct "remove" wording from `setHeader`).
    if response_headers_sent(handle) {
        perry_ffi::throw_with_code(
            "Cannot remove headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return,
    };
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.headers.remove(&name);
            sr.header_value_lists.remove(&name);
            sr.raw_header_names.remove(&name);
            sr.header_order.retain(|header| header != &name);
        }
    }
}

/// `res.hasHeader(name)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_has_header(
    handle: i64,
    name_ptr: *const StringHeader,
) -> i32 {
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return 0,
    };
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if sr.headers.contains_key(&name) {
            return 1;
        }
    }
    0
}

/// `res.hasHeader(name)` boolean wrapper for static dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_has_header_value(
    handle: i64,
    name_ptr: *const StringHeader,
) -> f64 {
    f64::from_bits(JsValue::from_bool(js_node_http_res_has_header(handle, name_ptr) != 0).bits())
}

/// `res.appendHeader(name, value)` — append another string value to the
/// in-memory header slot. Multi-value header storage is represented as the
/// same comma-joined string Node exposes through `String(res.getHeader())`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_append_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> i64 {
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    let value = read_string_header(value_ptr as *mut _).unwrap_or_default();
    if name.is_empty() {
        return handle;
    }
    let lower = name.to_lowercase();
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.remember_header(&lower);
            if let Some(list) = sr.header_value_lists.get_mut(&lower) {
                // Already a multi-value header (e.g. Set-Cookie): append a new
                // element so it emits its own wire line (#4826).
                list.push(value.clone());
                let joined = list.join(", ");
                sr.headers.insert(lower.clone(), joined);
            } else {
                sr.headers
                    .entry(lower.clone())
                    .and_modify(|existing| {
                        existing.push(',');
                        existing.push_str(&value);
                    })
                    .or_insert(value);
            }
            sr.raw_header_names.entry(lower).or_insert(name);
        }
    }
    handle
}

/// `res.getHeaders()` — JSON-stringify the lowercase-keyed map.
/// TS-side parses with `JSON.parse`.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_headers_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<ServerResponse>(handle)
        .map(|sr| serde_json::to_string(&sr.headers).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&s).as_raw()
}

/// `res.getHeaderNames()` — JSON-stringify the list of lowercase
/// header names (matches Node — `getHeaderNames` returns lowercase).
#[no_mangle]
pub extern "C" fn js_node_http_res_get_header_names_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<ServerResponse>(handle)
        .map(|sr| {
            let mut names: Vec<&String> = sr.headers.keys().collect();
            names.sort();
            serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
        })
        .unwrap_or_else(|| "[]".to_string());
    alloc_string(&s).as_raw()
}

/// `res.setHeaders(headers)` — Node accepts only a `Headers` or a `Map`
/// (anything else is `ERR_INVALID_ARG_TYPE`), and throws
/// `ERR_HTTP_HEADERS_SENT` if the head was already committed. The runtime's
/// `js_node_setheaders_entries_json` normalizes the argument into a JSON
/// `[name, value]` entries array (or null for an invalid type) WITHOUT ever
/// dereferencing a registry handle — the old path JSON-stringified the
/// `Headers` handle directly, walking its fetch-band id (`0x40000`+) as a heap
/// `GcHeader` and segfaulting nondeterministically (#4965).
#[no_mangle]
pub extern "C" fn js_node_http_res_set_headers(handle: i64, headers_value: f64) -> i64 {
    // Node order: the headers-sent check fires before the argument is
    // validated. `header_committed` covers a prior `writeHead`; `headers_sent`
    // covers an already-flushed body.
    let committed = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.headers_sent || sr.header_committed)
        .unwrap_or(false);
    if committed {
        perry_ffi::throw_with_code(
            "Cannot set headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    let entries_ptr = unsafe { js_node_setheaders_entries_json(headers_value) };
    if entries_ptr.is_null() {
        perry_ffi::throw_with_code(
            "The \"headers\" argument must be an instance of Headers or Map.",
            "ERR_INVALID_ARG_TYPE",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    if let Some(json) = read_string_header(entries_ptr) {
        if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
            if !sr.headers_sent {
                apply_headers_entries(sr, &json);
            }
        }
    }
    handle
}

/// Apply a normalized `setHeaders` entries array: `[[name, value], …]` where
/// `value` is a string or (for `Set-Cookie`/multi-valued headers) an array of
/// strings. The pairwise (vs object) shape preserves a `Set-Cookie` array as a
/// per-element list so the wire layer emits one line each (#4826/#4965).
fn apply_headers_entries(sr: &mut ServerResponse, json: &str) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return;
    };
    for item in items {
        let serde_json::Value::Array(pair) = item else {
            continue;
        };
        let mut pair = pair.into_iter();
        let (Some(name_v), Some(value_v)) = (pair.next(), pair.next()) else {
            continue;
        };
        let name = match name_v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        sr.remember_header(&lower);
        if let serde_json::Value::Array(elems) = value_v {
            let elems: Vec<String> = elems
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect();
            sr.headers.insert(lower.clone(), elems.join(", "));
            sr.header_value_lists.insert(lower.clone(), elems);
        } else {
            let value = match value_v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sr.headers.insert(lower.clone(), value);
            sr.header_value_lists.remove(&lower);
        }
        sr.raw_header_names.insert(lower, name);
    }
}

/// `res.statusMessage` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_status_message(handle: i64) -> f64 {
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if let Some(message) = &sr.status_message {
            let header = alloc_string(message);
            return f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK));
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `res.finished` getter. Node aliases this to the ended state.
#[no_mangle]
pub extern "C" fn js_node_http_res_finished(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.writable_ended { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.sendDate` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_send_date(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.send_date { 1 } else { 0 })
        .unwrap_or(1)
}

/// `res.sendDate = bool` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_send_date(handle: i64, value: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.send_date = jsvalue_truthy(value);
    }
}

/// `res.strictContentLength` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_strict_content_length(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.strict_content_length { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.strictContentLength = bool` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_strict_content_length(handle: i64, value: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.strict_content_length = jsvalue_truthy(value);
    }
}

/// Paired request handle for `res.req`.
#[no_mangle]
pub extern "C" fn js_node_http_res_req_handle(handle: i64) -> i64 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| sr.req_handle)
        .unwrap_or(0)
}

/// `res.headersSent` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_headers_sent(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.headers_sent { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.writableEnded` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_writable_ended(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.writable_ended { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.writableFinished` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_writable_finished(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.writable_finished { 1 } else { 0 })
        .unwrap_or(0)
}

/// Merge a JSON-encoded header object (`{"Content-Type":"text/plain",...}`)
/// into a `ServerResponse`'s header map, preserving the original case for
/// `getHeaderNames()` while keying the lookup table lowercase. Shared by
/// `writeHead`'s bulk-header path.
fn apply_headers_json(sr: &mut ServerResponse, json: &str) {
    if json.is_empty() || json == "null" || json == "undefined" {
        return;
    }
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(json) {
        for (k, v) in obj {
            let lower = k.to_lowercase();
            sr.remember_header(&lower);
            // Array values (e.g. Set-Cookie) emit one wire line per element.
            // Node coerces the scalar `getHeader`/lookup value to the
            // elements joined with `, `, and keeps the per-element list so
            // the response serializer can emit each on its own line (#4826).
            if let serde_json::Value::Array(items) = v {
                let elems: Vec<String> = items
                    .into_iter()
                    .map(|item| match item {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    })
                    .collect();
                sr.headers.insert(lower.clone(), elems.join(", "));
                sr.header_value_lists.insert(lower.clone(), elems);
                sr.raw_header_names.insert(lower, k);
                continue;
            }
            let value = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sr.headers.insert(lower.clone(), value);
            sr.header_value_lists.remove(&lower);
            sr.raw_header_names.insert(lower, k);
        }
    }
}

/// `res.writeHead(statusCode[, statusMessage][, headers])` — set status +
/// optional status message + bulk headers.
///
/// #2132: `arg2`/`arg3` arrive as raw NaN-boxed JSValues (`NA_JSV`) so the
/// runtime can resolve Node's overloads — a string in slot 2 is the
/// `statusMessage`, an object in slot 2 or 3 is the bulk `headers`. Headers
/// objects are serialized here via `js_json_stringify`. The previous wiring
/// typed both slots `NA_STR`, which coerced a headers *object* to the literal
/// `"[object Object]"` and silently dropped every header set through
/// `writeHead` (visible as missing `Content-Type` etc. on the wire).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_write_head(
    handle: i64,
    status: f64,
    arg2: i64,
    arg3: i64,
) {
    let v2 = JsValue::from_bits(arg2 as u64);
    let v3 = JsValue::from_bits(arg3 as u64);

    // Resolve the (statusMessage?, headers?) overload. `is_pointer()` is the
    // POINTER_TAG heap-object test — exactly the shape of a headers object
    // literal; strings (STRING_TAG) and primitives are excluded.
    let mut status_message: Option<String> = None;
    let mut headers_value: Option<f64> = None;
    if v3.is_pointer() {
        headers_value = Some(f64::from_bits(arg3 as u64));
        if v2.is_string() {
            status_message = read_string_header(v2.as_string_ptr());
        }
    } else if v2.is_pointer() {
        headers_value = Some(f64::from_bits(arg2 as u64));
    } else if v2.is_string() {
        status_message = read_string_header(v2.as_string_ptr());
    }

    let headers_json = headers_value.and_then(|hv| {
        let ptr = js_json_stringify(hv, 0);
        if ptr.is_null() {
            None
        } else {
            read_string_header(ptr)
        }
    });

    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if sr.headers_sent {
            return;
        }
        if status.is_finite() && status > 0.0 {
            sr.status_code = status as u16;
        }
        if let Some(m) = status_message {
            if !m.is_empty() {
                sr.status_message = Some(m);
            }
        }
        if let Some(json) = headers_json {
            // Node's `writeHead` accepts the headers as an object OR as a flat
            // array `[name, value, name, value, …]` (even offsets are names,
            // odd are values — NOT a list of tuples). Route the array form to
            // the pairwise applier; objects keep the original path (#4965).
            if json.trim_start().starts_with('[') {
                apply_headers_flat_array(sr, &json);
            } else {
                apply_headers_json(sr, &json);
            }
        }
        // Mark the head committed (Node's `_header`) so a later
        // `res.setHeaders(...)` throws `ERR_HTTP_HEADERS_SENT`. The actual wire
        // flush still happens lazily at `write`/`end` (`headers_sent`), so the
        // deferred-send path is unchanged (#4965).
        sr.header_committed = true;
    }
}

/// Apply a Node `writeHead` flat-array headers value (`[name, value, …]`).
/// Even offsets are header names, odd offsets the associated values; an array
/// element may itself be an array (multi-valued header). Mirrors
/// `apply_headers_json`'s lowercase-key / original-case / array-list handling.
fn apply_headers_flat_array(sr: &mut ServerResponse, json: &str) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return;
    };
    let mut it = items.into_iter();
    while let (Some(name_v), Some(value_v)) = (it.next(), it.next()) {
        let name = match name_v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        sr.remember_header(&lower);
        if let serde_json::Value::Array(elems) = value_v {
            let elems: Vec<String> = elems
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect();
            sr.headers.insert(lower.clone(), elems.join(", "));
            sr.header_value_lists.insert(lower.clone(), elems);
        } else {
            let value = match value_v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sr.headers.insert(lower.clone(), value);
            sr.header_value_lists.remove(&lower);
        }
        sr.raw_header_names.insert(lower, name);
    }
}

/// Stream `bytes` to the connection. Returns `Some(below_hwm)` when the response is streaming
/// (`begin_streaming` succeeded now or earlier), `None` when it isn't —
/// the caller falls back to the legacy buffered path.
fn stream_write(handle: i64, bytes: &[u8]) -> Option<bool> {
    stream_write_with_cb(handle, bytes, 0)
}

/// `stream_write`, but also enqueues `callback` (if non-zero) into
/// `pending_write_callbacks` for the chunk it belongs to.
fn stream_write_with_cb(handle: i64, bytes: &[u8], callback: i64) -> Option<bool> {
    if !begin_streaming(handle) {
        return None;
    }
    let (conn, seq) = get_handle::<ServerResponse>(handle).and_then(|sr| sr.turnloop)?;
    // A backpressured HTTP/2 write has already accepted these bytes. Preserve
    // its answer instead of falling back to buffering a duplicate chunk.
    let below_hwm = crate::server::turnloop_route::send_body(conn, seq, bytes)?;
    let sr = get_handle_mut::<ServerResponse>(handle)?;
    if callback != 0 {
        sr.pending_write_callbacks.push(callback);
    }
    if !below_hwm {
        sr.needs_drain = true;
    }
    Some(below_hwm)
}

/// `res.write(chunk)` — flush the head on first write (Node's behavior)
/// and stream the chunk to the wire; falls back to buffering for handle
/// flavors that can't stream. Returns 0 (backpressure: "wait for drain")
/// once the queued-but-unsent bytes pass the HWM, else 1.
#[no_mangle]
pub extern "C" fn js_node_http_res_write(handle: i64, chunk: f64) -> i32 {
    let bytes = match jsvalue_to_body_bytes(chunk) {
        Some(b) => b,
        None => return 1,
    };
    let ended = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true);
    if ended {
        return 1;
    }
    if let Some(below_hwm) = stream_write(handle, &bytes) {
        return below_hwm as i32;
    }
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            sr.buffered_body.extend_from_slice(&bytes);
        }
    }
    1
}

/// Return the closure pointer carried by `value_bits` if it is a real callable
/// (POINTER_TAG + CLOSURE_MAGIC), else 0. Uses `js_value_is_closure` so a
/// `Buffer`/object chunk — also POINTER_TAG — is never mistaken for a callback.
pub(crate) fn callback_from_bits(value_bits: i64) -> i64 {
    if unsafe { js_value_is_closure(value_bits) } != 0 {
        (value_bits as u64 & PTR_MASK) as i64
    } else {
        0
    }
}

/// Pick the callback from a `(encoding?, callback?)` trailing arg pair, the
/// later slot first — mirroring Node's `(chunk, encoding, callback)` rule. A
/// string encoding is not callable, so it is skipped.
pub(crate) fn pick_trailing_callback(arg2: i64, arg3: i64) -> i64 {
    let c3 = callback_from_bits(arg3);
    if c3 != 0 {
        c3
    } else {
        callback_from_bits(arg2)
    }
}

/// `res.write(chunk[, encoding][, callback])` — the full Node surface routed
/// from the static native dispatch table. The trailing `(encoding?,
/// callback?)` args arrive as raw NaN-boxed JSValues (`NA_JSV`); the callback
/// is queued (it fires in order at `.end()`, #4904) and the encoding string is
/// ignored for the buffered body. Returns a NaN-boxed boolean: `false` once
/// the buffered body passes the 16 KiB high-water mark (Node's backpressure
/// signal, which terminates `while (res.write(buf))` producer loops), else
/// `true` (#4909).
#[no_mangle]
pub extern "C" fn js_node_http_res_write_full(
    handle: i64,
    chunk: f64,
    arg2: i64,
    arg3: i64,
) -> f64 {
    let callback = pick_trailing_callback(arg2, arg3);
    let bytes = jsvalue_to_body_bytes(chunk);
    let ended = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true);
    if ended {
        return f64::from_bits(TAG_TRUE);
    }
    if let Some(b) = &bytes {
        if let Some(below_hwm) = stream_write(handle, b) {
            // Streaming: the chunk is on its way to the wire, so the write
            // callback fires now rather than queueing for `.end()`.
            if callback != 0 {
                call_closure0(callback);
            }
            return f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE });
        }
    }
    let mut below_hwm = true;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            if let Some(b) = &bytes {
                sr.buffered_body.extend_from_slice(b);
            }
            if callback != 0 {
                sr.pending_write_callbacks.push(callback);
            }
            below_hwm = sr.buffered_body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    }
    f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE })
}

/// `res.addTrailers(headers)` — store HTTP trailers emitted after the
/// response body, per Node's `ServerResponse.addTrailers`. Trailers carry
/// metadata that isn't known until the body has been produced.
#[no_mangle]
pub extern "C" fn js_node_http_res_add_trailers(handle: i64, headers_value: f64) {
    let v = JsValue::from_bits(headers_value.to_bits());
    if v.is_undefined() || v.is_null() {
        return;
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return,
    };
    let Some(obj) = parsed.as_object() else {
        return;
    };
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if sr.writable_ended {
            return;
        }
        for (k, v) in obj {
            let lower = k.to_lowercase();
            let value = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            sr.trailers.insert(lower.clone(), value);
            sr.raw_trailer_names.insert(lower, k.clone());
        }
    }
}

/// Finalize a buffered response: append the final chunk, hand it to the
/// connection's encoder, and return the `(finish, close)`
/// listener lists **without** firing them — the caller controls ordering so
/// that `res.end(cb)` can run write/end callbacks before `'finish'` (Node's
/// contract, where `'finish'` never precedes the end callback). Returns
/// `None` if the response was already ended or the handle is gone.
pub(crate) fn finalize_buffered_end(handle: i64, chunk: f64) -> Option<(Vec<i64>, Vec<i64>)> {
    let v = JsValue::from_bits(chunk.to_bits());
    let final_chunk = if v.is_undefined() || v.is_null() {
        None
    } else {
        jsvalue_to_body_bytes(chunk)
    };

    let sr = get_handle_mut::<ServerResponse>(handle)?;
    if sr.writable_ended {
        return None;
    }

    // P5 streaming: the head already went to the wire on this thread, so the
    // final chunk and the trailer block are encoded and submitted directly.
    if sr.turnloop_streaming {
        let (conn, seq) = sr.turnloop.expect("streaming implies a turnloop target");
        let chunk = final_chunk.clone();
        let trailers = sr.snapshot_trailers();
        sr.writable_ended = true;
        sr.writable_finished = true;
        sr.needs_drain = false;
        let finish_listeners = take_event_listeners(sr, "finish");
        let close_listeners = take_event_listeners(sr, "close");
        if let Some(c) = chunk {
            let _ = crate::server::turnloop_route::send_body(conn, seq, &c);
        }
        crate::server::turnloop_route::finish_body(conn, seq, &trailers);
        crate::server::request::mark_connection_written(req_handle_of(handle));
        return Some((finish_listeners, close_listeners));
    }

    if let Some(c) = final_chunk {
        sr.buffered_body.extend_from_slice(&c);
    }
    sr.headers_sent = true;
    sr.writable_ended = true;
    let auto_content_length = sr.trailers.is_empty()
        && !sr.headers.contains_key("content-length")
        && !sr.headers.contains_key("transfer-encoding");
    sr.ensure_content_length();
    let body = std::mem::take(&mut sr.buffered_body);
    let headers = sr.snapshot_headers();
    let trailers = sr.snapshot_trailers();
    let shape = ResponseShape {
        status: sr.status_code,
        status_message: sr.status_message.clone(),
        headers,
        trailers,
        body,
        auto_content_length,
    };
    let finish_listeners = take_event_listeners(sr, "finish");
    let close_listeners = take_event_listeners(sr, "close");
    let turnloop = sr.turnloop;
    let req_handle = sr.req_handle;
    // P5: the handler, the codec and the socket are on the same thread, so
    // the response is encoded and submitted here. A response with no
    // connection (the connection's request was never delivered) has nowhere
    // to go, and is simply finished.
    if let Some((conn, seq)) = turnloop {
        crate::server::turnloop_route::send_response(conn, seq, shape);
    }
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.writable_finished = true;
    }
    crate::server::request::mark_connection_written(req_handle);
    Some((finish_listeners, close_listeners))
}

/// Flush the response head to the wire now and switch the response into
/// streaming mode: the status line + headers are written immediately, and
/// subsequent `res.write(...)` chunks flow
/// straight to the client (chunked transfer-encoding unless the handler set
/// Content-Length). This is what makes Node shapes like "send headers, keep
/// the response open, write later" (SSE, long-poll, `res.flushHeaders()`,
/// `res.write()` before an async gap) observable client-side before
/// `.end()`.
///
/// Returns `true` when the response is in streaming mode after the call.
/// Standalone (`assignSocket`) and bare `OutgoingMessage` handles keep the
/// buffered path, as does a response whose connection already died.
pub(crate) fn begin_streaming(handle: i64) -> bool {
    let Some(sr) = get_handle_mut::<ServerResponse>(handle) else {
        return false;
    };
    if sr.writable_ended {
        return false;
    }
    if sr.turnloop_streaming {
        return true;
    }
    if sr.standalone || sr.outgoing_message_only {
        return false;
    }
    let Some((conn, seq)) = sr.turnloop else {
        return false;
    };
    let shape = ResponseShape {
        status: sr.status_code,
        status_message: sr.status_message.clone(),
        headers: sr.snapshot_headers(),
        trailers: Vec::new(),
        body: Vec::new(),
        auto_content_length: false,
    };
    let first = std::mem::take(&mut sr.buffered_body);
    sr.headers_sent = true;
    sr.turnloop_streaming = true;
    if !crate::server::turnloop_route::begin_stream(conn, seq, shape) {
        if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
            sr.turnloop_streaming = false;
        }
        return false;
    }
    if !first.is_empty() {
        let _ = crate::server::turnloop_route::send_body(conn, seq, &first);
    }
    true
}

/// If a streaming response previously hit backpressure (`res.write()`
/// returned `false`) and its queued bytes have since drained below the
/// HWM, clear the flag and return its `'drain'` listeners for the caller
/// (the pump) to fire. Empty otherwise.
pub(crate) fn take_drain_listeners_if_ready(handle: i64) -> Vec<i64> {
    let Some(sr) = get_handle_mut::<ServerResponse>(handle) else {
        return Vec::new();
    };
    if !sr.needs_drain || sr.writable_ended {
        return Vec::new();
    }
    let below = match sr.turnloop {
        Some((conn, seq)) if sr.turnloop_streaming => {
            crate::server::turnloop_route::writable_below_watermark(conn, seq)
        }
        _ => false,
    };
    if !below {
        return Vec::new();
    }
    sr.needs_drain = false;
    take_event_listeners(sr, "drain")
}

/// `res.flushHeaders()` — Node sends headers immediately even before
/// any body. Flushes the head to the wire and switches to the streaming
/// body path; falls back to marking headers-sent for handle flavors that
/// can't stream (standalone / bare OutgoingMessage).
#[no_mangle]
pub extern "C" fn js_node_http_res_flush_headers(handle: i64) {
    if !begin_streaming(handle) {
        if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
            sr.headers_sent = true;
        }
    }
}

/// `res.cork()` — buffered responses are already corked until `.end()`.
#[no_mangle]
pub extern "C" fn js_node_http_res_cork(_handle: i64) {}

/// `res.uncork()` — no-op counterpart to `cork()`.
#[no_mangle]
pub extern "C" fn js_node_http_res_uncork(_handle: i64) {}

/// `res.setTimeout(msecs[, callback])` — expose Node's chainable control
/// surface; actual transport timeout scheduling is handled at the server.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_timeout(handle: i64, _msecs: f64, _callback: i64) -> i64 {
    handle
}

/// `res.writeEarlyHints(hints[, cb])` — interim 103 response is not yet sent
/// (accepted no-op), but the `hints` argument is validated to match Node
/// (#4907): a non-object throws `ERR_INVALID_ARG_TYPE`, and a malformed
/// `link` value throws `ERR_INVALID_ARG_VALUE`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_write_early_hints(
    _handle: i64,
    hints: f64,
    _callback: i64,
) {
    // Serialize the hints object and inspect it. Node requires `typeof hints
    // === 'object'` (and non-null); a string / number / null throws
    // `ERR_INVALID_ARG_TYPE`.
    let json = {
        let ptr = js_json_stringify(hints, 0);
        if ptr.is_null() {
            None
        } else {
            read_string_header(ptr)
        }
    };
    let value: Option<serde_json::Value> = json.and_then(|j| serde_json::from_str(&j).ok());
    match value {
        // Arrays are objects in JS — `hints.link` is simply undefined, so no
        // validation fires.
        Some(serde_json::Value::Object(map)) => {
            if let Some(link) = map.get("link") {
                let invalid = match link {
                    serde_json::Value::Null => false,
                    serde_json::Value::String(s) => !is_valid_link_header(s),
                    serde_json::Value::Array(items) => items
                        .iter()
                        .any(|i| i.as_str().map(|s| !is_valid_link_header(s)).unwrap_or(true)),
                    _ => true,
                };
                if invalid {
                    perry_ffi::throw_with_code(
                        "The property 'hints.link' must be an array or string of format \"</styles.css>; rel=preload; as=style\".",
                        "ERR_INVALID_ARG_VALUE",
                        perry_ffi::ErrorKind::TypeError,
                    );
                }
            }
        }
        Some(serde_json::Value::Array(_)) => {}
        _ => {
            perry_ffi::throw_with_code(
                "The \"hints\" argument must be of type object.",
                "ERR_INVALID_ARG_TYPE",
                perry_ffi::ErrorKind::TypeError,
            );
        }
    }
}

/// `res.on(event, cb)` — register a listener.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    let mut should_fire_now = false;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.listeners
            .entry(event.clone())
            .or_default()
            .push(callback);
        // If `.end()` already fired, late listeners for `'finish'` /
        // `'close'` should still see them (Node fires them
        // asynchronously, so a late `on` registration is racy but
        // observed; our synchronous emit means we fire on
        // registration if already done).
        if sr.writable_finished && (event == "finish" || event == "close") {
            should_fire_now = true;
        }
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if should_fire_now && callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
    handle_to_pointer_f64(handle)
}

/// `res.once(event, cb)` — register a one-shot listener. Mirrors
/// `js_node_http_res_on` but stores into `once_listeners`, which every
/// event-consumption site drains via [`take_event_listeners`] after firing so
/// the listener runs exactly once (Node's `EventEmitter.once` contract).
///
/// Without this the `once` dispatch arm did not exist at all: `res.once(...)`
/// fell through to a no-op, so Perry's `createReadStream().pipe(res)` pump —
/// which re-arms `res.once('drain')` after each backpressure pause — never got
/// its drain callback and stalled after the first 64 KB chunk, truncating every
/// static file larger than the high-water mark.
///
/// # Safety
/// FFI entry; `event_name_ptr` must be a valid `StringHeader` for its length.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_once(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    let mut should_fire_now = false;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        // `finish`/`close` already fired: run immediately without storing,
        // matching the late-registration behavior of `js_node_http_res_on`.
        if sr.writable_finished && (event == "finish" || event == "close") {
            should_fire_now = true;
        } else {
            sr.once_listeners.entry(event).or_default().push(callback);
        }
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if should_fire_now && callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
    handle_to_pointer_f64(handle)
}

/// Listeners to fire for `event`: persistent `on` listeners (cloned, retained)
/// followed by one-shot `once` listeners (removed, so they fire exactly once).
pub(crate) fn take_event_listeners(sr: &mut ServerResponse, event: &str) -> Vec<i64> {
    let mut out = sr.listeners.get(event).cloned().unwrap_or_default();
    if let Some(once) = sr.once_listeners.remove(event) {
        out.extend(once);
    }
    out
}

// ============================================================================
// Allocation helper used by server.rs
// ============================================================================

#[no_mangle]
pub extern "C" fn js_node_http_outgoing_message_new() -> i64 {
    register_handle(ServerResponse::outgoing_message())
}

// ============================================================================
// #4904: standalone `new http.ServerResponse(req)` + `assignSocket` support
// ============================================================================

/// `new http.ServerResponse(req)` — a response not bound to a live
/// connection. `req` contributes only the method (Node skips the body on
/// flush when it was a HEAD request); writes buffer until `.end()`, which
/// flushes through the socket assigned via `res.assignSocket(socket)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_response_standalone_new(req: f64) -> i64 {
    crate::server::ensure_gc_scanner_registered();
    let mut sr = ServerResponse::new();
    sr.standalone = true;
    sr.send_date = false;
    if JsValue::from_bits(req.to_bits()).is_pointer() {
        extern "C" {
            fn js_object_get_field_by_name(
                obj: *const perry_ffi::ObjectHeader,
                key: *const StringHeader,
            ) -> JsValue;
        }
        let key = alloc_string("method");
        let m = js_object_get_field_by_name(
            (req.to_bits() & PTR_MASK) as *const perry_ffi::ObjectHeader,
            key.as_raw(),
        );
        if JsValue::from_bits(m.bits()).is_string() {
            sr.standalone_req_method = read_string_header((m.bits() & PTR_MASK) as *mut _);
        }
    }
    register_handle(sr)
}

/// `res.assignSocket(socket)` — wire a (possibly userland) Writable as the
/// flush target. Node throws `ERR_HTTP_SOCKET_ASSIGNED` on a second call.
#[no_mangle]
pub extern "C" fn js_node_http_res_assign_socket(handle: i64, socket: f64) {
    let already = get_handle::<ServerResponse>(handle)
        .map(|sr| !JsValue::from_bits(sr.standalone_socket.to_bits()).is_undefined())
        .unwrap_or(false);
    if already {
        perry_ffi::throw_with_code(
            "Socket already assigned",
            "ERR_HTTP_SOCKET_ASSIGNED",
            perry_ffi::ErrorKind::Error,
        );
    }
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.standalone_socket = socket;
        sr.standalone = true;
    }
}

/// `res.detachSocket(socket)` — counterpart of `assignSocket`.
#[no_mangle]
pub extern "C" fn js_node_http_res_detach_socket(handle: i64, _socket: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.standalone_socket = f64::from_bits(TAG_UNDEFINED);
    }
}

/// `res.write(chunk[, encoding][, callback])` — callback-aware variant of
/// `js_node_http_res_write`. The callback queues until the buffered body
/// flushes on `.end()`, preserving call order (#4904).
#[no_mangle]
pub extern "C" fn js_node_http_res_write_with_cb(handle: i64, chunk: f64, callback: i64) -> i32 {
    // #4975: `write()` after `destroy()` must not buffer; Node invokes the
    // callback with an `ERR_STREAM_DESTROYED` error and returns `false` (no
    // `'error'` event is emitted, so an `on('error', …)` listener stays silent).
    if get_handle::<ServerResponse>(handle)
        .map(|sr| sr.destroyed)
        .unwrap_or(false)
    {
        if callback != 0 {
            let err = perry_ffi::error_value_with_code(
                "Cannot call write after a stream was destroyed",
                "ERR_STREAM_DESTROYED",
                perry_ffi::ErrorKind::Error,
            );
            crate::server::http2_server::call1(callback, f64::from_bits(err.bits()));
        }
        return 0;
    }
    let bytes = jsvalue_to_body_bytes(chunk);
    // Honor streaming mode (after `res.flushHeaders()` / a prior streamed
    // `res.write`) exactly like `js_node_http_res_write`: the chunk must go down
    // the stream channel, NOT into `buffered_body`. Without this, a streamed
    // response whose chunks arrive via the callback-aware dispatch arm
    // (`res.write(chunk)` from Next's `pipeToNodeResponse` WritableStream)
    // buffered every chunk while `.end()` took the stream-finalize path — which
    // only sends the final `.end(chunk)` arg and drops `buffered_body` entirely,
    // so the JSON API-route body never reached the wire (HTTP 200, 0 bytes).
    // #5437 (Next.js app-route response pipe).
    if let Some(b) = &bytes {
        let ended = get_handle::<ServerResponse>(handle)
            .map(|sr| sr.writable_ended)
            .unwrap_or(true);
        if !ended {
            // Register the write callback BEFORE the data frame is published so
            // a receiver that drains immediately can't run it out of order
            // relative to later writes / `.end()` (Node fires it once the chunk
            // is flushed; queued, it drains in order, #4904). `stream_write_with_cb`
            // enqueues the callback ahead of the `tx.send`.
            if let Some(below_hwm) = stream_write_with_cb(handle, b, callback) {
                return below_hwm as i32;
            }
        }
    }
    // #4909 — real backpressure boolean (mirrors `js_node_http_res_write_full`
    // on the static path): `false` past the 16 KiB high-water mark, so dynamic
    // `while (res.write(buf, cb))` producer loops terminate.
    let mut below_hwm = true;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            if let Some(b) = &bytes {
                sr.buffered_body.extend_from_slice(b);
            }
            if callback != 0 {
                sr.pending_write_callbacks.push(callback);
            }
            below_hwm = sr.buffered_body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    }
    if below_hwm {
        1
    } else {
        0
    }
}

/// Invoke `socket.write(chunk)` on an arbitrary JS value through the
/// runtime's dynamic method-call path.
pub(crate) unsafe fn socket_write_str(socket: f64, chunk: &str) {
    extern "C" {
        fn js_native_call_method_str_key(
            object: f64,
            name_handle: i64,
            args_ptr: *const f64,
            args_len: usize,
        ) -> f64;
    }
    let name = alloc_string("write");
    let chunk_val = f64::from_bits(JsValue::from_string_ptr(alloc_string(chunk).as_raw()).bits());
    let args = [chunk_val];
    let _ = js_native_call_method_str_key(socket, name.as_raw() as i64, args.as_ptr(), 1);
}

fn jsvalue_truthy(value: f64) -> bool {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_bool() {
        v.to_bool()
    } else if v.is_undefined() || v.is_null() {
        false
    } else if v.is_number() {
        v.to_number() != 0.0
    } else {
        true
    }
}

#[allow(dead_code)]
pub(crate) fn _force_link_helpers(v: f64) -> bool {
    f64::from_bits(TAG_NULL) == v
}

#[path = "response_turnloop.rs"]
mod turnloop_shape;
pub(crate) use turnloop_shape::{
    alloc_server_response_for_turnloop, req_handle_of, stream_receiver_gone,
};

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
