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

mod agent;
pub use agent::*;

// Client factory overload normalization (#3226 / #3227 / #3228) —
// extracted from this file to stay under the 2000-line lint cap.
mod client_overload;
use client_overload::{merge_url_and_options, method_for_overload, parse_client_args};

mod client_request_surface;

// Client-side TLS options (rejectUnauthorized / ca / checkServerIdentity)
// for `https.request` / `https.get` (#4906) — kept out of this file to
// stay under the 2000-line lint cap.
mod tls_client;

// Raw-socket trailer-aware HTTP/1.1 client (`TE: trailers` bypass) +
// response parser, extracted to keep `lib.rs` under the 2000-line lint cap.
mod plain_client;
use plain_client::{dispatch_plain_http_request, parse_http_response};

// Raw-socket `Expect: 100-continue` client path (#5080) — flushes the head,
// observes the interim `100 Continue`, emits `'continue'`, then sends the
// withheld body. reqwest swallows the interim response, so this bypass is
// needed to surface it.
mod continue_client;

// Async reqwest dispatch (`dispatch_request` + TLS-client selection),
// extracted to keep `lib.rs` under the 2000-line lint cap.
mod client_dispatch;
use client_dispatch::dispatch_request;

// Client-request event drain helpers (#4905) — extracted from this file
// to stay under the 2000-line lint cap.
mod client_events;

// Client OutgoingMessage write/end callback + backpressure + setTimeout
// surface (#4909) — extracted to stay under the 2000-line lint cap.
mod client_outgoing;

// Node-compatible argument/header/URL validation for the client factories
// (#4907) — throws `ERR_*`-coded errors on bad input.
mod validation;
use validation::{validate_client_options, validate_client_url_string};

// Classifies transport-layer client failures (connect refused, DNS lookup
// failure, …) into the Node `Error` shape (`.code`/`.syscall`/`.errno`).
mod transport_error;

mod response_headers;
use response_headers::build_response_headers_object;

use bytes::Bytes;
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
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

// ------------------------------------------------------------------
// Pending event queue + GC scanner
// ------------------------------------------------------------------

/// Events queued by the tokio blocking-pool worker for the main thread.
pub(crate) enum PendingHttpEvent {
    Response {
        request_handle: Handle,
        status: u16,
        status_message: String,
        headers: Vec<(String, String)>,
        trailers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Streaming delivery (reqwest path): the response head arrived — fire
    /// the `http.request` callback / `'response'` listeners now; body
    /// chunks follow as [`PendingHttpEvent::ResponseChunk`]s. This is what
    /// lets client code observe headers (and start timers / destroy the
    /// request) while the server is still writing.
    ResponseHead {
        request_handle: Handle,
        status: u16,
        status_message: String,
        headers: Vec<(String, String)>,
    },
    /// One streamed body chunk following a `ResponseHead`. Carried as a
    /// refcounted `Bytes` (reqwest hands `chunk()` out this way) so the
    /// streaming path stays zero-copy from the receive buffer to the drain
    /// handler, which only ever borrows it as `&[u8]`.
    ResponseChunk {
        request_handle: Handle,
        chunk: Bytes,
    },
    /// The streamed body finished — `'end'` on the message, `'close'` on
    /// the request.
    ResponseEnd { request_handle: Handle },
    Error {
        request_handle: Handle,
        error_message: String,
    },
    /// A classified transport failure (connect refused, DNS lookup failure,
    /// connection reset, …). Unlike [`PendingHttpEvent::Error`] — which hands
    /// listeners a bare string — this carries the Node error shape so the
    /// drain builds a real coded `Error` with `.code`/`.syscall`/`.errno`,
    /// matching what Node passes to `request.on('error')`.
    TransportError {
        request_handle: Handle,
        message: String,
        code: String,
        syscall: String,
        errno: i64,
    },
    /// #4905 — the transport deadline from `req.setTimeout(ms)` /
    /// `options.timeout` fired. Drains to the request's `'timeout'`
    /// listeners when any exist; falls back to the Error surface
    /// otherwise.
    Timeout { request_handle: Handle },
    /// #4909 — the request body was handed to the transport at `end()`.
    /// Drains the queued `write(chunk, cb)` callbacks, then `'finish'`,
    /// then the `end(..., cb)` callback — Node's flush ordering.
    Flushed { request_handle: Handle },
    /// #5080 — the server answered an `Expect: 100-continue` request with
    /// an interim `100 Continue`. Drains to the request's `'continue'`
    /// listeners; the canonical handler then sends the withheld body.
    Continue { request_handle: Handle },
    /// #5080 — arm the `Expect: 100-continue` head flush on the next event-loop
    /// tick (Node's nextTick), so post-construction `setHeader(...)` — including
    /// a late `Expect` — reaches the wire. No-op for non-continue/sent requests.
    DeferredArmContinue { request_handle: Handle },
}

/// #5779 follow-up — count of in-flight HTTP/HTTPS CLIENT requests (the detached
/// reqwest task spawned per `http.request`/`http.get`, from dispatch until the
/// response fully streams or errors).
///
/// `EXT_BLOCKING_TASKS_INFLIGHT` (perry-stdlib's idle-kick / active-handle gate)
/// only stays up for the SHORT outer `spawn_blocking` closure that *launches* the
/// reqwest task and returns; it drops to 0 while the actual fetch is still in
/// flight. So the runtime's idle-kick never fires during a fetch, and a lost
/// tokio worker-unpark for the reqwest task (the canonical failure under a
/// `Promise.all` burst of fetches) is never recovered — the main thread parks
/// forever with the responses received but undelivered. This counter, exposed via
/// [`js_ext_http_client_inflight`], lets the idle-kick + active-handle gate honor
/// a fetch's TRUE lifetime so a stranded reqwest task gets roused.
static CLIENT_REQUESTS_INFLIGHT: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// RAII in-flight marker. Created right before the reqwest task is spawned and
/// MOVED INTO the task, so the count tracks the task's full lifetime — including
/// a task scheduled-but-stranded by a lost worker-unpark (its future, holding the
/// guard, is never dropped while stranded). Drop wakes the main loop so its
/// active-handle gate re-evaluates promptly.
pub(crate) struct ClientInflightGuard;
impl ClientInflightGuard {
    pub(crate) fn new() -> Self {
        CLIENT_REQUESTS_INFLIGHT.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        ClientInflightGuard
    }
}
impl Drop for ClientInflightGuard {
    fn drop(&mut self) {
        CLIENT_REQUESTS_INFLIGHT.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        notify_main_thread();
    }
}

/// Exposed for perry-stdlib's idle-kick + active-handle gate (#5779 follow-up):
/// returns nonzero while any HTTP client fetch is outstanding.
#[no_mangle]
pub extern "C" fn js_ext_http_client_inflight() -> i32 {
    CLIENT_REQUESTS_INFLIGHT
        .load(std::sync::atomic::Ordering::Acquire)
        .clamp(0, i32::MAX as i64) as i32
}

/// Pure predicate behind [`node_env_proxy_enabled`]. Node treats its
/// `NODE_*` boolean env vars as enabled only when the value is exactly `"1"`.
/// Split out so the policy is unit-testable without mutating process env.
fn proxy_enabled_from_env_value(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Whether Node's `--use-env-proxy` / `NODE_USE_ENV_PROXY=1` is active.
///
/// Node's built-in `fetch` and `node:http`/`node:https` ignore the standard
/// `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` env vars unless this is set. perry
/// mirrors that opt-in so its bindings are Node-conformant — reqwest would
/// otherwise honor the proxy env unconditionally, diverging from Node.
fn node_env_proxy_enabled() -> bool {
    proxy_enabled_from_env_value(std::env::var("NODE_USE_ENV_PROXY").ok().as_deref())
}

/// Apply the Node-conformant proxy policy to a reqwest client builder: honor
/// the standard proxy env vars only when `NODE_USE_ENV_PROXY=1`, matching Node.
pub(crate) fn apply_node_proxy_policy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    if node_env_proxy_enabled() {
        builder
    } else {
        builder.no_proxy()
    }
}

/// A default reqwest client with the Node-conformant proxy policy applied.
/// Used as the fallback when a customized builder fails to build.
pub(crate) fn default_client() -> reqwest::Client {
    apply_node_proxy_policy(reqwest::Client::builder())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(test)]
mod proxy_policy_tests {
    use super::proxy_enabled_from_env_value;

