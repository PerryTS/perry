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

use perry_ext_net::native_transport as net;
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope};
use std::sync::OnceLock;

use perry_ffi::turnloop_net as tl;

mod conn;
pub(crate) mod wire;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use perry_ffi::agent_post::{self, AgentJob};

pub(crate) use conn::{
    adopt_alpn_http1, begin_stream, destroy_connection, finish_body, is_busy, note_aborted_handle,
    send_body, send_interim, send_response, take_aborted, take_pending, with_connections,
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
        assert!(
            tl::register_link_sink(SUBSYSTEM, conn::sink),
            "HTTP link sink registration refused"
        );
        // An attached `WebSocketServer` runs on this connection, so give
        // perry-ext-ws the writer it needs to reach it (see `conn::on_websocket`).
    });
    net::enabled() && tl::available(SUBSYSTEM)
}

/// Node's `err.code` for a listen this agent has no loop to run on.
///
/// Only reachable on a host where turnloop's `Loop::new` failed: every other
/// thread either owns its agent's loop or can post to the thread that does.
pub(crate) const NO_LOOP_CODE: &str = "ENOTSUP";

/// Node's `err.code` for a listen whose post to the loop's owner was refused
/// transiently every time it was retried (the owner's postbox stayed full).
/// Distinct from [`NO_LOOP_CODE`]: the loop exists, it was just busy.
pub(crate) const POST_BUSY_CODE: &str = "EAGAIN";

/// How many times a transiently refused post is retried before the operation
/// is reported as failed. A refusal is `Again` only while the owner is between
/// claiming its route and publishing its loop, or while its postbox is full —
/// both drain within a turn. The first [`POST_SPIN_ATTEMPTS`] retries only
/// yield; the rest back off by a millisecond each, so the whole budget spans
/// roughly a quarter second rather than a few microseconds of spinning.
const POST_ATTEMPTS: usize = 256;
const POST_SPIN_ATTEMPTS: usize = 16;

/// What became of a job handed to [`post_to_owner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Posted {
    /// The owner will run it.
    Accepted,
    /// No loop exists for this agent anywhere: report [`NO_LOOP_CODE`].
    NoRoute,
    /// The loop exists but refused every retry: report [`POST_BUSY_CODE`].
    Busy,
}

impl Posted {
    /// The `err.code` a listen reports for a job that did not land.
    pub(crate) fn error_code(self) -> Option<&'static str> {
        match self {
            Posted::Accepted => None,
            Posted::NoRoute => Some(NO_LOOP_CODE),
            Posted::Busy => Some(POST_BUSY_CODE),
        }
    }
}

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
/// used to run (perry-ext-net's `turnloop_io::on_loop` is the same route):
/// such a thread is a second thread acting for an agent another thread already
/// owns, so the owner binds and serves for it. It is also how a
/// thread that is not a JS thread at all (the SCHED_RR descriptor bridge) gets
/// a connection onto the loop — which is why this does not ask [`enabled`]
/// first: the first thread to ask *claims* its agent's route, and a foreign
/// thread that won that race would own a loop nobody turns.
///
/// Anything but [`Posted::Accepted`] means `op` was dropped unrun:
/// [`Posted::NoRoute`] when no loop exists for this agent anywhere (a host
/// where `Loop::new` failed), [`Posted::Busy`] when the loop exists but every
/// bounded retry was refused transiently.
pub(crate) fn post_to_owner(op: Box<dyn FnOnce() + Send>) -> Posted {
    post_with(Box::new(LoopJob(op)), agent_post::post_job)
}

