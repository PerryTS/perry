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
//! - A mutable GC root scanner keeps every closure pointer stored in a
//!   `ClientRequestHandle` or `IncomingMessageHandle` live and rewrites
//!   moved pointers after copied-minor GC so a malloc-triggered sweep
//!   between scheduling and tick can't free them (issue #35 pattern).
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
    alloc_string, gc_register_mutable_root_scanner_named, get_handle_mut, iter_handles_of_mut,
    json_stringify, notify_main_thread, register_handle,
    spawn_blocking_with_reactor as spawn_blocking, with_handle_mut, ArrayHeader, GcRootVisitor,
    Handle, JsClosure, JsString, JsValue, ObjectHeader, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::sync::{Mutex, Once};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        trailers: Vec<(String, String)>,
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
        gc_register_mutable_root_scanner_named("perry-ext-http", scan_http_roots);
    });
}

/// GC root scanner: walks every ClientRequestHandle (response_callback
/// + listeners) and IncomingMessageHandle (listeners). Closures stored
/// as raw i64 pointers are handed to the runtime as mutable slots.
fn scan_http_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<ClientRequestHandle, _>(|req| {
        visitor.visit_i64_slot(&mut req.response_callback);
        for cbs in req.listeners.values_mut() {
            for cb in cbs {
                visitor.visit_i64_slot(cb);
            }
        }
    });

    iter_handles_of_mut::<IncomingMessageHandle, _>(|msg| {
        for cbs in msg.listeners.values_mut() {
            for cb in cbs {
                visitor.visit_i64_slot(cb);
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

fn map_to_js_object(map: &HashMap<String, String>) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);
    let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
    let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
    let count = keys.len() as u32;
    let obj: *mut ObjectHeader = unsafe {
        perry_ffi::js_object_alloc_with_shape(shape_id, count, packed.as_ptr(), packed.len() as u32)
    };
    if !obj.is_null() {
        for (i, key) in keys.iter().enumerate() {
            if let Some(val) = map.get(*key) {
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
    out
}

fn expects_response_trailers(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("te")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("trailers"))
    })
}

async fn dispatch_plain_http_request(
    request_handle: Handle,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    timeout_ms: Option<u64>,
) -> Option<Result<(), String>> {
    if !expects_response_trailers(headers) {
        return None;
    }
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) if u.scheme() == "http" => u,
        _ => return None,
    };
    let host = match parsed.host_str() {
        Some(h) => h.to_string(),
        None => return Some(Err("missing host".to_string())),
    };
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }

    let fut = async {
        let mut stream = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
        let host_header = if parsed.port().is_some() {
            format!("{}:{}", host, port)
        } else {
            host.clone()
        };
        let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", method, path, host_header);
        let mut has_content_length = false;
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("content-length") {
                has_content_length = true;
            }
            if k.eq_ignore_ascii_case("connection") {
                // The raw trailer-aware path reads until EOF after the final
                // chunk/trailer block. Force close here so an explicit
                // `Connection: keep-alive` cannot hang until timeout.
                continue;
            }
            req.push_str(k);
            req.push_str(": ");
            req.push_str(v);
            req.push_str("\r\n");
        }
        req.push_str("Connection: close\r\n");
        if !body.is_empty() && !has_content_length {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).await?;
        if !body.is_empty() {
            stream.write_all(body).await?;
        }

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await?;
        Ok::<Vec<u8>, std::io::Error>(raw)
    };

    let raw = match timeout_ms {
        Some(ms) => match tokio::time::timeout(std::time::Duration::from_millis(ms), fut).await {
            Ok(r) => r,
            Err(_) => return Some(Err("request timed out".to_string())),
        },
        None => match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
            Ok(r) => r,
            Err(_) => return Some(Err("request timed out".to_string())),
        },
    };
    let raw = match raw {
        Ok(r) => r,
        Err(e) => return Some(Err(e.to_string())),
    };

    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Some(Err("invalid HTTP response".to_string()));
    };
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut status_parts = status_line.splitn(3, ' ');
    let _version = status_parts.next();
    let status = status_parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let status_message = status_parts.next().unwrap_or("").to_string();
    let mut hdrs = Vec::new();
    let mut is_chunked = false;
    let mut content_length: Option<usize> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                is_chunked = true;
            }
            if name == "content-length" {
                content_length = value.parse::<usize>().ok();
            }
            hdrs.push((name, value));
        }
    }
    let payload = &raw[header_end + 4..];
    let mut decoded = Vec::new();
    let mut trailers = Vec::new();
    if is_chunked {
        let mut pos = 0;
        while pos < payload.len() {
            let Some(line_end_rel) = payload[pos..].windows(2).position(|w| w == b"\r\n") else {
                break;
            };
            let line_end = pos + line_end_rel;
            let size_line = String::from_utf8_lossy(&payload[pos..line_end]);
            let size_hex = size_line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_hex, 16).unwrap_or(0);
            pos = line_end + 2;
            if size == 0 {
                if pos <= payload.len() {
                    let rest = &payload[pos..];
                    let trailer_end = rest
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .unwrap_or(rest.len());
                    let trailer_text = String::from_utf8_lossy(&rest[..trailer_end]);
                    for line in trailer_text.split("\r\n") {
                        if let Some((name, value)) = line.split_once(':') {
                            trailers
                                .push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
                        }
                    }
                }
                break;
            }
            if pos + size > payload.len() {
                break;
            }
            decoded.extend_from_slice(&payload[pos..pos + size]);
            pos += size + 2;
        }
    } else if let Some(len) = content_length {
        decoded.extend_from_slice(&payload[..payload.len().min(len)]);
    } else {
        decoded.extend_from_slice(payload);
    }

    push_event(PendingHttpEvent::Response {
        request_handle,
        status,
        status_message,
        headers: hdrs,
        trailers,
        body: decoded,
    });
    Some(Ok(()))
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
    pub trailers: HashMap<String, String>,
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
            if let Some(result) = dispatch_plain_http_request(
                request_handle,
                method.as_str(),
                &url,
                &headers,
                &body,
                timeout_ms,
            )
            .await
            {
                if let Err(error_message) = result {
                    push_event(PendingHttpEvent::Error {
                        request_handle,
                        error_message,
                    });
                }
                return;
            }

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
                        trailers: Vec::new(),
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
// http.Agent / https.Agent (#2129)
// ------------------------------------------------------------------
//
// Perry doesn't pool sockets today — every `http.request(...)` opens a
// fresh reqwest connection — so the Agent state below is pure metadata
// that mirrors Node's defaults. The only method whose output the Node
// test suite asserts byte-for-byte is `getName(options)`; the rest are
// chainable no-ops so user code that touches `.destroy()` / `.close()`
// / `.keepSocketAlive()` keeps working.
//
// Mirrors `crates/perry-stdlib/src/http.rs`'s `AgentHandle` so both
// link targets (well-known flip → ext crate; default `full` build →
// perry-stdlib's http-client) expose the same surface.

