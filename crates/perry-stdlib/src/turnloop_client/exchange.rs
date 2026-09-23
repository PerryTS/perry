//! The per-request state machine: connect, handshake, encode, decode, redirect,
//! decompress, deliver.
//!
//! Everything here runs on the loop-owning thread, either from `submit` (the
//! caller's own thread, before the first turn) or from the completion sink
//! (inside `dispatch_staged`, after a turn has returned). Nothing here runs JS:
//! a finished response is pushed onto the engine's `pending` list and the sink
//! is called by `drain_pending` once the tables are no longer borrowed.

use std::sync::atomic::Ordering;
use std::time::Instant;

use perry_runtime::turnloop_net as tl;
use turnloop_http::client::{self as tlc, Acquire, PoolKey};
use turnloop_http::http1;

use super::content_decoding::{apply_default_accept_encoding, ContentDecoder};
use super::{
    deliver, Conn, Engine, Outcome, Req, ResponseOut, BODY_LIMIT, CONNECTED, DECODED, ENGINE,
    REDIRECTS, REUSED, SUBSYSTEM, TUNNELS,
};

/// A transport failure in the shape `fetch` reports it: Node's `cause.code`,
/// the message, and the `syscall` when one is known.
#[derive(Clone, Debug)]
pub(crate) struct ClientError {
    /// Node's `cause.code`. `&'static str` because that is what
    /// `register_error_code_pub` takes, and every producer on this path —
    /// `turnloop::Error`, `turnloop_net::NodeError`, `turnloop_tls::
    /// node_error_code` — already has one.
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) syscall: Option<&'static str>,
    /// Set for an `AbortSignal` cancellation, which rejects with `AbortError`
    /// rather than a `TypeError` with a cause.
    pub(crate) aborted: bool,
}

impl ClientError {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            syscall: None,
            aborted: false,
        }
    }

    fn aborted() -> Self {
        Self {
            code: "ABORT_ERR",
            message: "This operation was aborted".into(),
            syscall: None,
            aborted: true,
        }
    }
}

/// The shared outbound TLS configuration (see `turnloop_tls_client`).
pub(super) fn tls_config() -> Option<&'static turnloop_tls::ClientConfig> {
    crate::turnloop_tls_client::client_config()
}

/// `PERRY_P6_DEBUG=1` prints one line per transport decision. Off by default
/// and asserted off by `debug_tracing_is_off_by_default`.
pub(super) fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("PERRY_P6_DEBUG")
            .map(|v| matches!(v.as_str(), "1" | "on" | "true"))
            .unwrap_or(false)
    })
}

macro_rules! trace {
    ($($arg:tt)*) => {
        if tracing() {
            eprintln!("[p6-http] {}", format!($($arg)*));
        }
    };
}

fn pool_key(request: &tlc::Request, proxy: Option<&url::Url>) -> PoolKey {
    PoolKey::new(&request.url, proxy)
}

/// The routing decision for one request: where to dial, and whether the socket
/// has to be tunnelled before the request can go out.
///
/// `turnloop_http::client::Route` owns the policy (which form the request line
/// takes, what the CONNECT head looks like, whether a 2xx means "upgrade now").
/// This module owns only the transport.
fn route_for(request: &tlc::Request, proxy: Option<&url::Url>) -> tlc::Route {
    tlc::Route::new(request.url.clone(), proxy.cloned())
}

// ── Starting a request ─────────────────────────────────────────────────────

/// Acquire a connection for `id` and begin (or queue) its exchange.
pub(super) fn start(id: u64) {
    ENGINE.with(|e| start_locked(&mut e.borrow_mut(), id));
}

fn start_locked(engine: &mut Engine, id: u64) {
    // Everything the pool decision needs is copied out first: `acquire` takes
    // `&mut Engine` and a live borrow of the request map would outlive it.
    let Some((key, host, port, secure, proxy, tunnelled)) = engine.requests.get(&id).map(|req| {
        let proxy = req.proxy.clone();
        // The socket's peer is the PROXY when one is configured. An `https`
        // target behind one needs a CONNECT tunnel first; an `http` target does
        // not — its request line just becomes absolute-form, which
        // `Route::request_head` handles.
        let (host, port) = match proxy.as_ref() {
            Some(p) => (
                p.host_str().unwrap_or("").to_string(),
                p.port_or_known_default().unwrap_or(80),
            ),
            None => (
                req.request.url.host_str().unwrap_or("").to_string(),
                req.request.url.port_or_known_default().unwrap_or(80),
            ),
        };
        let secure = req.request.url.scheme() == "https";
        (
            pool_key(&req.request, proxy.as_ref()),
            host,
            port,
            secure,
            proxy,
            secure && req.proxy.is_some(),
        )
    }) else {
        return;
    };
    let now = Instant::now();
    match engine.pool().acquire(&key, now) {
        Acquire::Reuse(pool_id) => {
            trace!("acquire reuse pool_id={pool_id:?} origin={}", key.origin);
            let Some(conn_id) = engine
                .conns
                .iter()
                .find(|(_, c)| c.pool_id == pool_id && !c.closing)
                .map(|(id, _)| *id)
            else {
                // The pool believes a connection exists that this table does
                // not have. Release the reservation and take a fresh one rather
                // than hanging: a lost socket must not strand the request.
                let _ = engine.pool().release(pool_id, false, now);
                let _ = engine.pool().closed(pool_id);
                start_locked(engine, id);
                return;
            };
            REUSED.fetch_add(1, Ordering::Relaxed);
            cancel_idle_timer(engine, conn_id);
            let _ = tl::set_ref(conn_id, true);
            attach(engine, conn_id, id);
            send_head(engine, conn_id);
        }
        Acquire::Connect(pool_id) => {
            trace!("acquire connect pool_id={pool_id:?} origin={}", key.origin);
            let conn_id = engine.alloc_id();
            let tls = if secure {
                let Some(config) = tls_config() else {
                    let _ = engine.pool().closed(pool_id);
                    deliver(
                        engine,
                        id,
                        Outcome::Err(ClientError::new(
                            "ERR_SSL_PROTOCOL_ERROR",
                            "TLS client configuration unavailable",
                        )),
                    );
                    return;
                };
                let name = match crate::turnloop_tls_client::server_name(&host) {
                    Ok(name) => name,
                    Err(message) => {
                        let _ = engine.pool().closed(pool_id);
                        deliver(
                            engine,
                            id,
                            Outcome::Err(ClientError::new("ERR_TLS_CERT_ALTNAME_INVALID", message)),
                        );
                        return;
                    }
                };
                match crate::turnloop_tls_client::TlsClientSession::new(config, name) {
                    Ok(session) => Some(Box::new(session)),
                    Err(message) => {
                        let _ = engine.pool().closed(pool_id);
                        deliver(
                            engine,
                            id,
                            Outcome::Err(ClientError::new("ERR_SSL_PROTOCOL_ERROR", message)),
                        );
                        return;
                    }
                }
            } else {
                None
            };
            engine.conns.insert(
                conn_id,
                Conn {
                    pool_id,
                    key,
                    tls,
                    http: tlc::Http1Connection::new(http1::Limits::default()),
                    input: Vec::new(),
                    request: Some(id),
                    idle_timer: None,
                    closing: false,
                    used: false,
                    proxy,
                    tunnel: tunnelled.then(|| {
                        Box::new(super::Tunnel {
                            http: tlc::Http1Connection::new(http1::Limits::default()),
                            input: Vec::new(),
                        })
                    }),
                },
            );
            if let Some(req) = engine.requests.get_mut(&id) {
                req.conn = Some(conn_id);
            }
            // `nodelay = true` matches what the tokio client path set on every
            // socket it created (`SocketState`'s `set_nodelay(true)`), and what
            // reqwest's hyper connector sets by default.
            if let Err(err) = tl::tcp_connect_host(conn_id, SUBSYSTEM, &host, port, true) {
                engine.conns.remove(&conn_id);
                let _ = engine.pool().closed(pool_id);
                deliver(
                    engine,
                    id,
                    Outcome::Err(from_node_error(err.code, err.syscall)),
                );
            }
        }
        Acquire::Wait => {
            trace!("acquire wait origin={}", key.origin);
            engine
                .waiting
                .entry(key.origin.clone())
                .or_default()
                .push_back(id);
        }
    }
}

