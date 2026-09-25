//! One turnloop-backed HTTP/1.1 server connection.
//!
//! The whole exchange lives on the loop-owning thread: bytes arrive as a
//! `NET_DATA` completion, `turnloop_http::http1::Decoder` turns them into a
//! request, the request is handed to the [`Host`](crate::Host), and the
//! response encodes and submits its own write. There is no task, no channel
//! and no cross-thread notify anywhere on that path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;
use turnloop_http::http1;

use crate::wire::{self, Framing};
use crate::{Host, Request, Response, Upgraded};

/// The request being decoded, before it becomes a [`Request`].
struct Building {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    version: u8,
    expects_continue: bool,
    /// The request's own `Connection` header value, needed to compute the
    /// response's default `Connection` / `Keep-Alive` pair.
    connection: Option<String>,
    /// `Connection: upgrade` with an `Upgrade` header — Node dispatches this
    /// to `'upgrade'` rather than `'request'`, *if* a listener exists.
    ///
    /// The decoder does not raise `Event::Upgrade` for it:
    /// `turnloop_http::http1`'s `State::Upgrade` is only reachable in
    /// `Mode::Response` (a client reading a 101), so on the request side an
    /// upgrade is an ordinary head with no body and the server is the one that
    /// has to recognize it (PerryTS/turnloop, reported in P5).
    upgrade: bool,
}

/// The request currently being answered.
struct Active {
    seq: u64,
    method: String,
    version: u8,
    connection: Option<String>,
    encoder: Option<http1::Encoder>,
    framing: Framing,
    head_sent: bool,
    /// Keep the connection after this response, as decided at head time.
    keep_alive: bool,
    /// Nothing has raised `'aborted'` for this request yet.
    abortable: bool,
}

pub(crate) struct Conn {
    id: i64,
    listener_id: i64,
    /// The host this connection was accepted for, held by the connection
    /// itself rather than looked up through its listener.
    ///
    /// `close_listener` removes the listener, and Node's contract for
    /// `server.close()` is that in-flight connections *finish*. Resolving the
    /// host through the listener map silently dropped every later completion
    /// on those connections: a WebSocket closed after `wss.close()` never saw
    /// its peer's close frame, so neither side finished the closing handshake
    /// and the program never exited (#11309).
    host: Arc<dyn Host>,
    peer_address: String,
    peer_port: u16,
    decoder: http1::Decoder,
    input: Vec<u8>,
    building: Option<Building>,
    active: Option<Active>,
    seq: u64,
    /// Requests decoded on this connection, for `maxRequestsPerSocket`.
    requests: u64,
    idle_close_ms: u64,
    /// Bytes still to decode are held while a response is in flight, so a
    /// pipelined request is not dispatched before the current one finishes.
    paused: bool,
    read_eof: bool,
    closing: bool,
    destroyed: bool,
    /// A `drain` is on the stack for this connection.
    in_decode: bool,
    /// A nested `decode` arrived while one was draining; drain again when the
    /// outer one finishes rather than recursing.
    decode_again: bool,
    /// The host took this connection over through [`Host::on_upgrade`]. HTTP
    /// decoding has stopped for good: every later byte and every terminal
    /// completion goes to [`Host::on_upgraded`] instead.
    upgraded: bool,
}

fn conns() -> &'static Mutex<HashMap<i64, Conn>> {
    static CONNS: OnceLock<Mutex<HashMap<i64, Conn>>> = OnceLock::new();
    CONNS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_conn<R>(id: i64, f: impl FnOnce(&mut Conn) -> R) -> Option<R> {
    let mut map = conns().lock().unwrap_or_else(|e| e.into_inner());
    map.get_mut(&id).map(f)
}

fn host_of(conn_id: i64) -> Option<Arc<dyn Host>> {
    with_conn(conn_id, |c| c.host.clone())
}

/// Every live connection of one listener.
pub fn connections_of(listener_id: i64) -> Vec<i64> {
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, c)| c.listener_id == listener_id)
        .map(|(id, _)| *id)
        .collect()
}

/// Whether this connection is mid-exchange — decoding or answering.
pub fn is_busy(id: i64) -> bool {
    with_conn(id, |c| c.active.is_some() || c.building.is_some()).unwrap_or(false)
}

