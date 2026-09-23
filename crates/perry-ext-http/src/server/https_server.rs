//! `https.createServer({ key, cert }, handler)` — TLS variant of
//! `http.createServer`. Re-uses the Phase 1 IncomingMessage /
//! ServerResponse / event-loop machinery. The turnloop connection layer
//! installs a rustls server session (perry-ext-net's `turnloop_tls_io`) on
//! every accepted connection before any HTTP byte is decoded.

use std::sync::Arc;

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};

use crate::server::ensure_gc_scanner_registered;
use crate::server::request::{handle_to_pointer_f64, with_implicit_this};
use crate::server::server::{
    sanitize_request_timeout, signal_connections_close, HttpPendingRequest, HttpServer,
};
use crate::server::tls::{
    build_certless_server_config, build_server_config, has_pem_material, json_value_to_pem_bytes,
    parse_cert_chain, parse_private_key, NodeTicketKey,
};

/// Decode `{ key, cert, alpnProtocols? }` from a NaN-boxed JsValue
/// object literal into the PEM byte buffers + a flag for whether to
/// advertise `h2` in ALPN. `key`/`cert` accept either a PEM string
/// OR a `Buffer` (the form `fs.readFileSync('key.pem')` returns when
/// no encoding is supplied) — see `json_value_to_pem_bytes`. Falls
/// back to empty PEMs (which the cert-chain parser then rejects) on
/// any extraction failure so the user sees a clear bind error.
unsafe fn parse_https_opts(opts_f64: f64) -> (Vec<u8>, Vec<u8>, bool, Option<Vec<u8>>, i64, u32) {
    use perry_ffi::JsValue;
    let mut v = JsValue::from_bits(opts_f64.to_bits());
    if !v.is_pointer_or_raw() {
        return (Vec::new(), Vec::new(), true, Some(default_alpn()), 0, 300);
    }
    // Native-call lowering may pass a heap options object as its legacy raw
    // pointer. `json_stringify` expects the canonical POINTER_TAG shape.
    if !v.is_pointer() {
        v = JsValue::from_object_ptr((v.bits() & PTR_MASK) as *mut u8);
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return (Vec::new(), Vec::new(), true, Some(default_alpn()), 0, 300),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), Vec::new(), true, Some(default_alpn()), 0, 300),
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
    let session_timeout = parsed
        .get("sessionTimeout")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(300);
    (
        key_pem,
        cert_pem,
        enable_h2,
        alpn_protocols,
        alpn_callback,
        session_timeout,
    )
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

    let (key_pem, cert_pem, enable_http2_alpn, alpn_protocols, alpn_callback, session_timeout) =
        parse_https_opts(opts_f64);
    let mut base = HttpServer::with_handler(handler);
    crate::server::server::apply_server_options(&mut base, opts_f64);
    let ticket_key = NodeTicketKey::random(session_timeout).unwrap_or_else(|error| {
        eprintln!("[node:https] {error}; TLS session tickets disabled");
        NodeTicketKey::disabled(session_timeout)
    });

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
    listen_https_server(
        server_handle,
        crate::server::types::parse_listen_args(args_array),
    )
}

pub(super) unsafe fn listen_https_server(
    server_handle: i64,
    parsed: crate::server::types::ListenArgs,
) -> i64 {
    // Returns `server_handle` for chainability (#2129).
    let opts_f64 = parsed.opts;
    let port = extract_port(opts_f64, 443);
    let host = parsed
        .host
        .unwrap_or_else(|| extract_host(opts_f64, "0.0.0.0"));
    let callback = parsed.callback;

    // P5: bind and accept on the agent's turnloop loop, with the server's
    // rustls configuration driven through perry-ext-net's unbuffered session
    // (`turnloop_tls_io`). A thread acting for an agent another thread owns
    // posts the bind to the owner, exactly as `http.Server.listen` does.
    if crate::server::turnloop_serve::enabled() {
        if turnloop_https_listen(server_handle, &host, port) {
            finish_https_listen(server_handle, callback);
        }
        return server_handle;
    }
    if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
        crate::server::server::register_listen_callback(&mut s.base, callback);
    }
    let job_host = host.clone();
    let posted = crate::server::turnloop_serve::post_to_owner(Box::new(move || {
        if crate::server::turnloop_serve::enabled()
            && turnloop_https_listen(server_handle, &job_host, port)
        {
            finish_https_listen(server_handle, 0);
        } else if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
            crate::server::server::withdraw_listen_callbacks(&mut s.base);
        }
    }));
    if !posted {
        eprintln!(
            "[node:https] bind {}:{} failed: {}",
            host,
            port,
            crate::server::turnloop_serve::NO_LOOP_CODE
        );
        if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
            crate::server::server::withdraw_listen_callbacks(&mut s.base);
        }
    }

    // Closes #604 — `listen()` is non-blocking. Pending requests are drained
    // via the unified `js_node_http_server_process_pending` pump in
    // `server.rs`, which iterates HTTP/1, HTTPS, and HTTP/2 handles each tick.
    server_handle
}