fn attach(engine: &mut Engine, conn_id: i64, req_id: u64) {
    if let Some(conn) = engine.conns.get_mut(&conn_id) {
        conn.request = Some(req_id);
        // A reused connection carries nothing over: this client never
        // pipelines, so anything still buffered would be a server that answered
        // a request it was not asked. Clearing it is what makes the next
        // response's head start at byte zero.
        conn.input.clear();
    }
    if let Some(req) = engine.requests.get_mut(&req_id) {
        req.conn = Some(conn_id);
        req.head = None;
        req.body.clear();
        req.decoded.clear();
        req.decoder = None;
        req.streaming = false;
    }
}

/// Encode the request head (and body) onto the connection and flush it.
fn send_head(engine: &mut Engine, conn_id: i64) {
    let Engine {
        conns, requests, ..
    } = &mut *engine;
    let Some(conn) = conns.get_mut(&conn_id) else {
        return;
    };
    let Some(req_id) = conn.request else { return };
    let Some(req) = requests.get_mut(&req_id) else {
        return;
    };
    if conn.tls.as_ref().is_some_and(|s| s.is_handshaking()) {
        // The head is written once `NET_DATA` reports the handshake done; the
        // TLS session would buffer it, but keeping the codec untouched until
        // then is what makes `Http1Connection::start`'s "one request in flight"
        // check meaningful.
        return;
    }
    let mut head = route_for(&req.request, conn.proxy.as_ref()).request_head(&req.request, None);
    // Perry's reqwest client has always set a default `User-Agent`
    // (`fetch_client_builder`), because endpoints that reject anonymous
    // requests are common — `api.github.com` is the canonical one, and it is
    // what #236 was about. A caller's own header wins, exactly as reqwest's
    // `RequestBuilder::header` overrode the client-level value.
    if !head
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("user-agent"))
    {
        head.headers.push(http1::Header::new(
            "user-agent",
            concat!("perry/", env!("CARGO_PKG_VERSION")).as_bytes(),
        ));
    }
    // undici's default `Accept-Encoding`, per hop (#10475). Without it a server
    // that negotiates compression never compresses for Perry, which is what
    // hid the missing decoding for so long.
    apply_default_accept_encoding(&mut head, req.request.url.scheme() == "https");
    // Let the encoder synthesize framing: a caller-supplied `content-length` or
    // `transfer-encoding` either duplicates it or conflicts with the body we
    // actually hold, and `Encoder::start` rejects the conflict outright.
    head.headers.retain(|h| {
        !h.name.eq_ignore_ascii_case("content-length")
            && !h.name.eq_ignore_ascii_case("transfer-encoding")
        // `expect` is refused by `fetch` before it reaches here
        // (`fetch::forbidden_header_failure`), matching undici. Stripping it
        // as a defence in depth would be wrong: `Http1Connection::start`
        // reads `expect: 100-continue` on a non-empty body as "park the
        // upload", `can_send_body()` goes false and the next `send_body`
        // fails `UND_ERR_INVALID_ARG "request body is not writable"` — a
        // failure whose message names the body, not the header, which is
        // what made this hard to see. A `node:http` client must implement
        // the real handshake instead; that is `continue_client.rs`.
    });
    let body = std::mem::take(&mut req.request.body);
    let length = body_length(&req.request.method, body.len());
    let result = conn
        .http
        .start(&head, length, None, None)
        .and_then(|()| {
            if matches!(length, http1::BodyLength::Empty) {
                Ok(())
            } else {
                conn.http.send_body(&body)
            }
        })
        .and_then(|()| conn.http.finish_body(&[]));
    req.request.body = body;
    if let Err(e) = result {
        let error = ClientError::new(e.code, e.message);
        deliver(engine, req_id, Outcome::Err(error));
        close_conn(engine, conn_id);
        return;
    }
    flush(engine, conn_id);
}

fn body_length(method: &str, len: usize) -> http1::BodyLength {
    if len > 0 {
        return http1::BodyLength::Known(len as u64);
    }
    // A bodyless GET/HEAD/OPTIONS/DELETE sends no `content-length`; a bodyless
    // POST/PUT/PATCH sends `content-length: 0`. That is what both Node's fetch
    // and reqwest put on the wire.
    match method {
        "GET" | "HEAD" | "OPTIONS" | "DELETE" | "TRACE" => http1::BodyLength::Empty,
        _ => http1::BodyLength::Known(0),
    }
}

