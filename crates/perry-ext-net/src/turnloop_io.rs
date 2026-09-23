//! `node:net` on turnloop handles (P1).
//!
//! What this replaces, one for one:
//!
//! | tokio | turnloop |
//! |---|---|
//! | a `spawn_async` accept loop per listening server (`lib.rs`, `ipc.rs`) | one multishot `accept_start` |
//! | a `run_socket_task` per connection, selecting on `read_buf` and a channel | one multishot `read_start`, plus direct submissions |
//! | `SocketCommand` over a per-socket `mpsc` for every write / end / destroy | `write` / `shutdown` / `close` submitted where the FFI call happens |
//! | `TcpStream::connect(&str)` on a task, which also resolved the name | `tcp_connect`, whose name lookup runs on the shared blocking pool |
//!
//! Everything downstream is untouched: this module produces exactly the same
//! [`PendingNetEvent`]s in the same order, into the same queue, drained by the
//! same `js_ext_net_drain_pending`. The JS-visible surface, the listener maps,
//! the GC root scanner and the buffer pool do not know which transport ran.
//!
//! # Which sockets come here
//!
//! All of them. This crate has no other transport: tokio and `tokio_rustls`
//! are gone from its manifest, and with them the per-socket task this table
//! replaced.
//!
//! What used to keep that task alive was the P1 coexistence rule — a thread
//! that could not get a loop of its own kept the tokio socket. Since turnloop
//! P9 every JS agent has a loop, so such a thread is a **second thread acting
//! for an agent another thread already owns** (an embedder's pump thread,
//! Android's UI thread for `perry-native`). It serves the same JS heap as the
//! owner, so [`on_loop`] hands its submission to the owner through
//! `perry_ffi::agent_post` and the completion is delivered where the socket's
//! JS values live. That is the whole replacement for the fallback.
//!
//! The one case posting cannot serve is an agent with no loop anywhere — a
//! host where `Loop::new` failed. There the operation fails with `ENOTSUP`
//! ([`NO_LOOP_CODE`]) through the socket's normal `'error'` path, instead of
//! silently running on a second event loop.
//!
//! TLS rides the same handles: the rustls session runs *above* the turnloop
//! socket (`turnloop_tls_io`), so neither `tls.connect` nor
//! `socket.upgradeToTLS` needs a descriptor to move.
//!
//! There is no handover in either direction — a socket belongs to one loop
//! from creation to close.
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread, from the event loop's own turn, so it
//! may touch the socket registries directly. It still only pushes
//! [`PendingNetEvent`]s: JS values are built later, in the pump, exactly as
//! the tokio path required, so the arena-safety rule in `lib.rs` holds
//! unchanged. Read bytes are copied out of turnloop's pooled lease before the
//! sink returns; write bytes were already copied into an owned `Vec` by
//! `jsvalue_to_socket_bytes`. No JS heap pointer reaches the driver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use perry_ffi::agent_post::{self, AgentJob};
use perry_ffi::turnloop_net as tl;

use crate::{
    buffer_pool, mark_closed, push_event, raw_bridge, server_state, statics, PendingNetEvent,
    SocketCommand,
};

/// This binding's slot in the runtime's sink registry.
pub(crate) const SUBSYSTEM: u8 = 0;