/// A listen that succeeded: create the server's async resource and queue the
/// deferred `'listening'` emit + the optional `cb` for the main-thread pump.
/// Node emits `'listening'` on a later tick, after `const server = ...` has
/// been assigned (#4903); the pump binds `this` to the server when it fires
/// them (#2132). See `server::drain_deferred_listen_for`.
fn finish_https_listen(server_handle: i64, callback: i64) {
    let server_async_id = unsafe {
        crate::js_async_hooks_provider_init(b"TCPSERVERWRAP".as_ptr(), b"TCPSERVERWRAP".len())
    };
    if let Some(s) = get_handle_mut::<HttpsServer>(server_handle) {
        s.base.async_id = server_async_id;
        crate::server::server::queue_deferred_listening_emit(&mut s.base, callback);
    } else {
        unsafe { crate::js_async_hooks_provider_destroy(server_async_id) };
    }
}

/// Bind `host:port` on this thread's turnloop loop and start accepting TLS
/// connections. Returns whether the server is now listening; a failure is
/// reported on stderr, as the HTTPS path always has.
///
/// A cluster worker binds with `ReusePort::Share` (turnloop 0.1.0-alpha.6),
/// which is what `SO_REUSEPORT` by hand used to do; see
/// `server::turnloop_listen::try_listen_on_turnloop` for why it is `Share`
/// and not `Distribute`. `https.createServer` has no SCHED_RR descriptor
/// path — only `http.createServer` does — so a SCHED_RR worker binds here too.
fn turnloop_https_listen(server_handle: i64, host: &str, port: u16) -> bool {
    let (tls_config, certificate_cn, no_delay, idle_close_ms) =
        match get_handle::<HttpsServer>(server_handle) {
            Some(s) => (
                s.tls_config.clone(),
                s.certificate_cn.clone(),
                s.base.no_delay,
                crate::server::server::idle_close_ms(&s.base),
            ),
            None => return false,
        };
    let Some(tls_config) = tls_config else {
        eprintln!("[node:https] tls config unavailable; refusing to listen");
        return false;
    };
    let reuse_port = crate::server::cluster_bind::is_cluster_worker();
    match crate::server::turnloop_serve::listen(
        server_handle,
        host,
        port,
        511,
        Some(tls_config),
        reuse_port,
        no_delay,
        idle_close_ms,
    ) {
        Ok((_id, actual_port, _bound_host)) => {
            crate::server::cluster_bind::notify_listening(host, actual_port);
            crate::tls_client::register_internal_https_server(actual_port, certificate_cn);
            match get_handle_mut::<HttpsServer>(server_handle) {
                Some(s) => {
                    s.base.bound_port = actual_port;
                    s.base.bound_host = host.to_string();
                    s.base.listening = true;
                    true
                }
                None => false,
            }
        }
        Err(err) => {
            eprintln!(
                "[node:https] bind {}:{} failed: {}",
                host,
                port,
                err.message()
            );
            false
        }
    }
}

pub(crate) fn try_recv_pending_https_nonblocking(server_handle: i64) -> Option<HttpPendingRequest> {
    crate::server::turnloop_serve::take_pending(server_handle)
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
        crate::server::server::queue_deferred_close_emit(&mut s.base, callback);
    }
    // P5: stop accepting on the turnloop listener, if this server has one.
    if let Some(listener) = crate::server::turnloop_serve::listener_for_server(handle) {
        crate::server::turnloop_serve::close_listener(listener);
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
/// Was a no-op stub pre-#4971.
#[no_mangle]
pub extern "C" fn js_node_https_server_close_all_connections(handle: i64) {
    // Delegate to the HTTP variant: the connection and IN_FLIGHT
    // registries are shared and keyed by server handle, and the HTTP
    // path also finalizes parked async requests whose connection just
    // died.
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
