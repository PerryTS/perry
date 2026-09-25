//! `HttpServer` — backing `http.createServer(handler)`. Binds on the
//! agent's turnloop loop (`turnloop_serve`), which decodes each request on
//! that thread and queues `(req, res)` for the main-thread pump; the pump
//! runs the user handler, and `res.end()` encodes and writes the response.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, iter_handles_of, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::server::ensure_gc_scanner_registered;
use crate::server::request::{handle_to_pointer_f64, with_implicit_this};
use crate::server::response::{ResponseShape, ServerResponse};
use crate::server::types::{
    extract_host, extract_port, js_handle_clear_side_tables, js_promise_run_microtasks,
    js_promise_state, js_value_is_closure, jsvalue_to_owned_string, read_string_header, Promise,
    POINTER_TAG, PTR_MASK, TAG_NULL, TAG_UNDEFINED,
};

// #4728 — in-flight (async-handler) request tracking + the reaper that
// finalizes parked requests. Extracted to keep this file under the 2000-line
// file-size limit; the dispatch paths here (`process_pending`,
// `js_node_http_server_close_all_connections`, the pump) call into it.
mod in_flight;
use in_flight::IN_FLIGHT;
pub(crate) use in_flight::{
    finalize_or_park_request, finalize_request_handles_deferred, has_in_flight_requests,
    reap_in_flight_requests, response_writable_ended,
};
mod deferred_events;
pub use deferred_events::ListenError;
use deferred_events::{drain_deferred_close_for, drain_deferred_listen_for, server_is_active};
pub(crate) use deferred_events::{
    queue_deferred_close_emit, queue_deferred_listening_emit, queue_listen_error_parts,
    register_listen_callback, withdraw_listen_callbacks,
};
mod turnloop_listen;
use turnloop_listen::try_listen_on_turnloop;
pub(crate) use turnloop_listen::{
    idle_close_ms, note_turnloop_request_aborted, queue_turnloop_connection_event,
    queue_turnloop_upgrade, turnloop_connection_closed,
};

/// Backing struct for an `http.Server` JS-side handle.
pub struct HttpServer {
    pub async_id: u64,
    /// User's `(req, res) => ...` handler. Stored as raw `i64`; the
    /// GC root scanner pins it across malloc-triggered sweeps.
    pub handler: i64,
    /// Server-level event listeners (`'request'`, `'connection'`,
    /// `'close'`, `'listening'`, `'error'`, `'upgrade'`).
    pub listeners: HashMap<String, Vec<i64>>,
    /// One-shot server listeners. Drained before their first matching emit so
    /// re-entrant emission cannot invoke them twice.
    pub once_listeners: HashMap<String, Vec<i64>>,
    /// Bound port — populated after `.listen()` resolves.
    pub bound_port: u16,
    /// Bound host (e.g. `"0.0.0.0"`).
    pub bound_host: String,
    /// True between `.listen()` and `.close()`.
    pub listening: bool,
    /// #4903 — `'listening'` emit pending: set by `.listen()` after the
    /// synchronous bind, consumed by the main-thread pump which fires the
    /// `'listening'` listeners on the next tick. Node never emits
    /// `'listening'` synchronously from inside `listen()`, so
    /// `const server = createServer().listen(0, cb)` must see `server`
    /// assigned (and late `.on('listening', ...)` registrations) before
    /// any callback runs.
    pub pending_listening_emit: bool,
    /// #4903 — `listen(port, cb)` callbacks for the pending deferred emit.
    /// Node registers the listen callback as a *once* `'listening'`
    /// listener, so `listen()` also appends it to `listeners["listening"]`
    /// (preserving Node's emit order vs. listeners added before/after
    /// `listen()`); this list is what the pump removes from the live
    /// listener list after the emit fires. Raw closure pointers; rooted
    /// by the GC scanner in lib.rs.
    pub deferred_listen_cbs: Vec<i64>,
    /// `close()` is asynchronous in Node. Keep the event pending until the
    /// next pump tick so `await once(server, 'close')` registered immediately
    /// after `close()` observes it.
    pub pending_close_emit: bool,
    pub deferred_close_cbs: Vec<i64>,
    /// A failed `listen()` waiting for its `'error'` emit on the pump's tick.
    pub pending_error_emit: Option<ListenError>,
    /// Issue #2210 — Node 18.4+ timeout knobs surfaced as both
    /// `createServer(handler, options)` and `server.<name>` property
    /// setters.
    ///
    /// Defaults mirror Node's `lib/_http_server.js`:
    ///   - `headersTimeout`: 60_000 ms
    ///   - `keepAliveTimeout`: 5_000 ms
    ///   - `keepAliveTimeoutBuffer`: 1_000 ms
    ///   - `requestTimeout`: 300_000 ms
    ///   - `timeout` (idle): 0 (disabled)
    ///   - `maxHeadersCount`: null
    ///   - `maxRequestsPerSocket`: 0 (no limit)
    ///   - `noDelay`: true (Node toggled the default in 21.0)
    ///   - `keepAlive`: false
    ///   - `keepAliveInitialDelay`: 0 ms
    pub headers_timeout: f64,
    pub keep_alive_timeout: f64,
    pub keep_alive_timeout_buffer: f64,
    pub request_timeout: f64,
    pub idle_timeout: f64,
    pub max_headers_count: f64,
    pub max_requests_per_socket: f64,
    pub no_delay: bool,
    pub keep_alive: bool,
    pub keep_alive_initial_delay: f64,
    /// #4974 — mirrors Node's `server[kConnectionsCheckingInterval]`
    /// timer state: Node creates the interval in the `Server`
    /// constructor and `clearInterval`s it in `close()`, so the timer's
    /// `_destroyed` flips to `true` once the server closes. Perry has
    /// no such timer (the connection layer owns connection lifecycle),
    /// so we track just the flag the `_http_server` introspection key
    /// exposes.
    pub connections_checking_interval_destroyed: bool,
    /// #5011 — mirrors Node's `server.unref()` / `server.ref()`. When
    /// `unref()`ed, a listening server no longer keeps the event loop
    /// alive, so the process can exit even while it's still bound. Starts
    /// `true` (refed), matching Node where a fresh server holds the loop
    /// open once it's listening.
    pub refed: bool,
    /// `Bun.serve` marks its backing `HttpServer` so pending requests use the
    /// Fetch Request/Response adapter instead of Node's `(req, res)` callback.
    pub is_bun_server: bool,
    /// Optional `Bun.serve({ error })` callback. `handler` stores the required
    /// `fetch` callback; both raw closure addresses are pinned by the scanner.
    pub bun_error_handler: i64,
    /// Observable `Server.development` option.
    pub bun_development: bool,
}

impl HttpServer {
    /// Build a new `HttpServer` with all Node 18.4+ timeout defaults.
    /// Keeps the field list off the `register_handle` call sites so a
    /// future field addition doesn't require updating every constructor
    /// (https / http2 / test fixtures).
    pub fn with_handler(handler: i64) -> Self {
        Self {
            async_id: 0,
            handler,
            listeners: HashMap::new(),
            once_listeners: HashMap::new(),
            bound_port: 0,
            bound_host: String::new(),
            listening: false,
            pending_listening_emit: false,
            deferred_listen_cbs: Vec::new(),
            pending_close_emit: false,
            deferred_close_cbs: Vec::new(),
            pending_error_emit: None,
            headers_timeout: 60_000.0,
            keep_alive_timeout: 5_000.0,
            keep_alive_timeout_buffer: 1_000.0,
            request_timeout: 300_000.0,
            idle_timeout: 0.0,
            max_headers_count: f64::from_bits(TAG_NULL),
            max_requests_per_socket: 0.0,
            no_delay: true,
            keep_alive: false,
            keep_alive_initial_delay: 0.0,
            connections_checking_interval_destroyed: false,
            refed: true,
            is_bun_server: false,
            bun_error_handler: 0,
            bun_development: false,
        }
    }
}

