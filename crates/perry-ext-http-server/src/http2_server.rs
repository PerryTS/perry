//! `http2.createSecureServer({ key, cert }, handler)` — Phase 3.
//!
//! Implementation strategy: the same `IncomingMessage` /
//! `ServerResponse` types that Phase 1 introduced are reused as
//! `Http2ServerRequest` / `Http2ServerResponse`. hyper's
//! `hyper-util::server::conn::auto::Builder` performs ALPN
//! negotiation on the rustls-wrapped stream, so HTTP/1.1 and HTTP/2
//! coexist on the same port. Phase 1's request-buffering model
//! works unchanged for HTTP/2 streams (each `:path` request becomes
//! a single buffered IncomingMessage).
//!
//! Server push (`response.createPushResponse`) is **not** implemented —
//! the Node.js docs deprecate it and modern frameworks have moved
//! away from it. RFC 8441 (WebSockets over HTTP/2) is also out of
//! scope; the upgrade path stays HTTP/1.1-only.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{HeaderName, HeaderValue};
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response, Version};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use lazy_static::lazy_static;
use perry_ffi::{
    alloc_buffer, alloc_string, get_handle, get_handle_mut, iter_handle_ids_of, iter_handles_of,
    iter_handles_of_mut, register_handle, JsClosure, JsValue, ObjectHeader, RawClosureHeader,
    StringHeader,
};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsAcceptor;

use crate::ensure_gc_scanner_registered;
use crate::http2_session_settings::Http2SettingsState;
use crate::request::{
    alloc_incoming_message, emit_no_arg_to_listeners, handle_to_pointer_f64, with_implicit_this,
    IncomingMessage,
};
use crate::response::{alloc_server_response_for_request, HyperResponseShape, ResponseBody};
use crate::server::{synthesize_default_response_if_needed, HttpPendingRequest, HttpServer};
use crate::tls::{
    build_server_config, has_pem_material, json_value_to_pem_bytes, parse_cert_chain,
    parse_private_key,
};
use crate::types::{
    extract_host, extract_port, js_promise_run_microtasks, js_value_is_closure,
    jsvalue_to_body_bytes, jsvalue_to_owned_string, read_string_header, POINTER_TAG, PTR_MASK,
    STRING_TAG, TAG_NULL, TAG_UNDEFINED,
};

extern "C" {
    fn js_json_parse(text_ptr: *const StringHeader) -> u64;
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;
}

mod controls;
pub(crate) mod dispatch;
mod pump;
mod session;

pub(crate) use controls::{
    numeric_value, queue_session_goaway, queue_session_ping, queue_session_settings,
};
pub(crate) use pump::{
    has_active_h2_clients, has_pending_h2_events, process_pending_h2, process_pending_h2_events,
    try_recv_pending_h2_nonblocking,
};
pub(crate) use session::{
    h2_listening_server_for_authority, local_client_connect_ready, local_server_handle_for_client,
    mark_server_sessions_closed, mark_session_closed, parse_headers_object,
    register_server_session, start_client_request,
};

// `handle_h2_request` is consumed by `js_node_http2_server_listen` below.
use pump::handle_h2_request;

lazy_static! {
    pub(crate) static ref H2_PENDING_EVENTS: Mutex<Vec<Http2PendingEvent>> = Mutex::new(Vec::new());
}

static NEXT_H2_STREAM_ID: AtomicI64 = AtomicI64::new(1);

pub(crate) fn next_stream_id() -> i64 {
    NEXT_H2_STREAM_ID.fetch_add(2, Ordering::SeqCst)
}

/// Decode `{ key, cert }` from a NaN-boxed JsValue object. Mirrors
/// the helper in `https_server.rs` (including Buffer-typed PEM
/// support, #2132) but omits the alpnProtocols flag since http2
/// server always advertises `[h2, http/1.1]`.
unsafe fn parse_h2_opts(opts_f64: f64) -> (Vec<u8>, Vec<u8>) {
    use perry_ffi::JsValue;
    let v = JsValue::from_bits(opts_f64.to_bits());
    if !v.is_pointer() {
        return (Vec::new(), Vec::new());
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return (Vec::new(), Vec::new()),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), Vec::new()),
    };
    let key_pem = json_value_to_pem_bytes(parsed.get("key"));
    let cert_pem = json_value_to_pem_bytes(parsed.get("cert"));
    (key_pem, cert_pem)
}
/// Backing struct for `http2.Http2SecureServer` JS-side handle.
pub struct Http2SecureServer {
    pub handler: i64,
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
    pub plaintext: bool,
    pub base: HttpServer,
}

