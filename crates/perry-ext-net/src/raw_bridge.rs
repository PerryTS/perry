//! #2154 — raw-consumer bridge for `http.Agent.createConnection`.
//!
//! `perry-ext-http` drives a full HTTP/1.1 exchange over a socket the user's
//! `agent.createConnection` override produced (typically a `net.connect(...)`
//! result). The C-ABI entry points here are published into perry-ffi's
//! raw-net slot (see [`register`]) so http can attach/write/read/close the
//! socket without a Cargo edge to this crate. They speak only raw bytes — the
//! HTTP framing lives in perry-ext-http.
//!
//! Split out of `lib.rs` to keep that file under the 2000-line size gate.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::{statics, SocketCommand};

/// Backing buffer for a socket in raw-consumer mode. `run_socket_task` pushes
/// inbound bytes into `buf`; [`perry_net_raw_poll_read`] drains them. `closed`
/// flips on peer-FIN / destroy / error so a drained reader sees EOF; `error`
/// carries the transport error message if one occurred.
#[derive(Default)]
pub(crate) struct RawReadState {
    pub(crate) buf: VecDeque<u8>,
    pub(crate) closed: bool,
    #[allow(dead_code)]
    pub(crate) error: Option<String>,
}

/// Return the raw-consumer buffer for `id` if the socket is in raw mode (an
/// `http.request` over an `agent.createConnection` socket), else `None`.
/// Cloning the `Arc` is cheap and lets `run_socket_task` route bytes without
/// holding the sockets-map lock across the buffer mutation.
pub(crate) fn raw_state_for(id: i64) -> Option<Arc<Mutex<RawReadState>>> {
    statics::sockets()
        .lock()
        .ok()
        .and_then(|g| g.get(&id).and_then(|s| s.raw.clone()))
}

/// Flip a raw-mode socket's buffer to closed (recording `error` if any) so a
/// draining `poll_read` observes EOF. No-op for JS-mode sockets.
fn raw_mark_closed(raw: &Arc<Mutex<RawReadState>>, error: Option<String>) {
    if let Ok(mut st) = raw.lock() {
        st.closed = true;
        if error.is_some() {
            st.error = error;
        }
    }
}

/// `run_socket_task` hook: route an inbound chunk for socket `id`. Returns
/// `true` if the socket is in raw mode (bytes buffered for `poll_read`),
/// `false` if the caller should emit a JS `'data'` event instead.
pub(crate) fn route_data(id: i64, bytes: &[u8]) -> bool {
    match raw_state_for(id) {
        Some(raw) => {
            if let Ok(mut st) = raw.lock() {
                st.buf.extend(bytes.iter().copied());
            }
            true
        }
        None => false,
    }
}

/// `run_socket_task` hook: mark socket `id` terminal (EOF / destroy / error).
/// Returns `true` if the socket is in raw mode (buffer flagged closed, no JS
/// events), `false` if the caller should emit the JS End/Close or Error/Close
/// events. `error` is recorded on the buffer in raw mode.
pub(crate) fn mark_terminal(id: i64, error: Option<String>) -> bool {
    match raw_state_for(id) {
        Some(raw) => {
            raw_mark_closed(&raw, error);
            true
        }
        None => false,
    }
}

/// Switch socket `socket_id` into raw-consumer mode. Inbound bytes are
/// buffered for [`perry_net_raw_poll_read`] instead of dispatched to JS
/// `'data'` listeners. Returns `1` on success, `0` if the handle is unknown.
extern "C" fn perry_net_raw_attach(socket_id: i64) -> i32 {
    if let Ok(mut g) = statics::sockets().lock() {
        if let Some(s) = g.get_mut(&socket_id) {
            if s.raw.is_none() {
                s.raw = Some(Arc::new(Mutex::new(RawReadState::default())));
            }
            return 1;
        }
    }
    0
}

/// Enqueue `len` bytes at `ptr` to be written to socket `socket_id`. The write
/// is buffered by the socket's command channel and flushed once the connection
/// is established. Returns `1` if queued, `0` otherwise.
extern "C" fn perry_net_raw_write(socket_id: i64, ptr: *const u8, len: usize) -> i32 {
    let bytes = if len == 0 {
        Vec::new()
    } else if ptr.is_null() {
        return 0;
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    };
    if let Ok(g) = statics::sockets().lock() {
        if let Some(s) = g.get(&socket_id) {
            return i32::from(s.cmd_tx.send(SocketCommand::Write(bytes)).is_ok());
        }
    }
    0
}

/// Drain up to `max` buffered inbound bytes from socket `socket_id` into `out`.
/// Returns the byte count (`> 0`), `0` for clean EOF once drained and the peer
/// closed, or `-1` when nothing is available but the socket is still open
/// ("would block" — the caller should yield and retry).
extern "C" fn perry_net_raw_poll_read(socket_id: i64, out: *mut u8, max: usize) -> isize {
    if out.is_null() || max == 0 {
        return -1;
    }
    let raw = match raw_state_for(socket_id) {
        Some(r) => r,
        None => return -1,
    };
    let mut st = match raw.lock() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    if st.buf.is_empty() {
        return if st.closed { 0 } else { -1 };
    }
    let n = st.buf.len().min(max);
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out, n) };
    for slot in out_slice.iter_mut() {
        // `pop_front` can't fail: we just bounded `n` by `buf.len()`.
        *slot = st.buf.pop_front().unwrap_or(0);
    }
    n as isize
}

/// Tear down socket `socket_id` (equivalent to `socket.destroy()`) and
/// unregister it. `perry-ext-http` calls this once the HTTP exchange is done
/// draining. Because raw mode suppresses the JS `Close` event (the path that
/// normally removes the socket from the registry in `js_net_process_pending`),
/// we must remove it here — otherwise the lingering entry keeps
/// `js_net_has_pending` / `js_ext_net_has_active_handles` returning 1 and the
/// program never exits.
extern "C" fn perry_net_raw_close(socket_id: i64) {
    // Signal the read/write task to stop (best-effort; dropping the
    // SocketState below also closes the cmd channel, ending the task).
    if let Ok(g) = statics::sockets().lock() {
        if let Some(s) = g.get(&socket_id) {
            let _ = s.cmd_tx.send(SocketCommand::Destroy);
        }
    }
    let _ = statics::sockets().lock().map(|mut g| g.remove(&socket_id));
    let _ = statics::listeners()
        .lock()
        .map(|mut g| g.remove(&socket_id));
    let _ = statics::once_flags()
        .lock()
        .map(|mut g| g.remove(&socket_id));
}

/// Publish the raw-net vtable into perry-ffi's slot so perry-ext-http can drive
/// an HTTP exchange over a socket produced by `agent.createConnection`. Called
/// once from the net GC-scanner init (first net FFI entry, i.e. any socket
/// creation), so the vtable is in place before http could reference a socket.
pub(crate) fn register() {
    perry_ffi::register_raw_net(perry_ffi::RawNetVtable {
        attach: perry_net_raw_attach,
        write: perry_net_raw_write,
        poll_read: perry_net_raw_poll_read,
        close: perry_net_raw_close,
    });
}