    #[test]
    fn env_proxy_enabled_only_for_exactly_one() {
        // Matches Node's NODE_USE_ENV_PROXY parsing: enabled iff "1".
        assert!(proxy_enabled_from_env_value(Some("1")));
        assert!(!proxy_enabled_from_env_value(None));
        assert!(!proxy_enabled_from_env_value(Some("0")));
        assert!(!proxy_enabled_from_env_value(Some("")));
        assert!(!proxy_enabled_from_env_value(Some("true")));
        assert!(!proxy_enabled_from_env_value(Some("2")));
    }
}

lazy_static! {
    static ref HTTP_PENDING_EVENTS: Mutex<Vec<PendingHttpEvent>> = Mutex::new(Vec::new());
    /// Shared HTTP client — reuses connection pool, DNS cache, TLS
    /// session cache. Without this each request allocs a fresh
    /// reqwest::Client (~250 KB) and the memory never gets reused.
    pub(crate) static ref HTTP_CLIENT: reqwest::Client = apply_node_proxy_policy(
        reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(16)
            .tcp_keepalive(std::time::Duration::from_secs(60)),
    )
    .build()
    .unwrap_or_else(|_| default_client());
}

static HTTP_GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_gc_scanner_registered() {
    HTTP_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-http", scan_http_roots);
        // #2532 — register the client response/error pump with perry-runtime
        // directly so `http.request` / `http.get` callbacks fire in an
        // out-of-tree install (prebuilt full stdlib has the
        // `external-http-client-pump` arm compiled out). No separate
        // has-active is needed: the in-flight request is a perry-ffi async
        // op, which `js_native_async_has_active` already keeps the loop alive
        // for. Idempotent on the runtime side.
        extern "C" {
            fn js_register_aux_pump(f: extern "C" fn() -> i32);
        }
        // `js_http_process_pending` is an `unsafe extern "C" fn`; the
        // registry takes a safe `extern "C" fn`, so route through a thin
        // safe shim.
        extern "C" fn client_pump() -> i32 {
            unsafe { js_http_process_pending() }
        }
        unsafe {
            js_register_aux_pump(client_pump);
        }
    });
}

/// GC root scanner: walks every ClientRequestHandle (response_callback
/// + listeners), IncomingMessageHandle (listeners), and AgentHandle
/// (createConnection / createSocket overrides). Closures stored as raw
/// i64 pointers are handed to the runtime as mutable slots.
fn scan_http_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<ClientRequestHandle, _>(|req| {
        visitor.visit_i64_slot(&mut req.response_callback);
        visitor.visit_i64_slot(&mut req.end_callback);
        for cb in &mut req.pending_write_callbacks {
            visitor.visit_i64_slot(cb);
        }
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
        // `.pipe(dest)` destinations are live JS values held until the body
        // streams through them; relocate them if the copying GC moves them.
        for dest in &mut msg.pipes {
            visitor.visit_nanbox_u64_slot(dest);
        }
    });

    // #2154: stored `agent.createConnection` / `.createSocket` closures.
    agent::scan_agent_roots(visitor);
    client_request_surface::scan_roots(visitor);
}