pub(crate) fn server_has_event_listener(server: &HttpServer, event: &str) -> bool {
    server
        .listeners
        .get(event)
        .is_some_and(|listeners| !listeners.is_empty())
        || server
            .once_listeners
            .get(event)
            .is_some_and(|listeners| !listeners.is_empty())
}

/// Snapshot persistent listeners and detach one-shot listeners before JS runs.
/// This follows EventEmitter's re-entrancy rule: a once listener is already
/// absent if its callback emits the same event recursively.
pub(crate) fn take_server_event_listeners(server: &mut HttpServer, event: &str) -> Vec<i64> {
    let mut listeners = server.listeners.get(event).cloned().unwrap_or_default();
    if let Some(once) = server.once_listeners.remove(event) {
        listeners.extend(once);
    }
    listeners
}

/// A decoded request queued for the main-thread pump.
pub struct HttpPendingRequest {
    pub server_handle: i64,
    pub request_handle: i64,
    pub response_handle: i64,
    pub skip_default_response: bool,
    pub h2_stream_handle: i64,
    pub h2_stream_headers: Vec<(String, String)>,
    /// #5080 — routing only: when set, the `'checkContinue'` listeners fire
    /// *instead of* the `'request'` listeners + handler (Node dispatches an
    /// `Expect: 100-continue` request to `'checkContinue'` when a listener
    /// exists, and only emits `'request'` otherwise). The listener ADDRESSES
    /// are deliberately not carried here: a snapshot parked in the channel
    /// goes stale across a moving collection, so the dispatcher re-reads them
    /// from the server handle (#8082).
    /// #5080 — route this request to `'checkContinue'` rather than the
    /// normal `'request'` path.
    pub is_check_continue: bool,
}

/// Phase 4 — pending upgrade ready to fire `'upgrade'` listeners, queued by
/// the turnloop connection layer once the connection has been handed to
/// perry-ext-ws (a WebSocket handshake) or perry-ext-net (a raw upgrade).
pub struct HttpPendingUpgrade {
    pub server_handle: i64,
    pub request_handle: i64,
    /// WebSocket path (real handshakes with a `Sec-WebSocket-Key`): the
    /// perry-ext-ws connection id. 0 on the raw path.
    pub ws_id: i64,
    /// #4973 raw path (keyless Upgrade requests): the perry-ext-net socket
    /// id adopted from the connection. 0 on the WebSocket path.
    pub raw_socket_id: i64,
    /// #4973 raw path: unconsumed bytes that followed the request head —
    /// Node's `upgradeHead` argument.
    pub head: Vec<u8>,
}

/// `(server_handle, socket_handle)` pairs whose accept loop saw a new
/// connection since the last pump tick. Drained by
/// `js_node_http_server_process_pending` to fire `'connection'` listeners on
/// the main thread (#4905). `socket_handle` is the connection's identity
/// (`crate::server::request::alloc_connection_socket`): the `IncomingMessage`-shaped object Node passes as the
/// listener's argument and that every request on the connection shares as
/// `req.socket`.
pub(crate) static PENDING_CONNECTION_EVENTS: Mutex<Vec<(i64, i64)>> = Mutex::new(Vec::new());

/// Read the `HttpServer` behind a JS server handle, whichever flavour it is.
///
/// `https.Server` and `http2.SecureServer` both embed an `HttpServer` as
/// `base`, and the turnloop connection layer needs the same five fields off
/// all three (`keepAliveTimeout`, `listening`, `maxRequestsPerSocket`,
/// `noDelay`, and the listener map) without caring which it has.
pub(crate) fn with_base_server<R>(handle: i64, f: impl FnOnce(&HttpServer) -> R) -> Option<R> {
    if let Some(server) = get_handle::<HttpServer>(handle) {
        return Some(f(&server));
    }
    if let Some(server) = get_handle::<crate::server::https_server::HttpsServer>(handle) {
        return Some(f(&server.base));
    }
    get_handle::<crate::server::http2_server::Http2SecureServer>(handle)
        .map(|server| f(&server.base))
}

/// Mutate the same base server used by connection admission. The callback must
/// finish before invoking JS, which can re-enter or close the server.
pub(crate) fn with_base_server_mut<R>(
    handle: i64,
    f: impl FnOnce(&mut HttpServer) -> R,
) -> Option<R> {
    if let Some(server) = get_handle_mut::<HttpServer>(handle) {
        return Some(f(server));
    }
    if let Some(server) = get_handle_mut::<crate::server::https_server::HttpsServer>(handle) {
        return Some(f(&mut server.base));
    }
    get_handle_mut::<crate::server::http2_server::Http2SecureServer>(handle)
        .map(|server| f(&mut server.base))
}

pub(crate) static TURNLOOP_UPGRADES: Mutex<std::collections::VecDeque<HttpPendingUpgrade>> =
    Mutex::new(std::collections::VecDeque::new());

/// Signal tracked connections of `server_handle` to close. With
/// `only_idle`, connections currently processing a request — or mid-way
/// through sending one (#4971) — are left alone (Node's
/// `closeIdleConnections` semantics; `server.close()` also closes idle
/// keep-alive sockets since Node 19).
pub(crate) fn signal_connections_close(server_handle: i64, only_idle: bool) {
    for id in crate::server::turnloop_serve::connections_of(server_handle) {
        if !only_idle || !crate::server::turnloop_serve::is_busy(id) {
            crate::server::turnloop_serve::destroy_connection(id);
        }
    }
}

// ============================================================================
// FFI: createServer / listen / close / address
// ============================================================================

/// `http.createServer(handler)` — register an `HttpServer` handle.
#[no_mangle]
pub extern "C" fn js_node_http_create_server(handler: i64) -> i64 {
    ensure_gc_scanner_registered();
    register_handle(HttpServer::with_handler(handler))
}

