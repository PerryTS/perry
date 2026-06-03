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
    register_handle, JsClosure, JsValue, ObjectHeader, RawClosureHeader, StringHeader,
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

lazy_static! {
    static ref H2_PENDING_EVENTS: Mutex<Vec<Http2PendingEvent>> = Mutex::new(Vec::new());
}

static NEXT_H2_STREAM_ID: AtomicI64 = AtomicI64::new(1);

fn next_stream_id() -> i64 {
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
    pub session_type: i32,
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

enum Http2PendingEvent {
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

fn push_h2_event(event: Http2PendingEvent) {
    if let Ok(mut q) = H2_PENDING_EVENTS.lock() {
        q.push(event);
    }
    perry_ffi::notify_main_thread();
}

fn pairs_to_js_object(pairs: &[(String, String)]) -> f64 {
    let mut map = HashMap::new();
    for (key, value) in pairs {
        map.insert(key.clone(), value.clone());
    }
    map_to_js_object(&map)
}

fn map_to_js_object(map: &HashMap<String, String>) -> f64 {
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

fn empty_object_value() -> f64 {
    let text = alloc_string("{}");
    unsafe { f64::from_bits(js_json_parse(text.as_raw())) }
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}

fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn string_value(value: &str) -> f64 {
    let header = alloc_string(value);
    f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
}

fn settings_value(settings: &Http2SettingsState) -> f64 {
    let text = alloc_string(&settings.to_json());
    unsafe { f64::from_bits(js_json_parse(text.as_raw())) }
}

fn session_state_value(session: &Http2SessionHandle) -> f64 {
    let json = format!(
        "{{\"localWindowSize\":{},\"effectiveLocalWindowSize\":{},\"nextStreamID\":{},\"lastProcStreamID\":0,\"remoteWindowSize\":65535,\"outboundQueueSize\":0,\"deflateDynamicTableSize\":0,\"inflateDynamicTableSize\":0}}",
        session.local_window_size,
        session.local_window_size,
        if session.session_type == 1 { 1 } else { 2 }
    );
    let text = alloc_string(&json);
    unsafe { f64::from_bits(js_json_parse(text.as_raw())) }
}

fn buffer_value_from_bytes(bytes: &[u8]) -> f64 {
    let buf = alloc_buffer(bytes);
    if buf.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
    }
}

fn bind_handle_method(handle: i64, name: &'static [u8]) -> f64 {
    unsafe { js_class_method_bind(handle_to_pointer_f64(handle), name.as_ptr(), name.len()) }
}

fn closure_arg(value: Option<f64>) -> i64 {
    let Some(value) = value else { return 0 };
    let bits = value.to_bits();
    if unsafe { js_value_is_closure(bits as i64) } == 0 {
        return 0;
    }
    (bits & PTR_MASK) as i64
}

fn raw_event_name(value: f64) -> Option<String> {
    jsvalue_to_owned_string(value)
}

fn call0(callback: i64) {
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

fn call1(callback: i64, arg: f64) {
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

fn call2(callback: i64, arg0: f64, arg1: f64) {
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

fn call3(callback: i64, arg0: f64, arg1: f64, arg2: f64) {
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

fn register_server_session(server_handle: i64) -> i64 {
    let session_handle = register_handle(Http2SessionHandle {
        session_type: 0,
        encrypted: false,
        alpn_protocol: "h2c".to_string(),
        connecting: false,
        closed: false,
        destroyed: false,
        pending_settings_ack: true,
        authority: String::new(),
        local_settings: Http2SettingsState::default(),
        remote_settings: Http2SettingsState::default(),
        local_window_size: 65_535,
        sender: Arc::new(Mutex::new(None)),
        listeners: HashMap::new(),
        close_callbacks: Vec::new(),
        pending_callbacks: Vec::new(),
        timeout_callback: 0,
    });
    let has_session_listener = get_handle::<Http2SecureServer>(server_handle)
        .and_then(|s| s.base.listeners.get("session"))
        .map(|listeners| !listeners.is_empty())
        .unwrap_or(false);
    if has_session_listener {
        push_h2_event(Http2PendingEvent::Session {
            server_handle,
            session_handle,
        });
    }
    session_handle
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
    let std_listener = match std::net::TcpListener::bind(addr) {
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

    let (tls_config, plaintext) =
        if let Some(s) = get_handle_mut::<Http2SecureServer>(server_handle) {
            s.base.bound_port = actual_port;
            s.base.bound_host = host.clone();
            s.base.listening = true;
            s.base.shutdown_tx = Some(shutdown_tx);
            s.base.request_rx = Some(request_rx);
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

    // Bind `this` to the server for the `'listening'` listeners + the
    // optional `cb` so `this.address().port` resolves inside the listen
    // callback, matching Node (#2132).
    let this_val = handle_to_pointer_f64(server_handle);
    let listening_listeners = get_handle::<Http2SecureServer>(server_handle)
        .and_then(|s| s.base.listeners.get("listening").cloned())
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

    // Closes #604 — `listen()` is now non-blocking; the unified
    // `js_node_http_server_process_pending` pump in server.rs drains
    // HTTP/2 pending requests alongside HTTP/1 + HTTPS each tick.
    server_handle
}

async fn handle_h2_request(
    server_handle: i64,
    session_handle: i64,
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
    headers_lower.insert(":method".to_string(), method.clone());
    headers_lower.insert(":path".to_string(), url.clone());
    headers_lower.insert(":scheme".to_string(), "http".to_string());
    if let Some(authority) = uri.authority() {
        headers_lower.insert(":authority".to_string(), authority.to_string());
    }
    for (n, v) in req.headers() {
        if let Ok(vs) = v.to_str() {
            headers_lower.insert(n.to_string().to_lowercase(), vs.to_string());
            raw_headers.push((n.to_string(), vs.to_string()));
        }
    }
    let stream_headers = headers_lower.clone();
    let body = match req.collect().await {
        Ok(c) => c.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };
    let mut im = IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        body,
        peer.ip().to_string(),
        peer.port(),
    );
    im.http_version = "2.0".to_string();
    let im_handle = alloc_incoming_message(im);
    let (response_tx, response_rx) = oneshot::channel::<HyperResponseShape>();
    let (request_listeners, stream_listeners, handler) =
        match get_handle::<Http2SecureServer>(server_handle) {
            Some(s) => (
                s.base.listeners.get("request").cloned().unwrap_or_default(),
                s.base.listeners.get("stream").cloned().unwrap_or_default(),
                s.handler,
            ),
            None => (Vec::new(), Vec::new(), 0),
        };
    let has_stream_listener = !stream_listeners.is_empty();
    let (sr_handle, h2_stream_handle, h2_stream_headers) = if has_stream_listener {
        let (dummy_tx, _dummy_rx) = oneshot::channel::<HyperResponseShape>();
        let stream_handle = register_handle(Http2StreamHandle {
            session_handle,
            id: next_stream_id(),
            pending: false,
            closed: false,
            destroyed: false,
            aborted: false,
            rst_code: 0,
            headers_sent: false,
            sent_headers: Vec::new(),
            request_headers: stream_headers.clone(),
            listeners: HashMap::new(),
            encoding: None,
            response_tx: Some(response_tx),
            response_status: 200,
            response_headers: Vec::new(),
        });
        let headers_vec = stream_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        (
            alloc_server_response_for_request(dummy_tx, im_handle),
            stream_handle,
            headers_vec,
        )
    } else {
        (
            alloc_server_response_for_request(response_tx, im_handle),
            0,
            Vec::new(),
        )
    };
    let pending = HttpPendingRequest {
        server_handle,
        request_handle: im_handle,
        response_handle: sr_handle,
        skip_default_response: has_stream_listener,
        h2_stream_handle,
        h2_stream_headers,
        request_listeners,
        handler,
    };
    if request_tx.send(pending).await.is_err() {
        return Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from("Server unavailable")).boxed())
            .unwrap());
    }
    perry_ffi::notify_main_thread();
    match response_rx.await {
        Ok(shape) => Ok(shape.into_hyper()),
        Err(_) => Ok(Response::builder()
            .status(500)
            .body(Full::new(Bytes::from("Handler error")).boxed())
            .unwrap()),
    }
}

/// Non-blocking try_recv for HTTP/2 pending requests. Called by
/// `js_node_http_server_process_pending` in `server.rs` each tick.
pub(crate) fn try_recv_pending_h2_nonblocking(server_handle: i64) -> Option<HttpPendingRequest> {
    if let Some(s) = get_handle_mut::<Http2SecureServer>(server_handle) {
        if let Some(rx) = s.base.request_rx.as_mut() {
            return rx.try_recv().ok();
        }
    }
    None
}

/// Dispatch one HTTP/2 pending request. Per the issue #604
/// architectural change, we no longer block on the handler-returned
/// Promise.
pub(crate) fn process_pending_h2(pending: HttpPendingRequest) {
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
    if pending.h2_stream_handle != 0 {
        let stream_f64 = handle_to_pointer_f64(pending.h2_stream_handle);
        let headers_f64 = pairs_to_js_object(&pending.h2_stream_headers);
        let stream_listeners = get_handle::<Http2SecureServer>(pending.server_handle)
            .and_then(|s| s.base.listeners.get("stream").cloned())
            .unwrap_or_default();
        for cb in &stream_listeners {
            if *cb == 0 {
                continue;
            }
            unsafe {
                let raw = *cb as *const RawClosureHeader;
                let closure = JsClosure::from_raw(raw);
                if !closure.is_null() {
                    let _ = closure.call2(stream_f64, headers_f64);
                }
                js_promise_run_microtasks();
            }
        }
        synthesize_default_h2_stream_response(pending.h2_stream_handle);
    }
    if !pending.skip_default_response {
        synthesize_default_response_if_needed(pending.response_handle);
    }
    perry_ffi::drop_handle(pending.request_handle);
    perry_ffi::drop_handle(pending.response_handle);
}

fn synthesize_default_h2_stream_response(stream_handle: i64) {
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(stream_handle) {
        if stream.response_tx.is_none() {
            return;
        }
        stream.headers_sent = true;
        stream.closed = true;
        stream.destroyed = true;
        let mut headers = stream.response_headers.clone();
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        {
            headers.push(("Content-Length".to_string(), "0".to_string()));
        }
        let shape = HyperResponseShape {
            status: stream.response_status,
            status_message: None,
            headers,
            trailers: Vec::new(),
            body: Vec::new(),
        };
        if let Some(tx) = stream.response_tx.take() {
            let _ = tx.send(shape);
        }
    }
}

pub(crate) fn has_pending_h2_events() -> bool {
    H2_PENDING_EVENTS
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false)
}

pub(crate) fn has_active_h2_clients() -> bool {
    if has_pending_h2_events() {
        return true;
    }
    let mut active = false;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.session_type == 1 && !session.closed && !session.destroyed {
            active = true;
        }
    });
    active
}

pub(crate) fn process_pending_h2_events() -> i32 {
    let mut events: Vec<Http2PendingEvent> = match H2_PENDING_EVENTS.lock() {
        Ok(mut q) => q.drain(..).collect(),
        Err(_) => return 0,
    };
    events.sort_by_key(|event| match event {
        Http2PendingEvent::Session { .. } => 0,
        _ => 1,
    });
    let count = events.len() as i32;
    for event in events {
        match event {
            Http2PendingEvent::Session {
                server_handle,
                session_handle,
            } => {
                let listeners = get_handle::<Http2SecureServer>(server_handle)
                    .and_then(|s| s.base.listeners.get("session").cloned())
                    .unwrap_or_default();
                let arg = handle_to_pointer_f64(session_handle);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientConnect { session_handle } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get("connect").cloned())
                    .unwrap_or_default();
                for cb in listeners {
                    call0(cb);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientResponse {
                stream_handle,
                headers,
            } => {
                let listeners = get_handle::<Http2StreamHandle>(stream_handle)
                    .and_then(|s| s.listeners.get("response").cloned())
                    .unwrap_or_default();
                let arg = map_to_js_object(&headers);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientData {
                stream_handle,
                body,
            } => {
                let (listeners, encoding) = get_handle::<Http2StreamHandle>(stream_handle)
                    .map(|s| {
                        (
                            s.listeners.get("data").cloned().unwrap_or_default(),
                            s.encoding.clone(),
                        )
                    })
                    .unwrap_or_default();
                if !listeners.is_empty() && !body.is_empty() {
                    let arg = match encoding.as_deref() {
                        Some(_) => string_value(&String::from_utf8_lossy(&body)),
                        None => {
                            let buf = alloc_buffer(&body);
                            if buf.is_null() {
                                f64::from_bits(TAG_UNDEFINED)
                            } else {
                                f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
                            }
                        }
                    };
                    if arg.to_bits() != TAG_UNDEFINED {
                        for cb in listeners {
                            call1(cb, arg);
                            unsafe {
                                js_promise_run_microtasks();
                            }
                        }
                    }
                }
            }
            Http2PendingEvent::ClientEnd { stream_handle } => {
                let listeners = get_handle::<Http2StreamHandle>(stream_handle)
                    .and_then(|s| s.listeners.get("end").cloned())
                    .unwrap_or_default();
                for cb in listeners {
                    call0(cb);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientClose {
                session_handle,
                callback,
            } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get("close").cloned())
                    .unwrap_or_default();
                for cb in listeners {
                    call0(cb);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
                call0(callback);
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.close_callbacks.retain(|cb| *cb != callback);
                }
            }
            Http2PendingEvent::SessionSettingsEvent {
                session_handle,
                event,
                settings,
            } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get(event).cloned())
                    .unwrap_or_default();
                let arg = settings_value(&settings);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::SessionSettingsCallback {
                session_handle,
                callback,
                settings,
            } => {
                call2(callback, null_value(), settings_value(&settings));
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.pending_callbacks.retain(|cb| *cb != callback);
                    session.pending_settings_ack = false;
                }
                unsafe {
                    js_promise_run_microtasks();
                }
            }
            Http2PendingEvent::SessionPingCallback {
                session_handle,
                callback,
                payload,
            } => {
                call3(
                    callback,
                    null_value(),
                    0.0,
                    buffer_value_from_bytes(&payload),
                );
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.pending_callbacks.retain(|cb| *cb != callback);
                }
                unsafe {
                    js_promise_run_microtasks();
                }
            }
            Http2PendingEvent::SessionGoaway {
                session_handle,
                code,
                last_stream_id,
                opaque_data,
            } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get("goaway").cloned())
                    .unwrap_or_default();
                let opaque = buffer_value_from_bytes(&opaque_data);
                for cb in listeners {
                    call3(cb, code, last_stream_id, opaque);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientError { handle, message } => {
                let listeners = get_handle::<Http2SessionHandle>(handle)
                    .and_then(|s| s.listeners.get("error").cloned())
                    .or_else(|| {
                        get_handle::<Http2StreamHandle>(handle)
                            .and_then(|s| s.listeners.get("error").cloned())
                    })
                    .unwrap_or_default();
                let arg = string_value(&message);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
        }
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn js_node_http2_connect(
    authority_f64: f64,
    options_f64: f64,
    listener: i64,
) -> i64 {
    ensure_gc_scanner_registered();
    let authority =
        jsvalue_to_owned_string(authority_f64).unwrap_or_else(|| "http://localhost:80".to_string());
    let callback = if listener != 0 {
        listener
    } else {
        closure_arg(Some(options_f64))
    };
    let (host, port, host_port) = parse_authority(&authority);
    let sender_slot = Arc::new(Mutex::new(None));
    let mut listeners = HashMap::new();
    if callback != 0 {
        listeners
            .entry("connect".to_string())
            .or_insert_with(Vec::new)
            .push(callback);
    }
    let session_handle = register_handle(Http2SessionHandle {
        session_type: 1,
        encrypted: false,
        alpn_protocol: "h2c".to_string(),
        connecting: true,
        closed: false,
        destroyed: false,
        pending_settings_ack: true,
        authority: host_port,
        local_settings: Http2SettingsState::default(),
        remote_settings: Http2SettingsState::default(),
        local_window_size: 65_535,
        sender: sender_slot.clone(),
        listeners,
        close_callbacks: Vec::new(),
        pending_callbacks: Vec::new(),
        timeout_callback: 0,
    });

    perry_ffi::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create http2 client runtime");
        runtime.block_on(async move {
            let addr = format!("{}:{}", host, port);
            let stream = match tokio::net::TcpStream::connect(&addr).await {
                Ok(stream) => stream,
                Err(err) => {
                    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                        session.connecting = false;
                        session.closed = true;
                        session.destroyed = true;
                    }
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: session_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            let (sender, connection) = match h2::client::handshake(stream).await {
                Ok(parts) => parts,
                Err(err) => {
                    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                        session.connecting = false;
                        session.closed = true;
                        session.destroyed = true;
                    }
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: session_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            if let Ok(mut slot) = sender_slot.lock() {
                *slot = Some(sender);
            }
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                session.connecting = false;
            }
            push_h2_event(Http2PendingEvent::ClientConnect { session_handle });
            let _ = connection.await;
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                session.closed = true;
                session.destroyed = true;
                if let Ok(mut slot) = session.sender.lock() {
                    *slot = None;
                }
            }
        });
    });

    session_handle
}

fn parse_authority(authority: &str) -> (String, u16, String) {
    let without_scheme = authority
        .strip_prefix("http://")
        .or_else(|| authority.strip_prefix("https://"))
        .unwrap_or(authority);
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    if let Some(rest) = host_port.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = rest[..end].to_string();
            let port = rest[end + 1..]
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(80);
            return (host, port, host_port.to_string());
        }
    }
    let mut parts = host_port.rsplitn(2, ':');
    let maybe_port = parts.next().unwrap_or("");
    let maybe_host = parts.next();
    if let (Some(host), Ok(port)) = (maybe_host, maybe_port.parse::<u16>()) {
        (host.to_string(), port, host_port.to_string())
    } else {
        (host_port.to_string(), 80, host_port.to_string())
    }
}