pub(crate) fn push_event(ev: PendingHttpEvent) {
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

// ------------------------------------------------------------------
// Handle types
// ------------------------------------------------------------------

pub struct ClientRequestHandle {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    response_callback: i64,
    /// `.on(event, cb)` listeners (`'response'` / `'error'` / `'timeout'`
    /// / `'finish'` / `'close'`).
    listeners: HashMap<String, Vec<i64>>,
    timeout_ms: Option<u64>,
    ended: bool,
    /// `flushHeaders()` dispatched the exchange before `end()` was called;
    /// the eventual `end()` still owes the write/finish/end callback
    /// ordering exactly once.
    flushed_early: bool,
    /// #4909 — `write(chunk, cb)` callbacks queued until the body is
    /// flushed at `end()` (Node fires them once the chunk hits the
    /// transport; our buffered MVP flushes everything at `end()`).
    pending_write_callbacks: Vec<i64>,
    /// #4909 — the `end(..., cb)` callback; fires after the queued write
    /// callbacks and the `'finish'` listeners.
    end_callback: i64,
    /// #4909 — set once the response/error was delivered (or the request
    /// destroyed); suppresses late `'timeout'` timers and stale events.
    completed: bool,
    /// #4909 — `'timeout'` fires at most once per request, no matter how
    /// many timers (`options.timeout` + `setTimeout()` reschedules) land.
    timeout_fired: bool,
    /// #4909 — `'close'` fires at most once per request.
    close_emitted: bool,
    /// `options.agent` handle id when the caller supplied an Agent
    /// (#2154). `0` = use the global `HTTP_CLIENT` (no pooling
    /// distinction). When set, `dispatch_request` calls
    /// `agent::client_for_agent` so requests share a per-Agent
    /// connection pool whose `keepAlive` / `maxFreeSockets` /
    /// `keepAliveMsecs` come from the Agent's stored options.
    agent_handle: Handle,
    /// Client-side TLS options (#4906): `rejectUnauthorized` / `ca` /
    /// `checkServerIdentity`. Default = no customization (pooled client).
    tls: tls_client::TlsOptions,
    /// The IncomingMessage handle created when a streamed `ResponseHead`
    /// arrived; later `ResponseChunk` / `ResponseEnd` events route to it.
    /// `0` until the head is delivered (and always for the full-buffer
    /// delivery paths).
    incoming_handle: Handle,
    /// #5080 — the request carries `Expect: 100-continue`, so its head was
    /// flushed up front by the raw-socket continue path and the body is
    /// withheld until the server's interim `100 Continue` arrives. `end()`
    /// hands the (now-known) body over the `continue_body_tx` channel
    /// instead of dispatching a fresh exchange.
    expects_continue: bool,
    /// #5080 — set while the continue exchange task is waiting for the
    /// deferred body; `end()` sends the buffered body here (once).
    continue_body_tx: Option<tokio::sync::oneshot::Sender<Vec<u8>>>,
}

// SAFETY: closure pointers point into program-global code/data and
// stay live for the program's lifetime; the GC scanner pins them.
unsafe impl Send for ClientRequestHandle {}
unsafe impl Sync for ClientRequestHandle {}

pub struct IncomingMessageHandle {
    pub status_code: u16,
    pub status_message: String,
    /// Raw `(name, value)` header pairs in arrival order, multiplicity
    /// preserved. The combined `res.headers` view (Node's
    /// `matchKnownFields` rules: `set-cookie` → array, single-value
    /// fields keep-first, everything else joined with `, `) is built
    /// lazily in [`build_response_headers_object`] (#5079).
    pub headers: Vec<(String, String)>,
    pub trailers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub listeners: HashMap<String, Vec<i64>>,
    pub encoding: Option<String>,
    /// `.pipe(dest)` destinations as NaN-boxed value bits. Node's
    /// `readable.pipe(writable)` forwards every body chunk to `dest.write()`
    /// and ends it with `dest.end()`; node-fetch reads the response body this
    /// way (`res.pipe(new PassThrough())`), so without it the destination
    /// stream never receives data and `response.text()` never settles.
    pub pipes: Vec<u64>,
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
pub(crate) unsafe fn parse_options_object(val_f64: f64) -> Option<serde_json::Value> {
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
    // Node defaults any falsy `options.method` (absent, `''`, `null`,
    // `undefined`) to `'GET'` — only a truthy string is used (#4970).
    opts.get("method")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
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
    agent_handle: Handle,
) -> Handle {
    let handle = register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: callback,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
        flushed_early: false,
        pending_write_callbacks: Vec::new(),
        end_callback: 0,
        completed: false,
        timeout_fired: false,
        close_emitted: false,
        agent_handle,
        tls: tls_client::TlsOptions::default(),
        incoming_handle: 0,
        expects_continue: false,
        continue_body_tx: None,
    });
    // #4909 — `options.timeout` arms the inactivity timer as soon as the
    // socket exists in Node, not at `end()`; a request that is never
    // dispatched (or whose server never answers) still gets `'timeout'`.
    if let Some(ms) = timeout_ms {
        if ms > 0 {
            client_outgoing::arm_client_timeout(handle, ms);
        }
    }
    handle
}

/// Parse the client-side TLS options (#4906) off a request options value
/// and store them on the freshly-built request handle. A no-op for
/// string-URL requests / plain http (parse yields the default).
unsafe fn attach_tls_options(handle: Handle, opts_f64: f64) {
    let tls = tls_client::parse_tls_options(opts_f64);
    if tls.needs_custom_client() {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| req.tls = tls);
    }
}

/// Serialize an HTTP/1.1 request (request line + headers + body) into the
/// bytes to write onto a socket. Forces `Connection: close` (the raw socket
/// path reads until EOF), drops any caller-supplied `Connection`/`Host`
/// header (we set `Host` from the URL), and adds `Content-Length` when a
/// body is present and the caller didn't.
fn serialize_http_request(
    method: &str,
    path: &str,
    host_header: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Vec<u8> {
    let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", method, path, host_header);
    let mut has_content_length = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        if k.eq_ignore_ascii_case("connection") || k.eq_ignore_ascii_case("host") {
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
    let mut out = req.into_bytes();
    out.extend_from_slice(body);
    out
}

/// #2154 — run an HTTP exchange over a socket that the agent's
/// `createConnection` override produced (`socket_id`), instead of through
/// reqwest. Writes the serialized request, reads the response until the peer
/// closes (we force `Connection: close`), parses it with
/// [`parse_http_response`], and pushes the same `Response` / `Error` event
/// the reqwest path produces — so the IncomingMessage surface is identical.
///
/// The socket I/O goes through perry-ffi's raw-net vtable (published by
/// perry-ext-net), so this crate needs no link edge to perry-ext-net. If no
/// net backend is linked the request errors out (the override couldn't have
/// produced a socket without `net`, so this is a defensive guard).
fn dispatch_request_over_socket(
    request_handle: Handle,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_ms: Option<u64>,
    socket_id: i64,
) {
    let parsed = match reqwest::Url::parse(&url) {
        Ok(u) => u,
        Err(e) => {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: e.to_string(),
            });
            return;
        }
    };
    let host = parsed.host_str().unwrap_or("localhost").to_string();
    let host_header = match parsed.port() {
        Some(p) => format!("{}:{}", host, p),
        None => host,
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }
    let req_bytes = serialize_http_request(&method, &path, &host_header, &headers, &body);
    let deadline = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));

    spawn_blocking(move || {
        let try_h = tokio::runtime::Handle::try_current();
        std::hint::black_box(&try_h);
        if try_h.is_err() {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: "http client runtime unavailable".to_string(),
            });
            return;
        }
        let handle = tokio::runtime::Handle::current();
        // #5779 follow-up: keep this fetch counted in-flight for its whole
        // lifetime so the idle-kick recovers a lost worker-unpark.
        let inflight_guard = ClientInflightGuard::new();
        let jh = handle.spawn(async move {
            let _inflight = inflight_guard;
            let vtable = match perry_ffi::raw_net() {
                Some(v) => v,
                None => {
                    push_event(PendingHttpEvent::Error {
                        request_handle,
                        error_message: "agent.createConnection requires node:net (not linked)"
                            .to_string(),
                    });
                    return;
                }
            };
            // Attach is idempotent — the request path also attaches on the
            // main thread before this task runs, to close any data race.
            (vtable.attach)(socket_id);
            if (vtable.write)(socket_id, req_bytes.as_ptr(), req_bytes.len()) == 0 {
                push_event(PendingHttpEvent::Error {
                    request_handle,
                    error_message: "failed to write request to agent socket".to_string(),
                });
                return;
            }

            let mut raw = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            let start = tokio::time::Instant::now();
            loop {
                let n = (vtable.poll_read)(socket_id, chunk.as_mut_ptr(), chunk.len());
                if n > 0 {
                    raw.extend_from_slice(&chunk[..n as usize]);
                } else if n == 0 {
                    break; // clean EOF — peer closed after the response
                } else {
                    if start.elapsed() >= deadline {
                        (vtable.close)(socket_id);
                        push_event(PendingHttpEvent::Timeout { request_handle });
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
            (vtable.close)(socket_id);

            match parse_http_response(&raw) {
                Ok(parsed) => push_event(PendingHttpEvent::Response {
                    request_handle,
                    status: parsed.status,
                    status_message: parsed.status_message,
                    headers: parsed.headers,
                    trailers: parsed.trailers,
                    body: parsed.body,
                }),
                Err(error_message) => push_event(PendingHttpEvent::Error {
                    request_handle,
                    error_message,
                }),
            }
        });
        std::hint::black_box(&jh);
        std::mem::forget(jh);
    });
}

