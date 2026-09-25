//! One turnloop-backed HTTP/1.1 server connection (P5).
//!
//! The whole exchange lives on the loop-owning thread: bytes arrive as a
//! `NET_DATA` completion, `turnloop_http::http1::Decoder` turns them into a
//! request, the request is queued for the existing main-thread pump, and
//! `res.end()` encodes the response and submits the write. There is no task,
//! no channel and no cross-thread notify anywhere on that path.
//!
//! What the sink may and may not do is the load-bearing rule: it runs inside
//! `dispatch_staged`, so it may allocate Rust state and register handles, but
//! it must **not** run JS. A decoded request is therefore pushed onto the
//! server's queue and dispatched by `js_node_http_server_process_pending` on
//! its own tick, exactly where hyper's `mpsc` delivered it.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;
use turnloop_http::http1;

use super::wire::{self, Framing};
use crate::server::request::{
    alloc_connection_socket, alloc_incoming_message, handle_to_pointer_f64, incoming_socket_assign,
    IncomingMessage,
};
use crate::server::response::{alloc_server_response_for_turnloop, ResponseShape};
use crate::server::server::{with_base_server, HttpPendingRequest};

/// The request being decoded, before it becomes an `IncomingMessage`.
struct Building {
    method: String,
    url: String,
    headers_lower: HashMap<String, String>,
    raw_headers: Vec<(String, String)>,
    body: Vec<u8>,
    version: u8,
    expects_continue: bool,
    /// The request's own `Connection` header value, needed to compute the
    /// response's default `Connection` / `Keep-Alive` pair.
    connection: Option<String>,
    /// `Connection: upgrade` with an `Upgrade` header — Node dispatches this
    /// to `'upgrade'` rather than `'request'`, *if* a listener exists.
    ///
    /// Recognized here, from the head, rather than taken from the decoder.
    /// Since turnloop-http 0.1.0-alpha.7 the request-mode decoder DOES raise
    /// `Event::Upgrade` (it used to be reachable only in `Mode::Response`, a
    /// client reading a 101), but that event is routed into the same arm as
    /// `Event::End` and the routing still keys off this flag — the two do not
    /// agree on CONNECT, which sets the decoder's `upgrade_request` but is not
    /// an `Upgrade` header and so is not one of these.
    upgrade: bool,
    /// `Connection: upgrade` naming `websocket`, with a `Sec-WebSocket-Key`.
    /// An attached `WebSocketServer` answers these itself.
    websocket: bool,
}

/// The request currently being answered.
struct Active {
    seq: u64,
    /// The `IncomingMessage` this answers, so a connection that dies before the
    /// response completes can raise Node's `'aborted'` on it.
    request_handle: i64,
    method: String,
    version: u8,
    connection: Option<String>,
    encoder: Option<http1::Encoder>,
    framing: Framing,
    head_sent: bool,
    /// Keep the connection after this response, as decided at head time.
    keep_alive: bool,
}

pub(crate) struct Conn {
    id: i64,
    server_handle: i64,
    peer_address: String,
    peer_port: u16,
    /// The `IncomingMessage`-shaped object
    /// handed to `server.on('connection', ...)` and shared as `req.socket`
    /// by every request this connection carries. One per connection, built
    /// once in `start_connection`/`adopt_alpn_http1`.
    socket_handle: i64,
    decoder: http1::Decoder,
    input: Vec<u8>,
    building: Option<Building>,
    active: Option<Active>,
    seq: u64,
    /// Requests decoded on this connection, for `maxRequestsPerSocket`.
    requests: u64,
    /// Node's idle close: `keepAliveTimeout + keepAliveTimeoutBuffer`, in ms.
    /// Zero means "never time out" (Node 26.5.1, measured).
    idle_close_ms: u64,
    /// `server.keepAliveTimeout` itself, which is what the `Keep-Alive`
    /// response header advertises.
    keep_alive_timeout_ms: f64,
    /// Bytes still to decode are held while a response is in flight, so a
    /// pipelined request is not dispatched before the current one finishes.
    paused: bool,
    read_eof: bool,
    closing: bool,
    destroyed: bool,
    secure: bool,
    /// The handshake has not completed, so no HTTP byte has been seen yet.
    handshaking: bool,
    /// The connection has been upgraded to WebSocket. Bytes now go to
    /// `perry_ext_ws::turnloop_link` rather than the HTTP decoder, and the
    /// connection stays ours: P5's `turnloop_net::transfer` moves an
    /// `'upgrade'` socket to `net` because a `net.Socket` outlives it, but a
    /// WebSocket has no such JS object and the protocol runs *above* the
    /// handle, TLS layer and all.
    websocket: bool,
}

fn conns() -> &'static Mutex<HashMap<i64, Conn>> {
    static CONNS: OnceLock<Mutex<HashMap<i64, Conn>>> = OnceLock::new();
    CONNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Requests decoded and waiting for the main-thread pump, per JS server handle.
fn pending() -> &'static Mutex<HashMap<i64, VecDeque<HttpPendingRequest>>> {
    static PENDING: OnceLock<Mutex<HashMap<i64, VecDeque<HttpPendingRequest>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `IncomingMessage` handles whose connection died before their response
/// completed. Node raises `'aborted'` on the request; the sink cannot run JS,
/// so the pump drains this and fires the listeners on its own tick.
fn aborted() -> &'static Mutex<Vec<i64>> {
    static ABORTED: OnceLock<Mutex<Vec<i64>>> = OnceLock::new();
    ABORTED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Queue an `IncomingMessage` handle for Node's `'aborted'`.
///
/// Shared with the HTTP/2 transport, which reaches the same queue for the same
/// reason: a stream reset or a dead connection leaves a request that will never
/// be answered, and the sink cannot run its listeners itself.
pub(crate) fn note_aborted_handle(handle: i64) {
    if handle == 0 {
        return;
    }
    aborted()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(handle);
}