/// Whether sink registration has been attempted. The runtime's own
/// registration is idempotent; this only keeps the ABI layout check behind it
/// off the per-socket path.
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Per-socket state this transport needs and `SocketState` has no field for.
#[derive(Default)]
struct Aux {
    /// `server_state::begin_local_connect`'s reservation, held until the
    /// connect completes so `PendingNetEvent::Connect` can carry it.
    local_server: Option<(i64, bool)>,
    /// The peer sent FIN and `'end'` has not been delivered yet.
    read_ended: bool,
    /// The readable side ended and Node's `allowHalfOpen: false` default is
    /// closing the socket — but only once the write-side shutdown completes,
    /// so writes issued from the `'end'` handler are not cancelled by the
    /// close (turnloop's `close` cancels every outstanding operation).
    close_after_shutdown: bool,
    /// The peer's FIN arrived before this accepted socket's `'connection'`
    /// callback had run, so `'end'` is held until it does. The event queue
    /// alone cannot order these: `server_state` may defer a loopback
    /// `ServerConnection` across a pump boundary, and turnloop can deliver the
    /// whole request plus its FIN inside the very first turn — so an `'end'`
    /// pushed at EOF time would be dispatched to a socket that has no
    /// listeners yet and be lost. The tokio task had the same hazard and
    /// solved it by blocking its post-EOF drain on the same marker.
    deferred_eof: bool,
    /// An `'error'` has been reported for this socket. The tokio task broke
    /// its loop after the first one; this keeps that "one error, then the
    /// terminal pair" shape when several operations fail in the same turn.
    errored: bool,
    /// `PendingNetEvent::Close` has been pushed. Node emits `'close'` AFTER
    /// `'error'`, so this guards double-emission — never emission itself.
    closed_emitted: bool,
    /// `socket.end()` has already submitted the write-side shutdown. Node's
    /// `allowHalfOpen: false` close on `'end'` must NOT submit a second one: a
    /// second `shutdown(2)` on a socket whose peer has gone answers `ENOTCONN`,
    /// which reached JS as a spurious `'error'` — visible on the TLS upgrade
    /// path, where `end()` always precedes the peer's FIN, and latent on a
    /// plain socket with the same ordering.
    write_ended: bool,
    /// That shutdown has completed, which means every write queued ahead of it
    /// has left. Until then the socket must not be closed: `Loop::close`
    /// cancels outstanding operations, so closing here would cancel exactly
    /// the writes an `'end'` handler just issued (P1's third behaviour note).
    shutdown_done: bool,
    /// Completion tokens of `end()` calls made after the first one. Node runs
    /// every `end(cb)` callback once the stream finishes; only the first
    /// `end()` submits the shutdown, and these complete with it.
    extra_end_users: Vec<u64>,
    /// The readable EOF has been delivered. A TLS socket can reach it twice —
    /// the peer's `close_notify` and then the TCP FIN — and Node emits
    /// `'end'` exactly once.
    eof_emitted: bool,
    /// P5: `tls.connect` asked for TLS from byte zero. The session is
    /// installed the instant the connect completes and before `'connect'` is
    /// pushed, which is the ordering the tokio path got by handshaking before
    /// it pushed the event.
    direct_tls: Option<(String, bool, crate::TlsClientConfigData)>,
    /// Application writes to a `tls.connect` socket made before its TCP
    /// connect completed, with their completion tokens. They must be
    /// encrypted, so they cannot wait in the runtime's plaintext backlog —
    /// that sent them in the clear ahead of the ClientHello. Replayed through
    /// the TLS layer the moment it is installed.
    held_tls_writes: Vec<(Vec<u8>, u64)>,
    /// An `end()` made in that same window, replayed after the writes.
    held_tls_end: Option<u64>,
}

fn aux() -> &'static Mutex<std::collections::HashMap<i64, Aux>> {
    static AUX: OnceLock<Mutex<std::collections::HashMap<i64, Aux>>> = OnceLock::new();
    AUX.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn with_aux<R>(id: i64, f: impl FnOnce(&mut Aux) -> R) -> R {
    let mut map = aux().lock().unwrap_or_else(|e| e.into_inner());
    f(map.entry(id).or_default())
}

fn forget_aux(id: i64) -> Aux {
    let mut map = aux().lock().unwrap_or_else(|e| e.into_inner());
    map.remove(&id).unwrap_or_default()
}

/// Whether a socket created *now, on this thread* should live on turnloop.
///
/// Deliberately not cached: availability is a property of the calling
/// *thread* (only the thread that owns its agent's loop may submit), not of
/// the process. Registration behind it is idempotent and costs one atomic once
/// it has happened. A `false` here sends the work to [`on_loop`]'s posting
/// route, never to a second transport.
pub(crate) fn enabled() -> bool {
    if !REGISTERED.load(Ordering::Acquire) {
        // `register_sink` refuses if the runtime's completion layout does not
        // match this crate's, which leaves `available` false rather than
        // submitting work nothing can deliver.
        tl::register_sink(SUBSYSTEM, sink, alloc_id);
        REGISTERED.store(true, Ordering::Release);
    }
    tl::available(SUBSYSTEM)
}

/// Allocate the JS-visible handle id for a connection turnloop just accepted.
extern "C" fn alloc_id() -> i64 {
    let id = crate::next_id();
    if id == perry_ffi::INVALID_HANDLE {
        0
    } else {
        id
    }
}

// ── Which thread submits ────────────────────────────────────────────────────

/// Node's `err.code` for an operation this agent has no loop to run on.
///
/// Only reachable on a host where turnloop's `Loop::new` failed: every other
/// thread either owns its agent's loop or can post to the thread that does.
pub(crate) const NO_LOOP_CODE: &str = "ENOTSUP";

/// How many times a transiently refused post is retried before the operation
/// is reported as failed. A refusal is `Again` only while the owner is between
/// claiming its route and publishing its loop, or while its postbox is full —
/// both drain within a turn, so a short spin is the whole remedy.
const POST_ATTEMPTS: usize = 64;

/// One submission carried to the thread that owns this agent's loop.
struct LoopJob(Box<dyn FnOnce() + Send>);

impl AgentJob for LoopJob {
    fn run(self: Box<Self>) {
        (self.0)();
    }
}