/// Move whatever the codec (and the TLS session) have produced onto the socket.
fn flush(engine: &mut Engine, conn_id: i64) {
    let Some(conn) = engine.conns.get_mut(&conn_id) else {
        return;
    };
    if conn.closing {
        return;
    }
    let plain = conn.http.output().to_vec();
    if !plain.is_empty() {
        let _ = conn.http.consume_output(plain.len());
    }
    let out = match conn.tls.as_mut() {
        Some(session) => {
            if !plain.is_empty() {
                session.write(&plain);
            }
            session.pump();
            session.take_output()
        }
        None => plain,
    };
    if out.is_empty() {
        return;
    }
    if let Err(err) = tl::write(conn_id, out, 0) {
        let req_id = conn.request;
        let error = from_node_error(err.code, err.syscall);
        if let Some(req_id) = req_id {
            deliver(engine, req_id, Outcome::Err(error));
        }
        close_conn(engine, conn_id);
    }
}

// ── Completions ────────────────────────────────────────────────────────────

/// Re-intern a Node error code the completion borrowed. `turnloop_net`'s codes
/// are `&'static str`s from one table, so this is a lookup rather than a leak;
/// an unrecognized one degrades to the generic socket error the same way
/// `map_error`'s own fallback does.
fn intern_code(code: Option<&str>) -> &'static str {
    const CODES: &[&str] = &[
        "EACCES",
        "EADDRINUSE",
        "EADDRNOTAVAIL",
        "EAGAIN",
        "EALREADY",
        "EBADF",
        "ECANCELED",
        "ECONNABORTED",
        "ECONNREFUSED",
        "ECONNRESET",
        "EEXIST",
        "EHOSTUNREACH",
        "EINVAL",
        "EIO",
        "EISCONN",
        "EMFILE",
        "ENETDOWN",
        "ENETUNREACH",
        "ENFILE",
        "ENOBUFS",
        "ENOENT",
        "ENOMEM",
        "ENOTCONN",
        "ENOTDIR",
        "ENOTFOUND",
        "ENOTSUP",
        "EPERM",
        "EPIPE",
        "EPROTO",
        "ETIMEDOUT",
        "UNKNOWN",
    ];
    let Some(code) = code else {
        return "UND_ERR_SOCKET";
    };
    CODES
        .iter()
        .find(|known| **known == code)
        .copied()
        .unwrap_or("UND_ERR_SOCKET")
}

fn intern_syscall(syscall: Option<&str>) -> &'static str {
    const SYSCALLS: &[&str] = &[
        "accept",
        "read",
        "write",
        "shutdown",
        "connect",
        "close",
        "getaddrinfo",
        "timer",
    ];
    let Some(syscall) = syscall else { return "" };
    SYSCALLS
        .iter()
        .find(|known| **known == syscall)
        .copied()
        .unwrap_or("")
}

pub(super) fn on_completion(
    kind: i32,
    id: i64,
    errno: i32,
    code: Option<&str>,
    syscall: Option<&str>,
    bytes: &[u8],
    terminal: bool,
) {
    let _ = errno;
    ENGINE.with(|e| {
        let mut engine = e.borrow_mut();
        match kind {
            tl::NET_TIMER => on_idle_timeout(&mut engine, id),
            tl::NET_CONNECT => on_connect(&mut engine, id),
            tl::NET_DATA => on_data(&mut engine, id, bytes),
            tl::NET_EOF => on_eof(&mut engine, id),
            tl::NET_ERROR => {
                if terminal || engine.conns.contains_key(&id) {
                    on_error(&mut engine, id, code, syscall);
                }
            }
            tl::NET_CLOSED => on_closed(&mut engine, id),
            // A write or shutdown completion is pure accounting here: turnloop
            // orders a handle's writes, and nothing waits on an individual one.
            _ => {}
        }
    });
}

fn on_connect(engine: &mut Engine, conn_id: i64) {
    CONNECTED.fetch_add(1, Ordering::Relaxed);
    if let Err(err) = tl::read_start(conn_id) {
        fail_conn(engine, conn_id, from_node_error(err.code, err.syscall));
        return;
    }
    let pool_id = engine.conns.get(&conn_id).map(|c| c.pool_id);
    if let Some(pool_id) = pool_id {
        let _ = engine.pool().connected(pool_id, tlc::Protocol::Http1, 1);
    }
    if engine
        .conns
        .get(&conn_id)
        .is_some_and(|c| c.tunnel.is_some())
    {
        // The peer is a proxy and the target is `https`: nothing of this
        // request — not the head, not the TLS ClientHello — may go out until
        // the proxy has answered 2xx to a CONNECT.
        send_connect(engine, conn_id);
        return;
    }
    let secure = engine.conns.get(&conn_id).is_some_and(|c| c.tls.is_some());
    if secure {
        // Start the handshake: the first flight is produced by a pump with no
        // input, and `flush` carries it to the socket.
        start_tls(engine, conn_id);
    } else {
        send_head(engine, conn_id);
    }
}

/// Produce the TLS first flight and put it on the socket.
fn start_tls(engine: &mut Engine, conn_id: i64) {
    if let Some(conn) = engine.conns.get_mut(&conn_id) {
        if let Some(session) = conn.tls.as_mut() {
            session.pump();
        }
    }
    flush_tls_only(engine, conn_id);
}

