//! `ServerResponse` — the Node.js Writable stream returned to a
//! `(req, res) => …` handler. The response buffers chunks until `.end()`
//! (or streams them once the head has been flushed), and the turnloop
//! connection that decoded the request encodes and writes it
//! (`turnloop_route`).

use super::response_payload::{self as heap, Key as ResponseKey};
use std::collections::HashMap;

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::server::request::handle_to_pointer_f64;
use crate::server::response_end::call_closure0;
use crate::server::types::{
    js_json_stringify, js_node_setheaders_entries, js_value_is_closure, jsvalue_to_body_bytes,
    read_string_header, PTR_MASK, STRING_TAG, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
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
    // An open typed payload is authoritative; only a released payload needs
    // the object's retained _header field.
    if let Some(sr) = response_state(handle) {
        return sr.headers_sent || sr.header_committed;
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    heap::closed_field(owner.get(), b"_header").is_some_and(heap::truthy)
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

/// Legacy transport/OutgoingMessage handle. Standalone ServerResponse objects
/// own a ResponseState payload and keep their JS edges on the object.
pub struct ServerResponse {
    pub(crate) owner_agent: u64,
    pub state: ResponseState,
    /// Traced original headers for registry-backed transport/OutgoingMessage owners.
    pub header_values: f64,
    pub listeners: HashMap<String, Vec<i64>>,
    pub once_listeners: HashMap<String, Vec<i64>>,
    pub standalone_socket: f64,
    pub pending_write_callbacks: Vec<i64>,
}
impl std::ops::Deref for ServerResponse {
    type Target = ResponseState;
    fn deref(&self) -> &ResponseState {
        &self.state
    }
}
impl std::ops::DerefMut for ServerResponse {
    fn deref_mut(&mut self) -> &mut ResponseState {
        &mut self.state
    }
}
pub struct ResponseState {
    /// Node distinguishes an absent outgoing map from a created, empty one.
    pub(crate) header_fields: Option<header_values::HeaderFields>,
    /// The committed wire representation, frozen by writeHead.
    pub committed_headers: Option<Vec<(String, String)>>,
    pub status_code: u16,
    pub status_message: Option<String>,
    /// Native wire fields synthesized by framing or the Bun Response adapter.
    /// Application headers live exclusively in the owner's traced JS record.
    pub native_headers: HashMap<String, String>,
    /// Lowercase-keyed multi-value header map. Populated when a header is
    /// assigned an array value (e.g. `Set-Cookie`): Node emits one header
    /// line per element rather than a single comma-joined line, so the wire
    /// serializer expands these into repeated `name: value` lines (#4826).
    pub native_header_value_lists: HashMap<String, Vec<String>>,
    /// Lowercase-keyed trailer map for HTTP trailers emitted after the
    /// response body, per Node's `ServerResponse.addTrailers` contract.
    pub trailers: HashMap<String, String>,
    /// Lowercase → original spelling for the wire. The original JS values
    /// are kept in the owner's traced record, not in this Rust metadata.
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
    /// The public connection Socket is a traced edge, retained after close.
    pub socket_value: f64,
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
    /// #4904: true for `new http.ServerResponse(req)` instances (and any
    /// response wired through `assignSocket`) — `.end()` flushes through
    /// `standalone_socket` instead of a connection.
    pub standalone: bool,
    /// #4904: `req.method` captured from the standalone constructor's
    /// request argument — `HEAD` suppresses the body on flush.
    pub standalone_req_method: Option<String>,
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
    pub(crate) turnloop: Option<(crate::server::turnloop_route::ResponseConnection, u64)>,
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
            owner_agent: perry_ffi::agent_post::current_agent(),
            state: ResponseState::new(),
            header_values: f64::from_bits(TAG_UNDEFINED),
            listeners: HashMap::new(),
            once_listeners: HashMap::new(),
            standalone_socket: f64::from_bits(TAG_UNDEFINED),
            pending_write_callbacks: Vec::new(),
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
}
impl ResponseState {
    pub fn new() -> Self {
        Self {
            header_fields: None,
            committed_headers: None,
            status_code: 200,
            status_message: None,
            native_headers: HashMap::new(),
            native_header_value_lists: HashMap::new(),
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
            socket_value: f64::from_bits(TAG_UNDEFINED),
            outgoing_message_only: false,
            buffered_body: Vec::new(),
            needs_drain: false,
            standalone: false,
            standalone_req_method: None,
            destroyed: false,
            turnloop: None,
            turnloop_streaming: false,
        }
    }

    /// Snapshot the current header map as `Vec<(orig_name, value)>`
    /// preserving original case. Array-valued headers (tracked in
    /// `native_header_value_lists`) expand to one entry per element so the wire
    /// layer emits a separate header line each (#4826).
    pub fn snapshot_headers(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.native_headers.len());
        let mut ordered = self.header_order.clone();
        for lower in self.native_headers.keys() {
            if !ordered.contains(lower) {
                ordered.push(lower.clone());
            }
        }
        for lower_k in &ordered {
            let Some(v) = self.native_headers.get(lower_k) else {
                continue;
            };
            let orig = self
                .raw_header_names
                .get(lower_k)
                .cloned()
                .unwrap_or_else(|| lower_k.clone());
            if let Some(values) = self.native_header_value_lists.get(lower_k) {
                for elem in values {
                    out.push((orig.clone(), elem.clone()));
                }
            } else {
                out.push((orig, v.clone()));
            }
        }
        if self.send_date && !self.native_headers.contains_key("date") {
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

    /// Auto-fill `Content-Length` only while the header block is still open.
    /// Return whether this call synthesized the header.
    pub(crate) fn ensure_content_length(&mut self) -> bool {
        // A response with trailers must not declare a fixed Content-Length:
        // the body length alone doesn't bound the response (trailing headers
        // still follow), and some clients/proxies treat a present
        // Content-Length as "body complete, no trailers expected".
        if self.header_committed || !self.trailers.is_empty() {
            return false;
        }
        if !self.native_headers.contains_key("content-length")
            && !self.native_headers.contains_key("transfer-encoding")
        {
            self.remember_header("content-length");
            let len = self.buffered_body.len();
            self.native_headers
                .insert("content-length".to_string(), len.to_string());
            self.raw_header_names
                .insert("content-length".to_string(), "Content-Length".to_string());
            return true;
        }
        false
    }
}

// ============================================================================
// FFI surface
// ============================================================================

/// `res.statusCode = N` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_status(handle: i64, code: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if heap::set_field(response_root.get(), b"statusCode", code) {
        return;
    }
    if let Some(sr) = response_state_mut(response_root.get()) {
        if !sr.headers_sent && code.is_finite() && code > 0.0 {
            sr.status_code = code as u16;
        }
    }
}

/// `res.statusCode` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_status(handle: i64) -> f64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::field(response_root.get(), b"statusCode") {
        return v;
    }
    response_state(response_root.get())
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
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if heap::set_field(
        response_root.get(),
        b"statusMessage",
        f64::from_bits(STRING_TAG | (msg_ptr as u64 & PTR_MASK)),
    ) {
        return;
    }
    let msg = read_string_header(msg_ptr as *mut _);
    if let Some(sr) = response_state_mut(response_root.get()) {
        if !sr.headers_sent {
            sr.status_message = msg;
        }
    }
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::restate(response_root.get());
    }
}