/// Run `op` on the thread that owns this agent's turnloop loop.
///
/// Inline when that is this thread — the common case, and exactly what the
/// call sites did before — otherwise posted to the owner, which serves the
/// same JS heap (module note). Returns `false`, with `op` dropped unrun, only
/// when no loop exists for this agent at all; the caller then reports
/// [`NO_LOOP_CODE`] through its usual error path.
///
/// `op` must not assume it runs synchronously: on the posting path it runs on
/// a later turn of the owner, so anything the caller needs to observe
/// immediately (a handle id, a `connecting` flag) is published before this is
/// called, and failures are reported from inside `op` as events.
pub(crate) fn on_loop(op: impl FnOnce() + Send + 'static) -> bool {
    if enabled() {
        op();
        return true;
    }
    post_to_owner(Box::new(op))
}

/// Post `op` to the owner without first asking whether this thread owns the
/// loop.
///
/// For callers that are not JS threads at all — a tokio worker in another
/// binding handing over an upgraded connection. [`enabled`] must not be asked
/// there: the first thread to ask *claims* its agent's route for life, and a
/// foreign thread that won that race would own a loop nobody turns.
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

/// A one-shot deadline this crate armed, and what to do when it fires.
///
/// Two transport-agnostic paths used to sleep on a tokio timer: the loopback
/// `'connection'` deferral (`server_state::schedule_server_connection`) and
/// the already-aborted `tls.connect` signal path (`tls::schedule_tls_abort`).
/// Both are deadlines on the loop now, delivered to this crate's sink as a
/// `NET_TIMER` completion naming the id armed here.
pub(crate) enum Deadline {
    /// Publish a deferred loopback `ServerConnection(server, socket, true)`.
    ServerConnection { server_id: i64, socket_id: i64 },
    /// Fire an already-aborted `tls.connect`'s `AbortError` + `'close'`.
    TlsAbort { socket_id: i64 },
}

fn deadlines() -> &'static Mutex<std::collections::HashMap<i64, Deadline>> {
    static DEADLINES: OnceLock<Mutex<std::collections::HashMap<i64, Deadline>>> = OnceLock::new();
    DEADLINES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Run `deadline` `delay_ms` from now, on the loop.
///
/// The deadline is unreferenced (turnloop's `timer_arm`): it never keeps the
/// process alive by itself. Both users are covered by a handle that does —
/// the listening server, or the aborted socket that `schedule_tls_abort`
/// marks open until its `'close'`.
pub(crate) fn arm_deadline(delay_ms: u64, deadline: Deadline) {
    on_loop_or_now(move || {
        // A fresh id from the shared allocator, never a socket's: the runtime
        // keys deadlines by id per thread across every subsystem, so reusing
        // an id another binding armed would *move* that binding's deadline.
        let id = crate::next_id();
        if id == perry_ffi::INVALID_HANDLE {
            fire_deadline(deadline);
            return;
        }
        deadlines()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, deadline);
        if tl::timer_arm(id, SUBSYSTEM, delay_ms).is_err() {
            // No deadline could be armed: fire now rather than never. The
            // delay only ever ordered an event behind a pump boundary.
            on_timer(id);
        }
    });
}

/// [`on_loop`], falling back to running `op` right here when this agent has
/// no loop at all. Only for work that is correct on any thread and merely
/// *prefers* the loop (a deadline that can fire immediately instead).
fn on_loop_or_now(op: impl FnOnce() + Send + 'static) {
    let op: Box<dyn FnOnce() + Send> = Box::new(op);
    if enabled() {
        op();
        return;
    }
    // `post_to_owner` consumes the job even on refusal, so decide first.
    if agent_post::available() {
        if post_to_owner(op) {
            return;
        }
        // Refused after `available()` said yes: the owner went away in
        // between. The deadline is lost with the job, which is the same
        // outcome as an agent torn down with a deadline pending.
        return;
    }
    op();
}

fn fire_deadline(deadline: Deadline) {
    match deadline {
        Deadline::ServerConnection {
            server_id,
            socket_id,
        } => {
            statics::pending_events()
                .lock()
                .unwrap()
                .push(PendingNetEvent::ServerConnection(
                    server_id, socket_id, true,
                ));
            perry_ffi::notify_main_thread();
        }
        Deadline::TlsAbort { socket_id } => crate::tls::fire_pending_tls_abort(socket_id),
    }
}

fn on_timer(id: i64) {
    let deadline = deadlines()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    // A fired one-shot is terminal on the runtime side too: it dropped its own
    // record, so there is nothing to cancel.
    if let Some(deadline) = deadline {
        // The id named only this deadline and was never handed to JS, so it
        // goes back to the shared allocator rather than leaking a slot of the
        // handle band per deferred connection.
        perry_ffi::free_handle_id(id);
        fire_deadline(deadline);
    }
}

// ── Submission helpers, called from the FFI entry points ────────────────────

