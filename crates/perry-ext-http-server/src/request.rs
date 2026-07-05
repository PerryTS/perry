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

use std::collections::{BTreeMap, HashMap};

use perry_ffi::{
    alloc_buffer, alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::types::{
    jsvalue_to_owned_string, read_string_header, POINTER_TAG, PTR_MASK, STRING_TAG,
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
    /// Encoding requested through `req.setEncoding(enc)`. `None`
    /// preserves Node's default Buffer chunks.
    pub encoding: Option<String>,
    /// Trailers placeholder — Node populates after `'end'`. We hand
    /// back an empty object until trailing-headers support lands.
    pub trailers: HashMap<String, String>,
    /// Lazily-created AbortController/AbortSignal pair backing
    /// `req.signal`.
    pub signal_controller: f64,
    pub signal: f64,
    /// True once `'close'` has fired.
    pub close_emitted: bool,
    /// #4904: true for `new http.IncomingMessage(socket)` instances —
    /// standalone messages not attached to any live connection.
    pub standalone: bool,
    /// #4904: the socket value Node mirrors through `req.socket` /
    /// `req.connection`. For standalone instances this holds the
    /// constructor argument verbatim; assigning either alias
    /// (`req.connection = v`) overwrites it (Node's `connection`
    /// accessor writes `this.socket`).
    pub socket_value: f64,
    /// #4904: true once `socket`/`connection` has been assigned, so
    /// server-attached requests also honor the override on reads.
    pub socket_overridden: bool,
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
            encoding: None,
            trailers: HashMap::new(),
            signal_controller: f64::from_bits(crate::types::TAG_UNDEFINED),
            signal: f64::from_bits(crate::types::TAG_UNDEFINED),
            close_emitted: false,
            standalone: false,
            socket_value: f64::from_bits(crate::types::TAG_UNDEFINED),
            socket_overridden: false,
        }
    }
}

extern "C" {
    fn js_abort_controller_new() -> *mut perry_ffi::ObjectHeader;
    fn js_abort_controller_signal(
        controller: *mut perry_ffi::ObjectHeader,
    ) -> *mut perry_ffi::ObjectHeader;
    fn js_abort_controller_abort(controller: *mut perry_ffi::ObjectHeader);
}

fn is_undefined(value: f64) -> bool {
    JsValue::from_bits(value.to_bits()).is_undefined()
}

fn object_value<T>(ptr: *mut T) -> f64 {
    if ptr.is_null() {
        f64::from_bits(crate::types::TAG_UNDEFINED)
    } else {
        f64::from_bits(JsValue::from_object_ptr(ptr).bits())
    }
}

fn ensure_signal(im: &mut IncomingMessage) -> f64 {
    if !is_undefined(im.signal) {
        return im.signal;
    }

    unsafe {
        let controller = js_abort_controller_new();
        let signal = js_abort_controller_signal(controller);
        im.signal_controller = object_value(controller);
        im.signal = object_value(signal);
    }
    im.signal
}

fn abort_signal(im: &mut IncomingMessage) {
    let controller =
        JsValue::from_bits(im.signal_controller.to_bits()).as_pointer::<perry_ffi::ObjectHeader>();
    if !controller.is_null() {
        unsafe {
            js_abort_controller_abort(controller);
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

/// `req.httpVersionMajor` — numeric major half of `httpVersion`.
#[no_mangle]
pub extern "C" fn js_node_http_im_http_version_major(handle: i64) -> f64 {
    incoming_http_version_part(handle, false)
}

/// `req.httpVersionMinor` — numeric minor half of `httpVersion`.
#[no_mangle]
pub extern "C" fn js_node_http_im_http_version_minor(handle: i64) -> f64 {
    incoming_http_version_part(handle, true)
}

/// `req.httpVersionMajor` / `req.httpVersionMinor` — numeric halves of
/// `httpVersion` ("1.0" → 1 / 0).
pub(crate) fn incoming_http_version_part(handle: i64, minor: bool) -> f64 {
    let version = get_handle::<IncomingMessage>(handle)
        .map(|im| im.http_version.clone())
        .unwrap_or_else(|| "1.1".to_string());
    let mut parts = version.split('.');
    let major = parts
        .next()
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(1.0);
    let minor_v = parts
        .next()
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(1.0);
    if minor {
        minor_v
    } else {
        major
    }
}

/// `req.headers` — JSON-stringify the lowercase-keyed header map.
/// Returned as a NaN-boxed STRING — TS-side parses with `JSON.parse`
/// at the binding wrapper. (Returning a runtime ObjectHeader directly
/// would require building shape metadata for an arbitrary key set;
/// JSON round-trip is simpler and same approach perry-ext-axios uses.)
#[no_mangle]
pub extern "C" fn js_node_http_im_headers_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| combined_headers_json(&im.raw_headers))
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&s).as_raw()
}