struct AgentHandle {
    protocol: Option<String>,
    keep_alive: bool,
    keep_alive_msecs: f64,
    max_sockets: f64,
    max_total_sockets: f64,
    max_free_sockets: f64,
    scheduling: String,
    timeout_ms: Option<f64>,
}

impl Default for AgentHandle {
    fn default() -> Self {
        AgentHandle {
            protocol: Some("http:".to_string()),
            keep_alive: false,
            keep_alive_msecs: 1000.0,
            max_sockets: f64::INFINITY,
            max_total_sockets: f64::INFINITY,
            max_free_sockets: 256.0,
            scheduling: "lifo".to_string(),
            timeout_ms: None,
        }
    }
}

unsafe fn agent_new_with_protocol(options_f64: f64, default_protocol: &str) -> Handle {
    let mut agent = AgentHandle {
        protocol: Some(default_protocol.to_string()),
        ..AgentHandle::default()
    };

    if let Some(opts) = parse_options_object(options_f64) {
        if let Some(v) = opts.get("keepAlive").and_then(|v| v.as_bool()) {
            agent.keep_alive = v;
        }
        if let Some(v) = opts.get("keepAliveMsecs").and_then(|v| v.as_f64()) {
            agent.keep_alive_msecs = v;
        }
        if let Some(v) = opts.get("maxSockets").and_then(|v| v.as_f64()) {
            agent.max_sockets = v;
        }
        if let Some(v) = opts.get("maxFreeSockets").and_then(|v| v.as_f64()) {
            agent.max_free_sockets = v;
        }
        if let Some(v) = opts.get("maxTotalSockets").and_then(|v| v.as_f64()) {
            agent.max_total_sockets = v;
        }
        if let Some(v) = opts.get("timeout").and_then(|v| v.as_f64()) {
            agent.timeout_ms = Some(v);
        }
        if let Some(v) = opts.get("scheduling").and_then(|v| v.as_str()) {
            agent.scheduling = v.to_string();
        }
    }

    register_handle(agent)
}