/// Deliver one socket command to a turnloop-backed socket.
///
/// **Called with the socket registry locked**, from `SocketState::command`, so
/// nothing here may take that lock again — the caller owns the `SocketState`
/// and applies the byte accounting this returns. `Err` carries the message the
/// caller emits once it has dropped the lock.
///
/// `queued_out` receives the socket's new queued byte count for a write.
pub(crate) fn command(
    id: i64,
    cmd: SocketCommand,
    queued_out: &mut Option<u64>,
) -> Result<(), String> {
    let secure = crate::turnloop_tls_io::installed(id);
    // A `tls.connect` still connecting has no TLS layer yet: hold its writes
    // (see `Aux::held_tls_writes`).
    let tls_pending = !secure && with_aux(id, |a| a.direct_tls.is_some());
    match cmd {
        SocketCommand::Write(bytes, completion) if tls_pending => {
            let held = with_aux(id, |a| {
                a.held_tls_writes.push((bytes, completion));
                a.held_tls_writes
                    .iter()
                    .map(|(b, _)| b.len())
                    .sum::<usize>()
            });
            *queued_out = Some(held as u64);
            Ok(())
        }
        SocketCommand::Write(bytes, completion) if secure => {
            match crate::turnloop_tls_io::write(id, &bytes, completion) {
                Ok(queued) => {
                    *queued_out = Some(queued as u64);
                    Ok(())
                }
                Err(message) => Err(message),
            }
        }
        SocketCommand::Write(bytes, completion) => match tl::write(id, &bytes, completion) {
            Ok(queued) => {
                *queued_out = Some(queued as u64);
                Ok(())
            }
            Err(err) => Err(err.message()),
        },
        // `end()` on a TLS socket sends close_notify first; the FIN is queued
        // behind it so the peer sees an orderly shutdown rather than a
        // truncation attack.
        SocketCommand::End(completion) => {
            // A repeated `end()` must not submit a second shutdown: it would
            // replace the first one's pending token (stranding that
            // callback), and a second `shutdown(2)` answers `ENOTCONN`. It
            // completes together with the first instead — or right away, if
            // that one already has.
            enum Repeat {
                First,
                Pending,
                Done,
            }
            let repeat = with_aux(id, |a| {
                if !a.write_ended {
                    a.write_ended = true;
                    Repeat::First
                } else if a.shutdown_done {
                    Repeat::Done
                } else {
                    if completion != 0 {
                        a.extra_end_users.push(completion);
                    }
                    Repeat::Pending
                }
            });
            match repeat {
                Repeat::First if tls_pending => {
                    with_aux(id, |a| a.held_tls_end = Some(completion));
                    Ok(())
                }
                Repeat::First if secure => crate::turnloop_tls_io::shutdown(id, completion),
                Repeat::First => tl::shutdown(id, completion).map_err(|e| e.message()),
                Repeat::Pending => Ok(()),
                Repeat::Done => {
                    if completion != 0 {
                        push_event(PendingNetEvent::ShutdownComplete(id, completion, None));
                    }
                    Ok(())
                }
            }
        }
        SocketCommand::Destroy => tl::close(id).map_err(|e| e.message()),
        // The accepted socket's `'connection'` callback has returned, so its
        // listeners exist: release an EOF that arrived before them.
        SocketCommand::ServerConnectionReady => {
            release_deferred_eof(id);
            Ok(())
        }
    }
}

/// [`command`] for a socket whose loop another thread owns.
///
/// The command is carried to that thread and applied there, with the same
/// accounting and the same failure path the inline call has: the queued byte
/// count is written back once the driver has answered, and a refused
/// submission is reported through [`submission_failed`] on the owner.
///
/// Returns `Err` with a Node-shaped message only when this agent has no loop
/// at all, in which case nothing was queued.
pub(crate) fn command_on_owner(id: i64, cmd: SocketCommand) -> Result<(), String> {
    let completion = match &cmd {
        SocketCommand::Write(_, completion) | SocketCommand::End(completion) => *completion,
        _ => 0,
    };
    let posted = post_to_owner(Box::new(move || {
        let mut queued = None;
        let result = command(id, cmd, &mut queued);
        if let Some(queued) = queued {
            if let Ok(mut sockets) = statics::sockets().lock() {
                if let Some(socket) = sockets.get_mut(&id) {
                    socket.bytes_queued = queued;
                }
            }
        }
        if let Err(message) = result {
            submission_failed(id, completion, message);
        }
    }));
    if posted {
        Ok(())
    } else {
        Err(NO_LOOP_CODE.to_string())
    }
}

/// A submission that failed before the driver accepted it.
///
/// Called with no lock held, after the caller released the socket registry.
/// Mirrors the tokio task's write-failure path: the write callback first, then
/// `'error'`, then the socket is torn down.
pub(crate) fn submission_failed(id: i64, completion: u64, message: String) {
    if completion != 0 {
        push_event(PendingNetEvent::WriteComplete(
            id,
            completion,
            Some(message.clone()),
        ));
    }
    if !with_aux(id, |a| std::mem::replace(&mut a.errored, true))
        && !raw_bridge::mark_terminal(id, Some(message.clone()))
    {
        push_event(PendingNetEvent::Error(id, message));
    }
    destroy(id);
}

