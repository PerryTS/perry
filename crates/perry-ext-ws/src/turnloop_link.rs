//! The turnloop transport: a WebSocket connection with no stream, no task and
//! no channel.
//!
//! # Why this exists, and what it answers
//!
//! P5 left one server surface on hyper, and named the reason: *"its handshake
//! needs an owned stream for `tokio_tungstenite`, which a turnloop connection
//! cannot produce."* The second half is true and the first half is a property
//! of `tokio_tungstenite`, not of WebSocket. `turnloop_websocket` is sans-I/O,
//! so both the handshake ([`crate::handshake`]) and the framing
//! ([`crate::codec`]) are pure functions over bytes — and a turnloop connection
//! has bytes.
//!
//! So nothing moves. The host — `perry-ext-http`'s `turnloop_serve` — keeps the
//! connection, its id, its outstanding multishot read and its TLS layer, and
//! simply stops handing the bytes to an HTTP decoder and starts handing them
//! here. That is the same shape P5 used for TLS (a session installed *above* a
//! turnloop handle so no descriptor has to move) rather than the shape it used
//! for `server.on('upgrade')` (`turnloop_net::transfer`, which changes the
//! owner). A WebSocket does not change owner: `perry-ext-http` still holds the
//! connection, and this module holds only the protocol state keyed by its id.
//!
//! # The dependency direction
//!
//! `perry-ext-http` already depends on `perry-ext-ws`; the reverse would be a
//! cycle. So the host installs a [`Transport`] of function pointers at
//! registration time — the same one-way trick `register_http_address_reader`
//! uses — and this module calls back through it to put bytes on the wire.
//! Nothing here knows whether the connection is TLS: the host's writer is
//! already TLS-transparent.
//!
//! # GC
//!
//! A link holds an id, a codec and owned `Vec<u8>`s. No JS value and no heap
//! pointer, so this module registers no root scanner — the ws client id it
//! allocates is scanned through the crate's existing `WS_CLIENT_LISTENERS`
//! entry like any other client.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use turnloop_http::http1::{Head, Header};

use crate::codec::{Codec, Incoming, Role};
use crate::handshake;

/// What a host transport must provide for a connection it keeps owning.
#[derive(Clone, Copy)]
pub struct Transport {
    /// Put these bytes on the connection. Must be TLS-transparent.
    pub write: fn(i64, &[u8]),
    /// Close gracefully: everything already queued goes out first, then FIN.
    ///
    /// This is NOT `destroy`. A closing handshake ends with a close frame
    /// written and then a shutdown, and a turnloop `close` cancels the
    /// connection's outstanding operations — including the write that was just
    /// queued. P5 hit the same edge from the other side (its `allowHalfOpen`
    /// close cancelled the writes an `'end'` handler had queued), so the two
    /// questions are kept apart here: whether to stop using the connection, and
    /// whether it may go away yet.
    pub finish: fn(i64),
    /// Tear the connection down now, cancelling whatever is in flight —
    /// `ws.terminate()` and the error paths.
    pub destroy: fn(i64),
}

static TRANSPORT: OnceLock<Transport> = OnceLock::new();

/// Install the *default* host transport — the one [`adopt`] uses.
///
/// Idempotent; the first registration wins, which matters because
/// `perry-ext-http` registers from more than one entry point. A second host in
/// the same binary does not fight over this slot: it calls [`adopt_with`] and
/// hands its own [`Transport`] per connection. That distinction is load-bearing
/// now that three hosts exist — `perry-ext-http`'s turnloop server, its hyper
/// upgrade path, and this crate's own standalone `WebSocketServer({ port })` —
/// and a single global would have silently given all three the first one's
/// writer.
pub fn register_transport(transport: Transport) {
    let _ = TRANSPORT.set(transport);
}

struct Link {
    ws_id: usize,
    codec: Codec,
    /// The closing handshake has been started from this side.
    closing: bool,
    /// How this particular connection's bytes reach the wire. Per link, not
    /// per process: see [`register_transport`].
    transport: Transport,
}