/// Note that this connection's in-flight request will never be answered,
/// exactly once per request.
fn note_aborted(id: i64) {
    let seq = with_conn(id, |c| {
        c.active.as_mut().filter(|a| a.abortable).map(|a| {
            a.abortable = false;
            a.seq
        })
    })
    .flatten();
    if let (Some(seq), Some(host)) = (seq, host_of(id)) {
        host.on_aborted(id, seq);
    }
}

// ── Completion sink ─────────────────────────────────────────────────────────

pub(crate) extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime passes a live completion for the duration of the
    // call, which is this function's body.
    let c = unsafe { &*completion };
    match c.kind {
        tl::NET_ACCEPT => on_accept(c.id, c.conn),
        // SAFETY: same call; the pooled lease outlives it.
        tl::NET_DATA => on_data(c.id, unsafe { c.bytes() }),
        tl::NET_EOF => on_eof(c.id),
        tl::NET_WROTE => {}
        tl::NET_SHUTDOWN => on_shutdown(c.id),
        tl::NET_CLOSED => on_closed(c.id),
        tl::NET_TIMER => on_timer(c.id),
        tl::NET_ERROR => on_error(c.id, c.terminal != 0),
        _ => {}
    }
}

fn on_accept(listener_id: i64, conn_id: i64) {
    if conn_id == 0 {
        return;
    }
    let Some((host, idle_close_ms)) =
        crate::with_listener(listener_id, |l| (l.host.clone(), l.idle_close_ms))
    else {
        let _ = tl::close(conn_id);
        return;
    };
    let peer = tl::peer_address(conn_id);
    conns().lock().unwrap_or_else(|e| e.into_inner()).insert(
        conn_id,
        Conn {
            id: conn_id,
            listener_id,
            host: host.clone(),
            peer_address: peer.as_ref().map(|e| e.address.clone()).unwrap_or_default(),
            peer_port: peer.as_ref().map(|e| e.port).unwrap_or(0),
            decoder: http1::Decoder::new(http1::Mode::Request, Default::default()),
            input: Vec::with_capacity(8 * 1024),
            building: None,
            active: None,
            seq: 0,
            requests: 0,
            idle_close_ms,
            paused: false,
            read_eof: false,
            closing: false,
            destroyed: false,
            in_decode: false,
            decode_again: false,
            upgraded: false,
        },
    );
    host.on_connection(conn_id);
    arm_idle(conn_id);
    if tl::read_start(conn_id).is_err() {
        destroy(conn_id);
    }
}

fn on_data(id: i64, bytes: &[u8]) {
    // An upgraded connection is no longer HTTP: no decoder, and no idle
    // deadline to refresh (the takeover cancelled it and nothing re-arms it —
    // a WebSocket that says nothing for an hour is not an idle keep-alive).
    if upgraded(id) {
        if let Some(host) = host_of(id) {
            host.on_upgraded(id, Upgraded::Data(bytes));
        }
        return;
    }
    // Every read refreshes the idle deadline; the connection is only "idle"
    // between a completed response and the next request byte.
    cancel_idle(id);
    feed(id, bytes);
}

/// Whether the host has taken this connection over.
fn upgraded(id: i64) -> bool {
    with_conn(id, |c| c.upgraded).unwrap_or(false)
}

