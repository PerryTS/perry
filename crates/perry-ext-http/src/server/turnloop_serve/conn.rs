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

use perry_ext_net::native_transport::{self as net, RootedSocket};
use perry_ffi::native_payload::{self as np, PayloadFamily};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{JsValue, TransientRootScope};
use turnloop_http::http1;

use super::wire::{self, Framing};
use crate::server::request::{alloc_incoming_message, incoming_socket_assign, IncomingMessage};
use crate::server::response::{alloc_http1_server_response_for_turnloop, ResponseShape};
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
    /// `100 Continue` already went out for this request, at head time.
    continue_sent: bool,
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
    /// `100 Continue` was already sent at head time (see `decode`), so a
    /// later `res.writeContinue()` must not send a second one.
    continue_sent: bool,
}

pub(crate) struct Conn {
    server_handle: i64,
    peer_address: String,
    peer_port: u16,
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
    /// The shutdown `finish_and_close` submitted has completed: our FIN is out.
    write_shut: bool,
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

static PARSER_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<Conn>(None);
static PARSER: PayloadFamily = PayloadFamily::new::<Conn>(
    perry_ffi::native_class_ids::HTTP_PARSER,
    "HTTPParser",
    false,
    &PARSER_VTABLE,
)
.with_constructor_length(0);

/// Explicit cleanup runs outside SocketFields::Drop. The ordinary `parser`
/// edge remains valid and CLOSED, including for a late socket close listener.
unsafe fn close_parser(owner: f64) {
    let socket = RootedSocket::new(owner);
    note_aborted(&socket);
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let parser = scope.root_nanbox(net::own_get(state.get(), "parser"));
    np::close(parser.get(), &PARSER);
}

/// Requests decoded and waiting for the main-thread pump, per JS server handle.
fn pending() -> &'static Mutex<HashMap<i64, VecDeque<HttpPendingRequest>>> {
    static PENDING: OnceLock<Mutex<HashMap<i64, VecDeque<HttpPendingRequest>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `IncomingMessage` handles whose connection died before their response
/// completed. Node raises `'aborted'` on the request; the sink cannot run JS,
/// so the pump drains this and fires the listeners on its own tick.
///
/// Each entry is tagged with the agent whose loop queued it — the server's
/// owner, since the completion sink runs there — and only that agent's pump
/// takes it (#11433).
fn aborted() -> &'static Mutex<Vec<(u64, i64)>> {
    static ABORTED: OnceLock<Mutex<Vec<(u64, i64)>>> = OnceLock::new();
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
        .push((perry_ffi::agent_post::current_agent(), handle));
}

/// Take the `IncomingMessage` handles whose connection died mid-request.
pub(crate) fn take_aborted() -> Vec<i64> {
    let mut queue = aborted().lock().unwrap_or_else(|e| e.into_inner());
    take_owned(&mut queue)
}

/// Remove and return the calling agent's entries, leaving every other agent's
/// in place for its own pump (#11433).
fn take_owned(queue: &mut Vec<(u64, i64)>) -> Vec<i64> {
    let agent = perry_ffi::agent_post::current_agent();
    let (mine, theirs): (Vec<_>, Vec<_>) = std::mem::take(queue)
        .into_iter()
        .partition(|(owner, _)| *owner == agent);
    *queue = theirs;
    mine.into_iter().map(|(_, handle)| handle).collect()
}

