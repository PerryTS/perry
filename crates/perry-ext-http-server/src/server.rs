//! `HttpServer` — backing `http.createServer(handler)`. Binds via
//! hyper's HTTP/1.1 service path, pushes `(req, res)` to the main
//! thread, runs the user handler synchronously (awaiting any
//! returned Promise), and flushes the buffered response back through
//! the per-request oneshot channel.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};

use lazy_static::lazy_static;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, iter_handles_of, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::ensure_gc_scanner_registered;
use crate::request::{
    alloc_incoming_message, close_incoming_message, emit_no_arg_to_listeners,
    handle_to_pointer_f64, with_implicit_this, IncomingMessage,
};
use crate::response::{
    alloc_server_response_for_request, HyperResponseShape, ResponseBody, ServerResponse,
};
use crate::types::{
    extract_host, extract_port, js_promise_run_microtasks, js_promise_state, js_value_is_closure,
    jsvalue_to_owned_string, read_string_header, Promise, POINTER_TAG, PTR_MASK, TAG_NULL,
    TAG_UNDEFINED,
};

/// Backing struct for an `http.Server` JS-side handle.
pub struct HttpServer {
    /// User's `(req, res) => ...` handler. Stored as raw `i64`; the
    /// GC root scanner pins it across malloc-triggered sweeps.
    pub handler: i64,
    /// Server-level event listeners (`'request'`, `'connection'`,
    /// `'close'`, `'listening'`, `'error'`, `'upgrade'`).
    pub listeners: HashMap<String, Vec<i64>>,
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
    /// Sent by `.close()` to wake the accept loop.
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    /// Channel main thread drains in the event loop. Hyper service
    /// fns push pending requests through this; main thread invokes
    /// the handler closure and flushes the response.
    pub request_rx: Option<mpsc::Receiver<HttpPendingRequest>>,
    /// Phase 4 — upgrade events queued from hyper service fns once
    /// the WebSocket handshake completes. Drained alongside
    /// `request_rx` in `event_loop`.
    pub upgrade_rx: Option<mpsc::Receiver<HttpPendingUpgrade>>,
    /// Issue #2210 — Node 18.4+ timeout knobs surfaced as both
    /// `createServer(handler, options)` and `server.<name>` property
    /// setters. Phase 1: store the values and read them back; the
    /// hyper accept-loop wiring (Phase 2) is the follow-up tracked in
    /// the same ticket.
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
    /// no such timer (the hyper accept loop owns connection lifecycle),
    /// so we track just the flag the `_http_server` introspection key
    /// exposes.
    pub connections_checking_interval_destroyed: bool,
    /// #5011 — mirrors Node's `server.unref()` / `server.ref()`. When
    /// `unref()`ed, a listening server no longer keeps the event loop
    /// alive, so the process can exit even while it's still bound. Starts
    /// `true` (refed), matching Node where a fresh server holds the loop
    /// open once it's listening.
    pub refed: bool,
}

impl HttpServer {
    /// Build a new `HttpServer` with all Node 18.4+ timeout defaults.
    /// Keeps the field list off the `register_handle` call sites so a
    /// future field addition doesn't require updating every constructor
    /// (https / http2 / test fixtures).
    pub fn with_handler(handler: i64) -> Self {
        Self {
            handler,
            listeners: HashMap::new(),
            bound_port: 0,
            bound_host: String::new(),
            listening: false,
            pending_listening_emit: false,
            deferred_listen_cbs: Vec::new(),
            shutdown_tx: None,
            request_rx: None,
            upgrade_rx: None,
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
        }
    }
}

/// Pending request from the hyper service fn to the main thread.
pub struct HttpPendingRequest {
    pub server_handle: i64,
    pub request_handle: i64,
    pub response_handle: i64,
    pub skip_default_response: bool,
    pub h2_stream_handle: i64,
    pub h2_stream_headers: Vec<(String, String)>,
    /// `'request'` listeners snapshotted at request time so the
    /// dispatch loop doesn't need to re-borrow the server handle.
    pub request_listeners: Vec<i64>,
    pub handler: i64,
    /// #5080 — `'checkContinue'` listeners snapshotted at request time.
    /// When `is_check_continue` is set these fire *instead of* the
    /// `'request'` listeners + handler (Node dispatches an
    /// `Expect: 100-continue` request to `'checkContinue'` when a listener
    /// exists, and only emits `'request'` otherwise).
    pub check_continue_listeners: Vec<i64>,
    /// #5080 — route this request to `'checkContinue'` rather than the
    /// normal `'request'` path.
    pub is_check_continue: bool,
}

/// Phase 4 — pending WebSocket upgrade ready to fire `'upgrade'`
/// listeners. Sent by the hyper service fn after the underlying
/// `hyper::upgrade::on` future resolves and the upgraded stream has
/// been registered with `perry_ext_ws::register_external_ws_stream`.
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

// ============================================================================
// #4905 — per-connection tracking for closeAllConnections/closeIdleConnections
// ============================================================================

/// Live HTTP/1.1 connection tracked so `server.closeAllConnections()` /
/// `server.closeIdleConnections()` can reach into the per-connection
/// tokio task. `busy` counts in-flight requests on the connection (0
/// between keep-alive requests); `read_active` flags request bytes
/// received since the last dispatched request (a half-sent request head
/// is "currently sending a request" in Node's idleness terms even
/// though nothing has reached the service yet — #4971); `close` wakes
/// the connection task's `select!`, which drops the hyper connection
/// and closes the socket. Shared with the HTTPS accept loop in
/// `https_server.rs`.
pub(crate) struct TrackedConnection {
    pub(crate) server_handle: i64,
    pub(crate) close: Arc<tokio::sync::Notify>,
    pub(crate) busy: Arc<AtomicUsize>,
    pub(crate) read_active: Arc<AtomicBool>,
}

lazy_static! {
    pub(crate) static ref CONNECTIONS: Mutex<HashMap<u64, TrackedConnection>> =
        Mutex::new(HashMap::new());
}

pub(crate) static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// AsyncRead/AsyncWrite passthrough that flips `saw_bytes` on every
/// read that produces data. Wrapped around the server-side stream (the
/// decrypted one, for HTTPS — handshake traffic must not count) so
/// idleness can see "client started sending a request whose head hasn't
/// parsed yet": hyper only invokes the service (and bumps `busy`) once
/// a complete head arrives. The flag resets at service entry; while a
/// request is in flight `busy > 0` covers activity, and a quiet
/// keep-alive socket after the response reads as idle again. #4971.
pub(crate) struct ReadActivity<S> {
    inner: S,
    saw_bytes: Arc<AtomicBool>,
}

