//! `IncomingMessage` — the Node.js Readable stream returned to a
//! `(req, res) => …` server handler. Phase 1 buffers the full body
//! before dispatching the handler; `.on('data', cb)` and
//! `.on('end', cb)` fire synchronously against the buffered bytes.
//!
//! The synchronous-emit semantics matter because the canonical
//! Express body-collection pattern is:
//!
//! ```js
//! const body = await new Promise(resolve => {
//!   const chunks = [];
//!   req.on('data', c => chunks.push(c));
//!   req.on('end',  () => resolve(Buffer.concat(chunks)));
//! });
//! ```
//!
//! As long as we emit `'data'` on the first registration and `'end'`
//! on its registration, the Promise resolves correctly through
//! the microtask queue.

use std::collections::HashMap;

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::types::{
    jsvalue_to_owned_string, read_string_header, POINTER_TAG, PTR_MASK, STRING_TAG, TAG_UNDEFINED,
};

/// Per-request handle backing `IncomingMessage` JS-side. Stored in
/// the perry-ffi handle registry; both Rust (request dispatch path)
/// and TS (handler accessing `req.method`, `req.headers`, etc.) read
/// through this.
pub struct IncomingMessage {
    pub method: String,
    /// Path + optional `?query`, matching Node's `req.url`.
    pub url: String,
    /// Lowercase-keyed headers — Node's `req.headers` shape.
    pub headers: HashMap<String, String>,
    /// Original-case header pairs preserving duplicates — `req.rawHeaders`.
    pub raw_headers: Vec<(String, String)>,
    /// HTTP protocol version string, e.g. `"1.1"`.
    pub http_version: String,
    /// Fully-buffered request body. Phase 1 collects the entire body
    /// before invoking the user handler, so Readable-stream consumers
    /// can drain it on a single tick.
    pub body_bytes: Vec<u8>,
    /// True once `'end'` has been emitted (or would have been —
    /// on a 0-listener request, body is implicitly complete).
    pub complete: bool,
    /// True once `.destroy()` has been called.
    pub destroyed: bool,
    /// True if the underlying connection was aborted by the client.
    pub aborted: bool,
    /// Socket-info accessors (`req.socket.remoteAddress`, etc.).
    pub remote_address: String,
    pub remote_port: u16,
    /// Event-name → list of registered listener closure pointers.
    pub listeners: HashMap<String, Vec<i64>>,
    /// True once the buffered body has been emitted to a `'data'`
    /// listener — used to dedupe synchronous emit on multiple
    /// `req.on('data', …)` registrations.
    pub data_emitted: bool,
    /// True once `'end'` has fired.
    pub end_emitted: bool,
    /// Pause/resume flag — toggled by `.pause()` / `.resume()`. Phase 1
    /// honors this only for the synthetic data emit.
    pub paused: bool,
    /// Trailers placeholder — Node populates after `'end'`. We hand
    /// back an empty object until trailing-headers support lands.
    pub trailers: HashMap<String, String>,
}

impl IncomingMessage {
    pub fn new(
        method: String,
        url: String,
        headers: HashMap<String, String>,
        raw_headers: Vec<(String, String)>,
        body: Vec<u8>,
        remote_address: String,
        remote_port: u16,
    ) -> Self {
        Self {
            method,
            url,
            headers,
            raw_headers,
            http_version: "1.1".to_string(),
            body_bytes: body,
            complete: false,
            destroyed: false,
            aborted: false,
            remote_address,
            remote_port,
            listeners: HashMap::new(),
            data_emitted: false,
            end_emitted: false,
            paused: false,
            trailers: HashMap::new(),
        }
    }
}

// ============================================================================
// FFI surface
// ============================================================================

/// `req.method` — returns the uppercase HTTP method string.
#[no_mangle]
pub extern "C" fn js_node_http_im_method(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| im.method.clone())
        .unwrap_or_default();
    alloc_string(&s).as_raw()
}

/// `req.url` — path + optional query string.
#[no_mangle]
pub extern "C" fn js_node_http_im_url(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| im.url.clone())
        .unwrap_or_default();
    alloc_string(&s).as_raw()
}

/// `req.httpVersion` — `"1.0"` / `"1.1"` / `"2.0"`.
#[no_mangle]
pub extern "C" fn js_node_http_im_http_version(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| im.http_version.clone())
        .unwrap_or_else(|| "1.1".to_string());
    alloc_string(&s).as_raw()
}

/// `req.headers` — JSON-stringify the lowercase-keyed header map.
/// Returned as a NaN-boxed STRING — TS-side parses with `JSON.parse`
/// at the binding wrapper. (Returning a runtime ObjectHeader directly
/// would require building shape metadata for an arbitrary key set;
/// JSON round-trip is simpler and same approach perry-ext-axios uses.)
#[no_mangle]
pub extern "C" fn js_node_http_im_headers_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| serde_json::to_string(&im.headers).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&s).as_raw()
}

