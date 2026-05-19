//! Native bindings for Node's `http` / `https` modules.
//!
//! Provides the callback-style ClientRequest / IncomingMessage API
//! that npm packages like twitter-api-v2, rss-parser, web-push use.
//! Both `http` and `https` flow through the same wrapper — reqwest
//! handles TLS based on URL scheme.
//!
//! # Server-side surface (issue #577)
//!
//! `perry-ext-http-server` ships the server-side counterpart —
//! `http.createServer`, `https.createServer`, `http2.createSecureServer`.
//! It's pulled in here as an rlib dep so its `js_node_http_*` /
//! `js_node_https_*` / `js_node_http2_*` symbols flow into
//! `libperry_ext_http.a`. Don't remove the `extern crate` declaration
//! after this docblock — it keeps the linker from dead-stripping the
//! server symbols when no client-side code happens to reference them.
//!
//! # Architecture (mirrors perry-ext-cron + perry-stdlib's http.rs)
//!
//! - `js_http_request(opts, cb)` / `js_http_get(...)` synchronously
//!   register a `ClientRequestHandle` and return its handle id. For
//!   `.get()` the request is auto-`end()`'d, kicking off an async
//!   `spawn_blocking + reqwest` send on a tokio blocking-pool thread.
//! - When the request completes (or errors), the worker thread pushes
//!   a `PendingHttpEvent` onto `HTTP_PENDING_EVENTS` and calls
//!   `perry_ffi::notify_main_thread()` to wake the main loop.
//! - `js_http_process_pending()` runs on the main thread (called from
//!   codegen's event-loop tick); it drains the queue and invokes the
//!   user's `(response) => { ... }` / `error` / `data` / `end`
//!   callbacks via `JsClosure::call0` / `call1`.
//! - A GC root scanner pins every closure pointer stored in a
//!   `ClientRequestHandle` or `IncomingMessageHandle` so a
//!   malloc-triggered sweep between scheduling and tick can't free
//!   them (issue #35 pattern).
//!
//! # Body chunking gap
//!
//! `reqwest::Response::chunk()` is async (`Future`), and we run inside
//! `spawn_blocking` so we can't directly await. We therefore deliver
//! the response body as a single `'data'` event with the entire body
//! buffer (matches perry-stdlib's existing copy). True streaming is
//! a v0.6.0 followup that needs a cooperative `spawn_async` surface
//! on perry-ffi (today's surface is sync-via-blocking-pool only).

#[allow(unused_imports)]
extern crate perry_ext_http_server as _server_link;