impl<S> ReadActivity<S> {
    pub(crate) fn new(inner: S, saw_bytes: Arc<AtomicBool>) -> Self {
        Self { inner, saw_bytes }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ReadActivity<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        if matches!(poll, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            this.saw_bytes.store(true, Ordering::SeqCst);
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for ReadActivity<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Server handles whose accept loop saw a new connection since the last
/// pump tick. Drained by `js_node_http_server_process_pending` to fire
/// `'connection'` listeners on the main thread (#4905). Node passes the
/// socket as the listener argument; we don't model a net.Socket for
/// hyper connections yet, so listeners fire with no args — enough for
/// the canonical connection-counting idiom.
pub(crate) static PENDING_CONNECTION_EVENTS: Mutex<Vec<i64>> = Mutex::new(Vec::new());

/// Signal tracked connections of `server_handle` to close. With
/// `only_idle`, connections currently processing a request — or mid-way
/// through sending one (`read_active`, #4971) — are left alone (Node's
/// `closeIdleConnections` semantics; `server.close()` also closes idle
/// keep-alive sockets since Node 19).
pub(crate) fn signal_connections_close(server_handle: i64, only_idle: bool) {
    let conns = CONNECTIONS.lock().unwrap();
    for entry in conns.values() {
        if entry.server_handle == server_handle
            && (!only_idle
                || (entry.busy.load(Ordering::SeqCst) == 0
                    && !entry.read_active.load(Ordering::SeqCst)))
        {
            entry.close.notify_one();
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
        server.request_timeout = v;
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
server_setter!(js_node_http_server_set_request_timeout, request_timeout);
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

/// Serve one accepted (or fd-passed, #4962) connection: register it for
/// `closeAll/IdleConnections`, peek for raw `'upgrade'`, then drive hyper.
/// Factored out of the accept loop so SCHED_RR workers (which receive fds from
/// the primary instead of calling `accept()`) share the exact same path.
fn serve_http_connection(
    server_handle: i64,
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    request_tx_for_spawn: Arc<mpsc::Sender<HttpPendingRequest>>,
    upgrade_tx_for_spawn: Arc<mpsc::Sender<HttpPendingUpgrade>>,
) {
    let request_tx = request_tx_for_spawn.clone();
    let upgrade_tx = upgrade_tx_for_spawn.clone();
    // #4905 — register the connection so
    // closeAllConnections/closeIdleConnections can
    // reach this task from the main thread.
    let conn_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::SeqCst);
    let busy = Arc::new(AtomicUsize::new(0));
    let read_active = Arc::new(AtomicBool::new(false));
    let close = Arc::new(tokio::sync::Notify::new());
    let read_active_for_io = read_active.clone();
    let upgrade_tx_peek = upgrade_tx_for_spawn.clone();
    CONNECTIONS.lock().unwrap().insert(
        conn_id,
        TrackedConnection {
            server_handle,
            close: close.clone(),
            busy: busy.clone(),
            read_active: read_active.clone(),
        },
    );
    if let Ok(mut q) = PENDING_CONNECTION_EVENTS.lock() {
        q.push(server_handle);
    }
    tokio::spawn(async move {
        // #4973 — when `'upgrade'` listeners exist, peek the
        // request head before hyper writes anything: a keyless
        // Upgrade request must reach JS as a raw net.Socket
        // with NO response on the wire (Node semantics). Other
        // connections replay the peeked bytes to hyper.
        let has_upgrade_listeners = get_handle::<HttpServer>(server_handle)
            .map(|s| {
                s.listeners
                    .get("upgrade")
                    .map(|v| !v.is_empty())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let stream = if has_upgrade_listeners {
            match crate::raw_upgrade::peek_and_maybe_dispatch_raw_upgrade(
                server_handle,
                peer,
                stream,
                &upgrade_tx_peek,
            )
            .await
            {
                crate::raw_upgrade::PeekResult::Handled => {
                    CONNECTIONS.lock().unwrap().remove(&conn_id);
                    return;
                }
                crate::raw_upgrade::PeekResult::Passthrough(s) => s,
            }
        } else {
            crate::raw_upgrade::PrefixedStream::empty(stream)
        };
        let io = TokioIo::new(ReadActivity::new(stream, read_active_for_io));
        let service = service_fn(move |req: Request<Incoming>| {
            let request_tx = request_tx.clone();
            let upgrade_tx = upgrade_tx.clone();
            let busy = busy.clone();
            let read_active = read_active.clone();
            async move {
                busy.fetch_add(1, Ordering::SeqCst);
                // The pending head is now an in-flight
                // request; `busy` covers activity until
                // the response ships (#4971).
                read_active.store(false, Ordering::SeqCst);
                let res = handle_request(server_handle, peer, req, request_tx, upgrade_tx).await;
                busy.fetch_sub(1, Ordering::SeqCst);
                res
            }
        });
        let conn = http1::Builder::new()
            .serve_connection(io, service)
            .with_upgrades();
        tokio::pin!(conn);
        tokio::select! {
            result = &mut conn => {
                // Common when client closes mid-request — silenced.
                let _ = result;
            }
            _ = close.notified() => {
                // closeAllConnections / closeIdleConnections:
                // dropping the pinned connection closes the
                // socket immediately (in-flight request gets
                // a reset, matching Node's socket.destroy()).
            }
        }
        CONNECTIONS.lock().unwrap().remove(&conn_id);
    });
}

/// SCHED_RR worker accept loop (#4962): pull connection fds the primary passes
/// over the IPC socketpair and serve each like a locally-accepted connection.
/// `recv_fd` blocks off-runtime, so a bridge thread forwards fds into a tokio
/// channel the async loop selects on (alongside `server.close()`'s shutdown).
#[cfg(unix)]
fn spawn_rr_inject_loop(
    server_handle: i64,
    key_id: u32,
    mut shutdown_rx: oneshot::Receiver<()>,
    request_tx_for_spawn: Arc<mpsc::Sender<HttpPendingRequest>>,
    upgrade_tx_for_spawn: Arc<mpsc::Sender<HttpPendingUpgrade>>,
) {
    use std::os::unix::io::{FromRawFd, RawFd};

    let (fd_tx, mut fd_rx) = mpsc::channel::<RawFd>(256);
    // Off-runtime bridge: `recv_fd` blocks until the primary passes an fd (or
    // the channel closes → fd < 0). The thread parks on `recv_fd` for the
    // server's lifetime; it ends when the IPC channel closes.
    std::thread::spawn(move || loop {
        let fd = crate::cluster_bind::recv_fd(key_id);
        if fd < 0 || fd_tx.blocking_send(fd).is_err() {
            break;
        }
    });

    perry_ffi::spawn_blocking_with_reactor(move || {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    maybe_fd = fd_rx.recv() => {
                        let Some(fd) = maybe_fd else { break };
                        let std_stream = unsafe { std::net::TcpStream::from_raw_fd(fd) };
                        if std_stream.set_nonblocking(true).is_err() {
                            continue;
                        }
                        let peer = std_stream
                            .peer_addr()
                            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
                        match tokio::net::TcpStream::from_std(std_stream) {
                            Ok(stream) => serve_http_connection(
                                server_handle,
                                stream,
                                peer,
                                request_tx_for_spawn.clone(),
                                upgrade_tx_for_spawn.clone(),
                            ),
                            Err(e) => eprintln!("[node:http] rr adopt failed: {}", e),
                        }
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });
    });
}

/// `server.listen(port?, host?, backlog?, cb?)` — bind + start accepting.
/// Returns immediately after spawning the accept loop on the tokio runtime
/// (non-blocking since #604); requests are drained from the main thread by
/// `js_node_http_server_process_pending`.
///
/// `args_array` is a raw `*const ArrayHeader` carrying every user-supplied
/// `listen()` argument (codegen packs them via the `NA_VARARGS` arg kind).
/// `parse_listen_args` resolves Node's variadic overloads by value type — a
/// bare numeric/options/path first arg, an optional standalone host string,
/// and the (single) function callback wherever it lands. Issue #2041.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_listen(server_handle: i64, args_array: i64) -> i64 {
    // Returns `server_handle` so `createServer(...).listen(...).on(...)` chains
    // correctly. Pre-#2129 this was `-> ()` and chained sites broke at runtime
    // with `undefined.on is not a function`.
    let parsed = crate::types::parse_listen_args(args_array);
    let opts_f64 = parsed.opts;
    let port = extract_port(opts_f64, 3000);
    let host = parsed
        .host
        .unwrap_or_else(|| extract_host(opts_f64, "0.0.0.0"));
    let callback = parsed.callback;

    let (request_tx, request_rx) = mpsc::channel::<HttpPendingRequest>(1024);
    let (upgrade_tx, upgrade_rx) = mpsc::channel::<HttpPendingUpgrade>(256);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    // #2132 — bind synchronously here so `server.address().port` returns
    // the OS-assigned ephemeral port when the user passed `port: 0`. The
    // pre-#2132 path wrote the *requested* port (0) into `bound_port`,
    // spawned an async task to do `TcpListener::bind(...).await`, and
    // fired the `listen(port, cb)` callback before the bind had actually
    // happened — so `server.address().port` inside the callback was 0
    // and downstream `http.get({port: 0, ...})` calls couldn't connect.
    //
    // `std::net::TcpListener::bind` is synchronous; we then hand the
    // standard listener to `tokio::net::TcpListener::from_std` for the
    // async accept loop. `set_nonblocking(true)` is required for
    // `from_std` to drive `.accept().await` correctly.
    // #4962 — cluster workers coordinate the bind with the primary. Under
    // SCHED_RR the primary owns the socket and passes accepted fds to this
    // worker (no local bind); otherwise (SCHED_NONE, or off-cluster) the
    // worker binds the primary-resolved port itself with SO_REUSEPORT so
    // `listen(0)` still shares one ephemeral port (#4914).
    let address_type: i32 = if host.contains(':') { 6 } else { 4 };
    let is_worker = crate::cluster_bind::is_cluster_worker();
    let rr = is_worker && crate::cluster_bind::worker_sched_is_rr();
    let resolved = if is_worker {
        crate::cluster_bind::worker_query_listen(&host, port as i32, address_type, rr)
    } else {
        None
    };
    #[cfg(unix)]
    let rr_inject = rr && resolved.is_some();
    #[cfg(not(unix))]
    let rr_inject = false;

    let request_tx = Arc::new(request_tx);
    let upgrade_tx = Arc::new(upgrade_tx);
    let request_tx_for_spawn = request_tx.clone();
    let upgrade_tx_for_spawn = upgrade_tx.clone();

    if rr_inject {
        #[cfg(unix)]
        {
            let actual_port = resolved.unwrap();
            crate::cluster_bind::notify_listening(&host, actual_port);
            if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
                s.bound_port = actual_port;
                s.bound_host = host.clone();
                s.listening = true;
                s.shutdown_tx = Some(shutdown_tx);
                s.request_rx = Some(request_rx);
                s.upgrade_rx = Some(upgrade_rx);
            } else {
                return server_handle;
            }
            let key_id = crate::cluster_bind::compute_key_id(&host, actual_port, address_type);
            spawn_rr_inject_loop(
                server_handle,
                key_id,
                shutdown_rx,
                request_tx_for_spawn,
                upgrade_tx_for_spawn,
            );
        }
    } else {
        // The worker binds the primary-resolved port (shared listen(0)); a
        // non-cluster server binds the requested port directly.
        let bind_port = resolved.map(|p| p as i32).unwrap_or(port as i32);
        let bind_str = format!("{}:{}", host, bind_port);
        let addr: SocketAddr = match bind_str.parse() {
            Ok(a) => a,
            Err(_) => SocketAddr::from(([0, 0, 0, 0], bind_port as u16)),
        };
        // #4914 — cluster workers bind with SO_REUSEPORT so N workers share
        // the port; `bind_listener` falls through to a plain bind otherwise.
        let std_listener = match crate::cluster_bind::bind_listener(addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[node:http] bind {}:{} failed: {}", host, bind_port, e);
                return server_handle;
            }
        };
        let actual_port = std_listener.local_addr().map(|a| a.port()).unwrap_or(port);
        if let Err(e) = std_listener.set_nonblocking(true) {
            eprintln!("[node:http] set_nonblocking failed: {}", e);
            return server_handle;
        }
        crate::cluster_bind::notify_listening(&host, actual_port);

        if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
            s.bound_port = actual_port;
            s.bound_host = host.clone();
            s.listening = true;
            s.shutdown_tx = Some(shutdown_tx);
            s.request_rx = Some(request_rx);
            s.upgrade_rx = Some(upgrade_rx);
        } else {
            return server_handle;
        }

        // Hyper workers queue Rust request handles; JS callbacks run later in
        // `js_node_http_server_process_pending` on the main thread. Keeping the
        // whole listener lifetime in a GC-unsafe zone would disable `gc()` for
        // long-running servers without adding safety.

        // The closure passed to `spawn_blocking_with_reactor` runs INSIDE
        // a tokio worker task (perry-stdlib's shim wraps it in
        // `runtime().spawn(async { invoke(...) })`), so calling
        // `Handle::current().block_on(fut)` would panic with
        // "Cannot start a runtime from within a runtime". Spawn the
        // accept loop as a separate async task on the existing runtime
        // and let the closure return immediately.
        perry_ffi::spawn_blocking_with_reactor(move || {
            tokio::spawn(async move {
                let listener = match TcpListener::from_std(std_listener) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("[node:http] tokio adopt failed: {}", e);
                        return;
                    }
                };
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            match accepted {
                                Ok((stream, peer)) => {
                                    serve_http_connection(
                                        server_handle,
                                        stream,
                                        peer,
                                        request_tx_for_spawn.clone(),
                                        upgrade_tx_for_spawn.clone(),
                                    );
                                }
                                Err(e) => eprintln!("[node:http] accept error: {}", e),
                            }
                        }
                        _ = &mut shutdown_rx => {
                            break;
                        }
                    }
                }
            });
        });
    }

    // #4903 — queue the `'listening'` emit + the optional `cb` argument for
    // the main-thread pump instead of firing them synchronously. Node emits
    // `'listening'` on a later event-loop tick, after the current synchronous
    // script segment finishes; firing inline ran the callback before
    // `const server = http.createServer().listen(0, cb)` had assigned
    // `server`, so `server.address()` inside the callback threw
    // "Cannot read properties of undefined". The pump fires both with
    // `this` bound to the server (#2132), via `drain_deferred_listen_events`.
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        queue_deferred_listening_emit(s, callback);
    }

    // Closes #604 — `listen()` is now non-blocking. The accept loop is
    // already spawned on the tokio runtime above, and the new
    // `js_node_http_server_has_active` / `js_node_http_server_process_pending`
    // externs let perry-stdlib's main-thread pump drain pending requests
    // and upgrades each tick. Without this change, `listen()` blocked the
    // main TS thread inside `event_loop(...)` for the process lifetime,
    // so `await new Promise(r => server.listen(port, r))` never returned
    // → no code after `listen()` ever ran (e.g. axios.get + server.close).
    server_handle
}