/// Take the `IncomingMessage` handles whose connection died mid-request.
pub(crate) fn take_aborted() -> Vec<i64> {
    let mut queue = aborted().lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *queue)
}

/// Connection-socket handles (`alloc_connection_socket`) whose TCP connection
/// has fully closed. Same pattern as `aborted()`: the completion sink cannot
/// run JS, so the pump drains this and fires the socket's `'close'`
/// listeners on its own tick.
fn closed_sockets() -> &'static Mutex<Vec<i64>> {
    static CLOSED: OnceLock<Mutex<Vec<i64>>> = OnceLock::new();
    CLOSED.get_or_init(|| Mutex::new(Vec::new()))
}

fn note_closed_socket(socket_handle: i64) {
    closed_sockets()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(socket_handle);
}

/// Take the connection-socket handles due a `'close'` emit.
pub(crate) fn take_closed_sockets() -> Vec<i64> {
    let mut queue = closed_sockets().lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *queue)
}

/// Note that this connection's in-flight request (if any) will never be
/// answered, exactly once per request.
fn note_aborted(id: i64) {
    let handle = with_conn(id, |c| {
        c.active
            .as_mut()
            .map(|a| std::mem::replace(&mut a.request_handle, 0))
    })
    .flatten()
    .filter(|h| *h != 0);
    if let Some(handle) = handle {
        aborted()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(handle);
    }
}

/// Take the next decoded request for `server_handle`, if any.
pub(crate) fn take_pending(server_handle: i64) -> Option<HttpPendingRequest> {
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(&server_handle)
        .and_then(|q| q.pop_front())
}

fn queue_pending(server_handle: i64, request: HttpPendingRequest) {
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(server_handle)
        .or_default()
        .push_back(request);
}

fn with_conn<R>(id: i64, f: impl FnOnce(&mut Conn) -> R) -> Option<R> {
    let mut map = conns().lock().unwrap_or_else(|e| e.into_inner());
    map.get_mut(&id).map(f)
}

/// Every live turnloop connection of one server.
pub(crate) fn connections_of(server_handle: i64) -> Vec<i64> {
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, c)| c.server_handle == server_handle)
        .map(|(id, _)| *id)
        .collect()
}

/// Whether a connection has a request in flight (`closeIdleConnections`).
pub(crate) fn is_busy(id: i64) -> bool {
    with_conn(id, |c| c.active.is_some() || c.building.is_some()).unwrap_or(false)
}

// ── Completion sink ─────────────────────────────────────────────────────────

pub(crate) extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime passes a live completion for the duration of the
    // call, which is this function's body.
    let c = unsafe { &*completion };
    // HTTP/2 shares this subsystem slot (see `turnloop_h2`'s module docs), and
    // answers first by id. A completion it claims never reaches the HTTP/1.1
    // state machine below.
    if crate::server::turnloop_h2::intercept(c) {
        return;
    }
    match c.kind {
        tl::NET_ACCEPT => on_accept(c.id, c.conn),
        // SAFETY: same call; the pooled lease outlives it.
        tl::NET_DATA => on_data(c.id, unsafe { c.bytes() }),
        tl::NET_EOF => on_eof(c.id),
        tl::NET_WROTE => on_wrote(c.id, c.len),
        tl::NET_CLOSED => on_closed(c.id),
        tl::NET_TIMER => on_timer(c.id),
        tl::NET_ERROR => {
            // SAFETY: same call; both point at `'static` string data.
            let (code, syscall) = unsafe { (c.code(), c.syscall()) };
            if crate::server::turnloop_h2::intercept_listener_error(c.id, c.terminal != 0) {
                return;
            }
            on_error(c.id, code, syscall, c.terminal != 0);
        }
        _ => {}
    }
}

fn on_accept(listener_id: i64, conn_id: i64) {
    if conn_id == 0 {
        return;
    }
    let Some((server_handle, tls, idle_close_ms)) = super::with_listener(listener_id, |l| {
        (l.server_handle, l.tls.clone(), l.idle_close_ms)
    }) else {
        let _ = tl::close(conn_id);
        return;
    };
    start_connection(conn_id, server_handle, tls, idle_close_ms);
}

/// Serve a connection that is already on this thread's loop under `conn_id`
/// — one turnloop accepted from a listener, or one adopted from a descriptor
/// another process accepted (a SCHED_RR cluster worker's, `adopt_connection`).
pub(crate) fn start_connection(
    conn_id: i64,
    server_handle: i64,
    tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    idle_close_ms: u64,
) {
    let secure = tls.is_some();
    if let Some(config) = tls {
        if let Err(_message) =
            perry_ext_net::turnloop_tls_io::install_server_session(conn_id, config)
        {
            let _ = tl::close(conn_id);
            return;
        }
    }
    let peer = tl::peer_address(conn_id);
    let peer_address = peer.as_ref().map(|e| e.address.clone()).unwrap_or_default();
    let peer_port = peer.as_ref().map(|e| e.port).unwrap_or(0);
    let keep_alive_timeout_ms =
        with_base_server(server_handle, |s| s.keep_alive_timeout).unwrap_or(5_000.0);
    let socket_handle = alloc_connection_socket(peer_address.clone(), peer_port);
    conns().lock().unwrap_or_else(|e| e.into_inner()).insert(
        conn_id,
        Conn {
            id: conn_id,
            server_handle,
            peer_address,
            peer_port,
            socket_handle,
            decoder: http1::Decoder::new(http1::Mode::Request, Default::default()),
            input: Vec::with_capacity(8 * 1024),
            building: None,
            active: None,
            seq: 0,
            requests: 0,
            idle_close_ms,
            keep_alive_timeout_ms,
            paused: false,
            read_eof: false,
            closing: false,
            destroyed: false,
            secure,
            handshaking: secure,
            websocket: false,
        },
    );
    crate::server::server::queue_turnloop_connection_event(server_handle, socket_handle);
    arm_idle(conn_id);
    if let Err(_err) = tl::read_start(conn_id) {
        destroy_connection(conn_id);
    }
}