/// `res.setHeader(name, value)`: retain the original value in the owner's
/// traced header record. Coercion is for validation and wire rendering only.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let value_root = response_scope.root_nanbox(value);
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    // #4907 — Node's `OutgoingMessage.setHeader` throws if headers are already
    // sent, then validates the field name, before touching any state.
    if response_headers_sent(response_root.get()) {
        perry_ffi::throw_with_code(
            "Cannot set headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    if !http_is_valid_token(&name) {
        perry_ffi::throw_with_code(
            &format!("Header name must be a valid HTTP token [\"{name}\"]"),
            "ERR_INVALID_HTTP_TOKEN",
            perry_ffi::ErrorKind::TypeError,
        );
    }

    header_values::validate(&name, value_root.get());
    header_values::set(response_root.get(), &name, value_root.get());
}

/// `res.setHeader(name, value)` chainable wrapper for static dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header_self(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) -> i64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    js_node_http_res_set_header(response_root.get(), name_ptr, value);
    response_root.get()
}

/// Case-insensitive lookup of the original JS value, or undefined.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_get_header(
    handle: i64,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    header_values::lookup(handle, &name)
}

/// `res.removeHeader(name)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_remove_header(
    handle: i64,
    name_ptr: *const StringHeader,
) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    // #4907 — Node's `OutgoingMessage.removeHeader` throws once headers are
    // sent (distinct "remove" wording from `setHeader`).
    if response_headers_sent(response_root.get()) {
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
    if name == "date" {
        // Node's removeHeader('Date') also suppresses the synthesized date.
        heap::set_field(response_root.get(), b"sendDate", f64::from_bits(TAG_FALSE));
        if let Some(sr) = response_state_mut(response_root.get()) {
            sr.send_date = false;
        }
    }
    header_values::remove(response_root.get(), &name);
    if let Some(sr) = response_state_mut(response_root.get()) {
        if !sr.headers_sent {
            sr.native_headers.remove(&name);
            sr.native_header_value_lists.remove(&name);
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
    header_values::has(handle, &name) as i32
}

/// `res.hasHeader(name)` boolean wrapper for static dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_has_header_value(
    handle: i64,
    name_ptr: *const StringHeader,
) -> f64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    f64::from_bits(
        JsValue::from_bool(js_node_http_res_has_header(response_root.get(), name_ptr) != 0).bits(),
    )
}

