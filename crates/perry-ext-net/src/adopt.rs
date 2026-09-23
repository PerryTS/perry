//! #4973 — adopt an already-connected TCP stream as a `net.Socket`
//! (the HTTP raw-`'upgrade'` handoff), plus the base64 helper the
//! string-encoding data delivery uses. Split out of `lib.rs` to keep it
//! under the 2000-line CI gate.

use crate::{
    dispatch, ensure_gc_scanner_registered, mark_closed, next_id, push_event, statics,
    turnloop_io, PendingNetEvent, SocketState,
};
use perry_ffi::turnloop_net as tl;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Adopt an already-connected TCP stream as a `net.Socket` handle.
///
/// perry-ext-http's raw `'upgrade'` paths (#4973) call this: Node hands the
/// `'upgrade'` listener the raw connection socket with nothing written to it,
/// so the HTTP side peels the response or request head off a stream *another*
/// transport opened and passes the live stream here. The returned id drives
/// the standard socket surface (`write` / `end` / `on('data')` / …) exactly
/// like a socket accepted by `net.createServer`.
///
/// The stream becomes a turnloop socket: its descriptor is handed to the
/// agent's loop (`turnloop_net::adopt_stream`) and read there, so this crate
/// needs no socket task of its own for it. That happens on the loop's owner —
/// the caller is a worker thread of the transport that opened the stream,
/// which must never claim a loop — so the stream is parked in
/// [`pending_adoptions`] and a job to adopt it is *posted*, and the id is
/// returned before it has run.
///
/// Whichever reaches the parked stream first adopts it: the posted job, or
/// [`ensure_adopted_socket_dispatch`] on the owner, which the HTTP side calls
/// from its upgrade-event drain before any listener can touch the socket. That
/// second route is what makes the order safe: without it, a drain that ran
/// before the owner's next turn would hand JS a socket the loop had not adopted
/// yet, and its first `write()` would find no handle.
///
/// Does NOT register the GC scanner or the runtime dispatch extensions — those
/// are main-thread-affine; call `ensure_adopted_socket_dispatch()` from the
/// main thread (the upgrade-event drain does) before user code touches the
/// socket.
///
/// Returns `INVALID_HANDLE`, with the stream closed, when the handle-id band is
/// exhausted or no loop exists for this agent; the caller aborts the upgrade.
pub fn adopt_upgraded_tcp_stream(stream: std::net::TcpStream) -> i64 {
    let id = next_id();
    // #6441: called from a background thread, so exhaustion can't throw to a
    // JS frame here. Drop the upgraded stream and return the `0` sentinel
    // rather than register a phantom socket under it; the caller aborts the
    // upgrade when it sees `INVALID_HANDLE`.
    if id == perry_ffi::INVALID_HANDLE {
        drop(stream);
        return perry_ffi::INVALID_HANDLE;
    }
    let local = stream.local_addr().ok();
    let remote = stream.peer_addr().ok();
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            tcp_async_id: 0,
            connect_async_id: 0,
            shutdown_async_id: 0,
            awaiting_connect: false,
            is_open: true,
            raw_fd: None,
            refed: true,
            local_addr: local,
            remote_addr: remote,
            raw: None,
            destroyed: false,
            connecting: false,
            has_opened: true,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: Default::default(),
            turnloop: true,
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    #[cfg(any(unix, windows))]
    let socket: tl::AdoptedSocket = stream.into();
    #[cfg(not(any(unix, windows)))]
    let socket = stream;
    pending_adoptions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, socket);
    // Not `turnloop_io::on_loop`: that asks whether *this* thread owns the
    // loop, and asking is what claims it. See `turnloop_io::post_to_owner`.
    let posted = turnloop_io::post_to_owner(Box::new(move || complete_adoption(id)));
    if !posted {
        // Closes the parked stream with it.
        pending_adoptions()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        forget_socket(id);
        return perry_ffi::INVALID_HANDLE;
    }
    id
}

#[cfg(any(unix, windows))]
type ParkedStream = tl::AdoptedSocket;
#[cfg(not(any(unix, windows)))]
type ParkedStream = std::net::TcpStream;

/// Streams handed over by [`adopt_upgraded_tcp_stream`] that the loop has not
/// adopted yet, keyed by the socket id already returned for them.
fn pending_adoptions() -> &'static Mutex<HashMap<i64, ParkedStream>> {
    static PENDING: OnceLock<Mutex<HashMap<i64, ParkedStream>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Adopt the stream parked for `id`, if nobody has yet. Runs on the loop's
