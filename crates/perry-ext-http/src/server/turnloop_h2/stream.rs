//! Per-stream state: multiplexing, flow control, and the response path.
//!
//! # Streams and handles
//!
//! An HTTP/2 stream id is a per-connection `u32`; a Perry handle is a
//! process-wide `i64`. The mapping is one `H2Stream` record per open stream,
//! held in its connection's `streams: Vec<H2Stream>`, carrying the **real**
//! stream id — `stream.id` in JS is now RFC 9113's number rather than the
//! process-global odd counter (`NEXT_H2_STREAM_ID`) the hyper path handed out,
//! which never corresponded to anything on the wire.
//!
//! A record is created when a stream's first HEADERS is seen (server) or when
//! `session.request()` opens one (client), and dropped when the stream reaches
//! a terminal state. **A stream's lifetime is strictly inside its
//! connection's**: sibling streams are independent records and a terminal event
//! on one touches nothing else. The only shared quantities are the connection
//! flow-control window and the core's stream table, and both are returned by
//! the same call — see "capacity" below.
//!
//! # Flow control (`release_capacity` is a policy, not plumbing)
//!
//! `turnloop_http::http2` never reopens a receive window on its own: `unreleased`
//! accumulates every DATA byte and only `release_capacity` turns it back into
//! WINDOW_UPDATE frames. So the host chooses. Perry's choice:
//!
//! 1. **Release on consume.** A DATA payload is copied into the stream's own
//!    buffer inside the sink, so by the time the event returns there is no
//!    downstream consumer left to wait for and withholding window would only
//!    idle the peer. This is the eager policy, and it keeps the receive window
//!    fully open for the common case.
//! 2. **Bounded by `maxSessionMemory`.** Node's default is 10 MB of buffered
//!    session state. Once a connection holds that much *undispatched* body, the
//!    release is withheld per stream (`H2Stream::withheld`) and the peer stalls
//!    — which is what a flow-control window is for. The withheld amounts are
//!    released as soon as a request is handed to the pump and its bytes leave
//!    this module. Node signals the same condition by destroying the session
//!    with `ENHANCE_YOUR_CALM`; stalling first is strictly gentler and is what
//!    the window exists to express.
//! 3. **A terminated stream still releases.** This is not optional and it is
//!    easy to miss: `Connection::add_stream` reuses a closed stream's slot only
//!    when its `unreleased` is zero, and `reset` does **not** zero it. A stream
//!    that is reset with DATA in flight and never released therefore holds its
//!    table slot **and** its share of the connection window forever, and the
//!    connection answers `REFUSED_STREAM` to new streams long before its peer's
//!    `MAX_CONCURRENT_STREAMS` would. [`terminate`] releases the remainder.
//!
//! The write side is the mirror image. `send_data` returns the number of bytes
//! it accepted and **zero** when the peer's window is shut; the remainder stays
//! in `H2Stream::outbox` and is retried from `Event::WindowUpdate`. That is
//! what `res.write()` returning `false` and the later `'drain'` mean in Node,
//! and [`writable_below_watermark`] is what answers the boolean.

use std::collections::HashMap;

use perry_ffi::turnloop_net as tl;
use turnloop_http::http1::Header;
use turnloop_http::http2::{HeadersKind, Role};

use super::conn::{flush, H2Conn};
use crate::server::response::ResponseShape;

/// Node's `http2.constants.NGHTTP2_*` error codes used here.
const INTERNAL_ERROR: u32 = 2;
const REFUSED_STREAM: u32 = 7;

/// `res.write()`'s boolean, and Node's default stream high-water mark.
const HIGH_WATER_MARK: usize = 16 * 1024;

/// Headers a peer must never see on an HTTP/2 stream. `validate_headers`
/// rejects every one of them as a **connection** error, so a handler that sets
/// `Connection: keep-alive` would otherwise take the whole session down.
const FORBIDDEN: [&str; 5] = [
    "connection",
    "proxy-connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
];