/// Note that this connection's in-flight request (if any) will never be
/// answered, exactly once per request.
fn note_aborted(id: &RootedSocket) {
    let handle = with_conn(id, |c| {
        c.active
            .as_mut()
            .map(|a| std::mem::replace(&mut a.request_handle, 0))
    })
    .flatten()
    .filter(|h| *h != 0);
    if let Some(handle) = handle {
        note_aborted_handle(handle);
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

/// The parser's owner is an ordinary child of the rooted Socket. `f` may
/// allocate Rust state, but must not allocate JS, call JS, close/reopen the
/// Socket, or enter the driver. Its ordinary parser edge cannot move here.
fn with_conn<R>(socket: &RootedSocket, f: impl FnOnce(&mut Conn) -> R) -> Option<R> {
    unsafe {
        socket
            .with_current(|_, state| {
                let parser = net::record_get(state, "parser");
                np::payload_mut::<Conn>(parser, &PARSER).ok().map(f)
            })
            .flatten()
    }
}

/// Visit children through the delegate's existing ordinary ownership graph.
/// Root the complete snapshot before any callback can allocate or close one.
pub(crate) fn with_connections(server_handle: i64, mut visit: impl FnMut(&RootedSocket)) {
    let Some(listener) = super::listener_for_server(server_handle) else {
        return;
    };
    net::for_each_server_child(listener, |socket| {
        // Upgraded sockets have released the HTTP parser and belong to the
        // upgraded protocol; Node excludes them from closeAllConnections.
        if with_conn(socket, |_| ()).is_some() {
            visit(socket);
        }
    });
}

/// Whether a connection has a request in flight (`closeIdleConnections`,
/// and `server.close()` since Node 19): a response under way, a request whose
/// head has been decoded, or bytes of a request whose head has not completed
/// yet. The decoder accepts a head only once it is whole (turnloop-http's
/// `is_mid_message` is false until then: "bytes of a head the host has
/// buffered but not yet completed are the host's to know"), so those bytes sit
/// in `input` with `building` still `None`. Node counts that connection as
/// active and leaves it open; treating it as idle destroyed a client mid-way
/// through sending its request (#11586, `test_issue_4971_tls_connect_options`
/// — the tokio implementation tracked this as `read_active`, and the turnloop
/// port in b77aba634 dropped it).
pub(crate) fn is_busy(id: &RootedSocket) -> bool {
    with_conn(id, |c| {
        c.active.is_some() || c.building.is_some() || !c.input.is_empty()
    })
    .unwrap_or(false)
}

// ── Completion sink ─────────────────────────────────────────────────────────

pub(crate) extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    let event = unsafe { &*completion };
    let Some(link) = event.link() else { return };
    let Some(owner) = (unsafe { np::link_event_owner(link) }) else {
        return;
    };
    if event.kind == tl::NET_DATA {
        let bytes = unsafe { event.bytes() };
        let received = unsafe {
            RootedSocket::receive(
                owner,
                event.flags & tl::NET_FLAG_PLAINTEXT != 0,
                bytes.len(),
                true,
                |socket, state| {
                    let parser = net::record_get(state, "parser");
                    np::payload_mut::<Conn>(parser, &PARSER)
                        .ok()
                        .map(|conn| feed_step(socket, conn, bytes))
                },
            )
        };
        match received {
            Some((socket, Some(Some(step)))) => deliver_feed(&socket, bytes, step),
            Some((socket, None)) => net::receive_tls(socket.value(), bytes),
            _ => {}
        }
        return;
    }
    let socket = RootedSocket::new(owner);
    match event.kind {
        tl::NET_EOF => on_eof(&socket),
        tl::NET_SHUTDOWN => {
            on_shutdown(&socket);
            unsafe {
                net::dispatch_common(event);
            }
        }
        tl::NET_CLOSED => {
            if event.flags & tl::NET_FLAG_STALE == 0 {
                on_closed(&socket)
            }
            unsafe {
                net::dispatch_common(event);
            }
        }
        tl::NET_TIMER => on_timer(&socket),
        tl::NET_ERROR => on_error(
            &socket,
            unsafe { event.code() },
            unsafe { event.syscall() },
            event.terminal != 0,
        ),
        _ => unsafe {
            net::dispatch_common(event);
        },
    }
}

/// An accepted/adopted connection is already an ordinary Socket with its
/// handle installed. No id or fake IncomingMessage socket is allocated.
pub(crate) fn start_connection(owner: f64, server_handle: i64) {
    initialize_connection(owner, server_handle, true);
}

fn initialize_connection(owner: f64, server_handle: i64, announce: bool) {
    let socket = RootedSocket::new(owner);
    let peer_address = net::endpoint(socket.value(), true)
        .map(|endpoint| endpoint.address)
        .unwrap_or_default();
    let peer_port = net::endpoint(socket.value(), true)
        .map(|endpoint| endpoint.port)
        .unwrap_or(0);
    let (idle_close_ms, keep_alive_timeout_ms) = with_base_server(server_handle, |server| {
        (
            crate::server::server::idle_close_ms(server),
            server.keep_alive_timeout,
        )
    })
    .unwrap_or((0, 5_000.0));
    let secure = net::tls_installed(socket.value());
    let scope = TransientRootScope::enter();
    let parser = scope.root_nanbox(unsafe {
        np::alloc_in(
            &PARSER,
            "",
            Conn {
                server_handle,
                peer_address,
                peer_port,
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
                write_shut: false,
                destroyed: false,
                secure,
                handshaking: secure,
                websocket: false,
            },
            std::mem::size_of::<Conn>() + 8 * 1024,
            &[],
        )
    });
    unsafe {
        net::set_codec(socket.value(), "parser", parser.get(), close_parser);
    }
    if net::set_route(socket.value(), super::SUBSYSTEM).is_err() {
        net::destroy(socket.value());
        return;
    }
    if announce {
        crate::server::server::emit_connection(server_handle, socket.value());
    }
    if socket.is_current() {
        arm_idle(&socket);
    }
}

/// The ALPN handoff replaces a separate codec on the same rooted Socket.
pub(crate) fn adopt_alpn_http1(socket: f64, server_handle: i64, leftover: Vec<u8>) -> bool {
    let socket = RootedSocket::new(socket);
    initialize_connection(socket.value(), server_handle, false);
    with_conn(&socket, |connection| connection.handshaking = false);
    if !leftover.is_empty() {
        feed(&socket, &leftover);
    }
    with_conn(&socket, |_| ()).is_some()
}

/// Is this connection carrying a WebSocket rather than HTTP?
fn is_websocket(id: &RootedSocket) -> bool {
    with_conn(id, |c| c.websocket).unwrap_or(false)
}

fn feed_step(id: &RootedSocket, c: &mut Conn, bytes: &[u8]) -> Option<Step> {
    c.handshaking = false;
    if c.websocket {
        return None;
    }
    c.input.extend_from_slice(bytes);
    Some(decode_step(id, c))
}

fn deliver_feed(id: &RootedSocket, bytes: &[u8], step: Option<Step>) {
    match step {
        Some(step) => decode_after(id, Some(step)),
        // End the HTTP borrow before WebSocket callbacks can close/reopen.
        None => perry_ext_ws::native_socket::on_data(id.value(), bytes),
    }
}

fn feed(id: &RootedSocket, bytes: &[u8]) {
    if let Some(step) = with_conn(id, |c| feed_step(id, c, bytes)) {
        deliver_feed(id, bytes, step);
    }
}

/// Drain as much of the buffered input as the connection is allowed to decode.
///
/// A connection decodes exactly one message ahead of its response: the decoder
/// is only `reset()` once the current response has been written, so a pipelined
/// request stays in `input` and is dispatched afterwards. That is Node's
/// per-connection serialization, and it is also what makes `res` unambiguous.
enum Step {
    Idle,
    Again,
    /// A decoded request, and whether the client is waiting for a
    /// `100 Continue` before it sends the body.
    Dispatch(HttpPendingRequest, bool),
    /// A head with `Expect: 100-continue` was decoded: answer
    /// `100 Continue` now, then keep decoding (the body follows it).
    Continue,
    Upgrade(Building),
    /// A WebSocket upgrade an attached `WebSocketServer` will answer.
    WebSocket(Building),
    Failed(&'static str),
}

fn decode_step(id: &RootedSocket, c: &mut Conn) -> Step {
    loop {
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
                let mut building = building_from(&head);
                // A client that sent `Expect: 100-continue` withholds the
                // body until it sees `100 Continue`, and this decoder only
                // dispatches a request at its END — so waiting for the
                // dispatch to send it deadlocked every such request
                // (#5080's test_http_100_continue_5080, a turnloop
                // regression: hyper sent it when the body was first
                // polled). Send it as soon as the head arrives, as hyper
                // did; a `'checkContinue'` listener still receives the
                // request, and its `writeContinue()` then has nothing left
                // to send.
                if building.expects_continue && building.version != 0 {
                    building.continue_sent = true;
                    outcome = Step::Continue;
                } else {
                    outcome = Step::Again;
                }
                c.building = Some(building);
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
                    Some(building) if building.upgrade && has_upgrade_listener(c.server_handle) => {
                        c.paused = true;
                        Step::Upgrade(building)
                    }
                    Some(building) => {
                        c.requests += 1;
                        c.seq += 1;
                        c.paused = true;
                        let (request, send_continue) = finish_request(id, c, building);
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
        if matches!(outcome, Step::Again) {
            continue;
        }
        return outcome;
    }
}

fn decode(id: &RootedSocket) {
    decode_after(id, None);
}

fn decode_after(id: &RootedSocket, mut initial: Option<Step>) {
    loop {
        // Head/body/trailer steps do not run JS or allocate GC objects.
        // Keep the same proven parser until an externally visible action.
        let step = initial
            .take()
            .or_else(|| with_conn(id, |c| decode_step(id, c)));
        match step {
            None | Some(Step::Idle) => return,
            Some(Step::Again) => continue,
            Some(Step::Continue) => {
                write_raw(id, b"HTTP/1.1 100 Continue\r\n\r\n");
                continue;
            }
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
        continue_sent: false,
        connection,
        upgrade,
        websocket: upgrade && websocket_upgrade,
    }
}

/// Turn a fully decoded request into the `(req, res)` handle pair the pump
/// dispatches.
fn finish_request(
    socket: &RootedSocket,
    c: &mut Conn,
    building: Building,
) -> (HttpPendingRequest, bool) {
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
    incoming_socket_assign(im_handle, socket.value());
    let sr_handle = alloc_http1_server_response_for_turnloop(socket.value(), c.seq, im_handle);

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
    let send_continue = building.expects_continue && !is_check_continue && !building.continue_sent;

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
        continue_sent: building.continue_sent,
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
pub(crate) fn send_response(conn_id: &RootedSocket, seq: u64, mut shape: ResponseShape) {
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
            // end() already synthesized a length if the headers were open.
            // A buffered body cannot change framing committed by writeHead().
            None,
            eof_framed,
        );
        if framing == Framing::UntilClose {
            shape.headers.retain(|(name, _)| {
                !name.eq_ignore_ascii_case("connection") && !name.eq_ignore_ascii_case("keep-alive")
            });
            shape.headers.push(("Connection".into(), "close".into()));
            if let Some(active) = c.active.as_mut() {
                active.keep_alive = false;
            }
        }
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
pub(crate) fn send_interim(conn_id: &RootedSocket, seq: u64, bytes: &[u8]) {
    let ours = with_conn(conn_id, |c| {
        owns(c, seq)
            && !(bytes.starts_with(b"HTTP/1.1 100 ")
                && c.active.as_ref().is_some_and(|a| a.continue_sent))
    })
    .unwrap_or(false);
    if ours {
        write_raw(conn_id, bytes);
    }
}

/// `res.flushHeaders()` / the first `res.write(...)`: send the head now and
/// stream the body afterwards.
pub(crate) fn begin_stream(conn_id: &RootedSocket, seq: u64, mut shape: ResponseShape) -> bool {
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
pub(crate) fn send_body(conn_id: &RootedSocket, seq: u64, bytes: &[u8]) -> bool {
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
pub(crate) fn finish_body(conn_id: &RootedSocket, seq: u64, trailers: &[(String, String)]) {
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
fn complete_response(conn_id: &RootedSocket, seq: u64, framing: Framing) {
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
pub(crate) fn destroy_connection(conn_id: &RootedSocket) {
    note_aborted(conn_id);
    net::destroy(conn_id.value());
}

fn finish_and_close(conn_id: &RootedSocket) {
    cancel_idle(conn_id);
    with_conn(conn_id, |c| c.closing = true);
    if net::shutdown(conn_id.value(), 0).is_err() {
        net::destroy(conn_id.value());
    }
}

fn write_raw(conn_id: &RootedSocket, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if conn_id
        .write(bytes, 0)
        .is_some_and(|result| result.is_err())
    {
        destroy_connection(conn_id)
    }
}

/// Answer a malformed request the way Node does: one 400, then close.
fn bad_request(conn_id: &RootedSocket, _code: &str) {
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

fn on_eof(id: &RootedSocket) {
    if is_websocket(id) {
        // An upgraded connection has no request in flight and no response to
        // finish; `ws` reports a missing close frame as 1006. Our own side is
        // closed here rather than by the ws layer, which owns the protocol and
        // not the connection — but only if the close handshake had not already
        // finished. Shutting down twice answers `ENOTCONN`, and answering that
        // with a destroy resets a connection whose answering close frame is
        // still on the wire.
        let shut = with_conn(id, |c| {
            c.read_eof = true;
            c.write_shut
        })
        .unwrap_or(false);
        if perry_ext_ws::native_socket::on_eof(id.value()) {
            finish_and_close(id);
        } else if shut {
            // The close handshake finished and our FIN went out first; this
            // FIN is the last thing either side sends (see `on_shutdown`).
            net::destroy(id.value());
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

/// The write side is shut down: every queued byte has left. Release the
/// descriptor.
///
/// Node's server socket ends an HTTP connection with `destroySoon()` —
/// `end()`, and `destroy()` on `'finish'` — so the handle is closed as soon as
/// the FIN is out, whichever side finished first. This used to wait for the
/// peer's EOF instead, and nothing did the waiting: `on_eof` returns early on
/// a connection that is already `closing`, and one whose peer FINed *before*
/// the shutdown (a keep-alive client that hangs up — `curl`, an
/// `agent: false` `http.get`) had already had its EOF. Either way the
/// connection was never closed: one descriptor per connection for the life of
/// the process (#11452).
///
/// A WebSocket is the exception, as it is in Node: `ws` ends its socket and
/// lets it go when the peer's FIN arrives too, so the close is whichever of
/// this and that EOF comes second. Only a shutdown this layer asked for
/// (`closing`) counts.
fn on_shutdown(id: &RootedSocket) {
    let close = with_conn(id, |c| {
        if !c.closing {
            return false;
        }
        c.write_shut = true;
        !c.websocket || c.read_eof
    })
    .unwrap_or(false);
    if close {
        net::destroy(id.value());
    }
}

fn on_closed(id: &RootedSocket) {
    note_aborted(id);
    unsafe { close_parser(id.value()) };
}

fn on_timer(id: &RootedSocket) {
    // The idle keep-alive deadline. Node closes the connection; an exchange
    // that started in the meantime cancelled the deadline already.
    let idle = with_conn(id, |c| c.active.is_none() && c.building.is_none()).unwrap_or(false);
    if idle {
        finish_and_close(id);
    }
}

fn on_error(id: &RootedSocket, code: Option<&str>, syscall: Option<&str>, terminal: bool) {
    if is_websocket(id) {
        let message = code.unwrap_or("WS_ERR_SOCKET");
        // Same rule, and the same reason P5 stopped reporting a rustls failure
        // raised after the application had asked to close: an error on a
        // connection this layer has already finished with is teardown noise,
        // and destroying the handle for it cancels writes that are still going
        // out.
        if perry_ext_ws::native_socket::on_error(id.value(), message) {
            destroy_connection(id);
        }
        return;
    }
    let _ = (code, syscall, terminal);
    destroy_connection(id);
}

// ── Idle deadline ───────────────────────────────────────────────────────────

/// Arm the connection's idle close.
///
/// Node 26.5.1, measured: the server FINs an idle keep-alive connection at
/// `keepAliveTimeout + keepAliveTimeoutBuffer` (defaults 5000 + 1000 ms), and
/// `keepAliveTimeout = 0` disables the close entirely while *keeping*
/// keep-alive on. Zero here therefore arms nothing.
fn arm_idle(id: &RootedSocket) {
    let ms = with_conn(id, |c| c.idle_close_ms).unwrap_or(0);
    if ms == 0 {
        return;
    }
    net::deadline_arm(id.value(), ms);
}

fn cancel_idle(id: &RootedSocket) {
    net::deadline_cancel(id.value());
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
fn on_websocket(id: &RootedSocket, building: Building) {
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
    unsafe { close_parser(id.value()) };
    let ws_id = perry_ext_ws::native_socket::adopt(id.value(), &leftover);

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
        owner_agent: perry_ffi::agent_post::current_agent(),
        raw_socket_value: f64::from_bits(JsValue::UNDEFINED.bits()),
        head: Vec::new(),
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
fn on_upgrade(id: &RootedSocket, building: Building) {
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
    #[cfg(test)]
    let sabotage_route = std::env::var("PERRY_NET_A_HTTP_SABOTAGE").as_deref() == Ok("route");
    #[cfg(not(test))]
    let sabotage_route = false;
    if !sabotage_route && net::set_route(id.value(), net::ROUTE).is_err() {
        destroy_connection(id);
        return;
    }
    unsafe { close_parser(id.value()) };

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
    incoming_socket_assign(request_handle, id.value());
    #[cfg(test)]
    let head = if std::env::var("PERRY_NET_A_HTTP_SABOTAGE").as_deref() == Ok("head") {
        Vec::new()
    } else {
        head
    };
    crate::server::server::queue_turnloop_upgrade(crate::server::server::HttpPendingUpgrade {
        server_handle,
        request_handle,
        ws_id: 0,
        owner_agent: perry_ffi::agent_post::current_agent(),
        raw_socket_value: id.value(),
        head,
    });
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod native_tests;