/// `req.rawHeaders` — JSON-stringify the original-case `[name, value, ...]`
/// flat list (alternating). TS-side reconstructs an array of strings.
#[no_mangle]
pub extern "C" fn js_node_http_im_raw_headers_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| {
            let flat: Vec<&str> = im
                .raw_headers
                .iter()
                .flat_map(|(k, v)| [k.as_str(), v.as_str()])
                .collect();
            serde_json::to_string(&flat).unwrap_or_else(|_| "[]".to_string())
        })
        .unwrap_or_else(|| "[]".to_string());
    alloc_string(&s).as_raw()
}

/// `req.complete` — `true` once body has been fully received +
/// emitted to listeners.
#[no_mangle]
pub extern "C" fn js_node_http_im_complete(handle: i64) -> i32 {
    get_handle::<IncomingMessage>(handle)
        .map(|im| if im.complete { 1 } else { 0 })
        .unwrap_or(0)
}

/// `req.aborted` — `true` if the underlying connection was reset.
#[no_mangle]
pub extern "C" fn js_node_http_im_aborted(handle: i64) -> i32 {
    get_handle::<IncomingMessage>(handle)
        .map(|im| if im.aborted { 1 } else { 0 })
        .unwrap_or(0)
}

/// `req.destroyed` — `true` after `.destroy()` was called.
#[no_mangle]
pub extern "C" fn js_node_http_im_destroyed(handle: i64) -> i32 {
    get_handle::<IncomingMessage>(handle)
        .map(|im| if im.destroyed { 1 } else { 0 })
        .unwrap_or(0)
}

/// `req.socket.remoteAddress` — peer IP as a dotted string.
#[no_mangle]
pub extern "C" fn js_node_http_im_remote_address(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| im.remote_address.clone())
        .unwrap_or_default();
    alloc_string(&s).as_raw()
}

/// `req.socket.remotePort` — peer ephemeral port.
#[no_mangle]
pub extern "C" fn js_node_http_im_remote_port(handle: i64) -> f64 {
    get_handle::<IncomingMessage>(handle)
        .map(|im| im.remote_port as f64)
        .unwrap_or(0.0)
}

/// `req.pause()` — record the paused flag. Phase 1 only honors it for
/// the synthetic single-shot data emit (a paused request defers its
/// `'data'` event until the next `resume()` or `read()`).
#[no_mangle]
pub extern "C" fn js_node_http_im_pause(handle: i64) {
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.paused = true;
    }
}

/// `req.resume()` — clear the paused flag. If a `'data'` listener was
/// registered while paused and we still have body bytes to emit,
/// the event-loop iterator will pick the request up on its next pass.
#[no_mangle]
pub extern "C" fn js_node_http_im_resume(handle: i64) {
    let mut should_emit_data = false;
    let mut should_emit_end = false;
    let body_bytes;
    let data_listeners;
    let end_listeners;
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.paused = false;
        if !im.data_emitted && !im.body_bytes.is_empty() {
            should_emit_data = true;
        }
        if !im.end_emitted {
            should_emit_end = true;
        }
        body_bytes = im.body_bytes.clone();
        data_listeners = im.listeners.get("data").cloned().unwrap_or_default();
        end_listeners = im.listeners.get("end").cloned().unwrap_or_default();
        if should_emit_data {
            im.data_emitted = true;
        }
        if should_emit_end {
            im.end_emitted = true;
            im.complete = true;
        }
    } else {
        return;
    }
    if should_emit_data {
        emit_data_to_listeners(&data_listeners, &body_bytes);
    }
    if should_emit_end {
        emit_end_to_listeners(&end_listeners);
    }
}

/// `req.destroy()` — mark destroyed and fire `'close'`.
#[no_mangle]
pub extern "C" fn js_node_http_im_destroy(handle: i64) {
    let close_listeners;
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.destroyed = true;
        close_listeners = im.listeners.get("close").cloned().unwrap_or_default();
    } else {
        return;
    }
    emit_no_arg_to_listeners(&close_listeners);
}