/// Single-value request headers: per Node's `_http_incoming.js`
/// `matchKnownFields`, duplicates of these are discarded (first wins)
/// rather than joined with `, `. `set-cookie` is excluded — it always
/// accumulates into an array.
fn is_single_value_header(name: &str) -> bool {
    matches!(
        name,
        "age"
            | "authorization"
            | "content-length"
            | "content-type"
            | "etag"
            | "expires"
            | "from"
            | "host"
            | "if-modified-since"
            | "if-unmodified-since"
            | "last-modified"
            | "location"
            | "max-forwards"
            | "proxy-authorization"
            | "referer"
            | "retry-after"
            | "server"
            | "user-agent"
    )
}

/// Build the combined `req.headers` JSON object from the raw
/// `(name, value)` pairs, applying Node's `matchKnownFields` rules
/// (#5079): `set-cookie` → string array (even for one cookie),
/// single-value fields keep-first, everything else joined with `, `.
/// Keys are lower-cased to match Node's `headers` view.
fn combined_headers_json(raw: &[(String, String)]) -> String {
    use serde_json::Value;
    // Key order in the serialized object is not significant here (the
    // previous `HashMap` serialization was already unordered); what
    // matters is that `set-cookie` surfaces as an array and other
    // duplicates combine per Node's rules.
    let mut map = serde_json::Map::new();
    for (name, value) in raw {
        let key = name.to_ascii_lowercase();
        if key == "set-cookie" {
            match map.get_mut(&key) {
                Some(Value::Array(arr)) => arr.push(Value::String(value.clone())),
                _ => {
                    map.insert(key, Value::Array(vec![Value::String(value.clone())]));
                }
            }
            continue;
        }
        match map.get_mut(&key) {
            Some(Value::String(existing)) => {
                if !is_single_value_header(&key) {
                    // Node's `matchKnownFields`: duplicate `cookie` headers are
                    // joined with "; ", everything else with ", ".
                    existing.push_str(if key == "cookie" { "; " } else { ", " });
                    existing.push_str(value);
                }
            }
            _ => {
                map.insert(key, Value::String(value.clone()));
            }
        }
    }
    serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string())
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

/// `req.headersDistinct` — lowercase keys mapped to arrays of values,
/// preserving duplicates from `rawHeaders`.
#[no_mangle]
pub extern "C" fn js_node_http_im_headers_distinct_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| {
            let mut distinct: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (name, value) in &im.raw_headers {
                distinct
                    .entry(name.to_lowercase())
                    .or_default()
                    .push(value.clone());
            }
            serde_json::to_string(&distinct).unwrap_or_else(|_| "{}".to_string())
        })
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&s).as_raw()
}

/// `req.trailers` — JSON-stringify the lowercase-keyed trailer map.
#[no_mangle]
pub extern "C" fn js_node_http_im_trailers_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| {
            let trailers: BTreeMap<&String, &String> = im.trailers.iter().collect();
            serde_json::to_string(&trailers).unwrap_or_else(|_| "{}".to_string())
        })
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&s).as_raw()
}

/// `req.rawTrailers` — flat original-case trailer list. Trailers are not
/// collected yet, so this is currently the Node-compatible empty array.
#[no_mangle]
pub extern "C" fn js_node_http_im_raw_trailers_json(_handle: i64) -> *mut StringHeader {
    alloc_string("[]").as_raw()
}

