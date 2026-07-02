//! `https.createServer({ key, cert }, handler)` — TLS variant of
//! `http.createServer`. Re-uses the Phase 1 IncomingMessage /
//! ServerResponse / event-loop machinery. The accept loop wraps each
//! TCP stream in `tokio_rustls::TlsAcceptor` before handing the
//! decrypted stream to hyper.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsAcceptor;

use crate::ensure_gc_scanner_registered;
use crate::request::{
    alloc_incoming_message, emit_no_arg_to_listeners, handle_to_pointer_f64, with_implicit_this,
    IncomingMessage,
};
use crate::response::{alloc_server_response_for_request, HyperResponseShape, ResponseBody};
use crate::server::{
    sanitize_request_timeout, signal_connections_close, HttpPendingRequest, HttpServer,
    ReadActivity, TrackedConnection, CONNECTIONS, NEXT_CONNECTION_ID, PENDING_CONNECTION_EVENTS,
};
use crate::tls::{
    build_certless_server_config, build_server_config, has_pem_material, json_value_to_pem_bytes,
    parse_cert_chain, parse_private_key,
};

/// Decode `{ key, cert, alpnProtocols? }` from a NaN-boxed JsValue
/// object literal into the PEM byte buffers + a flag for whether to
/// advertise `h2` in ALPN. `key`/`cert` accept either a PEM string
/// OR a `Buffer` (the form `fs.readFileSync('key.pem')` returns when
/// no encoding is supplied) — see `json_value_to_pem_bytes`. Falls
/// back to empty PEMs (which the cert-chain parser then rejects) on
/// any extraction failure so the user sees a clear bind error.
unsafe fn parse_https_opts(opts_f64: f64) -> (Vec<u8>, Vec<u8>, bool) {
    use perry_ffi::JsValue;
    let v = JsValue::from_bits(opts_f64.to_bits());
    if !v.is_pointer() {
        return (Vec::new(), Vec::new(), true);
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return (Vec::new(), Vec::new(), true),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), Vec::new(), true),
    };
    let key_pem = json_value_to_pem_bytes(parsed.get("key"));
    let cert_pem = json_value_to_pem_bytes(parsed.get("cert"));
    // Default ALPN to `[http/1.1]` only — node:https is HTTP/1.1
    // by spec; users wanting HTTP/2 should reach for node:http2's
    // createSecureServer instead. Opt-in via `alpnProtocols: ["h2", "http/1.1"]`.
    // Without this, an HTTP/2-aware client (curl --http2) negotiates h2
    // via ALPN against our http1::Builder accept loop and the request
    // hangs because we never speak h2 frames back.
    let enable_h2 = parsed
        .get("alpnProtocols")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("h2")))
        .unwrap_or(false);
    (key_pem, cert_pem, enable_h2)
}
use crate::types::{
    extract_host, extract_port, js_promise_run_microtasks, read_string_header, POINTER_TAG,
    PTR_MASK,
};

/// `https.createServer(opts, handler)` — opts carries `{ key, cert }`
/// (PEM strings) plus optional `passphrase`/`ca`. `handler` is the
/// usual `(req, res) => …` closure.
///
/// `opts_f64` is the NaN-boxed `{ key, cert, alpnProtocols? }` object
/// the TS user passes to `https.createServer(opts, handler)`. Read
/// via `json_stringify` so binary cert data has to fit through a
/// PEM round-trip — fine since key + cert PEM are both ASCII.
#[no_mangle]
pub unsafe extern "C" fn js_node_https_create_server(opts_f64: f64, handler: i64) -> i64 {
    ensure_gc_scanner_registered();

    let (key_pem, cert_pem, enable_http2_alpn) = parse_https_opts(opts_f64);
    let mut base = HttpServer::with_handler(handler);
    crate::server::apply_server_options(&mut base, opts_f64);

    let cert_chain = parse_cert_chain(&cert_pem);
    let has_tls_material = has_pem_material(&key_pem, &cert_pem);
    if !has_tls_material {
        // `https.createServer()` with no key/cert — Node constructs and
        // listens fine; the handshake fails per-connection instead. A
        // `None` config here used to make `listen()` refuse outright
        // ("tls config unavailable"), so the 'listening' callback never
        // fired (#4974).
        return register_handle(HttpsServer {
            handler,
            tls_config: Some(build_certless_server_config(enable_http2_alpn)),
            base,
        });
    }
    let private_key = match parse_private_key(&key_pem) {
        Some(k) => k,
        None => {
            eprintln!("[node:https] no recognized PEM private key");
            // Still register the handle so the user gets a `.listen`
            // call that fails with a clear bind error rather than a
            // silent zero-handle.
            return register_handle(HttpsServer {
                handler,
                tls_config: None,
                base,
            });
        }
    };
    let tls_config = match build_server_config(cert_chain, private_key, enable_http2_alpn) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[node:https] {}", e);
            None
        }
    };

    register_handle(HttpsServer {
        handler,
        tls_config,
        base,
    })
}