/// One open HTTP/2 stream.
pub(crate) struct H2Stream {
    /// RFC 9113's stream identifier, and what JS sees as `stream.id`.
    pub(crate) h2_id: u32,
    /// The `Http2StreamHandle`, or zero when JS has no object for this stream.
    pub(crate) handle: i64,
    /// Server side: the `IncomingMessage` / `ServerResponse` pair.
    pub(crate) request_handle: i64,
    pub(crate) response_handle: i64,
    /// Request (server) or response (client) headers, lowercase-keyed.
    pub(crate) headers: HashMap<String, String>,
    pub(crate) raw_headers: Vec<(String, String)>,
    /// Inbound body, buffered until the message is complete.
    pub(crate) body: Vec<u8>,
    /// Trailers received after the body.
    pub(crate) trailers: Vec<(String, String)>,
    /// Outbound body still waiting for peer window.
    pub(crate) outbox: Vec<u8>,
    /// Once `outbox` drains, END_STREAM goes with the last frame.
    pub(crate) outbox_end: bool,
    /// Trailers to send instead of END_STREAM on the last DATA frame.
    pub(crate) send_trailers: Vec<(String, String)>,
    pub(crate) head_received: bool,
    pub(crate) headers_sent: bool,
    pub(crate) remote_end: bool,
    pub(crate) local_end: bool,
    /// Dispatched to the JS pump already (server), or `'response'` emitted
    /// (client).
    pub(crate) dispatched: bool,
    /// The response may carry no body: a HEAD request, or 204/304.
    pub(crate) no_body: bool,
    /// Received-but-unreleased bytes, mirroring the core's own counter so the
    /// remainder can be returned when the stream terminates.
    pub(crate) unreleased: u32,
    /// Part of `unreleased` deliberately held back by the memory bound.
    pub(crate) withheld: u32,
}

impl H2Stream {
    fn new(h2_id: u32) -> Self {
        Self {
            h2_id,
            handle: 0,
            request_handle: 0,
            response_handle: 0,
            headers: HashMap::new(),
            raw_headers: Vec::new(),
            body: Vec::new(),
            trailers: Vec::new(),
            outbox: Vec::new(),
            outbox_end: false,
            send_trailers: Vec::new(),
            head_received: false,
            headers_sent: false,
            remote_end: false,
            local_end: false,
            dispatched: false,
            no_body: false,
            unreleased: 0,
            withheld: 0,
        }
    }
}