pub struct Http2SessionHandle {
    pub server_handle: i64,
    pub session_event_emitted: bool,
    pub session_type: i32,
    pub connected: bool,
    pub encrypted: bool,
    pub alpn_protocol: String,
    pub connecting: bool,
    pub closed: bool,
    pub destroyed: bool,
    pub pending_settings_ack: bool,
    pub authority: String,
    pub local_settings: Http2SettingsState,
    pub remote_settings: Http2SettingsState,
    pub local_window_size: i64,
    pub sender: Arc<Mutex<Option<h2::client::SendRequest<Bytes>>>>,
    pub listeners: HashMap<String, Vec<i64>>,
    pub close_callbacks: Vec<i64>,
    pub pending_callbacks: Vec<i64>,
    pub timeout_callback: i64,
}

pub struct Http2StreamHandle {
    pub session_handle: i64,
    pub id: i64,
    pub pending: bool,
    pub closed: bool,
    pub destroyed: bool,
    pub aborted: bool,
    pub rst_code: i32,
    pub headers_sent: bool,
    pub sent_headers: Vec<(String, String)>,
    pub request_headers: HashMap<String, String>,
    pub listeners: HashMap<String, Vec<i64>>,
    pub encoding: Option<String>,
    pub response_tx: Option<oneshot::Sender<HyperResponseShape>>,
    pub response_status: u16,
    pub response_headers: Vec<(String, String)>,
}

pub(crate) enum Http2PendingEvent {
    Session {
        server_handle: i64,
        session_handle: i64,
    },
    ClientConnect {
        session_handle: i64,
    },
    ClientResponse {
        stream_handle: i64,
        headers: HashMap<String, String>,
    },
    ClientData {
        stream_handle: i64,
        body: Vec<u8>,
    },
    ClientEnd {
        stream_handle: i64,
    },
    ClientClose {
        session_handle: i64,
        callback: i64,
    },
    SessionSettingsEvent {
        session_handle: i64,
        event: &'static str,
        settings: Http2SettingsState,
    },
    SessionSettingsCallback {
        session_handle: i64,
        callback: i64,
        settings: Http2SettingsState,
    },
    SessionPingCallback {
        session_handle: i64,
        callback: i64,
        payload: Vec<u8>,
    },
    SessionGoaway {
        session_handle: i64,
        code: f64,
        last_stream_id: f64,
        opaque_data: Vec<u8>,
    },
    ClientError {
        handle: i64,
        message: String,
    },
}

pub(crate) fn push_h2_event(event: Http2PendingEvent) {
    if let Ok(mut q) = H2_PENDING_EVENTS.lock() {
        q.push(event);
    }
    perry_ffi::notify_main_thread();
}

pub(crate) fn pairs_to_js_object(pairs: &[(String, String)]) -> f64 {
    let mut map = HashMap::new();
    for (key, value) in pairs {
        map.insert(key.clone(), value.clone());
    }
    map_to_js_object(&map)
}

pub(crate) fn map_to_js_object(map: &HashMap<String, String>) -> f64 {
    let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
    let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
    let obj: *mut ObjectHeader = unsafe {
        perry_ffi::js_object_alloc_with_shape(
            shape_id,
            keys.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    for (i, key) in keys.iter().enumerate() {
        if let Some(value) = map.get(*key) {
            let str_value = alloc_string(value);
            let js_value = JsValue::from_string_ptr(str_value.as_raw());
            unsafe {
                perry_ffi::js_object_set_field(obj, i as u32, js_value);
            }
        }
    }
    f64::from_bits(JsValue::from_object_ptr(obj as *mut u8).bits())
}

pub(crate) fn empty_object_value() -> f64 {
    let text = alloc_string("{}");
    unsafe { f64::from_bits(js_json_parse(text.as_raw())) }
}

pub(crate) fn bool_value(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}

pub(crate) fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

pub(crate) fn string_value(value: &str) -> f64 {
    let header = alloc_string(value);
    f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
}

pub(crate) fn settings_value(settings: &Http2SettingsState) -> f64 {
    let text = alloc_string(&settings.to_json());
    unsafe { f64::from_bits(js_json_parse(text.as_raw())) }
}

pub(crate) fn session_state_value(session: &Http2SessionHandle) -> f64 {
    let json = format!(
        "{{\"localWindowSize\":{},\"effectiveLocalWindowSize\":{},\"nextStreamID\":{},\"lastProcStreamID\":0,\"remoteWindowSize\":65535,\"outboundQueueSize\":0,\"deflateDynamicTableSize\":0,\"inflateDynamicTableSize\":0}}",
        session.local_window_size,
        session.local_window_size,
        if session.session_type == 1 { 1 } else { 2 }
    );
    let text = alloc_string(&json);
    unsafe { f64::from_bits(js_json_parse(text.as_raw())) }
}

pub(crate) fn buffer_value_from_bytes(bytes: &[u8]) -> f64 {
    let buf = alloc_buffer(bytes);
    if buf.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
    }
}

pub(crate) fn bind_handle_method(handle: i64, name: &'static [u8]) -> f64 {
    unsafe { js_class_method_bind(handle_to_pointer_f64(handle), name.as_ptr(), name.len()) }
}

pub(crate) fn closure_arg(value: Option<f64>) -> i64 {
    let Some(value) = value else { return 0 };
    let bits = value.to_bits();
    if unsafe { js_value_is_closure(bits as i64) } == 0 {
        return 0;
    }
    (bits & PTR_MASK) as i64
}

pub(crate) fn raw_event_name(value: f64) -> Option<String> {
    jsvalue_to_owned_string(value)
}

pub(crate) fn call0(callback: i64) {
    if callback == 0 {
        return;
    }
    unsafe {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}

pub(crate) fn call1(callback: i64, arg: f64) {
    if callback == 0 {
        return;
    }
    unsafe {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call1(arg);
        }
    }
}

pub(crate) fn call2(callback: i64, arg0: f64, arg1: f64) {
    if callback == 0 {
        return;
    }
    unsafe {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call2(arg0, arg1);
        }
    }
}