use lazy_static::lazy_static;
use perry_ffi::{
    alloc_string, gc_register_root_scanner, get_handle_mut, iter_handles_of, json_stringify,
    notify_main_thread, register_handle, spawn_blocking_with_reactor as spawn_blocking,
    with_handle_mut, ArrayHeader, Handle, JsClosure, JsString, JsValue, ObjectHeader,
    RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::sync::{Mutex, Once};

const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

// ------------------------------------------------------------------
// Pending event queue + GC scanner
// ------------------------------------------------------------------

/// Events queued by the tokio blocking-pool worker for the main thread.
enum PendingHttpEvent {
    Response {
        request_handle: Handle,
        status: u16,
        status_message: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Error {
        request_handle: Handle,
        error_message: String,
    },
}

lazy_static! {
    static ref HTTP_PENDING_EVENTS: Mutex<Vec<PendingHttpEvent>> = Mutex::new(Vec::new());
    /// Shared HTTP client — reuses connection pool, DNS cache, TLS
    /// session cache. Without this each request allocs a fresh
    /// reqwest::Client (~250 KB) and the memory never gets reused.
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent(concat!("perry/", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(16)
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
}

static HTTP_GC_REGISTERED: Once = Once::new();

fn ensure_gc_scanner_registered() {
    HTTP_GC_REGISTERED.call_once(|| {
        gc_register_root_scanner(scan_http_roots);
    });
}

/// GC root scanner: walks every ClientRequestHandle (response_callback
/// + listeners) and IncomingMessageHandle (listeners). Closures stored
/// as raw i64 pointers must be re-NaN-boxed with POINTER_TAG before
/// being handed to the runtime's `mark`.
fn scan_http_roots(mark: &mut dyn FnMut(f64)) {
    let mark_cb = |cb: i64, m: &mut dyn FnMut(f64)| {
        if cb != 0 {
            let boxed = f64::from_bits(POINTER_TAG | (cb as u64 & PTR_MASK));
            m(boxed);
        }
    };

    iter_handles_of::<ClientRequestHandle, _>(|req| {
        mark_cb(req.response_callback, mark);
        for cbs in req.listeners.values() {
            for &cb in cbs {
                mark_cb(cb, mark);
            }
        }
    });

    iter_handles_of::<IncomingMessageHandle, _>(|msg| {
        for cbs in msg.listeners.values() {
            for &cb in cbs {
                mark_cb(cb, mark);
            }
        }
    });
}

fn push_event(ev: PendingHttpEvent) {
    if let Ok(mut q) = HTTP_PENDING_EVENTS.lock() {
        q.push(ev);
    }
    notify_main_thread();
}

// ------------------------------------------------------------------
// Handle types
// ------------------------------------------------------------------

pub struct ClientRequestHandle {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    response_callback: i64,
    /// `'error'` is the only event ClientRequest emits today.
    listeners: HashMap<String, Vec<i64>>,
    timeout_ms: Option<u64>,
    ended: bool,
}

// SAFETY: closure pointers point into program-global code/data and
// stay live for the program's lifetime; the GC scanner pins them.
unsafe impl Send for ClientRequestHandle {}
unsafe impl Sync for ClientRequestHandle {}

pub struct IncomingMessageHandle {
    pub status_code: u16,
    pub status_message: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub listeners: HashMap<String, Vec<i64>>,
}

unsafe impl Send for IncomingMessageHandle {}
unsafe impl Sync for IncomingMessageHandle {}

// ------------------------------------------------------------------
// String / value helpers
// ------------------------------------------------------------------

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let h = JsString::from_raw(ptr as *mut StringHeader);
    perry_ffi::read_string(h).map(String::from)
}

/// Pull a string out of a NaN-boxed JS value, accepting STRING_TAG,
/// POINTER_TAG (some heap strings come in tagged this way) and bare
/// raw pointers (legacy codegen path).
unsafe fn extract_string_value(val_f64: f64) -> Option<String> {
    let bits = val_f64.to_bits();
    let upper = bits >> 48;
    let ptr: *const StringHeader = if upper == 0x7FFF || upper == 0x7FFD {
        (bits & PTR_MASK) as *const StringHeader
    } else if upper == 0 && bits >= 0x10000 {
        bits as *const StringHeader
    } else {
        return None;
    };
    if ptr.is_null() {
        return None;
    }
    read_str(ptr)
}

fn is_string_value(val: f64) -> bool {
    let upper = val.to_bits() >> 48;
    upper == 0x7FFF || upper == 0x7FF9 // STRING_TAG or SHORT_STRING_TAG
}

/// Parse a NaN-boxed JS object via `json_stringify` → `serde_json::Value`.
/// Returns `None` on null pointer or stringify failure.
unsafe fn parse_options_object(val_f64: f64) -> Option<serde_json::Value> {
    let v = JsValue::from_bits(val_f64.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let json = json_stringify(v)?;
    if json.is_empty() || json == "null" || json == "undefined" {
        return None;
    }
    serde_json::from_str(&json).ok()
}

/// Build a URL from a Node http.request options object.
/// Recognized keys: protocol, hostname, host, port, path.
fn url_from_options(opts: &serde_json::Value, default_protocol: &str) -> String {
    let protocol = opts
        .get("protocol")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches(':').to_string())
        .unwrap_or_else(|| default_protocol.to_string());

    let raw_host = opts
        .get("hostname")
        .and_then(|v| v.as_str())
        .or_else(|| opts.get("host").and_then(|v| v.as_str()))
        .unwrap_or("localhost");
    // host may carry "hostname:port" — strip the port suffix.
    let hostname = raw_host.split(':').next().unwrap_or("localhost");

    let port = opts.get("port").and_then(|v| {
        v.as_str()
            .map(String::from)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
            .or_else(|| v.as_u64().map(|n| n.to_string()))
            .or_else(|| v.as_f64().map(|n| (n as u64).to_string()))
    });

    let path = opts
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("/")
        .to_string();

    match port {
        Some(p) if !p.is_empty() => format!("{}://{}:{}{}", protocol, hostname, p, path),
        _ => format!("{}://{}{}", protocol, hostname, path),
    }
}

fn headers_from_options(opts: &serde_json::Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(headers) = opts.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers {
            if let Some(s) = v.as_str() {
                out.insert(k.clone(), s.to_string());
            } else if let Some(n) = v.as_i64() {
                out.insert(k.clone(), n.to_string());
            } else {
                out.insert(k.clone(), v.to_string());
            }
        }
    }
    out
}

fn timeout_from_options(opts: &serde_json::Value) -> Option<u64> {
    opts.get("timeout").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
            .or_else(|| v.as_f64().map(|n| n.max(0.0) as u64))
    })
}