fn parse_headers_object(value: f64) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let v = JsValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return out;
    }
    let Some(json) = perry_ffi::json_stringify(v) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) else {
        return out;
    };
    let Some(obj) = parsed.as_object() else {
        return out;
    };
    for (key, value) in obj {
        let value = value
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| value.to_string().trim_matches('"').to_string());
        out.insert(key.to_ascii_lowercase(), value);
    }
    out
}

fn start_client_request(stream_handle: i64, body: Vec<u8>) {
    let (session_handle, headers, sender_slot, authority) =
        match get_handle::<Http2StreamHandle>(stream_handle) {
            Some(stream) => {
                let session_handle = stream.session_handle;
                let Some(session) = get_handle::<Http2SessionHandle>(session_handle) else {
                    return;
                };
                (
                    session_handle,
                    stream.request_headers.clone(),
                    session.sender.clone(),
                    session.authority.clone(),
                )
            }
            None => return,
        };

    perry_ffi::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create http2 request runtime");
        runtime.block_on(async move {
            let sender = match sender_slot.lock().ok().and_then(|mut slot| slot.take()) {
                Some(sender) => sender,
                None => {
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: "HTTP/2 session is not connected".to_string(),
                    });
                    return;
                }
            };
            let mut sender = match sender.ready().await {
                Ok(sender) => sender,
                Err(err) => {
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };

            let method = headers
                .get(":method")
                .cloned()
                .unwrap_or_else(|| "GET".to_string());
            let path = headers
                .get(":path")
                .cloned()
                .unwrap_or_else(|| "/".to_string());
            let uri = format!("http://{}{}", authority, path);
            let mut builder = Request::builder().method(method.as_str()).uri(uri.as_str());
            for (name, value) in &headers {
                if name.starts_with(':') {
                    continue;
                }
                if let (Ok(header_name), Ok(header_value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    builder = builder.header(header_name, header_value);
                }
            }
            let mut request = match builder.body(()) {
                Ok(request) => request,
                Err(err) => {
                    if let Ok(mut slot) = sender_slot.lock() {
                        *slot = Some(sender);
                    }
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            *request.version_mut() = Version::HTTP_2;
            let end_of_stream = body.is_empty();
            let (response_future, mut send_stream) =
                match sender.send_request(request, end_of_stream) {
                    Ok(parts) => parts,
                    Err(err) => {
                        if let Ok(mut slot) = sender_slot.lock() {
                            *slot = Some(sender);
                        }
                        push_h2_event(Http2PendingEvent::ClientError {
                            handle: stream_handle,
                            message: err.to_string(),
                        });
                        return;
                    }
                };
            if !body.is_empty() {
                let _ = send_stream.send_data(Bytes::from(body), true);
            }
            if let Ok(mut slot) = sender_slot.lock() {
                *slot = Some(sender);
            }
            let response = match response_future.await {
                Ok(response) => response,
                Err(err) => {
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            let mut response_headers = HashMap::new();
            response_headers.insert(
                ":status".to_string(),
                response.status().as_u16().to_string(),
            );
            for (name, value) in response.headers() {
                if let Ok(value) = value.to_str() {
                    response_headers.insert(name.as_str().to_ascii_lowercase(), value.to_string());
                }
            }
            push_h2_event(Http2PendingEvent::ClientResponse {
                stream_handle,
                headers: response_headers,
            });
            let mut body = response.into_body();
            while let Some(chunk) = body.data().await {
                match chunk {
                    Ok(bytes) => {
                        push_h2_event(Http2PendingEvent::ClientData {
                            stream_handle,
                            body: bytes.to_vec(),
                        });
                    }
                    Err(err) => {
                        push_h2_event(Http2PendingEvent::ClientError {
                            handle: stream_handle,
                            message: err.to_string(),
                        });
                        return;
                    }
                }
            }
            let _ = session_handle;
            push_h2_event(Http2PendingEvent::ClientEnd { stream_handle });
        });
    });
}

fn numeric_value(value: f64) -> Option<f64> {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_int32() || v.is_number() {
        Some(v.to_number())
    } else {
        None
    }
}

fn queue_session_ping(handle: i64, args: &[f64]) -> f64 {
    let first_callback = args
        .first()
        .copied()
        .map(|v| closure_arg(Some(v)))
        .unwrap_or(0);
    let second_callback = args
        .get(1)
        .copied()
        .map(|v| closure_arg(Some(v)))
        .unwrap_or(0);
    let (callback, payload_value) = if second_callback != 0 {
        (second_callback, args.first().copied())
    } else {
        (first_callback, None)
    };
    if callback == 0 {
        return bool_value(false);
    }
    let mut payload = payload_value
        .and_then(jsvalue_to_body_bytes)
        .unwrap_or_else(|| vec![0; 8]);
    if payload.len() != 8 {
        payload.resize(8, 0);
        payload.truncate(8);
    }
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
        session.pending_callbacks.push(callback);
    }
    push_h2_event(Http2PendingEvent::SessionPingCallback {
        session_handle: handle,
        callback,
        payload,
    });
    bool_value(true)
}

fn queue_session_settings(handle: i64, args: &[f64]) -> f64 {
    let settings_value_arg = args
        .first()
        .copied()
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    let callback = args
        .get(1)
        .copied()
        .map(|v| closure_arg(Some(v)))
        .unwrap_or(0);
    let mut settings = get_handle::<Http2SessionHandle>(handle)
        .map(|session| session.local_settings.clone())
        .unwrap_or_default();
    settings.apply_value(settings_value_arg);
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
        session.local_settings = settings.clone();
        session.pending_settings_ack = true;
        if callback != 0 {
            session.pending_callbacks.push(callback);
        }
    }

    let caller_type = get_handle::<Http2SessionHandle>(handle)
        .map(|session| session.session_type)
        .unwrap_or(1);
    let peer_type = if caller_type == 1 { 0 } else { 1 };
    let mut peer_ids = Vec::new();
    iter_handle_ids_of::<Http2SessionHandle, _>(|peer_id| {
        if get_handle::<Http2SessionHandle>(peer_id)
            .map(|session| {
                session.session_type == peer_type && !session.closed && !session.destroyed
            })
            .unwrap_or(false)
        {
            peer_ids.push(peer_id);
        }
    });
    for peer_id in peer_ids {
        if let Some(session) = get_handle_mut::<Http2SessionHandle>(peer_id) {
            session.remote_settings = settings.clone();
            push_h2_event(Http2PendingEvent::SessionSettingsEvent {
                session_handle: peer_id,
                event: "remoteSettings",
                settings: settings.clone(),
            });
        }
    }
    if callback != 0 {
        push_h2_event(Http2PendingEvent::SessionSettingsCallback {
            session_handle: handle,
            callback,
            settings: settings.clone(),
        });
    }
    push_h2_event(Http2PendingEvent::SessionSettingsEvent {
        session_handle: handle,
        event: "localSettings",
        settings,
    });
    f64::from_bits(TAG_UNDEFINED)
}