/// owner, with no registry lock held.
fn complete_adoption(id: i64) {
    let parked = pending_adoptions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    if let Some(socket) = parked {
        adopt_on_loop(id, socket);
    }
}

/// The loop-side half of [`adopt_upgraded_tcp_stream`]: hand the descriptor to
/// the driver under `id` and start reading it.
#[cfg(any(unix, windows))]
fn adopt_on_loop(id: i64, socket: tl::AdoptedSocket) {
    // Make sure this crate's sink is installed before the first completion
    // for `id` can exist; `enabled` is also the ownership check, and on this
    // thread (the loop's owner) it is true.
    if !turnloop_io::enabled() {
        drop(socket);
        fail_adopted(id, turnloop_io::NO_LOOP_CODE.to_string());
        return;
    }
    match tl::adopt_stream(id, turnloop_io::SUBSYSTEM, socket) {
        Ok(()) => turnloop_io::start_reading(id),
        Err(err) => fail_adopted(id, err.message()),
    }
}

#[cfg(not(any(unix, windows)))]
fn adopt_on_loop(id: i64, socket: std::net::TcpStream) {
    drop(socket);
    fail_adopted(id, turnloop_io::NO_LOOP_CODE.to_string());
}

/// The adoption was refused after the id was handed out: report it on the
/// socket the caller already published.
fn fail_adopted(id: i64, message: String) {
    push_event(PendingNetEvent::Error(id, message));
    push_event(PendingNetEvent::Close(id));
    mark_closed(id);
}

/// Undo the registration of a socket whose adoption could not even be posted.
fn forget_socket(id: i64) {
    statics::sockets().lock().unwrap().remove(&id);
    statics::listeners().lock().unwrap().remove(&id);
}

/// Adopt an already-accepted **turnloop** connection as a `net.Socket` (P5).
///
/// The HTTP `'upgrade'` handoff with no descriptor moving: the runtime's
/// `turnloop_net::transfer` has already pointed the connection's completions
/// at this crate's sink, keeping the id and the outstanding multishot read, so
/// the next byte arrives here with no gap and no resubmission. All this has to
/// do is publish the JS-visible socket record under the same id.
///
/// Called on the loop thread from perry-ext-http's completion sink, so it may
/// touch the registries directly but must not build JS values — it doesn't.
pub fn adopt_turnloop_upgrade(id: i64) -> bool {
    if id == perry_ffi::INVALID_HANDLE {
        return false;
    }
    let local = crate::turnloop_io::local_endpoint(id)
        .as_ref()
        .and_then(endpoint_to_addr);
    let remote = perry_ffi::turnloop_net::peer_address(id)
        .as_ref()
        .and_then(endpoint_to_addr);
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            tcp_async_id: 0,
            connect_async_id: 0,
            shutdown_async_id: 0,
            awaiting_connect: false,
            is_open: true,
            raw_fd: None,
            refed: true,
            local_addr: local,
            remote_addr: remote,
            raw: None,
            destroyed: false,
            // #10465: an accepted socket never went through a connect attempt
            // and is open the moment it is registered, so it starts
            // `connecting: false` / `has_opened: true`; neither half has been
            // ended yet.
            connecting: false,
            has_opened: true,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: Default::default(),
            turnloop: true,
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    true
}

fn endpoint_to_addr(endpoint: &perry_ffi::turnloop_net::Endpoint) -> Option<std::net::SocketAddr> {
    endpoint
        .address
        .parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| std::net::SocketAddr::new(ip, endpoint.port))
}

/// Main-thread companion to `adopt_upgraded_tcp_stream`: registers the GC
/// root scanner and the runtime handle-dispatch/pump extensions so an
/// adopted socket's methods, events, and liveness work even when no other
/// `js_net_*` entry point has run yet (an http-only program receiving a raw
/// upgrade).
///
/// On the thread that owns the agent's loop it also adopts every stream still
/// parked by [`adopt_upgraded_tcp_stream`], so the socket the caller is about
/// to hand to JS is already on the loop (see that function's note on order).
pub fn ensure_adopted_socket_dispatch() {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    if turnloop_io::enabled() {
        let parked: Vec<i64> = pending_adoptions()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
        for id in parked {
            complete_adoption(id);
        }
    }
}

/// Minimal standard-alphabet base64 (with padding) for `setEncoding('base64')`
/// data delivery — avoids pulling a base64 crate into perry-ext-net.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHA[(n >> 18) as usize & 63] as char);
        out.push(ALPHA[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHA[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHA[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
