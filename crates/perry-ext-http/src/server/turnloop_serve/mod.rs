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

use perry_ffi::agent_post::{self, AgentJob};

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

/// Whether a server created *now, on this thread* should live on turnloop.
///
/// Deliberately not cached: availability is a property of the calling agent.
/// A thread acting for an agent another thread already owns has no loop and
/// posts to the owner instead ([`post_to_owner`]); caching its "no" would
/// strand the owner too.
///
/// The one-shot registration is a `Once` rather than an `AtomicBool::swap`,
/// which is what `perry-ext-ws` uses and what this was not. `swap` publishes
/// "registered" on entry, so a second thread arriving mid-registration skipped
/// it and went straight to `available()` — which asks
/// `turnloop_net::sink_installed` and, with the sink not yet in place, answered
/// no. The caller then declined a loop it actually had. `Once`
/// makes that thread wait for the registration instead of racing past it.
/// (Found by a multi-threaded `cargo test`: two `enabled()` assertions on
/// different test threads, one green and one red in the same run.)
pub(crate) fn enabled() -> bool {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        // Registration is refused if the runtime's completion layout does not
        // match this crate's, which leaves `available` false rather than
        // submitting work nothing can deliver.
        tl::register_sink(SUBSYSTEM, conn::sink, alloc_id);
        // An attached `WebSocketServer` runs on this connection, so give
        // perry-ext-ws the writer it needs to reach it (see `conn::on_websocket`).
        conn::register_ws_transport();
    });
    tl::available(SUBSYSTEM)
}

/// Node's `err.code` for a listen this agent has no loop to run on.
///
/// Only reachable on a host where turnloop's `Loop::new` failed: every other
/// thread either owns its agent's loop or can post to the thread that does.
pub(crate) const NO_LOOP_CODE: &str = "ENOTSUP";

/// How many times a transiently refused post is retried before the operation
/// is reported as failed. A refusal is `Again` only while the owner is between
/// claiming its route and publishing its loop, or while its postbox is full —
/// both drain within a turn, so a short spin is the whole remedy.
const POST_ATTEMPTS: usize = 64;

/// One piece of work carried to the thread that owns this agent's loop.
struct LoopJob(Box<dyn FnOnce() + Send>);

impl AgentJob for LoopJob {
    fn run(self: Box<Self>) {
        (self.0)();
    }
}

/// Run `op` on the thread that owns this agent's turnloop loop, which serves
/// the same JS heap as the caller (turnloop P10, `perry_ffi::agent_post`).
///
/// This is what replaced the hyper accept loop a thread without its own loop
/// used to run (perry-ext-net's `turnloop_io::on_loop` is the same route): such a thread is a second thread acting for an agent another
/// thread already owns, so the owner binds and serves for it. It is also how a
/// thread that is not a JS thread at all (the SCHED_RR descriptor bridge) gets
/// a connection onto the loop — which is why this does not ask [`enabled`]
/// first: the first thread to ask *claims* its agent's route, and a foreign
/// thread that won that race would own a loop nobody turns.
///
/// Returns `false`, with `op` dropped unrun, only when no loop exists for this
/// agent anywhere (a host where `Loop::new` failed).
pub(crate) fn post_to_owner(op: Box<dyn FnOnce() + Send>) -> bool {
    let mut job = Box::new(LoopJob(op));
    for _ in 0..POST_ATTEMPTS {
        match agent_post::post_job(job) {
            Ok(()) => return true,
            Err(rejected) if rejected.is_permanent() => return false,
            Err(rejected) => {
                job = rejected.into_job();
                std::thread::yield_now();
            }
        }
    }
    false
}

/// Serve a connection some other process accepted — a SCHED_RR cluster
/// worker's, whose primary owns the listening socket and passes each accepted
/// descriptor over the cluster IPC channel (#4962).
///
/// The descriptor is put on this agent's loop (`turnloop_net::adopt_stream`,
/// turnloop's `Detached::from_fd` + `Driver::attach`) on the loop's owner, and
/// from then on it is indistinguishable from a connection turnloop accepted:
/// the same `'connection'` event, codec, idle deadline and response path.
///
/// Called from the descriptor bridge thread, so the adoption is always
/// posted. A connection that cannot be adopted is closed, which is what the
/// peer of a worker that stopped accepting sees.
#[cfg(any(unix, windows))]
pub(crate) fn adopt_connection(server_handle: i64, socket: tl::AdoptedSocket) {
    let _posted = post_to_owner(Box::new(move || {
        // On the owner: `enabled` installs this crate's sink before the first
        // completion for the new id can exist, and is true here.
        if !enabled() {
            return;
        }
        let id = next_id();
        if id == perry_ffi::INVALID_HANDLE {
            return;
        }
        if tl::adopt_stream(id, SUBSYSTEM, socket).is_err() {
            return;
        }
        let idle_close_ms = crate::server::server::with_base_server(
            server_handle,
            crate::server::server::idle_close_ms,
        )
        .unwrap_or(0);
        conn::start_connection(id, server_handle, None, idle_close_ms);
    }));
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
    reuse_port: bool,
    no_delay: bool,
    idle_close_ms: u64,
) -> Result<(i64, u16, String), tl::NetError> {
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        return Err(tl::error_from_os(None, "listen"));
    }
    // `reuse_port` is now an argument, and it is FALSE for every ordinary
    // server. It used to be hard-wired false for a reason that has since
    // changed, and before that it accidentally received `no_delay` — which
    // defaults to true (`http.createServer`'s Node default), so every turnloop
    // HTTP and HTTPS listener bound with `SO_REUSEPORT` and a second `listen()`
    // on the same port quietly succeeded where Node answers EADDRINUSE. That
    // must not come back: a plain server passes `false` and an `EADDRINUSE`
    // stays an `EADDRINUSE`.
    //
    // What changed is the cluster worker. It is the one caller that *wants*
    // `SO_REUSEPORT`, and the hard-wired `false` is why it had to decline the
    // turnloop path and bind a `std::net::TcpListener` by hand
    // (`cluster_bind::bind_listener`, `socket.set_reuse_port(true)`). turnloop
    // 0.1.0-alpha.6 split its own `bool` into `ReusePort::{No, Share,
    // Distribute}`, and `perry-runtime`'s `listen_opts` maps this `true` to
    // `Share` — which is exactly what `bind_listener` does by hand, on every
    // platform Perry ships. NOT `Distribute`: that is the kernel-balanced
    // variant, it is `Unsupported` on macOS and the non-FreeBSD BSDs, and
    // Perry's cluster has never had it.
    //
    // `no_delay` reaches the option it names. Node's `http.createServer`
    // defaults it to true and applies it to every accepted connection; the
    // hyper path did that by hand and the turnloop path did not do it at all,
    // because this argument was landing in `reuse_port` instead.
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