/// Issue #2210 — `http.createServer([options][, handler])` (Node 18.4+).
/// The native-table row passes both user arguments as full NaN-boxed
/// values so this entry can normalize Node's overloads:
/// `createServer(handler, options)`, `createServer(options, handler)`,
/// `createServer(options)`, and `createServer(handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_create_server_with_options(
    first_arg: f64,
    second_arg: f64,
) -> i64 {
    ensure_gc_scanner_registered();
    let first_bits = first_arg.to_bits();
    let second_bits = second_arg.to_bits();
    let first_is_closure = js_value_is_closure(first_bits as i64) != 0;
    let second_is_closure = js_value_is_closure(second_bits as i64) != 0;
    let first_is_options = (first_bits & !PTR_MASK) == POINTER_TAG && !first_is_closure;
    let second_is_options = (second_bits & !PTR_MASK) == POINTER_TAG && !second_is_closure;
    let handler = if first_is_closure {
        (first_bits & PTR_MASK) as i64
    } else if second_is_closure {
        (second_bits & PTR_MASK) as i64
    } else {
        0
    };
    let options_f64 = if first_is_options {
        first_arg
    } else if second_is_options {
        second_arg
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let mut server = HttpServer::with_handler(handler);
    apply_server_options(&mut server, options_f64);
    register_handle(server)
}

/// Largest `requestTimeout` Node accepts, mirroring its
/// `kMaxRequestTimeout = MAX_SAFE_INTEGER` (`2**53 - 1`) ceiling in
/// `lib/_http_server.js`.
const MAX_REQUEST_TIMEOUT_MS: f64 = 9_007_199_254_740_991.0;

/// Coerce a `requestTimeout` (ms) into the same finite, non-negative,
/// integer, `MAX_SAFE_INTEGER`-bounded domain Node enforces via
/// `validateInteger(value, 'requestTimeout', 0, kMaxRequestTimeout)`,
/// so the value the in-flight reaper later casts to `u64` to build a
/// `Duration` deadline can never overflow.
///
/// The bug this guards (CodeRabbit, #5663): an un-sanitized `f64`
/// reaching `grace_ms as u64` produces a garbage deadline — Rust's
/// saturating float→int cast turns `Infinity` into `u64::MAX` (a
/// deadline that never fires) and an oversized finite value into a
/// nonsensical one. Node rejects those at construction with
/// `ERR_OUT_OF_RANGE`; lacking a throw path here, we coerce to the
/// nearest in-range value instead — non-finite falls back to Node's
/// 300s default, out-of-range clamps to `[0, MAX_SAFE_INTEGER]`, and
/// the fractional part is truncated to an integer ms count. `0` is
/// preserved (Node's "disabled" sentinel; the reaper maps it back to
/// the default).
pub(crate) fn sanitize_request_timeout(ms: f64) -> f64 {
    if !ms.is_finite() {
        return 300_000.0;
    }
    ms.trunc().clamp(0.0, MAX_REQUEST_TIMEOUT_MS)
}

/// Read each Node-documented timeout/socket knob off the options
/// object and overwrite the server's default. Missing keys leave the
/// default in place; non-numeric values silently no-op (matches
/// Node, which coerces or ignores most invalid types).
///
/// Uses the same JSON-round-trip pattern as `extract_port`/`extract_host`
/// in `types.rs` so we don't introduce a second runtime-object-read API
/// surface — keeps the crate independent of perry-runtime's internal
/// ObjectHeader layout.
pub(crate) fn apply_server_options(server: &mut HttpServer, options_f64: f64) {
    use perry_ffi::JsValue;
    let v = JsValue::from_bits(options_f64.to_bits());
    if !v.is_pointer() {
        return;
    }
    let Some(json) = perry_ffi::json_stringify(v) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) else {
        return;
    };
    let as_num = |key: &str| -> Option<f64> {
        parsed
            .get(key)
            .and_then(|v| v.as_f64())
            .filter(|n| !n.is_nan())
    };
    let as_bool = |key: &str| -> Option<bool> { parsed.get(key).and_then(|v| v.as_bool()) };

    if let Some(v) = as_num("headersTimeout") {
        server.headers_timeout = v;
    }
    if let Some(v) = as_num("keepAliveTimeout") {
        server.keep_alive_timeout = v;
    }
    if let Some(v) = as_num("keepAliveTimeoutBuffer") {
        server.keep_alive_timeout_buffer = v;
    }
    if let Some(v) = as_num("requestTimeout") {
        server.request_timeout = sanitize_request_timeout(v);
    }
    if let Some(v) = as_num("timeout") {
        server.idle_timeout = v;
    }
    if let Some(v) = as_num("maxHeadersCount") {
        server.max_headers_count = v;
    }
    if let Some(v) = as_num("maxRequestsPerSocket") {
        server.max_requests_per_socket = v;
    }
    if let Some(v) = as_num("keepAliveInitialDelay") {
        server.keep_alive_initial_delay = v;
    }
    if let Some(v) = as_bool("noDelay") {
        server.no_delay = v;
    }
    if let Some(v) = as_bool("keepAlive") {
        server.keep_alive = v;
    }
}

// ============================================================================
// Issue #2210 — server.<timeout> property accessors
// ============================================================================
//
// Seven numeric knobs (`headersTimeout`, `keepAliveTimeout`,
// `keepAliveTimeoutBuffer`, `requestTimeout`, `timeout`,
// `maxHeadersCount`, `maxRequestsPerSocket`)
// plus the `setTimeout(ms, cb?)` instance method. Phase 1 stores +
// reads back; Phase 2 (hyper connection-builder + per-request deadline)
// is the follow-up tracked in #2210. The getter/setter naming follows
// the existing `__get_<prop>` / `__set_<prop>` convention from the
// Agent and ServerResponse rows in `native_table/http.rs`.

macro_rules! server_getter {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: i64) -> f64 {
            get_handle::<HttpServer>(handle)
                .map(|s| s.$field)
                .unwrap_or(0.0)
        }
    };
}

macro_rules! server_setter {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: i64, value: f64) -> f64 {
            if let Some(s) = get_handle_mut::<HttpServer>(handle) {
                s.$field = value;
            }
            value
        }
    };
}

server_getter!(js_node_http_server_headers_timeout, headers_timeout);
server_setter!(js_node_http_server_set_headers_timeout, headers_timeout);
server_getter!(js_node_http_server_keep_alive_timeout, keep_alive_timeout);
server_setter!(
    js_node_http_server_set_keep_alive_timeout,
    keep_alive_timeout
);
server_getter!(
    js_node_http_server_keep_alive_timeout_buffer,
    keep_alive_timeout_buffer
);
server_setter!(
    js_node_http_server_set_keep_alive_timeout_buffer,
    keep_alive_timeout_buffer
);
server_getter!(js_node_http_server_request_timeout, request_timeout);
/// `server.requestTimeout = ms` — sanitized rather than macro-generated
/// so the stored value is always a finite, non-negative, integer,
/// `MAX_SAFE_INTEGER`-bounded ms count (see `sanitize_request_timeout`).
/// This keeps the in-flight reaper's `grace_ms as u64` cast from
/// overflowing on `Infinity`/oversized values regardless of which
/// setter path (constructor option or this property) wrote the field.
#[no_mangle]
pub extern "C" fn js_node_http_server_set_request_timeout(handle: i64, value: f64) -> f64 {
    let sanitized = sanitize_request_timeout(value);
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        s.request_timeout = sanitized;
    }
    value
}
server_getter!(js_node_http_server_idle_timeout, idle_timeout);
server_setter!(js_node_http_server_set_idle_timeout, idle_timeout);
server_getter!(js_node_http_server_max_headers_count, max_headers_count);
server_setter!(js_node_http_server_set_max_headers_count, max_headers_count);
server_getter!(
    js_node_http_server_max_requests_per_socket,
    max_requests_per_socket
);
server_setter!(
    js_node_http_server_set_max_requests_per_socket,
    max_requests_per_socket
);

/// `server.setTimeout(msecs, [callback])` — the canonical EventEmitter-
/// style setter. The callback (if provided) is registered as a
/// `'timeout'` listener; we store the raw closure handle and let the
/// existing listener-firing path emit it once Phase 2 wires up the
/// idle-detector. Returns the server handle for chaining.
#[no_mangle]
pub extern "C" fn js_node_http_server_set_timeout_method(
    handle: i64,
    msecs: f64,
    callback: i64,
) -> i64 {
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        s.idle_timeout = msecs;
        if callback != 0 {
            s.listeners
                .entry("timeout".to_string())
                .or_default()
                .push(callback);
        }
    }
    handle
}