/// `server.close(cb?)` — drop the shutdown channel, fire `'close'`.
/// The accept loop's `tokio::select!` picks up the channel close and
/// exits.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_close(server_handle: i64, callback: i64) {
    let close_listeners;
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        s.listening = false;
        s.connections_checking_interval_destroyed = true;
        s.shutdown_tx.take();
        close_listeners = s.listeners.get("close").cloned().unwrap_or_default();
    } else {
        close_listeners = Vec::new();
    }
    // Node 19+: `server.close()` destroys idle keep-alive connections
    // (active requests are allowed to finish) (#4905).
    signal_connections_close(server_handle, true);
    emit_no_arg_to_listeners(&close_listeners);
    if callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}

/// `server.closeAllConnections()` — destroy every tracked connection
/// of this server, including ones with an in-flight request (#4905).
#[no_mangle]
pub extern "C" fn js_node_http_server_close_all_connections(handle: i64) {
    signal_connections_close(handle, false);
    // Parked async requests on the destroyed connections can never flush
    // a response (the per-request oneshot receiver died with the
    // connection task) — drop them now so `has_in_flight_requests()`
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
        } else if let Some(event) = read_string_header(event_name_ptr as *mut _) {
            s.listeners.remove(&event);
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
        if let Some(cbs) = s.listeners.get_mut(&event) {
            if let Some(pos) = cbs.iter().rposition(|&c| c == callback) {
                cbs.remove(pos);
            }
        }
    }
    handle_to_pointer_f64(handle)
}

