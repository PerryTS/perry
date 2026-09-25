//! #4973 — adopt an already-connected TCP stream as a `net.Socket`
//! (the HTTP raw-`'upgrade'` handoff), plus the base64 helper the
//! string-encoding data delivery uses. Split out of `lib.rs` to keep it
//! under the 2000-line CI gate.

use crate::{dispatch, ensure_gc_scanner_registered, next_id, statics, turnloop_io, SocketState};
#[cfg(any(unix, windows))]
use perry_ffi::turnloop_net as tl;
use std::collections::HashMap;

/// Adopt an already-connected TCP stream as a `net.Socket` handle.
///
/// Node hands an `'upgrade'` listener the raw connection socket with nothing
/// written to it; this publishes a live stream some other code connected under
/// the standard socket surface (`write` / `end` / `on('data')` / …), exactly
/// like a socket accepted by `net.createServer`. perry-ext-http's own raw
/// `'upgrade'` paths no longer come here — they keep the connection on the
/// loop and move it with `turnloop_net::transfer` ([`adopt_turnloop_upgrade`]).
///
/// The stream becomes a turnloop socket: its descriptor is handed to the
/// agent's loop (`turnloop_net::adopt_stream`) and read there, **synchronously,
/// on the calling thread**, which must be a JS agent thread that owns that
/// loop — never a transport's worker thread, which asking would make claim a
/// loop nobody turns (`turnloop_io::post_to_owner`). There is deliberately no
/// posting route (#11155). The one this replaced parked the stream in a
/// process-wide map and posted the adoption without naming an agent, so a
/// caller on a tokio worker — which acts for no agent — reached the *primary*
/// agent's loop whoever owned the connection, and any loop draining that map
/// could adopt another agent's stream onto its own thread and heap. A socket
/// belongs to one loop from creation to close; the caller that knows which
/// loop that is is the one that has to be on it.
///
/// Returns `INVALID_HANDLE`, with the stream closed and nothing registered,
/// when the handle-id band is exhausted, when this thread does not own its
/// agent's loop, or when the loop refuses the descriptor. The caller must treat
/// that as a failed upgrade (Node emits `'error'`), never hand JS the `0` as a
/// socket. Call [`ensure_adopted_socket_dispatch`] before user code touches a
/// socket this returns.
pub fn adopt_upgraded_tcp_stream(stream: std::net::TcpStream) -> i64 {
    // Asked before an id is spent: `enabled` is the ownership check, and a
    // `false` means there is no loop here to put the descriptor on.
    if !turnloop_io::enabled() {
        drop(stream);
        return perry_ffi::INVALID_HANDLE;
    }
    let id = next_id();
    // #6441: exhaustion can't throw to a JS frame from here. Drop the stream
    // and return the `0` sentinel rather than register a phantom socket.
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
            need_drain: false,
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
    if !adopt_on_loop(id, stream) {
        forget_socket(id);
        return perry_ffi::INVALID_HANDLE;
    }
    id
}

/// Hand the descriptor to this thread's loop under `id` and start reading it.
/// `false`, with the stream closed, if the driver refused it.
#[cfg(any(unix, windows))]
fn adopt_on_loop(id: i64, stream: std::net::TcpStream) -> bool {
    let socket: tl::AdoptedSocket = stream.into();
    match tl::adopt_stream(id, turnloop_io::SUBSYSTEM, socket) {
        Ok(()) => {
            turnloop_io::start_reading(id);
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(any(unix, windows)))]
fn adopt_on_loop(_id: i64, stream: std::net::TcpStream) -> bool {
    drop(stream);
    false
}

/// Undo the registration of a socket the loop would not take.
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
            need_drain: false,
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
pub fn ensure_adopted_socket_dispatch() {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
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