/// `server.ref()` — mark the server as keeping the event loop alive
/// (the default) and return the receiver handle so chains like
/// `server.ref().listen(...)` work. #5011 — without this row the call
/// fell through to a generic handler that returned the handle as a raw
/// number, so `s.ref() === s` was false and chaining broke.
#[no_mangle]
pub extern "C" fn js_node_http_server_ref(handle: i64) -> i64 {
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        s.refed = true;
    }
    handle
}

/// `server.unref()` — stop the server from keeping the process alive and
/// return the receiver handle (Node returns `this`). Clearing `refed`
/// drops the server out of `server_is_active`, so the event loop can
/// exit even while the server is still bound. #5011.
#[no_mangle]
pub extern "C" fn js_node_http_server_unref(handle: i64) -> i64 {
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        s.refed = false;
    }
    handle
}

/// `server.listen(port?, host?, backlog?, cb?)` — bind + start accepting.
/// Returns immediately; requests are decoded by the turnloop connection layer
/// and dispatched from the main-thread pump
/// (`js_node_http_server_process_pending`).
///
/// `args_array` is a raw `*const ArrayHeader` carrying every user-supplied
/// `listen()` argument (codegen packs them via the `NA_VARARGS` arg kind).
/// `parse_listen_args` resolves Node's variadic overloads by value type — a
/// bare numeric/options/path first arg, an optional standalone host string,
/// and the (single) function callback wherever it lands. Issue #2041.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_listen(server_handle: i64, args_array: i64) -> i64 {
    listen_http_server(
        server_handle,
        crate::server::types::parse_listen_args(args_array),
    )
}

pub(super) unsafe fn listen_http_server(
    server_handle: i64,
    parsed: crate::server::types::ListenArgs,
) -> i64 {
    // Returns `server_handle` so `createServer(...).listen(...).on(...)` chains
    // correctly. Pre-#2129 this was `-> ()` and chained sites broke at runtime
    // with `undefined.on is not a function`.
    let opts_f64 = parsed.opts;
    let port = extract_port(opts_f64, 3000);
    let host = parsed
        .host
        .unwrap_or_else(|| extract_host(opts_f64, "0.0.0.0"));
    let callback = parsed.callback;

    // #4962 — cluster workers coordinate the bind with the primary. Under
    // SCHED_RR the primary owns the socket and passes accepted fds to this
    // worker (no local bind); otherwise (SCHED_NONE, or off-cluster) the
    // worker binds the primary-resolved port itself with SO_REUSEPORT so
    // `listen(0)` still shares one ephemeral port (#4914).
    let address_type: i32 = if host.contains(':') { 6 } else { 4 };
    let is_worker = crate::server::cluster_bind::is_cluster_worker();
    let rr = is_worker && crate::server::cluster_bind::worker_sched_is_rr();
    let resolved = if is_worker {
        crate::server::cluster_bind::worker_query_listen(&host, port as i32, address_type, rr)
    } else {
        None
    };
    let plan = ListenPlan {
        server_handle,
        host,
        port,
        resolved,
        rr_inject: cfg!(unix) && rr && resolved.is_some(),
        address_type,
    };

    // The bind happens on the thread that owns this agent's turnloop loop.
    // That is this thread in every ordinary program. A thread acting for an
    // agent another thread owns (an embedder's pump thread) posts the bind to
    // the owner — which serves the same JS heap — instead of running a second
    // transport of its own; see `turnloop_serve::post_to_owner`.
    if crate::server::turnloop_serve::enabled() {
        if plan.bind() {
            finish_listen(server_handle, callback);
        }
        return server_handle;
    }

    // Posted: the `listen(cb)` callback is registered now, where the closure
    // is rooted by this crate's scanner, and only the `'listening'` emit waits
    // for the bind to land on the owner's turn.
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        register_listen_callback(s, callback);
    }
    let (host, port) = (plan.host.clone(), plan.port);
    let posted = crate::server::turnloop_serve::post_to_owner(Box::new(move || {
        let server_handle = plan.server_handle;
        if crate::server::turnloop_serve::enabled() && plan.bind() {
            finish_listen(server_handle, 0);
        } else {
            if !crate::server::turnloop_serve::enabled() {
                queue_listen_error_parts(
                    server_handle,
                    &plan.host,
                    plan.port,
                    crate::server::turnloop_serve::NO_LOOP_CODE,
                    0,
                    "listen",
                );
            }
            if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
                withdraw_listen_callbacks(s);
            }
        }
    }));
    if let Some(code) = posted.error_code() {
        // No loop exists for this agent anywhere (a host where `Loop::new`
        // failed), or the owner's postbox refused every retry: report it
        // where Node reports a failed listen.
        queue_listen_error_parts(server_handle, &host, port, code, 0, "listen");
        if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
            withdraw_listen_callbacks(s);
        }
    }
    server_handle
}

/// Everything `listen()` needs on the loop's owner thread, owned so it can be
/// posted there.
struct ListenPlan {
    server_handle: i64,
    host: String,
    port: u16,
    /// The port the cluster primary handed back, if any.
    resolved: Option<u16>,
    /// A SCHED_RR worker whose primary answered: the primary owns the socket
    /// and passes accepted descriptors instead of this worker binding.
    rr_inject: bool,
    /// 4 or 6; keys the primary's descriptor channel (unix only).
    #[cfg_attr(not(unix), allow(dead_code))]
    address_type: i32,
}

impl ListenPlan {
    /// Bind (or, for a SCHED_RR worker, start adopting the primary's
    /// descriptors) on this thread's loop. Returns whether the server is now
    /// listening; a failure has already been reported (queued as `'error'`).
    fn bind(&self) -> bool {
        if self.rr_inject {
            return self.start_rr_inject();
        }
        match try_listen_on_turnloop(self.server_handle, &self.host, self.port, self.resolved) {
            // The bind failed and `try_listen_on_turnloop` queued the error.
            Some(0) => false,
            Some(_) => true,
            // Only reachable if the loop vanished between the caller's check
            // and the bind: report it rather than leave the listen hanging.
            None => {
                queue_listen_error_parts(
                    self.server_handle,
                    &self.host,
                    self.port,
                    crate::server::turnloop_serve::NO_LOOP_CODE,
                    0,
                    "listen",
                );
                false
            }
        }
    }

    /// SCHED_RR worker (#4962): the primary accepts and passes each
    /// connection's descriptor over the cluster IPC channel. A bridge thread
    /// parks on `recv_fd` and hands every descriptor to this agent's loop
    /// (`turnloop_serve::adopt_connection`), where it is served exactly like a
    /// connection turnloop accepted itself.
    #[cfg(unix)]
    fn start_rr_inject(&self) -> bool {
        let actual_port = self.resolved.expect("rr_inject implies a resolved port");
        crate::server::cluster_bind::notify_listening(&self.host, actual_port);
        let no_delay = match get_handle_mut::<HttpServer>(self.server_handle) {
            Some(s) => {
                s.bound_port = actual_port;
                s.bound_host = self.host.clone();
                s.listening = true;
                s.no_delay
            }
            None => return false,
        };
        let key_id =
            crate::server::cluster_bind::compute_key_id(&self.host, actual_port, self.address_type);
        let closed = Arc::new(AtomicBool::new(false));
        RR_INJECT_CLOSED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.server_handle, closed.clone());
        let server_handle = self.server_handle;
        // `recv_fd` blocks until the primary passes an fd (or the channel
        // closes → fd < 0), so it runs on its own thread for the server's
        // lifetime. It never touches the loop: each descriptor is posted to
        // the owner, which adopts it.
        std::thread::spawn(move || loop {
            let fd = crate::server::cluster_bind::recv_fd(key_id);
            if fd < 0 {
                break;
            }
            // SAFETY: `recv_fd` returns a descriptor received over SCM_RIGHTS
            // that nothing else in this process owns.
            let stream =
                unsafe { <std::net::TcpStream as std::os::unix::io::FromRawFd>::from_raw_fd(fd) };
            if closed.load(Ordering::Acquire) {
                // `server.close()` ran: stop accepting. Dropping the stream
                // closes the connection the primary handed over.
                drop(stream);
                break;
            }
            // Match Node's per-connection `noDelay` (default on).
            let _ = stream.set_nodelay(no_delay);
            crate::server::turnloop_serve::adopt_connection(server_handle, stream.into());
        });
        true
    }

    #[cfg(not(unix))]
    fn start_rr_inject(&self) -> bool {
        false
    }
}