/// Write the proxy `CONNECT` head. Always in the clear — TLS is what the tunnel
/// exists to carry, so it cannot also wrap it.
fn send_connect(engine: &mut Engine, conn_id: i64) {
    let Engine {
        conns, requests, ..
    } = &mut *engine;
    let Some(conn) = conns.get_mut(&conn_id) else {
        return;
    };
    let Some(req_id) = conn.request else { return };
    let Some(req) = requests.get(&req_id) else {
        return;
    };
    let route = route_for(&req.request, conn.proxy.as_ref());
    // `connect_head` returns `None` only when there is no proxy or the target is
    // not `https`, and `Conn::tunnel` is set exactly when both hold — so a
    // `None` here is this module contradicting itself, not a request shape.
    let Some(head) = route.connect_head(None) else {
        let Some(tunnel) = conn.tunnel.take() else {
            return;
        };
        drop(tunnel);
        send_head(engine, conn_id);
        return;
    };
    let Some(tunnel) = conn.tunnel.as_mut() else {
        return;
    };
    let result = tunnel
        .http
        .start(&head, http1::BodyLength::Empty, None, None)
        .and_then(|()| tunnel.http.finish_body(&[]));
    if let Err(e) = result {
        let error = ClientError::new(e.code, e.message);
        deliver(engine, req_id, Outcome::Err(error));
        close_conn(engine, conn_id);
        return;
    }
    flush_tunnel(engine, conn_id);
}

/// The tunnel's own flush: it must bypass `conn.tls`, which is the session the
/// tunnel is being built *for*.
fn flush_tunnel(engine: &mut Engine, conn_id: i64) {
    let Some(conn) = engine.conns.get_mut(&conn_id) else {
        return;
    };
    if conn.closing {
        return;
    }
    let Some(tunnel) = conn.tunnel.as_mut() else {
        return;
    };
    let out = tunnel.http.output().to_vec();
    if out.is_empty() {
        return;
    }
    let _ = tunnel.http.consume_output(out.len());
    if let Err(err) = tl::write(conn_id, out, 0) {
        fail_conn(engine, conn_id, from_node_error(err.code, err.syscall));
    }
}

/// Read a CONNECT response's status out of whatever bytes have arrived.
///
/// Returns `(consumed, Some(status))` once the head is complete and
/// `(consumed, None)` while it is not. Only the HEAD matters: a CONNECT
/// response has no body to wait for — the socket becomes the tunnel — so
/// waiting for `Event::End` here would hang against every correct proxy.
///
/// Pure, so the split-read hazard is testable without a socket. The loop obeys
/// the same rule `feed` documents (PerryTS/turnloop#50): `Decoder` can raise an
/// event from a step that consumed NOTHING, so a loop that stops at
/// `consumed >= input.len()` never asks for it.
fn decode_connect_status(
    http: &mut tlc::Http1Connection,
    input: &[u8],
) -> Result<(usize, Option<u16>), turnloop_http::Error> {
    let mut consumed = 0usize;
    loop {
        let step = http.receive(&input[consumed..])?;
        let produced = step.event.is_some();
        consumed += step.consumed;
        if let Some(http1::Event::Head(head)) = step.event {
            return Ok((consumed, Some(head.status)));
        }
        if !produced && step.consumed == 0 {
            return Ok((consumed, None));
        }
    }
}

/// Feed the proxy's answer to the CONNECT.
///
/// Returns the bytes left over once the tunnel is up — a proxy is allowed to
/// coalesce its `200` with nothing else, but a buffer that dropped a stray tail
/// would lose the first TLS record, so the leftover is handed back rather than
/// discarded. `None` means the tunnel is still pending (or the connection is
/// gone) and the caller must stop.
fn tunnel_data(engine: &mut Engine, conn_id: i64, bytes: &[u8]) -> Option<Vec<u8>> {
    {
        let conn = engine.conns.get_mut(&conn_id)?;
        let tunnel = conn.tunnel.as_mut()?;
        tunnel.input.extend_from_slice(bytes);
    }
    let status = {
        let conn = engine.conns.get_mut(&conn_id)?;
        let tunnel = conn.tunnel.as_mut()?;
        let input = std::mem::take(&mut tunnel.input);
        match decode_connect_status(&mut tunnel.http, &input) {
            Ok((consumed, status)) => {
                tunnel.input = input;
                tunnel.input.drain(..consumed.min(tunnel.input.len()));
                status
            }
            Err(e) => {
                let error = ClientError::new(e.code, e.message);
                fail_conn(engine, conn_id, error);
                return None;
            }
        }
    };
    let status = status?;
    // Ask the route, rather than testing `200` here: it owns the rule, and it
    // is what flips its own `tunnel` flag.
    let decision = {
        let conn = engine.conns.get(&conn_id)?;
        let req_id = conn.request?;
        let req = engine.requests.get(&req_id)?;
        let mut route = route_for(&req.request, conn.proxy.as_ref());
        route.tunnel_response(status)
    };
    match decision {
        Ok(_) => {}
        Err(e) => {
            let error = ClientError::new(e.code, format!("{} ({status})", e.message));
            fail_conn(engine, conn_id, error);
            return None;
        }
    }
    let leftover = {
        let conn = engine.conns.get_mut(&conn_id)?;
        let tunnel = conn.tunnel.take()?;
        tunnel.input
    };
    TUNNELS.fetch_add(1, Ordering::Relaxed);
    trace!("tunnel established conn_id={conn_id} status={status}");
    // The tunnel is the transport now. Everything past here is the ordinary
    // path: an `https` target always has a TLS session, so hand it the first
    // flight and let `on_data` carry the handshake.
    start_tls(engine, conn_id);
    Some(leftover)
}

/// Handshake flights have no `http.output()` behind them, so they get their own
/// flush that does not touch the codec.
fn flush_tls_only(engine: &mut Engine, conn_id: i64) {
    let out = engine
        .conns
        .get_mut(&conn_id)
        .and_then(|c| c.tls.as_mut())
        .map(|s| s.take_output())
        .unwrap_or_default();
    if out.is_empty() {
        return;
    }
    if let Err(err) = tl::write(conn_id, out, 0) {
        fail_conn(engine, conn_id, from_node_error(err.code, err.syscall));
    }
}