/// Append originals, mutating an existing header array in place.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_append_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) -> i64 {
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let value = scope.root_nanbox(value);
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    if response_headers_sent(owner.get()) {
        perry_ffi::throw_with_code(
            "Cannot append headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    if !http_is_valid_token(&name) {
        perry_ffi::throw_with_code(
            &format!("Header name must be a valid HTTP token [\"{name}\"]"),
            "ERR_INVALID_HTTP_TOKEN",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    header_values::validate(&name, value.get());
    header_values::append(owner.get(), &name, value.get());
    owner.get()
}

/// A fresh null-prototype map containing the original values (a shallow copy).
#[no_mangle]
pub extern "C" fn js_node_http_res_get_headers(handle: i64) -> f64 {
    header_values::get_headers(handle)
}

/// Lowercase names in Node's own-property order.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_header_names(handle: i64) -> f64 {
    header_values::get_names(handle)
}

/// `res.setHeaders(headers)` — Node accepts only a `Headers` or a `Map`
/// (anything else is `ERR_INVALID_ARG_TYPE`), and throws
/// `ERR_HTTP_HEADERS_SENT` if the head was already committed. The runtime's
/// `js_node_setheaders_entries` returns original Map entries or converted
/// Headers entries (undefined for an invalid type) without ever
/// dereferencing a registry handle — the old path JSON-stringified the
/// `Headers` handle directly, walking its fetch-band id (`0x40000`+) as a heap
/// `GcHeader` and segfaulting nondeterministically (#4965).
#[no_mangle]
pub extern "C" fn js_node_http_res_set_headers(handle: i64, headers_value: f64) -> i64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    // Node order: the headers-sent check fires before the argument is
    // validated. `header_committed` covers a prior `writeHead`; `headers_sent`
    // covers an already-flushed body.
    let committed = response_state(response_root.get())
        .map(|sr| sr.headers_sent || sr.header_committed)
        .unwrap_or(false);
    if committed {
        perry_ffi::throw_with_code(
            "Cannot set headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    let headers_value = response_scope.root_nanbox(headers_value);
    let entries =
        response_scope.root_nanbox(unsafe { js_node_setheaders_entries(headers_value.get()) });
    if JsValue::from_bits(entries.get().to_bits()).is_undefined() {
        perry_ffi::throw_with_code(
            "The \"headers\" argument must be an instance of Headers or Map.",
            "ERR_INVALID_ARG_TYPE",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    for i in 0..header_values::array_len(entries.get()) {
        let pair = response_scope.root_nanbox(header_values::array_get(entries.get(), i));
        let name = header_values::string(header_values::array_get(pair.get(), 0));
        let value = response_scope.root_nanbox(header_values::array_get(pair.get(), 1));
        let name_value = alloc_string(&name);
        unsafe {
            js_node_http_res_set_header(response_root.get(), name_value.as_raw(), value.get());
        }
    }
    response_root.get()
}

/// `res.statusMessage` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_status_message(handle: i64) -> f64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::field(response_root.get(), b"statusMessage") {
        return v;
    }
    if let Some(sr) = response_state(response_root.get()) {
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
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::field(response_root.get(), b"finished") {
        return i32::from(heap::truthy(v));
    }
    response_state(response_root.get())
        .map(|sr| if sr.writable_ended { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.sendDate` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_send_date(handle: i64) -> i32 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::field(response_root.get(), b"sendDate") {
        return i32::from(heap::truthy(v));
    }
    response_state(response_root.get())
        .map(|sr| if sr.send_date { 1 } else { 0 })
        .unwrap_or(1)
}

/// `res.sendDate = bool` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_send_date(handle: i64, value: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if heap::set_field(response_root.get(), b"sendDate", value) {
        return;
    }
    if let Some(sr) = response_state_mut(response_root.get()) {
        sr.send_date = jsvalue_truthy(value);
    }
}

/// `res.strictContentLength` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_strict_content_length(handle: i64) -> i32 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::field(response_root.get(), b"strictContentLength") {
        return i32::from(heap::truthy(v));
    }
    response_state(response_root.get())
        .map(|sr| if sr.strict_content_length { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.strictContentLength = bool` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_strict_content_length(handle: i64, value: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if heap::set_field(response_root.get(), b"strictContentLength", value) {
        return;
    }
    if let Some(sr) = response_state_mut(response_root.get()) {
        sr.strict_content_length = jsvalue_truthy(value);
    }
}

/// Paired request handle for `res.req`.
#[no_mangle]
pub extern "C" fn js_node_http_res_req_handle(handle: i64) -> i64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    response_state(response_root.get())
        .map(|sr| sr.req_handle)
        .unwrap_or(0)
}

/// `res.headersSent` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_headers_sent(handle: i64) -> i32 {
    i32::from(response_headers_sent(handle))
}

/// `res.writableEnded` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_writable_ended(handle: i64) -> i32 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::field(response_root.get(), b"finished") {
        return i32::from(heap::truthy(v));
    }
    response_state(response_root.get())
        .map(|sr| if sr.writable_ended { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.writableFinished` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_writable_finished(handle: i64) -> i32 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if let Some(v) = heap::closed_field(response_root.get(), b"finished") {
        return i32::from(heap::truthy(v));
    }
    response_state(response_root.get())
        .map(|sr| if sr.writable_finished { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.writeHead(statusCode[, statusMessage][, headers])` — set status +
/// optional status message + bulk headers.
///
/// #2132: `arg2`/`arg3` arrive as raw NaN-boxed JSValues (`NA_JSV`) so the
/// runtime can resolve Node's overloads — a string in slot 2 is the
/// `statusMessage`, an object in slot 2 or 3 is the bulk `headers`. Headers
/// objects are read directly so their original values and identities survive.
/// A previously created header map is merged; otherwise Node renders the
/// supplied headers directly without making them visible through getHeader.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_write_head(
    handle: i64,
    status: f64,
    arg2: i64,
    arg3: i64,
) -> i64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let arg2 = response_scope.root_nanbox(f64::from_bits(arg2 as u64));
    let arg3 = response_scope.root_nanbox(f64::from_bits(arg3 as u64));
    if response_headers_sent(response_root.get()) {
        perry_ffi::throw_with_code(
            "Cannot write headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    heap::sync_head_options(response_root.get());
    let arg2 = arg2.get().to_bits() as i64;
    let arg3 = arg3.get().to_bits() as i64;
    let v2 = JsValue::from_bits(arg2 as u64);
    let v3 = JsValue::from_bits(arg3 as u64);

    // Resolve the (statusMessage?, headers?) overload. `is_pointer()` is the
    // POINTER_TAG heap-object test — exactly the shape of a headers object
    // literal; strings (STRING_TAG) and primitives are excluded.
    let mut status_message: Option<String> = None;
    let mut headers_value: Option<f64> = None;
    if v3.is_pointer() {
        headers_value = Some(f64::from_bits(arg3 as u64));
        // `to_owned_string` also reads an inline SSO message (#11519).
        status_message = v2.to_owned_string();
    } else if v2.is_pointer() {
        headers_value = Some(f64::from_bits(arg2 as u64));
    } else {
        status_message = v2.to_owned_string();
    }

    let headers_value = headers_value.map(|v| response_scope.root_nanbox(v));
    let direct_headers = headers_value
        .as_ref()
        .and_then(|v| header_values::apply_write_head(response_root.get(), v.get()));

    let status_message_arg = status_message.clone();
    if let Some(sr) = response_state_mut(response_root.get()) {
        if sr.headers_sent {
            return response_root.get();
        }
        if status.is_finite() && status > 0.0 {
            sr.status_code = status as u16;
        }
        if let Some(m) = status_message {
            if !m.is_empty() {
                sr.status_message = Some(m);
            }
        }
        if let Some(headers) = direct_headers {
            sr.committed_headers = Some(headers);
        }
        // Mark the head committed (Node's `_header`) so a later
        // `res.setHeaders(...)` throws `ERR_HTTP_HEADERS_SENT`. The actual wire
        // flush still happens lazily at `write`/`end` (`headers_sent`), so the
        // deferred-send path is unchanged (#4965).
        sr.header_committed = true;
    }
    if heap::is_response(response_root.get()) {
        // The public writeHead already read the program's head fields above.
        finish_heap_head(response_root.get(), status, status_message_arg);
    } else if response_state(response_root.get()).is_some_and(|s| s.committed_headers.is_none()) {
        let headers = header_values::snapshot(response_root.get());
        if let Some(sr) = response_state_mut(response_root.get()) {
            sr.committed_headers = Some(headers);
        }
    }
    response_root.get()
}

/// node's `_implicitHeader` on a heap response's first write:
/// `writeHead(this.statusCode)`.
fn heap_implicit_head(handle: i64) {
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    if !heap::is_response(owner.get())
        || response_state(owner.get()).is_none_or(|sr| sr.header_committed || sr.writable_ended)
    {
        return;
    }
    commit_heap_head(owner.get(), f64::NAN, None);
    if let Some(sr) = response_state_mut(owner.get()) {
        sr.header_committed = true;
    }
}

/// node's `writeHead` on a heap response: `statusMessage`, `statusCode`, then
/// `_header` (`_storeHeader`).
fn commit_heap_head(handle: i64, status: f64, reason: Option<String>) {
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    heap::sync_head_fields(owner.get());
    finish_heap_head(owner.get(), status, reason);
}
fn finish_heap_head(handle: i64, status: f64, reason: Option<String>) {
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let code = if status.is_finite() && status > 0.0 {
        status as u16
    } else {
        response_state(owner.get()).map_or(200, |sr| sr.status_code)
    };
    heap::write_head_fields(owner.get(), code, reason.filter(|m| !m.is_empty()));
    if response_state(owner.get()).is_some_and(|s| s.committed_headers.is_none()) {
        let headers = header_values::snapshot(owner.get());
        if let Some(sr) = response_state_mut(owner.get()) {
            sr.committed_headers = Some(headers);
        }
    }
    heap::restate(owner.get());
    if let Some(head) = super::response_end::render_head(owner.get()) {
        heap::set_header_text(owner.get(), &head);
    }
}

/// Stream `bytes` to the connection. Returns `Some(below_hwm)` when the response is streaming
/// (`begin_streaming` succeeded now or earlier), `None` when it isn't —
/// the caller falls back to the legacy buffered path.
fn stream_write(handle: i64, bytes: &[u8]) -> Option<bool> {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    stream_write_with_cb(response_root.get(), bytes, 0)
}

/// `stream_write`, but also enqueues `callback` (if non-zero) into
/// `pending_write_callbacks` for the chunk it belongs to.
fn stream_write_with_cb(handle: i64, bytes: &[u8], callback: i64) -> Option<bool> {
    let callback_scope = perry_ffi::TransientRootScope::enter();
    let callback_root = callback_scope.root_addr(callback);
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if !begin_streaming(response_root.get()) {
        return None;
    }
    let (conn, seq) = response_state(response_root.get()).and_then(|sr| sr.turnloop)?;
    let conn = conn.root();
    // A backpressured HTTP/2 write has already accepted these bytes. Preserve
    // its answer instead of falling back to buffering a duplicate chunk.
    let below_hwm = crate::server::turnloop_route::send_body(&conn, seq, bytes)?;
    if !below_hwm {
        response_state_mut(response_root.get())?.needs_drain = true;
    }
    if callback != 0 {
        push_write_callback(response_root.get(), callback_root.get());
    }
    Some(below_hwm)
}

/// `res.write(chunk)` — flush the head on first write (Node's behavior)
/// and stream the chunk to the wire; falls back to buffering for handle
/// flavors that can't stream. Returns 0 (backpressure: "wait for drain")
/// once the queued-but-unsent bytes pass the HWM, else 1.
#[no_mangle]
pub extern "C" fn js_node_http_res_write(handle: i64, chunk: f64) -> i32 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let bytes = match jsvalue_to_body_bytes(chunk) {
        Some(b) => b,
        None => return 1,
    };
    let ended = response_state(response_root.get())
        .map(|sr| sr.writable_ended)
        .unwrap_or(true);
    if ended {
        return 1;
    }
    if let Some(below_hwm) = stream_write(response_root.get(), &bytes) {
        return below_hwm as i32;
    }
    heap_implicit_head(response_root.get());
    if let Some(sr) = response_state_mut(response_root.get()) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            sr.buffered_body.extend_from_slice(&bytes);
        }
    }
    if heap::is_response(response_root.get()) {
        heap::restate(response_root.get());
        heap::set_field(
            response_root.get(),
            b"_headerSent",
            f64::from_bits(TAG_TRUE),
        );
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
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let callback_scope = perry_ffi::TransientRootScope::enter();
    let callback_root = callback_scope.root_addr(pick_trailing_callback(arg2, arg3));
    let callback = callback_root.get();
    let bytes = jsvalue_to_body_bytes(chunk);
    let ended = response_state(response_root.get())
        .map(|sr| sr.writable_ended)
        .unwrap_or(true);
    if ended {
        return f64::from_bits(TAG_TRUE);
    }
    if let Some(b) = &bytes {
        if let Some(below_hwm) = stream_write(response_root.get(), b) {
            // Streaming: the chunk is on its way to the wire, so the write
            // callback fires now rather than queueing for `.end()`.
            if callback != 0 {
                call_closure0(callback_root.get());
            }
            return f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE });
        }
    }
    let mut below_hwm = true;
    let mut accepted = false;
    heap_implicit_head(response_root.get());
    if let Some(sr) = response_state_mut(response_root.get()) {
        if !sr.writable_ended {
            accepted = true;
            sr.headers_sent = true;
            if let Some(b) = &bytes {
                sr.buffered_body.extend_from_slice(b);
            }
            below_hwm = sr.buffered_body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    }
    // Object-owned JS state may allocate or invoke a property accessor. End
    // the native borrow before touching it; reentrant JS may close the payload.
    if accepted && callback != 0 {
        push_write_callback(response_root.get(), callback_root.get());
    }
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::restate(response_root.get());
    }
    f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE })
}