/// `req.trailersDistinct` — lowercase trailer keys mapped to arrays.
#[no_mangle]
pub extern "C" fn js_node_http_im_trailers_distinct_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<IncomingMessage>(handle)
        .map(|im| {
            let mut distinct: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (name, value) in &im.trailers {
                distinct
                    .entry(name.to_lowercase())
                    .or_default()
                    .push(value.clone());
            }
            serde_json::to_string(&distinct).unwrap_or_else(|_| "{}".to_string())
        })
        .unwrap_or_else(|| "{}".to_string());
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

/// `req.readable` — `true` while the request body stream can still be consumed:
/// not yet fully received (`complete`), not destroyed, not aborted. Node's
/// `Readable.readable` flips to `false` once the stream ends. `req.socket`
/// aliases the request handle, so `on-finished`'s `isFinished()` consults
/// `socket.readable` through this; returning `true` up front keeps express
/// body-parser from treating an unread POST body as already finished.
#[no_mangle]
pub extern "C" fn js_node_http_im_readable(handle: i64) -> i32 {
    get_handle::<IncomingMessage>(handle)
        .map(|im| {
            if !im.complete && !im.destroyed && !im.aborted {
                1
            } else {
                0
            }
        })
        .unwrap_or(0)
}

/// `req.rawBody` — the fully-collected request body as a `Buffer`.
///
/// Perry's HTTP server buffers the entire request body before invoking the
/// handler (`req.collect().await`), so the bytes are available synchronously
/// here. `@hono/node-server` checks `"rawBody" in incoming && incoming.rawBody
/// instanceof Buffer` and, when present, builds the `Request` body from it via a
/// synchronous single-chunk `ReadableStream` — avoiding the data-less
/// `Readable.toWeb(incoming)` stub path (the #1540 Node↔WHATWG stream gap).
/// Exposing it makes `await c.req.text()` / `.json()` / `.formData()` on a POST
/// resolve to the real body instead of an empty/garbage value. Returns an empty
/// Buffer when there is no body (harmless: node-server only consults it for
/// non-GET/HEAD methods).
#[no_mangle]
pub extern "C" fn js_node_http_im_raw_body(handle: i64) -> f64 {
    let bytes = get_handle::<IncomingMessage>(handle)
        .map(|im| im.body_bytes.clone())
        .unwrap_or_default();
    let buf = alloc_buffer(&bytes);
    if buf.is_null() {
        f64::from_bits(crate::types::TAG_UNDEFINED)
    } else {
        f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
    }
}

/// `req.signal` — lazily-created AbortSignal for the request lifetime.
#[no_mangle]
pub extern "C" fn js_node_http_im_signal(handle: i64) -> f64 {
    get_handle_mut::<IncomingMessage>(handle)
        .map(ensure_signal)
        .unwrap_or_else(|| f64::from_bits(crate::types::TAG_UNDEFINED))
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

/// Chainable `req.pause()` — Node's `Readable.pause()` returns `this`, so
/// `res.pause().on(...)` keeps flowing. Returns the receiver handle (boxed
/// as a pointer via the `NR_PTR` row) instead of `undefined`/a raw number.
/// Mirrors `js_node_http_res_set_header_self` (#5011/#2129 self-return).
#[no_mangle]
pub extern "C" fn js_node_http_im_pause_self(handle: i64) -> i64 {
    js_node_http_im_pause(handle);
    handle
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
    let encoding;
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.paused = false;
        body_bytes = im.body_bytes.clone();
        data_listeners = im.listeners.get("data").cloned().unwrap_or_default();
        end_listeners = im.listeners.get("end").cloned().unwrap_or_default();
        encoding = im.encoding.clone();
        // #4909 — only consume the one-shot emit flags when a listener is
        // actually present. `req.resume(); req.on('end', cb)` (the canonical
        // body-drain pattern) registers the listener one statement AFTER
        // resume; marking `end_emitted` here meant `im_on`'s sync-emit arm
        // saw the event as already delivered and the server hung without
        // ever responding.
        if !im.data_emitted && !im.body_bytes.is_empty() && !data_listeners.is_empty() {
            should_emit_data = true;
            im.data_emitted = true;
        }
        if !im.end_emitted && !end_listeners.is_empty() {
            should_emit_end = true;
            im.end_emitted = true;
            im.complete = true;
        }
    } else {
        return;
    }
    if should_emit_data {
        emit_data_to_listeners(&data_listeners, &body_bytes, encoding.as_deref());
    }
    if should_emit_end {
        emit_end_to_listeners(&end_listeners);
    }
}