/// #2154 — invoke a user `createSocket(req, options, cb)` override on the
/// request path (Node's `Agent.prototype.addRequest` semantics). Builds the
/// three arguments Node passes:
///
/// - `req`  — the ClientRequest, NaN-boxed the same way every http handle
///   value is (`POINTER_TAG | handle`), so an override that reads `req.method`
///   etc. dispatches through the http native table.
/// - `options` — the `{ host, port, path }` object (shared with the
///   `createConnection` path).
/// - `cb` — a native closure backed by [`http_create_socket_cb`]. When the
///   override calls `cb(err, socket)`, the continuation surfaces the error or
///   drives the HTTP/1.1 exchange over the delivered socket.
///
/// Must run on the main thread — it calls a JS closure, and arena-bound
/// JSValues are invalid off-thread.
unsafe fn invoke_create_socket(
    request_handle: Handle,
    agent_handle: Handle,
    host: &str,
    port: u16,
    path: &str,
) {
    let cs = agent::create_socket_override(agent_handle);
    if cs == 0 {
        return;
    }
    // Register the continuation's arity as 2 so a 1-arg `cb(err)` pads the
    // socket slot with `undefined` (via the runtime's arity dispatch) instead
    // of reading an uninitialized register for the second parameter.
    static REGISTER_ARITY: Once = Once::new();
    REGISTER_ARITY.call_once(|| {
        perry_runtime::closure::js_register_closure_arity(http_create_socket_cb as *const u8, 2);
    });

    let cb = perry_runtime::closure::js_closure_alloc(http_create_socket_cb as *const u8, 1);
    if cb.is_null() {
        return;
    }
    // Capture the ClientRequest handle so the continuation can re-read the
    // (still-stored) method/url/headers/body and resume dispatch. Stored as an
    // f64 (a small registry id, not a heap pointer) — pointer-free, so it
    // needs no GC layout fixup, matching `sqlite_tx_wrapper`'s db-handle slot.
    perry_runtime::closure::js_closure_set_capture_f64(cb, 0, request_handle as f64);

    let cb_val = f64::from_bits(POINTER_TAG | (cb as usize as u64 & PTR_MASK));
    let req_val = f64::from_bits(POINTER_TAG | (request_handle as u64 & PTR_MASK));
    let options = agent::build_connect_options(host, port, path);

    let closure = JsClosure::from_raw(cs as *const RawClosureHeader);
    closure.call3(req_val, options, cb_val);
}