/// `res.addTrailers(headers)` — store HTTP trailers emitted after the
/// response body, per Node's `ServerResponse.addTrailers`. Trailers carry
/// metadata that isn't known until the body has been produced.
#[no_mangle]
pub extern "C" fn js_node_http_res_add_trailers(handle: i64, headers_value: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
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
    if let Some(sr) = response_state_mut(response_root.get()) {
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
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::restate(response_root.get());
    }
}

/// What `finalize_buffered_end` leaves for the caller to emit.
pub(crate) enum EndOutcome {
    /// Nothing is still on its way to the socket: the caller runs the tail
    /// with these `'finish'` / `'close'` listener lists.
    Immediate { finish: Vec<i64>, close: Vec<i64> },
    /// The socket write callback runs the whole tail once the response bytes
    /// have reached the OS.
    Deferred,
}

/// Finalize a buffered response: append the final chunk and hand it to the
/// connection's encoder. On an HTTP/1.1 connection the write callback owns the
/// tail (queued write callbacks, `'finish'`, the end callback, `'close'`) and
/// the result is `Deferred`; otherwise the `(finish, close)` listener lists
/// come back **without** being fired — the caller controls ordering so that
/// `res.end(cb)` runs write/end callbacks before `'finish'` (Node's contract,
/// where `'finish'` never precedes the end callback). Returns `None` if the
/// response was already ended or the handle is gone.
pub(crate) fn finalize_buffered_end(
    handle: i64,
    chunk: f64,
    end_callback: i64,
) -> Option<EndOutcome> {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let end_callback = response_scope.root_addr(end_callback);
    let v = JsValue::from_bits(chunk.to_bits());
    let final_chunk = if v.is_undefined() || v.is_null() {
        None
    } else {
        jsvalue_to_body_bytes(chunk)
    };

    let original_headers = header_values::snapshot(response_root.get());
    let has_explicit_length = original_headers.iter().any(|(n, _)| {
        n.eq_ignore_ascii_case("content-length") || n.eq_ignore_ascii_case("transfer-encoding")
    });
    let sr = response_state_mut(response_root.get())?;
    if sr.writable_ended {
        return None;
    }

    // P5 streaming: the head already went to the wire on this thread, so the
    // final chunk and the trailer block are encoded and submitted directly.
    if sr.turnloop_streaming {
        let (conn, seq) = sr.turnloop.expect("streaming implies a turnloop target");
        let conn = conn.root();
        let chunk = final_chunk.clone();
        let trailers = sr.snapshot_trailers();
        sr.writable_ended = true;
        sr.writable_finished = true;
        sr.needs_drain = false;
        if let Some(c) = chunk {
            let _ = crate::server::turnloop_route::send_body(&conn, seq, &c);
        }
        let deferred = crate::server::turnloop_route::finish_body_notified(
            &conn,
            seq,
            &trailers,
            Some(&mut || {
                crate::server::response_end::flush_callback(response_root.get(), end_callback.get())
            }),
        );
        crate::server::request::mark_connection_written(req_handle_of(response_root.get()));
        return Some(end_outcome(response_root.get(), deferred));
    }

    if let Some(c) = final_chunk {
        sr.buffered_body.extend_from_slice(&c);
    }
    sr.headers_sent = true;
    sr.writable_ended = true;
    let auto_content_length = !has_explicit_length && sr.ensure_content_length();
    let body = std::mem::take(&mut sr.buffered_body);
    let mut headers = original_headers;
    if auto_content_length {
        headers.push(("Content-Length".into(), body.len().to_string()));
    }
    let trailers = sr.snapshot_trailers();
    let shape = ResponseShape {
        status: sr.status_code,
        status_message: sr.status_message.clone(),
        headers,
        trailers,
        body,
        auto_content_length,
    };
    let turnloop = sr.turnloop;
    let req_handle = sr.req_handle;
    // P5: the handler, the codec and the socket are on the same thread, so
    // the response is encoded and submitted here. A response with no
    // connection (the connection's request was never delivered) has nowhere
    // to go, and is simply finished.
    let mut deferred = false;
    if let Some((conn, seq)) = turnloop {
        let conn = conn.root();
        deferred = crate::server::turnloop_route::send_response_notified(
            &conn,
            seq,
            shape,
            Some(&mut || {
                crate::server::response_end::flush_callback(response_root.get(), end_callback.get())
            }),
        );
    }
    if let Some(sr) = response_state_mut(response_root.get()) {
        sr.writable_finished = true;
    }
    crate::server::request::mark_connection_written(req_handle);
    Some(end_outcome(response_root.get(), deferred))
}