/// A `session.request()` issued before the transport was ready.
///
/// The body travels with it: a `stream.end(body)` that races the TCP connect
/// must produce HEADERS and DATA in that order on one stream, and holding the
/// bytes here is the only way to guarantee it — the stream does not exist yet,
/// so there is nowhere else to put them.
pub(crate) struct QueuedOpen {
    pub(crate) stream_handle: i64,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

fn index_of(conn: &H2Conn, h2_id: u32) -> Option<usize> {
    conn.streams.iter().position(|s| s.h2_id == h2_id)
}

// ── Inbound ─────────────────────────────────────────────────────────────────

pub(crate) fn on_peer_settings(conn: &mut H2Conn) {
    let session = conn.session_handle;
    if session == 0 {
        return;
    }
    // `Event::Settings` is a unit variant, so the values came from the
    // pre-scan in `conn.rs` rather than from the core.
    let settings = conn.peer_settings.take().unwrap_or_default();
    crate::server::http2_server::queue_turnloop_remote_settings(session, settings);
}

/// `kind` comes from the protocol core, which enforces the distinction between
/// the three header blocks. This used to be re-derived here — 1xx by sniffing
/// `:status` for a leading `1`, trailers from a `head_received` flag this
/// module kept itself — and both were duplicated state that could drift from
/// the connection's. turnloop-http 0.1.0-alpha.6 reports it directly, so the
/// core is now the single source of truth for which block this is.
pub(crate) fn on_headers(
    conn: &mut H2Conn,
    h2_id: u32,
    headers: Vec<Header>,
    end_stream: bool,
    kind: HeadersKind,
) {
    let i = match index_of(conn, h2_id) {
        Some(i) => i,
        None => {
            conn.streams.push(H2Stream::new(h2_id));
            conn.streams.len() - 1
        }
    };
    if kind == HeadersKind::Informational {
        // A 1xx does not open the message; Node surfaces it separately and
        // Perry has no surface for it yet, so it is dropped rather than
        // mistaken for the real head.
        return;
    }
    if kind == HeadersKind::Trailers {
        // The core has already enforced that a trailer block carries
        // END_STREAM and follows a head.
        for h in &headers {
            conn.streams[i].trailers.push((
                h.name.clone(),
                String::from_utf8_lossy(&h.value).into_owned(),
            ));
        }
    } else {
        for h in &headers {
            let value = String::from_utf8_lossy(&h.value).into_owned();
            conn.streams[i]
                .headers
                .insert(h.name.to_ascii_lowercase(), value.clone());
            if !h.name.starts_with(':') {
                conn.streams[i].raw_headers.push((h.name.clone(), value));
            }
        }
        conn.streams[i].head_received = true;
        if conn.role == Role::Client {
            complete_client_head(conn, i);
        }
    }
    if end_stream {
        conn.streams[i].remote_end = true;
        complete_inbound(conn, i);
    }
}

pub(crate) fn on_data(conn: &mut H2Conn, h2_id: u32, bytes: Vec<u8>, end_stream: bool) {
    let Some(i) = index_of(conn, h2_id) else {
        // DATA for a stream we already retired. The core still charged its
        // window, so the capacity has to go back even though nothing will read
        // the bytes.
        release_orphan(conn, h2_id, bytes.len() as u32);
        return;
    };
    let len = bytes.len() as u32;
    conn.streams[i].unreleased += len;
    conn.buffered += bytes.len();
    conn.streams[i].body.extend_from_slice(&bytes);
    grant_window(conn, i);
    if end_stream {
        conn.streams[i].remote_end = true;
        complete_inbound(conn, i);
    }
}

/// Policy point: reopen this stream's window unless the connection is already
/// holding more undispatched body than `maxSessionMemory` allows.
fn grant_window(conn: &mut H2Conn, i: usize) {
    let over_budget = conn.buffered > conn.max_session_memory;
    let stream = &mut conn.streams[i];
    if over_budget {
        stream.withheld = stream.unreleased;
        return;
    }
    let n = stream.unreleased;
    if n == 0 {
        return;
    }
    stream.unreleased = 0;
    stream.withheld = 0;
    let h2_id = stream.h2_id;
    if let Some(core) = conn.core.as_mut() {
        if core.release_capacity(h2_id, n).is_err() {
            // The core no longer knows this stream; the window it charged is
            // gone with it and there is nothing to return.
        }
    }
}

/// Return the window of a stream whose record we no longer hold.
fn release_orphan(conn: &mut H2Conn, h2_id: u32, n: u32) {
    if n == 0 {
        return;
    }
    if let Some(core) = conn.core.as_mut() {
        let _ = core.release_capacity(h2_id, n);
    }
}

/// Once the memory bound clears, hand back everything that was withheld.
fn release_withheld(conn: &mut H2Conn) {
    if conn.buffered > conn.max_session_memory {
        return;
    }
    let pending: Vec<(u32, u32)> = conn
        .streams
        .iter()
        .filter(|s| s.unreleased > 0)
        .map(|s| (s.h2_id, s.unreleased))
        .collect();
    for (h2_id, n) in pending {
        if let Some(i) = index_of(conn, h2_id) {
            conn.streams[i].unreleased = 0;
            conn.streams[i].withheld = 0;
        }
        if let Some(core) = conn.core.as_mut() {
            let _ = core.release_capacity(h2_id, n);
        }
    }
}

/// The inbound message is complete: hand it to JS (through the pump, never
/// from here) and free the bytes it was holding.
fn complete_inbound(conn: &mut H2Conn, i: usize) {
    if conn.streams[i].dispatched {
        // Trailers arriving after the body was already dispatched.
        return;
    }
    conn.streams[i].dispatched = true;
    let held = conn.streams[i].body.len();
    match conn.role {
        Role::Server => dispatch_request(conn, i),
        Role::Client => complete_client_body(conn, i),
    }
    conn.buffered = conn.buffered.saturating_sub(held);
    release_withheld(conn);
}

/// Build the `(req, res)` pair and queue it for `js_node_http_server_process_pending`.
fn dispatch_request(conn: &mut H2Conn, i: usize) {
    let server_handle = conn.server_handle;
    let peer_address = conn.peer_address.clone();
    let peer_port = conn.peer_port;
    let conn_id = conn.id;
    let session_handle = conn.session_handle;
    let stream = &mut conn.streams[i];
    let h2_id = stream.h2_id;

    let method = stream
        .headers
        .get(":method")
        .cloned()
        .unwrap_or_else(|| "GET".to_string());
    let url = stream
        .headers
        .get(":path")
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let body = std::mem::take(&mut stream.body);
    stream.no_body = method.eq_ignore_ascii_case("HEAD");

    let mut im = crate::server::request::IncomingMessage::new(
        method,
        url,
        stream.headers.clone(),
        stream.raw_headers.clone(),
        body,
        peer_address,
        peer_port,
    );
    im.http_version = "2.0".to_string();
    let request_handle = crate::server::request::alloc_incoming_message(im);
    let response_handle = crate::server::response::alloc_server_response_for_turnloop(
        conn_id,
        h2_id as u64,
        request_handle,
    );
    let headers_vec: Vec<(String, String)> = stream
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let has_stream_listener =
        crate::server::http2_server::server_has_stream_listener(server_handle);
    let stream_handle = if has_stream_listener {
        crate::server::http2_server::register_turnloop_stream_handle(
            session_handle,
            conn_id,
            h2_id as i64,
            headers_vec.clone(),
        )
    } else {
        0
    };
    stream.handle = stream_handle;
    stream.request_handle = request_handle;
    stream.response_handle = response_handle;

    super::queue_pending(
        server_handle,
        crate::server::server::HttpPendingRequest {
            server_handle,
            request_handle,
            response_handle,
            // A `'stream'` listener answers the request itself, exactly as it
            // does on the hyper path; synthesizing a default response here as
            // well would put two responses on one stream.
            skip_default_response: has_stream_listener,
            h2_stream_handle: stream_handle,
            h2_stream_headers: headers_vec,
            is_check_continue: false,
        },
    );
}

// ── Client inbound ──────────────────────────────────────────────────────────

fn complete_client_head(conn: &mut H2Conn, i: usize) {
    let stream = &conn.streams[i];
    if stream.handle == 0 {
        return;
    }
    crate::server::http2_server::queue_turnloop_client_response(
        stream.handle,
        stream.headers.clone(),
    );
}

fn complete_client_body(conn: &mut H2Conn, i: usize) {
    let stream = &mut conn.streams[i];
    let handle = stream.handle;
    if handle == 0 {
        return;
    }
    let body = std::mem::take(&mut stream.body);
    let trailers = std::mem::take(&mut stream.trailers);
    crate::server::http2_server::queue_turnloop_client_body(handle, body, trailers);
    // The response is complete; retire the record so its core slot is reused.
    let h2_id = stream.h2_id;
    retire(conn, h2_id);
}

// ── Terminal events ─────────────────────────────────────────────────────────

/// A peer RST_STREAM. This is the sibling-isolation point: exactly one stream
/// ends, the connection and every other stream keep going.
pub(crate) fn on_reset(conn: &mut H2Conn, h2_id: u32, code: u32) {
    let Some(i) = index_of(conn, h2_id) else {
        return;
    };
    let handle = conn.streams[i].handle;
    let request_handle = conn.streams[i].request_handle;
    if handle != 0 {
        crate::server::http2_server::queue_turnloop_stream_reset(handle, code);
    }
    if request_handle != 0 {
        crate::server::server::note_turnloop_request_aborted(request_handle);
    }
    terminate(conn, h2_id, None);
}

pub(crate) fn on_goaway(conn: &mut H2Conn, last_stream: u32, code: u32, opaque: Vec<u8>) {
    let session = conn.session_handle;
    if session != 0 {
        crate::server::http2_server::queue_turnloop_goaway(session, code, last_stream, opaque);
    }
    // Node lets streams at or below `lastStreamID` finish and fails the rest.
    let doomed: Vec<u32> = conn
        .streams
        .iter()
        .filter(|s| s.h2_id > last_stream)
        .map(|s| s.h2_id)
        .collect();
    for h2_id in doomed {
        if let Some(i) = index_of(conn, h2_id) {
            let handle = conn.streams[i].handle;
            if handle != 0 {
                crate::server::http2_server::queue_turnloop_stream_reset(handle, REFUSED_STREAM);
            }
        }
        terminate(conn, h2_id, None);
    }
}

pub(crate) fn on_ping(conn: &mut H2Conn, ack: bool, data: [u8; 8]) {
    if !ack {
        // The core already queued the echo.
        return;
    }
    let session = conn.session_handle;
    if session != 0 {
        crate::server::http2_server::complete_turnloop_ping(session, data);
    }
}

/// Retire a stream, returning its capacity first.
///
/// Releasing before dropping the record is what keeps the core's stream table
/// from filling with closed-but-uncollectable slots (see the module docs).
pub(crate) fn terminate(conn: &mut H2Conn, h2_id: u32, error: Option<&str>) {
    let Some(i) = index_of(conn, h2_id) else {
        return;
    };
    let handle = conn.streams[i].handle;
    let request_handle = conn.streams[i].request_handle;
    let held = conn.streams[i].body.len();
    conn.buffered = conn.buffered.saturating_sub(held);
    if let Some(message) = error {
        if handle != 0 {
            crate::server::http2_server::queue_turnloop_stream_error(handle, message);
        }
        if request_handle != 0 {
            crate::server::server::note_turnloop_request_aborted(request_handle);
        }
    }
    retire(conn, h2_id);
    release_withheld(conn);
}

/// Drop the record and hand its window back.
fn retire(conn: &mut H2Conn, h2_id: u32) {
    let Some(i) = index_of(conn, h2_id) else {
        return;
    };
    let outstanding = conn.streams[i].unreleased;
    let handle = conn.streams[i].handle;
    conn.streams.remove(i);
    if outstanding > 0 {
        if let Some(core) = conn.core.as_mut() {
            let _ = core.release_capacity(h2_id, outstanding);
        }
    }
    if handle != 0 {
        crate::server::http2_server::mark_turnloop_stream_closed(handle);
    }
}

// ── Outbound: the response path ─────────────────────────────────────────────

/// Translate a `ResponseShape`'s header block into HTTP/2 form.
///
/// Two things are load-bearing. The `:status` pseudo-header must come first and
/// every name must be lowercase, or `validate_headers` rejects the block. And
/// the connection-specific headers in [`FORBIDDEN`] must be dropped: a handler
/// that sets `Connection: close` on an HTTP/2 response is legal Node and would
/// otherwise take the session down with a PROTOCOL_ERROR.
pub(crate) fn response_headers(
    status: u16,
    headers: &[(String, String)],
    body_len: Option<usize>,
) -> Vec<Header> {
    let mut out = Vec::with_capacity(headers.len() + 2);
    out.push(Header::new(":status", status.to_string()));
    let mut seen_length = false;
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if lower.starts_with(':') || FORBIDDEN.contains(&lower.as_str()) {
            continue;
        }
        if lower == "content-length" {
            seen_length = true;
        }
        out.push(Header::new(&lower, value.clone()));
    }
    if !seen_length {
        if let Some(len) = body_len {
            out.push(Header::new("content-length", len.to_string()));
        }
    }
    out
}