/// Continuation for a `createSocket` override's `cb(err, socket)` callback.
/// Capture slot 0 holds the ClientRequest handle id (as f64).
///
/// Mirrors the socket-id extraction in `agent::try_create_connection_socket`:
/// the override hands back a `net.Socket` (POINTER_TAG-boxed handle, or a bare
/// small handle on some codegen paths).
unsafe extern "C" fn http_create_socket_cb(
    closure: *const perry_runtime::ClosureHeader,
    err: f64,
    socket: f64,
) -> f64 {
    let request_handle =
        perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as i64 as Handle;

    // Node calls `cb(err)` on failure, `cb(null, socket)` on success.
    let err_bits = err.to_bits();
    if err_bits != TAG_UNDEFINED && err_bits != TAG_NULL {
        // Use the value only when it's genuinely a string (STRING_TAG); an
        // `Error` object is a POINTER_TAG value that `extract_string_value`
        // would misread as a `StringHeader`. Surfacing a full Error object on
        // the request's `'error'` event would need object introspection Perry
        // doesn't expose to this crate yet — a generic message keeps the event
        // firing without a bogus read.
        let error_message = if err_bits >> 48 == 0x7FFF {
            extract_string_value(err).unwrap_or_else(|| "socket creation failed".to_string())
        } else {
            "socket creation failed".to_string()
        };
        push_event(PendingHttpEvent::Error {
            request_handle,
            error_message,
        });
        return f64::from_bits(TAG_UNDEFINED);
    }

    let bits = socket.to_bits();
    let upper = bits >> 48;
    let socket_id = if upper == 0x7FFD {
        (bits & PTR_MASK) as i64
    } else if upper == 0 && bits >= 0x10000 {
        bits as i64
    } else {
        0
    };
    if socket_id <= 0 {
        push_event(PendingHttpEvent::Error {
            request_handle,
            error_message: "agent.createSocket callback did not provide a socket".to_string(),
        });
        return f64::from_bits(TAG_UNDEFINED);
    }

    // The request fields were cloned (not cleared) by `request_end`, so they're
    // still readable on the handle — re-snapshot and drive the exchange.
    let snap = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        (
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.timeout_ms,
        )
    });
    if let Some((method, url, headers, body, timeout_ms)) = snap {
        // Attach raw mode on the main thread before the async task runs, to
        // close the same data race the `createConnection` path guards against.
        if let Some(vt) = perry_ffi::raw_net() {
            (vt.attach)(socket_id);
        }
        dispatch_request_over_socket(
            request_handle,
            method,
            url,
            headers,
            body,
            timeout_ms,
            socket_id,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

// ------------------------------------------------------------------
// FFI: http.request / https.request / http.get / https.get
// ------------------------------------------------------------------

unsafe fn request_common(arg_f64: f64, callback: i64, default_protocol: &str) -> Handle {
    ensure_gc_scanner_registered();
    // Issue #769 — accept either a URL string or an options object. Mirrors
    // the dispatch in `get_common` so `http.request("http://…", cb)` works
    // the same as `http.request({ host, port, path }, cb)`.
    let (method, url, headers, timeout, agent_handle) = if is_string_value(arg_f64) {
        let raw = extract_string_value(arg_f64).unwrap_or_default();
        validate_client_url_string(&raw); // #4907
        let url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw
        } else if !raw.is_empty() {
            format!("{}://{}", default_protocol, raw)
        } else {
            String::new()
        };
        ("GET".to_string(), url, HashMap::new(), None, 0)
    } else {
        let opts = parse_options_object(arg_f64).unwrap_or(serde_json::Value::Null);
        validate_client_options(&opts, default_protocol); // #4907
        let method = method_from_options(&opts);
        let url = url_from_options(&opts, default_protocol);
        let headers = headers_from_options(&opts);
        let timeout = timeout_from_options(&opts);
        // #2154: `options.agent` doesn't survive the JSON round-trip
        // (pointer-tagged values get dropped) — read the field straight
        // off the NaN-boxed object instead.
        let agent_handle = agent::agent_handle_from_options(arg_f64).unwrap_or(0);
        (method, url, headers, timeout, agent_handle)
    };
    let handle = make_request_handle(method, url, headers, timeout, callback, agent_handle);
    attach_tls_options(handle, arg_f64); // #4906
    continue_client::defer_arm(handle); // #5080 (next-tick head flush)
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_http_request(opts_f64: f64, callback_i64: i64) -> Handle {
    request_common(opts_f64, callback_i64, "http")
}

/// `new http.ClientRequest(options)` (#4904). Perry's client model defers
/// the actual send to `.end()`, so constructing is exactly `http.request`
/// without a response callback. Node coerces a falsy `options.method` /
/// `options.path` to the `GET` / `/` defaults — `method_from_options`
/// handles the method side (#4970) and the empty path already reads back
/// as `/` through the surface.
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_standalone_new(opts_f64: f64) -> Handle {
    request_common(opts_f64, 0, "http")
}

#[no_mangle]
pub unsafe extern "C" fn js_https_request(opts_f64: f64, callback_i64: i64) -> Handle {
    request_common(opts_f64, callback_i64, "https")
}

unsafe fn get_common(arg_f64: f64, callback: i64, default_protocol: &str) -> Handle {
    ensure_gc_scanner_registered();
    let (url, headers, timeout, agent_handle) = if is_string_value(arg_f64) {
        let raw = extract_string_value(arg_f64).unwrap_or_default();
        validate_client_url_string(&raw); // #4907
        let url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw
        } else if !raw.is_empty() {
            format!("{}://{}", default_protocol, raw)
        } else {
            String::new()
        };
        (url, HashMap::new(), None, 0)
    } else {
        let opts = parse_options_object(arg_f64).unwrap_or(serde_json::Value::Null);
        validate_client_options(&opts, default_protocol); // #4907
        let url = url_from_options(&opts, default_protocol);
        let headers = headers_from_options(&opts);
        let timeout = timeout_from_options(&opts);
        let agent_handle = agent::agent_handle_from_options(arg_f64).unwrap_or(0);
        (url, headers, timeout, agent_handle)
    };

    let handle = make_request_handle(
        "GET".to_string(),
        url,
        headers,
        timeout,
        callback,
        agent_handle,
    );
    attach_tls_options(handle, arg_f64); // #4906
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
// FFI: overload-normalizing client factories (#3226 / #3227 / #3228)
//
// Codegen routes `http.request` / `http.get` / `https.request` /
// `https.get` to these `*_overload` entry points with a single
// `NA_VARARGS` argument — a JS array holding every user argument.
// `parse_client_args` resolves `(url, options, callback)` by value
// type so all overloads work: `(url[, cb])`, `(options[, cb])`, and
// `(url, options[, cb])`. The URL supplies protocol/host/port/path;
// options override method/headers/timeout/agent (and any explicitly
// set protocol/host/port/path).
// ------------------------------------------------------------------

unsafe fn request_overload(args_array: i64, default_protocol: &str, force_get: bool) -> Handle {
    ensure_gc_scanner_registered();
    let parsed = parse_client_args(args_array);
    // #4907 — validate before building the request handle. A string URL
    // argument is validated as a WHATWG URL; the options bag is validated for
    // method / path / headers / protocol / option types.
    if is_string_value(parsed.url) {
        let raw = extract_string_value(parsed.url).unwrap_or_default();
        validate_client_url_string(&raw);
    }
    if let Some(opts) = parse_options_object(parsed.opts) {
        validate_client_options(&opts, default_protocol);
    }
    let method = method_for_overload(parsed.opts);
    let (url, headers, timeout, agent_handle) =
        merge_url_and_options(parsed.url, parsed.opts, default_protocol);
    let handle = make_request_handle(method, url, headers, timeout, parsed.callback, agent_handle);
    attach_tls_options(handle, parsed.opts); // #4906 — TLS options ride on the options bag
    if force_get {
        // `get()` auto-`end()`s, kicking off the request.
        js_http_client_request_end(handle, f64::from_bits(TAG_UNDEFINED));
    } else {
        continue_client::defer_arm(handle); // #5080 (next-tick head flush)
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_http_request_overload(args_array: i64) -> Handle {
    request_overload(args_array, "http", false)
}

#[no_mangle]
pub unsafe extern "C" fn js_https_request_overload(args_array: i64) -> Handle {
    request_overload(args_array, "https", false)
}

#[no_mangle]
pub unsafe extern "C" fn js_http_get_overload(args_array: i64) -> Handle {
    request_overload(args_array, "http", true)
}

#[no_mangle]
pub unsafe extern "C" fn js_https_get_overload(args_array: i64) -> Handle {
    request_overload(args_array, "https", true)
}

// http.Agent / https.Agent (#2129 / #2154) lives in `agent.rs`.

// ------------------------------------------------------------------
// FFI: ClientRequest accessors
// ------------------------------------------------------------------

/// `req.write(chunk)` — append data to the request body.
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_write(handle: Handle, body_f64: f64) -> Handle {
    client_request_write_impl(handle, body_f64)
}

unsafe fn client_request_write_impl(handle: Handle, body_f64: f64) -> Handle {
    // #4909 — Buffer chunks used to be misread as StringHeaders (and
    // dropped); route through the buffer-aware chunk reader.
    if let Some(body) = client_outgoing::chunk_to_bytes(body_f64) {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            req.body.extend_from_slice(&body);
        });
    }
    handle
}

/// `req.end(body?)` — finalize and dispatch the request. Optional
/// trailing body chunk is appended before sending. Idempotent: a
/// second call after `ended=true` is a no-op.
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_end(handle: Handle, body_f64: f64) -> Handle {
    client_request_end_impl(handle, body_f64)
}

pub(crate) unsafe fn client_request_end_impl(handle: Handle, body_f64: f64) -> Handle {
    // An aborted/destroyed request never dispatches — Node's `abort()`
    // before `end()` means the server must not see the request and no
    // `'error'` fires (test-http-abort-before-end).
    if client_request_surface::request_destroyed(handle) {
        return handle;
    }
    if let Some(body) = client_outgoing::chunk_to_bytes(body_f64) {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            req.body.extend_from_slice(&body);
        });
    }

    // #5080 — `end()` is a send boundary: arm the continue path now if it
    // carries `Expect: 100-continue` and the next-tick arm hasn't run yet.
    continue_client::arm_expect_continue(handle);

    // #5080 — an `Expect: 100-continue` request flushed its head up front;
    // this `end()` just hands the (now-known) body to the in-flight continue
    // exchange over the oneshot. The first call fires the flush ordering
    // (write/finish/end callbacks); a later one is an idempotent no-op.
    let (is_continue, first_end) = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if !req.expects_continue {
            return (false, false);
        }
        if let Some(tx) = req.continue_body_tx.take() {
            let body = std::mem::take(&mut req.body);
            let _ = tx.send(body);
            req.ended = true;
            (true, true)
        } else {
            (true, false)
        }
    })
    .unwrap_or((false, false));
    if is_continue {
        if first_end {
            push_event(PendingHttpEvent::Flushed {
                request_handle: handle,
            });
        }
        return handle;
    }

    let snapshot = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if req.ended {
            // Already dispatched by `flushHeaders()` — the exchange is in
            // flight, but this `end()` still owes its write/finish/end
            // callback ordering (once).
            if req.flushed_early {
                req.flushed_early = false;
                return Err(true);
            }
            return Err(false);
        }
        req.ended = true;
        Ok((
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.timeout_ms,
            req.agent_handle,
            req.tls.clone(),
        ))
    });

    let snapshot = match snapshot {
        Some(Ok(s)) => s,
        Some(Err(owes_flush)) => {
            if owes_flush {
                push_event(PendingHttpEvent::Flushed {
                    request_handle: handle,
                });
            }
            return handle;
        }
        None => return handle,
    };

    // #4909 — queue the flush notification before dispatching so the
    // write/end callbacks and `'finish'` drain ahead of any `'response'`.
    push_event(PendingHttpEvent::Flushed {
        request_handle: handle,
    });

    dispatch_request_snapshot(handle, snapshot);
    handle
}