fn method_from_options(opts: &serde_json::Value) -> String {
    opts.get("method")
        .and_then(|v| v.as_str())
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "GET".to_string())
}

// ------------------------------------------------------------------
// Common request building blocks
// ------------------------------------------------------------------

fn make_request_handle(
    method: String,
    url: String,
    headers: HashMap<String, String>,
    timeout_ms: Option<u64>,
    callback: i64,
) -> Handle {
    register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: callback,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
    })
}

/// Spawn the actual reqwest send. The `spawn_blocking_with_reactor`
/// shim runs the closure inside `runtime().spawn(async { ... })`, so
/// we're already in an async context — `Handle::current().block_on`
/// from here would panic with "Cannot start a runtime from within a
/// runtime" (issue #769). Instead, spawn the request future as a
/// fresh detached task on the same multi-thread runtime; it drives
/// itself via `await` chains while we return immediately. Mirrors
/// the `spawn_socket_runner` pattern in `perry-ext-net`.
fn dispatch_request(
    request_handle: Handle,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_ms: Option<u64>,
) {
    spawn_blocking(move || {
        // Defeat LTO dead-stripping of tokio's CONTEXT statics — same
        // workaround perry-ext-net needs (see spawn_socket_runner).
        let try_h = tokio::runtime::Handle::try_current();
        std::hint::black_box(&try_h);
        if try_h.is_err() {
            eprintln!(
                "[perry-ext-http] BUG: dispatch_request Handle::try_current returned Err — \
                 LTO has likely dead-stripped tokio's CONTEXT statics."
            );
            return;
        }
        let handle = tokio::runtime::Handle::current();
        let jh = handle.spawn(async move {
            let mut req = match method.as_str() {
                "POST" => HTTP_CLIENT.post(&url),
                "PUT" => HTTP_CLIENT.put(&url),
                "DELETE" => HTTP_CLIENT.delete(&url),
                "PATCH" => HTTP_CLIENT.patch(&url),
                "HEAD" => HTTP_CLIENT.head(&url),
                "OPTIONS" => HTTP_CLIENT.request(reqwest::Method::OPTIONS, &url),
                _ => HTTP_CLIENT.get(&url),
            };
            for (k, v) in &headers {
                req = req.header(k.as_str(), v.as_str());
            }
            if let Some(ms) = timeout_ms {
                req = req.timeout(std::time::Duration::from_millis(ms));
            } else {
                req = req.timeout(std::time::Duration::from_secs(30));
            }
            if !body.is_empty() {
                req = req.body(body);
            }
            match req.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let status_message = response
                        .status()
                        .canonical_reason()
                        .unwrap_or("")
                        .to_string();
                    let mut hdrs = Vec::new();
                    for (k, v) in response.headers() {
                        if let Ok(s) = v.to_str() {
                            hdrs.push((k.to_string(), s.to_string()));
                        }
                    }
                    let body = response
                        .bytes()
                        .await
                        .map(|b| b.to_vec())
                        .unwrap_or_default();
                    push_event(PendingHttpEvent::Response {
                        request_handle,
                        status,
                        status_message,
                        headers: hdrs,
                        body,
                    });
                }
                Err(e) => {
                    push_event(PendingHttpEvent::Error {
                        request_handle,
                        error_message: e.to_string(),
                    });
                }
            }
        });
        std::hint::black_box(&jh);
        std::mem::forget(jh);
    });
}