fn links() -> &'static Mutex<HashMap<i64, Link>> {
    static LINKS: OnceLock<Mutex<HashMap<i64, Link>>> = OnceLock::new();
    LINKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Does this turnloop connection carry a WebSocket?
pub fn owns(conn_id: i64) -> bool {
    links()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&conn_id)
}

/// The transport a live link uses, or the default one for a link this side has
/// already forgotten (a close racing a write).
fn transport_of(conn_id: i64) -> Option<Transport> {
    with_link(conn_id, |link| link.transport).or_else(|| TRANSPORT.get().copied())
}

fn write(conn_id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if let Some(transport) = transport_of(conn_id) {
        (transport.write)(conn_id, bytes);
    }
}

/// Rebuild the decoded request as a `turnloop_http` head.
///
/// The host has already parsed the request; `accept` needs it back in the crate's
/// own shape and nothing else. Only the raw header list matters — `accept`
/// reads `connection`, `upgrade`, `sec-websocket-{key,version,protocol}`.
pub fn request_head(method: &str, target: &str, version: u8, headers: &[(String, String)]) -> Head {
    Head {
        method: method.to_string(),
        target: target.to_string(),
        status: 0,
        version,
        headers: headers
            .iter()
            .map(|(name, value)| Header::new(name, value.as_bytes()))
            .collect(),
        keep_alive: true,
    }
}

/// The server-side handshake, for a connection the host keeps.
///
/// Returns the `101` bytes to write, or the refusal to write instead. **This
/// function performs no I/O** — it is the whole of the "does the handshake need
/// an owned stream" question, and the answer is the signature.
pub fn accept_response(
    request: &Head,
    protocols: &[&str],
) -> Result<(Vec<u8>, Option<String>), handshake::HandshakeError> {
    handshake::accept(request, protocols)
}

/// The canned refusal `ws` writes for a request it will not upgrade.
pub fn reject_response(status: u16, message: &str) -> Vec<u8> {
    handshake::reject(status, message)
}

/// Adopt a connection whose `101` the host has already written.
///
/// `leftover` is whatever followed the request head in the same read — frame
/// data the peer pipelined behind its handshake, which `ws` delivers.
pub fn adopt(conn_id: i64, leftover: &[u8]) -> i64 {
    let Some(transport) = TRANSPORT.get().copied() else {
        // No default transport was ever registered, so nothing could put a
        // byte on this connection. Refusing is the honest answer; adopting
        // would produce a client that silently never sends.
        return 0;
    };
    adopt_with(conn_id, transport, Role::Server, leftover)
}

/// Adopt a connection whose host supplies its own [`Transport`].
///
/// `role` is this end of the WebSocket: [`Role::Server`] for a connection this
/// process accepted, [`Role::Client`] for one it dialled — the codec masks
/// client frames and refuses masked server ones, so it is not cosmetic.
pub fn adopt_with(conn_id: i64, transport: Transport, role: Role, leftover: &[u8]) -> i64 {
    let ws_id = crate::register_turnloop_client(conn_id);
    adopt_existing(conn_id, ws_id, transport, role, leftover);
    ws_id as i64
}

/// Install the protocol on a connection whose JS-visible id already exists.
///
/// The outbound client and the standalone server both hand their id out — or
/// publish their parent-server link — *before* the handshake finishes, so that
/// `ws.on(...)` can be registered against a connecting socket and so that a
/// frame pipelined behind the `101` routes to the server's own `'message'`
/// listener. Allocating a second id here would strand both.
pub(crate) fn adopt_existing(
    conn_id: i64,
    ws_id: usize,
    transport: Transport,
    role: Role,
    leftover: &[u8],
) {
    // Publish the link BEFORE decoding the leftover: `on_data` delivers events
    // through the same tables, and a message pipelined behind the handshake
    // would otherwise be emitted for a client nothing can route.
    links().lock().unwrap_or_else(|e| e.into_inner()).insert(
        conn_id,
        Link {
            ws_id,
            codec: Codec::new(role),
            closing: false,
            transport,
        },
    );
    if !leftover.is_empty() {
        on_data(conn_id, leftover);
    }
}