fn queue_session_goaway(handle: i64, args: &[f64]) -> f64 {
    let code = args.first().and_then(|v| numeric_value(*v)).unwrap_or(0.0);
    let last_stream_id = args.get(1).and_then(|v| numeric_value(*v)).unwrap_or(0.0);
    let opaque_data = args
        .get(2)
        .copied()
        .and_then(jsvalue_to_body_bytes)
        .unwrap_or_default();
    let caller_type = get_handle::<Http2SessionHandle>(handle)
        .map(|session| session.session_type)
        .unwrap_or(1);
    let peer_type = if caller_type == 1 { 0 } else { 1 };
    let mut peer_ids = Vec::new();
    iter_handle_ids_of::<Http2SessionHandle, _>(|peer_id| {
        if get_handle::<Http2SessionHandle>(peer_id)
            .map(|session| {
                session.session_type == peer_type && !session.closed && !session.destroyed
            })
            .unwrap_or(false)
        {
            peer_ids.push(peer_id);
        }
    });
    for peer_id in peer_ids {
        push_h2_event(Http2PendingEvent::SessionGoaway {
            session_handle: peer_id,
            code,
            last_stream_id,
            opaque_data: opaque_data.clone(),
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_ext_http2_session_is_handle(handle: i64) -> i32 {
    if get_handle::<Http2SessionHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_ext_http2_stream_is_handle(handle: i64) -> i32 {
    if get_handle::<Http2StreamHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_session_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let method =
        String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len)).into_owned();
    let args = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let self_ref = handle_to_pointer_f64(handle);
    match method.as_str() {
        "request" => {
            let headers = args.first().copied().unwrap_or(undef);
            let request_headers = parse_headers_object(headers);
            let stream_handle = register_handle(Http2StreamHandle {
                session_handle: handle,
                id: next_stream_id(),
                pending: false,
                closed: false,
                destroyed: false,
                aborted: false,
                rst_code: 0,
                headers_sent: false,
                sent_headers: Vec::new(),
                request_headers,
                listeners: HashMap::new(),
                encoding: None,
                response_tx: None,
                response_status: 200,
                response_headers: Vec::new(),
            });
            handle_to_pointer_f64(stream_handle)
        }
        "on" | "addListener" if args.len() >= 2 => {
            if let Some(event) = raw_event_name(args[0]) {
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                    session
                        .listeners
                        .entry(event)
                        .or_default()
                        .push(closure_arg(Some(args[1])));
                }
            }
            self_ref
        }
        "close" => {
            let callback = closure_arg(args.first().copied());
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                session.closed = true;
                session.destroyed = true;
                if let Ok(mut slot) = session.sender.lock() {
                    *slot = None;
                }
                if callback != 0 {
                    session.close_callbacks.push(callback);
                }
            }
            push_h2_event(Http2PendingEvent::ClientClose {
                session_handle: handle,
                callback,
            });
            self_ref
        }
        "destroy" => {
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                session.closed = true;
                session.destroyed = true;
                if let Ok(mut slot) = session.sender.lock() {
                    *slot = None;
                }
            }
            self_ref
        }
        "ref" | "unref" => undef,
        "setLocalWindowSize" => {
            if let Some(window_size) = args.first().and_then(|v| numeric_value(*v)) {
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                    session.local_window_size = window_size as i64;
                }
            }
            undef
        }
        "setTimeout" => {
            let callback = args
                .get(1)
                .copied()
                .map(|v| closure_arg(Some(v)))
                .unwrap_or(0);
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                session.timeout_callback = callback;
            }
            self_ref
        }
        "ping" => queue_session_ping(handle, args),
        "settings" => queue_session_settings(handle, args),
        "goaway" => queue_session_goaway(handle, args),
        _ => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_session_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = String::from_utf8_lossy(std::slice::from_raw_parts(property_ptr, property_len))
        .into_owned();
    match property.as_str() {
        "request" => bind_handle_method(handle, b"request"),
        "on" => bind_handle_method(handle, b"on"),
        "addListener" => bind_handle_method(handle, b"addListener"),
        "close" => bind_handle_method(handle, b"close"),
        "destroy" => bind_handle_method(handle, b"destroy"),
        "ref" => bind_handle_method(handle, b"ref"),
        "unref" => bind_handle_method(handle, b"unref"),
        "setTimeout" => bind_handle_method(handle, b"setTimeout"),
        "setLocalWindowSize" => bind_handle_method(handle, b"setLocalWindowSize"),
        "ping" => bind_handle_method(handle, b"ping"),
        "settings" => bind_handle_method(handle, b"settings"),
        "goaway" => bind_handle_method(handle, b"goaway"),
        "type" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| s.session_type as f64)
            .unwrap_or(0.0),
        "encrypted" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.encrypted)
                .unwrap_or(false),
        ),
        "connecting" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.connecting)
                .unwrap_or(false),
        ),
        "closed" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.closed)
                .unwrap_or(false),
        ),
        "destroyed" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.destroyed)
                .unwrap_or(false),
        ),
        "alpnProtocol" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| string_value(&s.alpn_protocol))
            .unwrap_or(undef),
        "pendingSettingsAck" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.pending_settings_ack)
                .unwrap_or(false),
        ),
        "localSettings" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| settings_value(&s.local_settings))
            .unwrap_or_else(empty_object_value),
        "remoteSettings" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| settings_value(&s.remote_settings))
            .unwrap_or_else(empty_object_value),
        "state" => get_handle::<Http2SessionHandle>(handle)
            .map(session_state_value)
            .unwrap_or_else(empty_object_value),
        "socket" => empty_object_value(),
        _ => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_stream_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let method =
        String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len)).into_owned();
    let args = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let self_ref = handle_to_pointer_f64(handle);
    match method.as_str() {
        "on" | "addListener" if args.len() >= 2 => {
            if let Some(event) = raw_event_name(args[0]) {
                if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                    stream
                        .listeners
                        .entry(event)
                        .or_default()
                        .push(closure_arg(Some(args[1])));
                }
            }
            self_ref
        }
        "setEncoding" if !args.is_empty() => {
            if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                stream.encoding = jsvalue_to_owned_string(args[0]);
            }
            self_ref
        }
        "respond" if !args.is_empty() => {
            let headers = parse_headers_object(args[0]);
            if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                stream.headers_sent = true;
                stream.sent_headers = headers
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                stream.response_status = headers
                    .get(":status")
                    .and_then(|status| status.parse::<u16>().ok())
                    .unwrap_or(200);
                stream.response_headers = headers
                    .iter()
                    .filter(|(name, _)| !name.starts_with(':'))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
            }
            self_ref
        }
        "end" => {
            let body = args
                .first()
                .copied()
                .and_then(jsvalue_to_body_bytes)
                .unwrap_or_default();
            let is_server_stream = get_handle::<Http2StreamHandle>(handle)
                .and_then(|stream| {
                    get_handle::<Http2SessionHandle>(stream.session_handle)
                        .map(|session| session.session_type == 0)
                })
                .unwrap_or(false);
            if is_server_stream {
                end_server_h2_stream(handle, body);
            } else {
                start_client_request(handle, body);
            }
            self_ref
        }
        "close" => {
            if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                stream.closed = true;
                stream.destroyed = true;
            }
            self_ref
        }
        "setTimeout" | "priority" | "additionalHeaders" | "pushStream" | "respondWithFD"
        | "respondWithFile" | "sendTrailers" => self_ref,
        _ => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_stream_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = String::from_utf8_lossy(std::slice::from_raw_parts(property_ptr, property_len))
        .into_owned();
    match property.as_str() {
        "on" => bind_handle_method(handle, b"on"),
        "addListener" => bind_handle_method(handle, b"addListener"),
        "setEncoding" => bind_handle_method(handle, b"setEncoding"),
        "respond" => bind_handle_method(handle, b"respond"),
        "end" => bind_handle_method(handle, b"end"),
        "close" => bind_handle_method(handle, b"close"),
        "setTimeout" => bind_handle_method(handle, b"setTimeout"),
        "priority" => bind_handle_method(handle, b"priority"),
        "additionalHeaders" => bind_handle_method(handle, b"additionalHeaders"),
        "pushStream" => bind_handle_method(handle, b"pushStream"),
        "respondWithFD" => bind_handle_method(handle, b"respondWithFD"),
        "respondWithFile" => bind_handle_method(handle, b"respondWithFile"),
        "sendTrailers" => bind_handle_method(handle, b"sendTrailers"),
        "id" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| s.id as f64)
            .unwrap_or(0.0),
        "pending" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.pending)
                .unwrap_or(false),
        ),
        "closed" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.closed)
                .unwrap_or(false),
        ),
        "destroyed" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.destroyed)
                .unwrap_or(false),
        ),
        "aborted" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.aborted)
                .unwrap_or(false),
        ),
        "rstCode" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| s.rst_code as f64)
            .unwrap_or(0.0),
        "headersSent" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.headers_sent)
                .unwrap_or(false),
        ),
        "sentHeaders" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| pairs_to_js_object(&s.sent_headers))
            .unwrap_or(undef),
        "session" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| handle_to_pointer_f64(s.session_handle))
            .unwrap_or(undef),
        "state" => empty_object_value(),
        "bufferSize" => 0.0,
        "endAfterHeaders" => bool_value(false),
        _ => undef,
    }
}

fn end_server_h2_stream(handle: i64, body: Vec<u8>) {
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
        stream.closed = true;
        stream.destroyed = true;
        stream.headers_sent = true;
        let mut headers = stream.response_headers.clone();
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        {
            headers.push(("Content-Length".to_string(), body.len().to_string()));
        }
        let shape = HyperResponseShape {
            status: stream.response_status,
            status_message: None,
            headers,
            trailers: Vec::new(),
            body,
        };
        if let Some(tx) = stream.response_tx.take() {
            let _ = tx.send(shape);
        }
    }
}

/// `http2SecureServer.address()`.
#[no_mangle]
pub extern "C" fn js_node_http2_server_address_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<Http2SecureServer>(handle)
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

/// `http2SecureServer.close(cb?)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_server_close(handle: i64, callback: i64) {
    let close_listeners;
    if let Some(s) = get_handle_mut::<Http2SecureServer>(handle) {
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

/// `http2SecureServer.on(event, cb)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_server_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if let Some(s) = get_handle_mut::<Http2SecureServer>(handle) {
        s.base.listeners.entry(event).or_default().push(callback);
    }
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}
