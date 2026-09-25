//! One client connection's state machine: connect, optional `CONNECT` tunnel,
//! optional TLS, then one HTTP/1.1 exchange at a time.
//!
//! Every function here runs with [`State`] locked and returns what it wants
//! done as [`Effect`]s; see the module header in `mod.rs` for why no `tl::`
//! call may be made from inside.
//!
//! # Settling
//!
//! An exchange lives in `Conn::exchange` from `start` until exactly one of:
//! `finish` (the response ended), `fail`/`premature` (an error), a deadline, or
//! `cancel`. Each of those *takes* it, so no second terminal event can be
//! produced for the same request, whichever completions arrive afterwards.
//!
//! # Input
//!
//! Bytes that the decoder has not consumed stay in `Conn::inbuf` across reads:
//! a response head or a chunk-size line split over two TCP reads is completed
//! by the next one. The loop runs while a step makes progress — consumed
//! bytes *or* produced an event — because `Event::End` and `Event::Upgrade`
//! arrive from zero-byte steps (turnloop#50's contract).

use bytes::Bytes;
use perry_ffi::Handle;
use perry_tls_session::TlsSession;
use turnloop_http::http1;

use super::{
    next_id, tls, wire, Effect, Mode, Outbound, PoolKey, State, Timer, COMPLETED, HANDSHAKES,
    REUSED, TIMED_OUT,
};
use crate::{ClientInflightGuard, PendingHttpEvent};

/// Bound on decode steps per delivery. Every step consumes input or produces
/// an event; this only stops a decoder that did neither from spinning the
/// agent's loop, which would be a worse failure than a dropped response.
const MAX_STEPS: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// `tcp_connect` submitted.
    Connecting,
    /// Connected to a proxy; waiting for the `CONNECT` response.
    Tunnel,
    /// Carrying (or about to carry) an exchange.
    Open,
    /// Parked in the pool between exchanges.
    Idle,
    /// `tl::close` submitted; waiting for `NET_CLOSED`.
    Closing,
}

/// The `Expect: 100-continue` body hand-off.
enum Continue {
    /// Not a continue exchange, or the body has been dealt with.
    Done,
    /// Waiting for the interim `100`; `end()` may already have supplied the
    /// body.
    AwaitingInterim(Option<Vec<u8>>),
    /// The `100` arrived before `end()`: the body goes out when it comes.
    Released,
}

struct ResponseHead {
    status: u16,
    reason: String,
    version: u8,
    headers: Vec<(String, String)>,
}

pub(super) struct Exchange {
    out: Box<Outbound>,
    framing: wire::Framing,
    /// The request asked the server to close.
    closes: bool,
    /// The request head has been handed to the socket (or TLS session).
    sent: bool,
    /// This exchange runs on a connection taken from the pool.
    reused: bool,
    /// This exchange is itself a retry; it is never retried again.
    retried: bool,
    /// Any response byte arrived. A reused connection that dies before one
    /// does is retried once on a fresh connection, as hyper's pool did: the
    /// peer closing an idle keep-alive socket just as it is reused is a race,
    /// not a failure of the request.
    got_bytes: bool,
    head: Option<ResponseHead>,
    /// A `ResponseHead` event has been queued (the drain has an
    /// `IncomingMessage` to fail with `'aborted'`).
    head_delivered: bool,
    buffered: Vec<u8>,
    trailers: Vec<(String, String)>,
    cont: Continue,
    deadline: i64,
    _inflight: ClientInflightGuard,
}

pub(super) struct Conn {
    key: PoolKey,
    /// What was dialed — the proxy when proxied — for Node's error messages.
    peer_host: String,
    peer_port: u16,
    phase: Phase,
    tls: Option<TlsSession>,
    handshake_done: bool,
    decoder: http1::Decoder,
    inbuf: Vec<u8>,
    exchange: Option<Exchange>,
    /// Exchanges this connection has completed.
    served: u32,
    idle_timer: i64,
}

fn new_decoder() -> http1::Decoder {
    http1::Decoder::new(http1::Mode::Response, http1::Limits::default())
}