/// The host's sink saw data. Runs on the loop thread inside the host's dispatch
/// call, so it queues JS events but never runs JS.
pub fn on_data(conn_id: i64, bytes: &[u8]) {
    let Some((ws_id, events, out, terminal)) = with_link(conn_id, |link| {
        let events = link.codec.receive(bytes);
        (
            link.ws_id,
            events,
            link.codec.take_output(),
            link.codec.is_terminal(),
        )
    }) else {
        return;
    };
    write(conn_id, &out);
    deliver(conn_id, ws_id, events, terminal);
}

/// The peer half-closed.
///
/// Returns whether this layer still owned the connection. `false` means the
/// close handshake already finished and the host has already shut its own side
/// down — a second shutdown then answers `ENOTCONN`, which arrives as a
/// `NET_ERROR` and used to be answered with `destroy_connection`, i.e. a reset.
/// The reset raced the answering close frame still on the wire, so an external
/// peer saw 1006 instead of the code it had just been echoed.
pub fn on_eof(conn_id: i64) -> bool {
    let Some((ws_id, code)) = with_link(conn_id, |link| (link.ws_id, link.codec.eof())) else {
        return false;
    };
    if let Some(code) = code {
        crate::connection_closed(ws_id, code, String::new());
    }
    // The host owns the connection and closes its own side; this layer owns
    // only the protocol.
    forget(conn_id);
    true
}

/// The connection is gone — the terminal completion, whatever caused it.
pub fn on_closed(conn_id: i64) {
    let Some((ws_id, code)) = with_link(conn_id, |link| (link.ws_id, link.codec.eof())) else {
        return;
    };
    crate::connection_closed(
        ws_id,
        code.unwrap_or(crate::codec::CLOSE_ABNORMAL),
        String::new(),
    );
    forget(conn_id);
}

/// A transport-level error.
///
/// Returns whether this layer still owned the connection. `false` means the
/// error arrived after the close handshake finished — teardown noise on a
/// socket nobody is reading, which Node does not report either, and which must
/// NOT be answered by destroying a handle whose last write may still be in
/// flight.
pub fn on_error(conn_id: i64, message: &str) -> bool {
    let Some(ws_id) = with_link(conn_id, |link| link.ws_id) else {
        return false;
    };
    crate::connection_error(ws_id, message);
    crate::connection_closed(ws_id, crate::codec::CLOSE_ABNORMAL, String::new());
    forget(conn_id);
    true
}

/// `ws.send(...)` on a turnloop-carried client.
pub(crate) fn send(conn_id: i64, outgoing: crate::WsOutgoing) -> bool {
    let Some(result) = with_link(conn_id, |link| {
        let sent = link.codec.send(outgoing.into_message());
        (link.ws_id, sent, link.codec.take_output())
    }) else {
        return false;
    };
    let (ws_id, sent, out) = result;
    match sent {
        Ok(()) => {
            write(conn_id, &out);
            true
        }
        Err(e) => {
            crate::connection_error(ws_id, &crate::codec_error_message(&e));
            false
        }
    }
}

/// `ws.close(code, reason)` on a turnloop-carried client.
///
/// This starts the closing handshake and returns; the connection lives until
/// the peer answers, `Codec`'s close deadline fires, or the transport dies.
/// That is `ws`'s contract, and it is why this does not destroy the handle.
pub(crate) fn close(conn_id: i64, code: Option<u16>, reason: &str) -> bool {
    let Some((ws_id, result, out, already)) = with_link(conn_id, |link| {
        if link.closing {
            return (link.ws_id, Ok(()), Vec::new(), true);
        }
        link.closing = true;
        let result = link.codec.close(code, reason);
        (link.ws_id, result, link.codec.take_output(), false)
    }) else {
        return false;
    };
    if already {
        return true;
    }
    if let Err(e) = result {
        crate::connection_error(ws_id, &crate::codec_error_message(&e));
        return false;
    }
    write(conn_id, &out);
    true
}

/// `ws.terminate()` — no closing handshake, just drop the connection.
pub(crate) fn terminate(conn_id: i64) -> bool {
    let Some(ws_id) = with_link(conn_id, |link| link.ws_id) else {
        return false;
    };
    crate::connection_closed(ws_id, crate::codec::CLOSE_ABNORMAL, String::new());
    forget_and(conn_id, |t| t.destroy);
    true
}