/// SCHED_RR workers' "stop accepting" flag, set by `server.close()`.
static RR_INJECT_CLOSED: std::sync::LazyLock<Mutex<HashMap<i64, Arc<AtomicBool>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// A listen that succeeded: create the server's async resource and queue the
/// deferred `'listening'` emit.
///
/// #4903 — queue the `'listening'` emit + the optional `cb` argument for the
/// main-thread pump instead of firing them synchronously. Node emits
/// `'listening'` on a later event-loop tick, after the current synchronous
/// script segment finishes; firing inline ran the callback before
/// `const server = http.createServer().listen(0, cb)` had assigned `server`,
/// so `server.address()` inside the callback threw "Cannot read properties of
/// undefined". The pump fires both with `this` bound to the server (#2132), via
/// `drain_deferred_listen_for`. The provider is initialized only after the
/// bind succeeded, so a failed listen cannot leak a resource with no matching
/// destroy edge.
fn finish_listen(server_handle: i64, callback: i64) {
    let server_async_id = unsafe {
        crate::js_async_hooks_provider_init(b"TCPSERVERWRAP".as_ptr(), b"TCPSERVERWRAP".len())
    };
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        s.async_id = server_async_id;
        queue_deferred_listening_emit(s, callback);
    } else {
        unsafe { crate::js_async_hooks_provider_destroy(server_async_id) };
    }
}

/// `server.close(cb?)` — stop accepting, fire `'close'`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_close(server_handle: i64, callback: i64) {
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        s.listening = false;
        s.connections_checking_interval_destroyed = true;
        queue_deferred_close_emit(s, callback);
    }
    // P5: stop accepting. In-flight connections finish, which is Node's
    // contract; the idle ones are destroyed by `signal_connections_close`
    // below.
    if let Some(listener) = crate::server::turnloop_serve::listener_for_server(server_handle) {
        crate::server::turnloop_serve::close_listener(listener);
    }
    // A SCHED_RR worker has no listener of its own: tell its descriptor
    // bridge to stop handing connections over.
    if let Some(closed) = RR_INJECT_CLOSED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&server_handle)
    {
        closed.store(true, Ordering::Release);
    }
    // Node 19+: `server.close()` destroys idle keep-alive connections
    // (active requests are allowed to finish) (#4905).
    signal_connections_close(server_handle, true);
}

/// `server.closeAllConnections()` — destroy every tracked connection
/// of this server, including ones with an in-flight request (#4905).
#[no_mangle]
pub extern "C" fn js_node_http_server_close_all_connections(handle: i64) {
    signal_connections_close(handle, false);
    // Parked async requests on the destroyed connections can never flush
    // a response (their connection is gone) — drop them now so `has_in_flight_requests()`
    // doesn't pin the event loop for the 300s grace window. The response
    // never ended, so a handler still suspended on a slow `await` could
    // resume and write through the bare id; defer recycling that id until the
    // request's grace deadline (same use-after-recycle guard as the reaper's
    // peer-gone path) so a late write hits an empty slot, not a recycled one.
    let mut to_finalize: Vec<(i64, i64, Option<Instant>)> = Vec::new();
    if let Ok(mut guard) = IN_FLIGHT.lock() {
        guard.retain(|e| {
            if e.server_handle == handle {
                let recycle_deadline = if response_writable_ended(e.response_handle) {
                    None
                } else {
                    Some(e.deadline)
                };
                to_finalize.push((e.request_handle, e.response_handle, recycle_deadline));
                false
            } else {
                true
            }
        });
    }
    for (req, res, recycle_deadline) in to_finalize {
        finalize_request_handles_deferred(req, res, recycle_deadline);
    }
}

/// `server.closeIdleConnections()` — destroy connections with no
/// in-flight request (idle keep-alive sockets) (#4905).
#[no_mangle]
pub extern "C" fn js_node_http_server_close_idle_connections(handle: i64) {
    signal_connections_close(handle, true);
}

/// `server.address()` — returns `{ port, address, family }` as a
/// JSON-stringified object. TS-side wrapper parses with `JSON.parse`.
#[no_mangle]
pub extern "C" fn js_node_http_server_address_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<HttpServer>(handle)
        .map(|s| {
            if !s.listening {
                "null".to_string()
            } else {
                let family = if s.bound_host.contains(':') {
                    "IPv6"
                } else {
                    "IPv4"
                };
                serde_json::json!({
                    "port": s.bound_port,
                    "address": s.bound_host,
                    "family": family,
                })
                .to_string()
            }
        })
        .unwrap_or_else(|| "null".to_string());
    alloc_string(&s).as_raw()
}

/// `server.listening` getter.
#[no_mangle]
pub extern "C" fn js_node_http_server_listening(handle: i64) -> i32 {
    get_handle::<HttpServer>(handle)
        .map(|s| if s.listening { 1 } else { 0 })
        .unwrap_or(0)
}

/// `server.listening` getter as a JS boolean value.
#[no_mangle]
pub extern "C" fn js_node_http_server_listening_value(handle: i64) -> f64 {
    f64::from_bits(
        JsValue::from_bool(
            get_handle::<HttpServer>(handle)
                .map(|s| s.listening)
                .unwrap_or(false),
        )
        .bits(),
    )
}

/// `server.on(event, cb)` — register a listener. Standard event names:
/// `'request'`, `'connection'`, `'close'`, `'listening'`, `'error'`,
/// `'upgrade'`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        s.listeners.entry(event).or_default().push(callback);
    }
    handle_to_pointer_f64(handle)
}

/// `server.removeAllListeners([event])` — drop every listener for `event`,
/// or every listener for every event when `event_name_ptr` is null (#4973:
/// test-http-upgrade-server clears its `'upgrade'` listeners between
/// phases so the next Upgrade request falls through to `'request'`).
///
/// # Safety
/// FFI entry; `event_name_ptr` is either null or a valid StringHeader.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_remove_all_listeners(
    handle: i64,
    event_name_ptr: *const StringHeader,
) -> f64 {
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        if event_name_ptr.is_null() {
            s.listeners.clear();
            s.once_listeners.clear();
            s.handler = 0;
        } else if let Some(event) = read_string_header(event_name_ptr as *mut _) {
            s.listeners.remove(&event);
            s.once_listeners.remove(&event);
            if event == "request" {
                s.handler = 0;
            }
        }
    }
    handle_to_pointer_f64(handle)
}