/// Adopt a TLS connection whose ALPN chose `http/1.1` from the HTTP/2 listener
/// (`http2.createSecureServer({ allowHTTP1: true })`).
///
/// The socket keeps its id, its installed TLS layer and its outstanding
/// multishot read: only the owning table changes, because both halves live in
/// the same subsystem slot. `leftover` is whatever plaintext the HTTP/2 side
/// had buffered but not decoded — with ALPN there is normally none, but a
/// client that pipelined its first request into the handshake's last flight
/// would lose it otherwise.
///
/// Returns false when no `Conn` could be made, in which case the caller closes
/// the socket rather than leaving an orphan.
pub(crate) fn adopt_alpn_http1(
    id: i64,
    server_handle: i64,
    peer_address: String,
    peer_port: u16,
    leftover: Vec<u8>,
) -> bool {
    // `id` is a connection, not a listener, so the idle deadline comes from the
    // server the same way P5's own `listen` derived it.
    let idle_close_ms =
        with_base_server(server_handle, crate::server::server::idle_close_ms).unwrap_or(0);
    let keep_alive_timeout_ms =
        with_base_server(server_handle, |s| s.keep_alive_timeout).unwrap_or(5_000.0);
    let mut input = Vec::with_capacity(8 * 1024);
    input.extend_from_slice(&leftover);
    // Pre-existing gap, not this fix's scope: this ALPN handoff never queued
    // `'connection'` (no `queue_turnloop_connection_event` call below) even
    // before connection sockets existed. `req.socket` on it is still wired so a
    // request landing here is consistent with the ordinary accept path.
    let socket_handle = alloc_connection_socket(peer_address.clone(), peer_port);
    conns().lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        Conn {
            id,
            server_handle,
            peer_address,
            peer_port,
            socket_handle,
            decoder: http1::Decoder::new(http1::Mode::Request, Default::default()),
            input,
            building: None,
            active: None,
            seq: 0,
            requests: 0,
            idle_close_ms,
            keep_alive_timeout_ms,
            paused: false,
            read_eof: false,
            closing: false,
            destroyed: false,
            secure: true,
            // The handshake is already complete: that is what decided ALPN.
            handshaking: false,
            websocket: false,
        },
    );
    if !leftover.is_empty() {
        decode(id);
    }
    true
}

fn on_data(id: i64, bytes: &[u8]) {
    // Every read refreshes the idle deadline; the connection is only "idle"
    // between a completed response and the next request byte. Park it rather
    // than cancel it: cancelling destroys the timer handle, and turnloop
    // answers that with a `Cancelled` and a `Closed` — two completions per
    // request, both routed nowhere — after which `arm_idle` has to build a
    // fresh handle. Parking keeps the handle, so this disarm and the re-arm in
    // `complete_response` are both a deadline move, which costs nothing.
    park_idle(id);
    let plaintext: Option<Vec<u8>> = if with_conn(id, |c| c.secure).unwrap_or(false) {
        match perry_ext_net::turnloop_tls_io::receive(id, bytes) {
            Some(received) => {
                if received.peer_closed {
                    // A TLS close_notify is the readable EOF.
                    let text = received.plaintext;
                    if !text.is_empty() {
                        feed(id, &text);
                    }
                    on_eof(id);
                    return;
                }
                Some(received.plaintext)
            }
            // The layer is gone (the handshake failed and destroyed the
            // connection); there is nothing to decode.
            None => return,
        }
    } else {
        None
    };
    if with_conn(id, |c| c.handshaking).unwrap_or(false)
        && perry_ext_net::turnloop_tls_io::handshake_done(id)
    {
        with_conn(id, |c| c.handshaking = false);
    }
    match plaintext {
        Some(text) if !text.is_empty() => feed(id, &text),
        Some(_) => {}
        None => feed(id, bytes),
    }
}

/// Is this connection carrying a WebSocket rather than HTTP?
fn is_websocket(id: i64) -> bool {
    with_conn(id, |c| c.websocket).unwrap_or(false)
}

fn feed(id: i64, bytes: &[u8]) {
    if is_websocket(id) {
        // Past the 101 these are frames, not HTTP. The connection, its id, its
        // outstanding multishot read and its TLS layer are all unchanged — only
        // who decodes the bytes.
        perry_ext_ws::turnloop_link::on_data(id, bytes);
        return;
    }
    let known = with_conn(id, |c| c.input.extend_from_slice(bytes)).is_some();
    if known {
        decode(id);
    }
}