fn feed(id: i64, bytes: &[u8]) {
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
/// per-connection serialization, and it is also what makes the response
/// unambiguous.
///
/// **The loop's progress rule is "an event, or bytes consumed" — not "bytes
/// consumed".** `http1::Decoder` raises `Event::End` from a step that consumes
/// **zero** bytes (PerryTS/turnloop#50): the terminating CRLF of a chunked body
/// or the last byte of a Content-Length body is consumed by the step that
/// produced the final `Event::Body`, and `End` arrives on the next call having
/// consumed nothing. A host that continues only while `consumed > 0` therefore
/// never sees `End`, never dispatches the request, and stalls with the client
/// waiting — the failure this comment exists to prevent, which has cost two
/// lanes a debugging cycle each. Note the inverse too: `None` with `consumed
/// == 0` is the real "needs more input", and is the only thing that ends the
/// loop.
/// Decode as much as the connection allows, without recursing.
///
/// Two paths re-enter: `complete_response` decodes again because a pipelined
/// request may already be buffered, and a host that answers *inside*
/// [`Host::on_request`] — a route table's 404, say — reaches
/// `complete_response` from within this very call. Recursing once per
/// pipelined request would make the stack depth the client's to choose, so a
/// nested entry sets a flag and the outer drain loops instead.
fn decode(id: i64) {
    if with_conn(id, |c| {
        if c.in_decode {
            c.decode_again = true;
            return false;
        }
        c.in_decode = true;
        true
    }) != Some(true)
    {
        return;
    }
    loop {
        drain(id);
        // The connection may be gone (destroyed mid-drain), in which case
        // there is no flag left to read and nothing left to drain.
        match with_conn(id, |c| std::mem::replace(&mut c.decode_again, false)) {
            Some(true) => continue,
            _ => break,
        }
    }
    with_conn(id, |c| c.in_decode = false);
}

fn drain(id: i64) {
    loop {
        enum Step {
            Idle,
            Again,
            /// A decoded request, and whether the client is waiting for a
            /// `100 Continue` before it sends the body.
            Dispatch(Request, bool),
            /// A `Connection: upgrade` request on a host that takes them, plus
            /// whatever the peer pipelined behind the handshake.
            Upgrade(Request, Vec<u8>),
            Failed,
        }
        let step = with_conn(id, |c| {
            // `upgraded` is the third and permanent one: the host owns the
            // connection, `input` was handed over whole, and the decoder must
            // never see another byte of it. `on_data` already routes past
            // `feed`, so this only closes the re-entrant path — `decode`'s
            // outer loop runs once more when a nested `decode` arrived while
            // the upgrade was being handed over.
            if c.destroyed || c.paused || c.upgraded {
                return Step::Idle;
            }
            let step = match c.decoder.receive(&c.input) {
                Ok(step) => step,
                Err(_) => return Step::Failed,
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
                // `Event::End` and `Event::Upgrade` are the same thing here:
                // a complete request, dispatched. Node serves an upgrade
                // request as an ordinary request when no `'upgrade'` listener
                // exists (#4973), and neither consumer of this crate has such
                // a listener on the turnloop path — fastify's declines to the
                // hyper loop at listen time. `Request::upgrade` says which it
                // was.
                //
                // `Event::Upgrade` is in fact unreachable on the request side:
                // `turnloop_http::http1`'s `State::Upgrade` is only reachable
                // in `Mode::Response` (a client reading a 101). The arm is kept
                // so a later decoder that does raise it cannot fall through to
                // "needs more input" and stall.
                Some(http1::Event::End) | Some(http1::Event::Upgrade) => {
                    outcome = match c.building.take() {
                        Some(building) => {
                            c.requests += 1;
                            c.seq += 1;
                            if building.upgrade && takes_upgrades(c) {
                                // No `active`, no `paused`, no idle deadline:
                                // this connection stops being an HTTP exchange
                                // here. `leftover` is filled below, once the
                                // head's own bytes have been drained off
                                // `input`.
                                c.upgraded = true;
                                Step::Upgrade(upgrade_request(c, building), Vec::new())
                            } else {
                                c.paused = true;
                                let (request, send_continue) = finish_request(c, building);
                                Step::Dispatch(request, send_continue)
                            }
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
            if let Step::Upgrade(_, leftover) = &mut outcome {
                // Everything still buffered arrived in the same read as the
                // handshake and belongs to the upgraded protocol. Handing it
                // over here is what stops it being parsed as a second HTTP
                // request — and dropping it would lose a message a client
                // pipelined behind its `Sec-WebSocket-Key`.
                *leftover = std::mem::take(&mut c.input);
            }
            outcome
        });
        match step {
            None | Some(Step::Idle) => return,
            Some(Step::Again) => continue,
            Some(Step::Upgrade(request, leftover)) => {
                // Outside the borrow: the host writes its `101` with
                // `write_raw`, which takes the same lock.
                cancel_idle(id);
                if let Some(host) = host_of(id) {
                    host.on_upgrade(request, leftover);
                } else {
                    destroy(id);
                }
                return;
            }
            Some(Step::Dispatch(request, send_continue)) => {
                // Outside the connection borrow: `write_raw` takes the same
                // lock, and `std::sync::Mutex` is not reentrant.
                if send_continue {
                    write_raw(id, b"HTTP/1.1 100 Continue\r\n\r\n");
                }
                if let Some(host) = host_of(id) {
                    host.on_request(request);
                }
                return;
            }
            Some(Step::Failed) => {
                bad_request(id);
                return;
            }
        }
    }
}

fn building_from(head: &http1::Head) -> Building {
    let mut headers = Vec::with_capacity(head.headers.len());
    for header in &head.headers {
        let Ok(value) = std::str::from_utf8(&header.value) else {
            continue;
        };
        // `http1::Header::name` is already lowercase: `Decoder` lowercases as
        // it parses, which matches Node's `req.headers`.
        headers.push((header.name.clone(), value.to_string()));
    }
    let connection = headers
        .iter()
        .find(|(k, _)| k == "connection")
        .map(|(_, v)| v.clone());
    let upgrade = headers.iter().any(|(k, _)| k == "upgrade")
        && connection.as_deref().is_some_and(|v| {
            v.to_ascii_lowercase()
                .split(',')
                .any(|t| t.trim() == "upgrade")
        });
    let expects_continue = headers
        .iter()
        .find(|(k, _)| k == "expect")
        .is_some_and(|(_, v)| v.to_ascii_lowercase().contains("100-continue"));
    Building {
        method: head.method.clone(),
        target: head.target.clone(),
        headers,
        body: Vec::new(),
        version: head.version,
        expects_continue,
        connection,
        upgrade,
    }
}

/// Whether this connection's listener diverts upgrades to its host.
fn takes_upgrades(c: &Conn) -> bool {
    c.host.takes_upgrades()
}

/// The [`Request`] handed to [`Host::on_upgrade`].
///
/// Deliberately not [`finish_request`]: that installs an `Active` so the
/// connection can be answered, and an upgraded connection is never answered
/// again. `seq` is carried anyway so a host can key its own state by the same
/// `(conn_id, seq)` pair every other request uses.
fn upgrade_request(c: &Conn, building: Building) -> Request {
    Request {
        conn_id: c.id,
        seq: c.seq,
        method: building.method,
        target: building.target,
        version: building.version,
        headers: building.headers,
        body: building.body,
        peer_address: c.peer_address.clone(),
        peer_port: c.peer_port,
        expects_continue: building.expects_continue,
        upgrade: true,
        request_number: c.requests,
    }
}

/// Turn a fully decoded request into the [`Request`] the host receives.
fn finish_request(c: &mut Conn, building: Building) -> (Request, bool) {
    let host_intercepts = c.host.intercepts_continue();
    // Node's `100 Continue` is automatic unless a `'checkContinue'` listener
    // takes over. hyper sent it when the body was polled; here it goes out as
    // soon as the head says the client is waiting, once the caller has
    // released the connection borrow.
    let send_continue = building.expects_continue && !host_intercepts;

    c.active = Some(Active {
        seq: c.seq,
        method: building.method.clone(),
        version: building.version,
        connection: building.connection.clone(),
        encoder: None,
        framing: Framing::Sized(0),
        head_sent: false,
        keep_alive: true,
        abortable: true,
    });

    (
        Request {
            conn_id: c.id,
            seq: c.seq,
            method: building.method,
            target: building.target,
            version: building.version,
            headers: building.headers,
            body: building.body,
            peer_address: c.peer_address.clone(),
            peer_port: c.peer_port,
            expects_continue: building.expects_continue,
            upgrade: building.upgrade,
            request_number: c.requests,
        },
        send_continue,
    )
}

// ── Response side ───────────────────────────────────────────────────────────

/// Whether `seq` still names the request this connection is answering.
fn owns(c: &Conn, seq: u64) -> bool {
    c.active.as_ref().is_some_and(|a| a.seq == seq) && !c.destroyed
}

/// Decide the response's `Connection` / `Keep-Alive` headers and whether the
/// connection survives it.
fn prepare_headers(c: &mut Conn, response: &mut Response) -> bool {
    let (version, connection) = {
        let active = c.active.as_ref().expect("an active request");
        (active.version, active.connection.clone())
    };
    // A closed listener still answers its in-flight requests, but never keeps
    // the connection for another one.
    let listener_open = crate::with_listener(c.listener_id, |_| ()).is_some();
    let (closing, max_requests, keep_alive_timeout_ms) = if listener_open {
        (
            c.host.is_closing(),
            c.host.max_requests_per_socket(),
            c.host.keep_alive_timeout_ms(),
        )
    } else {
        (true, 0, 0.0)
    };
    let over_quota = max_requests > 0 && c.requests >= max_requests;
    let keep_alive = crate::connection_headers(
        &mut response.headers,
        version,
        connection.as_deref(),
        keep_alive_timeout_ms,
        closing || over_quota,
    );
    if let Some(active) = c.active.as_mut() {
        active.keep_alive = keep_alive;
    }
    keep_alive
}

/// The `500` a response the encoder refuses is answered with, rather than
/// letting it silently vanish — which is what Node does for an invalid
/// outgoing header it catches late.
const ENCODER_REFUSED: &[u8] =
    b"HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

/// Write a fully buffered response and retire the exchange.
pub fn respond(conn_id: i64, seq: u64, mut response: Response) {
    let encoded = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let keep_alive = prepare_headers(c, &mut response);
        let (method, version) = {
            let a = c.active.as_ref().expect("an active request");
            (a.method.clone(), a.version)
        };
        // An HTTP/1.0 response that will close the connection is
        // close-delimited in Node, with no length header — but only when the
        // length was synthesized; a handler that set `Content-Length` keeps it.
        let eof_framed = version == 0 && !keep_alive && response.auto_content_length;
        let framing = wire::framing_for(
            &response.headers,
            response.status,
            &method,
            version,
            Some(response.body.len() as u64),
            eof_framed,
        );
        wire::align_headers(&mut response.headers, framing, response.auto_content_length);
        let head = match wire::encode_head(
            response.status,
            response.status_message.as_deref(),
            &response.headers,
            framing,
        ) {
            Ok(head) => head,
            Err(_) => return Some((ENCODER_REFUSED.to_vec(), Framing::UntilClose)),
        };
        let mut out = head.bytes;
        if !matches!(framing, Framing::NoBody) && !response.body.is_empty() {
            match head.encoder {
                Some(mut encoder) => {
                    let _ = encoder.body(&response.body, &mut out);
                    let _ = encoder.finish(&trailers_of(&response), &mut out);
                }
                None => out.extend_from_slice(&response.body),
            }
        } else if let Some(mut encoder) = head.encoder {
            let _ = encoder.finish(&trailers_of(&response), &mut out);
        }
        if let Some(active) = c.active.as_mut() {
            active.head_sent = true;
            active.framing = framing;
            // The response reached the wire: there is nothing left to abort.
            active.abortable = false;
        }
        Some((out, framing))
    });
    let Some(Some((out, framing))) = encoded else {
        return;
    };
    write_raw(conn_id, &out);
    complete_response(conn_id, seq, framing);
}

fn trailers_of(response: &Response) -> Vec<http1::Header> {
    response
        .trailers
        .iter()
        .map(|(name, value)| http1::Header {
            name: name.clone(),
            value: value.as_bytes().to_vec(),
        })
        .collect()
}

/// Write an interim (1xx) response on the connection answering `seq`.
pub fn send_interim(conn_id: i64, seq: u64, bytes: &[u8]) {
    if with_conn(conn_id, |c| owns(c, seq)).unwrap_or(false) {
        write_raw(conn_id, bytes);
    }
}

/// Open a streaming response: write the head, keep the encoder for the chunks
/// that follow. `response.body` is ignored; pass the first chunk to
/// [`stream_body`].
///
/// Returns false when `seq` no longer names the connection's active request.
pub fn stream_begin(conn_id: i64, seq: u64, mut response: Response) -> bool {
    let encoded = with_conn(conn_id, |c| {
        if !owns(c, seq) || c.active.as_ref().is_some_and(|a| a.head_sent) {
            return None;
        }
        let keep_alive = prepare_headers(c, &mut response);
        let (method, version) = {
            let a = c.active.as_ref().expect("an active request");
            (a.method.clone(), a.version)
        };
        let eof_framed = version == 0 && !keep_alive;
        let framing = wire::framing_for(
            &response.headers,
            response.status,
            &method,
            version,
            None,
            eof_framed,
        );
        wire::align_headers(&mut response.headers, framing, response.auto_content_length);
        let head = match wire::encode_head(
            response.status,
            response.status_message.as_deref(),
            &response.headers,
            framing,
        ) {
            Ok(head) => head,
            // Refused: answer 500 and close. The flag is carried explicitly
            // rather than inferred from `encoder.is_none()`, which is ALSO true
            // for a legitimate `UntilClose` or `NoBody` framing — inferring it
            // would have completed a real HTTP/1.0 streaming response the
            // instant its head was written.
            Err(_) => return Some((ENCODER_REFUSED.to_vec(), None, Framing::UntilClose, true)),
        };
        if let Some(active) = c.active.as_mut() {
            active.head_sent = true;
            active.framing = framing;
            active.abortable = false;
        }
        Some((head.bytes, head.encoder, framing, false))
    });
    let Some(Some((bytes, encoder, _framing, refused))) = encoded else {
        return false;
    };
    with_conn(conn_id, |c| {
        if let Some(active) = c.active.as_mut() {
            active.encoder = encoder;
        }
    });
    write_raw(conn_id, &bytes);
    if refused {
        complete_response(conn_id, seq, Framing::UntilClose);
    }
    true
}

/// Write one chunk of a streaming response.
pub fn stream_body(conn_id: i64, seq: u64, bytes: &[u8]) -> bool {
    let out = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let active = c.active.as_mut()?;
        if matches!(active.framing, Framing::NoBody) {
            // A HEAD response advertises a length and sends no body.
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
    let Some(Some(out)) = out else {
        return false;
    };
    write_raw(conn_id, &out);
    true
}

/// Finish a streaming response and retire the exchange.
pub fn stream_end(conn_id: i64, seq: u64, trailers: &[(String, String)]) {
    let finished = with_conn(conn_id, |c| {
        if !owns(c, seq) {
            return None;
        }
        let active = c.active.as_mut()?;
        let framing = active.framing;
        let mut out = Vec::new();
        if let Some(mut encoder) = active.encoder.take() {
            let encoded: Vec<http1::Header> = trailers
                .iter()
                .map(|(name, value)| http1::Header {
                    name: name.clone(),
                    value: value.as_bytes().to_vec(),
                })
                .collect();
            let _ = encoder.finish(&encoded, &mut out);
        }
        Some((out, framing))
    });
    let Some(Some((out, framing))) = finished else {
        return;
    };
    write_raw(conn_id, &out);
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

/// Tear the connection down now, raising `'aborted'` on an unanswered request.
pub fn destroy(conn_id: i64) {
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

/// End the connection gracefully: everything already queued goes out, then FIN.
///
/// Public because a host that took a connection over through
/// [`Host::on_upgrade`](crate::Host::on_upgrade) needs the distinction
/// [`destroy`] does not make. A WebSocket closing handshake ends with a close
/// frame written and *then* a shutdown, and turnloop's `close` cancels the
/// connection's outstanding operations — including the write that was just
/// queued — so ending with `destroy` makes the peer see a reset and report
/// 1006 instead of the code it was just sent.
pub fn finish(conn_id: i64) {
    finish_and_close(conn_id);
}

/// End the write side and close once it has drained. turnloop orders a
/// handle's writes ahead of its shutdown, so a completed shutdown means every
/// queued byte left — closing outright would cancel them.
fn finish_and_close(conn_id: i64) {
    cancel_idle(conn_id);
    with_conn(conn_id, |c| c.closing = true);
    if tl::shutdown(conn_id, 0).is_err() {
        let _ = tl::close(conn_id);
    }
}

/// Submit bytes on the connection.
///
/// Public because an upgrade the host took over writes its own `101` here.
pub fn write_raw(conn_id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if tl::write(conn_id, bytes, 0).is_err() {
        destroy(conn_id);
    }
}

/// Answer a malformed request the way Node does: one 400, then close.
fn bad_request(conn_id: i64) {
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
    if upgraded(id) {
        // The upgraded protocol decides what a half-close means — a WebSocket
        // close handshake ends in exactly one — so the core neither aborts a
        // request (there is none) nor closes the handle here.
        if let Some(host) = host_of(id) {
            host.on_upgraded(id, Upgraded::Eof);
        }
        return;
    }
    let state = with_conn(id, |c| {
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
        // response never completed — which is this one.
        note_aborted(id);
    }
    if partial {
        // A half-sent request: Node destroys the socket without answering.
        destroy(id);
        return;
    }
    if !answering {
        finish_and_close(id);
    }
    // A request still being answered keeps the connection until its response
    // has been written; `complete_response` sees `read_eof` and closes.
}

/// The write-side shutdown submitted by [`finish_and_close`] completed: every
/// byte queued ahead of it has left, because turnloop orders a handle's writes
/// before its shutdown. Close now, which is what reclaims the runtime entry and
/// hands the id back.
///
/// Without this arm a connection the SERVER ends — `Connection: close`, an
/// HTTP/1.0 response, a 400, the idle keep-alive deadline, `server.close()` —
/// is shut down and then never closed: `on_eof` returns early once `closing` is
/// set, so no terminal `Closed` ever arrives and `free_handle_id` never runs.
/// That is the #6441 id-exhaustion shape, one id per server-closed connection.
/// `perry-ext-net`'s `on_shutdown` does the same thing for the same reason
/// (`close_after_shutdown` -> `destroy`).
fn on_shutdown(id: i64) {
    let closing = with_conn(id, |c| c.closing).unwrap_or(false);
    if closing {
        let _ = tl::close(id);
    }
}

fn on_closed(id: i64) {
    if upgraded(id) {
        if let Some(host) = host_of(id) {
            host.on_upgraded(id, Upgraded::Closed);
        }
    }
    // A peer that vanished mid-request reaches the terminal `Closed` without
    // ever passing through `destroy`.
    note_aborted(id);
    cancel_idle(id);
    let owned = conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id)
        .is_some();
    if owned {
        // The terminal completion: no completion can name this id again, and
        // unlike a `net.Socket` id no JS object still holds it, so it goes
        // back to the shared band instead of leaking one id per connection for
        // the life of a server (the #6441 exhaustion class).
        perry_ffi::free_handle_id(id);
    }
}

fn on_timer(id: i64) {
    // The idle keep-alive deadline. Node closes the connection; an exchange
    // that started in the meantime cancelled the deadline already, and an
    // upgraded connection has no keep-alive deadline at all.
    let idle = with_conn(id, |c| {
        !c.upgraded && c.active.is_none() && c.building.is_none()
    })
    .unwrap_or(false);
    if idle {
        finish_and_close(id);
    }
}

fn on_error(id: i64, terminal: bool) {
    if crate::with_listener(id, |_| ()).is_some() {
        // A transient accept failure does not end the listener, exactly as a
        // hyper accept loop kept going on one.
        if terminal {
            crate::close_listener(id);
        }
        return;
    }
    if upgraded(id) {
        if let Some(host) = host_of(id) {
            host.on_upgraded(id, Upgraded::Error("socket error"));
        }
    }
    destroy(id);
}

// ── Idle deadline ───────────────────────────────────────────────────────────

/// Arm the connection's idle close.
///
/// Node 26.5.1, measured: the server FINs an idle keep-alive connection at
/// `keepAliveTimeout + keepAliveTimeoutBuffer` (defaults 5000 + 1000 ms), and
/// `keepAliveTimeout = 0` disables the close entirely while *keeping*
/// keep-alive on. Zero here therefore arms nothing.
fn arm_idle(id: i64) {
    let armed = with_conn(id, |c| (c.idle_close_ms, c.listener_id));
    let Some((ms, listener_id)) = armed else {
        return;
    };
    if ms == 0 {
        return;
    }
    let subsystem = crate::subsystem_of_listener(listener_id);
    let _ = tl::timer_arm(id, subsystem, ms);
}

fn cancel_idle(id: i64) {
    let _ = tl::timer_cancel(id);
}