/// `socket.destroy()` on a turnloop socket. The `'close'` event is pushed when
/// the driver reports the handle really gone, never before.
pub(crate) fn destroy(id: i64) {
    if tl::close(id).is_ok() {
        // The driver will deliver `Closed`, and that is what emits `'close'`.
        return;
    }
    // The handle is already gone (a destroy that raced the peer's reset, or a
    // second `destroy()`): emit the terminal event the caller is waiting for
    // rather than stranding the socket.
    emit_close_once(id);
}

/// Push `'close'` and retire the socket, at most once per socket.
fn emit_close_once(id: i64) {
    if with_aux(id, |a| std::mem::replace(&mut a.closed_emitted, true)) {
        return;
    }
    crate::turnloop_tls_io::forget(id);
    if !raw_bridge::mark_terminal(id, None) {
        push_event(PendingNetEvent::Close(id));
    }
    mark_closed(id);
    forget_aux(id);
}

/// Start the readable side. Called once the socket is connected or accepted.
pub(crate) fn start_reading(id: i64) {
    if let Err(err) = tl::read_start(id) {
        push_event(PendingNetEvent::Error(id, err.message()));
        destroy(id);
    }
}

/// Finish a readable-EOF that the pump has now delivered to `'end'` listeners.
///
/// Node's default (`allowHalfOpen: false`) ends the writable side once the
/// readable side has ended, and only then closes. Doing it here — after the
/// pump fired `'end'` — is what gives a synchronous `socket.write()` inside an
/// `'end'` handler the same chance it had inside the tokio task's post-EOF
/// command drain.
pub(crate) fn finish_read_end(id: i64) {
    // The pump that delivered `'end'` may be a second thread acting for this
    // agent; the shutdown and close below are submissions, so they run on the
    // loop's owner. A socket exists only if a loop does, so a refused post has
    // nothing left to finish.
    if !enabled() {
        let _ = post_to_owner(Box::new(move || finish_read_end(id)));
        return;
    }
    if !with_aux(id, |a| std::mem::replace(&mut a.read_ended, false)) {
        return;
    }
    // The application already ended the writable side inside its `'end'`
    // handler, so the shutdown is submitted and there is nothing to ask for
    // again (a second `shutdown(2)` answers `ENOTCONN`). Whether the socket may
    // close *now* is the whole question: closing while that shutdown is still
    // outstanding cancels the writes queued ahead of it, which is how a
    // `socket.write()` from an `'end'` handler went missing.
    if with_aux(id, |a| a.write_ended) {
        if with_aux(id, |a| a.shutdown_done) {
            destroy(id);
        } else {
            with_aux(id, |a| a.close_after_shutdown = true);
        }
        return;
    }
    // Queue the shutdown BEHIND whatever the `'end'` handler just wrote, and
    // close only when it completes. turnloop orders a handle's writes and its
    // shutdown, so a completed shutdown means every queued byte left — while
    // closing here instead would cancel those writes outright.
    match tl::shutdown(id, 0) {
        Ok(()) => with_aux(id, |a| a.close_after_shutdown = true),
        Err(_) => destroy(id),
    }
}

/// Record the local-connect reservation for a socket that is connecting.
pub(crate) fn note_local_connect(id: i64, local_server: Option<(i64, bool)>) {
    with_aux(id, |a| a.local_server = local_server);
}

/// Record that this connecting socket is a `tls.connect`, so the handshake
/// starts as soon as the connect completes.
pub(crate) fn note_direct_tls(
    id: i64,
    servername: String,
    verify: bool,
    config: crate::TlsClientConfigData,
) {
    with_aux(id, |a| a.direct_tls = Some((servername, verify, config)));
}

/// Start a local (Unix socket / named pipe) client connect.
pub(crate) fn connect_pipe(id: i64, path: &str) -> Result<(), tl::NetError> {
    tl::pipe_connect(id, SUBSYSTEM, path)
}

/// Start an outbound TCP client connect.
///
/// P1 deliberately left this class on tokio: `socket.upgradeToTLS` moved a
/// live `TcpStream` into `tokio_rustls`, turnloop owns its descriptor without
/// exposing it, and a socket's transport is fixed at creation — so a client
/// that *might* be upgraded could not be created on turnloop. P5 removes the
/// premise rather than the restriction: TLS now runs above the turnloop handle
/// (`turnloop_tls_io`), so nothing has to move and the class comes over.
///
/// A hostname is resolved by the driver off the loop thread, which is the
/// property `TcpStream::connect(&str)` had and a `to_socket_addrs()` here
/// would have silently lost.
pub(crate) fn connect_tcp(
    id: i64,
    host: &str,
    port: u16,
    nodelay: bool,
) -> Result<(), tl::NetError> {
    tl::tcp_connect(id, SUBSYSTEM, host, port, nodelay)
}

