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
//! [`enabled`] is false on a second thread acting for an agent another
//! thread already owns — turnloop P9 gave every JS agent a loop, so a
//! `worker_threads` Worker is no longer a reason it is false — and the
//! route is claimed once per thread by the first to ask
//! (`event_pump::agent_loop::claim_route`). That keeps the tokio path.
//!
//! **TLS no longer does.** This paragraph used to say a socket that might be
//! upgraded was created on tokio and stayed there for life, because
//! `socket.upgradeToTLS` moved a live `TcpStream` into `tokio_rustls` and
//! turnloop owns its descriptor without exposing it. P5 removed the premise
//! rather than the restriction: the rustls session runs *above* the turnloop
//! handle (`turnloop_tls_io`), so the upgrade needs no descriptor, and both
//! `tls.connect` and `socket.upgradeToTLS` come here. `lib.rs`'s connect sites
//! have said so since P5; this header did not.
//!
//! There is no handover in either direction — a socket belongs to one transport
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
    /// The readable EOF has been delivered. A TLS socket can reach it twice —
    /// the peer's `close_notify` and then the TCP FIN — and Node emits
    /// `'end'` exactly once.
    eof_emitted: bool,
    /// P5: `tls.connect` asked for TLS from byte zero. The session is
    /// installed the instant the connect completes and before `'connect'` is
    /// pushed, which is the ordering the tokio path got by handshaking before
    /// it pushed the event.
    direct_tls: Option<(String, bool, crate::TlsClientConfigData)>,
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
    let secure = crate::turnloop_tls_io::installed(id);
    match cmd {
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
        SocketCommand::End(completion) if secure => {
            with_aux(id, |a| a.write_ended = true);
            crate::turnloop_tls_io::shutdown(id, completion)
        }
        SocketCommand::End(completion) => {
            with_aux(id, |a| a.write_ended = true);
            tl::shutdown(id, completion).map_err(|e| e.message())
        }
        SocketCommand::Destroy => tl::close(id).map_err(|e| e.message()),
        // TCP_NODELAY is settable on a turnloop socket only at creation
        // (`TcpOpts`), which covers the paths P1 moves. An accepted connection
        // and a later `socket.setNoDelay()` have no turnloop API to reach, so
        // the call keeps Node's chainable semantics — the flag is not
        // observable from JS. Needs a turnloop socket-option API to finish.
        SocketCommand::SetNoDelay(_) => Ok(()),
        // The accepted socket's `'connection'` callback has returned, so its
        // listeners exist: release an EOF that arrived before them.
        SocketCommand::ServerConnectionReady => {
            release_deferred_eof(id);
            Ok(())
        }
        // P5: `UpgradeTls` never reaches here — `js_net_socket_upgrade_tls`
        // and `tls::begin_tls_upgrade` branch on the transport and install a
        // `turnloop_tls_io` layer directly, because the upgrade's result is a
        // promise settled on the loop thread rather than a channel reply.
        // Anything else is a command with no turnloop meaning.
        _ => Err("unsupported socket command on a turnloop socket".to_string()),
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
    if let Some((servername, verify, config)) = with_aux(id, |a| a.direct_tls.take()) {
        if let Err(message) =
            crate::turnloop_tls_io::begin_client_upgrade(id, servername, verify, config, None)
        {
            server_state::cancel_pending_connection(id);
            push_event(PendingNetEvent::Error(id, message));
            destroy(id);
            return;
        }
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
    // tokio path at lib.rs:1494 holds `connecting` true until the transport
    // INCLUDING TLS is established; that is the deviation, not this. Nothing
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
    with_aux(id, |a| a.shutdown_done = true);
    push_event(PendingNetEvent::ShutdownComplete(id, user, None));
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
        if let Ok(mut sockets) = statics::sockets().lock() {
            if let Some(s) = sockets.get_mut(&id) {
                s.bytes_queued = queued as u64;
                for (_, plain_len) in &completed {
                    s.bytes_written += *plain_len as u64;
                }
            }
        }
        for (user, _) in completed {
            if user != 0 {
                push_event(PendingNetEvent::WriteComplete(id, user, None));
            }
        }
        return;
    }
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