// ============================================================================
// Request dispatch — hyper service fn + main-thread event loop
// ============================================================================

async fn handle_request(
    server_handle: i64,
    peer: SocketAddr,
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<HttpPendingRequest>>,
    upgrade_tx: Arc<mpsc::Sender<HttpPendingUpgrade>>,
) -> Result<Response<ResponseBody>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let url = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };

    let mut headers_lower: HashMap<String, String> = HashMap::new();
    let mut raw_headers: Vec<(String, String)> = Vec::new();
    for (name, value) in req.headers() {
        if let Ok(v) = value.to_str() {
            headers_lower.insert(name.to_string().to_lowercase(), v.to_string());
            raw_headers.push((name.to_string(), v.to_string()));
        }
    }
    // #2132 — capture the bits needed to synthesize Node's default
    // `Connection` / `Keep-Alive` response headers before `req` (and
    // `headers_lower`) are consumed below.
    let http_version = req.version();
    let req_connection = headers_lower.get("connection").cloned();
    // #5080 — `Expect: 100-continue` routes to `'checkContinue'` when a
    // listener exists. hyper auto-sends the interim `100 Continue` once the
    // body below is polled, so the client's withheld body arrives before
    // `req.collect()` completes.
    let expects_continue = headers_lower
        .get("expect")
        .map(|v| v.to_ascii_lowercase().contains("100-continue"))
        .unwrap_or(false);

    // Phase 4 — WebSocket upgrade detection. If the request looks
    // like a WS upgrade, branch into the handshake path: build the
    // 101 Switching Protocols response synchronously and spawn a
    // task that awaits hyper's upgraded stream + completes the
    // tungstenite server handshake + registers the resulting
    // WebSocketStream with perry-ext-ws.
    //
    // #4973 gates: (a) Node dispatches an Upgrade request as a normal
    // `'request'` when the server has no `'upgrade'` listeners — the
    // unconditional branch used to hijack it into a bogus 101; (b) only a
    // real WebSocket handshake (`Sec-WebSocket-Key` present) belongs on the
    // tungstenite path — keyless Upgrade requests are served Node-style by
    // the raw peek path in raw_upgrade.rs and only reach hyper when no
    // listener was attached at accept time.
    if crate::upgrade::is_websocket_upgrade(&req) {
        let has_upgrade_listeners = get_handle::<HttpServer>(server_handle)
            .map(|s| {
                s.listeners
                    .get("upgrade")
                    .map(|v| !v.is_empty())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if has_upgrade_listeners && req.headers().contains_key("sec-websocket-key") {
            return handle_websocket_upgrade(
                server_handle,
                peer,
                req,
                method,
                url,
                headers_lower,
                raw_headers,
                upgrade_tx,
            )
            .await;
        }
    }

    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };

    let mut im = IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        body_bytes,
        peer.ip().to_string(),
        peer.port(),
    );
    // `req.httpVersion` reflects the wire version, not a constant — an
    // HTTP/1.0 request must read "1.0" (test-http-1.0 asserts all three
    // httpVersion fields).
    im.http_version = match http_version {
        hyper::Version::HTTP_10 => "1.0".to_string(),
        hyper::Version::HTTP_2 => "2.0".to_string(),
        _ => "1.1".to_string(),
    };
    let im_handle = alloc_incoming_message(im);

    let (response_tx, response_rx) = oneshot::channel::<HyperResponseShape>();
    let sr_handle = alloc_server_response_for_request(response_tx, im_handle);

    let (request_listeners, handler, keep_alive_timeout, check_continue_listeners) =
        match get_handle::<HttpServer>(server_handle) {
            Some(s) => (
                s.listeners.get("request").cloned().unwrap_or_default(),
                s.handler,
                s.keep_alive_timeout,
                s.listeners
                    .get("checkContinue")
                    .cloned()
                    .unwrap_or_default(),
            ),
            None => (Vec::new(), 0, 5_000.0, Vec::new()),
        };

    let is_check_continue = expects_continue && !check_continue_listeners.is_empty();

    let pending = HttpPendingRequest {
        server_handle,
        request_handle: im_handle,
        response_handle: sr_handle,
        skip_default_response: false,
        h2_stream_handle: 0,
        h2_stream_headers: Vec::new(),
        request_listeners,
        handler,
        check_continue_listeners,
        is_check_continue,
    };

    if request_tx.send(pending).await.is_err() {
        // Channel closed — return 503 directly.
        return Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from("Server unavailable")).boxed())
            .unwrap());
    }

    perry_ffi::notify_main_thread();

    match response_rx.await {
        Ok(mut shape) => {
            shape.apply_default_connection_headers(
                http_version,
                req_connection.as_deref(),
                keep_alive_timeout,
            );
            Ok(shape.into_hyper())
        }
        Err(_) => Ok(Response::builder()
            .status(500)
            .body(Full::new(Bytes::from("Handler error")).boxed())
            .unwrap()),
    }
}