/// `req.on(event, cb)` — register a listener. For `'data'` and `'end'`
/// the listener fires immediately against the buffered body so the
/// canonical Express body-collection pattern resolves on its
/// microtask. `'close'` and `'error'` listeners are stored but not
/// emitted by Phase 1 (no streaming error path yet).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_im_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    let body_to_emit;
    let should_emit_end;
    {
        let im = match get_handle_mut::<IncomingMessage>(handle) {
            Some(im) => im,
            None => {
                // Issue #1124 followup — the same `("http",
                // "IncomingMessage", "on")` dispatch row services
                // BOTH the server-side IncomingMessage (registered
                // here in perry-ext-http-server) and the client-side
                // IncomingMessage that `http.get(url, (res) => …)`'s
                // callback receives from perry-ext-http. The
                // codegen can't distinguish them at compile time
                // (the HIR pre-scan tags the param as
                // `("http", "IncomingMessage")` regardless of
                // factory), so we cross-route here on a miss:
                // forward to perry-ext-http's `js_http_on` which
                // checks its own registry and registers the
                // listener under the client-side `IncomingMessageHandle`.
                // Without this, client `res.on('end', cb)` was a
                // silent no-op and the `'end'` event never fired,
                // even with the new Buffer-shaped data dispatch.
                extern "C" {
                    fn js_http_on(
                        handle: i64,
                        event_ptr: *const StringHeader,
                        callback: i64,
                    ) -> i64;
                }
                let _ = js_http_on(handle, event_name_ptr, callback);
                // Node's `.on()` returns the receiver — re-NaN-box
                // the handle so chained calls still work.
                return f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK));
            }
        };
        im.listeners
            .entry(event.clone())
            .or_default()
            .push(callback);

        // Synchronous emit semantics for the Readable-stream surface.
        match event.as_str() {
            "data" if !im.paused && !im.data_emitted && !im.body_bytes.is_empty() => {
                im.data_emitted = true;
                body_to_emit = Some(im.body_bytes.clone());
                should_emit_end = false;
            }
            "end" if !im.paused && !im.end_emitted => {
                im.end_emitted = true;
                im.complete = true;
                body_to_emit = None;
                should_emit_end = true;
            }
            _ => {
                body_to_emit = None;
                should_emit_end = false;
            }
        }
    }

    if let Some(bytes) = body_to_emit {
        let cbs = vec![callback];
        emit_data_to_listeners(&cbs, &bytes);
    }
    if should_emit_end {
        let cbs = vec![callback];
        emit_end_to_listeners(&cbs);
    }
    // Node's `req.on` returns the IncomingMessage itself for chaining.
    // Return the same handle re-NaN-boxed so `req.on().on()` works.
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

/// `req.read()` — return a buffered chunk as a string, or null if
/// nothing left. Phase 1 returns the full body on first call, then
/// `null` thereafter.
#[no_mangle]
pub extern "C" fn js_node_http_im_read(handle: i64) -> f64 {
    let bytes = match get_handle_mut::<IncomingMessage>(handle) {
        Some(im) => {
            if im.data_emitted {
                return f64::from_bits(crate::types::TAG_NULL);
            }
            im.data_emitted = true;
            im.body_bytes.clone()
        }
        None => return f64::from_bits(crate::types::TAG_NULL),
    };
    if bytes.is_empty() {
        return f64::from_bits(crate::types::TAG_NULL);
    }
    let s = String::from_utf8_lossy(&bytes).into_owned();
    let header = alloc_string(&s);
    f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
}

// ============================================================================
// Internal helpers — listener invocation
// ============================================================================

/// Fire a `'data'` event to every registered listener, passing the
/// body bytes as a single string chunk.
pub(crate) fn emit_data_to_listeners(listeners: &[i64], body: &[u8]) {
    if listeners.is_empty() || body.is_empty() {
        return;
    }
    let s = String::from_utf8_lossy(body).into_owned();
    let header = alloc_string(&s);
    let chunk_f64 = f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK));
    for cb in listeners {
        if *cb == 0 {
            continue;
        }
        unsafe {
            let raw = *cb as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                let _ = closure.call1(chunk_f64);
            }
        }
    }
}

/// Fire a no-arg event (`'end'`, `'close'`).
pub(crate) fn emit_end_to_listeners(listeners: &[i64]) {
    emit_no_arg_to_listeners(listeners);
}

pub(crate) fn emit_no_arg_to_listeners(listeners: &[i64]) {
    for cb in listeners {
        if *cb == 0 {
            continue;
        }
        unsafe {
            let raw = *cb as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                let _ = closure.call0();
            }
        }
    }
}

// ============================================================================
// Allocation helper used by server.rs
// ============================================================================

/// Allocate a fresh `IncomingMessage` and return its handle id.
pub(crate) fn alloc_incoming_message(im: IncomingMessage) -> i64 {
    register_handle(im)
}

/// Re-NaN-box a handle id with `POINTER_TAG` so JS-side method dispatch
/// (codegen treats it as an object pointer, calls accessors via the
/// vtable) Just Works.
pub(crate) fn handle_to_pointer_f64(handle: i64) -> f64 {
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

#[allow(dead_code)]
pub(crate) fn _force_jsvalue_link(v: f64) -> Option<String> {
    jsvalue_to_owned_string(v)
}

#[allow(dead_code)]
pub(crate) fn _force_jsvalue_extract(v: f64) -> bool {
    JsValue::from_bits(v.to_bits()).is_pointer()
}
