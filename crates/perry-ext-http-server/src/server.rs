//! `HttpServer` — backing `http.createServer(handler)`. Binds via
//! hyper's HTTP/1.1 service path, pushes `(req, res)` to the main
//! thread, runs the user handler synchronously (awaiting any
//! returned Promise), and flushes the buffered response back through
//! the per-request oneshot channel.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, iter_handles_of, register_handle, JsClosure,
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
    ///   - `maxHeadersCount`: 2000
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
            shutdown_tx: None,
            request_rx: None,
            upgrade_rx: None,
            headers_timeout: 60_000.0,
            keep_alive_timeout: 5_000.0,
            keep_alive_timeout_buffer: 1_000.0,
            request_timeout: 300_000.0,
            idle_timeout: 0.0,
            max_headers_count: 2000.0,
            max_requests_per_socket: 0.0,
            no_delay: true,
            keep_alive: false,
            keep_alive_initial_delay: 0.0,
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
}

/// Phase 4 — pending WebSocket upgrade ready to fire `'upgrade'`
/// listeners. Sent by the hyper service fn after the underlying
/// `hyper::upgrade::on` future resolves and the upgraded stream has
/// been registered with `perry_ext_ws::register_external_ws_stream`.
pub struct HttpPendingUpgrade {
    pub server_handle: i64,
    pub request_handle: i64,
    pub ws_id: i64,
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
    let bind_str = format!("{}:{}", host, port);
    let addr: SocketAddr = match bind_str.parse() {
        Ok(a) => a,
        Err(_) => SocketAddr::from(([0, 0, 0, 0], port)),
    };
    let std_listener = match std::net::TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[node:http] bind {}:{} failed: {}", host, port, e);
            return server_handle;
        }
    };
    let actual_port = std_listener.local_addr().map(|a| a.port()).unwrap_or(port);
    if let Err(e) = std_listener.set_nonblocking(true) {
        eprintln!("[node:http] set_nonblocking failed: {}", e);
        return server_handle;
    }

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

    let request_tx = Arc::new(request_tx);
    let upgrade_tx = Arc::new(upgrade_tx);
    let request_tx_for_spawn = request_tx.clone();
    let upgrade_tx_for_spawn = upgrade_tx.clone();

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
                                let io = TokioIo::new(stream);
                                let request_tx = request_tx_for_spawn.clone();
                                let upgrade_tx = upgrade_tx_for_spawn.clone();
                                let server_handle = server_handle;
                                tokio::spawn(async move {
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        let upgrade_tx = upgrade_tx.clone();
                                        async move {
                                            handle_request(server_handle, peer, req, request_tx, upgrade_tx).await
                                        }
                                    });
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .with_upgrades()
                                        .await
                                    {
                                        // Common when client closes mid-request — silenced.
                                        let _ = e;
                                    }
                                });
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

    // Fire `'listening'` listeners + the optional `cb` argument. Node
    // invokes both with `this` bound to the server, so the canonical
    // `server.listen(0, function() { this.address().port })` idiom works
    // (#2132). Set the implicit-`this` cell to the server's JS value
    // (POINTER_TAG-boxed handle, identical to what `createServer`
    // returned) for the duration of each callback, then restore.
    let this_val = handle_to_pointer_f64(server_handle);
    let listening_listeners = get_handle::<HttpServer>(server_handle)
        .and_then(|s| s.listeners.get("listening").cloned())
        .unwrap_or_default();
    with_implicit_this(this_val, || emit_no_arg_to_listeners(&listening_listeners));
    if callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            with_implicit_this(this_val, || {
                let _ = closure.call0();
            });
        }
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
        s.shutdown_tx.take();
        close_listeners = s.listeners.get("close").cloned().unwrap_or_default();
    } else {
        close_listeners = Vec::new();
    }
    emit_no_arg_to_listeners(&close_listeners);
    if callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}

