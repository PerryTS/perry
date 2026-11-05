//! One place that decides which turnloop transport a `ServerResponse` belongs
//! to.
//!
//! `ServerResponse::turnloop` is `Some((connection id, seq))` for both
//! transports: P5's HTTP/1.1 connections number their responses with a
//! per-connection sequence, and HTTP/2's carry the RFC 9113 stream id in the
//! same field. The id domains are shared (both modules allocate from
//! `turnloop_serve::next_id`), so the discriminator is ownership —
//! `turnloop_h2::owns` answers by id, and a connection is in exactly one table.
//!
//! Routing here rather than at each of `response.rs`'s call sites keeps the
//! decision in one readable place and stops a new response entry point from
//! silently reaching only HTTP/1.1.

use crate::server::response::ResponseShape;
use perry_ext_net::native_transport::{self as net, RootedSocket};

/// HTTP responses write to the ordinary Socket in their own `socket` slot,
/// or to their logical HTTP/2 session. Neither variant is a transport id or
/// an id-to-cell alias, and neither holds a JS value: the socket is read from
/// the response at the call (`response_socket`).
#[derive(Clone, Copy)]
pub(crate) enum ResponseConnection {
    Socket {
        incarnation: perry_ffi::turnloop_net::HandleSnapshot,
    },
    H2Session(i64),
}
impl ResponseConnection {
    /// `socket` is the response's socket, read at the call.
    pub(crate) fn root(self, socket: f64) -> RootedConnection {
        match self {
            Self::Socket { incarnation } => RootedConnection::Socket {
                owner: RootedSocket::with_snapshot(socket, incarnation),
            },
            Self::H2Session(session) => RootedConnection::H2Session(session),
        }
    }
}
pub(crate) enum RootedConnection {
    Socket { owner: RootedSocket },
    H2Session(i64),
}
impl RootedConnection {
    fn socket(&self) -> Option<&RootedSocket> {
        match self {
            Self::Socket { owner } if owner.is_current() => Some(owner),
            _ => None,
        }
    }
    fn h2(&self) -> Option<crate::server::turnloop_h2::target::Target> {
        match self {
            Self::H2Session(session) => {
                crate::server::http2_server::turnloop_conn_of_session(*session)
            }
            _ => None,
        }
    }
}

pub(crate) fn destroy(connection: &RootedConnection) {
    if let Some(socket) = connection.socket() {
        net::destroy(socket.value())
    } else if let Some(connection) = connection.h2() {
        crate::server::turnloop_h2::conn::destroy_connection(connection)
    }
}
pub(crate) fn is_live(connection: &RootedConnection) -> bool {
    connection.socket().is_some() || connection.h2().is_some_and(|target| target.is_live())
}
pub(crate) fn send_interim(connection: &RootedConnection, seq: u64, bytes: &[u8]) {
    if let Some(socket) = connection.socket() {
        crate::server::turnloop_serve::send_interim(socket, seq, bytes)
    }
}

/// `res.end(body)` on a fully buffered response.
pub(crate) fn send_response(conn: &RootedConnection, seq: u64, shape: ResponseShape) {
    send_response_notified(conn, seq, shape, None);
}

/// [`send_response`] on an HTTP/1.1 connection that asks `notify` for a
/// callback to run once the response bytes have reached the OS. Returns
/// whether it was armed; HTTP/2 never arms it.
pub(crate) fn send_response_notified(
    conn: &RootedConnection,
    seq: u64,
    shape: ResponseShape,
    notify: Option<&mut dyn FnMut() -> f64>,
) -> bool {
    if let Some(h2) = conn.h2() {
        crate::server::turnloop_h2::h2_send_response(h2, seq as u32, shape);
        false
    } else if let Some(socket) = conn.socket() {
        crate::server::turnloop_serve::send_response_notified(socket, seq, shape, notify)
    } else {
        false
    }
}

/// `res.flushHeaders()` / the first `res.write(...)`: start the stream. An
/// HTTP/1 head is held back when `defer_head` is set (a corked first write);
/// an HTTP/2 stream has no message-level cork and sends its head now.
pub(crate) fn begin_stream(
    conn: &RootedConnection,
    seq: u64,
    shape: ResponseShape,
    defer_head: bool,
) -> bool {
    if let Some(h2) = conn.h2() {
        crate::server::turnloop_h2::h2_begin_stream(h2, seq as u32, shape)
    } else {
        conn.socket().is_some_and(|socket| {
            crate::server::turnloop_serve::begin_stream(socket, seq, shape, defer_head)
        })
    }
}

/// The length of an HTTP/1 head that is encoded but not sent yet.
pub(crate) fn unsent_head_len(conn: &RootedConnection, seq: u64) -> usize {
    if conn.h2().is_some() {
        return 0;
    }
    conn.socket().map_or(0, |socket| {
        crate::server::turnloop_serve::unsent_head_len(socket, seq)
    })
}

/// The HTTP/1 framing of a streaming response: `Some(true)` for a chunked
/// body, `Some(false)` for a length- or close-delimited one, and `None` on an
/// HTTP/2 stream, which is not an HTTP/1 `OutgoingMessage` and has no
/// message-level cork.
pub(crate) fn http1_chunked(conn: &RootedConnection, seq: u64) -> Option<bool> {
    if conn.h2().is_some() {
        return None;
    }
    conn.socket()
        .map(|socket| crate::server::turnloop_serve::body_is_chunked(socket, seq))
}

/// A streaming `res.write(chunk)`: accepted with Node's backpressure answer,
/// or `None` when the transport no longer owns the response.
pub(crate) fn send_body(conn: &RootedConnection, seq: u64, bytes: &[u8]) -> Option<bool> {
    if let Some(h2) = conn.h2() {
        crate::server::turnloop_h2::h2_send_body(h2, seq as u32, bytes)
    } else {
        conn.socket()
            .filter(|socket| crate::server::turnloop_serve::send_body(socket, seq, bytes))
            .map(|_| writable_below_watermark(conn, seq))
    }
}

/// A streaming `res.end()`: close the body framing and finish the stream.
pub(crate) fn finish_body(conn: &RootedConnection, seq: u64, trailers: &[(String, String)]) {
    finish_body_notified(conn, seq, trailers, None);
}

/// [`finish_body`] with the callback contract of [`send_response_notified`].
pub(crate) fn finish_body_notified(
    conn: &RootedConnection,
    seq: u64,
    trailers: &[(String, String)],
    notify: Option<&mut dyn FnMut() -> f64>,
) -> bool {
    if let Some(h2) = conn.h2() {
        crate::server::turnloop_h2::h2_finish_body(h2, seq as u32, trailers);
        false
    } else if let Some(socket) = conn.socket() {
        crate::server::turnloop_serve::finish_body_notified(socket, seq, trailers, notify)
    } else {
        false
    }
}

/// HTTP/2 also buffers bytes above the socket while waiting for peer credit.
/// An empty socket queue alone does not mean that stream can emit `drain`.
pub(crate) fn writable_below_watermark(conn: &RootedConnection, seq: u64) -> bool {
    if let Some(h2) = conn.h2() {
        crate::server::turnloop_h2::conn::peek(h2, |connection| {
            connection.streams.iter().any(|s| s.h2_id == seq as u32)
                && crate::server::turnloop_h2::stream::writable_below_watermark(
                    connection, seq as u32,
                )
        })
        .unwrap_or(false)
    } else {
        conn.socket()
            .is_some_and(|socket| net::queued_bytes(socket.value()) <= 16 * 1024)
    }
}
