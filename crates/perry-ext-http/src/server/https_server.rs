//! `https.createServer({ key, cert }, handler)` — TLS variant of
//! `http.createServer`. Re-uses the Phase 1 IncomingMessage /
//! ServerResponse / event-loop machinery. The accept loop wraps each
//! TCP stream in `tokio_rustls::TlsAcceptor` before handing the
//! decrypted stream to hyper.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

use crate::server::ensure_gc_scanner_registered;
use crate::server::request::{
    alloc_incoming_message, handle_to_pointer_f64, with_implicit_this, IncomingMessage,
};
use crate::server::response::{
    alloc_server_response_for_request, HyperResponseShape, ResponseBody,
};
use crate::server::server::{
    sanitize_request_timeout, signal_connections_close, HttpPendingRequest, HttpServer,
    ReadActivity, TrackedConnection, CONNECTIONS, NEXT_CONNECTION_ID, PENDING_CONNECTION_EVENTS,
};
use crate::server::tls::{
    build_certless_server_config, build_server_config, has_pem_material, json_value_to_pem_bytes,
    parse_cert_chain, parse_private_key, ConnectionKeyLog, NodeTicketKey,
};

/// Decode `{ key, cert, alpnProtocols? }` from a NaN-boxed JsValue
/// object literal into the PEM byte buffers + a flag for whether to
/// advertise `h2` in ALPN. `key`/`cert` accept either a PEM string
/// OR a `Buffer` (the form `fs.readFileSync('key.pem')` returns when
/// no encoding is supplied) — see `json_value_to_pem_bytes`. Falls
/// back to empty PEMs (which the cert-chain parser then rejects) on
/// any extraction failure so the user sees a clear bind error.
unsafe fn parse_https_opts(opts_f64: f64) -> (Vec<u8>, Vec<u8>, bool, Option<Vec<u8>>, i64) {
    use perry_ffi::JsValue;
    let mut v = JsValue::from_bits(opts_f64.to_bits());
    if !v.is_pointer_or_raw() {
        return (Vec::new(), Vec::new(), true, Some(default_alpn()), 0);
    }
    // Native-call lowering may pass a heap options object as its legacy raw
    // pointer. `json_stringify` expects the canonical POINTER_TAG shape.
    if !v.is_pointer() {
        v = JsValue::from_object_ptr((v.bits() & PTR_MASK) as *mut u8);
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return (Vec::new(), Vec::new(), true, Some(default_alpn()), 0),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), Vec::new(), true, Some(default_alpn()), 0),
    };
    let key_pem = json_value_to_pem_bytes(parsed.get("key"));
    let cert_pem = json_value_to_pem_bytes(parsed.get("cert"));
    // Default ALPN to `[http/1.1]` only — node:https is HTTP/1.1
    // by spec; users wanting HTTP/2 should reach for node:http2's
    // createSecureServer instead. Opt-in via `alpnProtocols: ["h2", "http/1.1"]`.
    // Without this, an HTTP/2-aware client (curl --http2) negotiates h2
    // via ALPN against our http1::Builder accept loop and the request
    // hangs because we never speak h2 frames back.
    let alpn_values = parsed
        .get("ALPNProtocols")
        .or_else(|| parsed.get("alpnProtocols"))
        .and_then(|a| a.as_array());
    let enable_h2 = alpn_values
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("h2")))
        .unwrap_or(false);
    let alpn_callback = raw_closure_field(f64::from_bits(v.bits()), "ALPNCallback");
    if alpn_callback != 0 && alpn_values.is_some() {
        perry_ffi::throw_with_code(
            "The ALPNCallback and ALPNProtocols TLS options are mutually exclusive",
            "ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    let alpn_protocols = if alpn_callback != 0 {
        None
    } else {
        Some(
            alpn_values
                .map(|values| encode_alpn(values))
                .unwrap_or_else(default_alpn),
        )
    };
    (key_pem, cert_pem, enable_h2, alpn_protocols, alpn_callback)
}

fn default_alpn() -> Vec<u8> {
    encode_alpn(&vec![serde_json::Value::String("http/1.1".to_string())])
}

fn encode_alpn(values: &[serde_json::Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        let Some(protocol) = value.as_str() else {
            continue;
        };
        let bytes = protocol.as_bytes();
        if bytes.len() <= u8::MAX as usize {
            out.push(bytes.len() as u8);
            out.extend_from_slice(bytes);
        }
    }
    out
}