/// Phase 4 — WebSocket upgrade dispatch.
///
/// Synchronously builds the 101 response (so hyper drives the
/// protocol switch) and spawns a tokio task that awaits the
/// upgraded stream + finishes the handshake server-side via
/// `tokio_tungstenite::WebSocketStream::from_raw_socket`. The
/// resulting WS stream is registered through perry-ext-ws and an
/// `HttpPendingUpgrade` is pushed to the main-thread upgrade
/// channel; the event-loop fires the user's `'upgrade'` listeners
/// with `(req, wsId, head)`.
async fn handle_websocket_upgrade(
    server_handle: i64,
    peer: SocketAddr,
    mut req: Request<Incoming>,
    method: String,
    url: String,
    headers_lower: HashMap<String, String>,
    raw_headers: Vec<(String, String)>,
    upgrade_tx: Arc<mpsc::Sender<HttpPendingUpgrade>>,
) -> Result<Response<ResponseBody>, hyper::Error> {
    // Compute the Sec-WebSocket-Accept value before consuming req.
    let accept_value = req
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .map(|k| tokio_tungstenite::tungstenite::handshake::derive_accept_key(k.as_bytes()))
        .unwrap_or_default();

    // Build the upgraded-protocol IncomingMessage now (no body — WS
    // upgrades carry no request body).
    let mut im = IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        Vec::new(),
        peer.ip().to_string(),
        peer.port(),
    );
    im.complete = true;
    let im_handle = alloc_incoming_message(im);

    // Spawn a task that waits for hyper to perform the protocol
    // switch + completes the tungstenite handshake + hands the
    // resulting stream to perry-ext-ws.
    tokio::spawn(async move {
        let upgraded = match hyper::upgrade::on(&mut req).await {
            Ok(u) => u,
            Err(_) => return,
        };
        let io = TokioIo::new(upgraded);
        let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
            io,
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        let ws_id = perry_ext_ws::register_external_ws_stream(ws);
        let pending = HttpPendingUpgrade {
            server_handle,
            request_handle: im_handle,
            ws_id,
            raw_socket_id: 0,
            head: Vec::new(),
        };
        let _ = upgrade_tx.send(pending).await;
        perry_ffi::notify_main_thread();
    });

    Ok(Response::builder()
        .status(101)
        .header("upgrade", "websocket")
        .header("connection", "Upgrade")
        .header("sec-websocket-accept", accept_value)
        .body(Full::new(Bytes::new()).boxed())
        .unwrap())
}

// ============================================================================
// Issue #604 — main-thread pump exposed to perry-stdlib's stdlib pump.
//
// Pre-#604, `js_node_http_server_listen` ended in `event_loop(...)` —
// an infinite blocking loop on the main TS thread that drained pending
// requests and upgrades synchronously. That blocked `await new
// Promise(r => server.listen(port, r))` from ever returning, so any
// code after `listen()` (e.g. `axios.get(...)`, `server.close()`)
// never ran.
//
// Replacement: `listen()` returns immediately after spawning the
// accept loop on the tokio runtime. The new
// `js_node_http_server_has_active` and `js_node_http_server_process_pending`
// externs are wired into perry-stdlib's `js_stdlib_has_active_handles` /
// `js_stdlib_process_pending` (gated on the `external-http-server-pump`
// feature). The codegen-emitted main loop calls those each tick, so
// requests + upgrades are dispatched on the same main thread as
// before — just driven from the outer event loop instead of an inner
// blocking one.
//
// Both externs walk the global handle registry via `iter_handles_of`
// (covers HTTP/1, HTTPS, and HTTP/2 — HTTPS / HTTP/2 wrap an
// `HttpServer` inside their own struct, so checking the standalone
// HttpServers + the `.base` of the wrappers covers all three).
// ============================================================================

/// Returns 1 if any registered HTTP/HTTPS/HTTP/2 server is currently
/// listening, has pending requests, or has pending upgrade events.
/// Wired into perry-stdlib's `js_stdlib_has_active_handles` via the
/// `external-http-server-pump` feature. Without this gate, the
/// codegen-emitted main loop would exit before the accept loop has
/// a chance to push the first request through the channel.
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
    iter_handles_of::<crate::https_server::HttpsServer, _>(|s| {
        if server_is_active(&s.base) {
            active = 1;
        }
    });
    if active != 0 {
        return 1;
    }
    iter_handles_of::<crate::http2_server::Http2SecureServer, _>(|s| {
        if server_is_active(&s.base) {
            active = 1;
        }
    });
    if active == 0 && crate::http2_server::has_active_h2_clients() {
        active = 1;
    }
    // #4728 — a request parked awaiting an async handler keeps the loop
    // alive so the deferred `res.end()` can still flush before exit.
    if active == 0 && has_in_flight_requests() {
        active = 1;
    }
    active
}