/// `req.flushHeaders()` — Node opens the connection and puts the request
/// head on the wire immediately. Our transport sends a complete request in
/// one shot, so for a request with no buffered body (and a method that
/// doesn't usually carry one) this dispatches the exchange now; a later
/// `end()` only drains the callback ordering. Requests that already
/// buffered body bytes (or use body-carrying methods) keep the
/// dispatch-at-`end()` behavior, since the head can't go out alone.
pub(crate) unsafe fn client_request_flush_headers(handle: Handle) {
    if client_request_surface::request_destroyed(handle) {
        return;
    }
    // #5080 — `flushHeaders()` is a send boundary; when it arms the continue
    // path, that exchange owns the head, so don't also dispatch via reqwest.
    continue_client::arm_expect_continue(handle);
    if with_handle_mut::<ClientRequestHandle, _, _>(handle, |r| r.expects_continue).unwrap_or(false)
    {
        return;
    }
    let snapshot = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if req.ended || !req.body.is_empty() {
            return None;
        }
        let method = req.method.to_ascii_uppercase();
        if !matches!(method.as_str(), "GET" | "HEAD" | "DELETE" | "OPTIONS") {
            return None;
        }
        req.ended = true;
        req.flushed_early = true;
        Some((
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            Vec::new(),
            req.timeout_ms,
            req.agent_handle,
            req.tls.clone(),
        ))
    })
    .flatten();
    if let Some(snapshot) = snapshot {
        dispatch_request_snapshot(handle, snapshot);
    }
}

type RequestSnapshot = (
    String,
    String,
    HashMap<String, String>,
    Vec<u8>,
    Option<u64>,
    Handle,
    tls_client::TlsOptions,
);

/// The shared dispatch tail of `end()` / `flushHeaders()`: route through the
/// agent's `createConnection` / `createSocket` override when present, else
/// the reqwest path.
unsafe fn dispatch_request_snapshot(handle: Handle, snapshot: RequestSnapshot) {
    let (method, url, headers, body, timeout_ms, agent_handle, tls) = snapshot;

    // #2154 — if the agent supplied a `createConnection` / `createSocket`
    // override, invoke it here on the main thread (JS closure calls must not
    // run on a tokio worker) and run the HTTP exchange over the socket it
    // produces instead of through reqwest. Falls back to the reqwest path when
    // there's no override or it didn't yield a usable socket.
    if agent_handle != 0 {
        if let Some((host, port, path)) = socket_connect_target(&url) {
            // Node's `Agent.prototype.addRequest` calls
            // `createSocket(req, options, cb)`; a user override is expected to
            // deliver the socket via `cb(err, socket)`. Prefer it over
            // `createConnection` — the cb continuation
            // (`http_create_socket_cb`) resumes the exchange — so we don't
            // fall through to reqwest after dispatching it.
            if agent::create_socket_override(agent_handle) != 0 {
                invoke_create_socket(handle, agent_handle, &host, port, &path);
                return;
            }
            if let Some(socket_id) =
                agent::try_create_connection_socket(agent_handle, &host, port, &path)
            {
                // Attach raw mode now (main thread) so no inbound byte can be
                // dispatched as a JS 'data' event before the task takes over.
                if let Some(vt) = perry_ffi::raw_net() {
                    (vt.attach)(socket_id);
                }
                dispatch_request_over_socket(
                    handle, method, url, headers, body, timeout_ms, socket_id,
                );
                return;
            }
        }
    }

    dispatch_request(
        handle,
        method,
        url,
        headers,
        body,
        timeout_ms,
        agent_handle,
        tls,
    );
}

