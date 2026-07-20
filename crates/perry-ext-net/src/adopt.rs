//! #4973 — adopt an already-connected TCP stream as a `net.Socket`
//! (the HTTP raw-`'upgrade'` handoff), plus the base64 helper the
//! string-encoding data delivery uses. Split out of `lib.rs` to keep it
//! under the 2000-line CI gate.

use crate::{
    dispatch, ensure_gc_scanner_registered, next_id, run_socket_task, statics, SocketCommand,
    SocketState, Transport,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Adopt an already-connected TCP stream as a `net.Socket` handle.
///
/// perry-ext-http-server's raw `'upgrade'` path (#4973) calls this: Node
/// hands the `'upgrade'` listener the raw connection socket with nothing
/// written to it, so the HTTP accept task peels the request head off the
/// stream and passes the live stream here. The returned id drives the
/// standard socket surface (`write` / `end` / `on('data')` / …) through the
/// existing `run_socket_task` + main-thread pump machinery, exactly like a
/// socket accepted by `net.createServer`.
///
/// Must be called from within a tokio runtime context (the HTTP accept task
/// qualifies); the per-socket IO loop is spawned on that runtime. Does NOT
/// register the GC scanner or the runtime dispatch extensions — those are
/// main-thread-affine; call `ensure_adopted_socket_dispatch()` from the
/// main thread (the upgrade-event drain does) before user code touches the
/// socket.
pub fn adopt_upgraded_tcp_stream(stream: tokio::net::TcpStream) -> i64 {
    let id = next_id();
    // #6441: called from the HTTP accept task (a background tokio thread), so
    // exhaustion can't throw to a JS frame here. Drop the upgraded stream and
    // return the `0` sentinel rather than register a phantom socket under it;
    // the caller aborts the upgrade when it sees `INVALID_HANDLE`.
    if id == perry_ffi::INVALID_HANDLE {
        drop(stream);
        return perry_ffi::INVALID_HANDLE;
    }
    let (tx, rx) = mpsc::unbounded_channel::<SocketCommand>();
    let local = stream.local_addr().ok();
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            cmd_tx: tx,
            pending_rx: None,
            is_open: true,
            local_addr: local,
            raw: None,
            destroyed: false,
            bytes_read: 0,
            bytes_written: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    tokio::spawn(async move {
        let mut rx = rx;
        run_socket_task(id, Transport::Plain(stream), &mut rx).await;
    });
    id
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