/// [`post_to_owner`]'s retry policy over an injectable poster, so the
/// classification is testable without a loop.
fn post_with<J>(
    mut job: Box<J>,
    mut post: impl FnMut(Box<J>) -> Result<(), agent_post::Rejected<J>>,
) -> Posted {
    for attempt in 0..POST_ATTEMPTS {
        match post(job) {
            Ok(()) => return Posted::Accepted,
            Err(rejected) if rejected.is_permanent() => return Posted::NoRoute,
            Err(rejected) => {
                job = rejected.into_job();
                if attempt < POST_SPIN_ATTEMPTS {
                    std::thread::yield_now();
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
    }
    Posted::Busy
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
///
/// Unix-only because its one caller is: cluster descriptor passing
/// (`cluster_bind::recv_fd`, SCM_RIGHTS) does not exist on Windows, where
/// Node's default scheduling policy is SCHED_NONE.
#[cfg(unix)]
pub(crate) fn adopt_connection(server_handle: i64, socket: tl::AdoptedSocket) {
    let _posted = post_to_owner(Box::new(move || {
        // On the owner: `enabled` installs this crate's sink before the first
        // completion for the new id can exist, and is true here.
        if !enabled() {
            return;
        }
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(net::new_socket(
            SUBSYSTEM,
            f64::from_bits(JsValue::UNDEFINED.bits()),
        ));
        let link = net::socket_link(owner.get()).expect("new Socket cell");
        let installed = unsafe {
            tl::link_adopt_stream(
                &mut *net::core(owner.get()).expect("new Socket core"),
                link,
                socket,
            )
        };
        if installed.is_err() {
            net::destroy(owner.get());
            return;
        }
        net::adopted(owner.get());
        conn::start_connection(owner.get(), server_handle);
        net::flow(owner.get());
    }));
}

/// The listener's connection handler captures the logical application server,
/// not a transport identity. Accepted sockets already own their link cell.
unsafe extern "C" fn accepted(closure: *const RawClosureHeader, _: JsThis, socket: f64) -> f64 {
    let server_handle = perry_ffi::closure_capture_f64(closure, 0) as i64;
    conn::start_connection(socket, server_handle);
    f64::from_bits(JsValue::UNDEFINED.bits())
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
) -> Result<(f64, u16, String), tl::NetError> {
    let scope = TransientRootScope::enter();
    let existing =
        crate::server::server::with_base_server(server_handle, |server| server.transport_listener)
            .ok_or_else(|| tl::error_from_os(None, "listen"))?;
    let listener = scope.root_nanbox(if net::server_link(existing).is_ok() {
        existing
    } else {
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(accepted, 1; with_flags(perry_ffi::FN_BUILTIN)),
            1,
        ) as i64);
        unsafe {
            perry_ffi::set_closure_capture_f64(
                callback.get() as *mut RawClosureHeader,
                0,
                server_handle as f64,
            );
        }
        net::new_server(
            f64::from_bits(JsValue::UNDEFINED.bits()),
            f64::from_bits(JsValue::from_object_ptr(callback.get() as *mut u8).bits()),
        )
    });
    // Publish the existing logical server's traced edge before init hooks.
    crate::server::server::with_base_server_mut(server_handle, |server| {
        server.transport_listener = listener.get();
    })
    .ok_or_else(|| tl::error_from_os(None, "listen"))?;
    let state = scope.root_nanbox(net::server_state(listener.get()));
    net::own_set(state.get(), "httpIdleCloseMs", idle_close_ms as f64);
    let bound = net::listen_tcp(
        listener.get(),
        host,
        port,
        backlog,
        reuse_port,
        no_delay,
        tls,
    )?;
    Ok((listener.get(), bound.port, bound.address))
}

/// `server.close()` — stop accepting. In-flight connections finish, which is
/// Node's contract; `closeAllConnections` is what tears those down.
pub(crate) fn close_listener(listener: f64) {
    net::close_server(listener);
}

/// The application's existing traced ownership edge, never a lookup table.
pub(crate) fn listener_for_server(server_handle: i64) -> Option<f64> {
    crate::server::server::with_base_server(server_handle, |server| server.transport_listener)
        .filter(|owner| net::server_link(*owner).is_ok())
}