/// Whether a response of this status, on this request, may carry a body.
pub(crate) fn body_forbidden(status: u16, head_request: bool) -> bool {
    head_request || status == 204 || status == 304 || (100..200).contains(&status)
}

/// `res.end(body)` on a fully buffered response.
pub(crate) fn h2_send_response(conn_id: i64, h2_id: u32, shape: ResponseShape) {
    super::conn::with_owned(conn_id, |conn| {
        let Some(i) = index_of(conn, h2_id) else {
            return;
        };
        let head_request = conn.streams[i].no_body;
        let body = shape.body;
        let forbidden = body_forbidden(shape.status, head_request);
        // A HEAD response advertises the length it *would* have sent; the core
        // suppresses the body itself once it knows the request was a HEAD.
        let advertised = if forbidden && !head_request {
            None
        } else {
            Some(body.len())
        };
        let headers = response_headers(shape.status, &shape.headers, advertised);
        let end_now = (body.is_empty() || forbidden) && shape.trailers.is_empty();
        if !send_head(conn, i, &headers, end_now) {
            // `send_head` reset the stream; the RST_STREAM is in the core's
            // output and still has to reach the peer.
            flush(conn);
            return;
        }
        if forbidden {
            conn.streams[i].local_end = true;
            finish_stream(conn, h2_id);
            // The HEADERS frame is the whole response. Without this the frame
            // sits in `core.output()` and a 204 or a HEAD never answers.
            flush(conn);
            return;
        }
        let stream = &mut conn.streams[i];
        stream.outbox = body;
        stream.outbox_end = true;
        stream.send_trailers = shape.trailers;
        pump_outbox(conn);
        flush(conn);
    });
}