fn on_data(engine: &mut Engine, conn_id: i64, bytes: &[u8]) {
    let Some(conn) = engine.conns.get(&conn_id) else {
        return;
    };
    if conn.closing {
        return;
    }
    if conn.tunnel.is_some() {
        // Still building the CONNECT tunnel: these bytes are the proxy's, not
        // the origin's, and they are in the clear even for an `https` target.
        let Some(leftover) = tunnel_data(engine, conn_id, bytes) else {
            return;
        };
        if leftover.is_empty() {
            return;
        }
        // A proxy that coalesced its `200` with the origin's first TLS record:
        // re-enter with the tail, now that `conn.tunnel` is gone.
        on_data(engine, conn_id, &leftover);
        return;
    }
    let Some(conn) = engine.conns.get_mut(&conn_id) else {
        return;
    };
    let plaintext = match conn.tls.as_mut() {
        Some(session) => {
            session.receive(bytes);
            let progress = session.pump();
            if let Some((code, text)) = session.failure() {
                let error = ClientError::new(code, text.to_string());
                fail_conn(engine, conn_id, error);
                return;
            }
            let plaintext = session.take_plaintext();
            let handshake_done = progress.handshake_done;
            // Only `http/1.1` is advertised, so a server that selected anything
            // else has violated ALPN. Asserting it is what makes "this path
            // never speaks h2" a fact rather than a configuration comment.
            let wrong_alpn = handshake_done
                && session
                    .alpn_protocol()
                    .is_some_and(|p| p.as_slice() != b"http/1.1");
            let peer_closed = progress.peer_closed;
            flush_tls_only(engine, conn_id);
            if wrong_alpn {
                fail_conn(
                    engine,
                    conn_id,
                    ClientError::new(
                        "ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL",
                        "server selected an ALPN protocol this client did not offer",
                    ),
                );
                return;
            }
            if handshake_done {
                send_head(engine, conn_id);
            }
            if peer_closed && plaintext.is_empty() {
                on_eof(engine, conn_id);
                return;
            }
            plaintext
        }
        None => bytes.to_vec(),
    };
    if plaintext.is_empty() {
        return;
    }
    // Append to whatever the codec has not consumed yet, feed the WHOLE buffer,
    // and keep the remainder. See `Conn::input` for why feeding only the new
    // bytes is wrong.
    let Some(conn) = engine.conns.get_mut(&conn_id) else {
        return;
    };
    let mut buffer = std::mem::take(&mut conn.input);
    if buffer.is_empty() {
        buffer = plaintext;
    } else {
        buffer.extend_from_slice(&plaintext);
    }
    let consumed = feed(engine, conn_id, &buffer);
    if let Some(conn) = engine.conns.get_mut(&conn_id) {
        if !conn.closing {
            buffer.drain(..consumed.min(buffer.len()));
            conn.input = buffer;
        }
    }
}

/// What one decoder step produced, copied out so the borrow on `input` ends
/// before any table is rearranged.
/// Whether a step did anything at all, kept apart from `Produced` because the
/// match below moves it.
enum Kind {
    Nothing,
    Event,
}

enum Produced {
    Nothing,
    /// The connection or its request went away under us; stop feeding.
    Stop,
    Head(Box<http1::Head>),
    Informational,
    End,
    Upgrade,
}

/// Returns how many leading bytes of `input` the codec consumed.
fn feed(engine: &mut Engine, conn_id: i64, input: &[u8]) -> usize {
    let mut pos = 0;
    loop {
        let step = (|| {
            let Engine {
                conns, requests, ..
            } = &mut *engine;
            let Some(conn) = conns.get_mut(&conn_id) else {
                return Ok((0, Produced::Stop));
            };
            if conn.closing {
                return Ok((0, Produced::Stop));
            }
            let Some(req_id) = conn.request else {
                return Ok((0, Produced::Stop));
            };
            let Some(req) = requests.get_mut(&req_id) else {
                return Ok((0, Produced::Stop));
            };
            match conn.http.receive(&input[pos..]) {
                Ok(step) => {
                    let consumed = step.consumed;
                    let produced = match step.event {
                        None => Produced::Nothing,
                        Some(http1::Event::Head(head)) => Produced::Head(Box::new(head)),
                        Some(http1::Event::Informational(_)) => Produced::Informational,
                        Some(http1::Event::Body(chunk)) => {
                            // Copied (or decoded) here, inside the borrow, so
                            // no reference into `input` escapes.
                            match absorb(req, chunk) {
                                Ok(()) => Produced::Nothing,
                                Err(error) => return Err(error),
                            }
                        }
                        // A client sends no trailers of its own and Perry's
                        // buffered fetch surface exposes none, so a response
                        // trailer block is decoded and dropped — the same thing
                        // reqwest's body API does with it.
                        Some(http1::Event::Trailers(_)) => Produced::Nothing,
                        Some(http1::Event::End) => Produced::End,
                        Some(http1::Event::Upgrade) => Produced::Upgrade,
                    };
                    Ok((consumed, produced))
                }
                Err(e) => Err(ClientError::new(e.code, e.message)),
            }
        })();
        let (consumed, produced) = match step {
            Ok(step) => step,
            Err(error) => {
                fail_conn(engine, conn_id, error);
                return pos;
            }
        };
        pos += consumed;
        let produced_kind = match &produced {
            Produced::Nothing => Kind::Nothing,
            _ => Kind::Event,
        };
        // The decoder emits `Event::End` from a step that consumes NOTHING:
        // `State::End -> Done` is a transition, not a parse. So the loop must
        // keep asking while EITHER a byte was consumed or an event was
        // produced, and stop only when both are zero — the `idle` test at the
        // bottom. Returning on `pos >= input.len()` alone (which is what this
        // did first) leaves the response one call short of `End`, and the only
        // thing that then finishes the request is the server's own keep-alive
        // timeout closing the socket: every fetch took five seconds and no
        // connection was ever reusable.
        match produced {
            Produced::Head(head) => {
                if let Err(error) = on_head(engine, conn_id, *head) {
                    fail_conn(engine, conn_id, error);
                    return pos;
                }
            }
            Produced::End => {
                on_end(engine, conn_id);
                // `on_end` may have released the connection to the pool and
                // started the next queued request on it, so anything still in
                // `input` belongs to that exchange.
                if pos >= input.len() {
                    return pos;
                }
                continue;
            }
            Produced::Upgrade => {
                // A 101 to a request this client never sends an `Upgrade`
                // header on. Treat it as a protocol error rather than leaving
                // the socket in a state nothing drains.
                fail_conn(
                    engine,
                    conn_id,
                    ClientError::new("HPE_INVALID_CONSTANT", "unexpected protocol upgrade"),
                );
                return pos;
            }
            Produced::Stop => return pos,
            Produced::Informational | Produced::Nothing => {}
        }
        if consumed == 0 && matches!(produced_kind, Kind::Nothing) {
            return pos;
        }
    }
}