unsafe fn raw_closure_field(options: f64, field: &str) -> i64 {
    let value = perry_ffi::object_field_by_name(JsValue::from_bits(options.to_bits()), field);
    crate::client_outgoing::callback_from_bits(value.bits() as i64)
}
use crate::server::types::{
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
pub unsafe extern "C" fn js_node_https_create_server(mut opts_f64: f64, mut handler: i64) -> i64 {
    ensure_gc_scanner_registered();

    if handler == 0 && crate::server::types::js_value_is_closure(opts_f64.to_bits() as i64) != 0 {
        handler = (opts_f64.to_bits() & PTR_MASK) as i64;
        opts_f64 = f64::from_bits(crate::server::types::TAG_UNDEFINED);
    }

    let (key_pem, cert_pem, enable_http2_alpn, alpn_protocols, alpn_callback) =
        parse_https_opts(opts_f64);
    let mut base = HttpServer::with_handler(handler);
    crate::server::server::apply_server_options(&mut base, opts_f64);
    let ticket_key = NodeTicketKey::random().unwrap_or_else(|error| panic!("{error}"));

    let cert_chain = parse_cert_chain(&cert_pem);
    let certificate_cn = cert_chain
        .first()
        .and_then(|certificate| crate::tls_client::certificate_common_name(certificate.as_ref()));
    let has_tls_material = has_pem_material(&key_pem, &cert_pem);
    if !has_tls_material {
        // `https.createServer()` with no key/cert — Node constructs and
        // listens fine; the handshake fails per-connection instead. A
        // `None` config here used to make `listen()` refuse outright
        // ("tls config unavailable"), so the 'listening' callback never
        // fired (#4974).
        let mut tls_config = build_certless_server_config(enable_http2_alpn);
        crate::server::tls::install_ticket_key(&mut tls_config, ticket_key.clone());
        return register_handle(HttpsServer {
            handler,
            tls_config: Some(tls_config),
            base,
            alpn_protocols,
            alpn_callback,
            ticket_key,
            certificate_cn,
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
                alpn_protocols,
                alpn_callback,
                ticket_key,
                certificate_cn,
            });
        }
    };
    let tls_config = match build_server_config(cert_chain, private_key, enable_http2_alpn) {
        Ok(mut config) => {
            crate::server::tls::install_ticket_key(&mut config, ticket_key.clone());
            Some(config)
        }
        Err(e) => {
            eprintln!("[node:https] {}", e);
            None
        }
    };

    register_handle(HttpsServer {
        handler,
        tls_config,
        base,
        alpn_protocols,
        alpn_callback,
        ticket_key,
        certificate_cn,
    })
}

/// Backing struct for an `https.Server` JS-side handle. Wraps the
/// HTTP/1.1 base server with a rustls `ServerConfig`.
pub struct HttpsServer {
    pub handler: i64,
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
    pub base: HttpServer,
    pub alpn_protocols: Option<Vec<u8>>,
    pub alpn_callback: i64,
    pub ticket_key: Arc<NodeTicketKey>,
    pub certificate_cn: Option<String>,
}

struct PendingTlsClientError {
    server_handle: i64,
    message: String,
}

struct PendingTlsKeylog {
    server_handle: i64,
    lines: Vec<Vec<u8>>,
}

static PENDING_TLS_CLIENT_ERRORS: Mutex<Vec<PendingTlsClientError>> = Mutex::new(Vec::new());
static PENDING_TLS_KEYLOGS: Mutex<Vec<PendingTlsKeylog>> = Mutex::new(Vec::new());

fn queue_tls_client_error(server_handle: i64, message: String) {
    if let Ok(mut pending) = PENDING_TLS_CLIENT_ERRORS.lock() {
        pending.push(PendingTlsClientError {
            server_handle,
            message,
        });
    }
    perry_ffi::notify_main_thread();
}

fn queue_tls_keylog(server_handle: i64, lines: Vec<Vec<u8>>) {
    if lines.is_empty() {
        return;
    }
    if let Ok(mut pending) = PENDING_TLS_KEYLOGS.lock() {
        pending.push(PendingTlsKeylog {
            server_handle,
            lines,
        });
    }
    perry_ffi::notify_main_thread();
}