/// Drain as much of the buffered input as the connection is allowed to decode.
///
/// A connection decodes exactly one message ahead of its response: the decoder
/// is only `reset()` once the current response has been written, so a pipelined
/// request stays in `input` and is dispatched afterwards. That is Node's
/// per-connection serialization, and it is also what makes `res` unambiguous.
fn decode(id: i64) {
    loop {
        enum Step {
            Idle,
            Again,
            /// A decoded request, and whether the client is waiting for a
            /// `100 Continue` before it sends the body.
            Dispatch(HttpPendingRequest, bool),
            Upgrade(Building),
            /// A WebSocket upgrade an attached `WebSocketServer` will answer.
            WebSocket(Building),
            Failed(&'static str),
        }
        let step = with_conn(id, |c| {
            if c.destroyed || c.paused || c.handshaking {
                return Step::Idle;
            }
            let step = match c.decoder.receive(&c.input) {
                Ok(step) => step,
                Err(e) => return Step::Failed(e.code),
            };
            let consumed = step.consumed;
            let mut outcome = Step::Idle;
            match step.event {
                Some(http1::Event::Head(head)) => {
                    c.building = Some(building_from(&head));
                    outcome = Step::Again;
                }
                Some(http1::Event::Body(chunk)) => {
                    if let Some(b) = c.building.as_mut() {
                        b.body.extend_from_slice(chunk);
                    }
                    outcome = Step::Again;
                }
                Some(http1::Event::Trailers(_)) => outcome = Step::Again,
                // `Upgrade` joins `End` here rather than getting its own arm.
                // turnloop-http 0.1.0-alpha.7 made the REQUEST-mode decoder end
                // an upgrade message with `Event::Upgrade` INSTEAD of
                // `Event::End` (`upgrade_request` = CONNECT, or HTTP/1.1 with
                // `Upgrade` + `Connection: upgrade`). Both mean the same thing
                // on this side — the message is complete — and all the routing
                // policy lives below, so they must not diverge.
                //
                // This was a silent regression waiting to happen: the old
                // `Event::Upgrade` arm was written as unreachable and routed
                // straight to `Step::Upgrade`, so once alpha.7 started raising
                // it, every upgrade would have bypassed BOTH the attached
                // `WebSocketServer` precedence and the `has_upgrade_listener`
                // test (#4973: an upgrade with no listener is served as an
                // ordinary request), and a CONNECT — which sets
                // `upgrade_request` but never `Building::upgrade` — would have
                // stopped being dispatched as a request at all. None of that is
                // a compile error, because the arm already existed.
                Some(http1::Event::End | http1::Event::Upgrade) => {
                    outcome = match c.building.take() {
                        // A WebSocket upgrade with a `WebSocketServer` attached
                        // to this server is answered here, before the generic
                        // `'upgrade'` route — that is `ws`'s own precedence,
                        // and it is the case P5 had to decline.
                        Some(building)
                            if building.websocket
                                && perry_ext_ws::has_attached_server(c.server_handle) =>
                        {
                            c.paused = true;
                            Step::WebSocket(building)
                        }
                        // Node dispatches an upgrade request to `'upgrade'`
                        // instead of `'request'` — but only when a listener
                        // exists; with none it is served as an ordinary
                        // request, which is #4973's rule.
                        Some(building)
                            if building.upgrade && has_upgrade_listener(c.server_handle) =>
                        {
                            c.paused = true;
                            Step::Upgrade(building)
                        }
                        Some(building) => {
                            c.requests += 1;
                            c.seq += 1;
                            c.paused = true;
                            let (request, send_continue) = finish_request(c, building);
                            Step::Dispatch(request, send_continue)
                        }
                        None => Step::Again,
                    };
                }
                Some(http1::Event::Informational(_)) => outcome = Step::Again,
                None => {
                    if consumed > 0 {
                        outcome = Step::Again;
                    }
                }
            }
            c.input.drain(..consumed.min(c.input.len()));
            outcome
        });
        match step {
            None | Some(Step::Idle) => return,
            Some(Step::Again) => continue,
            Some(Step::Dispatch(request, send_continue)) => {
                let server_handle = request.server_handle;
                queue_pending(server_handle, request);
                // Outside the connection borrow: `write_raw` takes the same
                // lock, and `std::sync::Mutex` is not reentrant.
                if send_continue {
                    write_raw(id, b"HTTP/1.1 100 Continue\r\n\r\n");
                }
                return;
            }
            Some(Step::Upgrade(building)) => {
                on_upgrade(id, building);
                return;
            }
            Some(Step::WebSocket(building)) => {
                on_websocket(id, building);
                return;
            }
            Some(Step::Failed(code)) => {
                bad_request(id, code);
                return;
            }
        }
    }
}

fn has_upgrade_listener(server_handle: i64) -> bool {
    with_base_server(server_handle, |server| {
        server
            .listeners
            .get("upgrade")
            .is_some_and(|l| !l.is_empty())
    })
    .unwrap_or(false)
}

fn building_from(head: &http1::Head) -> Building {
    let mut headers_lower = HashMap::new();
    let mut raw_headers = Vec::with_capacity(head.headers.len());
    for header in &head.headers {
        let Ok(value) = std::str::from_utf8(&header.value) else {
            continue;
        };
        // `http1::Header::name` is already lowercase: `Decoder` lowercases as
        // it parses, which matches Node's `req.headers` and leaves
        // `req.rawHeaders` reporting the same name. (Node's rawHeaders keeps
        // the sender's case; that difference is the decoder's, and it is
        // recorded in the P5 report rather than papered over here.)
        headers_lower.insert(header.name.clone(), value.to_string());
        raw_headers.push((header.name.clone(), value.to_string()));
    }
    let connection = headers_lower.get("connection").cloned();
    let websocket_upgrade = headers_lower
        .get("upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        && headers_lower.contains_key("sec-websocket-key");
    let upgrade = headers_lower.contains_key("upgrade")
        && connection.as_deref().is_some_and(|v| {
            v.to_ascii_lowercase()
                .split(',')
                .any(|t| t.trim() == "upgrade")
        });
    let expects_continue = headers_lower
        .get("expect")
        .is_some_and(|v| v.to_ascii_lowercase().contains("100-continue"));
    Building {
        method: head.method.clone(),
        url: head.target.clone(),
        headers_lower,
        raw_headers,
        body: Vec::new(),
        version: head.version,
        expects_continue,
        connection,
        upgrade,
        websocket: upgrade && websocket_upgrade,
    }
}

/// Turn a fully decoded request into the `(req, res)` handle pair the pump
/// dispatches.
fn finish_request(c: &mut Conn, building: Building) -> (HttpPendingRequest, bool) {
    let mut im = IncomingMessage::new(
        building.method.clone(),
        building.url.clone(),
        building.headers_lower.clone(),
        building.raw_headers.clone(),
        building.body,
        c.peer_address.clone(),
        c.peer_port,
    );
    im.http_version = if building.version == 0 {
        "1.0".to_string()
    } else {
        "1.1".to_string()
    };
    let im_handle = alloc_incoming_message(im);
    // `req.socket === conn` for the
    // `'connection'` listener's argument — same handle, every request on
    // this connection.
    incoming_socket_assign(im_handle, handle_to_pointer_f64(c.socket_handle));
    let sr_handle = alloc_server_response_for_turnloop(c.id, c.seq, im_handle);

    let is_check_continue = building.expects_continue
        && with_base_server(c.server_handle, |server| {
            server
                .listeners
                .get("checkContinue")
                .is_some_and(|l| !l.is_empty())
        })
        .unwrap_or(false);
    // Node's `100 Continue` is automatic unless a `'checkContinue'` listener
    // takes over. hyper sent it when the body was polled; here the caller
    // sends it as soon as the head says the client is waiting, once it has
    // released the connection borrow.
    let send_continue = building.expects_continue && !is_check_continue;

    c.active = Some(Active {
        seq: c.seq,
        request_handle: im_handle,
        method: building.method,
        version: building.version,
        connection: building.connection,
        encoder: None,
        framing: Framing::Sized(0),
        head_sent: false,
        keep_alive: true,
    });

    (
        HttpPendingRequest {
            server_handle: c.server_handle,
            request_handle: im_handle,
            response_handle: sr_handle,
            skip_default_response: false,
            h2_stream_handle: 0,
            h2_stream_headers: Vec::new(),
            is_check_continue,
        },
        send_continue,
    )
}

// ── Response side, called from `ServerResponse` on the main thread ──────────

/// Whether `seq` still names the request this connection is answering.
fn owns(c: &Conn, seq: u64) -> bool {
    c.active.as_ref().is_some_and(|a| a.seq == seq) && !c.destroyed
}

/// Decide the response's `Connection` / `Keep-Alive` headers and whether the
/// connection survives it.
fn prepare_headers(c: &mut Conn, shape: &mut ResponseShape) -> bool {
    let (version, connection) = {
        let active = c.active.as_ref().expect("an active request");
        (active.version, active.connection.clone())
    };
    let server_closing =
        with_base_server(c.server_handle, |server| !server.listening).unwrap_or(false);
    let max_requests =
        with_base_server(c.server_handle, |server| server.max_requests_per_socket).unwrap_or(0.0);
    let over_quota = max_requests > 0.0 && c.requests as f64 >= max_requests;
    let default_connection = if server_closing || over_quota {
        Some("close".to_string())
    } else {
        connection
    };
    shape.apply_default_connection_headers_for(
        version,
        default_connection.as_deref(),
        c.keep_alive_timeout_ms,
    );
    let keep_alive = shape
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("connection"))
        .is_some_and(|(_, v)| v.eq_ignore_ascii_case("keep-alive"));
    if let Some(active) = c.active.as_mut() {
        active.keep_alive = keep_alive;
    }
    keep_alive
}

/// `res.end()` on a fully buffered response.
pub(crate) fn send_response(conn_id: i64, seq: u64, mut shape: ResponseShape) {
    let bytes = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let keep_alive = prepare_headers(c, &mut shape);
        let (method, version) = {
            let a = c.active.as_ref().expect("an active request");
            (a.method.clone(), a.version)
        };
        let body = std::mem::take(&mut shape.body);
        // An HTTP/1.0 response that will close the connection is close-delimited
        // in Node, with no length header — but only when the length was Perry's
        // own synthesis; a handler that set `Content-Length` keeps it.
        let eof_framed = version == 0 && !keep_alive && shape.auto_content_length;
        let framing = wire::framing_for(
            &shape.headers,
            shape.status,
            &method,
            version,
            Some(body.len() as u64),
            eof_framed,
        );
        wire::align_headers(&mut shape.headers, framing, shape.auto_content_length);
        let head = match wire::encode_head(
            shape.status,
            shape.status_message.as_deref(),
            &shape.headers,
            framing,
        ) {
            Ok(head) => head,
            // A response the encoder refuses (a header the handler set that is
            // not a valid field, a declared length that contradicts the body)
            // must not silently vanish: answer 500 and close, which is what
            // Node does for an invalid outgoing header it catches late.
            Err(_message) => {
                return Some((
                    b"HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                        .to_vec(),
                    Framing::UntilClose,
                ))
            }
        };
        let mut out = head.bytes;
        if !matches!(framing, Framing::NoBody) && !body.is_empty() {
            match head.encoder {
                Some(mut encoder) => {
                    let _ = encoder.body(&body, &mut out);
                    let trailers: Vec<http1::Header> = shape
                        .trailers
                        .iter()
                        .map(|(name, value)| http1::Header {
                            name: name.clone(),
                            value: value.as_bytes().to_vec(),
                        })
                        .collect();
                    let _ = encoder.finish(&trailers, &mut out);
                }
                None => out.extend_from_slice(&body),
            }
        } else if let Some(mut encoder) = head.encoder {
            let _ = encoder.finish(&[], &mut out);
        }
        if let Some(active) = c.active.as_mut() {
            active.head_sent = true;
            active.framing = framing;
        }
        Some((out, framing))
    });
    let Some(Some((out, framing))) = bytes else {
        return;
    };
    write_raw(conn_id, &out);
    complete_response(conn_id, seq, framing);
}

