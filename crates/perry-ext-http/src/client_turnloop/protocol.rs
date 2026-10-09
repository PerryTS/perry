//! Pure HTTP client framing owned by a Socket's separate parser payload.
//! There are no transport identities, JS values, GC pointers, driver calls or
//! retirement guards here. The rooted Socket runner performs copied effects
//! after this borrow ends, using its existing generational handle capability.

use super::{tls, wire, Mode, Outbound, PoolKey, Reuse};
use crate::PendingHttpEvent;
use bytes::Bytes;
use perry_ffi::Handle;
use turnloop_http::http1;
const MAX_STEPS: usize = 1 << 20;

pub(super) enum Effect {
    Write(Vec<u8>),
    InstallTls(tls::TlsPlan),
    Ciphertext(Vec<u8>),
    Close,
    Deadline(Option<u64>),
    Push(PendingHttpEvent),
    RetireRequest(Handle),
    Redispatch(Box<Outbound>),
    Park(Reuse),
    Upgrade {
        request: Handle,
        status: u16,
        reason: String,
        headers: Vec<(String, String)>,
        head: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
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
}

pub(super) struct Conn {
    key: PoolKey,
    /// What was dialed — the proxy when proxied — for Node's error messages.
    peer_host: String,
    peer_port: u16,
    pub(super) phase: Phase,
    decoder: http1::Decoder,
    inbuf: Vec<u8>,
    exchange: Option<Exchange>,
    /// Exchanges this connection has completed.
    served: u32,
    native_tls: bool,
    preset_head: Option<Vec<u8>>,
}

fn new_decoder() -> http1::Decoder {
    http1::Decoder::new(http1::Mode::Response, http1::Limits::default())
}

pub(super) fn dial_host(url: &url::Url) -> Option<String> {
    Some(match url.host()? {
        url::Host::Domain(domain) => domain.to_string(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => address.to_string(),
    })
}

fn streams(out: &Outbound) -> bool {
    match out.mode {
        Mode::Normal | Mode::Continue => true,
        Mode::Trailers => false,
        // Upgrade is deferred until its complete head is known, including
        // over TLS: a 101 fires upgrade without an earlier response callback.
        Mode::Upgrade => false,
    }
}

impl Conn {
    pub(super) fn new(out: Outbound, native_tls: bool, preset_head: Option<Vec<u8>>) -> Self {
        let (peer_host, peer_port) = match &out.proxy {
            Some(proxy) => (
                dial_host(proxy).unwrap_or_default(),
                proxy.port_or_known_default().unwrap_or(80),
            ),
            None => (out.key.host.clone(), out.key.port),
        };
        Self {
            key: out.key.clone(),
            peer_host,
            peer_port,
            phase: Phase::Connecting,
            decoder: new_decoder(),
            inbuf: Vec::new(),
            exchange: None,
            served: 0,
            native_tls,
            preset_head,
        }
        .with_exchange(out, false, false)
    }
    fn with_exchange(mut self, out: Outbound, reused: bool, retried: bool) -> Self {
        self.exchange = Some(Exchange {
            framing: wire::Framing::Raw,
            closes: false,
            sent: false,
            reused,
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
            out: Box::new(out),
        });
        self
    }
    pub(super) fn peer(&self) -> (&str, u16) {
        (&self.peer_host, self.peer_port)
    }
    pub(super) fn retry(out: Outbound) -> Self {
        let mut conn = Self::new(out, false, None);
        if let Some(exchange) = &mut conn.exchange {
            exchange.retried = true;
        }
        conn
    }
    pub(super) fn key(&self) -> &PoolKey {
        &self.key
    }
    pub(super) fn request(&self) -> Option<Handle> {
        self.exchange.as_ref().map(|ex| ex.out.request_handle)
    }
    pub(super) fn reusable(&self) -> bool {
        self.phase == Phase::Idle && self.exchange.is_none()
    }
    pub(super) fn restart(&mut self, out: Outbound) -> Vec<Effect> {
        self.exchange = Self::new(out, self.native_tls, None).exchange;
        if let Some(ex) = &mut self.exchange {
            ex.reused = true;
        }
        self.phase = Phase::Open;
        let mut fx = Vec::new();
        if let Some(ms) = self.exchange.as_ref().and_then(|ex| ex.out.timeout_ms) {
            fx.push(Effect::Deadline(Some(ms)));
        }
        send_request(self, &mut fx);
        fx
    }
}

pub(super) fn on_connect(conn: &mut Conn) -> Vec<Effect> {
    let mut fx = Vec::new();
    if conn.phase != Phase::Connecting {
        return fx;
    }
    // Reading starts before anything is written: a loopback peer's reply can
    // be in flight before the write submission returns.
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
        fx.push(Effect::Write(head));
        return fx;
    }
    conn.phase = Phase::Open;
    if conn.key.https && !open_tls(conn, &mut fx) {
        return fx;
    }
    send_request(conn, &mut fx);
    fx
}

fn open_tls(conn: &mut Conn, fx: &mut Vec<Effect>) -> bool {
    if conn.native_tls {
        return true;
    }
    if let Some(plan) = conn.exchange.as_ref().and_then(|ex| ex.out.tls.clone()) {
        conn.native_tls = true;
        fx.push(Effect::InstallTls(plan));
        true
    } else {
        fail_coded(
            conn,
            "https request without a TLS configuration".into(),
            "ERR_SSL_PROTOCOL_ERROR",
            fx,
        );
        false
    }
}

fn send_request(conn: &mut Conn, fx: &mut Vec<Effect>) {
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
    if let Some(bytes) = conn.preset_head.take() {
        ex.sent = true;
        conn.decoder
            .response_to(&ex.out.method.to_ascii_uppercase());
        fx.push(Effect::Write(bytes));
        return;
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
    send(conn, bytes, fx);
}

fn send(_: &mut Conn, bytes: Vec<u8>, fx: &mut Vec<Effect>) {
    fx.push(Effect::Write(bytes));
}

pub(super) fn on_data(conn: &mut Conn, bytes: &[u8]) -> Vec<Effect> {
    let mut fx = Vec::new();
    match conn.phase {
        Phase::Connecting | Phase::Closing => (),
        Phase::Tunnel => tunnel_input(conn, bytes, &mut fx),
        Phase::Open | Phase::Idle => http_input(conn, bytes, &mut fx),
    }
    fx
}

fn tunnel_input(conn: &mut Conn, bytes: &[u8], fx: &mut Vec<Effect>) {
    conn.inbuf.extend_from_slice(bytes);
    for _ in 0..MAX_STEPS {
        let step = match conn.decoder.receive(&conn.inbuf) {
            Ok(step) => step,
            Err(error) => {
                fail_coded(
                    conn,
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
                fail_coded(conn, message, "ERR_PROXY_TUNNEL", fx);
                return;
            }
            Some(Ok(())) => {
                let leftover = std::mem::take(&mut conn.inbuf);
                conn.decoder = new_decoder();
                conn.phase = Phase::Open;
                if !open_tls(conn, fx) {
                    return;
                }
                send_request(conn, fx);
                if !leftover.is_empty() {
                    fx.push(Effect::Ciphertext(leftover));
                }
                return;
            }
        }
    }
}

fn http_input(conn: &mut Conn, bytes: &[u8], fx: &mut Vec<Effect>) {
    conn.inbuf.extend_from_slice(bytes);
    process(conn, fx);
}

enum Decoded {
    Head(http1::Head, String),
    Informational(u16),
    Body(Vec<u8>),
    Trailers(Vec<http1::Header>),
    End,
    Upgrade,
    Nothing,
}

fn process(conn: &mut Conn, fx: &mut Vec<Effect>) {
    for _ in 0..MAX_STEPS {
        let Some(ex) = conn.exchange.as_mut() else {
            // Bytes on a connection with no request in flight: unsolicited
            // data on an idle keep-alive socket, or bytes after a response
            // ended. Neither can be framed; the connection is done.
            if !conn.inbuf.is_empty() {
                close(conn, fx);
            }
            return;
        };
        if !conn.inbuf.is_empty() {
            ex.got_bytes = true;
        }
        let step = match conn.decoder.receive(&conn.inbuf) {
            Ok(step) => step,
            Err(error) => {
                fail_protocol(conn, error, fx);
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
            Decoded::Informational(status) => on_informational(conn, status, fx),
            Decoded::Head(head, reason) => on_head(conn, head, reason, fx),
            Decoded::Body(chunk) => on_body(conn, chunk, fx),
            Decoded::Trailers(trailers) => {
                if let Some(ex) = conn.exchange.as_mut() {
                    ex.trailers = trailers
                        .into_iter()
                        .map(|h| (h.name, String::from_utf8_lossy(&h.value).into_owned()))
                        .collect();
                }
            }
            Decoded::End => finish(conn, false, fx),
            Decoded::Upgrade => on_upgrade(conn, fx),
        }
    }
}

fn on_informational(conn: &mut Conn, status: u16, fx: &mut Vec<Effect>) {
    if status != 100 {
        // `102`/`103` — Node's `'information'`, which neither the reqwest
        // path nor the bypasses surfaced.
        return;
    }
    let Some(ex) = conn.exchange.as_mut() else {
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
        send(conn, framed, fx);
    }
}

pub(super) fn continue_body(
    conn: &mut Conn,
    request_handle: Handle,
    body: Vec<u8>,
    fx: &mut Vec<Effect>,
) {
    let Some(ex) = conn.exchange.as_mut() else {
        return;
    };
    if ex.out.request_handle != request_handle || ex.out.mode != Mode::Continue {
        return;
    }
    match std::mem::replace(&mut ex.cont, Continue::Done) {
        Continue::AwaitingInterim(_) => ex.cont = Continue::AwaitingInterim(Some(body)),
        Continue::Released => {
            let framed = wire::frame_body(&body, ex.framing);
            send(conn, framed, fx);
        }
        Continue::Done => {}
    }
}

fn on_head(conn: &mut Conn, head: http1::Head, reason: String, fx: &mut Vec<Effect>) {
    let Some(ex) = conn.exchange.as_mut() else {
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

fn on_body(conn: &mut Conn, chunk: Vec<u8>, fx: &mut Vec<Effect>) {
    let Some(ex) = conn.exchange.as_mut() else {
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

fn on_upgrade(conn: &mut Conn, fx: &mut Vec<Effect>) {
    let requested = conn.exchange.as_ref().is_some_and(|ex| {
        ex.out.mode == Mode::Upgrade && ex.head.as_ref().is_some_and(|head| head.status == 101)
    });
    if !requested {
        finish(conn, true, fx);
        return;
    }
    let Some(ex) = take_exchange(conn, fx) else {
        return;
    };
    let head = ex.head.unwrap();
    // TLS remains on the Socket. Parser disposal cannot discard the session.
    fx.push(Effect::Upgrade {
        request: ex.out.request_handle,
        status: head.status,
        reason: head.reason,
        headers: head.headers,
        head: std::mem::take(&mut conn.inbuf),
    });
    conn.phase = Phase::Closing;
}
fn take_exchange(conn: &mut Conn, fx: &mut Vec<Effect>) -> Option<Exchange> {
    let ex = conn.exchange.take()?;
    fx.push(Effect::Deadline(None));
    fx.push(Effect::RetireRequest(ex.out.request_handle));
    Some(ex)
}

fn finish(conn: &mut Conn, never_reuse: bool, fx: &mut Vec<Effect>) {
    let Some(ex) = take_exchange(conn, fx) else {
        return;
    };
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
    let reuse = ex.out.reuse.filter(|_| {
        !never_reuse
            && ex.out.mode == Mode::Normal
            && !ex.closes
            && conn.decoder.reusable()
            && conn.inbuf.is_empty()
    });
    drop(ex);
    match reuse {
        Some(reuse) => park(conn, reuse, fx),
        None => close(conn, fx),
    }
}

fn park(conn: &mut Conn, reuse: Reuse, fx: &mut Vec<Effect>) {
    conn.phase = Phase::Idle;
    conn.served = conn.served.saturating_add(1);
    fx.push(Effect::Park(reuse));
}
fn close(conn: &mut Conn, fx: &mut Vec<Effect>) {
    if conn.phase != Phase::Closing {
        conn.phase = Phase::Closing;
        fx.push(Effect::Close);
    }
}

fn fail_coded(conn: &mut Conn, message: String, code: &str, fx: &mut Vec<Effect>) {
    if let Some(ex) = take_exchange(conn, fx) {
        fx.push(Effect::Push(PendingHttpEvent::CodedError {
            request_handle: ex.out.request_handle,
            message,
            code: code.to_string(),
        }));
    }
    close(conn, fx);
}

fn fail_protocol(conn: &mut Conn, error: turnloop_http::Error, fx: &mut Vec<Effect>) {
    if let Some(ex) = take_exchange(conn, fx) {
        fx.push(Effect::Push(PendingHttpEvent::CodedError {
            request_handle: ex.out.request_handle,
            message: format!("Parse Error: {}", error.message),
            code: error.code.to_string(),
        }));
    }
    close(conn, fx);
}

fn premature(conn: &mut Conn, fx: &mut Vec<Effect>) {
    let Some(ex) = take_exchange(conn, fx) else {
        close(conn, fx);
        return;
    };
    close(conn, fx);
    report_premature(ex, fx);
}

fn report_premature(ex: Exchange, fx: &mut Vec<Effect>) {
    let request_handle = ex.out.request_handle;
    if ex.reused && !ex.got_bytes && !ex.retried {
        // The stale keep-alive race: nothing of the response arrived, so the
        // request may safely be sent again on a fresh connection.
        let Exchange { out, .. } = ex;
        fx.push(Effect::Redispatch(out));
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

fn eof(conn: &mut Conn, fx: &mut Vec<Effect>) {
    match conn.phase {
        Phase::Closing => return,
        Phase::Idle | Phase::Connecting => {
            close(conn, fx);
            return;
        }
        Phase::Tunnel => {
            premature(conn, fx);
            return;
        }
        Phase::Open => {}
    }
    if conn.exchange.is_none() {
        close(conn, fx);
        return;
    }
    // A body delimited by the close ends here: the decoder turns EOF into
    // `End`, which the next step delivers.
    if conn.decoder.eof().is_ok() {
        process(conn, fx);
    }
    let still_open = conn.exchange.is_some();
    if still_open {
        premature(conn, fx);
    }
}

pub(super) fn on_eof(conn: &mut Conn) -> Vec<Effect> {
    let mut fx = Vec::new();
    eof(conn, &mut fx);
    fx
}

pub(super) fn on_error(conn: &mut Conn, code: &str, syscall: &str, errno: i64) -> Vec<Effect> {
    let mut fx = Vec::new();
    match conn.phase {
        Phase::Closing => return fx,
        Phase::Idle => {
            close(conn, &mut fx);
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
            if let Some(ex) = take_exchange(conn, &mut fx) {
                fx.push(Effect::Push(PendingHttpEvent::TransportError {
                    request_handle: ex.out.request_handle,
                    message,
                    code,
                    syscall,
                    errno,
                }));
            }
            close(conn, &mut fx);
            return fx;
        }
        Phase::Tunnel | Phase::Open => {}
    }
    if matches!(code, "ECONNRESET" | "EPIPE" | "ECONNABORTED") {
        premature(conn, &mut fx);
        return fx;
    }
    if let Some(ex) = take_exchange(conn, &mut fx) {
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
    close(conn, &mut fx);
    fx
}

pub(super) fn on_closed(conn: &mut Conn) -> Vec<Effect> {
    let mut fx = Vec::new();
    if let Some(ex) = take_exchange(conn, &mut fx) {
        report_premature(ex, &mut fx);
    }
    conn.phase = Phase::Closing;
    fx
}
pub(super) fn on_timer(conn: &mut Conn) -> Vec<Effect> {
    let mut fx = Vec::new();
    if conn.phase == Phase::Idle {
        close(conn, &mut fx);
    } else if let Some(ex) = take_exchange(conn, &mut fx) {
        fx.push(Effect::Push(PendingHttpEvent::Timeout {
            request_handle: ex.out.request_handle,
        }));
        close(conn, &mut fx);
    }
    fx
}
pub(super) fn cancel(conn: &mut Conn, request: Handle) -> Vec<Effect> {
    let mut fx = Vec::new();
    if conn.request() == Some(request) {
        drop(take_exchange(conn, &mut fx));
        close(conn, &mut fx);
    }
    fx
}

/// The Socket's explicit close hook cannot enter the driver. It copies the
/// terminal app outcome, then releases this parser before returning.
pub(super) fn on_destroy(conn: &mut Conn, code: &str, message: &str) -> Vec<Effect> {
    let mut fx = Vec::new();
    fail_coded(conn, message.to_owned(), code, &mut fx);
    fx
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