/// Chainable `req.resume()` — Node's `Readable.resume()` returns `this`, so
/// the canonical `res.resume().on('end', …)` body-drain chain keeps flowing.
/// Returns the receiver handle (boxed as a pointer via the `NR_PTR` row)
/// instead of `undefined`/a raw number. Mirrors
/// `js_node_http_res_set_header_self` (#5011/#2129 self-return).
#[no_mangle]
pub extern "C" fn js_node_http_im_resume_self(handle: i64) -> i64 {
    js_node_http_im_resume(handle);
    handle
}

/// `req.destroy()` — mark destroyed and fire `'close'`.
#[no_mangle]
pub extern "C" fn js_node_http_im_destroy(handle: i64) {
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.destroyed = true;
    } else {
        return;
    }
    close_incoming_message(handle);
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
    let encoding_to_emit;
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
                encoding_to_emit = im.encoding.clone();
                should_emit_end = false;
            }
            "end" if !im.paused && !im.end_emitted => {
                im.end_emitted = true;
                im.complete = true;
                body_to_emit = None;
                encoding_to_emit = None;
                should_emit_end = true;
            }
            _ => {
                body_to_emit = None;
                encoding_to_emit = None;
                should_emit_end = false;
            }
        }
    }

    if let Some(bytes) = body_to_emit {
        let cbs = vec![callback];
        emit_data_to_listeners(&cbs, &bytes, encoding_to_emit.as_deref());
    }
    if should_emit_end {
        let cbs = vec![callback];
        emit_end_to_listeners(&cbs);
    }
    // Node's `req.on` returns the IncomingMessage itself for chaining.
    // Return the same handle re-NaN-boxed so `req.on().on()` works.
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

/// `req.setEncoding(encoding)` — switch future `'data'` events from Buffer
/// chunks to decoded string chunks. Returns the receiver for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_im_set_encoding(
    handle: i64,
    encoding_ptr: *const StringHeader,
) -> i64 {
    let encoding = read_string_header(encoding_ptr as *mut _).unwrap_or_else(|| "utf8".to_string());
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.encoding = Some(encoding);
    }
    handle
}

/// `req.setTimeout(msecs[, callback])` — timeout scheduling is transport-owned
/// for now; expose Node's chainable shape.
#[no_mangle]
pub extern "C" fn js_node_http_im_set_timeout(handle: i64, _msecs: f64, _callback: i64) -> i64 {
    handle
}

/// `req.read()` — return a buffered chunk as a string, or null if
/// nothing left. Phase 1 returns the full body on first call, then
/// `null` thereafter.
#[no_mangle]
pub extern "C" fn js_node_http_im_read(handle: i64) -> f64 {
    let (bytes, encoding) = match get_handle_mut::<IncomingMessage>(handle) {
        Some(im) => {
            if im.data_emitted {
                return f64::from_bits(crate::types::TAG_NULL);
            }
            im.data_emitted = true;
            (im.body_bytes.clone(), im.encoding.clone())
        }
        None => return f64::from_bits(crate::types::TAG_NULL),
    };
    if bytes.is_empty() {
        return f64::from_bits(crate::types::TAG_NULL);
    }
    // #5437 POST-body: Node's `req.read()` returns a Buffer by default and a
    // string only after `setEncoding()`. `Readable.toWeb(req)` (Next.js App
    // Router's request-body adapter) and the Web `Request` body reader require
    // Uint8Array/Buffer chunks — a string chunk is dropped, so `request.json()`
    // saw `{}`. Mirror `emit_data_to_listeners`: Buffer by default, string only
    // when an encoding was set.
    match encoding {
        Some(_) => {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            let header = alloc_string(&s);
            f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
        }
        None => {
            let buf = alloc_buffer(&bytes);
            if buf.is_null() {
                return f64::from_bits(crate::types::TAG_NULL);
            }
            f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
        }
    }
}

