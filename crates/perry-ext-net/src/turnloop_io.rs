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
//! [`enabled`] is false on a `worker_threads` agent (no loop until P3/P4) and
//! in the `tokio-wait-driver` A/B arm, so those keep the tokio path. TLS also
//! keeps it: `socket.upgradeToTLS` moves a live `TcpStream` into
//! `tokio_rustls`, and turnloop owns its descriptor without exposing it, so a
//! socket that may be upgraded is created on tokio and stays there for its
//! whole life. There is no handover in either direction — a socket belongs to
//! one transport from creation to close.
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
    /// An `'error'` has been reported for this socket. The tokio task broke
    /// its loop after the first one; this keeps that "one error, then the
    /// terminal pair" shape when several operations fail in the same turn.
    errored: bool,
    /// `PendingNetEvent::Close` has been pushed. Node emits `'close'` AFTER
    /// `'error'`, so this guards double-emission — never emission itself.
    closed_emitted: bool,
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
/// Deliberately not cached: availability is a property of the calling agent,
/// not of the process. A `worker_threads` Worker has no loop before P3/P4, and
/// caching its "no" would strand the primary agent on tokio for the rest of
/// the run. Registration behind it is idempotent and costs one atomic once it
/// has happened.
pub(crate) fn enabled() -> bool {
    if !REGISTERED.load(Ordering::Acquire) {
        // `register_sink` refuses if the runtime's completion layout does not
        // match this crate's, which leaves `available` false and keeps every
        // socket on tokio rather than submitting work nothing can deliver.
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
    match cmd {
        SocketCommand::Write(bytes, completion) => match tl::write(id, &bytes, completion) {
            Ok(queued) => {
                *queued_out = Some(queued as u64);
                Ok(())
            }
            Err(err) => Err(err.message()),
        },
        SocketCommand::End(completion) => tl::shutdown(id, completion).map_err(|e| e.message()),
        SocketCommand::Destroy => tl::close(id).map_err(|e| e.message()),
        // TCP_NODELAY is settable on a turnloop socket only at creation
        // (`TcpOpts`), which covers the paths P1 moves. An accepted connection
        // and a later `socket.setNoDelay()` have no turnloop API to reach, so
        // the call keeps Node's chainable semantics — the flag is not
        // observable from JS. Needs a turnloop socket-option API to finish.
        SocketCommand::SetNoDelay(_) => Ok(()),
        // The tokio task used this to know when a deferred `'connection'`
        // callback had run before deciding how long to wait for writes after
        // EOF. Nothing defers here: submissions go straight to the driver.
        SocketCommand::ServerConnectionReady => Ok(()),
        // Only `UpgradeTls` (and the test-only probe) reach this, and a
        // turnloop socket is never TLS-upgradable.
        _ => Err("TLS upgrade is unsupported on a turnloop socket".to_string()),
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
    if !with_aux(id, |a| std::mem::replace(&mut a.read_ended, false)) {
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

/// Start a local (Unix socket / named pipe) client connect.
pub(crate) fn connect_pipe(id: i64, path: &str) -> Result<(), tl::NetError> {
    tl::pipe_connect(id, SUBSYSTEM, path)
}

/// Bind, listen and start accepting on a TCP server.
pub(crate) fn listen_tcp(id: i64, host: &str, port: u16, backlog: u32) -> Result<(), tl::NetError> {
    tl::tcp_listen(id, SUBSYSTEM, host, port, backlog, false)?;
    tl::accept_start(id)
}

/// Bind, listen and start accepting on a local server.
pub(crate) fn listen_pipe(id: i64, path: &str, backlog: u32) -> Result<(), tl::NetError> {
    tl::pipe_listen(id, SUBSYSTEM, path, backlog)?;
    tl::accept_start(id)
}

/// `server.close()` on a turnloop-backed server.
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
    push_event(PendingNetEvent::Connect(id, local_server));
    start_reading(id);
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
    if with_aux(id, |a| a.closed_emitted || a.errored) {
        return;
    }
    if raw_bridge::mark_terminal(id, None) {
        // Raw (`http.Agent`) consumers own their own terminal state.
        destroy(id);
        return;
    }
    with_aux(id, |a| a.read_ended = true);
    push_event(PendingNetEvent::End(id));
}

fn on_shutdown(id: i64, user: u64) {
    push_event(PendingNetEvent::ShutdownComplete(id, user, None));
    if with_aux(id, |a| {
        std::mem::replace(&mut a.close_after_shutdown, false)
    }) {
        destroy(id);
    }
}

fn on_wrote(id: i64, user: u64, len: usize, queued: usize) {
    if let Ok(mut sockets) = statics::sockets().lock() {
        if let Some(s) = sockets.get_mut(&id) {
            s.bytes_written += len as u64;
            s.bytes_queued = queued as u64;
        }
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
        // socket error. `setNoDelay` and the server-ready marker have no
        // turnloop submission and must stay silent no-ops; only the TLS
        // upgrade is a real refusal.
        let mut queued = None;
        assert!(super::command(-1, SocketCommand::SetNoDelay(true), &mut queued).is_ok());
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