/// `server.removeListener(event, cb)` / `server.off(event, cb)` — remove one
/// registration of `cb` for `event` (last-registered first, matching Node).
///
/// # Safety
/// FFI entry; pointers must be valid.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_remove_listener(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if let Some(s) = get_handle_mut::<HttpServer>(handle) {
        let removed = if let Some(callbacks) = s.listeners.get_mut(&event) {
            if let Some(position) = callbacks.iter().rposition(|entry| *entry == callback) {
                callbacks.remove(position);
                true
            } else {
                false
            }
        } else {
            false
        };
        if !removed {
            let mut removed_once = false;
            if let Some(callbacks) = s.once_listeners.get_mut(&event) {
                if let Some(position) = callbacks.iter().rposition(|entry| *entry == callback) {
                    callbacks.remove(position);
                    removed_once = true;
                }
            }
            if !removed_once && event == "request" && s.handler == callback {
                s.handler = 0;
            }
        }
    }
    handle_to_pointer_f64(handle)
}

// ============================================================================
// Request dispatch — main-thread event loop
// ============================================================================

// ============================================================================
// Issue #604/#9696 — main-thread pump registered with perry-runtime.
//
// Pre-#604, `js_node_http_server_listen` ended in `event_loop(...)` —
// an infinite blocking loop on the main TS thread that drained pending
// requests and upgrades synchronously. That blocked `await new
// Promise(r => server.listen(port, r))` from ever returning, so any
// code after `listen()` (e.g. `axios.get(...)`, `server.close()`)
// never ran.
//
// Replacement: `listen()` returns immediately after starting to accept
// on the agent's turnloop loop. The new
// `js_node_http_server_has_active` and `js_node_http_server_process_pending`
// callbacks are registered with the runtime when this extension initializes.
// The codegen-emitted main loop invokes the registry each tick, so requests +
// upgrades are dispatched on the same main thread as before — just driven
// from the outer event loop instead of an inner blocking one.
//
// Both externs walk the global handle registry via `iter_handles_of`
// (covers HTTP/1, HTTPS, and HTTP/2 — HTTPS / HTTP/2 wrap an
// `HttpServer` inside their own struct, so checking the standalone
// HttpServers + the `.base` of the wrappers covers all three).
// ============================================================================

/// Returns 1 if any registered HTTP/HTTPS/HTTP/2 server is currently
/// listening, has pending requests, or has pending upgrade events.
/// Registered as a runtime keepalive contributor so the codegen-emitted main
/// loop cannot exit before the accept loop has a chance to push the first
/// request through the channel.
#[no_mangle]
pub extern "C" fn js_node_http_server_has_active() -> i32 {
    let mut active = 0i32;
    iter_handles_of::<HttpServer, _>(|s| {
        if server_is_active(s) {
            active = 1;
        }
    });
    if active != 0 {
        return 1;
    }
    iter_handles_of::<crate::server::https_server::HttpsServer, _>(|s| {
        if server_is_active(&s.base) {
            active = 1;
        }
    });
    if active != 0 {
        return 1;
    }
    iter_handles_of::<crate::server::http2_server::Http2SecureServer, _>(|s| {
        if server_is_active(&s.base) {
            active = 1;
        }
    });
    if active == 0 && crate::server::http2_server::has_active_h2_clients() {
        active = 1;
    }
    // #4728 — a request parked awaiting an async handler keeps the loop
    // alive so the deferred `res.end()` can still flush before exit.
    if active == 0 && has_in_flight_requests() {
        active = 1;
    }
    active
}

/// Drain pending requests + upgrades from every registered server,
/// dispatching to the user handler / `'upgrade'` listener on the
/// main thread. Called each tick by perry-stdlib's pump (gated on
/// `external-http-server-pump`). Returns the total count drained.
///
/// **Async-handler caveat**: pre-#604 `process_pending` blocked on a
/// `wait_for_promise(...)` synchronous spin so an `async (req, res) =>
/// { await x; res.end(...) }` handler had its returned Promise fully
/// settled before the next tick. With `listen()` now non-blocking,
/// blocking the pump on a per-handler basis would re-introduce the
/// same problem (the pump runs on the main TS thread, so a blocking
/// wait here would block subsequent timer ticks / other pending
/// resolutions). The current implementation drops that wait — the
/// handler's microtasks fire via the next iteration of the
/// codegen-emitted event loop, and the
/// `synthesize_default_response_if_needed` safety net catches the
/// case where the response oneshot hasn't fired by the time we drop
/// the per-request handles. **Follow-up**: track an in-flight
/// per-request set so the pump only frees the request handles after
/// the handler-returned Promise settles, allowing async handlers
/// that yield across multiple microtask cycles. The simple
/// `(req, res) => res.end(...)` shape that the load-bearing #604
/// fixture uses works without this — the response oneshot fires
/// synchronously from inside `js_node_http_res_end`.
#[no_mangle]
pub extern "C" fn js_node_http_server_process_pending() -> i32 {
    let mut count = 0i32;

    // The runtime's outer-pump tick-begin hook promotes prior-tick handle
    // quarantine before any extension callback runs. Do not drain it again
    // here: nested pumps and later extensions still belong to the same tick.
    // Settle `Bun.serve` fetch/error promises before the ordinary in-flight
    // reaper observes their ServerResponse handles.
    count += crate::server::bun_server::process_pending_promises();

    // #4728 — finalize any async-handler requests that have flushed their
    // response since the last tick (or timed out) before draining new ones.
    reap_in_flight_requests();

    // #4905 — fire `'connection'` listeners for connections accepted since
    // the last tick, before their requests are dispatched (Node fires
    // `'connection'` ahead of `'request'`).
    let connection_events: Vec<(i64, i64)> = PENDING_CONNECTION_EVENTS
        .lock()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    for (server_handle, socket_handle) in connection_events {
        // The handle may back an HttpServer or an HttpsServer (whose
        // accept loop pushes here too since #4971) — probe both.
        let listeners = get_handle_mut::<HttpServer>(server_handle)
            .map(|server| take_server_event_listeners(server, "connection"))
            .or_else(|| {
                get_handle_mut::<crate::server::https_server::HttpsServer>(server_handle)
                    .map(|server| take_server_event_listeners(&mut server.base, "connection"))
            })
            .unwrap_or_default();
        if listeners.is_empty() {
            continue;
        }
        let this_val = handle_to_pointer_f64(server_handle);
        let socket_val = handle_to_pointer_f64(socket_handle);
        with_implicit_this(this_val, || {
            crate::server::request::emit_one_arg_to_listeners(&listeners, socket_val)
        });
        count += 1;
    }

    // Fire `'close'` on every connection
    // socket whose TCP connection fully closed since the last tick — before
    // this tick's server `'close'` callback below (`drain_deferred_close_for`),
    // matching Node's ordering.
    for socket_handle in crate::server::turnloop_serve::take_closed_sockets() {
        crate::server::request::close_incoming_message(socket_handle);
        count += 1;
    }

    // P5: a connection that died before its response completed raises Node's
    // `'aborted'` on the request; the sink queued it because it may not run JS.
    count += turnloop_listen::drain_aborted_requests();

    // Snapshot handle ids first so we can mutate handle state
    // (drain channels, free per-request handles) without the
    // DashMap iterator dangling.
    let mut http_handles: Vec<i64> = Vec::new();
    perry_ffi::iter_handle_ids_of::<HttpServer, _>(|id| http_handles.push(id));
    for h in http_handles {
        // #4903 — fire the deferred `'listening'` emit + listen callbacks
        // before draining requests: the listen callback is usually what
        // kicks off the client request in the first place.
        count += drain_deferred_listen_for::<HttpServer, _>(h, |s| s);
        count += drain_deferred_close_for::<HttpServer, _>(h, |s| s);
        // Drain upgrades first so they don't get starved by a busy
        // request stream.
        count += drain_upgrades(h);
        while let Some(p) = try_recv_pending_nonblocking(h) {
            process_pending(p);
            count += 1;
        }
    }

    let mut https_handles: Vec<i64> = Vec::new();
    perry_ffi::iter_handle_ids_of::<crate::server::https_server::HttpsServer, _>(|id| {
        https_handles.push(id)
    });
    for h in https_handles {
        count += drain_deferred_listen_for::<crate::server::https_server::HttpsServer, _>(h, |s| {
            &mut s.base
        });
        count += drain_deferred_close_for::<crate::server::https_server::HttpsServer, _>(h, |s| {
            &mut s.base
        });
        // An HTTPS server's upgrades were never drained at all. It did not show
        // until the turnloop path started answering an attached
        // `WebSocketServer` on an `https.createServer()`: the `101` went out
        // over TLS and the client opened, and then `wss.on('connection')` never
        // fired, because the record queued against the HTTPS server's handle
        // had no reader. The queue is keyed by server handle and is transport-
        // agnostic, so this is the same drain as the HTTP one.
        count += drain_upgrades(h);
        while let Some(p) = crate::server::https_server::try_recv_pending_https_nonblocking(h) {
            crate::server::https_server::process_pending_https(p);
            count += 1;
        }
    }

    let mut h2_handles: Vec<i64> = Vec::new();
    perry_ffi::iter_handle_ids_of::<crate::server::http2_server::Http2SecureServer, _>(|id| {
        h2_handles.push(id)
    });
    for h in h2_handles {
        count += drain_deferred_listen_for::<crate::server::http2_server::Http2SecureServer, _>(
            h,
            |s| &mut s.base,
        );
        count +=
            drain_deferred_close_for::<crate::server::http2_server::Http2SecureServer, _>(h, |s| {
                &mut s.base
            });
        count += crate::server::http2_server::process_pending_h2_events();
        while let Some(p) = crate::server::http2_server::try_recv_pending_h2_nonblocking(h) {
            crate::server::http2_server::process_pending_h2(p);
            count += 1;
            count += crate::server::http2_server::process_pending_h2_events();
        }
    }
    count += crate::server::http2_server::process_pending_h2_events();

    // #5010 — drain perry-ext-net's own pending-event queue. A raw
    // `'upgrade'` (#4973) hands the listener a real `net.Socket` adopted into
    // perry-ext-net (`adopt_upgraded_tcp_stream`); when user code destroys it,
    // the socket task queues a `Close` event in perry-ext-net's queue. For an
    // http-only program perry-stdlib runs with its OWN bundled net (so its
    // `external-net-pump` arm is OFF and never touches ext-net's queue), and
    // the perry-ext-net aux pump proved unreliable across workspace link
    // layouts. The http-server pump, by contrast, runs every tick
    // (external-http-server-pump) and directly depends on perry-ext-net, so
    // draining here — through the UNIQUE `js_ext_net_drain_pending` symbol
    // (no stdlib twin) — reliably empties that queue so the destroyed upgrade
    // socket stops pinning the event loop. Cheap (one mutex peek) when empty.
    count += unsafe { perry_ext_net::js_ext_net_drain_pending() };

    count
}