/// Backing struct for an `https.Server` JS-side handle. Wraps the
/// HTTP/1.1 base server with a rustls `ServerConfig`.
pub struct HttpsServer {
    pub handler: i64,
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
    pub base: HttpServer,
}

/// `httpsServer.listen(port?, host?, backlog?, cb?)` — binds + starts
/// accepting TLS-wrapped connections. `args_array` carries the variadic
/// `listen()` arguments; see `js_node_http_server_listen` / `parse_listen_args`
/// for the overload resolution. Issue #2041.
#[no_mangle]
pub unsafe extern "C" fn js_node_https_server_listen(server_handle: i64, args_array: i64) -> i64 {
    // Returns `server_handle` for chainability (#2129).
    let parsed = crate::types::parse_listen_args(args_array);
    let opts_f64 = parsed.opts;
    let port = extract_port(opts_f64, 443);
    let host = parsed
        .host
        .unwrap_or_else(|| extract_host(opts_f64, "0.0.0.0"));
    let callback = parsed.callback;

    let (request_tx, request_rx) = mpsc::channel::<HttpPendingRequest>(1024);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    // #2132 — synchronous bind so `server.address().port` reflects the
    // OS-assigned ephemeral port before the `listen(port, cb)` callback
    // fires. See `server::js_node_http_server_listen` for the full
    // rationale; same shape here, on top of the TLS-acceptor wrap.
    let bind_str = format!("{}:{}", host, port);
    let addr: SocketAddr = match bind_str.parse() {
        Ok(a) => a,
        Err(_) => SocketAddr::from(([0, 0, 0, 0], port)),
    };
    // #4914 — SO_REUSEPORT in cluster workers; plain bind otherwise.
    let std_listener = match crate::cluster_bind::bind_listener(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[node:https] bind {}:{} failed: {}", host, port, e);
            return server_handle;
        }
    };
    let actual_port = std_listener.local_addr().map(|a| a.port()).unwrap_or(port);
    if let Err(e) = std_listener.set_nonblocking(true) {
        eprintln!("[node:https] set_nonblocking failed: {}", e);
        return server_handle;
    }
    crate::cluster_bind::notify_listening(&host, actual_port);

    // Capture `noDelay` (default true) under the same handle lock as the TLS
    // config, so the accept loop can apply it per connection without re-locking
    // the handle map. Mirrors the HTTP/1 + HTTP/2 paths in server.rs.
    let no_delay;
    let tls_config = if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
        s.base.bound_port = actual_port;
        s.base.bound_host = host.clone();
        s.base.listening = true;
        s.base.shutdown_tx = Some(shutdown_tx);
        s.base.request_rx = Some(request_rx);
        no_delay = s.base.no_delay;
        s.tls_config.clone()
    } else {
        return server_handle;
    };

    let tls_config = match tls_config {
        Some(c) => c,
        None => {
            eprintln!("[node:https] tls config unavailable; refusing to listen");
            return server_handle;
        }
    };

    // TLS accept workers queue Rust request handles; JS callbacks run from
    // the main-thread HTTP pump, so listener lifetime is GC-safe.

    let request_tx = Arc::new(request_tx);
    let request_tx_for_spawn = request_tx.clone();
    let acceptor = TlsAcceptor::from(tls_config);

    perry_ffi::spawn_blocking_with_reactor(move || {
        tokio::spawn(async move {
            let listener = match TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[node:https] tokio adopt failed: {}", e);
                    return;
                }
            };
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, peer)) => {
                                // Node sets TCP_NODELAY on accepted connections by
                                // default. Honor the server's `noDelay` option
                                // (default true) on the raw TCP socket before the
                                // TLS handshake; the option persists through rustls.
                                crate::server::apply_accept_no_delay(&stream, no_delay);
                                let acceptor = acceptor.clone();
                                let request_tx = request_tx_for_spawn.clone();
                                // #4905/#4971 — register the connection so
                                // close()/closeAllConnections/
                                // closeIdleConnections can reach this task
                                // from the main thread, and queue the
                                // 'connection' emit (Node fires it on the raw
                                // TCP connection, before the TLS handshake).
                                let conn_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::SeqCst);
                                let busy = Arc::new(AtomicUsize::new(0));
                                let read_active = Arc::new(AtomicBool::new(false));
                                let close = Arc::new(tokio::sync::Notify::new());
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
                                    let tls_stream = match acceptor.accept(stream).await {
                                        Ok(s) => s,
                                        Err(e) => {
                                            eprintln!("[node:https] tls handshake: {}", e);
                                            CONNECTIONS.lock().unwrap().remove(&conn_id);
                                            return;
                                        }
                                    };
                                    // Track read activity on the DECRYPTED
                                    // stream — handshake bytes must not mark
                                    // a request-less socket non-idle (#4971).
                                    let io = TokioIo::new(ReadActivity::new(
                                        tls_stream,
                                        read_active.clone(),
                                    ));
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        let busy = busy.clone();
                                        let read_active = read_active.clone();
                                        async move {
                                            busy.fetch_add(1, Ordering::SeqCst);
                                            read_active.store(false, Ordering::SeqCst);
                                            let res = handle_https_request(server_handle, peer, req, request_tx).await;
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
                                            // Common when the client closes
                                            // mid-request — silenced.
                                            let _ = result;
                                        }
                                        _ = close.notified() => {
                                            // close()/closeAllConnections/
                                            // closeIdleConnections: dropping
                                            // the pinned connection closes the
                                            // socket immediately.
                                        }
                                    }
                                    CONNECTIONS.lock().unwrap().remove(&conn_id);
                                });
                            }
                            Err(e) => eprintln!("[node:https] accept error: {}", e),
                        }
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });
    });

    // #4903 — queue the `'listening'` emit + the optional `cb` for the
    // main-thread pump instead of firing synchronously; Node emits
    // `'listening'` on a later tick, after `const server = ...` has been
    // assigned. The pump binds `this` to the server when it fires them
    // (#2132). See `server::drain_deferred_listen_for`.
    if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
        crate::server::queue_deferred_listening_emit(&mut s.base, callback);
    }

    // Closes #604 — `listen()` is now non-blocking. Pending requests
    // are drained via the unified `js_node_http_server_process_pending`
    // pump in `server.rs`, which iterates HTTP/1, HTTPS, and HTTP/2
    // handles each tick.
    server_handle
}