/// Bind, listen and start accepting on a TCP server.
pub(crate) fn listen_tcp(id: i64, host: &str, port: u16, backlog: u32) -> Result<(), tl::NetError> {
    // `net.createServer({ noDelay })` defaults to FALSE in Node, unlike
    // `http.createServer`'s, and Perry's `net` surface has never applied it —
    // so this stays false and the behaviour is unchanged.
    tl::tcp_listen(id, SUBSYSTEM, host, port, backlog, false, false)?;
    tl::accept_start(id)
}

/// Bind, listen and start accepting on a local server.
pub(crate) fn listen_pipe(id: i64, path: &str, backlog: u32) -> Result<(), tl::NetError> {
    tl::pipe_listen(id, SUBSYSTEM, path, backlog)?;
    tl::accept_start(id)
}

/// `server.close()` on a turnloop-backed server.
///
/// Must run on the loop's owner; `js_net_server_close` routes it there.
pub(crate) fn close_server(id: i64) {
    if tl::close(id).is_err() {
        // Never listened, or already closing: the caller still needs its
        // terminal event.
        push_event(PendingNetEvent::ServerClose(id));
    }
}

/// Whether `id` is a live turnloop handle, for the FFI entry points that have
/// to choose a transport without a `SocketState` in hand.
pub(crate) fn owns(id: i64) -> bool {
    tl::is_live(id)
}

/// `server.address()` for a turnloop-backed listener.
pub(crate) fn local_endpoint(id: i64) -> Option<tl::Endpoint> {
    tl::local_address(id)
}

// ── Completion sink ─────────────────────────────────────────────────────────

extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime passes a live completion for the duration of the
    // call, which is this function's body.
    let c = unsafe { &*completion };
    match c.kind {
        tl::NET_CONNECT => on_connect(c.id),
        tl::NET_ACCEPT => on_accept(c.id, c.conn),
        // SAFETY: same call; the pooled lease outlives it.
        tl::NET_DATA => on_data(c.id, unsafe { c.bytes() }),
        tl::NET_EOF => on_eof(c.id),
        tl::NET_WROTE => on_wrote(c.id, c.user, c.len, c.queued),
        tl::NET_SHUTDOWN => on_shutdown(c.id, c.user),
        tl::NET_CLOSED => on_closed(c.id),
        tl::NET_TIMER => on_timer(c.id),
        tl::NET_ERROR => {
            // SAFETY: same call; both point at `'static` string data.
            let (code, syscall) = unsafe { (c.code(), c.syscall()) };
            on_error(c.id, c.user, code, syscall, c.terminal != 0);
        }
        _ => {}
    }
}

fn on_connect(id: i64) {
    let local = tl::local_address(id);
    let peer = tl::peer_address(id);
    if let Ok(mut sockets) = statics::sockets().lock() {
        if let Some(s) = sockets.get_mut(&id) {
            s.is_open = true;
            s.local_addr = local.as_ref().and_then(endpoint_to_addr);
            s.remote_addr = peer.as_ref().and_then(endpoint_to_addr);
        }
    }
    let local_server = with_aux(id, |a| a.local_server.take());
    if let Some((servername, verify, config)) = with_aux(id, |a| a.direct_tls.take()) {
        if let Err(message) =
            crate::turnloop_tls_io::begin_client_upgrade(id, servername, verify, config, None)
        {
            server_state::cancel_pending_connection(id);
            push_event(PendingNetEvent::Error(id, message));
            destroy(id);
            return;
        }
        replay_held_tls(id);
    }
    // #10465: clear the connect-phase flags at the same tick the JS 'connect'
    // event is emitted, so a listener observing the socket sees Node's state.
    // Both tokio connect paths (lib.rs) set all three together; this one set
    // only `is_open`, so `connecting` stayed true forever and `readyState`
    // (lifecycle.rs:275 returns "opening" whenever `connecting`) never left
    // "opening" for the socket's whole connected life, while `pending`
    // (keyed on `has_opened`, lifecycle.rs:144) never cleared. A driver that
    // waits for `readyState === "open"` or guards on `!connecting` before
    // writing therefore never proceeds.
    //
    // Deliberately here rather than in the `is_open` block above: the
    // direct-TLS branch can fail and return early, and that socket is being
    // destroyed, so it must NOT be recorded as opened.
    //
    // DO NOT "fix" this to wait for the TLS handshake. It looks early — the
    // flags clear while a direct-TLS upgrade is still in flight — but it is
    // what Node does: a TLS socket's underlying connection completes at the
    // TCP level, which is when `'connect'` fires and `connecting` goes false,
    // and the handshake is signalled separately by `'secureConnect'`. The
    // deleted tokio path held `connecting` true until the transport INCLUDING
    // TLS was established; that was the deviation, not this. Nothing
    // pins it yet — the parity fixture is plain-socket only, so both timings
    // pass today (see #11056).
    if let Ok(mut sockets) = statics::sockets().lock() {
        if let Some(s) = sockets.get_mut(&id) {
            s.has_opened = true;
            s.connecting = false;
        }
    }
    push_event(PendingNetEvent::Connect(id, local_server));
    start_reading(id);
}