/// Deliver every pending `'upgrade'` for one server handle.
///
/// Shared by the HTTP and HTTPS drains: `TURNLOOP_UPGRADES` is keyed by server
/// handle and knows nothing about which of the two queued the record.
fn drain_upgrades(server_handle: i64) -> i32 {
    let mut count = 0;
    while let Some(up) = try_recv_upgrade(server_handle) {
        // #6710 — the upgrade path bypasses `process_pending`, but its request
        // handle and the adopted socket / WebSocket handles are recycled from
        // the same freelist. Clear their per-handle JS side tables here, on the
        // main thread, before any upgrade listener sees them (no-op for a zero
        // handle).
        unsafe {
            js_handle_clear_side_tables(up.request_handle);
            js_handle_clear_side_tables(up.raw_socket_id);
            js_handle_clear_side_tables(up.ws_id);
        }
        if up.raw_socket_id != 0 {
            // #4973 raw path: make sure the adopted net.Socket's dispatch
            // extensions + GC scanner are registered on the main thread before
            // user code touches the socket.
            perry_ext_net::ensure_adopted_socket_dispatch();
            if !crate::server::upgrade::fire_upgrade_listeners(
                up.server_handle,
                up.request_handle,
                up.raw_socket_id,
                up.head,
            ) {
                // Listeners may disappear between connection admission and
                // delivery. Nobody owns this raw upgrade in that case.
                perry_ext_net::js_ext_net_destroy_socket(up.raw_socket_id);
                perry_ffi::drop_handle(up.request_handle);
            }
        } else {
            perry_ext_ws::accept_attached_connection(
                up.server_handle,
                handle_to_pointer_f64(up.request_handle),
                up.ws_id,
            );
            crate::server::upgrade::fire_upgrade_listeners(
                up.server_handle,
                up.request_handle,
                up.ws_id,
                Vec::new(),
            );
        }
        count += 1;
    }
    count
}

fn try_recv_upgrade(server_handle: i64) -> Option<HttpPendingUpgrade> {
    let mut q = TURNLOOP_UPGRADES.lock().ok()?;
    let index = q.iter().position(|p| p.server_handle == server_handle)?;
    q.remove(index)
}

/// Non-blocking try_recv. Unlike the pre-#604 `try_recv_pending` which
/// spun for up to 10ms waiting for a message, this returns
/// immediately so the pump can move on to the next server / next tick.
/// The codegen-emitted main loop's `js_wait_for_event` provides the
/// blocking wait at the outer level via condvar, so we don't need to
/// spin here.
pub(crate) fn try_recv_pending_nonblocking(server_handle: i64) -> Option<HttpPendingRequest> {
    // P5: a turnloop server decodes on this thread, so its requests are in a
    // plain queue — no channel, no cross-thread notify.
    crate::server::turnloop_serve::take_pending(server_handle)
}