#[no_mangle]
pub unsafe extern "C" fn js_http_agent_new(options_f64: f64) -> Handle {
    agent_new_with_protocol(options_f64, "http:")
}

#[no_mangle]
pub unsafe extern "C" fn js_https_agent_new(options_f64: f64) -> Handle {
    agent_new_with_protocol(options_f64, "https:")
}

/// `agent.getName([options])` — Node's canonical key under which sockets
/// are pooled. Format: `${host}:${port}:${localAddress}` with optional
/// `:${socketPath}` or `:${family}` appended. Tests assert exact strings
/// (see `test/parallel/test-http-agent-getname.js`).
#[no_mangle]
pub unsafe extern "C" fn js_http_agent_get_name(
    handle: Handle,
    options_f64: f64,
) -> *mut StringHeader {
    let _ = handle; // name is computed purely from `options`
    let opts = match parse_options_object(options_f64) {
        Some(v) => v,
        None => return alloc_string("localhost::").as_raw(),
    };

    let host = opts
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or("localhost");
    let port = opts
        .get("port")
        .map(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .or_else(|| v.as_f64().map(|n| (n as i64).to_string()))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let local_address = opts
        .get("localAddress")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut name = format!("{}:{}:{}", host, port, local_address);

    if let Some(socket_path) = opts.get("socketPath").and_then(|v| v.as_str()) {
        name.push(':');
        name.push_str(socket_path);
    } else if let Some(family) = opts.get("family") {
        // Node only appends family when it is exactly 4 or 6; every
        // other value (0, null, undefined, strings) is silently dropped.
        let f = family.as_i64().unwrap_or(0);
        if f == 4 || f == 6 {
            name.push(':');
            name.push_str(&f.to_string());
        }
    }

    alloc_string(&name).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_agent_noop_self(handle: Handle) -> Handle {
    handle
}

fn agent_field<T, F>(handle: Handle, default: T, f: F) -> T
where
    F: FnOnce(&AgentHandle) -> T,
{
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| f(a))
        .unwrap_or(default)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_sockets(handle: Handle) -> f64 {
    agent_field(handle, f64::INFINITY, |a| a.max_sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_free_sockets(handle: Handle) -> f64 {
    agent_field(handle, 256.0, |a| a.max_free_sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_total_sockets(handle: Handle) -> f64 {
    agent_field(handle, f64::INFINITY, |a| a.max_total_sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive_msecs(handle: Handle) -> f64 {
    agent_field(handle, 1000.0, |a| a.keep_alive_msecs)
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive(handle: Handle) -> f64 {
    if agent_field(handle, false, |a| a.keep_alive) {
        1.0
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_protocol(handle: Handle) -> *mut StringHeader {
    let s = get_handle_mut::<AgentHandle>(handle)
        .and_then(|a| a.protocol.clone())
        .unwrap_or_else(|| "http:".to_string());
    alloc_string(&s).as_raw()
}

#[no_mangle]
pub unsafe extern "C" fn js_http_agent_set_protocol(
    handle: Handle,
    value_ptr: *const StringHeader,
) {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        if value_ptr.is_null() {
            agent.protocol = None;
        } else {
            let js = JsString::from_raw(value_ptr as *mut StringHeader);
            if let Some(s) = perry_ffi::read_string(js) {
                agent.protocol = Some(s.to_string());
            }
        }
    }
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
        out = map_to_js_object(&res.headers);
    });
    out
}

/// `res.trailers` — HTTP trailers populated after the body completes.
#[no_mangle]
pub extern "C" fn js_http_response_trailers(handle: Handle) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = map_to_js_object(&res.trailers);
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
                trailers,
                body,
            } => {
                let response_callback = get_handle_mut::<ClientRequestHandle>(request_handle)
                    .map(|r| r.response_callback)
                    .unwrap_or(0);

                let mut headers_map = HashMap::new();
                for (k, v) in headers {
                    headers_map.insert(k, v);
                }
                let mut trailers_map = HashMap::new();
                for (k, v) in trailers {
                    trailers_map.insert(k, v);
                }

                let body_clone = body.clone();
                let incoming = register_handle(IncomingMessageHandle {
                    status_code: status,
                    status_message,
                    headers: headers_map,
                    trailers: trailers_map,
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
    use perry_ffi::{drop_handle, get_handle, register_handle};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn assert_rewritten(before: i64, after: i64) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
    }

    #[test]
    fn gc_scanner_registers_idempotently() {
        // Calling ensure_gc_scanner_registered twice must not panic
        // and must not register the scanner twice (Once guarantees).
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
    }

    #[test]
    fn gc_mutable_scanner_rewrites_request_response_listener_roots() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-http", scan_http_roots);

        let response_callback = young_gc_root();
        let request_listener = young_gc_root();
        let incoming_listener = young_gc_root();
        let mut request_listeners = HashMap::new();
        request_listeners.insert("error".to_string(), vec![request_listener]);
        let request_handle = register_handle(ClientRequestHandle {
            method: "GET".to_string(),
            url: "http://localhost/".to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
            response_callback,
            listeners: request_listeners,
            timeout_ms: None,
            ended: false,
        });

        let mut incoming_listeners = HashMap::new();
        incoming_listeners.insert("data".to_string(), vec![incoming_listener]);
        let incoming_handle = register_handle(IncomingMessageHandle {
            status_code: 200,
            status_message: "OK".to_string(),
            headers: HashMap::new(),
            trailers: HashMap::new(),
            body: Vec::new(),
            listeners: incoming_listeners,
        });

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let req = get_handle::<ClientRequestHandle>(request_handle)
                .expect("request handle should remain live");
            assert_rewritten(response_callback, req.response_callback);
            assert_rewritten(request_listener, req.listeners["error"][0]);
            let msg = get_handle::<IncomingMessageHandle>(incoming_handle)
                .expect("incoming message handle should remain live");
            assert_rewritten(incoming_listener, msg.listeners["data"][0]);
        }
        drop_handle(request_handle);
        drop_handle(incoming_handle);
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

// #1652: force the linker to retain perry-ext-http-server's `#[no_mangle]`
// FFI symbols. The `extern crate perry_ext_http_server as _server_link`
// at the top of this file pulls the rlib into the dependency graph, but
// the server functions are referenced only by codegen-generated callsites
// in the *user* program — never by this crate's Rust. Under LTO / staticlib
// emission they can therefore be dead-stripped, and the final link then
// fails with `Undefined symbols: _js_node_http_create_server` for any
// program that does `import { createServer } from 'node:http'` (the failure
// originally tracked at #589, reopened as #1652). Anchoring their addresses
// in a `#[used]` table makes the retention explicit so it can't silently
// regress when nobody's npm import happens to reference a given symbol.
//
// Resolution is by symbol name (C ABI): the `()` signatures below are only
// ever used to take the function's address, never to call it, so they need
// not match the real definitions — the linker keys off the `#[no_mangle]`
// symbol name alone.
//
// `cfg(not(test))`: the anchor must NOT fire in `cargo test -p perry-ext-http`.
// Forcing the server functions into the unit-test binary drags in their
// transitive `perry_ffi_spawn_blocking*` references, which only the host
// (perry-stdlib) provides at the final perry-compile link — the test binary
// has no host, so it fails with `undefined symbol: perry_ffi_spawn_blocking`.
// The staticlib (the real perry-compile artifact) is built without `test`,
// so retention there is unaffected. Nothing cargo-depends on this crate, so
// gating on `test` is sufficient.
#[cfg(not(test))]
#[allow(dead_code)]
mod force_link_http_server {
    extern "C" {
        // http server + IncomingMessage + ServerResponse entry points.
        pub fn js_node_http_create_server();
        pub fn js_node_http_server_listen();
        pub fn js_node_http_server_listening();
        pub fn js_node_http_server_close();
        pub fn js_node_http_server_on();
        pub fn js_node_http_server_address_json();
        pub fn js_node_http_server_process_pending();
        pub fn js_node_http_server_has_active();
        pub fn js_node_http_server_close_all_connections();
        pub fn js_node_http_server_close_idle_connections();
        pub fn js_node_http_res_end();
        pub fn js_node_http_res_write();
        pub fn js_node_http_res_write_head();
        pub fn js_node_http_res_set_header();
        pub fn js_node_http_res_get_header();
        pub fn js_node_http_res_get_header_names_json();
        pub fn js_node_http_res_get_headers_json();
        pub fn js_node_http_res_has_header();
        pub fn js_node_http_res_remove_header();
        pub fn js_node_http_res_set_status();
        pub fn js_node_http_res_get_status();
        pub fn js_node_http_res_set_status_message();
        pub fn js_node_http_res_headers_sent();
        pub fn js_node_http_res_writable_ended();
        pub fn js_node_http_res_writable_finished();
        pub fn js_node_http_res_flush_headers();
        pub fn js_node_http_res_write_continue();
        pub fn js_node_http_res_write_processing();
        pub fn js_node_http_res_on();
        pub fn js_node_http_im_method();
        pub fn js_node_http_im_url();
        pub fn js_node_http_im_http_version();
        pub fn js_node_http_im_headers_json();
        pub fn js_node_http_im_raw_headers_json();
        pub fn js_node_http_im_remote_address();
        pub fn js_node_http_im_remote_port();
        pub fn js_node_http_im_on();
        pub fn js_node_http_im_read();
        pub fn js_node_http_im_pause();
        pub fn js_node_http_im_resume();
        pub fn js_node_http_im_aborted();
        pub fn js_node_http_im_complete();
        pub fn js_node_http_im_destroy();
        pub fn js_node_http_im_destroyed();
        // https server.
        pub fn js_node_https_create_server();
        pub fn js_node_https_server_listen();
        pub fn js_node_https_server_close();
        pub fn js_node_https_server_on();
        pub fn js_node_https_server_address_json();
        // http2 secure server.
        pub fn js_node_http2_create_secure_server();
        pub fn js_node_http2_server_listen();
        pub fn js_node_http2_server_close();
        pub fn js_node_http2_server_on();
        pub fn js_node_http2_server_address_json();
    }
}

/// `#[used]` anchor table referencing every server FFI entry point so the
/// linker keeps them in `libperry_ext_http.a`. See the module above (#1652).
/// Gated with the module on `not(test)` so the unit-test binary doesn't drag
/// in the server's host-provided `perry_ffi_*` symbols.
#[cfg(not(test))]
#[used]
static FORCE_LINK_HTTP_SERVER: &[unsafe extern "C" fn()] = {
    use force_link_http_server::*;
    &[
        js_node_http_create_server,
        js_node_http_server_listen,
        js_node_http_server_listening,
        js_node_http_server_close,
        js_node_http_server_on,
        js_node_http_server_address_json,
        js_node_http_server_process_pending,
        js_node_http_server_has_active,
        js_node_http_server_close_all_connections,
        js_node_http_server_close_idle_connections,
        js_node_http_res_end,
        js_node_http_res_write,
        js_node_http_res_write_head,
        js_node_http_res_set_header,
        js_node_http_res_get_header,
        js_node_http_res_get_header_names_json,
        js_node_http_res_get_headers_json,
        js_node_http_res_has_header,
        js_node_http_res_remove_header,
        js_node_http_res_set_status,
        js_node_http_res_get_status,
        js_node_http_res_set_status_message,
        js_node_http_res_headers_sent,
        js_node_http_res_writable_ended,
        js_node_http_res_writable_finished,
        js_node_http_res_flush_headers,
        js_node_http_res_write_continue,
        js_node_http_res_write_processing,
        js_node_http_res_on,
        js_node_http_im_method,
        js_node_http_im_url,
        js_node_http_im_http_version,
        js_node_http_im_headers_json,
        js_node_http_im_raw_headers_json,
        js_node_http_im_remote_address,
        js_node_http_im_remote_port,
        js_node_http_im_on,
        js_node_http_im_read,
        js_node_http_im_pause,
        js_node_http_im_resume,
        js_node_http_im_aborted,
        js_node_http_im_complete,
        js_node_http_im_destroy,
        js_node_http_im_destroyed,
        js_node_https_create_server,
        js_node_https_server_listen,
        js_node_https_server_close,
        js_node_https_server_on,
        js_node_https_server_address_json,
        js_node_http2_create_secure_server,
        js_node_http2_server_listen,
        js_node_http2_server_close,
        js_node_http2_server_on,
        js_node_http2_server_address_json,
    ]
};