fn end_outcome(handle: i64, deferred: bool) -> EndOutcome {
    if deferred {
        return EndOutcome::Deferred;
    }
    EndOutcome::Immediate {
        finish: response_event_listeners(handle, "finish"),
        close: response_event_listeners(handle, "close"),
    }
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
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let headers = header_values::snapshot(response_root.get());
    let Some(sr) = response_state_mut(response_root.get()) else {
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
        headers,
        trailers: Vec::new(),
        body: Vec::new(),
        auto_content_length: false,
    };
    let conn = conn.root();
    let first = std::mem::take(&mut sr.buffered_body);
    sr.headers_sent = true;
    sr.turnloop_streaming = true;
    if !crate::server::turnloop_route::begin_stream(&conn, seq, shape) {
        if let Some(sr) = response_state_mut(response_root.get()) {
            sr.turnloop_streaming = false;
        }
        return false;
    }
    if !first.is_empty() {
        let _ = crate::server::turnloop_route::send_body(&conn, seq, &first);
    }
    true
}

/// If a streaming response previously hit backpressure (`res.write()`
/// returned `false`) and its queued bytes have since drained below the
/// HWM, clear the flag and return its `'drain'` listeners for the caller
/// (the pump) to fire. Empty otherwise.
pub(crate) fn take_drain_listeners_if_ready(handle: i64) -> Vec<i64> {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let Some(sr) = response_state_mut(response_root.get()) else {
        return Vec::new();
    };
    if !sr.needs_drain || sr.writable_ended {
        return Vec::new();
    }
    let below = match sr.turnloop {
        Some((conn, seq)) if sr.turnloop_streaming => {
            crate::server::turnloop_route::writable_below_watermark(&conn.root(), seq)
        }
        _ => false,
    };
    if !below {
        return Vec::new();
    }
    sr.needs_drain = false;
    response_event_listeners(response_root.get(), "drain")
}