/// Encrypt and submit what a `tls.connect` socket was given while connecting,
/// in order, now that its session exists (`Aux::held_tls_writes`).
fn replay_held_tls(id: i64) {
    let (writes, end) = with_aux(id, |a| {
        (
            std::mem::take(&mut a.held_tls_writes),
            a.held_tls_end.take(),
        )
    });
    for (bytes, completion) in writes {
        match crate::turnloop_tls_io::write(id, &bytes, completion) {
            Ok(queued) => {
                if let Ok(mut sockets) = statics::sockets().lock() {
                    if let Some(s) = sockets.get_mut(&id) {
                        s.bytes_queued = queued as u64;
                    }
                }
            }
            Err(message) => {
                submission_failed(id, completion, message);
                return;
            }
        }
    }
    if let Some(completion) = end {
        if let Err(message) = crate::turnloop_tls_io::shutdown(id, completion) {
            submission_failed(id, completion, message);
        }
    }
}

fn endpoint_to_addr(endpoint: &tl::Endpoint) -> Option<std::net::SocketAddr> {
    endpoint
        .address
        .parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| std::net::SocketAddr::new(ip, endpoint.port))
}

fn on_accept(server_id: i64, socket_id: i64) {
    if socket_id == 0 {
        server_state::cancel_pending_connection(server_id);
        return;
    }
    let local = tl::local_address(socket_id)
        .as_ref()
        .and_then(endpoint_to_addr);
    let peer = tl::peer_address(socket_id)
        .as_ref()
        .and_then(endpoint_to_addr);
    if let Some(info) = server_state::should_drop_accepted(server_id, local, peer) {
        push_event(PendingNetEvent::ServerDrop(server_id, info));
        let _ = tl::close(socket_id);
        return;
    }
    crate::register_turnloop_socket(server_id, socket_id, local, peer);
    push_event(PendingNetEvent::ServerConnection(
        server_id, socket_id, false,
    ));
    start_reading(socket_id);
}

fn on_data(id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    // A TLS socket receives ciphertext. Decrypting here, inside the dispatch
    // call on the loop thread, keeps the rule that no JS value and no heap
    // pointer ever reaches the driver: plaintext is an owned `Vec` that the
    // plaintext path below copies into this crate's read pool exactly as it
    // would a cleartext read.
    if let Some(received) = crate::turnloop_tls_io::receive(id, bytes) {
        if !received.plaintext.is_empty() {
            deliver_plaintext(id, &received.plaintext);
        }
        if received.peer_closed {
            on_eof(id);
        }
        return;
    }
    deliver_plaintext(id, bytes);
}

fn deliver_plaintext(id: i64, bytes: &[u8]) {
    if let Ok(mut sockets) = statics::sockets().lock() {
        if let Some(s) = sockets.get_mut(&id) {
            s.bytes_read += bytes.len() as u64;
        }
    }
    // Copy out of turnloop's pooled lease into this crate's own read pool, so
    // the `Bytes` handed to the pump has the same ownership and lifetime the
    // tokio path gave it and the lease can go straight back.
    let mut buf = buffer_pool::checkout();
    buf.extend_from_slice(bytes);
    let chunk = buf.split_to(bytes.len()).freeze();
    buffer_pool::checkin(buf);
    if !raw_bridge::route_data(id, &chunk) {
        push_event(PendingNetEvent::Data(id, chunk));
    }
}

fn on_eof(id: i64) {
    if with_aux(id, |a| {
        a.closed_emitted || a.errored || std::mem::replace(&mut a.eof_emitted, true)
    }) {
        return;
    }
    if raw_bridge::mark_terminal(id, None) {
        // Raw (`http.Agent`) consumers own their own terminal state.
        destroy(id);
        return;
    }
    if awaiting_connection_callback(id) {
        with_aux(id, |a| a.deferred_eof = true);
        return;
    }
    with_aux(id, |a| a.read_ended = true);
    push_event(PendingNetEvent::End(id));
}

/// Whether this is an accepted socket whose `'connection'` callback has not
/// been dispatched yet.
fn awaiting_connection_callback(id: i64) -> bool {
    statics::sockets()
        .lock()
        .map(|sockets| {
            sockets
                .get(&id)
                .is_some_and(|s| s.server_id.is_some() && !s.server_connection_active)
        })
        .unwrap_or(false)
}

/// Deliver an `'end'` that was held for the `'connection'` callback.
///
/// Called from `release_connection_callback`, which holds the socket registry
/// lock — so this must not take it.
fn release_deferred_eof(id: i64) {
    if with_aux(id, |a| std::mem::replace(&mut a.deferred_eof, false)) {
        with_aux(id, |a| a.read_ended = true);
        push_event(PendingNetEvent::End(id));
    }
}