/// The host to dial for a URL: brackets stripped from an IPv6 literal, which
/// `Url::host_str` keeps.
pub(super) fn dial_host(url: &url::Url) -> Option<String> {
    Some(match url.host()? {
        url::Host::Domain(domain) => domain.to_string(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => address.to_string(),
    })
}

/// Streamed (`ResponseHead` → chunks → `ResponseEnd`) rather than buffered
/// into one `Response` event.
fn streams(out: &Outbound) -> bool {
    match out.mode {
        Mode::Normal | Mode::Continue => true,
        Mode::Trailers => false,
        // A cleartext upgrade that the server declined was delivered buffered
        // by its bypass; over TLS, reqwest streamed whatever came back.
        Mode::Upgrade => out.key.https,
    }
}

// ── Starting ────────────────────────────────────────────────────────────────

/// Start an exchange. `inflight` is the guard of the exchange this one
/// retries, carried over so the exit gate never sees the request drop out of
/// the in-flight set between the two.
pub(super) fn start(
    st: &mut State,
    out: Outbound,
    inflight: Option<ClientInflightGuard>,
    fx: &mut Vec<Effect>,
) {
    let request_handle = out.request_handle;
    let retried = inflight.is_some();
    let mut exchange = Exchange {
        framing: wire::Framing::Raw,
        closes: false,
        sent: false,
        reused: false,
        retried,
        got_bytes: false,
        head: None,
        head_delivered: false,
        buffered: Vec::new(),
        trailers: Vec::new(),
        cont: if out.mode == Mode::Continue {
            Continue::AwaitingInterim(None)
        } else {
            Continue::Done
        },
        deadline: 0,
        _inflight: inflight.unwrap_or_else(|| ClientInflightGuard::new(request_handle)),
        out: Box::new(out),
    };

    // A kept-alive connection first, when the agent allows one. A retry never
    // takes one: it exists because a reused connection just failed.
    let pooled = if !retried && exchange.out.reuse.is_some() {
        take_idle(st, &exchange.out.key, fx)
    } else {
        None
    };
    let id = match pooled {
        Some(id) => id,
        None => {
            let id = next_id();
            if id == perry_ffi::INVALID_HANDLE {
                fx.push(Effect::Push(PendingHttpEvent::Error {
                    request_handle,
                    error_message: "http client: connection ids exhausted".to_string(),
                }));
                return;
            }
            id
        }
    };

    if let Some(ms) = exchange.out.timeout_ms {
        let timer = next_id();
        if timer != perry_ffi::INVALID_HANDLE {
            st.timers.insert(
                timer,
                Timer::Deadline {
                    conn: id,
                    request: request_handle,
                },
            );
            exchange.deadline = timer;
            fx.push(Effect::ArmTimer(timer, ms));
        }
    }
    st.by_request.insert(request_handle, id);

    if pooled.is_some() {
        REUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        exchange.reused = true;
        let Some(conn) = st.conns.get_mut(&id) else {
            return;
        };
        conn.exchange = Some(exchange);
        send_request(st, id, fx);
        return;
    }

    let (peer_host, peer_port) = match &exchange.out.proxy {
        Some(proxy) => (
            dial_host(proxy).unwrap_or_default(),
            proxy.port_or_known_default().unwrap_or(80),
        ),
        None => (exchange.out.key.host.clone(), exchange.out.key.port),
    };
    st.conns.insert(
        id,
        Conn {
            key: exchange.out.key.clone(),
            peer_host: peer_host.clone(),
            peer_port,
            phase: Phase::Connecting,
            tls: None,
            handshake_done: false,
            decoder: new_decoder(),
            inbuf: Vec::new(),
            exchange: Some(exchange),
            served: 0,
            idle_timer: 0,
        },
    );
    fx.push(Effect::Connect {
        id,
        host: peer_host,
        port: peer_port,
    });
}

pub(super) fn on_connect(st: &mut State, id: i64) -> Vec<Effect> {
    let mut fx = Vec::new();
    let Some(conn) = st.conns.get_mut(&id) else {
        return fx;
    };
    if conn.phase != Phase::Connecting {
        return fx;
    }
    // Reading starts before anything is written: a loopback peer's reply can
    // be in flight before the write submission returns.
    fx.push(Effect::ReadStart(id));
    let tunnel = conn.exchange.as_ref().and_then(|ex| {
        let out = &ex.out;
        (out.key.https)
            .then(|| out.proxy.as_ref())
            .flatten()
            .map(|proxy| wire::connect_head(&out.key.host, out.key.port, proxy))
    });
    if let Some(head) = tunnel {
        conn.phase = Phase::Tunnel;
        conn.decoder.response_to("CONNECT");
        fx.push(Effect::Write(id, head));
        return fx;
    }
    conn.phase = Phase::Open;
    if conn.key.https && !open_tls(st, id, &mut fx) {
        return fx;
    }
    send_request(st, id, &mut fx);
    fx
}

/// Install the TLS session and send the ClientHello. `false` means it failed
/// and the exchange has been settled.
fn open_tls(st: &mut State, id: i64, fx: &mut Vec<Effect>) -> bool {
    let Some(conn) = st.conns.get_mut(&id) else {
        return false;
    };
    let plan = conn.exchange.as_ref().and_then(|ex| ex.out.tls.clone());
    let session = match plan.as_ref().map(tls::open) {
        Some(Ok(session)) => session,
        Some(Err(message)) => {
            fail_coded(st, id, message, "ERR_SSL_PROTOCOL_ERROR", fx);
            return false;
        }
        None => {
            fail_coded(
                st,
                id,
                "https request without a TLS configuration".to_string(),
                "ERR_SSL_PROTOCOL_ERROR",
                fx,
            );
            return false;
        }
    };
    conn.tls = Some(session);
    pump_tls(st, id, fx);
    st.conns.contains_key(&id)
}

/// Put the current exchange's request on the wire (or into the TLS session,
/// which holds it until the handshake allows).
fn send_request(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    let Some(ex) = conn.exchange.as_mut() else {
        return;
    };
    if ex.sent {
        return;
    }
    if conn.served > 0 && conn.decoder.reset().is_err() {
        // Only a connection the decoder called reusable is ever parked, so
        // this cannot happen; if it does, a fresh decoder is the safe state.
        conn.decoder = new_decoder();
    }
    let method = ex.out.method.to_ascii_uppercase();
    conn.decoder.response_to(&method);

    let out = &ex.out;
    let absolute = out.proxy.is_some() && !out.key.https;
    let target = wire::request_target(&out.url, absolute);
    let mut extra = out.extra.clone();
    if absolute {
        if let Some(credentials) = out.proxy.as_ref().and_then(wire::basic_credentials) {
            extra.push(("Proxy-Authorization".to_string(), credentials));
        }
    }
    let serialized = wire::serialize_head(
        &out.method,
        &target,
        &out.url,
        &out.headers,
        &extra,
        out.body.len(),
        out.mode,
    );
    let mut bytes = serialized.head;
    if out.mode != Mode::Continue {
        bytes.extend_from_slice(&wire::frame_body(&out.body, serialized.framing));
    }
    ex.framing = serialized.framing;
    ex.closes = serialized.closes;
    ex.sent = true;
    send(st, id, bytes, fx);
}

/// Hand plaintext to the connection: through the TLS session when there is
/// one, straight to the socket otherwise.
fn send(st: &mut State, id: i64, bytes: Vec<u8>, fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    match conn.tls.as_mut() {
        Some(session) => {
            session.write(&bytes);
            pump_tls(st, id, fx);
        }
        None => fx.push(Effect::Write(id, bytes)),
    }
}

/// Run the TLS state machine and flush its ciphertext. Returns decrypted
/// plaintext and whether the peer sent `close_notify`; `None` if the session
/// failed (the exchange has then been settled).
fn pump_tls(st: &mut State, id: i64, fx: &mut Vec<Effect>) -> Option<(Vec<u8>, bool)> {
    let conn = st.conns.get_mut(&id)?;
    let session = conn.tls.as_mut()?;
    let progress = session.pump();
    let ciphertext = session.take_output();
    if !ciphertext.is_empty() {
        fx.push(Effect::Write(id, ciphertext));
    }
    if let Some(failure) = session.failure() {
        let code = failure.code;
        let message = tls::node_failure_message(code, &failure.message);
        let handshaking = !conn.handshake_done;
        if handshaking {
            fail_coded(st, id, message, code, fx);
        } else {
            premature(st, id, fx);
        }
        return None;
    }
    if progress.handshake_done {
        conn.handshake_done = true;
        HANDSHAKES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    Some((session.take_plaintext(), progress.peer_closed))
}

// ── Input ───────────────────────────────────────────────────────────────────

pub(super) fn on_data(st: &mut State, id: i64, bytes: &[u8]) -> Vec<Effect> {
    let mut fx = Vec::new();
    let Some(conn) = st.conns.get_mut(&id) else {
        return fx;
    };
    match conn.phase {
        Phase::Connecting | Phase::Closing => {}
        Phase::Tunnel => tunnel_input(st, id, bytes, &mut fx),
        Phase::Open | Phase::Idle => {
            if conn.tls.is_some() {
                let Some(session) = conn.tls.as_mut() else {
                    return fx;
                };
                session.receive(bytes);
                if let Some((plaintext, peer_closed)) = pump_tls(st, id, &mut fx) {
                    if !plaintext.is_empty() {
                        http_input(st, id, &plaintext, &mut fx);
                    }
                    if peer_closed {
                        eof(st, id, &mut fx);
                    }
                }
            } else {
                http_input(st, id, bytes, &mut fx);
            }
        }
    }
    fx
}

/// The proxy's answer to `CONNECT`. A 2xx is an `Upgrade` from the decoder's
/// point of view; whatever follows it is the target's TLS.
fn tunnel_input(st: &mut State, id: i64, bytes: &[u8], fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    conn.inbuf.extend_from_slice(bytes);
    for _ in 0..MAX_STEPS {
        let Some(conn) = st.conns.get_mut(&id) else {
            return;
        };
        let step = match conn.decoder.receive(&conn.inbuf) {
            Ok(step) => step,
            Err(error) => {
                fail_coded(
                    st,
                    id,
                    format!("{} {}", error.code, error.message),
                    "ERR_PROXY_TUNNEL",
                    fx,
                );
                return;
            }
        };
        let consumed = step.consumed;
        let outcome = match step.event {
            Some(http1::Event::Head(head)) if (200..300).contains(&head.status) => None,
            Some(http1::Event::Head(head)) => Some(Err(format!(
                "Failed to establish tunnel to {}:{}: HTTP/1.{} {} {}",
                conn.key.host,
                conn.key.port,
                head.version,
                head.status,
                wire::reason_phrase(&conn.inbuf[..consumed]),
            ))),
            Some(http1::Event::Upgrade) => Some(Ok(())),
            Some(_) => None,
            None if consumed == 0 => return,
            None => None,
        };
        conn.inbuf.drain(..consumed);
        match outcome {
            None => {}
            Some(Err(message)) => {
                fail_coded(st, id, message, "ERR_PROXY_TUNNEL", fx);
                return;
            }
            Some(Ok(())) => {
                let leftover = std::mem::take(&mut conn.inbuf);
                conn.decoder = new_decoder();
                conn.phase = Phase::Open;
                if !open_tls(st, id, fx) {
                    return;
                }
                send_request(st, id, fx);
                if !leftover.is_empty() {
                    let mut more = on_data(st, id, &leftover);
                    fx.append(&mut more);
                }
                return;
            }
        }
    }
}

fn http_input(st: &mut State, id: i64, bytes: &[u8], fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    conn.inbuf.extend_from_slice(bytes);
    process(st, id, fx);
}

/// One decoded event, owned so the connection can be released before it is
/// acted on.
enum Decoded {
    Head(http1::Head, String),
    Informational(u16),
    Body(Vec<u8>),
    Trailers(Vec<http1::Header>),
    End,
    Upgrade,
    Nothing,
}

fn process(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    for _ in 0..MAX_STEPS {
        let Some(conn) = st.conns.get_mut(&id) else {
            return;
        };
        let Some(ex) = conn.exchange.as_mut() else {
            // Bytes on a connection with no request in flight: unsolicited
            // data on an idle keep-alive socket, or bytes after a response
            // ended. Neither can be framed; the connection is done.
            if !conn.inbuf.is_empty() {
                close(st, id, fx);
            }
            return;
        };
        if !conn.inbuf.is_empty() {
            ex.got_bytes = true;
        }
        let step = match conn.decoder.receive(&conn.inbuf) {
            Ok(step) => step,
            Err(error) => {
                fail_protocol(st, id, error, fx);
                return;
            }
        };
        let consumed = step.consumed;
        let decoded = match step.event {
            Some(http1::Event::Head(head)) => {
                let reason = wire::reason_phrase(&conn.inbuf[..consumed]);
                Decoded::Head(head, reason)
            }
            Some(http1::Event::Informational(head)) => Decoded::Informational(head.status),
            Some(http1::Event::Body(chunk)) => Decoded::Body(chunk.to_vec()),
            Some(http1::Event::Trailers(trailers)) => Decoded::Trailers(trailers),
            Some(http1::Event::End) => Decoded::End,
            Some(http1::Event::Upgrade) => Decoded::Upgrade,
            None => Decoded::Nothing,
        };
        conn.inbuf.drain(..consumed);
        match decoded {
            Decoded::Nothing => {
                if consumed == 0 {
                    return;
                }
            }
            Decoded::Informational(status) => on_informational(st, id, status, fx),
            Decoded::Head(head, reason) => on_head(st, id, head, reason, fx),
            Decoded::Body(chunk) => on_body(st, id, chunk, fx),
            Decoded::Trailers(trailers) => {
                if let Some(ex) = st.conns.get_mut(&id).and_then(|c| c.exchange.as_mut()) {
                    ex.trailers = trailers
                        .into_iter()
                        .map(|h| (h.name, String::from_utf8_lossy(&h.value).into_owned()))
                        .collect();
                }
            }
            Decoded::End => finish(st, id, false, fx),
            Decoded::Upgrade => on_upgrade(st, id, fx),
        }
    }
}

fn on_informational(st: &mut State, id: i64, status: u16, fx: &mut Vec<Effect>) {
    if status != 100 {
        // `102`/`103` — Node's `'information'`, which neither the reqwest
        // path nor the bypasses surfaced.
        return;
    }
    let Some(ex) = st.conns.get_mut(&id).and_then(|c| c.exchange.as_mut()) else {
        return;
    };
    if ex.out.mode != Mode::Continue {
        return;
    }
    let request_handle = ex.out.request_handle;
    let body = match std::mem::replace(&mut ex.cont, Continue::Done) {
        Continue::AwaitingInterim(Some(body)) => Some(body),
        Continue::AwaitingInterim(None) => {
            ex.cont = Continue::Released;
            None
        }
        other => {
            ex.cont = other;
            return;
        }
    };
    fx.push(Effect::Push(PendingHttpEvent::Continue { request_handle }));
    if let Some(body) = body {
        let framed = wire::frame_body(&body, ex.framing);
        send(st, id, framed, fx);
    }
}

/// `end()` supplied the body of an `Expect: 100-continue` request.
pub(super) fn continue_body(
    st: &mut State,
    request_handle: Handle,
    body: Vec<u8>,
    fx: &mut Vec<Effect>,
) {
    let Some(&id) = st.by_request.get(&request_handle) else {
        return;
    };
    let Some(ex) = st.conns.get_mut(&id).and_then(|c| c.exchange.as_mut()) else {
        return;
    };
    if ex.out.request_handle != request_handle || ex.out.mode != Mode::Continue {
        return;
    }
    match std::mem::replace(&mut ex.cont, Continue::Done) {
        Continue::AwaitingInterim(_) => ex.cont = Continue::AwaitingInterim(Some(body)),
        Continue::Released => {
            let framed = wire::frame_body(&body, ex.framing);
            send(st, id, framed, fx);
        }
        Continue::Done => {}
    }
}

fn on_head(st: &mut State, id: i64, head: http1::Head, reason: String, fx: &mut Vec<Effect>) {
    let Some(ex) = st.conns.get_mut(&id).and_then(|c| c.exchange.as_mut()) else {
        return;
    };
    // A final response without the interim `100`: the server declined to see
    // the body, which is never sent.
    if matches!(ex.cont, Continue::AwaitingInterim(_) | Continue::Released) {
        ex.cont = Continue::Done;
    }
    let headers: Vec<(String, String)> = head
        .headers
        .iter()
        .map(|h| {
            (
                h.name.clone(),
                String::from_utf8_lossy(&h.value).into_owned(),
            )
        })
        .collect();
    let status = head.status;
    if streams(&ex.out) {
        ex.head_delivered = true;
        fx.push(Effect::Push(PendingHttpEvent::ResponseHead {
            request_handle: ex.out.request_handle,
            status,
            status_message: reason.clone(),
            headers: headers.clone(),
            // `Head::version` is the HTTP/1 minor: 0 for 1.0, 1 for 1.1.
            http_version: (1, head.version),
        }));
    }
    ex.head = Some(ResponseHead {
        status,
        reason,
        version: head.version,
        headers,
    });
}

fn on_body(st: &mut State, id: i64, chunk: Vec<u8>, fx: &mut Vec<Effect>) {
    let Some(ex) = st.conns.get_mut(&id).and_then(|c| c.exchange.as_mut()) else {
        return;
    };
    if ex.head_delivered {
        fx.push(Effect::Push(PendingHttpEvent::ResponseChunk {
            request_handle: ex.out.request_handle,
            chunk: Bytes::from(chunk),
        }));
    } else {
        ex.buffered.extend_from_slice(&chunk);
    }
}

fn on_upgrade(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    let handoff = conn.exchange.as_ref().is_some_and(|ex| {
        ex.out.mode == Mode::Upgrade
            && !ex.out.key.https
            && ex.head.as_ref().is_some_and(|h| h.status == 101)
    });
    if !handoff {
        // A `CONNECT` 2xx, an unrequested `101`, or a `101` over TLS: the
        // message is over and the connection cannot be reused.
        finish(st, id, true, fx);
        return;
    }
    let Some(conn) = st.conns.remove(&id) else {
        return;
    };
    let Some(ex) = conn.exchange else {
        return;
    };
    clear_deadline(st, &ex, fx);
    forget_request(st, ex.out.request_handle, id);
    COMPLETED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let head = ex.head.unwrap_or(ResponseHead {
        status: 101,
        reason: String::new(),
        version: 1,
        headers: Vec::new(),
    });
    // The connection now belongs to `net`: no close, no id release here.
    fx.push(Effect::Handoff(
        id,
        PendingHttpEvent::Upgrade {
            request_handle: ex.out.request_handle,
            status: head.status,
            status_message: head.reason,
            headers: head.headers,
            socket_handle: id,
            head: conn.inbuf,
        },
    ));
}

// ── Settling ────────────────────────────────────────────────────────────────

fn clear_deadline(st: &mut State, ex: &Exchange, fx: &mut Vec<Effect>) {
    if ex.deadline != 0 && st.timers.remove(&ex.deadline).is_some() {
        fx.push(Effect::CancelTimer(ex.deadline));
    }
}

fn forget_request(st: &mut State, request_handle: Handle, id: i64) {
    if st.by_request.get(&request_handle) == Some(&id) {
        st.by_request.remove(&request_handle);
    }
}

/// Take the exchange off a connection, clearing its deadline and request
/// mapping. Every terminal path goes through here exactly once.
fn take_exchange(st: &mut State, id: i64, fx: &mut Vec<Effect>) -> Option<Exchange> {
    let ex = st.conns.get_mut(&id)?.exchange.take()?;
    clear_deadline(st, &ex, fx);
    forget_request(st, ex.out.request_handle, id);
    Some(ex)
}

/// The response ended. Deliver it, then park or close the connection.
fn finish(st: &mut State, id: i64, never_reuse: bool, fx: &mut Vec<Effect>) {
    let Some(ex) = take_exchange(st, id, fx) else {
        return;
    };
    COMPLETED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request_handle = ex.out.request_handle;
    if ex.head_delivered {
        fx.push(Effect::Push(PendingHttpEvent::ResponseEnd {
            request_handle,
        }));
    } else {
        let head = ex.head.as_ref();
        fx.push(Effect::Push(PendingHttpEvent::Response {
            request_handle,
            status: head.map_or(0, |h| h.status),
            status_message: head.map(|h| h.reason.clone()).unwrap_or_default(),
            headers: head.map(|h| h.headers.clone()).unwrap_or_default(),
            trailers: ex.trailers.clone(),
            body: ex.buffered.clone(),
            http_version: (1, head.map_or(1, |h| h.version)),
        }));
    }
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    let reuse = ex.out.reuse.filter(|_| {
        !never_reuse
            && ex.out.mode == Mode::Normal
            && !ex.closes
            && conn.decoder.reusable()
            && conn.inbuf.is_empty()
            && conn
                .tls
                .as_ref()
                .is_none_or(|s| !s.peer_closed() && s.failure().is_none())
    });
    drop(ex);
    match reuse {
        Some(reuse) => park(st, id, reuse, fx),
        None => close(st, id, fx),
    }
}

/// Return a connection to the pool (see `pool.rs` for the ordering rule this
/// is the only caller of).
fn park(st: &mut State, id: i64, reuse: super::Reuse, fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    let key = conn.key.clone();
    let parked = st.idle.get(&key).map_or(0, Vec::len);
    if parked >= reuse.max_free {
        close(st, id, fx);
        return;
    }
    let timer = next_id();
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    conn.phase = Phase::Idle;
    conn.served = conn.served.saturating_add(1);
    if timer != perry_ffi::INVALID_HANDLE {
        conn.idle_timer = timer;
        st.timers.insert(timer, Timer::Idle { conn: id });
        fx.push(Effect::ArmTimer(timer, reuse.idle_ms));
    }
    fx.push(Effect::SetRef(id, false));
    st.idle.entry(key).or_default().push(id);
}

/// Take the most recently parked live connection for `key`.
fn take_idle(st: &mut State, key: &PoolKey, fx: &mut Vec<Effect>) -> Option<i64> {
    loop {
        let id = st.idle.get_mut(key)?.pop()?;
        let Some(conn) = st.conns.get_mut(&id) else {
            continue;
        };
        if conn.phase != Phase::Idle {
            continue;
        }
        conn.phase = Phase::Open;
        let timer = std::mem::take(&mut conn.idle_timer);
        if timer != 0 && st.timers.remove(&timer).is_some() {
            fx.push(Effect::CancelTimer(timer));
        }
        fx.push(Effect::SetRef(id, true));
        return Some(id);
    }
}

fn unpark(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    let timer = std::mem::take(&mut conn.idle_timer);
    let key = conn.key.clone();
    if timer != 0 && st.timers.remove(&timer).is_some() {
        fx.push(Effect::CancelTimer(timer));
    }
    if let Some(list) = st.idle.get_mut(&key) {
        list.retain(|&parked| parked != id);
        if list.is_empty() {
            st.idle.remove(&key);
        }
    }
}

/// Submit the close. The record stays until `NET_CLOSED`, which frees the id.
fn close(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    unpark(st, id, fx);
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    if conn.phase == Phase::Closing {
        return;
    }
    conn.phase = Phase::Closing;
    fx.push(Effect::Close(id));
}

fn fail_coded(st: &mut State, id: i64, message: String, code: &str, fx: &mut Vec<Effect>) {
    if let Some(ex) = take_exchange(st, id, fx) {
        fx.push(Effect::Push(PendingHttpEvent::CodedError {
            request_handle: ex.out.request_handle,
            message,
            code: code.to_string(),
        }));
    }
    close(st, id, fx);
}

/// A framing refusal from the codec, which carries an llhttp-style code.
fn fail_protocol(st: &mut State, id: i64, error: turnloop_http::Error, fx: &mut Vec<Effect>) {
    if let Some(ex) = take_exchange(st, id, fx) {
        fx.push(Effect::Push(PendingHttpEvent::CodedError {
            request_handle: ex.out.request_handle,
            message: format!("Parse Error: {}", error.message),
            code: error.code.to_string(),
        }));
    }
    close(st, id, fx);
}

/// The connection went away before the response ended.
fn premature(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    let Some(ex) = take_exchange(st, id, fx) else {
        close(st, id, fx);
        return;
    };
    close(st, id, fx);
    report_premature(ex, fx);
}

fn report_premature(ex: Exchange, fx: &mut Vec<Effect>) {
    let request_handle = ex.out.request_handle;
    if ex.reused && !ex.got_bytes && !ex.retried {
        // The stale keep-alive race: nothing of the response arrived, so the
        // request may safely be sent again on a fresh connection.
        let Exchange { out, _inflight, .. } = ex;
        fx.push(Effect::Redispatch(out, _inflight));
        return;
    }
    if ex.head_delivered {
        // Node: the response is `'aborted'`; the drain builds that error from
        // the `IncomingMessage` it already has.
        fx.push(Effect::Push(PendingHttpEvent::Error {
            request_handle,
            error_message: "aborted".to_string(),
        }));
    } else {
        fx.push(Effect::Push(PendingHttpEvent::CodedError {
            request_handle,
            message: "socket hang up".to_string(),
            code: "ECONNRESET".to_string(),
        }));
    }
}

/// Readable EOF, from the socket or a TLS `close_notify`.
fn eof(st: &mut State, id: i64, fx: &mut Vec<Effect>) {
    let Some(conn) = st.conns.get_mut(&id) else {
        return;
    };
    match conn.phase {
        Phase::Closing => return,
        Phase::Idle | Phase::Connecting => {
            close(st, id, fx);
            return;
        }
        Phase::Tunnel => {
            premature(st, id, fx);
            return;
        }
        Phase::Open => {}
    }
    if conn.exchange.is_none() {
        close(st, id, fx);
        return;
    }
    // A body delimited by the close ends here: the decoder turns EOF into
    // `End`, which the next step delivers.
    if conn.decoder.eof().is_ok() {
        process(st, id, fx);
    }
    let still_open = st
        .conns
        .get(&id)
        .is_some_and(|conn| conn.exchange.is_some());
    if still_open {
        premature(st, id, fx);
    }
}

pub(super) fn on_eof(st: &mut State, id: i64) -> Vec<Effect> {
    let mut fx = Vec::new();
    eof(st, id, &mut fx);
    fx
}

pub(super) fn on_error(
    st: &mut State,
    id: i64,
    code: &str,
    syscall: &str,
    errno: i64,
) -> Vec<Effect> {
    let mut fx = Vec::new();
    let Some(conn) = st.conns.get_mut(&id) else {
        return fx;
    };
    match conn.phase {
        Phase::Closing => return fx,
        Phase::Idle => {
            close(st, id, &mut fx);
            return fx;
        }
        Phase::Connecting => {
            // Node's connect-failure message names the peer:
            // `connect ECONNREFUSED 127.0.0.1:1`, `getaddrinfo ENOTFOUND host`.
            let (message, code, syscall, errno) = crate::transport_error::connect_failure(
                code,
                syscall,
                errno,
                &conn.peer_host,
                conn.peer_port,
            );
            if let Some(ex) = take_exchange(st, id, &mut fx) {
                fx.push(Effect::Push(PendingHttpEvent::TransportError {
                    request_handle: ex.out.request_handle,
                    message,
                    code,
                    syscall,
                    errno,
                }));
            }
            close(st, id, &mut fx);
            return fx;
        }
        Phase::Tunnel | Phase::Open => {}
    }
    if matches!(code, "ECONNRESET" | "EPIPE" | "ECONNABORTED") {
        premature(st, id, &mut fx);
        return fx;
    }
    if let Some(ex) = take_exchange(st, id, &mut fx) {
        let message = if syscall.is_empty() {
            code.to_string()
        } else {
            format!("{syscall} {code}")
        };
        fx.push(Effect::Push(PendingHttpEvent::TransportError {
            request_handle: ex.out.request_handle,
            message,
            code: code.to_string(),
            syscall: syscall.to_string(),
            errno,
        }));
    }
    close(st, id, &mut fx);
    fx
}

pub(super) fn on_closed(st: &mut State, id: i64) -> Vec<Effect> {
    let mut fx = Vec::new();
    unpark(st, id, &mut fx);
    let Some(mut conn) = st.conns.remove(&id) else {
        return fx;
    };
    if let Some(ex) = conn.exchange.take() {
        // The socket went away with a request still on it and no terminal
        // event of its own.
        clear_deadline(st, &ex, &mut fx);
        forget_request(st, ex.out.request_handle, id);
        report_premature(ex, &mut fx);
    }
    // The terminal completion: nothing can name this id again, so it goes
    // back to the shared band (the #6441 id-exhaustion shape).
    fx.push(Effect::FreeId(id));
    fx
}

pub(super) fn on_timer(st: &mut State, timer: i64) -> Vec<Effect> {
    let mut fx = Vec::new();
    let Some(kind) = st.timers.remove(&timer) else {
        return fx;
    };
    // A fired one-shot is terminal on the runtime side: nothing to cancel.
    fx.push(Effect::FreeId(timer));
    match kind {
        Timer::Deadline { conn, request } => {
            let owns = st
                .conns
                .get(&conn)
                .and_then(|c| c.exchange.as_ref())
                .is_some_and(|ex| ex.out.request_handle == request && ex.deadline == timer);
            if !owns {
                return fx;
            }
            if let Some(mut ex) = st.conns.get_mut(&conn).and_then(|c| c.exchange.take()) {
                // Already removed from `timers`; don't cancel it again.
                ex.deadline = 0;
                forget_request(st, request, conn);
                TIMED_OUT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                fx.push(Effect::Push(PendingHttpEvent::Timeout {
                    request_handle: request,
                }));
            }
            close(st, conn, &mut fx);
        }
        Timer::Idle { conn } => {
            let idle = st
                .conns
                .get_mut(&conn)
                .filter(|c| c.phase == Phase::Idle && c.idle_timer == timer);
            if let Some(c) = idle {
                c.idle_timer = 0;
                close(st, conn, &mut fx);
            }
        }
        Timer::Deferred(event) => {
            super::DEFERRED_FIRED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            fx.push(Effect::Push(event));
        }
        Timer::RawDrain { socket } => {
            fx.extend(super::raw_socket::on_raw_drain_timer(st, socket, timer))
        }
        Timer::RawDeadline { socket } => fx.extend(super::raw_socket::on_raw_deadline(st, socket)),
    }
    fx
}

pub(super) fn cancel(st: &mut State, request_handle: Handle, fx: &mut Vec<Effect>) {
    let Some(id) = st.by_request.get(&request_handle).copied() else {
        return;
    };
    let owns = st
        .conns
        .get(&id)
        .and_then(|c| c.exchange.as_ref())
        .is_some_and(|ex| ex.out.request_handle == request_handle);
    if owns {
        drop(take_exchange(st, id, fx));
        close(st, id, fx);
    } else {
        st.by_request.remove(&request_handle);
    }
}

pub(super) fn purge_agent(st: &mut State, agent_handle: Handle, fx: &mut Vec<Effect>) {
    let idle: Vec<i64> = st
        .idle
        .iter()
        .filter(|(key, _)| key.agent == agent_handle)
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect();
    for id in idle {
        close(st, id, fx);
    }
}