/// Write an interim (1xx) response on the connection answering `seq`.
///
/// `res.writeContinue()` and `res.writeProcessing()` were no-ops on the hyper
/// path — hyper sent `100 Continue` itself when it first polled the body, and
/// 102 was never sent at all. On turnloop nothing is automatic once a
/// `'checkContinue'` listener has taken the request over, so the call has to
/// reach the wire.
pub(crate) fn send_interim(conn_id: i64, seq: u64, bytes: &[u8]) {
    let ours = with_conn(conn_id, |c| owns(c, seq)).unwrap_or(false);
    if ours {
        write_raw(conn_id, bytes);
    }
}

/// `res.flushHeaders()` / the first `res.write(...)`: send the head now and
/// stream the body afterwards.
pub(crate) fn begin_stream(conn_id: i64, seq: u64, mut shape: ResponseShape) -> bool {
    let prepared = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        prepare_headers(c, &mut shape);
        let (method, version) = {
            let a = c.active.as_ref().expect("an active request");
            (a.method.clone(), a.version)
        };
        let framing =
            wire::framing_for(&shape.headers, shape.status, &method, version, None, false);
        wire::align_headers(&mut shape.headers, framing, shape.auto_content_length);
        let head = match wire::encode_head(
            shape.status,
            shape.status_message.as_deref(),
            &shape.headers,
            framing,
        ) {
            Ok(head) => head,
            Err(_message) => return None,
        };
        if let Some(active) = c.active.as_mut() {
            active.head_sent = true;
            active.framing = framing;
            active.encoder = head.encoder;
        }
        Some(head.bytes)
    });
    let Some(Some(head_bytes)) = prepared else {
        return false;
    };
    write_raw(conn_id, &head_bytes);
    true
}