fn on_shutdown(id: i64, user: u64) {
    // Every byte queued ahead of the shutdown has left: turnloop orders a
    // handle's writes before its shutdown.
    let extra = with_aux(id, |a| {
        a.shutdown_done = true;
        std::mem::take(&mut a.extra_end_users)
    });
    push_event(PendingNetEvent::ShutdownComplete(id, user, None));
    for user in extra {
        push_event(PendingNetEvent::ShutdownComplete(id, user, None));
    }
    if with_aux(id, |a| {
        std::mem::replace(&mut a.close_after_shutdown, false)
    }) {
        destroy(id);
    }
}

fn on_wrote(id: i64, user: u64, len: usize, queued: usize) {
    // On a TLS socket `len` is ciphertext and `user` is always zero (the
    // ciphertext submission is not an application write). The layer maps the
    // acknowledgement back to the application writes it covers, so
    // `bytesWritten` stays plaintext and `write(chunk, cb)` still fires when
    // the bytes have left.
    if let Some(completed) = crate::turnloop_tls_io::wrote(id, len) {
        // `queued` is the driver's ciphertext count; `writableLength` is the
        // application bytes still outstanding, which the layer tracks.
        let _ = queued;
        let outstanding = crate::turnloop_tls_io::outstanding_plaintext(id);
        let mut drain = false;
        if let Ok(mut sockets) = statics::sockets().lock() {
            if let Some(s) = sockets.get_mut(&id) {
                s.bytes_queued = outstanding as u64;
                for (_, plain_len) in &completed {
                    s.bytes_written += *plain_len as u64;
                }
                drain = crate::lifecycle::take_drain(s);
            }
        }
        if drain {
            push_event(PendingNetEvent::Drain(id));
        }
        for (user, _) in completed {
            if user != 0 {
                push_event(PendingNetEvent::WriteComplete(id, user, None));
            }
        }
        return;
    }
    let mut drain = false;
    if let Ok(mut sockets) = statics::sockets().lock() {
        if let Some(s) = sockets.get_mut(&id) {
            s.bytes_written += len as u64;
            s.bytes_queued = queued as u64;
            drain = crate::lifecycle::take_drain(s);
        }
    }
    // #11111 — Node's `afterWrite` emits `'drain'` BEFORE the callbacks of
    // the writes that just completed.
    if drain {
        push_event(PendingNetEvent::Drain(id));
    }
    if user != 0 {
        push_event(PendingNetEvent::WriteComplete(id, user, None));
    }
}

fn on_closed(id: i64) {
    let is_server = statics::servers()
        .lock()
        .map(|servers| servers.contains_key(&id))
        .unwrap_or(false);
    if is_server {
        forget_aux(id);
        if let Ok(mut servers) = statics::servers().lock() {
            if let Some(server) = servers.get_mut(&id) {
                server.listening = false;
            }
        }
        push_event(PendingNetEvent::ServerClose(id));
        return;
    }
    emit_close_once(id);
}

fn on_error(id: i64, user: u64, code: Option<&str>, syscall: Option<&str>, terminal: bool) {
    let message = match (syscall, code) {
        (Some(syscall), Some(code)) if !syscall.is_empty() => format!("{syscall} {code}"),
        (_, Some(code)) => code.to_string(),
        _ => "UNKNOWN".to_string(),
    };
    let is_server = statics::servers()
        .lock()
        .map(|servers| servers.contains_key(&id))
        .unwrap_or(false);
    if is_server {
        push_event(PendingNetEvent::ServerError(id, message));
        // A transient accept failure (EMFILE, a peer that reset between the
        // SYN and the accept) does not end the listener — the tokio accept
        // loop deliberately kept going on one too, and Node does the same.
        if terminal {
            close_server(id);
        }
        return;
    }
    if user != 0 {
        push_event(PendingNetEvent::WriteComplete(
            id,
            user,
            Some(message.clone()),
        ));
    }
    // One `'error'` per socket, as the tokio task gave by breaking its loop.
    // `'close'` is NOT suppressed with it: Node emits close after error, and
    // it arrives from the driver's own terminal `Closed`.
    if !with_aux(id, |a| std::mem::replace(&mut a.errored, true))
        && !raw_bridge::mark_terminal(id, Some(message.clone()))
    {
        push_event(PendingNetEvent::Error(id, message));
    }
    destroy(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands_with_no_driver_equivalent_succeed_rather_than_fall_through() {
        // A command that returned `Err` here would be reported to JS as a
        // socket error. The server-ready marker has no turnloop submission and
        // must stay a silent no-op.
        let mut queued = None;
        assert!(super::command(-1, SocketCommand::ServerConnectionReady, &mut queued).is_ok());
        assert_eq!(queued, None, "a non-write never reports a queue length");
    }

    #[test]
    fn the_subsystem_slot_is_within_the_runtime_registry() {
        // `register_sink` refuses an out-of-range slot; a binding that picked
        // one would register nothing and look like a socket with no events.
        assert!((SUBSYSTEM as usize) < 4);
    }
}