/// Parse a request URL into the `(host, port, path)` an
/// `agent.createConnection` override expects in its options object. Returns
/// `None` if the URL doesn't parse or has no host.
fn socket_connect_target(url: &str) -> Option<(String, u16, String)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_string();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }
    Some((host, port, path))
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
    http_on_impl(handle, event_ptr, callback)
}

unsafe fn http_on_impl(handle: Handle, event_ptr: *const StringHeader, callback: i64) -> Handle {
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
    client_request_surface::set_header(handle, &name, value);
    handle
}

/// `req.setTimeout(ms)`.
#[no_mangle]
pub unsafe extern "C" fn js_http_set_timeout(handle: Handle, ms: f64) -> Handle {
    client_request_set_timeout_impl(handle, ms)
}

pub(crate) unsafe fn client_request_set_timeout_impl(handle: Handle, ms: f64) -> Handle {
    // Node's `socket.setTimeout` (which backs `ClientRequest.setTimeout`)
    // routes the delay through validateTimerDuration → enroll: an out-of-range
    // (> 2**31-1) delay is clamped to TIMEOUT_MAX and a `TimeoutOverflowWarning`
    // is emitted. Mirror that so `req.setTimeout(0xffffffff)` parity-matches
    // Node instead of silently storing the raw value. (#4910)
    const TIMEOUT_MAX: f64 = 2_147_483_647.0;
    let effective = if ms > TIMEOUT_MAX {
        emit_socket_timeout_overflow_warning(ms);
        TIMEOUT_MAX
    } else {
        ms
    };
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        // Node: `setTimeout(0)` clears the inactivity timer.
        req.timeout_ms = if effective > 0.0 {
            Some(effective as u64)
        } else {
            None
        };
    });
    handle
}

/// Emit Node's `TimeoutOverflowWarning` for an out-of-range socket timeout.
/// The net/timers path warns with a message distinct from the global timer
/// path ("Timer duration was truncated to 2147483647." rather than "Timeout
/// duration was set to 1.") because the socket timeout clamps to TIMEOUT_MAX,
/// not 1. (#4910)
unsafe fn emit_socket_timeout_overflow_warning(ms: f64) {
    let value_text = if ms.is_finite() && ms.fract() == 0.0 {
        format!("{}", ms as i64)
    } else {
        format!("{ms}")
    };
    let message = format!(
        "{value_text} does not fit into a 32-bit signed integer.\n\
         Timer duration was truncated to 2147483647."
    );
    let msg_ptr = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let label = "TimeoutOverflowWarning";
    let label_ptr = perry_runtime::js_string_from_bytes(label.as_ptr(), label.len() as u32);
    let msg_value = f64::from_bits(perry_runtime::JSValue::string_ptr(msg_ptr).bits());
    let label_value = f64::from_bits(perry_runtime::JSValue::string_ptr(label_ptr).bits());
    perry_runtime::process::js_process_emit_warning(
        msg_value,
        label_value,
        f64::from_bits(TAG_UNDEFINED),
    );
}

/// `IncomingMessage.setEncoding(encoding)` for client responses. The same
/// static `IncomingMessage` class tag is used for server requests, so a client
/// registry miss is forwarded to the server-side handle implementation.
#[no_mangle]
pub unsafe extern "C" fn js_http_incoming_message_set_encoding(
    handle: Handle,
    encoding_ptr: *const StringHeader,
) -> Handle {
    let encoding = read_str(encoding_ptr).unwrap_or_else(|| "utf8".to_string());
    let mut matched = false;
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.encoding = Some(encoding.clone());
        matched = true;
    });
    if matched {
        return handle;
    }

    extern "C" {
        fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
        fn js_node_http_im_set_encoding(handle: i64, encoding_ptr: *const StringHeader) -> i64;
    }
    if js_ext_http_incoming_message_is_handle(handle) != 0 {
        js_node_http_im_set_encoding(handle, encoding_ptr);
    }
    handle
}

/// `res.pipe(dest)` for a client `IncomingMessage`: remember the destination
/// writable so the body-delivery handlers forward each chunk to `dest.write()`
/// and finish it with `dest.end()`. Returns `dest` per Node's
/// pipe-returns-destination contract (node-fetch keeps only the return value:
/// `const body = res.pipe(new PassThrough())`). Without this the destination
/// never receives data and `response.text()` hangs forever.
#[no_mangle]
pub unsafe extern "C" fn js_http_incoming_message_pipe(handle: Handle, dest: f64) -> f64 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.pipes.push(dest.to_bits());
    });
    dest
}

/// Distinct external-client setter for stdlib fallback dispatch. The legacy
/// `js_http_incoming_message_set_encoding` symbol is shared with perry-stdlib.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_client_incoming_message_set_encoding(
    handle: Handle,
    encoding_ptr: *const StringHeader,
) -> Handle {
    let encoding = read_str(encoding_ptr).unwrap_or_else(|| "utf8".to_string());
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.encoding = Some(encoding);
    });
    handle
}

#[no_mangle]
pub extern "C" fn js_http_client_request_method(handle: Handle) -> *mut StringHeader {
    let method = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| req.method.clone())
        .unwrap_or_default();
    alloc_string(&method).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_protocol(handle: Handle) -> *mut StringHeader {
    let protocol = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        reqwest::Url::parse(&req.url)
            .map(|u| format!("{}:", u.scheme()))
            .unwrap_or_default()
    })
    .unwrap_or_default();
    alloc_string(&protocol).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_host(handle: Handle) -> *mut StringHeader {
    let host = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        reqwest::Url::parse(&req.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_default()
    })
    .unwrap_or_default();
    alloc_string(&host).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_path(handle: Handle) -> *mut StringHeader {
    let path = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        reqwest::Url::parse(&req.url)
            .map(|u| {
                let mut path = u.path().to_string();
                if path.is_empty() {
                    path.push('/');
                }
                if let Some(q) = u.query() {
                    path.push('?');
                    path.push_str(q);
                }
                path
            })
            .unwrap_or_default()
    })
    .unwrap_or_default();
    alloc_string(&path).as_raw()
}