pub(crate) fn call3(callback: i64, arg0: f64, arg1: f64, arg2: f64) {
    if callback == 0 {
        return;
    }
    unsafe {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call3(arg0, arg1, arg2);
        }
    }
}

/// `http2.createSecureServer(opts, handler)` — opts carries `{ key, cert }`
/// PEM strings + the usual handler closure. ALPN advertises both
/// `h2` and `http/1.1` so non-HTTP/2 clients are still served (matches
/// Node's behavior with `allowHTTP1: true`, default in Node 14+).
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_create_secure_server(opts_f64: f64, handler: i64) -> i64 {
    ensure_gc_scanner_registered();

    let (key_pem, cert_pem) = parse_h2_opts(opts_f64);
    let cert_chain = parse_cert_chain(&cert_pem);
    let has_tls_material = has_pem_material(&key_pem, &cert_pem);
    let private_key = parse_private_key(&key_pem);

    let tls_config = match private_key {
        Some(k) => match build_server_config(cert_chain, k, true) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("[node:http2] {}", e);
                None
            }
        },
        None => {
            if has_tls_material {
                eprintln!("[node:http2] no recognized PEM private key");
            }
            None
        }
    };

    register_handle(Http2SecureServer {
        handler,
        tls_config,
        plaintext: false,
        base: HttpServer::with_handler(handler),
    })
}

/// `http2.createServer([options][, handler])` — plaintext h2c server.
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_create_server(first_arg: f64, second_arg: f64) -> i64 {
    ensure_gc_scanner_registered();
    let first_bits = first_arg.to_bits();
    let second_bits = second_arg.to_bits();
    let handler = if js_value_is_closure(first_bits as i64) != 0 {
        (first_bits & PTR_MASK) as i64
    } else if js_value_is_closure(second_bits as i64) != 0 {
        (second_bits & PTR_MASK) as i64
    } else {
        0
    };

    register_handle(Http2SecureServer {
        handler,
        tls_config: None,
        plaintext: true,
        base: HttpServer::with_handler(handler),
    })
}