/// A streaming `res.write(chunk)`.
pub(crate) fn send_body(conn_id: i64, seq: u64, bytes: &[u8]) -> bool {
    let framed = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let active = c.active.as_mut().expect("an active request");
        if matches!(active.framing, Framing::NoBody) {
            // A HEAD (or 204/304) response writes no body bytes, but the call
            // still succeeds — Node accepts the write and drops it.
            return Some(Vec::new());
        }
        let mut out = Vec::with_capacity(bytes.len() + 16);
        match active.encoder.as_mut() {
            Some(encoder) => {
                let _ = encoder.body(bytes, &mut out);
            }
            None => out.extend_from_slice(bytes),
        }
        Some(out)
    });
    let Some(Some(out)) = framed else {
        return false;
    };
    if !out.is_empty() {
        write_raw(conn_id, &out);
    }
    true
}

/// A streaming `res.end()`: close the body framing and finish the exchange.
pub(crate) fn finish_body(conn_id: i64, seq: u64, trailers: &[(String, String)]) {
    let framed = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let active = c.active.as_mut().expect("an active request");
        let mut out = Vec::new();
        if let Some(encoder) = active.encoder.as_mut() {
            let headers: Vec<http1::Header> = trailers
                .iter()
                .map(|(name, value)| http1::Header {
                    name: name.clone(),
                    value: value.as_bytes().to_vec(),
                })
                .collect();
            let _ = encoder.finish(&headers, &mut out);
        }
        Some((out, active.framing))
    });
    let Some(Some((out, framing))) = framed else {
        return;
    };
    if !out.is_empty() {
        write_raw(conn_id, &out);
    }
    complete_response(conn_id, seq, framing);
}

/// Retire the answered request and decide the connection's fate.
fn complete_response(conn_id: i64, seq: u64, framing: Framing) {
    let decision = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let keep_alive = c.active.as_ref().is_some_and(|a| a.keep_alive);
        c.active = None;
        c.paused = false;
        // A close-delimited body ends *by* closing, so the connection cannot
        // be reused whatever the headers said.
        let reuse = keep_alive
            && framing != Framing::UntilClose
            && !c.closing
            && !c.read_eof
            // `reset` refuses a decoder the request itself made unreusable (a
            // `Connection: close` request, an unframed body). Trusting the
            // response headers alone would leave the next request parsed
            // against a decoder that never restarted.
            && c.decoder.reset().is_ok();
        Some(reuse)
    });
    match decision {
        Some(Some(true)) => {
            arm_idle(conn_id);
            // A pipelined request may already be buffered.
            decode(conn_id);
        }
        Some(Some(false)) => finish_and_close(conn_id),
        _ => {}
    }
}

/// `res.destroy()` / `socket.destroy()` on the turnloop connection.
pub(crate) fn destroy_connection(conn_id: i64) {
    note_aborted(conn_id);
    let known = with_conn(conn_id, |c| {
        c.destroyed = true;
        c.closing = true;
    })
    .is_some();
    if known {
        cancel_idle(conn_id);
        let _ = tl::close(conn_id);
    }
}

/// End the write side and close once it has drained. turnloop orders a
/// handle's writes ahead of its shutdown, so a completed shutdown means every
/// queued byte left — closing outright would cancel them.
fn finish_and_close(conn_id: i64) {
    cancel_idle(conn_id);
    let secure = with_conn(conn_id, |c| {
        c.closing = true;
        c.secure
    })
    .unwrap_or(false);
    if secure {
        // `close_notify` first, then the FIN, then the close.
        let _ = perry_ext_net::turnloop_tls_io::shutdown(conn_id, 0);
        return;
    }
    if tl::shutdown(conn_id, 0).is_err() {
        let _ = tl::close(conn_id);
    }
}

fn write_raw(conn_id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let secure = with_conn(conn_id, |c| c.secure).unwrap_or(false);
    let result = if secure {
        perry_ext_net::turnloop_tls_io::write(conn_id, bytes, 0).map(|_| ())
    } else {
        tl::write(conn_id, bytes, 0)
            .map(|_| ())
            .map_err(|e| e.message())
    };
    if result.is_err() {
        destroy_connection(conn_id);
    }
}

/// Answer a malformed request the way Node does: one 400, then close.
fn bad_request(conn_id: i64, _code: &str) {
    write_raw(
        conn_id,
        b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
    );
    with_conn(conn_id, |c| {
        c.closing = true;
        c.building = None;
        c.active = None;
    });
    finish_and_close(conn_id);
}

// ── Terminal completions ────────────────────────────────────────────────────