fn with_link<R>(conn_id: i64, f: impl FnOnce(&mut Link) -> R) -> Option<R> {
    let mut map = links().lock().unwrap_or_else(|e| e.into_inner());
    map.get_mut(&conn_id).map(f)
}

fn forget(conn_id: i64) -> Option<Transport> {
    links()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&conn_id)
        .map(|link| link.transport)
}

/// Forget the link, then end the connection through the transport the link
/// was adopted with.
///
/// The transport is taken out of the removed link rather than looked up
/// afterwards. A lookup after `forget` finds no link and falls back to the
/// process-wide default, which only `perry-ext-http` registers: in a program
/// with a standalone `WebSocketServer({ port })` and a `ws` client, and no
/// `http` import, that default does not exist, so the socket was never shut
/// down and its handle kept the event loop alive for ever (#11309). Where the
/// default does exist it is `perry-ext-http`'s writer, the wrong one for a
/// connection this crate dialled or accepted.
fn forget_and(conn_id: i64, end: impl FnOnce(Transport) -> fn(i64)) {
    if let Some(transport) = forget(conn_id).or_else(|| TRANSPORT.get().copied()) {
        end(transport)(conn_id);
    }
}

fn deliver(
    conn_id: i64,
    ws_id: usize,
    events: Result<Vec<Incoming>, crate::codec::WsError>,
    terminal: bool,
) {
    match events {
        Ok(events) => {
            let mut closed = None;
            for event in events {
                if let Incoming::Close(frame) = &event {
                    closed = Some(
                        frame
                            .clone()
                            .unwrap_or((crate::codec::CLOSE_NO_STATUS, String::new())),
                    );
                }
                crate::emit_incoming(ws_id, event);
            }
            if let Some((code, reason)) = closed {
                // The codec queued the answering close and the host wrote it
                // just above. `finish`, not `destroy`: a close that cancels its
                // own close frame is a reset, and the peer would then report
                // 1006 instead of the code it sent.
                crate::connection_closed(ws_id, code, reason);
                forget_and(conn_id, |t| t.finish);
            } else if terminal {
                crate::connection_closed(ws_id, crate::codec::CLOSE_ABNORMAL, String::new());
                forget_and(conn_id, |t| t.finish);
            }
        }
        Err(e) => {
            // A protocol error: the codec has queued a close frame naming it,
            // which `ws` sends before going away.
            crate::connection_error(ws_id, &crate::codec_error_message(&e));
            crate::connection_closed(ws_id, crate::codec::CLOSE_ABNORMAL, String::new());
            forget_and(conn_id, |t| t.finish);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static WRITES: AtomicUsize = AtomicUsize::new(0);
    static SECOND_WRITES: AtomicUsize = AtomicUsize::new(0);

    fn count_write(_id: i64, bytes: &[u8]) {
        WRITES.fetch_add(bytes.len(), Ordering::SeqCst);
    }
    fn count_second_write(_id: i64, bytes: &[u8]) {
        SECOND_WRITES.fetch_add(bytes.len(), Ordering::SeqCst);
    }
    fn noop_destroy(_id: i64) {}

    /// The acceptance property, stated as a test: a server handshake is a pure
    /// function of the request head and produces bytes, with no stream, no
    /// descriptor and no transport of any kind in scope.
    #[test]
    fn a_handshake_needs_no_stream() {
        let head = request_head(
            "GET",
            "/socket",
            1,
            &[
                ("host".into(), "example.test".into()),
                ("upgrade".into(), "websocket".into()),
                ("connection".into(), "Upgrade".into()),
                (
                    "sec-websocket-key".into(),
                    "dGhlIHNhbXBsZSBub25jZQ==".into(),
                ),
                ("sec-websocket-version".into(), "13".into()),
            ],
        );
        let (bytes, protocol) = accept_response(&head, &[]).expect("a 101");
        assert_eq!(protocol, None);
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            text.starts_with("HTTP/1.1 101 Switching Protocols\r\n"),
            "{text}"
        );
        assert!(
            text.contains("sec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
            "{text}"
        );
    }

    #[test]
    fn a_request_that_is_not_an_upgrade_is_refused_and_has_a_canned_response() {
        let head = request_head("GET", "/", 1, &[("host".into(), "h".into())]);
        assert!(accept_response(&head, &[]).is_err());
        let refusal = String::from_utf8(reject_response(400, "Bad Request")).unwrap();
        assert!(
            refusal.starts_with("HTTP/1.1 400 Bad Request\r\n"),
            "{refusal}"
        );
        assert!(refusal.contains("connection: close"), "{refusal}");
    }

    /// A per-link transport is not a per-process one: two hosts in the same
    /// binary each get their own writer. The single `OnceLock` this replaced
    /// silently handed all of them the first registration's.
    #[test]
    fn each_link_keeps_the_transport_it_was_adopted_with() {
        WRITES.store(0, Ordering::SeqCst);
        SECOND_WRITES.store(0, Ordering::SeqCst);
        adopt_existing(
            -101,
            9101,
            Transport {
                write: count_write,
                finish: noop_destroy,
                destroy: noop_destroy,
            },
            Role::Server,
            &[],
        );
        adopt_existing(
            -102,
            9102,
            Transport {
                write: count_second_write,
                finish: noop_destroy,
                destroy: noop_destroy,
            },
            Role::Server,
            &[],
        );
        // A ping is answered by the codec, so each link writes on its own
        // transport and nowhere else.
        on_data(-101, &[0x89, 0x80, 0, 0, 0, 0]);
        assert!(WRITES.load(Ordering::SeqCst) > 0, "the first link wrote");
        assert_eq!(
            SECOND_WRITES.load(Ordering::SeqCst),
            0,
            "and not through the second link's transport"
        );
        forget(-101);
        forget(-102);
    }

    static FINISHES: AtomicUsize = AtomicUsize::new(0);
    static DESTROYS: AtomicUsize = AtomicUsize::new(0);

    fn count_finish(_id: i64) {
        FINISHES.fetch_add(1, Ordering::SeqCst);
    }
    fn count_destroy(_id: i64) {
        DESTROYS.fetch_add(1, Ordering::SeqCst);
    }

    fn counting_transport() -> Transport {
        Transport {
            write: count_write,
            finish: count_finish,
            destroy: count_destroy,
        }
    }

    /// #11309: a completed closing handshake ends the connection through the
    /// transport the link was adopted with. It used to `forget` the link first
    /// and then look the transport up, which found only the process default —
    /// absent unless `perry-ext-http` is linked — so the socket was never shut
    /// down and a standalone server + client program never exited.
    #[test]
    fn a_completed_close_finishes_through_the_links_own_transport() {
        FINISHES.store(0, Ordering::SeqCst);
        adopt_existing(-201, 9201, counting_transport(), Role::Server, &[]);
        // A masked, empty client close frame.
        on_data(-201, &[0x88, 0x80, 0, 0, 0, 0]);
        assert_eq!(FINISHES.load(Ordering::SeqCst), 1);
        assert!(
            !owns(-201),
            "the link is forgotten once the close completes"
        );
    }

    #[test]
    fn terminate_destroys_through_the_links_own_transport() {
        DESTROYS.store(0, Ordering::SeqCst);
        adopt_existing(-202, 9202, counting_transport(), Role::Client, &[]);
        assert!(terminate(-202));
        assert_eq!(DESTROYS.load(Ordering::SeqCst), 1);
        assert!(!owns(-202));
    }

    #[test]
    fn an_unregistered_connection_is_inert_rather_than_a_panic() {
        register_transport(Transport {
            write: count_write,
            finish: noop_destroy,
            destroy: noop_destroy,
        });
        // Every entry point must tolerate an id it has never seen: the host's
        // sink can deliver a completion for a connection this side already
        // forgot (a close racing a read).
        on_data(-1, b"\x81\x00");
        on_eof(-1);
        on_closed(-1);
        on_error(-1, "gone");
        assert!(!owns(-1));
    }
}
