//! turnloop HTTP/2: `node:http2` servers and `http2.connect` on turnloop
//! handles, driving `turnloop_http::http2::Connection` sans-I/O.
//!
//! # What this replaces
//!
//! | hyper / `h2` / tokio | turnloop |
//! |---|---|
//! | `hyper_util::server::conn::auto::Builder` doing ALPN and HTTP/2 framing | `turnloop_http::http2::Connection` (`Role::Server`) over one multishot read |
//! | a `tokio::spawn` accept loop per `http2.createSecureServer` | one multishot `accept_start` |
//! | `h2::client::handshake` on a **private `current_thread` runtime per session** | `turnloop_http::http2::Connection` (`Role::Client`) on the agent's own loop |
//! | a **second** private runtime per `session.request()` | `Connection::open`, on the loop, multiplexed |
//! | `tokio_rustls::TlsAcceptor` | `perry_ext_net::turnloop_tls_io`'s unbuffered session |
//! | an `mpsc` + `oneshot` pair per request | a queue on this thread, because the codec already runs on it |
//!
//! The three CLIENT rows are now *deletions*, not bypasses: `perry-ext-http`
//! has no `h2` manifest edge left, and there is no second HTTP/2 client behind
//! this one. An agent that cannot reach a loop gets an `'error'` from
//! `http2.connect` (`http2_server::session::decline_client_session`), the
//! `perry-ext-ws` rule. The SERVER rows are deletions too: a thread that does
//! not own its agent's loop posts `listen()` to the owner
//! (`turnloop_serve::post_to_owner`), and there is no hyper accept path left.
//!
//! # Why sans-I/O
//!
//! Identical to P5's reason, and it applies to `turnloop_http::asynchronous`'s
//! HTTP/2 driver too: `LocalExecutor::with_config` constructs its **own**
//! `Driver`, and `Shared::dispatch` returns early for any token without its own
//! tag bit, so P1's net, P2's process, P3's timer and P4's pool completions
//! would be **silently dropped** (PerryTS/turnloop#45). Perry already owns one
//! `turnloop::Loop` per agent. The protocol core is the part that replaces
//! hyper and `h2`, and it has no such coupling.
//!
//! # The subsystem slot
//!
//! This module does **not** take a slot of its own. `perry-ext-http` is one
//! linked staticlib with one sink per subsystem, and slot 1 is already its own
//! ([`super::turnloop_serve::SUBSYSTEM`]). Sharing it buys the thing a separate
//! slot would have cost work to get back: an ALPN negotiation that lands on
//! `http/1.1` hands the connection to P5 by **moving one table entry**, with no
//! `turnloop_net::transfer` and no window in which a completion could be
//! misrouted. [`intercept`] is called first from P5's sink and answers "mine"
//! by id.
//!
//! # Ordering, and why JS never runs inside a turn
//!
//! P5's rule, unchanged: the sink runs inside `dispatch_staged`, after a turn
//! has returned. It may allocate Rust state and register handles, but it must
//! **not** call JS. A decoded request is queued and the existing pump
//! (`js_node_http_server_process_pending`) dispatches it on its own tick,
//! exactly where hyper's `mpsc` delivered it; a client-side response, body
//! chunk or session event is queued as an `Http2PendingEvent` and fired by
//! `process_pending_h2_events`, exactly where the `h2` task's `push_h2_event`
//! put it.
//!
//! # GC
//!
//! A connection holds request/response bytes as owned `Vec<u8>`s and the
//! *handle ids* of the JS objects it produced. No JS value and no heap pointer
//! reaches the driver, so this module registers no root scanner of its own: the
//! `IncomingMessage` / `ServerResponse` handles are scanned by
//! `scan_http_server_roots` and the queued event callbacks by
//! `scan_h2_pending_event_roots`, both of which already exist.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;

pub(crate) mod conn;
pub(crate) mod control;
pub(crate) mod stream;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use conn::{connect_client, intercept, intercept_listener_error, owns, ClientTls};
pub(crate) use stream::{
    destroy_stream, h2_begin_stream, h2_finish_body, h2_send_body, h2_send_response,
};

/// The completion-sink slot. Shared with P5 on purpose — see the module docs.
pub(crate) const SUBSYSTEM: u8 = super::turnloop_serve::SUBSYSTEM;