/// Append one body chunk, decoding it if the response carried a
/// `Content-Encoding`, and hand it to a streaming sink.
fn absorb(req: &mut Req, chunk: &[u8]) -> Result<(), ClientError> {
    if req.decoder.is_none() {
        if req.body.len() + chunk.len() > BODY_LIMIT {
            return Err(ClientError::new("UND_ERR_BODY_TOO_LARGE", "body too large"));
        }
        req.body.extend_from_slice(chunk);
        if req.streaming {
            if let (Some(on_chunk), ctx) = (req.sink.on_chunk, req.sink.ctx) {
                on_chunk(ctx, chunk);
            }
            req.body.clear();
        }
        return Ok(());
    }
    let Req {
        decoder,
        decoded,
        streaming,
        sink,
        ..
    } = req;
    let decoder = decoder.as_mut().unwrap();
    decoder.feed(chunk, &mut |produced| {
        if *streaming {
            if let Some(on_chunk) = sink.on_chunk {
                on_chunk(sink.ctx, produced);
            }
        } else {
            if decoded.len() + produced.len() > BODY_LIMIT {
                return Err(ClientError::new("UND_ERR_BODY_TOO_LARGE", "body too large"));
            }
            decoded.extend_from_slice(produced);
        }
        Ok(())
    })
}