fn on_eof(id: i64) {
    if is_websocket(id) {
        // An upgraded connection has no request in flight and no response to
        // finish; `ws` reports a missing close frame as 1006. Our own side is
        // closed here rather than by the ws layer, which owns the protocol and
        // not the connection — but only if the close handshake had not already
        // finished. Shutting down twice answers `ENOTCONN`, and answering that
        // with a destroy resets a connection whose answering close frame is
        // still on the wire.
        if perry_ext_ws::turnloop_link::on_eof(id) {
            finish_and_close(id);
        }
        return;
    }
    let state = with_conn(id, |c| {
        // A TLS connection reaches EOF twice — the peer's `close_notify` and
        // then the TCP FIN — and the close must only be driven once.
        let already = std::mem::replace(&mut c.read_eof, true) || c.closing;
        (already, c.active.is_some(), c.building.is_some())
    });
    let Some((already, answering, partial)) = state else {
        return;
    };
    if already {
        return;
    }
    if answering {
        // The peer stopped sending before its response was written. Node's
        // server socket is `allowHalfOpen: false`, so its own EOF closes the
        // socket and `abortIncoming` raises `'aborted'` on every request whose
        // response never completed — which is this one. Noting it here rather
        // than at the terminal `Closed` is what makes it observable at all:
        // this connection stays open until the handler answers, and by then
        // the request has been retired.
        note_aborted(id);
    }
    if partial {
        // A half-sent request: Node destroys the socket without answering.
        destroy_connection(id);
        return;
    }
    if !answering {
        finish_and_close(id);
    }
    // A request still being answered keeps the connection until its response
    // has been written; `complete_response` sees `read_eof` and closes.
}

fn on_wrote(_id: i64, _len: usize) {
    // Backpressure for `res.write()`'s boolean return is read directly from
    // `tl::queued_bytes` at the call site, so a write completion needs no
    // bookkeeping here.
}

fn on_closed(id: i64) {
    if is_websocket(id) {
        // The ws side has to learn the connection is gone before the id is
        // recycled, or a later connection drawing the same id would find a
        // stale link — the same class of bug the `turnloop_tls_io::forget`
        // comment below records.
        perry_ext_ws::turnloop_link::on_closed(id);
    }
    // A peer that vanished mid-request reaches the terminal `Closed` without
    // ever passing through `destroy_connection`.
    note_aborted(id);
    cancel_idle(id);
    let removed = conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    let owned = removed.is_some();
    // The connection is fully gone — queue its
    // socket's `'close'` for the pump's next tick. This sink runs inside
    // `dispatch_staged` and must not run JS (same rule as `note_aborted`
    // above), so `js_node_http_server_process_pending` (via
    // `take_closed_sockets`) fires the listeners instead.
    if let Some(conn) = removed {
        if conn.socket_handle != 0 {
            note_closed_socket(conn.socket_handle);
        }
    }
    crate::server::server::turnloop_connection_closed(id);
    if owned {
        // The rustls session has to go BEFORE the id does. `turnloop_tls_io`
        // keys its layer table by connection id, and nothing on this
        // subsystem's terminal path was dropping it — perry-ext-net's
        // `emit_close_once` is the only caller of `forget`, and that is
        // subsystem 0's socket path, not this one. So every HTTPS connection
        // left a `Layer` (a rustls session plus its buffers) behind for the
        // life of the process, and — worse — once `free_handle_id` handed the
        // id back and the next accepted connection drew it, `install_server_
        // session` answered "socket is already TLS" and the connection was
        // closed before a byte was read.
        //
        // Found through the HTTP/2 `allowHTTP1` handoff, which routes an ALPN
        // `http/1.1` connection here and then closes it: the next TLS
        // connection to that server got EOF, every time.
        perry_ext_net::turnloop_tls_io::forget(id);
        // The terminal completion: no completion can name this id again, and
        // unlike a `net.Socket` id there is no JS object still holding it, so
        // it goes back to the shared band instead of leaking one id per
        // connection for the life of a server (the #6441 exhaustion class).
        perry_ffi::free_handle_id(id);
    }
}

fn on_timer(id: i64) {
    // The idle keep-alive deadline. Node closes the connection; an exchange
    // that started in the meantime cancelled the deadline already.
    let idle = with_conn(id, |c| c.active.is_none() && c.building.is_none()).unwrap_or(false);
    if idle {
        finish_and_close(id);
    }
}

fn on_error(id: i64, code: Option<&str>, syscall: Option<&str>, terminal: bool) {
    if super::with_listener(id, |_| ()).is_some() {
        // A transient accept failure does not end the listener, exactly as the
        // hyper accept loop kept going on one.
        if terminal {
            super::close_listener(id);
        }
        return;
    }
    if is_websocket(id) {
        let message = code.unwrap_or("WS_ERR_SOCKET");
        // Same rule, and the same reason P5 stopped reporting a rustls failure
        // raised after the application had asked to close: an error on a
        // connection this layer has already finished with is teardown noise,
        // and destroying the handle for it cancels writes that are still going
        // out.
        if perry_ext_ws::turnloop_link::on_error(id, message) {
            destroy_connection(id);
        }
        return;
    }
    let _ = (code, syscall);
    destroy_connection(id);
}

// ── Idle deadline ───────────────────────────────────────────────────────────

/// Arm the connection's idle close.
///
/// Node 26.5.1, measured: the server FINs an idle keep-alive connection at
/// `keepAliveTimeout + keepAliveTimeoutBuffer` (defaults 5000 + 1000 ms), and
/// `keepAliveTimeout = 0` disables the close entirely while *keeping*
/// keep-alive on. Zero here therefore arms nothing.
fn arm_idle(id: i64) {
    let ms = with_conn(id, |c| c.idle_close_ms).unwrap_or(0);
    if ms == 0 {
        return;
    }
    let _ = tl::timer_arm(id, super::SUBSYSTEM, ms);
}