// ============================================================================
// Internal helpers — listener invocation
// ============================================================================

/// Fire a `'data'` event to every registered listener, passing the
/// body bytes as a Buffer by default or as decoded text after setEncoding().
pub(crate) fn emit_data_to_listeners(listeners: &[i64], body: &[u8], encoding: Option<&str>) {
    if listeners.is_empty() || body.is_empty() {
        return;
    }
    let chunk_f64 = match encoding {
        Some(_) => {
            let s = String::from_utf8_lossy(body).into_owned();
            let header = alloc_string(&s);
            f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
        }
        None => {
            let buf = alloc_buffer(body);
            if buf.is_null() {
                return;
            }
            f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
        }
    };
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

/// Mark the request as closed, abort `req.signal`, and fire `'close'` once.
pub(crate) fn close_incoming_message(handle: i64) {
    let close_listeners;
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        if im.close_emitted {
            return;
        }
        im.close_emitted = true;
        close_listeners = im.listeners.get("close").cloned().unwrap_or_default();
        if !close_listeners.is_empty() {
            ensure_signal(im);
        }
        abort_signal(im);
    } else {
        return;
    }
    emit_no_arg_to_listeners(&close_listeners);
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

extern "C" {
    /// `perry-runtime`'s implicit-`this` cell setter (resolved at final
    /// link; declared here because `perry-runtime` is only a
    /// dev-dependency of this crate). Returns the previous value.
    fn js_implicit_this_set(value: f64) -> f64;
}

/// Run `f` with the runtime's implicit-`this` cell bound to `this_val`,
/// restoring the previous binding afterward. Node invokes server,
/// socket, and request callbacks with `this` bound to the emitting
/// object, so the canonical
/// `server.listen(0, function() { this.address().port })` idiom
/// resolves `this` to the server (#2132).
pub(crate) fn with_implicit_this<R>(this_val: f64, f: impl FnOnce() -> R) -> R {
    let prev = unsafe { js_implicit_this_set(this_val) };
    let r = f();
    unsafe { js_implicit_this_set(prev) };
    r
}

// ============================================================================
// #4904: standalone `new http.IncomingMessage(socket)` support
// ============================================================================

/// `new http.IncomingMessage(socket)` — construct a standalone message not
/// attached to any live connection. Node keeps the constructor argument
/// verbatim on `req.socket` / `req.connection` and leaves the parse-state
/// fields empty.
#[no_mangle]
pub extern "C" fn js_node_http_incoming_message_standalone_new(socket: f64) -> i64 {
    crate::ensure_gc_scanner_registered();
    let mut im = IncomingMessage::new(
        String::new(),
        String::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        String::new(),
        0,
    );
    im.standalone = true;
    im.socket_value = socket;
    register_handle(im)
}

/// `req.socket` / `req.connection` read override. `None` means the request
/// is server-attached and untouched — dispatch falls back to the legacy
/// self-pointer placeholder.
pub(crate) fn incoming_socket_override(handle: i64) -> Option<f64> {
    get_handle::<IncomingMessage>(handle).and_then(|im| {
        if im.standalone || im.socket_overridden {
            Some(im.socket_value)
        } else {
            None
        }
    })
}

/// `req.socket = v` / `req.connection = v`. Node's `connection` accessor
/// writes `this.socket`, so both aliases land on the same slot.
pub(crate) fn incoming_socket_assign(handle: i64, value: f64) -> bool {
    if let Some(im) = get_handle_mut::<IncomingMessage>(handle) {
        im.socket_value = value;
        im.socket_overridden = true;
        true
    } else {
        false
    }
}

/// Header-field classification mirroring Node's `matchKnownFields`
/// (lib/_http_incoming.js): the flag drives `_addHeaderLine` dedupe/merge.
enum HeaderFieldKind {
    /// Known single-value field — duplicate lines are dropped (first wins).
    Single,
    /// `', '`-joined list (known list fields and every unknown field).
    List,
    /// `Cookie` — joined with `'; '`.
    Cookie,
    /// `Set-Cookie` — accumulates an array.
    SetCookie,
}

fn match_known_fields(field: &str) -> (HeaderFieldKind, String) {
    // Node first tries an exact match against the canonical and all-lowercase
    // spellings, then retries fully lowercased — observably identical to
    // lowercasing up front and matching the lowercase table.
    let lower = field.to_lowercase();
    let kind = match lower.as_str() {
        "age"
        | "host"
        | "from"
        | "etag"
        | "server"
        | "referer"
        | "expires"
        | "location"
        | "user-agent"
        | "retry-after"
        | "content-type"
        | "max-forwards"
        | "authorization"
        | "last-modified"
        | "content-length"
        | "if-modified-since"
        | "proxy-authorization"
        | "if-unmodified-since" => HeaderFieldKind::Single,
        "set-cookie" => HeaderFieldKind::SetCookie,
        "cookie" => HeaderFieldKind::Cookie,
        // Known list fields ("date", "vary", "origin", "expect", "accept",
        // "upgrade", "if-match", "connection", "cache-control",
        // "if-none-match", "accept-encoding", "accept-language",
        // "x-forwarded-for", "content-encoding", "x-forwarded-host",
        // "transfer-encoding", "x-forwarded-proto") and every unknown field
        // share the same ', '-join behavior.
        _ => HeaderFieldKind::List,
    };
    (kind, lower)
}

/// Stringify a JS value the way `'' + value` would inside Node's
/// `dest[field] += ', ' + value` merge.
fn header_value_to_string(value: f64) -> String {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_undefined() {
        return "undefined".to_string();
    }
    if v.is_null() {
        return "null".to_string();
    }
    jsvalue_to_owned_string(value).unwrap_or_default()
}

/// Node's `IncomingMessage.prototype._addHeaderLine(field, value, dest)` —
/// internal-by-convention API exercised directly by Node's own tests and by
/// userland HTTP shims (#4904). Mutates `dest` in place per the
/// `matchKnownFields` flag.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_im_add_header_line(
    _handle: i64,
    field: f64,
    value: f64,
    dest: f64,
) {
    extern "C" {
        fn js_object_get_field_by_name(
            obj: *const perry_ffi::ObjectHeader,
            key: *const StringHeader,
        ) -> JsValue;
        fn js_object_set_field_by_name(
            obj: *mut perry_ffi::ObjectHeader,
            key: *const StringHeader,
            value: f64,
        );
    }
    if !JsValue::from_bits(dest.to_bits()).is_pointer() {
        return;
    }
    let dest_ptr = (dest.to_bits() & PTR_MASK) as *mut perry_ffi::ObjectHeader;
    let field_name = jsvalue_to_owned_string(field).unwrap_or_default();
    let (kind, name) = match_known_fields(&field_name);
    let key = alloc_string(&name);
    let existing = js_object_get_field_by_name(dest_ptr as *const _, key.as_raw());
    let existing_f64 = f64::from_bits(existing.bits());
    match kind {
        HeaderFieldKind::List | HeaderFieldKind::Cookie => {
            if JsValue::from_bits(existing.bits()).is_string() {
                let sep = if matches!(kind, HeaderFieldKind::Cookie) {
                    "; "
                } else {
                    ", "
                };
                let merged = format!(
                    "{}{}{}",
                    jsvalue_to_owned_string(existing_f64).unwrap_or_default(),
                    sep,
                    header_value_to_string(value),
                );
                let merged_str = alloc_string(&merged);
                js_object_set_field_by_name(
                    dest_ptr,
                    key.as_raw(),
                    f64::from_bits(STRING_TAG | (merged_str.as_raw() as u64 & PTR_MASK)),
                );
            } else {
                js_object_set_field_by_name(dest_ptr, key.as_raw(), value);
            }
        }
        HeaderFieldKind::SetCookie => {
            if JsValue::from_bits(existing.bits()).is_undefined() {
                let mut arr = perry_ffi::js_array_alloc(1);
                arr = perry_ffi::js_array_push(arr, JsValue::from_bits(value.to_bits()));
                js_object_set_field_by_name(
                    dest_ptr,
                    key.as_raw(),
                    f64::from_bits(JsValue::from_object_ptr(arr as *mut _).bits()),
                );
            } else if JsValue::from_bits(existing.bits()).is_pointer() {
                let arr = (existing.bits() & PTR_MASK) as *mut perry_ffi::ArrayHeader;
                let new_arr = perry_ffi::js_array_push(arr, JsValue::from_bits(value.to_bits()));
                if new_arr != arr {
                    js_object_set_field_by_name(
                        dest_ptr,
                        key.as_raw(),
                        f64::from_bits(JsValue::from_object_ptr(new_arr as *mut _).bits()),
                    );
                }
            }
        }
        HeaderFieldKind::Single => {
            if JsValue::from_bits(existing.bits()).is_undefined() {
                js_object_set_field_by_name(dest_ptr, key.as_raw(), value);
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) fn _force_jsvalue_link(v: f64) -> Option<String> {
    jsvalue_to_owned_string(v)
}

#[allow(dead_code)]
pub(crate) fn _force_jsvalue_extract(v: f64) -> bool {
    JsValue::from_bits(v.to_bits()).is_pointer()
}

#[cfg(test)]
mod add_header_line_tests {
    use super::*;

    fn kind(field: &str) -> (u8, String) {
        let (k, name) = match_known_fields(field);
        let tag = match k {
            HeaderFieldKind::Single => 0,
            HeaderFieldKind::List => 1,
            HeaderFieldKind::Cookie => 2,
            HeaderFieldKind::SetCookie => 3,
        };
        (tag, name)
    }

    #[test]
    fn known_single_value_fields_classify_and_lowercase() {
        // First-wins fields from Node's matchKnownFields — including the
        // odd-cased spellings the lowercase retry handles ('Etag' is neither
        // the canonical 'ETag' nor all-lowercase).
        for f in [
            "Content-Type",
            "content-type",
            "Etag",
            "User-Agent",
            "If-Modified-Since",
            "Proxy-Authorization",
            "Max-Forwards",
            "Retry-After",
            "Last-Modified",
            "Host",
            "Age",
            "Expires",
            "Server",
            "Location",
            "Referer",
            "Authorization",
            "If-Unmodified-Since",
        ] {
            let (tag, name) = kind(f);
            assert_eq!(tag, 0, "{f} should be single-valued");
            assert_eq!(name, f.to_lowercase());
        }
    }

    #[test]
    fn list_fields_and_unknown_fields_join() {
        for f in [
            "Date",
            "Connection",
            "Transfer-Encoding",
            "Cache-Control",
            "Form",
            "X-Totally-Custom",
            "",
        ] {
            let (tag, name) = kind(f);
            assert_eq!(tag, 1, "{f} should be ', '-joined");
            assert_eq!(name, f.to_lowercase());
        }
    }

    #[test]
    fn cookie_kinds() {
        assert_eq!(kind("Cookie"), (2, "cookie".to_string()));
        assert_eq!(kind("Set-Cookie"), (3, "set-cookie".to_string()));
        assert_eq!(kind("set-cookie"), (3, "set-cookie".to_string()));
    }

    #[test]
    fn standalone_socket_assignment_aliases() {
        // `new http.IncomingMessage()` then `req.connection = v` must read
        // back through both `socket` and `connection` (#4904).
        let handle = js_node_http_incoming_message_standalone_new(f64::from_bits(
            crate::types::TAG_UNDEFINED,
        ));
        assert!(incoming_socket_override(handle).is_some());
        let marker = 1234.5_f64;
        assert!(incoming_socket_assign(handle, marker));
        assert_eq!(incoming_socket_override(handle), Some(marker));
        perry_ffi::drop_handle(handle);
    }
}