/// `server.closeAllConnections()` — placeholder. Active hyper
/// connections live in their own tokio tasks; we'd need to thread an
/// abort handle through every task. For Phase 1 this is a no-op
/// (matches `closeIdleConnections` too).
#[no_mangle]
pub extern "C" fn js_node_http_server_close_all_connections(_handle: i64) {}

#[no_mangle]
pub extern "C" fn js_node_http_server_close_idle_connections(_handle: i64) {}

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

    // Phase 4 — WebSocket upgrade detection. If the request looks
    // like a WS upgrade, branch into the handshake path: build the
    // 101 Switching Protocols response synchronously and spawn a
    // task that awaits hyper's upgraded stream + completes the
    // tungstenite server handshake + registers the resulting
    // WebSocketStream with perry-ext-ws.
    if crate::upgrade::is_websocket_upgrade(&req) {
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

    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };

    let im = IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        body_bytes,
        peer.ip().to_string(),
        peer.port(),
    );
    let im_handle = alloc_incoming_message(im);

    let (response_tx, response_rx) = oneshot::channel::<HyperResponseShape>();
    let sr_handle = alloc_server_response_for_request(response_tx, im_handle);

    let (request_listeners, handler, keep_alive_timeout) =
        match get_handle::<HttpServer>(server_handle) {
            Some(s) => (
                s.listeners.get("request").cloned().unwrap_or_default(),
                s.handler,
                s.keep_alive_timeout,
            ),
            None => (Vec::new(), 0, 5_000.0),
        };

    let pending = HttpPendingRequest {
        server_handle,
        request_handle: im_handle,
        response_handle: sr_handle,
        skip_default_response: false,
        h2_stream_handle: 0,
        h2_stream_headers: Vec::new(),
        request_listeners,
        handler,
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

    // Snapshot handle ids first so we can mutate handle state
    // (drain channels, free per-request handles) without the
    // DashMap iterator dangling.
    let mut http_handles: Vec<i64> = Vec::new();
    perry_ffi::iter_handle_ids_of::<HttpServer, _>(|id| http_handles.push(id));
    for h in http_handles {
        // Drain upgrades first so they don't get starved by a busy
        // request stream.
        while let Some(up) = try_recv_upgrade(h) {
            crate::upgrade::fire_upgrade_listeners(
                up.server_handle,
                up.request_handle,
                up.ws_id,
                Vec::new(),
            );
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
        count += crate::http2_server::process_pending_h2_events();
        while let Some(p) = crate::http2_server::try_recv_pending_h2_nonblocking(h) {
            crate::http2_server::process_pending_h2(p);
            count += 1;
            count += crate::http2_server::process_pending_h2_events();
        }
    }
    count += crate::http2_server::process_pending_h2_events();

    count
}

fn server_is_active(s: &HttpServer) -> bool {
    if s.listening {
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
    for cb in &pending.request_listeners {
        if *cb == 0 {
            continue;
        }
        unsafe {
            let raw = *cb as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                let _ = closure.call2(req_f64, res_f64);
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
                let _ = closure.call2(req_f64, res_f64);
            }
            js_promise_run_microtasks();
        }
    }

    // If the handler didn't call `res.end()` (still has the channel),
    // synthesize a default 200 with empty body so hyper's service fn
    // doesn't hang.
    if !pending.skip_default_response {
        synthesize_default_response_if_needed(pending.response_handle);
    }

    // Free the per-request handles.
    close_incoming_message(pending.request_handle);
    perry_ffi::drop_handle(pending.request_handle);
    perry_ffi::drop_handle(pending.response_handle);
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
            let body = std::mem::take(&mut sr.buffered_body);
            let mut headers = Vec::with_capacity(sr.headers.len());
            for (lower_k, v) in &sr.headers {
                let orig = sr
                    .raw_header_names
                    .get(lower_k)
                    .cloned()
                    .unwrap_or_else(|| lower_k.clone());
                headers.push((orig, v.clone()));
            }
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
                body,
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