#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_listener_count(
    handle: Handle,
    event_ptr: *const StringHeader,
) -> f64 {
    let event = match read_str(event_ptr) {
        Some(e) => e,
        None => return 0.0,
    };
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        let explicit = req.listeners.get(&event).map(|v| v.len()).unwrap_or(0);
        let implicit_response = if event == "response" && req.response_callback != 0 {
            1
        } else {
            0
        };
        (explicit + implicit_response) as f64
    })
    .unwrap_or(0.0)
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

/// Distinct external-client probe for stdlib fallback dispatch.
#[no_mangle]
pub extern "C" fn js_ext_http_client_incoming_message_is_handle(handle: Handle) -> i32 {
    js_http_is_incoming_message(handle)
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
        out = build_response_headers_object(&res.headers);
    });
    if out.to_bits() == TAG_UNDEFINED {
        if let Some(server_out) = server_incoming_property(handle, "headers") {
            return server_out;
        }
    }
    out
}

/// `res.trailers` — HTTP trailers populated after the body completes.
#[no_mangle]
pub extern "C" fn js_http_response_trailers(handle: Handle) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = map_to_js_object(&res.trailers);
    });
    if out.to_bits() == TAG_UNDEFINED {
        if let Some(server_out) = server_incoming_property(handle, "trailers") {
            return server_out;
        }
    }
    out
}

fn server_incoming_property(handle: Handle, property_name: &str) -> Option<f64> {
    extern "C" {
        fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
        fn js_ext_http_incoming_message_dispatch_property(
            handle: i64,
            property_ptr: *const u8,
            property_len: usize,
        ) -> f64;
    }
    unsafe {
        if js_ext_http_incoming_message_is_handle(handle) == 0 {
            return None;
        }
        Some(js_ext_http_incoming_message_dispatch_property(
            handle,
            property_name.as_ptr(),
            property_name.len(),
        ))
    }
}

pub(crate) fn body_chunk_value(body: &[u8], encoding: Option<&str>) -> f64 {
    match encoding {
        Some(_) => {
            let s = String::from_utf8_lossy(body).into_owned();
            let header = alloc_string(&s);
            f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
        }
        None => {
            let buf = perry_ffi::alloc_buffer(body);
            if buf.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
            }
        }
    }
}

// ------------------------------------------------------------------
// Event-loop pump
// ------------------------------------------------------------------

/// Number of pending events the main loop should drain.
#[no_mangle]
pub extern "C" fn js_http_has_pending() -> i32 {
    let has_events = HTTP_PENDING_EVENTS
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false);
    if has_events {
        unsafe {
            js_http_process_pending();
        }
    }
    HTTP_PENDING_EVENTS
        .lock()
        .map(|q| if q.is_empty() { 0 } else { 1 })
        .unwrap_or(0)
}

/// Drain the pending HTTP-event queue and fire user callbacks. Called
/// from codegen's event-loop tick. Returns count of events drained.
#[no_mangle]
pub unsafe extern "C" fn js_http_process_pending() -> i32 {
    // Process events ONE AT A TIME, re-reading the shared queue each iteration
    // rather than draining the whole batch into a local Vec up front.
    //
    // Why (#5783 follow-up): a response handler may RE-ENTER the event loop —
    // e.g. a `ResponseHead` invokes an async response callback that drives a
    // `for await` / `.toArray()` over a `res.pipe(PassThrough())` body. That
    // consumer's `await` block-waits, pumping the loop (which re-enters this
    // function), and its resolution depends on the body arriving via the LATER
    // `ResponseChunk`/`ResponseEnd` events of this same batch. If those were
    // already drained into a local Vec, the re-entrant pump would find an empty
    // `HTTP_PENDING_EVENTS` and the consumer would deadlock (empty body / hang).
    // Keeping unprocessed events in the shared queue lets the re-entrant drain
    // deliver them. FIFO `remove(0)` preserves event order; each event is taken
    // by exactly one (outer or re-entrant) frame, so there is no double-dispatch.
    let mut count = 0i32;
    loop {
        let ev = match HTTP_PENDING_EVENTS.lock() {
            Ok(mut q) => {
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            }
            Err(_) => return count,
        };
        let Some(ev) = ev else {
            break;
        };
        count += 1;
        match ev {
            PendingHttpEvent::Response {
                request_handle,
                status,
                status_message,
                headers,
                trailers,
                body,
            } => {
                client_events::handle_response_event(
                    request_handle,
                    status,
                    status_message,
                    headers,
                    trailers,
                    body,
                );
            }
            PendingHttpEvent::ResponseHead {
                request_handle,
                status,
                status_message,
                headers,
            } => {
                client_events::handle_response_head_event(
                    request_handle,
                    status,
                    status_message,
                    headers,
                );
            }
            PendingHttpEvent::ResponseChunk {
                request_handle,
                chunk,
            } => {
                client_events::handle_response_chunk_event(request_handle, chunk);
            }
            PendingHttpEvent::ResponseEnd { request_handle } => {
                client_events::handle_response_end_event(request_handle);
            }
            PendingHttpEvent::Error {
                request_handle,
                error_message,
            } => {
                client_events::handle_error_event(request_handle, &error_message);
            }
            PendingHttpEvent::TransportError {
                request_handle,
                message,
                code,
                syscall,
                errno,
            } => {
                client_events::handle_transport_error_event(
                    request_handle,
                    &message,
                    &code,
                    &syscall,
                    errno,
                );
            }
            PendingHttpEvent::Timeout { request_handle } => {
                client_events::handle_timeout_event(request_handle);
            }
            PendingHttpEvent::Flushed { request_handle } => {
                client_events::handle_flushed_event(request_handle);
            }
            PendingHttpEvent::Continue { request_handle } => {
                // #5080 — the server sent an interim `100 Continue`; fire the
                // request's `'continue'` listeners (the canonical handler then
                // sends the withheld body via `req.end(...)`).
                client_events::fire_request_event_listeners(request_handle, "continue");
            }
            PendingHttpEvent::DeferredArmContinue { request_handle } => {
                // #5080 — next-tick arming (see the enum variant docs).
                continue_client::arm_expect_continue(request_handle);
            }
        }
    }

    count
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests;
// Test-only `perry_ffi_*` async-bridge shims so the lib test links without the
// host stdlib archive (mirrors perry-ext-net / perry-ext-http-server).
#[cfg(test)]
#[cfg(test)]
mod test_async_shims;

// Suppress unused-import warnings for FFI-only types.
#[allow(dead_code)]
fn _force_link() -> Option<*mut ArrayHeader> {
    None
}

// #1652 / #4975: linker-retention anchors for the server FFI symbols live
// in force_link.rs (extracted to keep this file under the 2000-line cap).
mod force_link;