async fn handle_https_request(
    server_handle: i64,
    peer: SocketAddr,
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<HttpPendingRequest>>,
) -> Result<Response<ResponseBody>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let url = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };
    let mut headers_lower = HashMap::new();
    let mut raw_headers = Vec::new();
    for (n, v) in req.headers() {
        if let Ok(vs) = v.to_str() {
            headers_lower.insert(n.to_string().to_lowercase(), vs.to_string());
            raw_headers.push((n.to_string(), vs.to_string()));
        }
    }
    // #2132 — capture before `req` / `headers_lower` are consumed below.
    let http_version = req.version();
    let req_connection = headers_lower.get("connection").cloned();
    // #5080 — `Expect: 100-continue` routes to `'checkContinue'` (hyper
    // auto-sends the interim `100 Continue` once the body is polled below).
    let expects_continue = headers_lower
        .get("expect")
        .map(|v| v.to_ascii_lowercase().contains("100-continue"))
        .unwrap_or(false);
    let body = match req.collect().await {
        Ok(c) => c.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };
    let im_handle = alloc_incoming_message(IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        body,
        peer.ip().to_string(),
        peer.port(),
    ));
    let (response_tx, response_rx) = oneshot::channel::<HyperResponseShape>();
    let sr_handle = alloc_server_response_for_request(response_tx, im_handle);
    let (request_listeners, handler, keep_alive_timeout, check_continue_listeners) =
        match get_handle::<HttpsServer>(server_handle) {
            Some(s) => (
                s.base.listeners.get("request").cloned().unwrap_or_default(),
                s.handler,
                s.base.keep_alive_timeout,
                s.base
                    .listeners
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

/// Non-blocking try_recv for HTTPS pending requests. Called by
/// `js_node_http_server_process_pending` in `server.rs` each tick.
pub(crate) fn try_recv_pending_https_nonblocking(server_handle: i64) -> Option<HttpPendingRequest> {
    if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
        if let Some(rx) = s.base.request_rx.as_mut() {
            return rx.try_recv().ok();
        }
    }
    None
}

/// Dispatch one HTTPS pending request — fire `'request'` listeners,
/// then the main handler. Same shape as `server::process_pending`
/// (the per-server struct differs but the dispatch logic is
/// identical). Per the issue #604 architectural change, we no
/// longer block on the handler-returned Promise.
pub(crate) fn process_pending_https(pending: HttpPendingRequest) {
    let req_f64 = handle_to_pointer_f64(pending.request_handle);
    let res_f64 = handle_to_pointer_f64(pending.response_handle);
    // #4903 — Node invokes `'request'` listeners (and the `createServer`
    // handler, which is one) with `this` bound to the server.
    let server_this = handle_to_pointer_f64(pending.server_handle);
    // #5080 — an `Expect: 100-continue` request with a `'checkContinue'`
    // listener fires that listener instead of the `'request'` path.
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
        crate::server::finalize_or_park_request(&pending);
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
    if pending.handler != 0 {
        unsafe {
            let raw = pending.handler as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                with_implicit_this(server_this, || {
                    let _ = closure.call2(req_f64, res_f64);
                });
            }
            js_promise_run_microtasks();
        }
    }
    // #4728 — an async handler (outbound `fetch()`, `setTimeout`, `await`
    // chain) returns before `res.end()` runs. Finalize now if the response
    // is already flushed, otherwise park it for the reaper instead of
    // synthesizing a premature empty response and freeing the handles out
    // from under the pending work.
    crate::server::finalize_or_park_request(&pending);
}