// ------------------------------------------------------------------
// FFI: http.request / https.request / http.get / https.get
// ------------------------------------------------------------------

unsafe fn request_common(arg_f64: f64, callback: i64, default_protocol: &str) -> Handle {
    ensure_gc_scanner_registered();
    // Issue #769 — accept either a URL string or an options object. Mirrors
    // the dispatch in `get_common` so `http.request("http://…", cb)` works
    // the same as `http.request({ host, port, path }, cb)`.
    let (method, url, headers, timeout) = if is_string_value(arg_f64) {
        let raw = extract_string_value(arg_f64).unwrap_or_default();
        let url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw
        } else if !raw.is_empty() {
            format!("{}://{}", default_protocol, raw)
        } else {
            String::new()
        };
        ("GET".to_string(), url, HashMap::new(), None)
    } else {
        let opts = parse_options_object(arg_f64).unwrap_or(serde_json::Value::Null);
        let method = method_from_options(&opts);
        let url = url_from_options(&opts, default_protocol);
        let headers = headers_from_options(&opts);
        let timeout = timeout_from_options(&opts);
        (method, url, headers, timeout)
    };
    make_request_handle(method, url, headers, timeout, callback)
}

#[no_mangle]
pub unsafe extern "C" fn js_http_request(opts_f64: f64, callback_i64: i64) -> Handle {
    request_common(opts_f64, callback_i64, "http")
}

#[no_mangle]
pub unsafe extern "C" fn js_https_request(opts_f64: f64, callback_i64: i64) -> Handle {
    request_common(opts_f64, callback_i64, "https")
}

unsafe fn get_common(arg_f64: f64, callback: i64, default_protocol: &str) -> Handle {
    ensure_gc_scanner_registered();
    let (url, headers, timeout) = if is_string_value(arg_f64) {
        let raw = extract_string_value(arg_f64).unwrap_or_default();
        let url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw
        } else if !raw.is_empty() {
            format!("{}://{}", default_protocol, raw)
        } else {
            String::new()
        };
        (url, HashMap::new(), None)
    } else {
        let opts = parse_options_object(arg_f64).unwrap_or(serde_json::Value::Null);
        let url = url_from_options(&opts, default_protocol);
        let headers = headers_from_options(&opts);
        let timeout = timeout_from_options(&opts);
        (url, headers, timeout)
    };

    let handle = make_request_handle("GET".to_string(), url, headers, timeout, callback);
    // GET auto-`end()`s, kicking off the request.
    js_http_client_request_end(handle, f64::from_bits(TAG_UNDEFINED));
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_http_get(arg_f64: f64, callback_i64: i64) -> Handle {
    get_common(arg_f64, callback_i64, "http")
}

#[no_mangle]
pub unsafe extern "C" fn js_https_get(arg_f64: f64, callback_i64: i64) -> Handle {
    get_common(arg_f64, callback_i64, "https")
}

