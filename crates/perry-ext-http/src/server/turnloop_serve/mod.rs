//! turnloop P5: `node:http` / `node:https` servers on turnloop handles.
//!
//! # What this replaces
//!
//! | hyper / tokio | turnloop |
//! |---|---|
//! | a `tokio::spawn` accept loop per listening server | one multishot `accept_start` |
//! | a `tokio::spawn`ed `http1::Builder::serve_connection` per connection | one multishot `read_start` + [`conn`]'s state machine |
//! | `hyper`'s HTTP/1 parser and framer | `turnloop_http::http1::{Decoder, Encoder}` (sans-I/O) |
//! | `tokio_rustls::TlsAcceptor` per HTTPS connection | `perry_ext_net::turnloop_tls`'s unbuffered session |
//! | an `mpsc` carrying `(req, res)` to the main thread, plus `notify_main_thread()` | a queue on this thread, because the codec already runs on it |
//! | a `oneshot` carrying the response shape back to the hyper task | `res.end()` encoding and submitting the write directly |
//!
//! # Why sans-I/O rather than `turnloop_http::asynchronous`
//!
//! `turnloop-http` also ships a futures-io server driver. It needs a
//! `turnloop_io::LocalExecutor`, and a `LocalExecutor` **constructs its own
//! `Driver`** and silently drops every completion whose token it did not issue
//! (`Shared::dispatch` returns early unless the token's top bit is set). Perry
//! already owns a `turnloop::Loop` and routes P1 net, P2 process and P3 timer
//! tokens through it, so adopting the executor would mean either a second loop
//! — the mixed-transport deadlock P1 had to paper over — or losing those
//! completions. The sans-I/O codecs have no such coupling, and they are the
//! part that actually replaces hyper.
//!
//! # Ordering, and why JS never runs inside a turn
//!
//! The sink runs inside `dispatch_staged`, which the event pump calls after a
//! turn has returned. It decodes, but it does **not** call JS: a fully decoded
//! request is pushed onto the server's queue and the existing pump
//! (`js_node_http_server_process_pending`) runs the handler on its own tick,
//! exactly where it ran when hyper delivered requests over an `mpsc`. So the
//! event-loop phase order the gap suite pins is unchanged; what disappears is
//! the thread hop, the channel and the cross-thread notify.
//!
//! # GC
//!
//! A connection holds decoded head/body bytes as owned `Vec<u8>`s and the two
//! handle ids of the request it produced. No JS value and no heap pointer
//! reaches the driver, and this module registers no root scanner: the
//! `IncomingMessage` / `ServerResponse` handles it allocates are scanned by
//! `perry-ext-http`'s existing `scan_http_server_roots`, which is also why a
//! request is carried as ids rather than as `f64` closures (#8082).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;

mod conn;
mod wire;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use conn::{
    adopt_alpn_http1, begin_stream, connections_of, destroy_connection, finish_body, is_busy,
    note_aborted_handle, send_body, send_interim, send_response, take_aborted, take_pending,
};

/// This crate's slot in the runtime's completion-sink registry.
/// `perry-ext-net` owns slot 0.
pub(crate) const SUBSYSTEM: u8 = 1;

/// One authoritative domain for the ids this crate allocates for turnloop
/// listeners and connections, sharing only the numeric pool with perry-ffi's
/// ordinary payload registry — the runtime keys its `Entry` map by this id
/// across every subsystem, so it has to be globally unique.
fn registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: OnceLock<perry_ffi::NativeRegistryDomain> = OnceLock::new();
    *DOMAIN.get_or_init(|| {
        perry_ffi::NativeRegistryDomain::new().expect("http native registry domains exhausted")
    })
}

pub(crate) fn next_id() -> i64 {
    perry_ffi::reserve_handle_id_in_domain(registry_domain())
}

static REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a server created *now, on this thread* should live on turnloop.
///
/// Deliberately not cached: availability is a property of the calling agent.
/// A `worker_threads` Worker has no loop before P3/P4 and must keep the hyper
/// path; caching its "no" would strand the primary agent too.
pub(crate) fn enabled() -> bool {
    if !REGISTERED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        // Registration is refused if the runtime's completion layout does not
        // match this crate's, which leaves `available` false and keeps every
        // server on hyper rather than submitting work nothing can deliver.
        tl::register_sink(SUBSYSTEM, conn::sink, alloc_id);
    }
    tl::available(SUBSYSTEM)
}

/// Allocate the id for a connection turnloop just accepted.
extern "C" fn alloc_id() -> i64 {
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        0
    } else {
        id
    }
}

/// A bound turnloop listener and the JS server it belongs to.
pub(crate) struct Listener {
    pub(crate) server_handle: i64,
    /// `Some` for `https.createServer` / `http2.createSecureServer`: every
    /// accepted connection starts a TLS handshake before any HTTP byte.
    pub(crate) tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    /// `server.keepAliveTimeout` + `server.keepAliveTimeoutBuffer`, in ms, as
    /// the *idle close* deadline. Zero means "never time out" — Node's
    /// documented meaning for `keepAliveTimeout = 0`, measured on 26.5.1.
    pub(crate) idle_close_ms: u64,
}

fn listeners() -> &'static Mutex<HashMap<i64, Listener>> {
    static LISTENERS: OnceLock<Mutex<HashMap<i64, Listener>>> = OnceLock::new();
    LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn with_listener<R>(id: i64, f: impl FnOnce(&Listener) -> R) -> Option<R> {
    let map = listeners().lock().unwrap_or_else(|e| e.into_inner());
    map.get(&id).map(f)
}

/// Bind and start accepting. Returns the listener id and the bound port.
///
/// The bind is synchronous, so `server.address().port` is correct inside the
/// `listen(0, cb)` callback — the property #2132 added and the hyper path got
/// by binding a `std::net::TcpListener` before spawning.
pub(crate) fn listen(
    server_handle: i64,
    host: &str,
    port: u16,
    backlog: u32,
    tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    no_delay: bool,
    idle_close_ms: u64,
) -> Result<(i64, u16, String), tl::NetError> {
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        return Err(tl::error_from_os(None, "listen"));
    }
    // `reuse_port` is FALSE. It used to receive `no_delay`, which defaults to
    // true (`http.createServer`'s Node default), so every turnloop HTTP and
    // HTTPS listener bound with `SO_REUSEPORT` and a second `listen()` on the
    // same port quietly succeeded where Node answers EADDRINUSE. `perry-ext-net`'s
    // own `tcp_listen` call always passed `false` here; only this one drifted.
    //
    // Nothing on this path wants `SO_REUSEPORT`: the cluster worker that does
    // declines the turnloop path in `turnloop_listen::try_listen_on_turnloop`
    // and binds a `std::net::TcpListener`, which is one of the two reasons that
    // decline exists.
    // `no_delay` now reaches the option it names. Node's `http.createServer`
    // defaults it to true and applies it to every accepted connection; the
    // hyper path did that by hand and the turnloop path did not do it at all,
    // because this argument was landing in `reuse_port` instead.
    tl::tcp_listen(id, SUBSYSTEM, host, port, backlog, false, no_delay)?;
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
                idle_close_ms,
            },
        );
    Ok((id, bound_port, bound_host))
}

/// `server.close()` — stop accepting. In-flight connections finish, which is
/// Node's contract; `closeAllConnections` is what tears those down.
pub(crate) fn close_listener(id: i64) {
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    let _ = tl::close(id);
}

/// Whether any turnloop listener belongs to this JS server handle.
pub(crate) fn listener_for_server(server_handle: i64) -> Option<i64> {
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(_, l)| l.server_handle == server_handle)
        .map(|(id, _)| *id)
}
