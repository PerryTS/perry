//! `https.createServer({ key, cert }, handler)` — TLS variant of
//! `http.createServer`. Re-uses the Phase 1 IncomingMessage /
//! ServerResponse / event-loop machinery. The accept loop wraps each
//! TCP stream in `tokio_rustls::TlsAcceptor` before handing the
//! decrypted stream to hyper.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, RawClosureHeader,
    StringHeader,
};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsAcceptor;

use crate::ensure_gc_scanner_registered;
use crate::request::{
    alloc_incoming_message, emit_no_arg_to_listeners, handle_to_pointer_f64, IncomingMessage,
};
use crate::response::{alloc_server_response, HyperResponseShape};
use crate::server::{HttpPendingRequest, HttpServer};
use crate::tls::{build_server_config, parse_cert_chain, parse_private_key};

/// Decode `{ key, cert, alpnProtocols? }` from a NaN-boxed JsValue
/// object literal into Rust strings + a flag for whether to advertise
/// `h2` in ALPN. Falls back to empty PEMs (which the cert-chain
/// parser then rejects) on any extraction failure so the user sees
/// a clear bind error.
unsafe fn parse_https_opts(opts_f64: f64) -> (String, String, bool) {
    use perry_ffi::JsValue;
    let v = JsValue::from_bits(opts_f64.to_bits());
    if !v.is_pointer() {
        return (String::new(), String::new(), true);
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return (String::new(), String::new(), true),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return (String::new(), String::new(), true),
    };
    let key_pem = parsed
        .get("key")
        .and_then(|k| k.as_str())
        .unwrap_or_default()
        .to_string();
    let cert_pem = parsed
        .get("cert")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
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

    let cert_chain = parse_cert_chain(cert_pem.as_bytes());
    let private_key = match parse_private_key(key_pem.as_bytes()) {
        Some(k) => k,
        None => {
            eprintln!("[node:https] no recognized PEM private key");
            // Still register the handle so the user gets a `.listen`
            // call that fails with a clear bind error rather than a
            // silent zero-handle.
            return register_handle(HttpsServer {
                handler,
                tls_config: None,
                base: HttpServer {
                    handler,
                    listeners: HashMap::new(),
                    bound_port: 0,
                    bound_host: String::new(),
                    listening: false,
                    shutdown_tx: None,
                    request_rx: None,
                    upgrade_rx: None,
                },
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
        base: HttpServer {
            handler,
            listeners: HashMap::new(),
            bound_port: 0,
            bound_host: String::new(),
            listening: false,
            shutdown_tx: None,
            request_rx: None,
            upgrade_rx: None,
        },
    })
}

/// Backing struct for an `https.Server` JS-side handle. Wraps the
/// HTTP/1.1 base server with a rustls `ServerConfig`.
pub struct HttpsServer {
    pub handler: i64,
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
    pub base: HttpServer,
}

/// `httpsServer.listen({ port, host? }, cb?)` — binds + starts
/// accepting TLS-wrapped connections.
#[no_mangle]
pub unsafe extern "C" fn js_node_https_server_listen(
    server_handle: i64,
    opts_f64: f64,
    callback: i64,
) {
    let port = extract_port(opts_f64, 443);
    let host = extract_host(opts_f64, "0.0.0.0");

    let (request_tx, request_rx) = mpsc::channel::<HttpPendingRequest>(1024);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    let tls_config = if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
        s.base.bound_port = port;
        s.base.bound_host = host.clone();
        s.base.listening = true;
        s.base.shutdown_tx = Some(shutdown_tx);
        s.base.request_rx = Some(request_rx);
        s.tls_config.clone()
    } else {
        return;
    };

    let tls_config = match tls_config {
        Some(c) => c,
        None => {
            eprintln!("[node:https] tls config unavailable; refusing to listen");
            return;
        }
    };

    // TLS accept workers queue Rust request handles; JS callbacks run from
    // the main-thread HTTP pump, so listener lifetime is GC-safe.

    let request_tx = Arc::new(request_tx);
    let request_tx_for_spawn = request_tx.clone();
    let host_for_spawn = host.clone();
    let acceptor = TlsAcceptor::from(tls_config);

    perry_ffi::spawn_blocking_with_reactor(move || {
        tokio::spawn(async move {
            let bind_str = format!("{}:{}", host_for_spawn, port);
            let addr: SocketAddr = match bind_str.parse() {
                Ok(a) => a,
                Err(_) => SocketAddr::from(([0, 0, 0, 0], port)),
            };
            let listener = match TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!(
                        "[node:https] bind {}:{} failed: {}",
                        host_for_spawn, port, e
                    );
                    return;
                }
            };
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, peer)) => {
                                let acceptor = acceptor.clone();
                                let request_tx = request_tx_for_spawn.clone();
                                tokio::spawn(async move {
                                    let tls_stream = match acceptor.accept(stream).await {
                                        Ok(s) => s,
                                        Err(e) => {
                                            eprintln!("[node:https] tls handshake: {}", e);
                                            return;
                                        }
                                    };
                                    let io = TokioIo::new(tls_stream);
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        async move {
                                            handle_https_request(server_handle, peer, req, request_tx).await
                                        }
                                    });
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .with_upgrades()
                                        .await
                                    {
                                        let _ = e;
                                    }
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

    let listening_listeners = get_handle::<HttpsServer>(server_handle)
        .and_then(|s| s.base.listeners.get("listening").cloned())
        .unwrap_or_default();
    emit_no_arg_to_listeners(&listening_listeners);
    if callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }

    // Closes #604 — `listen()` is now non-blocking. Pending requests
    // are drained via the unified `js_node_http_server_process_pending`
    // pump in `server.rs`, which iterates HTTP/1, HTTPS, and HTTP/2
    // handles each tick.
}

async fn handle_https_request(
    server_handle: i64,
    peer: SocketAddr,
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<HttpPendingRequest>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
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
    let sr_handle = alloc_server_response(response_tx);
    let (request_listeners, handler) = match get_handle::<HttpsServer>(server_handle) {
        Some(s) => (
            s.base.listeners.get("request").cloned().unwrap_or_default(),
            s.handler,
        ),
        None => (Vec::new(), 0),
    };
    let pending = HttpPendingRequest {
        server_handle,
        request_handle: im_handle,
        response_handle: sr_handle,
        request_listeners,
        handler,
    };
    if request_tx.send(pending).await.is_err() {
        return Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from("Server unavailable")))
            .unwrap());
    }
    perry_ffi::notify_main_thread();
    match response_rx.await {
        Ok(shape) => Ok(shape.into_hyper()),
        Err(_) => Ok(Response::builder()
            .status(500)
            .body(Full::new(Bytes::from("Handler error")))
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
    crate::server::synthesize_default_response_if_needed(pending.response_handle);
    perry_ffi::drop_handle(pending.request_handle);
    perry_ffi::drop_handle(pending.response_handle);
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
        s.base.shutdown_tx.take();
        close_listeners = s.base.listeners.get("close").cloned().unwrap_or_default();
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