/// `res.flushHeaders()` / the first `res.write(...)`: send the head now.
pub(crate) fn h2_begin_stream(conn_id: i64, h2_id: u32, shape: ResponseShape) -> bool {
    super::conn::with_owned(conn_id, |conn| {
        let Some(i) = index_of(conn, h2_id) else {
            return false;
        };
        let head_request = conn.streams[i].no_body;
        let forbidden = body_forbidden(shape.status, head_request);
        // A streaming response has no known length unless the handler set one.
        let headers = response_headers(shape.status, &shape.headers, None);
        if !send_head(conn, i, &headers, forbidden) {
            flush(conn);
            return false;
        }
        if forbidden {
            conn.streams[i].local_end = true;
            finish_stream(conn, h2_id);
        }
        flush(conn);
        true
    })
    .unwrap_or(false)
}

/// A streaming `res.write(chunk)`. The boolean is Node's backpressure answer.
pub(crate) fn h2_send_body(conn_id: i64, h2_id: u32, bytes: &[u8]) -> bool {
    super::conn::with_owned(conn_id, |conn| {
        let Some(i) = index_of(conn, h2_id) else {
            return false;
        };
        if conn.streams[i].no_body {
            return true;
        }
        conn.streams[i].outbox.extend_from_slice(bytes);
        pump_outbox(conn);
        flush(conn);
        writable_below_watermark(conn, h2_id)
    })
    .unwrap_or(false)
}