/// `Err` fails the connection (undici rejects a response carrying more than
/// five content codings before reading its body).
fn on_head(engine: &mut Engine, conn_id: i64, head: http1::Head) -> Result<(), ClientError> {
    let Engine {
        conns, requests, ..
    } = &mut *engine;
    let Some(conn) = conns.get_mut(&conn_id) else {
        return Ok(());
    };
    let Some(req_id) = conn.request else {
        return Ok(());
    };
    let Some(req) = requests.get_mut(&req_id) else {
        return Ok(());
    };
    req.body.clear();
    req.decoded.clear();
    req.decoder = None;
    // Is this response going to be followed? If so its body is scratch, and a
    // streaming sink must not see it.
    let location = head
        .get("location")
        .map(|v| String::from_utf8_lossy(v).to_string());
    let will_follow = {
        let mut probe = req.request.clone();
        probe
            .redirect(
                head.status,
                location.as_deref(),
                req.spec.redirect,
                tlc::DEFAULT_MAX_REDIRECTS,
            )
            .unwrap_or(false)
    };
    if !will_follow {
        if let Some(encoding) = head.get("content-encoding") {
            let value = String::from_utf8_lossy(encoding);
            match ContentDecoder::for_header(&value, BODY_LIMIT) {
                Ok(Some(decoder)) => {
                    DECODED.fetch_add(1, Ordering::Relaxed);
                    req.decoder = Some(Box::new(decoder));
                }
                // A coding undici does not decode (or `identity`): the body is
                // delivered as received, as Node does.
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        if req.sink.on_head.is_some() {
            req.streaming = true;
            let headers = header_pairs(&head);
            if let Some(on_head) = req.sink.on_head {
                on_head(req.sink.ctx, head.status, &headers);
            }
        }
    }
    req.head = Some(head);
    Ok(())
}

fn header_pairs(head: &http1::Head) -> Vec<(String, String)> {
    head.headers
        .iter()
        .map(|h| {
            (
                h.name.clone(),
                String::from_utf8_lossy(&h.value).to_string(),
            )
        })
        .collect()
}

fn on_end(engine: &mut Engine, conn_id: i64) {
    trace!("end conn={conn_id}");
    let (req_id, head, reusable) = {
        let Engine {
            conns, requests, ..
        } = &mut *engine;
        let Some(conn) = conns.get_mut(&conn_id) else {
            return;
        };
        conn.used = true;
        let Some(req_id) = conn.request.take() else {
            return;
        };
        let Some(req) = requests.get_mut(&req_id) else {
            return;
        };
        let head = match req.head.take() {
            Some(head) => head,
            None => return,
        };
        // Drain the codec's completion so the connection reports `reusable`.
        let _ = conn.http.poll_completion();
        (req_id, head, conn.http.reusable())
    };
    // Finish any decoder that still holds buffered output.
    if let Some(req) = engine.requests.get_mut(&req_id) {
        let Req {
            decoder,
            decoded,
            streaming,
            sink,
            ..
        } = req;
        if let Some(decoder) = decoder.as_mut() {
            decoder.finish(&mut |produced| {
                if *streaming {
                    if let Some(on_chunk) = sink.on_chunk {
                        on_chunk(sink.ctx, produced);
                    }
                } else {
                    decoded.extend_from_slice(produced);
                }
                Ok(())
            });
        }
    }
    let location = head
        .get("location")
        .map(|v| String::from_utf8_lossy(v).to_string());
    let follow = engine.requests.get_mut(&req_id).and_then(|req| {
        match req.request.redirect(
            head.status,
            location.as_deref(),
            req.spec.redirect,
            tlc::DEFAULT_MAX_REDIRECTS,
        ) {
            Ok(true) => Some(Ok(())),
            Ok(false) => None,
            Err(e) => Some(Err(ClientError::new(e.code, e.message))),
        }
    });
    release(engine, conn_id, reusable);
    match follow {
        Some(Ok(())) => {
            REDIRECTS.fetch_add(1, Ordering::Relaxed);
            if let Some(req) = engine.requests.get_mut(&req_id) {
                req.redirected = true;
                req.conn = None;
                req.retried = false;
                // Re-resolve the route for the NEW url. `HTTP_PROXY` and
                // `HTTPS_PROXY` are different variables and `NO_PROXY` is
                // per-host, so a redirect that changes scheme or host can change
                // whether — and through what — this request is proxied. Carrying
                // the original decision forward would tunnel a plaintext hop, or
                // send an absolute-form request line straight at an origin.
                // A refusal here (a proxy URL that stopped parsing) keeps the
                // previous route rather than failing the redirect.
                if let Ok(proxy) = super::proxy_for(&req.request.url) {
                    req.proxy = proxy;
                }
            }
            start_locked(engine, req_id);
        }
        Some(Err(error)) => deliver(engine, req_id, Outcome::Err(error)),
        None => {
            let response = engine.requests.get_mut(&req_id).map(|req| {
                let body = if req.decoder.is_some() {
                    std::mem::take(&mut req.decoded)
                } else {
                    std::mem::take(&mut req.body)
                };
                Box::new(ResponseOut {
                    status: head.status,
                    status_text: reason(head.status),
                    headers: header_pairs(&head),
                    body: if req.streaming { Vec::new() } else { body },
                    final_url: req.request.url.as_str().to_string(),
                    redirected: req.redirected,
                })
            });
            if let Some(response) = response {
                deliver(engine, req_id, Outcome::Ok(response));
            }
        }
    }
}

/// The reason phrase Node reports for a status. `Response.statusText` is the
/// canonical reason in Node's fetch, not the one on the wire.
fn reason(status: u16) -> String {
    http::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("")
        .to_string()
}

// ── Connection lifecycle ───────────────────────────────────────────────────

/// Hand a finished connection back to the pool, or close it. Then start
/// whatever was queued behind it.
fn release(engine: &mut Engine, conn_id: i64, reusable: bool) {
    let Some(conn) = engine.conns.get(&conn_id) else {
        return;
    };
    let pool_id = conn.pool_id;
    let origin = conn.key.origin.clone();
    let now = Instant::now();
    trace!("release conn={conn_id} reusable={reusable}");
    let _ = engine.pool().release(pool_id, reusable, now);
    if reusable {
        // An idle pooled socket must not keep the process alive on its own, and
        // it must not stay open forever: unreference it and arm the pool's own
        // idle deadline as a turnloop timer (P5's `NET_TIMER`).
        let _ = tl::set_ref(conn_id, false);
        arm_idle_timer(engine, conn_id);
    } else {
        close_conn(engine, conn_id);
    }
    admit_waiter(engine, &origin);
}

/// Start one request the pool parked at `Acquire::Wait`, if there is one.
///
/// This must run wherever an origin's capacity comes back — which is every
/// place a pool seat is retired, **not** only the one happy path through
/// [`release`].
///
/// It was `release`-only at first, and `release` is reached from exactly one
/// place (`on_end`). Every failure path — a connect error, a TLS failure, a
/// `NET_ERROR`, an EOF with no head, an idle close, an abort — goes straight to
/// [`close_conn`]. So sixteen concurrent requests to one origin that all failed
/// left the seventeenth parked forever: its sink was never called, and
/// `has_pending_requests()` then kept the event loop alive on a promise that
/// could not settle, so **the process never exited**. A later request to the
/// same origin would meanwhile connect immediately and overtake it.
/// `test_gap_turnloop_fetch_pool_wait.ts` is the regression test, and it
/// asserts the exit rather than the callback: a fixture that only checked the
/// rejection would pass while the loop still refused to drain.
fn admit_waiter(engine: &mut Engine, origin: &str) {
    let next = engine
        .waiting
        .get_mut(origin)
        .and_then(std::collections::VecDeque::pop_front);
    if engine
        .waiting
        .get(origin)
        .is_some_and(std::collections::VecDeque::is_empty)
    {
        engine.waiting.remove(origin);
    }
    if let Some(next) = next {
        start_locked(engine, next);
    }
}

fn arm_idle_timer(engine: &mut Engine, conn_id: i64) {
    let timer_id = engine.alloc_id();
    if tl::timer_arm(timer_id, SUBSYSTEM, super::POOL_IDLE.as_millis() as u64).is_ok() {
        if let Some(conn) = engine.conns.get_mut(&conn_id) {
            conn.idle_timer = Some(timer_id);
        }
        return;
    }
    // `timer_arm` fails only when this thread has no loop (impossible here — a
    // connection exists) or when the driver refuses another handle, i.e. the
    // net profile's 4096-handle table is full. That is reachable on a busy
    // server, not a theoretical precondition.
    //
    // Nothing else would ever reclaim this connection: `release` has already
    // unreferenced it, so it cannot keep the process alive, and with no
    // deadline it would sit in `conns` — and on one of the origin's
    // `max_per_host` pool seats — for the life of the process, reachable only
    // if some later request happened to target the same origin. A socket that
    // cannot be aged out is not worth pooling, and closing it is also what
    // gives the seat back to a waiter.
    close_conn(engine, conn_id);
}

fn cancel_idle_timer(engine: &mut Engine, conn_id: i64) {
    if let Some(conn) = engine.conns.get_mut(&conn_id) {
        if let Some(timer_id) = conn.idle_timer.take() {
            let _ = tl::timer_cancel(timer_id);
        }
    }
}

fn on_idle_timeout(engine: &mut Engine, timer_id: i64) {
    let conn_id = engine
        .conns
        .iter()
        .find(|(_, c)| c.idle_timer == Some(timer_id))
        .map(|(id, _)| *id);
    if let Some(conn_id) = conn_id {
        if let Some(conn) = engine.conns.get_mut(&conn_id) {
            conn.idle_timer = None;
        }
        close_conn(engine, conn_id);
    }
}

/// Submit the close and mark the entry. The entry survives until `NET_CLOSED`,
/// which is where it is dropped (DESIGN D4).
///
/// Retiring the pool seat frees one of the origin's slots, so this also admits
/// whatever the pool had parked behind it — see [`admit_waiter`] for why doing
/// that only in [`release`] left requests undeliverable and the process unable
/// to exit.
///
/// (There used to be a `pooled: bool` parameter whose two arms did exactly the
/// same thing. A distinction no caller can observe is one a future change gets
/// silently wrong, so it is gone.)
fn close_conn(engine: &mut Engine, conn_id: i64) {
    let Some(conn) = engine.conns.get_mut(&conn_id) else {
        return;
    };
    if conn.closing {
        return;
    }
    conn.closing = true;
    trace!(
        "close conn={conn_id} had_request={}",
        conn.request.is_some()
    );
    if let Some(timer_id) = conn.idle_timer.take() {
        let _ = tl::timer_cancel(timer_id);
    }
    if let Some(session) = conn.tls.as_mut() {
        session.close_notify();
        session.pump();
        let out = session.take_output();
        if !out.is_empty() {
            let _ = tl::write(conn_id, out, 0);
        }
    }
    let pool_id = conn.pool_id;
    let origin = conn.key.origin.clone();
    let _ = engine.pool().closed(pool_id);
    if tl::close(conn_id).is_err() {
        // The handle is already gone; run the terminal path now so the entry
        // and any attached request cannot be stranded.
        on_closed(engine, conn_id);
    }
    admit_waiter(engine, &origin);
}

fn on_closed(engine: &mut Engine, conn_id: i64) {
    let Some(conn) = engine.conns.remove(&conn_id) else {
        return;
    };
    if let Some(req_id) = conn.request {
        fail_request(
            engine,
            req_id,
            conn.used,
            ClientError::new("ECONNRESET", "socket hang up"),
        );
    }
}

fn on_eof(engine: &mut Engine, conn_id: i64) {
    trace!("eof conn={conn_id}");
    let Engine {
        conns, requests, ..
    } = &mut *engine;
    let Some(conn) = conns.get_mut(&conn_id) else {
        return;
    };
    if conn.closing {
        return;
    }
    // A close-delimited response ends at EOF: tell the decoder, which turns a
    // pending `Eof` body into `Event::End`.
    let had_head = conn
        .request
        .and_then(|id| requests.get(&id))
        .is_some_and(|req| req.head.is_some());
    let ended = conn.http.eof().is_ok();
    if had_head && ended {
        on_end(engine, conn_id);
        close_conn(engine, conn_id);
        return;
    }
    let reused = conn.used;
    // Detached for the same reason as in `fail_conn`.
    let req_id = conn.request.take();
    close_conn(engine, conn_id);
    if let Some(req_id) = req_id {
        fail_request(
            engine,
            req_id,
            reused,
            ClientError::new("ECONNRESET", "socket hang up"),
        );
    }
}

fn on_error(engine: &mut Engine, conn_id: i64, code: Option<&str>, syscall: Option<&str>) {
    // `code`/`syscall` point at `&'static str` data in `turnloop_net::errors`
    // (its module note says so), but the sink hands them over with the
    // completion's borrow, so they are re-interned against the known table.
    let error = from_node_error(intern_code(code), intern_syscall(syscall));
    fail_conn(engine, conn_id, error);
}

fn fail_conn(engine: &mut Engine, conn_id: i64, error: ClientError) {
    // Detach the request BEFORE closing. `close_conn` can run its terminal path
    // synchronously (when the handle is already gone — a connect that never
    // produced one, which is exactly the `getaddrinfo ENOTFOUND` case), and
    // that path reports a generic socket hang-up. Delivering it first would
    // win the exactly-once guard and throw away the real error.
    let (req_id, reused) = match engine.conns.get_mut(&conn_id) {
        Some(conn) => (conn.request.take(), conn.used),
        None => (None, false),
    };
    close_conn(engine, conn_id);
    if let Some(req_id) = req_id {
        fail_request(engine, req_id, reused, error);
    }
}

/// Fail a request, retrying once on a fresh connection when the failure landed
/// on a *reused* socket before any response byte — the idle-connection race
/// every pooling client has to absorb.
fn fail_request(engine: &mut Engine, req_id: u64, reused: bool, mut error: ClientError) {
    // Node's `getaddrinfo ENOTFOUND <host>` names the host it could not
    // resolve, and callers match on that text. The socket layer reports the
    // code and the syscall; the host is the request's, so it is appended here
    // rather than threaded through `turnloop_net`'s error table.
    if error.code == "ENOTFOUND" {
        if let Some(host) = engine
            .requests
            .get(&req_id)
            .and_then(|req| req.request.url.host_str())
        {
            if !error.message.ends_with(host) {
                error.message = format!("{} {host}", error.message);
            }
        }
    }
    let retry = engine.requests.get(&req_id).is_some_and(|req| {
        reused && !req.retried && req.head.is_none() && req.request.replayable && !req.delivered
    });
    if retry {
        if let Some(req) = engine.requests.get_mut(&req_id) {
            req.retried = true;
            req.conn = None;
            req.head = None;
            req.body.clear();
            req.decoded.clear();
            req.decoder = None;
        }
        start_locked(engine, req_id);
        return;
    }
    deliver(engine, req_id, Outcome::Err(error));
}

/// Cancel one request. The socket is closed so the in-flight operation is
/// cancelled on the loop exactly once; the request is then delivered as
/// aborted.
pub(super) fn abort(req_id: u64) {
    ENGINE.with(|e| {
        let mut engine = e.borrow_mut();
        let conn = engine.requests.get(&req_id).and_then(|req| req.conn);
        if let Some(conn_id) = conn {
            if let Some(conn) = engine.conns.get_mut(&conn_id) {
                // Detach first: `close_conn`'s terminal path must not report a
                // socket hang-up for a request that is being aborted.
                conn.request = None;
            }
            close_conn(&mut engine, conn_id);
        } else {
            // Still queued behind the pool's per-origin limit. Drop an emptied
            // queue too, so an origin nothing waits on stops being a key.
            for queue in engine.waiting.values_mut() {
                queue.retain(|id| *id != req_id);
            }
            engine.waiting.retain(|_, queue| !queue.is_empty());
        }
        deliver(&mut engine, req_id, Outcome::Err(ClientError::aborted()));
    });
}

fn from_node_error(code: &'static str, syscall: &'static str) -> ClientError {
    ClientError {
        code,
        message: if syscall.is_empty() {
            code.to_string()
        } else {
            format!("{syscall} {code}")
        },
        syscall: (!syscall.is_empty()).then_some(syscall),
        aborted: false,
    }
}

// ── Test seams ─────────────────────────────────────────────────────────────
//
// The functions below are the private decisions this module makes, named so
// `tests.rs` can assert them directly rather than inferring them from a passing
// workload.

#[cfg(test)]
pub(super) fn body_length_for_test(method: &str, len: usize) -> http1::BodyLength {
    body_length(method, len)
}

#[cfg(test)]
pub(super) fn intern_code_for_test(code: Option<&str>) -> &'static str {
    intern_code(code)
}

#[cfg(test)]
/// Test seam for [`decode_connect_status`].
pub(super) fn decode_connect_status_for_test(
    http: &mut tlc::Http1Connection,
    input: &[u8],
) -> Result<(usize, Option<u16>), turnloop_http::Error> {
    decode_connect_status(http, input)
}

#[cfg(test)]
pub(super) fn intern_syscall_for_test(syscall: Option<&str>) -> &'static str {
    intern_syscall(syscall)
}
