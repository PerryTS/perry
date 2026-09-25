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

fn is_h2(conn: i64) -> bool {
    crate::server::turnloop_h2::owns(conn)
}

/// `res.end(body)` on a fully buffered response.
pub(crate) fn send_response(conn: i64, seq: u64, shape: ResponseShape) {
    if is_h2(conn) {
        crate::server::turnloop_h2::h2_send_response(conn, seq as u32, shape);
    } else {
        crate::server::turnloop_serve::send_response(conn, seq, shape);
    }
}

/// `res.flushHeaders()` / the first `res.write(...)`: send the head now.
pub(crate) fn begin_stream(conn: i64, seq: u64, shape: ResponseShape) -> bool {
    if is_h2(conn) {
        crate::server::turnloop_h2::h2_begin_stream(conn, seq as u32, shape)
    } else {
        crate::server::turnloop_serve::begin_stream(conn, seq, shape)
    }
}

/// A streaming `res.write(chunk)`: accepted with Node's backpressure answer,
/// or `None` when the transport no longer owns the response.
pub(crate) fn send_body(conn: i64, seq: u64, bytes: &[u8]) -> Option<bool> {
    if is_h2(conn) {
        crate::server::turnloop_h2::h2_send_body(conn, seq as u32, bytes)
    } else {
        crate::server::turnloop_serve::send_body(conn, seq, bytes)
            .then(|| writable_below_watermark(conn, seq))
    }
}

/// A streaming `res.end()`: close the body framing and finish the stream.
pub(crate) fn finish_body(conn: i64, seq: u64, trailers: &[(String, String)]) {
    if is_h2(conn) {
        crate::server::turnloop_h2::h2_finish_body(conn, seq as u32, trailers);
    } else {
        crate::server::turnloop_serve::finish_body(conn, seq, trailers);
    }
}

/// HTTP/2 also buffers bytes above the socket while waiting for peer credit.
/// An empty socket queue alone does not mean that stream can emit `drain`.
pub(crate) fn writable_below_watermark(conn: i64, seq: u64) -> bool {
    if is_h2(conn) {
        crate::server::turnloop_h2::conn::peek(conn, |connection| {
            connection.streams.iter().any(|s| s.h2_id == seq as u32)
                && crate::server::turnloop_h2::stream::writable_below_watermark(
                    connection, seq as u32,
                )
        })
        .unwrap_or(false)
    } else {
        perry_ffi::turnloop_net::queued_bytes(conn) <= 16 * 1024
    }
}