/// Disarm the idle close for the duration of an exchange, keeping the handle.
/// Teardown still uses `cancel_idle`: there the handle really is going away.
fn park_idle(id: i64) {
    let _ = tl::timer_park(id);
}

fn cancel_idle(id: i64) {
    let _ = tl::timer_cancel(id);
}

// ── WebSocket ───────────────────────────────────────────────────────────────

/// Answer a WebSocket upgrade for a server with a `WebSocketServer` attached,
/// on the connection we already have.
///
/// This is what P5 could not do, and the reason it could not is worth naming
/// precisely: `tokio_tungstenite::WebSocketStream<S>` needs an owned
/// `AsyncRead + AsyncWrite`, and a turnloop connection is an `i64` handle id
/// with a completion sink. The protocol never needed the stream —
/// `turnloop_websocket` is sans-I/O, so the handshake is a function of the
/// request head and the framing is a function of byte slices.
///
/// So nothing moves. No `turnloop_net::transfer`, no descriptor handoff, no
/// second owner: the connection, its id, its outstanding multishot read and its
/// TLS layer all stay exactly as they are, and only the decoder changes. That
/// is the shape P5 used for TLS (a session *above* the handle), applied one
/// layer up.
fn on_websocket(id: i64, building: Building) {
    let Some((server_handle, leftover, secure)) = with_conn(id, |c| {
        (c.server_handle, std::mem::take(&mut c.input), c.secure)
    }) else {
        return;
    };
    let _ = secure;
    let head = perry_ext_ws::turnloop_link::request_head(
        &building.method,
        &building.url,
        building.version,
        &building.raw_headers,
    );
    let (response, _protocol) = match perry_ext_ws::turnloop_link::accept_response(&head, &[]) {
        Ok(accepted) => accepted,
        Err(e) => {
            // `ws` answers a malformed handshake with a 400 and closes rather
            // than dropping the connection.
            write_raw(
                id,
                &perry_ext_ws::turnloop_link::reject_response(400, &e.message),
            );
            finish_and_close(id);
            return;
        }
    };
    cancel_idle(id);
    // The 101 goes out through the ordinary write path, so an HTTPS server's
    // attached WebSocket is encrypted exactly like its HTTP responses were.
    write_raw(id, &response);
    // Flip before adopting: `adopt` decodes the pipelined leftover, which can
    // deliver a frame, and `write_raw` from that path must not re-enter the
    // HTTP encoder.
    with_conn(id, |c| {
        c.websocket = true;
        c.paused = false;
    });
    let ws_id = perry_ext_ws::turnloop_link::adopt(id, &leftover);

    let mut im = IncomingMessage::new(
        building.method,
        building.url,
        building.headers_lower,
        building.raw_headers,
        Vec::new(),
        String::new(),
        0,
    );
    im.http_version = if building.version == 0 { "1.0" } else { "1.1" }.to_string();
    im.complete = true;
    let request_handle = alloc_incoming_message(im);
    // The same queue the hyper path uses, so the main-thread drain fires
    // `wss.on('connection')` and the server's `'upgrade'` listeners in the
    // order they already ran in.
    crate::server::server::queue_turnloop_upgrade(crate::server::server::HttpPendingUpgrade {
        server_handle,
        request_handle,
        ws_id,
        raw_socket_id: 0,
        head: Vec::new(),
    });
}

/// Install this crate as `perry-ext-ws`'s turnloop transport.
///
/// `perry-ext-http` already depends on `perry-ext-ws`, so the reverse would be
/// a cycle; function pointers are the same one-way seam
/// `register_http_address_reader` uses. Both are TLS-transparent because
/// `write_raw` and `destroy_connection` are.
pub(crate) fn register_ws_transport() {
    perry_ext_ws::turnloop_link::register_transport(perry_ext_ws::turnloop_link::Transport {
        write: |id, bytes| write_raw(id, bytes),
        finish: finish_and_close,
        destroy: destroy_connection,
    });
}

// ── Upgrade ─────────────────────────────────────────────────────────────────

/// Node's `'upgrade'`: hand the whole connection to `net` as a raw
/// `net.Socket`, with the bytes that followed the head as `upgradeHead`.
///
/// Nothing is written to the wire first — Node gives the listener an untouched
/// socket, and the 101 (or a rejection) is the listener's to send. The handoff
/// itself is `turnloop_net::transfer`: the id and the outstanding multishot
/// read stay exactly as they are and only the completion route changes, so no
/// byte can be lost between the two owners and no descriptor moves.
fn on_upgrade(id: i64, building: Building) {
    let (server_handle, head) = match with_conn(id, |c| {
        c.closing = true;
        (c.server_handle, std::mem::take(&mut c.input))
    }) {
        Some(parts) => parts,
        None => return,
    };
    let has_listener = with_base_server(server_handle, |server| {
        server
            .listeners
            .get("upgrade")
            .is_some_and(|l| !l.is_empty())
    })
    .unwrap_or(false);
    if !has_listener {
        // Node destroys a connection whose upgrade nobody claimed.
        destroy_connection(id);
        return;
    }
    cancel_idle(id);
    if tl::transfer(id, perry_ext_net::TURNLOOP_SUBSYSTEM).is_err()
        || !perry_ext_net::adopt_turnloop_upgrade(id)
    {
        destroy_connection(id);
        return;
    }
    // The connection is no longer ours: drop our record without closing the
    // handle, which now belongs to `net`.
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);

    let mut im = IncomingMessage::new(
        building.method,
        building.url,
        building.headers_lower,
        building.raw_headers,
        Vec::new(),
        String::new(),
        0,
    );
    im.http_version = if building.version == 0 { "1.0" } else { "1.1" }.to_string();
    let request_handle = alloc_incoming_message(im);
    crate::server::server::queue_turnloop_upgrade(crate::server::server::HttpPendingUpgrade {
        server_handle,
        request_handle,
        ws_id: 0,
        raw_socket_id: id,
        head,
    });
}
