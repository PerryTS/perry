//! HTTP/2 protocol framing over the agent's own loop.
//!
//! Server connections are ordinary net.Socket objects from accept onward.
//! The Socket's ordinary `h2` JS edge owns a separate pure Rust codec payload;
//! the logical HTTP/2 session retains its public Socket. An ALPN handoff to
//! HTTP/1 replaces the codec and stores the new route on the existing core,
//! preserving its cell, driver handle and pending read.
//!
//! HTTP/2 clients use only their own ids and stay in phase C. Their existing
//! transport table and id sink use slot 18; server codecs use link slot 15.
//! Protocol output is copied out of a server codec before any transport call
//! or JS callback. Decoded requests and session events use the existing pumps.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;

pub(crate) mod conn;
pub(crate) mod control;
mod native_server;
pub(crate) mod stream;
pub(crate) mod target;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use conn::{connect_client, ClientTls};
pub(crate) use native_server::{close_listener, listen};
pub(crate) use stream::{
    destroy_stream, h2_begin_stream, h2_finish_body, h2_send_body, h2_send_response,
};

/// The phase-C client completion slot; server codecs have a link sink.
/// It is id-routed, so it may sit above the 16 link-route slots, which are
/// all claimed (the HTTP/2 client used to share the HTTP server's slot 1,
/// which is now a link sink).
pub(crate) const SUBSYSTEM: u8 = 16;

/// Whether HTTP/2 on turnloop is available to a server or session created
/// *now, on this thread*.
///
/// Deliberately not cached, for P5's reason: availability is a property of the
/// calling agent, and caching a loop-less agent's "no" would strand the others.
pub(crate) fn enabled() -> bool {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        extern "C" fn sink(event: *const tl::NetCompletion) {
            if !event.is_null() {
                conn::intercept(unsafe { &*event });
            }
        }
        extern "C" fn alloc() -> i64 {
            next_id()
        }
        assert!(
            tl::register_sink(SUBSYSTEM, sink, alloc),
            "HTTP2 client sink registration refused"
        );
    });
    perry_ext_net::native_transport::enabled() && tl::available(SUBSYSTEM)
}

/// Ids come from P5's domain: the runtime keys its `Entry` map by this id
/// across every subsystem, so the two must not collide, and sharing the sink
/// slot means sharing the numeric domain is the simplest way to guarantee it.
pub(crate) fn next_id() -> i64 {
    super::turnloop_serve::next_id()
}

// ── The request queue ───────────────────────────────────────────────────────

/// Requests decoded and waiting for the main-thread pump, per JS server handle.
///
/// The same `HttpPendingRequest` P5 queues, and drained by the same pump: the
/// struct already carries `h2_stream_handle` / `h2_stream_headers`, because the
/// hyper HTTP/2 path used them for the `'stream'` event.
fn pending() -> &'static Mutex<HashMap<i64, VecDeque<crate::server::server::HttpPendingRequest>>> {
    static PENDING: OnceLock<
        Mutex<HashMap<i64, VecDeque<crate::server::server::HttpPendingRequest>>>,
    > = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn take_pending(
    server_handle: i64,
) -> Option<crate::server::server::HttpPendingRequest> {
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(&server_handle)
        .and_then(|q| q.pop_front())
}

pub(crate) fn queue_pending(
    server_handle: i64,
    request: crate::server::server::HttpPendingRequest,
) {
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(server_handle)
        .or_default()
        .push_back(request);
}

/// Whether any turnloop HTTP/2 work is outstanding, so the pump keeps the
/// process alive while a session is live or a request is queued.
pub(crate) fn has_pending() -> bool {
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .any(|q| !q.is_empty())
}
