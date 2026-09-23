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

/// A streaming `res.write(chunk)`. The boolean is Node's backpressure answer.
pub(crate) fn send_body(conn: i64, seq: u64, bytes: &[u8]) -> bool {
    if is_h2(conn) {
        crate::server::turnloop_h2::h2_send_body(conn, seq as u32, bytes)
    } else {
        crate::server::turnloop_serve::send_body(conn, seq, bytes)
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