/// Emit rustls key-log output after every successful handshake, even when the
/// peer closes before sending an HTTP request. JS still runs exclusively on
/// the main thread; the TLS worker only parks owned byte records here.
pub(crate) fn process_pending_tls_keylogs(server_handle: i64) -> i32 {
    let events = PENDING_TLS_KEYLOGS
        .lock()
        .map(|mut pending| {
            let mut selected = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                if pending[index].server_handle == server_handle {
                    selected.push(pending.remove(index));
                } else {
                    index += 1;
                }
            }
            selected
        })
        .unwrap_or_default();
    if events.is_empty() {
        return 0;
    }
    let scope = perry_ffi::TransientRootScope::enter();
    for event in &events {
        let socket = perry_ffi::alloc_null_proto_object(&[
            ("encrypted", JsValue::from_bool(true)),
            ("destroyed", JsValue::from_bool(false)),
            ("servername", JsValue::from_bool(false)),
        ]);
        let socket = scope.root_nanbox(f64::from_bits(socket.bits()));
        emit_keylog_lines(server_handle, socket.get(), &event.lines);
    }
    events.len() as i32
}

/// Dispatch failed TLS handshakes from the main JS thread. The socket value is
/// a minimal destroyed TLSSocket-compatible facade because rustls never yields
/// a decrypted stream that can be adopted after a failed handshake.
pub(crate) fn process_pending_tls_client_errors(server_handle: i64) -> i32 {
    let events = PENDING_TLS_CLIENT_ERRORS
        .lock()
        .map(|mut pending| {
            let mut selected = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                if pending[index].server_handle == server_handle {
                    selected.push(pending.remove(index));
                } else {
                    index += 1;
                }
            }
            selected
        })
        .unwrap_or_default();
    if events.is_empty() {
        return 0;
    }
    let this_value = handle_to_pointer_f64(server_handle);
    for event in &events {
        let listeners = get_handle_mut::<HttpsServer>(server_handle)
            .map(|server| {
                crate::server::server::take_server_event_listeners(
                    &mut server.base,
                    "tlsClientError",
                )
            })
            .unwrap_or_default();
        let scope = perry_ffi::TransientRootScope::enter();
        let listeners = scope.root_addrs(&listeners);
        let error = perry_ffi::error_value_with_code(
            &event.message,
            "ERR_SSL_TLS_HANDSHAKE",
            perry_ffi::ErrorKind::Error,
        );
        let error = scope.root_nanbox(f64::from_bits(error.bits()));
        let socket = perry_ffi::alloc_null_proto_object(&[
            ("encrypted", JsValue::from_bool(true)),
            ("destroyed", JsValue::from_bool(true)),
            ("servername", JsValue::from_bool(false)),
        ]);
        let socket = scope.root_nanbox(f64::from_bits(socket.bits()));
        for listener in &listeners {
            let listener = listener.get();
            if listener == 0 {
                continue;
            }
            let closure = unsafe { JsClosure::from_raw(listener as *const RawClosureHeader) };
            if !closure.is_null() {
                with_implicit_this(this_value, || unsafe {
                    let _ = closure.call2(error.get(), socket.get());
                });
            }
        }
    }
    events.len() as i32
}