// ------------------------------------------------------------------
// FFI: ClientRequest accessors
// ------------------------------------------------------------------

/// `req.write(chunk)` — append data to the request body.
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_write(handle: Handle, body_f64: f64) -> Handle {
    if let Some(body) = extract_string_value(body_f64) {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            req.body.extend_from_slice(body.as_bytes());
        });
    }
    handle
}

/// `req.end(body?)` — finalize and dispatch the request. Optional
/// trailing body chunk is appended before sending. Idempotent: a
/// second call after `ended=true` is a no-op.
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_end(handle: Handle, body_f64: f64) -> Handle {
    if let Some(body) = extract_string_value(body_f64) {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            req.body.extend_from_slice(body.as_bytes());
        });
    }

    let snapshot = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if req.ended {
            return None;
        }
        req.ended = true;
        Some((
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.timeout_ms,
        ))
    });

    let snapshot = match snapshot.flatten() {
        Some(s) => s,
        None => return handle,
    };

    let (method, url, headers, body, timeout_ms) = snapshot;
    dispatch_request(handle, method, url, headers, body, timeout_ms);
    handle
}

/// `req.on(event, cb)` / `res.on(event, cb)` — register an event
/// listener. Works on both ClientRequest and IncomingMessage handles
/// (we try ClientRequest first, then IncomingMessage).
#[no_mangle]
pub unsafe extern "C" fn js_http_on(
    handle: Handle,
    event_ptr: *const StringHeader,
    callback: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let event = match read_str(event_ptr) {
        Some(e) => e,
        None => return handle,
    };
    if callback == 0 {
        return handle;
    }

    let mut matched = false;
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        req.listeners
            .entry(event.clone())
            .or_default()
            .push(callback);
        matched = true;
    });
    if matched {
        return handle;
    }
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.listeners.entry(event).or_default().push(callback);
    });
    handle
}

/// `req.setHeader(name, value)`.
#[no_mangle]
pub unsafe extern "C" fn js_http_set_header(
    handle: Handle,
    name_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> Handle {
    let name = match read_str(name_ptr) {
        Some(n) => n,
        None => return handle,
    };
    let value = match read_str(value_ptr) {
        Some(v) => v,
        None => return handle,
    };
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        req.headers.insert(name, value);
    });
    handle
}

/// `req.setTimeout(ms)`.
#[no_mangle]
pub unsafe extern "C" fn js_http_set_timeout(handle: Handle, ms: f64) -> Handle {
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        req.timeout_ms = Some(ms.max(0.0) as u64);
    });
    handle
}

// ------------------------------------------------------------------
// FFI: IncomingMessage accessors
// ------------------------------------------------------------------

/// `1` if `handle` is registered as an `IncomingMessageHandle`,
/// `0` otherwise. Used by perry-stdlib's `js_handle_property_dispatch`
/// to gate the `res.statusCode` / `res.headers` arms — keeps the
/// property-name match from accidentally returning IncomingMessage
/// fields for an unrelated handle whose id collides.
#[no_mangle]
pub extern "C" fn js_http_is_incoming_message(handle: Handle) -> i32 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |_| ())
        .map(|_| 1)
        .unwrap_or(0)
}

/// `res.statusCode`.
#[no_mangle]
pub extern "C" fn js_http_status_code(handle: Handle) -> f64 {
    let mut out = 0.0;
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = res.status_code as f64;
    });
    out
}

/// `res.statusMessage`.
#[no_mangle]
pub extern "C" fn js_http_status_message(handle: Handle) -> *mut StringHeader {
    let mut out: *mut StringHeader = std::ptr::null_mut();
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = alloc_string(&res.status_message).as_raw();
    });
    if out.is_null() {
        alloc_string("").as_raw()
    } else {
        out
    }
}