// ============================================================================
// #4728 — in-flight (async-handler) request tracking.
//
// A `(req, res) => { … }` handler that finishes the response on a *later*
// event-loop tick — an outbound `fetch()`, a `setTimeout`, any `await`
// chain that calls `res.end()` from a microtask/timer/tokio resolution —
// returns to `process_pending` before `res.end()` has run. Pre-#4728,
// `process_pending` then synthesized a default empty 200 and freed the
// per-request handles immediately, so the real `res.end(...)` later fired
// on a dropped handle (no-op) and the client saw an empty/closed reply.
//
// Fix: when the handler returns without ending the response, park the
// request here instead of synthesizing+freeing. The reaper runs each pump
// tick (the codegen-emitted main loop keeps ticking while the server is a
// live handle, draining timers / fetch resolutions / microtasks), and
// finalizes a parked request once `res.end()` has flushed the real
// response — or, as a safety net mirroring Node's `requestTimeout`,
// synthesizes the default response and frees the handles if the handler
// never responds within the grace window so a buggy handler can't pin a
// hyper connection (and its request handles) forever.
// ============================================================================

/// A request whose handler returned before finishing the response.
struct InFlightRequest {
    /// Owning server — lets `closeAllConnections()` drop parked requests
    /// whose connection it just destroyed (#4905).
    server_handle: i64,
    request_handle: i64,
    response_handle: i64,
    /// Mirrors `HttpPendingRequest::skip_default_response`: when true the
    /// response is driven elsewhere (e.g. an upgraded/stream path) so the
    /// reaper must not synthesize a default on timeout.
    skip_default_response: bool,
    /// Grace deadline. Past this, synthesize the default response (unless
    /// `skip_default_response`) and free the handles regardless.
    deadline: Instant,
}

static IN_FLIGHT: Mutex<Vec<InFlightRequest>> = Mutex::new(Vec::new());

/// True iff `res.end()` has flushed the response (or the handle is already
/// gone). A missing handle reads as "done" so a stray entry can't wedge
/// the reaper.
fn response_writable_ended(response_handle: i64) -> bool {
    get_handle::<ServerResponse>(response_handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true)
}

/// Free the per-request request + response handles. Mirrors the tail of
/// the synchronous-handler path in `process_pending`. The response is ended
/// (or about to be, via synthesize), so its id takes the one-tick fast path.
fn finalize_request_handles(request_handle: i64, response_handle: i64) {
    finalize_request_handles_deferred(request_handle, response_handle, None);
}

/// Free the per-request handles. `recycle_deadline` is `Some` only when the
/// response is being finalized WITHOUT having ended (the reaper's
/// peer-disconnect / server-force-close paths) — its id is then held in the
/// deadline-gated quarantine through the request's grace window so a
/// long-suspended handler that resumes and writes hits an empty slot rather
/// than a recycled response (see `perry_ffi::drop_handle_until`). `None` is the
/// ended / synchronous case. The request id always takes the one-tick path —
/// `IncomingMessage` has no late-write surface, and it is closed here first.
fn finalize_request_handles_deferred(
    request_handle: i64,
    response_handle: i64,
    recycle_deadline: Option<Instant>,
) {
    close_incoming_message(request_handle);
    perry_ffi::drop_handle(request_handle);
    match recycle_deadline {
        Some(deadline) => perry_ffi::drop_handle_until(response_handle, deadline),
        None => perry_ffi::drop_handle(response_handle),
    };
}

/// True iff any request is parked awaiting an async handler — keeps the
/// server's handle "active" so the main loop doesn't exit before the
/// pending response is flushed.
fn has_in_flight_requests() -> bool {
    IN_FLIGHT.lock().map(|g| !g.is_empty()).unwrap_or(false)
}

/// Finalize parked requests whose handler has now called `res.end()` (the
/// common case — fetch/timer/await resolved on a later tick), or whose
/// grace deadline has elapsed (a handler that never responds). Called each
/// pump tick. #4728.
fn reap_in_flight_requests() {
    // (request_handle, response_handle, needs_synthesize, recycle_deadline)
    let mut to_finalize: Vec<(i64, i64, bool, Option<Instant>)> = Vec::new();
    let mut drain_listeners: Vec<Vec<i64>> = Vec::new();
    {
        let mut guard = match IN_FLIGHT.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.is_empty() {
            return;
        }
        let now = Instant::now();
        guard.retain(|e| {
            let ended = response_writable_ended(e.response_handle);
            if !ended {
                // Streaming backpressure cleared — fire `'drain'` (outside
                // the lock) so `res.on('drain')` producer loops resume.
                let ls = crate::response::take_drain_listeners_if_ready(e.response_handle);
                if !ls.is_empty() {
                    drain_listeners.push(ls);
                }
            }
            // #4905: the per-request oneshot receiver died with its
            // connection task (client disconnected / closeAllConnections)
            // — the response can never be flushed, so don't pin the event
            // loop for the rest of the grace window. A streaming response
            // whose body receiver dropped is the same edge.
            let peer_gone = get_handle::<ServerResponse>(e.response_handle)
                .and_then(|sr| sr.response_tx.as_ref())
                .map(|tx| tx.is_closed())
                .unwrap_or(false)
                || crate::response::stream_receiver_gone(e.response_handle);
            let expired = now >= e.deadline;
            if ended || expired || peer_gone {
                // Only synthesize when we're giving up on a handler
                // that never ended the response — not when it ended
                // it itself, never for skip-default paths, and never
                // when the peer is gone (nothing to deliver to).
                let needs_synth = !ended && !e.skip_default_response && !peer_gone;
                // If the response will NOT be ended after finalize (it wasn't
                // ended and we're not synthesizing it to ended — the peer-gone
                // and not-ended-skip-default paths), its `writable_ended` stays
                // unset, so a handler suspended on a slow `await` could resume
                // later and write through the bare id. Defer recycling that id
                // until the request's grace deadline so the late write lands on
                // an empty slot for the whole window, not a recycled response.
                let recycle_deadline = if ended || needs_synth {
                    None
                } else {
                    Some(e.deadline)
                };
                to_finalize.push((
                    e.request_handle,
                    e.response_handle,
                    needs_synth,
                    recycle_deadline,
                ));
                false
            } else {
                true
            }
        });
    }
    for ls in drain_listeners {
        crate::request::emit_no_arg_to_listeners(&ls);
    }
    // Finalize outside the lock — `synthesize_default_response_if_needed`
    // and `drop_handle` don't touch `IN_FLIGHT`, but keeping them off the
    // lock avoids any future re-entrancy surprise.
    for (req, res, needs_synth, recycle_deadline) in to_finalize {
        if needs_synth {
            synthesize_default_response_if_needed(res);
        }
        finalize_request_handles_deferred(req, res, recycle_deadline);
    }
}