/// Validate and install Node's 48-byte server ticket-key blob. The rustls
/// provider is shared by future per-connection configs, so rotation takes
/// effect without replacing the accept loop or touching another server.
pub(crate) fn set_ticket_keys(server_handle: i64, value: f64) {
    let Some(bytes) = perry_ffi::value_byte_slice(JsValue::from_bits(value.to_bits())) else {
        perry_ffi::throw_with_code(
            "The session ticket keys argument must be a Buffer or TypedArray",
            "ERR_INVALID_ARG_TYPE",
            perry_ffi::ErrorKind::TypeError,
        );
    };
    if bytes.len() != 48 {
        perry_ffi::throw_with_code(
            "Session ticket keys must be a 48-byte buffer",
            "ERR_INVALID_ARG_VALUE",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    let mut keys = [0_u8; 48];
    keys.copy_from_slice(bytes);
    let (ticket_key, port) = get_handle::<HttpsServer>(server_handle)
        .map(|server| (server.ticket_key.clone(), server.base.bound_port))
        .unwrap_or_else(|| {
            perry_ffi::throw_with_code(
                "setTicketKeys requires an HTTPS server",
                "ERR_INVALID_THIS",
                perry_ffi::ErrorKind::TypeError,
            )
        });
    if let Err(error) = ticket_key.set_keys(&keys) {
        perry_ffi::throw_with_code(&error, "ERR_TLS_TICKET_KEYS", perry_ffi::ErrorKind::Error);
    }
    // Perry exposes an opaque public session id alongside rustls' real cache.
    // Invalidate only identities for this receiving server so the facade tracks
    // the server-side ticket rotation without discarding unrelated sessions.
    if port != 0 {
        crate::agent::invalidate_tls_sessions_for_server_port(port);
    }
}

/// `httpsServer.listen(port?, host?, backlog?, cb?)` — binds + starts
/// accepting TLS-wrapped connections. `args_array` carries the variadic
/// `listen()` arguments; see `js_node_http_server_listen` / `parse_listen_args`
/// for the overload resolution. Issue #2041.
#[no_mangle]
pub unsafe extern "C" fn js_node_https_server_listen(server_handle: i64, args_array: i64) -> i64 {
    // Returns `server_handle` for chainability (#2129).
    let parsed = crate::server::types::parse_listen_args(args_array);
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
    let std_listener = match crate::server::cluster_bind::bind_listener(addr) {
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
    crate::server::cluster_bind::notify_listening(&host, actual_port);

    // Capture `noDelay` (default true) under the same handle lock as the TLS
    // config, so the accept loop can apply it per connection without re-locking
    // the handle map. Mirrors the HTTP/1 + HTTP/2 paths in server.rs.
    let no_delay;
    let (tls_config, certificate_cn) = if let Some(s) = get_handle_mut::<HttpsServer>(server_handle)
    {
        s.base.bound_port = actual_port;
        s.base.bound_host = host.clone();
        s.base.listening = true;
        s.base.shutdown_tx = Some(shutdown_tx);
        s.base.request_rx = Some(request_rx);
        no_delay = s.base.no_delay;
        (s.tls_config.clone(), s.certificate_cn.clone())
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
    crate::tls_client::register_internal_https_server(actual_port, certificate_cn);

    // TLS accept workers queue Rust request handles; JS callbacks run from
    // the main-thread HTTP pump, so listener lifetime is GC-safe.

    let request_tx = Arc::new(request_tx);
    let request_tx_for_spawn = request_tx.clone();
    let tls_config_for_spawn = tls_config;

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
                                crate::server::server::apply_accept_no_delay(&stream, no_delay);
                                let tls_config = tls_config_for_spawn.clone();
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
                                let rewrite_chunked_header = Arc::new(AtomicBool::new(false));
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
                                    let keylog = Arc::new(ConnectionKeyLog::default());
                                    let mut connection_config = (*tls_config).clone();
                                    connection_config.key_log = keylog.clone();
                                    let acceptor = TlsAcceptor::from(Arc::new(connection_config));
                                    let tls_stream = match acceptor.accept(stream).await {
                                        Ok(s) => s,
                                        Err(error) => {
                                            queue_tls_client_error(
                                                server_handle,
                                                format!("TLS handshake failed: {error}"),
                                            );
                                            CONNECTIONS.lock().unwrap().remove(&conn_id);
                                            return;
                                        }
                                    };
                                    let negotiated_servername = tls_stream
                                        .get_ref()
                                        .1
                                        .server_name()
                                        .map(String::from);
                                    queue_tls_keylog(server_handle, keylog.drain());
                                    // Track read activity on the DECRYPTED
                                    // stream — handshake bytes must not mark
                                    // a request-less socket non-idle (#4971).
                                    let io = TokioIo::new(ReadActivity::new(
                                        tls_stream,
                                        read_active.clone(),
                                        rewrite_chunked_header.clone(),
                                    ));
                                    let close_for_service = close.clone();
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        let busy = busy.clone();
                                        let read_active = read_active.clone();
                                        let connection_close = close_for_service.clone();
                                        let rewrite_chunked_header = rewrite_chunked_header.clone();
                                        let negotiated_servername = negotiated_servername.clone();
                                        async move {
                                            busy.fetch_add(1, Ordering::SeqCst);
                                            read_active.store(false, Ordering::SeqCst);
                                            let res = handle_https_request(
                                                server_handle,
                                                peer,
                                                req,
                                                request_tx,
                                                connection_close,
                                                rewrite_chunked_header,
                                                negotiated_servername,
                                            )
                                            .await;
                                            busy.fetch_sub(1, Ordering::SeqCst);
                                            res
                                        }
                                    });
                                    let mut builder = http1::Builder::new();
                                    builder.auto_date_header(false).title_case_headers(true);
                                    let conn = builder.serve_connection(io, service).with_upgrades();
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
        crate::server::server::queue_deferred_listening_emit(&mut s.base, callback);
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
    connection_close: Arc<tokio::sync::Notify>,
    rewrite_chunked_header: Arc<AtomicBool>,
    negotiated_servername: Option<String>,
) -> Result<Response<ResponseBody>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let url = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };
    let mut headers_lower = HashMap::new();
    let mut raw_headers = Vec::new();
    let trusted_internal = req
        .headers()
        .get("x-perry-internal-tls-token")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|token| {
            get_handle::<HttpsServer>(server_handle)
                .and_then(|server| {
                    crate::tls_client::internal_https_token_for_port(server.base.bound_port)
                })
                .is_some_and(|expected| token == expected)
        });
    let mut forwarded_servername: Option<Option<String>> = None;
    let mut forwarded_peer_cn: Option<String> = None;
    for (n, v) in req.headers() {
        if let Ok(vs) = v.to_str() {
            if trusted_internal
                && n.as_str()
                    .eq_ignore_ascii_case("x-perry-internal-tls-token")
            {
                continue;
            }
            if trusted_internal && n.as_str().eq_ignore_ascii_case("x-perry-tls-servername") {
                forwarded_servername = Some(if vs == "<false>" {
                    None
                } else {
                    Some(vs.to_string())
                });
                continue;
            }
            if trusted_internal && n.as_str().eq_ignore_ascii_case("x-perry-tls-peer-cn") {
                forwarded_peer_cn = Some(vs.to_string());
                continue;
            }
            headers_lower.insert(n.to_string().to_lowercase(), vs.to_string());
            raw_headers.push((n.to_string(), vs.to_string()));
        }
    }
    // #2132 — capture before `req` / `headers_lower` are consumed below.
    let http_version = req.version();
    let req_connection = headers_lower.get("connection").cloned();
    let req_te = headers_lower.get("te").cloned();
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
    crate::server::request::mark_incoming_tls(
        im_handle,
        forwarded_servername.unwrap_or(negotiated_servername),
    );
    crate::server::request::mark_incoming_peer_certificate(im_handle, forwarded_peer_cn);
    let (response_tx, response_rx) = oneshot::channel::<HyperResponseShape>();
    let transport_destroyed = Arc::new(AtomicBool::new(false));
    let sr_handle = alloc_server_response_for_request(
        response_tx,
        im_handle,
        Some(connection_close),
        Some(transport_destroyed.clone()),
    );
    let keep_alive_timeout = get_handle::<HttpsServer>(server_handle)
        .map(|server| server.base.keep_alive_timeout)
        .unwrap_or(5_000.0);
    let is_check_continue = expects_continue
        && get_handle::<HttpsServer>(server_handle).is_some_and(|server| {
            crate::server::server::server_has_event_listener(&server.base, "checkContinue")
        });
    let pending = HttpPendingRequest {
        server_handle,
        request_handle: im_handle,
        response_handle: sr_handle,
        skip_default_response: false,
        h2_stream_handle: 0,
        h2_stream_headers: Vec::new(),
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
            if http_version == hyper::Version::HTTP_10 {
                shape.response_version = Some(hyper::Version::HTTP_10);
            }
            let server_closing = get_handle::<HttpsServer>(server_handle)
                .map(|server| !server.base.listening)
                .unwrap_or(false);
            let default_connection = if server_closing {
                Some("close")
            } else {
                req_connection.as_deref()
            };
            shape.apply_default_connection_headers(
                http_version,
                default_connection,
                keep_alive_timeout,
            );
            let chunked = shape.apply_http10_chunked_framing(http_version, req_te.as_deref());
            if chunked {
                rewrite_chunked_header.store(true, Ordering::Release);
            } else {
                shape.apply_http10_eof_framing(http_version, req_te.as_deref());
            }
            Ok(shape.into_hyper())
        }
        Err(_) if transport_destroyed.load(Ordering::Acquire) => std::future::pending().await,
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
    // #6710 — clear a possibly-recycled handle id's per-handle JS side tables
    // before the handler observes req/res (see process_pending in server.rs).
    unsafe {
        crate::server::types::js_handle_clear_side_tables(pending.request_handle);
        crate::server::types::js_handle_clear_side_tables(pending.response_handle);
    }
    // #4903 — Node invokes `'request'` listeners (and the `createServer`
    // handler, which is one) with `this` bound to the server.
    let server_this = handle_to_pointer_f64(pending.server_handle);
    // #8082 (same as the HTTP path): the channel-parked snapshot's closure
    // addresses are copies no scanner rewrites — re-read them from the
    // scanner-maintained server handle at dispatch, then root the refreshed
    // values across the callbacks (each can run a moving collection). The
    // routing decision keeps the arrival-time `is_check_continue` snapshot.
    let (fresh_request_listeners, fresh_check_continue_listeners, fresh_handler) =
        match get_handle_mut::<HttpsServer>(pending.server_handle) {
            Some(server) if pending.is_check_continue => (
                Vec::new(),
                crate::server::server::take_server_event_listeners(
                    &mut server.base,
                    "checkContinue",
                ),
                server.handler,
            ),
            Some(server) => (
                crate::server::server::take_server_event_listeners(&mut server.base, "request"),
                Vec::new(),
                server.handler,
            ),
            None => (Vec::new(), Vec::new(), 0),
        };
    let scope = perry_ffi::TransientRootScope::enter();
    let check_continue_rooted = scope.root_addrs(&fresh_check_continue_listeners);
    let request_rooted = scope.root_addrs(&fresh_request_listeners);
    let handler_rooted = scope.root_addr(fresh_handler);
    // #5080 — an `Expect: 100-continue` request with a `'checkContinue'`
    // listener fires that listener instead of the `'request'` path.
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
        crate::server::server::finalize_or_park_request(&pending);
        return;
    }
    if handler_rooted.get() != 0 {
        unsafe {
            let raw = handler_rooted.get() as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
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
    // #4728 — an async handler (outbound `fetch()`, `setTimeout`, `await`
    // chain) returns before `res.end()` runs. Finalize now if the response
    // is already flushed, otherwise park it for the reaper instead of
    // synthesizing a premature empty response and freeing the handles out
    // from under the pending work.
    crate::server::server::finalize_or_park_request(&pending);
}

fn emit_keylog_lines(server_handle: i64, socket: f64, lines: &[Vec<u8>]) {
    if lines.is_empty() {
        return;
    }
    for line in lines {
        // Re-read persistent listeners for every record and drain once
        // listeners only for the first record. Snapshotting once for the
        // whole handshake would invoke `once('keylog')` repeatedly.
        let listeners = get_handle_mut::<HttpsServer>(server_handle)
            .map(|server| {
                crate::server::server::take_server_event_listeners(&mut server.base, "keylog")
            })
            .unwrap_or_default();
        let scope = perry_ffi::TransientRootScope::enter();
        let listeners = scope.root_addrs(&listeners);
        let line = perry_ffi::alloc_buffer(line);
        let line = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(line).bits()));
        for callback in &listeners {
            let callback = callback.get();
            if callback == 0 {
                continue;
            }
            unsafe {
                let closure = JsClosure::from_raw(callback as *const RawClosureHeader);
                if !closure.is_null() {
                    let _ = closure.call2(line.get(), socket);
                }
            }
        }
    }
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
    if let Some(s) = get_handle_mut::<HttpsServer>(handle) {
        crate::tls_client::unregister_internal_https_server(s.base.bound_port);
        s.base.listening = false;
        s.base.connections_checking_interval_destroyed = true;
        s.base.shutdown_tx.take();
        crate::server::server::queue_deferred_close_emit(&mut s.base, callback);
    }
    // Node 19+: `server.close()` destroys idle keep-alive connections
    // (active requests are allowed to finish) (#4905/#4971).
    signal_connections_close(handle, true);
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
    crate::server::server::js_node_http_server_close_all_connections(handle);
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

    use crate::server::server::apply_accept_no_delay;
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
