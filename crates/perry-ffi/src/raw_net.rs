//! Cross-crate "raw socket" bridge (#2154).
//!
//! `http.Agent` lets user code override how a connection is produced
//! (`agent.createConnection = (opts, cb) => net.connect(...)`). When
//! `http.request({ agent })` services a request, Node writes the raw
//! HTTP bytes onto whatever `net.Socket` the override returned. To honor
//! that in Perry, `perry-ext-http` has to drive byte-level read/write
//! over a socket that `perry-ext-net` created and owns.
//!
//! The two crates are linked as independent well-known-flip staticlibs
//! (`node:http` → perry-ext-http, `node:net` → perry-ext-net). They have
//! no Cargo edge between them and can't share Rust channel types across
//! the boundary. So perry-ext-net publishes a small C-ABI vtable into
//! this slot at startup, and perry-ext-http looks it up at request time:
//!
//! - If net is linked (any program that calls `net.connect`, which an
//!   `agent.createConnection` override does), the slot is populated and
//!   the HTTP request flows over the user's socket.
//! - If net is *not* linked (an http-only binary), the slot stays empty
//!   and perry-ext-http falls back to its own transport. No unconditional
//!   `extern` edge is created, so http-only programs still link.
//!
//! The vtable speaks only C-ABI primitives (socket id, byte pointers,
//! lengths), keeping the layering clean and the linkage decoupled.

use std::sync::OnceLock;

/// C-ABI surface a `net` backend exposes so an `http` backend can drive
/// raw I/O over a socket it created. All functions take the `i64` socket
/// handle id that `net.connect` / `net.createConnection` returned.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RawNetVtable {
    /// Switch the socket into "raw" mode: inbound bytes are buffered for
    /// [`RawNetVtable::poll_read`] instead of being dispatched to the
    /// socket's JS `'data'` listeners. Returns `1` on success, `0` if the
    /// handle is unknown. Idempotent.
    pub attach: extern "C" fn(socket_id: i64) -> i32,
    /// Enqueue `len` bytes at `ptr` to be written to the socket. Returns
    /// `1` if queued, `0` on an unknown/closed handle. The write is
    /// buffered if the socket hasn't finished connecting yet.
    pub write: extern "C" fn(socket_id: i64, ptr: *const u8, len: usize) -> i32,
    /// Drain up to `max` buffered inbound bytes into `out`. Returns the
    /// number of bytes copied (`> 0`), `0` for clean EOF once the buffer
    /// is drained and the peer closed, or `-1` when no bytes are
    /// currently available but the socket is still open ("would block").
    pub poll_read: extern "C" fn(socket_id: i64, out: *mut u8, max: usize) -> isize,
    /// Tear the socket down (sends the equivalent of `socket.destroy()`).
    pub close: extern "C" fn(socket_id: i64),
}

// SAFETY: the vtable holds only `extern "C" fn` pointers into
// program-global code; sharing it across threads is sound.
unsafe impl Send for RawNetVtable {}
unsafe impl Sync for RawNetVtable {}

static RAW_NET: OnceLock<RawNetVtable> = OnceLock::new();

/// Publish the raw-net vtable. Called once by `perry-ext-net` from a
/// guaranteed-early entry point (e.g. when the first socket is created).
/// Subsequent calls are ignored — the first registration wins.
pub fn register_raw_net(vtable: RawNetVtable) {
    let _ = RAW_NET.set(vtable);
}

/// Fetch the registered raw-net vtable, or `None` when no `net` backend
/// is linked into this binary. `perry-ext-http` uses this to decide
/// whether it can honor an `agent.createConnection` override.
pub fn raw_net() -> Option<&'static RawNetVtable> {
    RAW_NET.get()
}