/// `httpsServer.address()` mirroring `http.Server.address()`.
#[no_mangle]
pub extern "C" fn js_node_https_server_address_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<HttpsServer>(handle)
        .map(|s| {
            if !s.base.listening {
                "null".to_string()
            } else {
                let family = if s.base.bound_host.contains(':') {
                    "IPv6"
                } else {
                    "IPv4"
                };
                serde_json::json!({
                    "port": s.base.bound_port,
                    "address": s.base.bound_host,
                    "family": family,
                })
                .to_string()
            }
        })
        .unwrap_or_else(|| "null".to_string());
    alloc_string(&s).as_raw()
}

/// `httpsServer.close(cb?)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_https_server_close(handle: i64, callback: i64) {
    let close_listeners;
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        s.base.listening = false;
        s.base.connections_checking_interval_destroyed = true;
        s.base.shutdown_tx.take();
        close_listeners = s.base.listeners.get("close").cloned().unwrap_or_default();
    } else {
        close_listeners = Vec::new();
    }
    // Node 19+: `server.close()` destroys idle keep-alive connections
    // (active requests are allowed to finish) (#4905/#4971).
    signal_connections_close(handle, true);
    emit_no_arg_to_listeners(&close_listeners);
    if callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}

/// `httpsServer.on(event, cb)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_https_server_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        s.base.listeners.entry(event).or_default().push(callback);
    }
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

/// `httpsServer.closeAllConnections()` — destroy every tracked
/// connection of this server, including ones with an in-flight request.
/// Was a no-op stub pre-#4971; the HTTPS accept loop now registers each
/// connection in the shared `CONNECTIONS` registry (#4905 machinery).
#[no_mangle]
pub extern "C" fn js_node_https_server_close_all_connections(handle: i64) {
    // Delegate to the HTTP variant: the CONNECTIONS and IN_FLIGHT
    // registries are shared and keyed by server handle, and the HTTP
    // path also finalizes parked async requests whose connection task
    // just died.
    crate::server::js_node_http_server_close_all_connections(handle);
}

/// `httpsServer.closeIdleConnections()` — destroy connections with no
/// in-flight request and no half-received one (#4971).
#[no_mangle]
pub extern "C" fn js_node_https_server_close_idle_connections(handle: i64) {
    signal_connections_close(handle, true);
}

/// `httpsServer.ref()` — keep the loop alive (default) and return the
/// receiver handle so chains work. Sets the flag on the wrapped base
/// `HttpServer`, which `server_is_active` reads for HTTPS too. #5011.
#[no_mangle]
pub extern "C" fn js_node_https_server_ref(handle: i64) -> i64 {
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        s.base.refed = true;
    }
    handle
}

/// `httpsServer.unref()` — stop keeping the process alive and return the
/// receiver handle (Node returns `this`). #5011.
#[no_mangle]
pub extern "C" fn js_node_https_server_unref(handle: i64) -> i64 {
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        s.base.refed = false;
    }
    handle
}

macro_rules! https_server_getter {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: i64) -> f64 {
            get_handle::<HttpsServer>(handle)
                .map(|s| s.base.$field)
                .unwrap_or(0.0)
        }
    };
}

macro_rules! https_server_setter {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: i64, value: f64) -> f64 {
            if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
                s.base.$field = value;
            }
            value
        }
    };
}