/// `res.flushHeaders()` — Node sends headers immediately even before
/// any body. Flushes the head to the wire and switches to the streaming
/// body path; falls back to marking headers-sent for handle flavors that
/// can't stream (standalone / bare OutgoingMessage).
#[no_mangle]
pub extern "C" fn js_node_http_res_flush_headers(handle: i64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    heap_implicit_head(response_root.get());
    if !begin_streaming(response_root.get()) {
        if let Some(sr) = response_state_mut(response_root.get()) {
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
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    response_root.get()
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
    let callback_scope = perry_ffi::TransientRootScope::enter();
    let callback_root = callback_scope.root_addr(callback);
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if heap::is_response(response_root.get()) {
        // A heap response is node's emitter: its listeners live in its
        // `_events`, through `EventEmitter.prototype.on`.
        return heap::add_listener(response_root.get(), b"on", &event, callback_root.get());
    }
    let mut should_fire_now = false;
    if let Some(sr) = get_handle_mut::<ServerResponse>(response_root.get()) {
        sr.listeners
            .entry(event.clone())
            .or_default()
            .push(callback_root.get());
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
    if should_fire_now && callback_root.get() != 0 {
        let raw = callback_root.get() as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0(perry_ffi::JsThis::UNDEFINED);
        }
    }
    handle_to_pointer_f64(response_root.get())
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
    let callback_scope = perry_ffi::TransientRootScope::enter();
    let callback_root = callback_scope.root_addr(callback);
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if heap::is_response(response_root.get()) {
        // A heap response is node's emitter: its listeners live in its
        // `_events`, through `EventEmitter.prototype.once`.
        return heap::add_listener(response_root.get(), b"once", &event, callback_root.get());
    }
    let mut should_fire_now = false;
    if let Some(sr) = get_handle_mut::<ServerResponse>(response_root.get()) {
        // `finish`/`close` already fired: run immediately without storing,
        // matching the late-registration behavior of `js_node_http_res_on`.
        if sr.writable_finished && (event == "finish" || event == "close") {
            should_fire_now = true;
        } else {
            sr.once_listeners
                .entry(event)
                .or_default()
                .push(callback_root.get());
        }
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if should_fire_now && callback_root.get() != 0 {
        let raw = callback_root.get() as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0(perry_ffi::JsThis::UNDEFINED);
        }
    }
    handle_to_pointer_f64(response_root.get())
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
    super::ensure_gc_scanner_registered();
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
    // Dispatch registration is independent of transport roots and pumps.
    // A response can be the program's first HTTP object.
    super::dispatch_ext::ensure_dispatch_extensions_registered();
    let scope = perry_ffi::TransientRootScope::enter();
    let req = scope.root_nanbox(req);
    heap::alloc(standalone_state(req.get()), req.get())
}

/// `ServerResponse.call(this, req)` / `super(req)`: the response is `this`
/// (node's constructor initializes it), so the native state attaches to it.
/// Returns `this`, or 0 when it cannot carry a response.
///
/// # Safety
/// FFI entry; `this` and `req` are live JS values.
pub(crate) unsafe fn server_response_construct_this(this: f64, req: f64) -> i64 {
    super::dispatch_ext::ensure_dispatch_extensions_registered();
    let scope = perry_ffi::TransientRootScope::enter();
    let this = scope.root_nanbox(this);
    let req = scope.root_nanbox(req);
    if !heap::attach_to(this.get(), standalone_state(req.get()), req.get()) {
        return 0;
    }
    (this.get().to_bits() & PTR_MASK) as i64
}

/// `OutgoingMessage.call(this)` / `super()`: node's constructor fields on
/// `this` (perry-runtime's `js_node_http_outgoing_message_init`). Returns
/// `this`.
///
/// # Safety
/// FFI entry; `this` is a live JS value.
pub(crate) unsafe fn outgoing_message_construct_this(this: f64) -> i64 {
    extern "C" {
        fn js_node_http_outgoing_message_init(this: f64);
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let this = scope.root_nanbox(this);
    js_node_http_outgoing_message_init(this.get());
    (this.get().to_bits() & PTR_MASK) as i64
}

/// The native state of a response not bound to a live connection. `req`
/// contributes only the method (a HEAD response has no body on flush).
unsafe fn standalone_state(req: f64) -> ResponseState {
    let mut sr = ResponseState::new();
    sr.standalone = true;
    sr.send_date = false;
    if JsValue::from_bits(req.to_bits()).is_pointer() {
        let m = perry_ffi::runtime_get(req, b"method");
        // Heap or inline SSO (`"GET"`, `"POST"` built at runtime, #11519).
        if let Some(method) = JsValue::from_bits(m.to_bits()).to_owned_string() {
            sr.standalone_req_method = Some(method);
        }
    }
    sr
}

/// `res.assignSocket(socket)` — wire a (possibly userland) Writable as the
/// flush target. Node throws `ERR_HTTP_SOCKET_ASSIGNED` on a second call.
#[no_mangle]
pub extern "C" fn js_node_http_res_assign_socket(handle: i64, socket: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let socket = response_scope.root_nanbox(socket);
    let already = response_state(response_root.get())
        .map(|_| !JsValue::from_bits(response_socket(response_root.get()).to_bits()).is_undefined())
        .unwrap_or(false);
    if already {
        perry_ffi::throw_with_code(
            "Socket already assigned",
            "ERR_HTTP_SOCKET_ASSIGNED",
            perry_ffi::ErrorKind::Error,
        );
    }
    if let Some(sr) = response_state_mut(response_root.get()) {
        sr.standalone = true;
    }
    set_response_socket(response_root.get(), socket.get());
    heap::relay_socket_close(response_root.get(), socket.get());
}

/// `res.detachSocket(socket)` — counterpart of `assignSocket`.
#[no_mangle]
pub extern "C" fn js_node_http_res_detach_socket(handle: i64, _socket: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    set_response_socket(response_root.get(), f64::from_bits(TAG_UNDEFINED));
}

/// `res.write(chunk[, encoding][, callback])` — callback-aware variant of
/// `js_node_http_res_write`. The callback queues until the buffered body
/// flushes on `.end()`, preserving call order (#4904).
#[no_mangle]
pub extern "C" fn js_node_http_res_write_with_cb(handle: i64, chunk: f64, callback: i64) -> i32 {
    let callback_scope = perry_ffi::TransientRootScope::enter();
    let callback_root = callback_scope.root_addr(callback);
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    // #4975: `write()` after `destroy()` must not buffer; Node invokes the
    // callback with an `ERR_STREAM_DESTROYED` error and returns `false` (no
    // `'error'` event is emitted, so an `on('error', …)` listener stays silent).
    if response_destroyed(response_root.get()) {
        if callback_root.get() != 0 {
            let err = perry_ffi::error_value_with_code(
                "Cannot call write after a stream was destroyed",
                "ERR_STREAM_DESTROYED",
                perry_ffi::ErrorKind::Error,
            );
            crate::server::http2_server::call1(callback_root.get(), f64::from_bits(err.bits()));
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
        let ended = response_state(response_root.get())
            .map(|sr| sr.writable_ended)
            .unwrap_or(true);
        if !ended {
            // Register the write callback BEFORE the data frame is published so
            // a receiver that drains immediately can't run it out of order
            // relative to later writes / `.end()` (Node fires it once the chunk
            // is flushed; queued, it drains in order, #4904). `stream_write_with_cb`
            // enqueues the callback ahead of the `tx.send`.
            if let Some(below_hwm) =
                stream_write_with_cb(response_root.get(), b, callback_root.get())
            {
                return below_hwm as i32;
            }
        }
    }
    // #4909 — real backpressure boolean (mirrors `js_node_http_res_write_full`
    // on the static path): `false` past the 16 KiB high-water mark, so dynamic
    // `while (res.write(buf, cb))` producer loops terminate.
    let mut below_hwm = true;
    let mut accepted = false;
    if let Some(sr) = response_state_mut(response_root.get()) {
        if !sr.writable_ended {
            accepted = true;
            sr.headers_sent = true;
            if let Some(b) = &bytes {
                sr.buffered_body.extend_from_slice(b);
            }
            below_hwm = sr.buffered_body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    }
    // Object-owned JS state may allocate or invoke a property accessor. End
    // the native borrow before touching it; reentrant JS may close the payload.
    if accepted && callback_root.get() != 0 {
        push_write_callback(response_root.get(), callback_root.get());
    }
    if accepted && heap::is_response(response_root.get()) {
        heap::restate(response_root.get());
        heap::set_field(
            response_root.get(),
            b"_headerSent",
            f64::from_bits(TAG_TRUE),
        );
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
    let scope = perry_ffi::TransientRootScope::enter();
    let socket = scope.root_nanbox(socket);
    let chunk_val = f64::from_bits(JsValue::from_string_ptr(alloc_string(chunk).as_raw()).bits());
    let args = [chunk_val];
    let _ = perry_ffi::runtime_call(socket.get(), b"write", &args);
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
    alloc_http1_server_response_for_turnloop, alloc_server_response_for_turnloop, req_handle_of,
    stream_receiver_gone,
};

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;

// State is owned either by an in-flight transport handle or by an ordinary
// standalone response's traced payload. The latter is never registered.
#[inline]
pub(crate) fn is_legacy_response_handle(handle: i64) -> bool {
    (handle as usize) < perry_ffi::RECEIVER_HANDLE_FLOOR
}
pub(crate) fn response_state(handle: i64) -> Option<&'static ResponseState> {
    if is_legacy_response_handle(handle) {
        get_handle::<ServerResponse>(handle).map(|r| &r.state)
    } else {
        unsafe { super::response_payload::state(handle) }.map(|r| &*r)
    }
}
pub(crate) fn response_state_mut(handle: i64) -> Option<&'static mut ResponseState> {
    if is_legacy_response_handle(handle) {
        get_handle_mut::<ServerResponse>(handle).map(|r| &mut r.state)
    } else {
        unsafe { super::response_payload::state(handle) }
    }
}

pub(crate) fn response_destroyed(handle: i64) -> bool {
    if let Some(v) = heap::field(handle, b"destroyed") {
        return heap::truthy(v);
    }
    response_state(handle).is_some_and(|s| s.destroyed)
}

pub(crate) fn response_socket(handle: i64) -> f64 {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::get(response_root.get(), ResponseKey::Socket)
    } else {
        get_handle::<ServerResponse>(response_root.get())
            .map_or(f64::from_bits(TAG_UNDEFINED), |r| r.standalone_socket)
    }
}
pub(crate) fn set_response_socket(handle: i64, socket: f64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::set(response_root.get(), ResponseKey::Socket, socket);
    } else if let Some(r) = get_handle_mut::<ServerResponse>(response_root.get()) {
        r.standalone_socket = socket;
    }
}
pub(crate) fn push_write_callback(handle: i64, cb: i64) {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::push(response_root.get(), ResponseKey::WriteCallbacks, cb);
    } else if let Some(r) = get_handle_mut::<ServerResponse>(response_root.get()) {
        r.pending_write_callbacks.push(cb);
    }
}
pub(crate) fn take_write_callbacks(handle: i64) -> Vec<i64> {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if super::response_payload::is_response(response_root.get()) {
        super::response_payload::callbacks(response_root.get(), ResponseKey::WriteCallbacks, true)
    } else {
        get_handle_mut::<ServerResponse>(response_root.get())
            .map(|r| std::mem::take(&mut r.pending_write_callbacks))
            .unwrap_or_default()
    }
}
pub(crate) fn response_event_listeners(handle: i64, event: &str) -> Vec<i64> {
    let response_scope = perry_ffi::TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    if heap::is_response(response_root.get()) {
        // Fired through the object's own `emit` (`response_payload::emit`).
        Vec::new()
    } else {
        get_handle_mut::<ServerResponse>(response_root.get())
            .map(|r| take_event_listeners(r, event))
            .unwrap_or_default()
    }
}

#[path = "response_header_values.rs"]
pub(crate) mod header_values;
#[cfg(test)]
#[path = "response_header_values_tests.rs"]
mod header_values_tests;