/// `res.headers` — returns a NaN-boxed object (bits returned as f64).
/// The receiving codegen-side `f64`-typed slot stores the bits, so
/// the user's TS code sees an Object as expected.
#[no_mangle]
pub extern "C" fn js_http_response_headers(handle: Handle) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        // Build a shape-keyed object whose keys are the response
        // header names, values are the header values.
        let keys: Vec<&str> = res.headers.keys().map(|s| s.as_str()).collect();
        let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
        let count = keys.len() as u32;
        // SAFETY: `packed` is owned for the duration of the call.
        let obj: *mut ObjectHeader = unsafe {
            perry_ffi::js_object_alloc_with_shape(
                shape_id,
                count,
                packed.as_ptr(),
                packed.len() as u32,
            )
        };
        if !obj.is_null() {
            for (i, key) in keys.iter().enumerate() {
                if let Some(val) = res.headers.get(*key) {
                    let s = alloc_string(val);
                    let v = JsValue::from_string_ptr(s.as_raw());
                    unsafe {
                        perry_ffi::js_object_set_field(obj, i as u32, v);
                    }
                }
            }
            let v = JsValue::from_object_ptr(obj as *mut u8);
            out = f64::from_bits(v.bits());
        }
    });
    out
}

// ------------------------------------------------------------------
// Event-loop pump
// ------------------------------------------------------------------

/// Number of pending events the main loop should drain.
#[no_mangle]
pub extern "C" fn js_http_has_pending() -> i32 {
    HTTP_PENDING_EVENTS
        .lock()
        .map(|q| if q.is_empty() { 0 } else { 1 })
        .unwrap_or(0)
}