https_server_getter!(js_node_https_server_headers_timeout, headers_timeout);
https_server_setter!(js_node_https_server_set_headers_timeout, headers_timeout);
https_server_getter!(js_node_https_server_keep_alive_timeout, keep_alive_timeout);
https_server_setter!(
    js_node_https_server_set_keep_alive_timeout,
    keep_alive_timeout
);
https_server_getter!(
    js_node_https_server_keep_alive_timeout_buffer,
    keep_alive_timeout_buffer
);
https_server_setter!(
    js_node_https_server_set_keep_alive_timeout_buffer,
    keep_alive_timeout_buffer
);
https_server_getter!(js_node_https_server_request_timeout, request_timeout);
/// `httpsServer.requestTimeout = ms` — sanitized rather than
/// macro-generated, mirroring the HTTP setter, so the stored value
/// stays in Node's finite/non-negative/`MAX_SAFE_INTEGER` domain and
/// the in-flight reaper's `as u64` cast can't overflow.
#[no_mangle]
pub extern "C" fn js_node_https_server_set_request_timeout(handle: i64, value: f64) -> f64 {
    let sanitized = sanitize_request_timeout(value);
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        s.base.request_timeout = sanitized;
    }
    value
}
https_server_getter!(js_node_https_server_idle_timeout, idle_timeout);
https_server_setter!(js_node_https_server_set_idle_timeout, idle_timeout);
https_server_getter!(js_node_https_server_max_headers_count, max_headers_count);
https_server_setter!(
    js_node_https_server_set_max_headers_count,
    max_headers_count
);
https_server_getter!(
    js_node_https_server_max_requests_per_socket,
    max_requests_per_socket
);
https_server_setter!(
    js_node_https_server_set_max_requests_per_socket,
    max_requests_per_socket
);

#[no_mangle]
pub extern "C" fn js_node_https_server_listening_value(handle: i64) -> f64 {
    f64::from_bits(
        JsValue::from_bool(
            get_handle::<HttpsServer>(handle)
                .map(|s| s.base.listening)
                .unwrap_or(false),
        )
        .bits(),
    )
}

#[no_mangle]
pub extern "C" fn js_node_https_server_set_timeout_method(
    handle: i64,
    msecs: f64,
    callback: i64,
) -> i64 {
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        s.base.idle_timeout = msecs;
        if callback != 0 {
            s.base
                .listeners
                .entry("timeout".to_string())
                .or_default()
                .push(callback);
        }
    }
    handle
}

#[cfg(test)]
mod nodelay_tests {
    //! The server accept loops apply the *server's* `noDelay` to each accepted
    //! TCP socket before any TLS handshake — they must NOT hardcode `true`, the
    //! way the HTTP/1 path in `server.rs` honors `s.no_delay`. Before this fix
    //! the HTTPS and HTTP/2-secure accept loops both called
    //! `stream.set_nodelay(true)` unconditionally, so a server created with
    //! `noDelay: false` still ran with Nagle disabled (#5658). Both now route
    //! through `apply_accept_no_delay`; these tests drive it — the literal call
    //! the accept loops make — over a real loopback accept and read
    //! `TCP_NODELAY` back off the accepted stream, exercising the production
    //! wiring rather than a stand-in. `noDelay`'s default is verified ON in
    //! `lib.rs` (`http_server_seeds_node_timeout_defaults`), so a default
    //! server continues to get `TCP_NODELAY` on.

    use crate::server::apply_accept_no_delay;
    use tokio::net::{TcpListener, TcpStream};

    /// Accept a loopback connection and return the SERVER-side stream — the
    /// stream the HTTPS accept loop owns and applies `noDelay` to before the
    /// TLS handshake. The client end is returned so the connection stays open
    /// for the assertion.
    async fn accept_loopback() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (server, client)
    }

    /// `noDelay: false` is honored on the HTTPS accept path: the accepted TLS
    /// socket runs with `TCP_NODELAY` OFF (Nagle on). This is the regression —
    /// the pre-fix hardcoded `true` left it ON and this assertion failed.
    #[tokio::test]
    async fn https_accept_honors_no_delay_false() {
        let (server, _client) = accept_loopback().await;
        apply_accept_no_delay(&server, false);
        assert!(
            !server.nodelay().unwrap(),
            "https server created with noDelay:false must leave TCP_NODELAY off on accepted sockets"
        );
    }

    /// The Node default (`noDelay: true`) keeps `TCP_NODELAY` ON, so default
    /// HTTPS servers are unchanged by the fix.
    #[tokio::test]
    async fn https_accept_default_no_delay_true() {
        let (server, _client) = accept_loopback().await;
        apply_accept_no_delay(&server, true);
        assert!(
            server.nodelay().unwrap(),
            "default HTTPS server (noDelay:true) must keep TCP_NODELAY on, matching Node"
        );
    }
}