/// Whether HTTP/2 on turnloop is available to a server or session created
/// *now, on this thread*.
///
/// Deliberately not cached, for P5's reason: availability is a property of the
/// calling agent, and caching a loop-less agent's "no" would strand the others.
pub(crate) fn enabled() -> bool {
    super::turnloop_serve::enabled()
}

/// Ids come from P5's domain: the runtime keys its `Entry` map by this id
/// across every subsystem, so the two must not collide, and sharing the sink
/// slot means sharing the numeric domain is the simplest way to guarantee it.
pub(crate) fn next_id() -> i64 {
    super::turnloop_serve::next_id()
}

/// A bound turnloop listener serving HTTP/2, and the JS server it belongs to.
pub(crate) struct Listener {
    pub(crate) server_handle: i64,
    /// `Some` for `http2.createSecureServer`; `None` for `createServer` (h2c,
    /// prior knowledge). The config's `alpn_protocols` decide what a secure
    /// connection may negotiate.
    pub(crate) tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    /// Node's `allowHTTP1`. With ALPN `http/1.1`, or a cleartext connection
    /// that does not start with the HTTP/2 preface, a false value destroys the
    /// connection and a true value hands it to P5's HTTP/1.1 server.
    pub(crate) allow_http1: bool,
    /// `options.settings` as the server's own SETTINGS, plus the two limits the
    /// core derives from them.
    pub(crate) settings: crate::server::http2_session_settings::Http2SettingsState,
    /// Node's `maxSessionMemory`, in bytes (the option is in MB). The receive
    /// window is not reopened past this much buffered, undispatched body.
    pub(crate) max_session_memory: usize,
}

fn listeners() -> &'static Mutex<HashMap<i64, Listener>> {
    static LISTENERS: OnceLock<Mutex<HashMap<i64, Listener>>> = OnceLock::new();
    LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn with_listener<R>(id: i64, f: impl FnOnce(&Listener) -> R) -> Option<R> {
    let map = listeners().lock().unwrap_or_else(|e| e.into_inner());
    map.get(&id).map(f)
}

pub(crate) fn is_listener(id: i64) -> bool {
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&id)
}

/// Bind and start accepting HTTP/2. Returns the listener id, port and host.
///
/// The bind is synchronous, so `server.address().port` is correct inside the
/// `listen(0, cb)` callback — the same property P5's `listen` preserves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn listen(
    server_handle: i64,
    host: &str,
    port: u16,
    backlog: u32,
    tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    allow_http1: bool,
    settings: crate::server::http2_session_settings::Http2SettingsState,
    max_session_memory: usize,
    reuse_port: bool,
    no_delay: bool,
) -> Result<(i64, u16, String), tl::NetError> {
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        return Err(tl::error_from_os(None, "listen"));
    }
    // `reuse_port` is false for every ordinary server: two
    // `http2.createServer().listen(p)` calls must race to `EADDRINUSE` the way
    // Node's do, not both succeed. It is true for exactly one caller, the
    // cluster worker, which shares the port on purpose — turnloop
    // 0.1.0-alpha.6's `ReusePort::Share`, which is what
    // `cluster_bind::bind_listener` did by hand and is why that worker had to
    // decline this path. See `server::turnloop_listen::try_listen_on_turnloop`
    // for why `Share` and not `Distribute`.
    //
    // `no_delay` is the server's own `noDelay` (Node defaults it to true), and
    // it reaches the listener rather than being applied per accepted socket:
    // `tcp_listen` hands it to the accepting loop, which applies it to every
    // connection before the completion reaches the binding. The hyper HTTP/2
    // path does the same thing by hand in `http2_server.rs`
    // (`apply_accept_no_delay`) — this is that behaviour on the turnloop path.
    tl::tcp_listen(id, SUBSYSTEM, host, port, backlog, reuse_port, no_delay)?;
    tl::accept_start(id)?;
    let bound = tl::local_address(id);
    let bound_port = bound.as_ref().map(|e| e.port).unwrap_or(port);
    let bound_host = bound
        .as_ref()
        .map(|e| e.address.clone())
        .unwrap_or_else(|| host.to_string());
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            id,
            Listener {
                server_handle,
                tls,
                allow_http1,
                settings,
                max_session_memory,
            },
        );
    Ok((id, bound_port, bound_host))
}

/// `server.close()` — stop accepting. Live sessions finish, which is Node's
/// contract: `server.close()` resolves once every session has closed.
pub(crate) fn close_listener(id: i64) {
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    let _ = tl::close(id);
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