/// A streaming `res.end()`: close the body framing and finish the stream.
pub(crate) fn h2_finish_body(conn_id: i64, h2_id: u32, trailers: &[(String, String)]) {
    super::conn::with_owned(conn_id, |conn| {
        let Some(i) = index_of(conn, h2_id) else {
            return;
        };
        conn.streams[i].outbox_end = true;
        conn.streams[i].send_trailers = trailers.to_vec();
        pump_outbox(conn);
        flush(conn);
    });
}

/// `res.destroy()` / `stream.close(code)` — one stream, not the connection.
pub(crate) fn destroy_stream(conn_id: i64, h2_id: u32, code: u32) {
    super::conn::with_owned(conn_id, |conn| {
        if index_of(conn, h2_id).is_none() {
            return;
        }
        if let Some(core) = conn.core.as_mut() {
            let _ = core.reset(h2_id, code);
        }
        terminate(conn, h2_id, None);
        flush(conn);
    });
    maybe_drain(conn_id);
}

fn send_head(conn: &mut H2Conn, i: usize, headers: &[Header], end_stream: bool) -> bool {
    if conn.streams[i].headers_sent {
        return true;
    }
    let h2_id = conn.streams[i].h2_id;
    let sent = match conn.core.as_mut() {
        Some(core) => core.send_headers(h2_id, headers, end_stream),
        None => return false,
    };
    match sent {
        Ok(()) => {
            conn.streams[i].headers_sent = true;
            conn.streams[i].local_end = end_stream;
            true
        }
        Err(_) => {
            // A malformed header block is this stream's problem, not the
            // connection's: reset it and leave the siblings running.
            if let Some(core) = conn.core.as_mut() {
                let _ = core.reset(h2_id, INTERNAL_ERROR);
            }
            terminate(conn, h2_id, Some("ERR_HTTP2_INVALID_HEADERS"));
            false
        }
    }
}