/// Finalize a just-dispatched request, or park it for the reaper if its
/// handler returned before finishing the response (an async handler that
/// will call `res.end()` on a later tick). Shared by the HTTP/1 and HTTPS
/// dispatch paths. #4728.
pub(crate) fn finalize_or_park_request(pending: &HttpPendingRequest) {
    if response_writable_ended(pending.response_handle) {
        finalize_request_handles(pending.request_handle, pending.response_handle);
        return;
    }
    // Grace window mirrors Node's `requestTimeout` (default 300s; `0` =
    // disabled, so fall back to the default rather than parking forever).
    let grace_ms = get_handle::<HttpServer>(pending.server_handle)
        .map(|s| s.request_timeout)
        .filter(|t| *t > 0.0)
        .unwrap_or(300_000.0);
    let deadline = Instant::now() + Duration::from_millis(grace_ms as u64);
    if let Ok(mut guard) = IN_FLIGHT.lock() {
        guard.push(InFlightRequest {
            server_handle: pending.server_handle,
            request_handle: pending.request_handle,
            response_handle: pending.response_handle,
            skip_default_response: pending.skip_default_response,
            deadline,
        });
    } else {
        // Lock poisoned — fall back to the old immediate behavior so we
        // never leak the handles.
        if !pending.skip_default_response {
            synthesize_default_response_if_needed(pending.response_handle);
        }
        finalize_request_handles(pending.request_handle, pending.response_handle);
    }
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
/// #4903 — record a pending `'listening'` emit on a server (http / https /
/// http2 all share the `HttpServer` base). Node registers the
/// `listen(port, cb)` callback as a *once* `'listening'` listener inside
/// `listen()`, so the callback goes into the live listener list (correct
/// emit order vs. listeners added before/after `listen()`) and into
/// `deferred_listen_cbs`, which the pump uses to remove it again after
/// the emit fires.
pub(crate) fn queue_deferred_listening_emit(s: &mut HttpServer, callback: i64) {
    s.pending_listening_emit = true;
    if callback != 0 {
        s.listeners
            .entry("listening".to_string())
            .or_default()
            .push(callback);
        s.deferred_listen_cbs.push(callback);
    }
}

/// #4903 — fire a server's queued `'listening'` listeners + `listen(cb)`
/// callbacks with implicit `this` bound to the server. Runs from the
/// main-thread pump, never from inside `listen()` itself: Node emits
/// `'listening'` on a later event-loop tick, so the listen callback only
/// runs after the current synchronous script segment (including the
/// `const server = ...` assignment) has finished, and `'listening'`
/// listeners registered after `listen()` returned still fire. The
/// listener snapshot is taken here at drain time for that same reason,
/// and the queue is detached (`mem::take`) before any callback runs so
/// a re-entrant `listen()` from a callback can't double-fire.
pub(crate) fn drain_deferred_listen_for<T, F>(server_handle: i64, base_of: F) -> i32
where
    T: Send + Sync + 'static,
    F: FnOnce(&mut T) -> &mut HttpServer,
{
    let cbs: Vec<i64> = match get_handle_mut::<T>(server_handle) {
        Some(t) => {
            let s = base_of(t);
            if !std::mem::take(&mut s.pending_listening_emit) {
                return 0;
            }
            let snapshot = s.listeners.get("listening").cloned().unwrap_or_default();
            // The `listen(port, cb)` callbacks are once-listeners: now that
            // this emit has snapshotted them, drop them from the live list
            // so a future emit / listener introspection doesn't see them.
            let once: Vec<i64> = std::mem::take(&mut s.deferred_listen_cbs);
            if let Some(ls) = s.listeners.get_mut("listening") {
                for cb in &once {
                    if let Some(pos) = ls.iter().position(|x| x == cb) {
                        ls.remove(pos);
                    }
                }
            }
            snapshot
        }
        None => return 0,
    };
    let this_val = handle_to_pointer_f64(server_handle);
    let mut fired = 0i32;
    for cb in cbs {
        if cb == 0 {
            continue;
        }
        let raw = cb as *const RawClosureHeader;
        let closure = unsafe { JsClosure::from_raw(raw) };
        if !closure.is_null() {
            with_implicit_this(this_val, || {
                let _ = unsafe { closure.call0() };
            });
            fired += 1;
        }
    }
    fired
}

#[no_mangle]
pub extern "C" fn js_node_http_server_process_pending() -> i32 {
    let mut count = 0i32;

    // Promote ids freed on PRIOR ticks from quarantine to the reusable
    // freelist — at the top of the tick, before this tick's finalizations
    // quarantine fresh ids. The one-tick deferral closes the same-tick ABA
    // hazard: a handler that returned before `res.end()` leaves a stale `res`
    // (a bare tagged handle id) outstanding, and recycling its id immediately
    // would let the next request re-occupy it and a microtask-deferred
    // `res.write`/`res.end` corrupt the new request's response. Holding the id
    // in quarantine for a full tick lets those stale calls spend themselves
    // against an empty slot first. (A write deferred MORE than a tick past
    // finalization is a write-after-end use error and out of scope — see
    // `perry_ffi::drain_quarantined_handles`.)
    perry_ffi::drain_quarantined_handles();

    // #4728 — finalize any async-handler requests that have flushed their
    // response since the last tick (or timed out) before draining new ones.
    reap_in_flight_requests();

    // #4905 — fire `'connection'` listeners for connections accepted since
    // the last tick, before their requests are dispatched (Node fires
    // `'connection'` ahead of `'request'`).
    let connection_events: Vec<i64> = PENDING_CONNECTION_EVENTS
        .lock()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    for server_handle in connection_events {
        // The handle may back an HttpServer or an HttpsServer (whose
        // accept loop pushes here too since #4971) — probe both.
        let listeners = get_handle::<HttpServer>(server_handle)
            .and_then(|s| s.listeners.get("connection").cloned())
            .or_else(|| {
                get_handle::<crate::https_server::HttpsServer>(server_handle)
                    .and_then(|s| s.base.listeners.get("connection").cloned())
            })
            .unwrap_or_default();
        if listeners.is_empty() {
            continue;
        }
        let this_val = handle_to_pointer_f64(server_handle);
        with_implicit_this(this_val, || emit_no_arg_to_listeners(&listeners));
        count += 1;
    }

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
        // Drain upgrades first so they don't get starved by a busy
        // request stream.
        while let Some(up) = try_recv_upgrade(h) {
            if up.raw_socket_id != 0 {
                // #4973 raw path: make sure the adopted net.Socket's
                // dispatch extensions + GC scanner are registered on the
                // main thread before user code touches the socket.
                perry_ext_net::ensure_adopted_socket_dispatch();
                crate::upgrade::fire_upgrade_listeners(
                    up.server_handle,
                    up.request_handle,
                    up.raw_socket_id,
                    up.head,
                );
            } else {
                crate::upgrade::fire_upgrade_listeners(
                    up.server_handle,
                    up.request_handle,
                    up.ws_id,
                    Vec::new(),
                );
            }
            count += 1;
        }
        while let Some(p) = try_recv_pending_nonblocking(h) {
            process_pending(p);
            count += 1;
        }
    }

    let mut https_handles: Vec<i64> = Vec::new();
    perry_ffi::iter_handle_ids_of::<crate::https_server::HttpsServer, _>(|id| {
        https_handles.push(id)
    });
    for h in https_handles {
        count +=
            drain_deferred_listen_for::<crate::https_server::HttpsServer, _>(h, |s| &mut s.base);
        while let Some(p) = crate::https_server::try_recv_pending_https_nonblocking(h) {
            crate::https_server::process_pending_https(p);
            count += 1;
        }
    }

    let mut h2_handles: Vec<i64> = Vec::new();
    perry_ffi::iter_handle_ids_of::<crate::http2_server::Http2SecureServer, _>(|id| {
        h2_handles.push(id)
    });
    for h in h2_handles {
        count += drain_deferred_listen_for::<crate::http2_server::Http2SecureServer, _>(h, |s| {
            &mut s.base
        });
        count += crate::http2_server::process_pending_h2_events();
        while let Some(p) = crate::http2_server::try_recv_pending_h2_nonblocking(h) {
            crate::http2_server::process_pending_h2(p);
            count += 1;
            count += crate::http2_server::process_pending_h2_events();
        }
    }
    count += crate::http2_server::process_pending_h2_events();

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

fn server_is_active(s: &HttpServer) -> bool {
    // #5011 — an `unref()`ed server no longer keeps the event loop alive
    // just by being bound, so a quietly-listening unref'd server lets the
    // process exit (Node semantics). Pending listen callbacks and queued
    // requests below still keep the loop alive long enough to flush any
    // in-flight work.
    if s.listening && s.refed {
        return true;
    }
    // #4903 — a queued `'listening'` emit / listen callback must keep the
    // loop alive until the pump fires it, even if `close()` already ran.
    if s.pending_listening_emit || !s.deferred_listen_cbs.is_empty() {
        return true;
    }
    // Even if the user has called close(), the channels may still
    // hold queued items the pump needs to drain on a subsequent tick
    // before the program can exit cleanly.
    if let Some(rx) = s.request_rx.as_ref() {
        if !rx.is_closed() && rx.len() > 0 {
            return true;
        }
    }
    if let Some(rx) = s.upgrade_rx.as_ref() {
        if !rx.is_closed() && rx.len() > 0 {
            return true;
        }
    }
    false
}

fn try_recv_upgrade(server_handle: i64) -> Option<HttpPendingUpgrade> {
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        if let Some(rx) = s.upgrade_rx.as_mut() {
            match rx.try_recv() {
                Ok(p) => return Some(p),
                Err(_) => return None,
            }
        }
    }
    None
}