/// Drain the pending HTTP-event queue and fire user callbacks. Called
/// from codegen's event-loop tick. Returns count of events drained.
#[no_mangle]
pub unsafe extern "C" fn js_http_process_pending() -> i32 {
    let events: Vec<PendingHttpEvent> = match HTTP_PENDING_EVENTS.lock() {
        Ok(mut q) => q.drain(..).collect(),
        Err(_) => return 0,
    };

    let count = events.len() as i32;

    for ev in events {
        match ev {
            PendingHttpEvent::Response {
                request_handle,
                status,
                status_message,
                headers,
                body,
            } => {
                let response_callback = get_handle_mut::<ClientRequestHandle>(request_handle)
                    .map(|r| r.response_callback)
                    .unwrap_or(0);

                let mut headers_map = HashMap::new();
                for (k, v) in headers {
                    headers_map.insert(k, v);
                }

                let body_clone = body.clone();
                let incoming = register_handle(IncomingMessageHandle {
                    status_code: status,
                    status_message,
                    headers: headers_map,
                    body,
                    listeners: HashMap::new(),
                });

                if response_callback != 0 {
                    // Hand the IncomingMessage handle to the user's
                    // `(res) => { ... }` callback. POINTER_TAG so the
                    // closure-arg unboxer extracts the i64.
                    let arg = f64::from_bits(POINTER_TAG | (incoming as u64 & PTR_MASK));
                    let closure = JsClosure::from_raw(response_callback as *const RawClosureHeader);
                    let _ = closure.call1(arg);
                }

                // `'data'` listeners — body is delivered as a single
                // Buffer chunk. True streaming requires a cooperative
                // spawn_async perry-ffi surface (v0.6.0 followup).
                //
                // Issue #1124 followup: pre-fix this allocated a JS
                // string via `alloc_string(str::from_utf8(&body).unwrap_or(""))`,
                // which silently collapsed any non-UTF-8 byte sequence
                // (PNG file-magic, gzip frames, binary protocols, …) to
                // the empty string before user code ever saw a byte.
                // The mirror of the #1124 server-side fix (where the
                // request body went the OTHER direction through a
                // wrongly-shaped StringHeader): allocate a JS Buffer
                // via `alloc_buffer(&bytes)` so the bytes survive the
                // FFI boundary intact. The Buffer registers itself
                // through perry-runtime's `is_registered_buffer` path
                // so the `chunk.toString(enc)` / `chunk.length` /
                // `Buffer.concat(...)` surface lights up on the
                // returned value.
                //
                // TODO: encoding-aware data events — Node lets users
                // call `res.setEncoding('utf8')` to get string chunks
                // instead of Buffers. Perry-ext-http doesn't yet
                // track a per-response encoding flag; default to
                // Buffer (matches Node behavior when no encoding is
                // set) and revisit when a caller demands the string
                // form.
                let data_listeners = get_handle_mut::<IncomingMessageHandle>(incoming)
                    .and_then(|r| r.listeners.get("data").cloned())
                    .unwrap_or_default();
                if !data_listeners.is_empty() && !body_clone.is_empty() {
                    let buf = perry_ffi::alloc_buffer(&body_clone);
                    if !buf.is_null() {
                        let arg = f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK));
                        for cb in data_listeners {
                            if cb != 0 {
                                let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
                                let _ = closure.call1(arg);
                            }
                        }
                    }
                }

                // `'end'` listeners — fire after data.
                let end_listeners = get_handle_mut::<IncomingMessageHandle>(incoming)
                    .and_then(|r| r.listeners.get("end").cloned())
                    .unwrap_or_default();
                for cb in end_listeners {
                    if cb != 0 {
                        let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
                        let _ = closure.call0();
                    }
                }
            }
            PendingHttpEvent::Error {
                request_handle,
                error_message,
            } => {
                let error_listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
                    .and_then(|r| r.listeners.get("error").cloned())
                    .unwrap_or_default();
                if !error_listeners.is_empty() {
                    let s = alloc_string(&error_message);
                    let arg = f64::from_bits(STRING_TAG | (s.as_raw() as u64 & PTR_MASK));
                    for cb in error_listeners {
                        if cb != 0 {
                            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
                            let _ = closure.call1(arg);
                        }
                    }
                }
            }
        }
    }

    count
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_scanner_registers_idempotently() {
        // Calling ensure_gc_scanner_registered twice must not panic
        // and must not register the scanner twice (Once guarantees).
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
    }

    #[test]
    fn has_pending_zero_when_idle() {
        // Drain anything other tests left; then assert zero.
        let _ = HTTP_PENDING_EVENTS.lock().map(|mut q| q.clear());
        assert_eq!(js_http_has_pending(), 0);
    }

    #[test]
    fn parse_options_safe_defaults() {
        // Null pointer / undefined value → safe defaults from
        // url_from_options + headers_from_options + timeout_from_options.
        let null_val = f64::from_bits(TAG_UNDEFINED);
        let parsed = unsafe { parse_options_object(null_val) };
        assert!(parsed.is_none());

        let synth = serde_json::Value::Null;
        assert_eq!(url_from_options(&synth, "http"), "http://localhost/");
        assert!(headers_from_options(&synth).is_empty());
        assert!(timeout_from_options(&synth).is_none());
        assert_eq!(method_from_options(&synth), "GET");
    }

    #[test]
    fn url_from_options_with_port_and_path() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"hostname":"api.example.com","port":8080,"path":"/v1/resource"}"#,
        )
        .unwrap();
        assert_eq!(
            url_from_options(&v, "https"),
            "https://api.example.com:8080/v1/resource"
        );
    }

    #[test]
    fn headers_from_options_extracts() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"headers":{"X-Foo":"bar","Authorization":"Bearer x"}}"#)
                .unwrap();
        let h = headers_from_options(&v);
        assert_eq!(h.get("X-Foo"), Some(&"bar".to_string()));
        assert_eq!(h.get("Authorization"), Some(&"Bearer x".to_string()));
    }
}

// Suppress unused-import warnings for FFI-only types.
#[allow(dead_code)]
fn _force_link() -> Option<*mut ArrayHeader> {
    None
}