/// Push as much of every stream's outbox as the peer's windows allow.
///
/// `send_data` accepts at most one frame's worth and returns zero on a stall,
/// so this loops per stream until it stops making progress — a single call per
/// event would move one 16 KiB frame per round trip.
pub(crate) fn pump_outbox(conn: &mut H2Conn) {
    let mut finished: Vec<u32> = Vec::new();
    let ids: Vec<u32> = conn.streams.iter().map(|s| s.h2_id).collect();
    for h2_id in ids {
        let Some(i) = index_of(conn, h2_id) else {
            continue;
        };
        if !conn.streams[i].headers_sent || conn.streams[i].local_end {
            continue;
        }
        loop {
            let (offset, end, remaining) = {
                let s = &conn.streams[i];
                (0usize, s.outbox_end, s.outbox.len())
            };
            let _ = offset;
            if remaining == 0 && !end {
                break;
            }
            // Trailers replace END_STREAM on the body's last frame.
            let end_with_data = end && conn.streams[i].send_trailers.is_empty();
            let accepted = {
                let H2Conn { core, streams, .. } = &mut *conn;
                let Some(core) = core.as_mut() else { break };
                match core.send_data(h2_id, &streams[i].outbox, end_with_data) {
                    Ok(n) => n,
                    Err(_) => {
                        finished.push(h2_id);
                        break;
                    }
                }
            };
            if accepted > 0 {
                conn.streams[i].outbox.drain(..accepted);
            }
            let drained = conn.streams[i].outbox.is_empty();
            if drained && end {
                if end_with_data {
                    conn.streams[i].local_end = true;
                } else {
                    let trailers: Vec<Header> = conn.streams[i]
                        .send_trailers
                        .iter()
                        .map(|(k, v)| Header::new(&k.to_ascii_lowercase(), v.clone()))
                        .collect();
                    if let Some(core) = conn.core.as_mut() {
                        let _ = core.send_headers(h2_id, &trailers, true);
                    }
                    conn.streams[i].local_end = true;
                }
                finished.push(h2_id);
                break;
            }
            if accepted == 0 {
                // Flow-control stall: the rest waits for a WINDOW_UPDATE.
                break;
            }
        }
    }
    for h2_id in finished {
        finish_stream(conn, h2_id);
    }
}

/// The response is fully written. If the peer has also finished, retire the
/// record; otherwise leave it so inbound DATA still has somewhere to land.
fn finish_stream(conn: &mut H2Conn, h2_id: u32) {
    let Some(i) = index_of(conn, h2_id) else {
        return;
    };
    if !conn.streams[i].local_end {
        return;
    }
    // Only when the peer has finished too. Retiring a half-closed(local)
    // stream would drop the record its inbound DATA has to land on, and the
    // bytes would be released as an orphan and thrown away.
    if conn.streams[i].remote_end {
        retire(conn, h2_id);
    }
}