/// Non-blocking try_recv. Unlike the pre-#604 `try_recv_pending` which
/// spun for up to 10ms waiting for a message, this returns
/// immediately so the pump can move on to the next server / next tick.
/// The codegen-emitted main loop's `js_wait_for_event` provides the
/// blocking wait at the outer level via condvar, so we don't need to
/// spin here.
pub(crate) fn try_recv_pending_nonblocking(server_handle: i64) -> Option<HttpPendingRequest> {
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        if let Some(rx) = s.request_rx.as_mut() {
            return rx.try_recv().ok();
        }
    }
    None
}

/// Dispatch one pending request — fire `'request'` listeners, then
/// the main handler, then await any returned Promise. The handler is
/// expected to call `res.end(...)` itself; the response oneshot
/// fires from inside `js_node_http_res_end`.
fn process_pending(pending: HttpPendingRequest) {
    let req_f64 = handle_to_pointer_f64(pending.request_handle);
    let res_f64 = handle_to_pointer_f64(pending.response_handle);

    // Fire `'request'` listeners (Node's `server.on('request', ...)`).
    // Node's emitter invokes them with `this` bound to the server, so the
    // `function (req, res) { this.address().port }` handler idiom works
    // (#4903). Bind for the synchronous call only — microtasks run outside.
    let server_this = handle_to_pointer_f64(pending.server_handle);

    // #5080 — an `Expect: 100-continue` request with a `'checkContinue'`
    // listener fires that listener *instead of* `'request'` + the handler
    // (Node's dispatch). The listener calls `res.writeContinue()` and then
    // drives the exchange itself.
    if pending.is_check_continue {
        for cb in &pending.check_continue_listeners {
            if *cb == 0 {
                continue;
            }
            unsafe {
                let raw = *cb as *const RawClosureHeader;
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
        return;
    }

    for cb in &pending.request_listeners {
        if *cb == 0 {
            continue;
        }
        unsafe {
            let raw = *cb as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                with_implicit_this(server_this, || {
                    let _ = closure.call2(req_f64, res_f64);
                });
            }
            js_promise_run_microtasks();
        }
    }

    // Main handler. Per the issue #604 architectural change documented
    // on `js_node_http_server_process_pending`, we no longer
    // synchronously block on the handler's returned Promise — that
    // would re-introduce the listen()-blocks-main-thread problem at
    // a per-request granularity. The handler is expected to call
    // `res.end(...)` itself; subsequent microtasks fire via the next
    // tick of the codegen-emitted main loop. The
    // `synthesize_default_response_if_needed` safety net below
    // catches the case where neither path completed in time.
    if pending.handler != 0 {
        unsafe {
            let raw = pending.handler as *const RawClosureHeader;
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
}

/// If the handler didn't call `res.end()`, finish the response
/// transparently with whatever buffer / status was set so hyper's
/// service fn doesn't hang awaiting the oneshot.
pub(crate) fn synthesize_default_response_if_needed(response_handle: i64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(response_handle) {
        if !sr.writable_ended {
            sr.writable_ended = true;
            sr.headers_sent = true;
            sr.writable_finished = true;
            // Streaming response whose handler never called `.end()`: the
            // head is already on the wire — just close the body channel.
            if sr.stream_tx.take().is_some() {
                sr.needs_drain = false;
                return;
            }
            let body = std::mem::take(&mut sr.buffered_body);
            // `snapshot_headers` expands array-valued headers (e.g.
            // Set-Cookie) into one entry per element so they emit a separate
            // wire line each (#4826).
            let mut headers = sr.snapshot_headers();
            if !sr.headers.contains_key("content-length")
                && !sr.headers.contains_key("transfer-encoding")
            {
                headers.push(("Content-Length".to_string(), body.len().to_string()));
            }
            let shape = HyperResponseShape {
                status: sr.status_code,
                status_message: sr.status_message.clone(),
                headers,
                trailers: Vec::new(),
                body: crate::response::ShapeBody::Full(body),
            };
            if let Some(tx) = sr.response_tx.take() {
                let _ = tx.send(shape);
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