/// Dispatch one pending request — fire `'request'` listeners, then
/// the main handler, then await any returned Promise. The handler is
/// expected to call `res.end(...)` itself; the response oneshot
/// fires from inside `js_node_http_res_end`.
fn process_pending(pending: HttpPendingRequest) {
    if crate::server::bun_server::is_bun_server(pending.server_handle) {
        crate::server::bun_server::process_request(pending);
        return;
    }
    let incoming_async_id = unsafe {
        crate::js_async_hooks_provider_init(
            b"HTTPINCOMINGMESSAGE".as_ptr(),
            b"HTTPINCOMINGMESSAGE".len(),
        )
    };
    if incoming_async_id != 0 {
        unsafe { crate::js_async_hooks_provider_enter(incoming_async_id) };
    }
    let req_f64 = handle_to_pointer_f64(pending.request_handle);
    let res_f64 = handle_to_pointer_f64(pending.response_handle);

    // #6710: this handler is about to run on a possibly-RECYCLED handle id —
    // perry-ffi's bounded freelist hands a freed `IncomingMessage` /
    // `ServerResponse` id back out to a later `register_handle`. The per-handle
    // JS-property side tables (`HANDLE_EXPANDO_PROPS` string props +
    // `SYMBOL_PROPERTIES`/attrs/accessors symbol props) are keyed by that id and
    // are NOT cleared on recycle, so without this a fresh request would inherit
    // the previous request's arbitrary own props — e.g. Next.js stores
    // `isRSCRequest` / `NextInternalRequestMeta` on `req` — crossing per-request
    // state across concurrent requests and wedging the App Router render
    // pipeline. Clear them here, on the MAIN thread that owns the thread-local
    // expando table, before any `'request'` listener or the handler observes
    // `req`/`res`. (The Rust `IncomingMessage`/`ServerResponse` structs are
    // already fresh per request via `IncomingMessage::new`; only these
    // id-keyed JS side tables leak.)
    unsafe {
        js_handle_clear_side_tables(pending.request_handle);
        js_handle_clear_side_tables(pending.response_handle);
    }

    // Fire `'request'` listeners (Node's `server.on('request', ...)`).
    // Node's emitter invokes them with `this` bound to the server, so the
    // `function (req, res) { this.address().port }` handler idiom works
    // (#4903). Bind for the synchronous call only — microtasks run outside.
    let server_this = handle_to_pointer_f64(pending.server_handle);

    // #5080 — an `Expect: 100-continue` request with a `'checkContinue'`
    // listener fires that listener *instead of* `'request'` + the handler
    // (Node's dispatch). The listener calls `res.writeContinue()` and then
    // drives the exchange itself.
    // #8082: `pending` is a snapshot built at REQUEST time by the connection
    // layer and parked in the pending queue until this tick — the handler and
    // listener addresses inside it are copies no scanner rewrites, so any
    // moving collection between arrival and dispatch leaves them stale (the
    // forced gate faulted on them at the microtask-pump safepoint minors).
    // Re-read them from the server handle, whose side tables the registered
    // scanner DOES rewrite; the routing decision (`is_check_continue`) keeps
    // the arrival-time snapshot semantics. Then root the refreshed values,
    // because each callback below can itself run a moving collection.
    // (`req_f64`/`res_f64`/`server_this` are small handle ids — no move.)
    let (fresh_request_listeners, fresh_check_continue_listeners, fresh_handler) =
        match get_handle_mut::<HttpServer>(pending.server_handle) {
            Some(server) if pending.is_check_continue => (
                Vec::new(),
                take_server_event_listeners(server, "checkContinue"),
                server.handler,
            ),
            Some(server) => (
                take_server_event_listeners(server, "request"),
                Vec::new(),
                server.handler,
            ),
            // Server gone: nothing safe to dispatch to.
            None => (Vec::new(), Vec::new(), 0),
        };
    let scope = perry_ffi::TransientRootScope::enter();
    let check_continue_rooted = scope.root_addrs(&fresh_check_continue_listeners);
    let request_rooted = scope.root_addrs(&fresh_request_listeners);
    let handler_rooted = scope.root_addr(fresh_handler);

    if pending.is_check_continue {
        for cb in &check_continue_rooted {
            let addr = cb.get();
            if addr == 0 {
                continue;
            }
            unsafe {
                let raw = addr as *const RawClosureHeader;
                let closure = JsClosure::from_raw(raw);
                if !closure.is_null() {
                    with_implicit_this(server_this, || {
                        let _ = closure.call2(req_f64, res_f64);
                    });
                }
                js_promise_run_microtasks();
            }
        }
        finalize_or_park_request(&pending);
        if incoming_async_id != 0 {
            unsafe {
                crate::js_async_hooks_provider_leave(incoming_async_id);
                crate::js_async_hooks_provider_destroy(incoming_async_id);
            }
        }
        return;
    }

    // The createServer handler is the first registered request listener.
    // Per the issue #604 architectural change documented
    // on `js_node_http_server_process_pending`, we no longer
    // synchronously block on the handler's returned Promise — that
    // would re-introduce the listen()-blocks-main-thread problem at
    // a per-request granularity. The handler is expected to call
    // `res.end(...)` itself; subsequent microtasks fire via the next
    // tick of the codegen-emitted main loop. The
    // `synthesize_default_response_if_needed` safety net below
    // catches the case where neither path completed in time.
    if handler_rooted.get() != 0 {
        unsafe {
            let raw = handler_rooted.get() as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                // `createServer(handler)` registers `handler` as a
                // `'request'` listener — same `this` = server binding.
                with_implicit_this(server_this, || {
                    let _ = closure.call2(req_f64, res_f64);
                });
            }
            js_promise_run_microtasks();
        }
    }
    for cb in &request_rooted {
        let addr = cb.get();
        if addr == 0 {
            continue;
        }
        unsafe {
            let raw = addr as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                with_implicit_this(server_this, || {
                    let _ = closure.call2(req_f64, res_f64);
                });
            }
            js_promise_run_microtasks();
        }
    }

    // #4728 — if the handler already finished the response (the common
    // synchronous `res.end(...)` shape, or an async handler whose
    // microtasks all settled within this tick), finalize now. Otherwise
    // it launched async work — an outbound `fetch()`, a `setTimeout`, an
    // `await` chain — that will call `res.end()` on a later event-loop
    // tick. Synthesizing a default response and freeing the handles here
    // would race that work: the real response is dropped and the client
    // sees an empty (or no) reply. Park the request for the reaper, which
    // finalizes it once `res.end()` flushes the real response (or the
    // grace deadline elapses for a handler that never responds).
    finalize_or_park_request(&pending);
    if incoming_async_id != 0 {
        unsafe {
            crate::js_async_hooks_provider_leave(incoming_async_id);
            crate::js_async_hooks_provider_destroy(incoming_async_id);
        }
    }
}

/// If the handler didn't call `res.end()`, finish the response
/// transparently with whatever buffer / status was set so the connection
/// does not hang waiting for it.
pub(crate) fn synthesize_default_response_if_needed(response_handle: i64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(response_handle) {
        if !sr.writable_ended {
            sr.writable_ended = true;
            sr.headers_sent = true;
            sr.writable_finished = true;
            // P5 streaming: the head is on the wire; close the body framing.
            if sr.turnloop_streaming {
                let (conn, seq) = sr.turnloop.expect("streaming implies a turnloop target");
                let trailers = sr.snapshot_trailers();
                sr.needs_drain = false;
                crate::server::turnloop_serve::finish_body(conn, seq, &trailers);
                return;
            }
            let body = std::mem::take(&mut sr.buffered_body);
            // `snapshot_headers` expands array-valued headers (e.g.
            // Set-Cookie) into one entry per element so they emit a separate
            // wire line each (#4826).
            let mut headers = sr.snapshot_headers();
            let auto_content_length = !sr.headers.contains_key("content-length")
                && !sr.headers.contains_key("transfer-encoding")
                && sr.trailers.is_empty();
            if auto_content_length {
                headers.push(("Content-Length".to_string(), body.len().to_string()));
            }
            let shape = ResponseShape {
                status: sr.status_code,
                status_message: sr.status_message.clone(),
                headers,
                trailers: Vec::new(),
                body,
                auto_content_length,
            };
            if let Some((conn, seq)) = sr.turnloop {
                crate::server::turnloop_serve::send_response(conn, seq, shape);
            }
        }
    }
}

#[allow(dead_code)]
fn _force_link_helpers(v: f64) -> Option<String> {
    jsvalue_to_owned_string(v)
}

#[allow(dead_code)]
fn _force_promise_link(p: *mut Promise) -> i32 {
    unsafe { js_promise_state(p) }
}

#[allow(dead_code)]
fn _force_tag_link() -> u64 {
    TAG_NULL | (POINTER_TAG & PTR_MASK)
}

#[cfg(test)]
#[path = "server/upgrade_tests.rs"]
mod upgrade_tests;