/// `res.write()`'s boolean and `res.writableNeedDrain`.
///
/// Two quantities matter and both are real backpressure: bytes this stream is
/// holding because the peer's window is shut, and bytes the transport has
/// queued but not yet sent.
pub(crate) fn writable_below_watermark(conn: &H2Conn, h2_id: u32) -> bool {
    let stalled = index_of(conn, h2_id)
        .map(|i| conn.streams[i].outbox.len())
        .unwrap_or(0);
    stalled + tl::queued_bytes(conn.id) <= HIGH_WATER_MARK
}

/// After a stream ends, a connection that was asked to close may now be drained.
fn maybe_drain(conn_id: i64) {
    let drained = super::conn::peek(conn_id, |conn| {
        conn.core.as_ref().is_some_and(|core| core.is_drained())
    });
    if drained == Some(true) {
        super::conn::graceful_close(conn_id);
    }
}

// ── Outbound: the client request path ───────────────────────────────────────

/// Open a client stream now and send its HEADERS.
pub(crate) fn open_client_stream(conn: &mut H2Conn, open: QueuedOpen) {
    let headers: Vec<Header> = open
        .headers
        .iter()
        .map(|(k, v)| Header::new(k, v.clone()))
        .collect();
    let end_stream = open.body.is_empty();
    let opened = match conn.core.as_mut() {
        Some(core) => core.open(&headers, end_stream),
        None => {
            crate::server::http2_server::queue_turnloop_stream_error(
                open.stream_handle,
                "ERR_HTTP2_INVALID_SESSION",
            );
            return;
        }
    };
    match opened {
        Ok(h2_id) => {
            let mut stream = H2Stream::new(h2_id);
            stream.handle = open.stream_handle;
            stream.headers_sent = true;
            stream.local_end = end_stream;
            stream.no_body = open
                .headers
                .iter()
                .any(|(k, v)| k == ":method" && v.eq_ignore_ascii_case("HEAD"));
            if !end_stream {
                stream.outbox = open.body;
                stream.outbox_end = true;
            }
            conn.streams.push(stream);
            crate::server::http2_server::bind_turnloop_stream_id(
                open.stream_handle,
                conn.id,
                h2_id as i64,
            );
            if !end_stream {
                pump_outbox(conn);
            }
        }
        Err(err) => {
            let code = if err.code == "REFUSED_STREAM" {
                "ERR_HTTP2_STREAM_ERROR"
            } else {
                "ERR_HTTP2_INVALID_HEADERS"
            };
            crate::server::http2_server::queue_turnloop_stream_error(open.stream_handle, code);
        }
    }
}

/// `session.request(headers)` from JS. Opens immediately when the transport is
/// ready — which is what Node does — and queues otherwise.
pub(crate) fn request(
    conn_id: i64,
    stream_handle: i64,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    super::conn::with_owned(conn_id, |conn| {
        let open = QueuedOpen {
            stream_handle,
            headers,
            body,
        };
        if conn.core.is_some() && !conn.connecting && !conn.handshaking {
            open_client_stream(conn, open);
            flush(conn);
        } else {
            // Before `NET_CONNECT` — Node lets `session.request()` be called
            // on a session that is still connecting and opens the stream when
            // the transport comes up.
            conn.queued_opens.push(open);
        }
    });
}

/// `session.close()` — Node's graceful GOAWAY.
pub(crate) fn session_close(conn_id: i64) {
    let drained = super::conn::with_owned(conn_id, |conn| {
        if !super::conn::transport_ready(conn) {
            // A `close()` on a session that is still connecting: the GOAWAY
            // goes out with the rest of the queued control frames.
            conn.pending_controls
                .push(super::conn::PendingControl::Close);
            return false;
        }
        if let Some(core) = conn.core.as_mut() {
            let _ = core.shutdown();
        }
        conn.draining = true;
        flush(conn);
        conn.core.as_ref().is_some_and(|core| core.is_drained())
    });
    if drained == Some(true) {
        super::conn::graceful_close(conn_id);
    }
}