/// `http2SecureServer.listen(port?, host?, backlog?, cb?)`. `args_array`
/// carries the variadic `listen()` arguments; see `js_node_http_server_listen`
/// / `parse_listen_args` for the overload resolution. Issue #2041.
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_server_listen(server_handle: i64, args_array: i64) -> i64 {
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

    // #2132 — synchronous bind so `server.address().port` is correct
    // before the `listen(port, cb)` callback fires. See
    // `server::js_node_http_server_listen` for the rationale.
    let bind_str = format!("{}:{}", host, port);
    let addr: SocketAddr = match bind_str.parse() {
        Ok(a) => a,
        Err(_) => SocketAddr::from(([0, 0, 0, 0], port)),
    };
    // #4914 — SO_REUSEPORT in cluster workers; plain bind otherwise.
    let std_listener = match crate::cluster_bind::bind_listener(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[node:http2] bind {}:{} failed: {}", host, port, e);
            return server_handle;
        }
    };
    let actual_port = std_listener.local_addr().map(|a| a.port()).unwrap_or(port);
    if let Err(e) = std_listener.set_nonblocking(true) {
        eprintln!("[node:http2] set_nonblocking failed: {}", e);
        return server_handle;
    }
    crate::cluster_bind::notify_listening(&host, actual_port);

    // Capture `noDelay` (default true) under the same handle lock as the TLS
    // config so the accept loop can apply it per connection. Mirrors the HTTP/1
    // path in server.rs and the HTTPS path in https_server.rs.
    let no_delay;
    let (tls_config, plaintext) =
        if let Some(s) = get_handle_mut::<Http2SecureServer>(server_handle) {
            s.base.bound_port = actual_port;
            s.base.bound_host = host.clone();
            s.base.listening = true;
            s.base.shutdown_tx = Some(shutdown_tx);
            s.base.request_rx = Some(request_rx);
            no_delay = s.base.no_delay;
            (s.tls_config.clone(), s.plaintext)
        } else {
            return server_handle;
        };

    let tls_config = if plaintext {
        None
    } else {
        match tls_config {
            Some(c) => Some(c),
            None => {
                eprintln!("[node:http2] tls config unavailable; refusing to listen");
                return server_handle;
            }
        }
    };

    // HTTP/2 accept workers queue Rust request handles; JS callbacks run from
    // the main-thread HTTP pump, so listener lifetime is GC-safe.

    let request_tx = Arc::new(request_tx);
    let request_tx_for_spawn = request_tx.clone();
    let acceptor = tls_config.map(TlsAcceptor::from);

    // Issue #577 Phase 3 — `tokio::spawn` from inside
    // `spawn_blocking_with_reactor`'s closure panics with
    // "no reactor running" specifically on the http2 binary because
    // the auto::Builder dep set somehow ends up with the ambient
    // tokio runtime context unset by the time the closure runs.
    // Workaround: use `perry_ffi::spawn_blocking` (no reactor) +
    // `Handle::current().block_on` — same pattern perry-ext-fastify
    // uses. The plain spawn_blocking variant runs the closure on a
    // tokio blocking-pool thread that does NOT have a runtime
    // context, so calling `block_on(fut)` is legal there (it spins
    // up a fresh current_thread runtime to drive the future). The
    // I/O reactor IS available because the inner runtime is built
    // with `enable_all`.
    perry_ffi::spawn_blocking(move || {
        let handle = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create http2 accept-loop runtime");
        handle.block_on(async move {
            let listener = match TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[node:http2] tokio adopt failed: {}", e);
                    return;
                }
            };
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, peer)) => {
                                // Node default: TCP_NODELAY on. Honor the
                                // server's `noDelay` on the raw TCP socket here,
                                // before the TLS or h2c branch — the option
                                // persists through any wrapping.
                                crate::server::apply_accept_no_delay(&stream, no_delay);
                                let acceptor = acceptor.clone();
                                let request_tx = request_tx_for_spawn.clone();
                                tokio::spawn(async move {
                                    let session_handle = register_server_session(server_handle);
                                    match acceptor {
                                        Some(acceptor) => {
                                            let tls_stream = match acceptor.accept(stream).await {
                                                Ok(s) => s,
                                                Err(e) => {
                                                    eprintln!("[node:http2] tls handshake: {}", e);
                                                    mark_session_closed(session_handle);
                                                    return;
                                                }
                                            };
                                            let io = TokioIo::new(tls_stream);
                                            let service = service_fn(move |req: Request<Incoming>| {
                                                let request_tx = request_tx.clone();
                                                async move {
                                                    handle_h2_request(server_handle, session_handle, peer, req, request_tx).await
                                                }
                                            });
                                            if let Err(e) = AutoBuilder::new(TokioExecutor::new())
                                                .serve_connection(io, service)
                                                .await
                                            {
                                                let _ = e;
                                            }
                                        }
                                        None => {
                                            let io = TokioIo::new(stream);
                                            let service = service_fn(move |req: Request<Incoming>| {
                                                let request_tx = request_tx.clone();
                                                async move {
                                                    handle_h2_request(server_handle, session_handle, peer, req, request_tx).await
                                                }
                                            });
                                            if let Err(e) = AutoBuilder::new(TokioExecutor::new())
                                                .serve_connection(io, service)
                                                .await
                                            {
                                                let _ = e;
                                            }
                                        }
                                    }
                                    mark_session_closed(session_handle);
                                });
                            }
                            Err(e) => eprintln!("[node:http2] accept error: {}", e),
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
    if let Some(s) = get_handle_mut::<Http2SecureServer>(server_handle) {
        crate::server::queue_deferred_listening_emit(&mut s.base, callback);
    }

    // Closes #604 — `listen()` is now non-blocking; the unified
    // `js_node_http_server_process_pending` pump in server.rs drains
    // HTTP/2 pending requests alongside HTTP/1 + HTTPS each tick.
    server_handle
}
