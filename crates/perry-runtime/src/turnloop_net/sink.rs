//! The subsystem registry: where a turnloop completion goes once the driver
//! has handed it back.
//!
//! A *subsystem* is one linked net binding — today `perry-ext-net`, with slots
//! reserved for the bundled stdlib `net` and for later phases. It registers
//! two function pointers:
//!
//! * a **sink**, called once per completion with a borrowed [`NetCompletion`];
//! * an **id allocator**, called when turnloop accepts a connection, because
//!   the accepted socket has to be named in the binding's own JS-visible
//!   handle space (`perry_ffi::reserve_handle_id_in_domain`), which this crate
//!   cannot allocate from.
//!
//! Both are plain `extern "C" fn`s in process-global slots, the same shape the
//! event pump already uses for its wait driver and aux pumps — a binding is a
//! separately linked `staticlib`, so there is no Rust type to share.
//!
//! The slots are process-global but every *call* happens on the loop-owning
//! thread inside `dispatch`, which is what makes it sound for a sink to
//! allocate JS values.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicPtr, Ordering};

use super::NodeError;

/// How many net bindings can be linked at once.
///
/// The slots are claimed, not merely reserved: 0 is `perry-ext-net` (P1), 1 is
/// `perry-ext-http` (P5), 3 is this module's own test slot, and 2/4/5/6 are
/// P7's four database bindings (`perry-db-turnloop::subsystem`). A binding is a
/// separately linked `staticlib` with its own sink function, so four database
/// bindings really do need four slots even though they share one transport
/// module. 7 is `perry-ext-ws`'s outbound client and 8 the HTTP/1.1 listener
/// its standalone `WebSocketServer({ port })` binds through
/// `perry-http-server`; those are two slots and not one because they are two
/// sink functions in the same binary — the crate's own, and the server core's.
///
/// The ceiling was 8, which slot 8 would have failed to register on: a
/// `register_sink` that returns `false` leaves `available()` false, so the
/// binding would have declined to a transport that no longer exists. Raising
/// it costs sixteen relaxed loads' worth of static array and nothing else —
/// it is **not** part of the ABI digest (`js_perry_net_abi_layout`), because a
/// binding names a slot number, never this constant. A fixed array keeps
/// routing to one relaxed load, and `register_sink` refuses an out-of-range
/// slot rather than letting a binding write past the end.
///
/// The slot map was over-subscribed until the database ledger moved to its own
/// band: the P7 database lane and the P5 server lane numbered from two
/// different ledgers, so `perry-ext-pg` sat on 2 with `perry-stdlib`'s turnloop
/// HTTP client, `perry-ext-mysql2` on 4 with `perry-ext-fastify`, and
/// `perry-ext-ioredis` on 5 with `perry-stdlib`'s bundled framework server.
/// Each pair needs a program linking both bindings to reach, which is why
/// nothing caught it — and a fastify app that uses mysql2 is not an exotic
/// shape. The database bindings now occupy 9..=12
/// (`perry-db-turnloop::subsystem`, which is the one authority for that band),
/// and [`register_sink`] refuses a slot already held by a *different* sink, so
/// a future collision declines loudly instead of silently misrouting.
pub const MAX_SUBSYSTEMS: usize = 16;

/// A completion sink: called on the loop-owning thread, once per completion.
pub type SinkFn = extern "C" fn(*const NetCompletion);

/// Allocates one id in the binding's handle space for an accepted connection.
/// Returning zero (or a negative value) refuses the connection.
pub type AllocFn = extern "C" fn() -> i64;

// ── Completion kinds, as seen over the C ABI ────────────────────────────────

/// A client socket finished connecting.
pub const NET_CONNECT: i32 = 1;
/// A listener produced a connection; `conn` names it.
pub const NET_ACCEPT: i32 = 2;
/// Bytes arrived; `data`/`len` borrow them for the duration of the call.
pub const NET_DATA: i32 = 3;
/// The peer closed its write side: readable EOF.
pub const NET_EOF: i32 = 4;
/// A write completed; `user` echoes the caller's token, `queued` is what is
/// still outstanding on the socket.
pub const NET_WROTE: i32 = 5;
/// The write-side shutdown submitted by `end()` completed.
pub const NET_SHUTDOWN: i32 = 6;
/// The handle's final completion; no further completion can name this id.
pub const NET_CLOSED: i32 = 7;
/// An operation failed; `code`/`errno`/`syscall` carry Node's triple.
pub const NET_ERROR: i32 = 8;
/// A subsystem-owned deadline expired (P5). `id` names the deadline, which is
/// the caller's own id — a connection's, not a socket handle's.
pub const NET_TIMER: i32 = 9;

/// One completion, in the shape a separately linked binding can read.
///
/// Borrowed for the duration of the sink call only. `data` points into
/// turnloop's pooled read buffer, which returns to the pool as soon as the
/// sink returns; `code` and `syscall` are static names with no NUL terminator,
/// carried as pointer + length so no CString allocation is needed on a path
/// that runs once per read.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NetCompletion {
    /// One of the `NET_*` constants above.
    pub kind: i32,
    /// Node's `err.errno` (negated OS code), zero when not an error.
    pub errno: i32,
    /// Nonzero when the operation that produced this will produce no more.
    /// A multishot accept or read that reports a *transient* failure is not
    /// terminal, and Node does not tear the server down for one — the tokio
    /// accept loop deliberately did not break on an accept error either.
    pub terminal: i32,
    /// Padding, so the struct's layout is identical on both sides of the ABI
    /// without depending on how the compiler packs two trailing i32s.
    pub _reserved: i32,
    /// The Perry-side id of the socket or listener this concerns.
    pub id: i64,
    /// For [`NET_ACCEPT`], the newly allocated connection id; else zero.
    pub conn: i64,
    /// The caller's write/end completion token, echoed back; zero if none.
    pub user: u64,
    /// Bytes read or written.
    pub len: usize,
    /// Bytes still queued on the socket's write side after this completion.
    pub queued: usize,
    /// Read payload; valid only until the sink returns. Null when `len` is 0.
    pub data: *const u8,
    /// Node error code (`"ECONNRESET"`), not NUL-terminated. Null if none.
    pub code: *const u8,
    /// Length of `code`.
    pub code_len: usize,
    /// Node `syscall` name (`"read"`), not NUL-terminated. Null if none.
    pub syscall: *const u8,
    /// Length of `syscall`.
    pub syscall_len: usize,
}

impl NetCompletion {
    fn blank(kind: i32, id: i64) -> Self {
        Self {
            kind,
            errno: 0,
            terminal: 0,
            _reserved: 0,
            id,
            conn: 0,
            user: 0,
            len: 0,
            queued: 0,
            data: std::ptr::null(),
            code: std::ptr::null(),
            code_len: 0,
            syscall: std::ptr::null(),
            syscall_len: 0,
        }
    }

    pub(super) fn connect(id: i64) -> Self {
        Self::blank(NET_CONNECT, id)
    }

    pub(super) fn accept(server: i64, conn: i64, _peer: Option<SocketAddr>) -> Self {
        let mut c = Self::blank(NET_ACCEPT, server);
        c.conn = conn;
        c
    }

    pub(super) fn data(id: i64, bytes: &[u8]) -> Self {
        let mut c = Self::blank(NET_DATA, id);
        c.len = bytes.len();
        c.data = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr()
        };
        c
    }

    pub(super) fn timer(id: i64) -> Self {
        Self::blank(NET_TIMER, id)
    }

    pub(super) fn eof(id: i64) -> Self {
        Self::blank(NET_EOF, id)
    }

    pub(super) fn wrote(id: i64, user: u64, len: usize, queued: usize) -> Self {
        let mut c = Self::blank(NET_WROTE, id);
        c.user = user;
        c.len = len;
        c.queued = queued;
        c
    }

    pub(super) fn shutdown(id: i64, user: u64) -> Self {
        let mut c = Self::blank(NET_SHUTDOWN, id);
        c.user = user;
        c
    }

    pub(super) fn closed(id: i64) -> Self {
        Self::blank(NET_CLOSED, id)
    }

    pub(super) fn error(id: i64, user: u64, queued: usize, err: NodeError, terminal: bool) -> Self {
        let mut c = Self::blank(NET_ERROR, id);
        c.terminal = i32::from(terminal);
        c.user = user;
        c.queued = queued;
        c.errno = err.errno;
        c.code = err.code.as_ptr();
        c.code_len = err.code.len();
        c.syscall = err.syscall.as_ptr();
        c.syscall_len = err.syscall.len();
        c
    }

    /// Borrow the read payload. Only valid inside the sink call.
    ///
    /// # Safety
    /// The caller must be inside the sink invocation that received this
    /// completion; `data` is a pooled lease that is released afterwards.
    pub unsafe fn bytes(&self) -> &[u8] {
        if self.data.is_null() || self.len == 0 {
            return &[];
        }
        // SAFETY: `dispatch` builds this from a live `BufLease` slice and the
        // lease outlives the sink call.
        unsafe { std::slice::from_raw_parts(self.data, self.len) }
    }

    /// Borrow the Node error code, if this is an error completion.
    ///
    /// # Safety
    /// Same contract as [`NetCompletion::bytes`]; in practice the pointer is
    /// `&'static str` data, so it outlives any call.
    pub unsafe fn code_str(&self) -> Option<&str> {
        if self.code.is_null() || self.code_len == 0 {
            return None;
        }
        // SAFETY: always built from a `&'static str` in `errors.rs`.
        unsafe { std::str::from_utf8(std::slice::from_raw_parts(self.code, self.code_len)).ok() }
    }

    /// Borrow the Node `syscall` name, if this is an error completion.
    ///
    /// # Safety
    /// Same contract as [`NetCompletion::code_str`].
    pub unsafe fn syscall_str(&self) -> Option<&str> {
        if self.syscall.is_null() || self.syscall_len == 0 {
            return None;
        }
        // SAFETY: always built from a `&'static str` in `errors.rs`.
        unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(self.syscall, self.syscall_len)).ok()
        }
    }
}

static SINKS: [AtomicPtr<()>; MAX_SUBSYSTEMS] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; MAX_SUBSYSTEMS];
static ALLOCS: [AtomicPtr<()>; MAX_SUBSYSTEMS] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; MAX_SUBSYSTEMS];

/// Install a binding's completion sink and accepted-connection id allocator.
///
/// Idempotent for the same pointers. A subsystem index at or above
/// [`MAX_SUBSYSTEMS`] is rejected (returns `false`) rather than silently
/// dropped, because a binding whose sink never registered would look like a
/// socket that simply never produces events.
pub fn register_sink(subsystem: u8, sink: SinkFn, alloc: AllocFn) -> bool {
    let slot = subsystem as usize;
    if slot >= MAX_SUBSYSTEMS {
        return false;
    }
    // A slot already held by a DIFFERENT sink means two bindings were numbered
    // the same, and the old behaviour — store and return true to both — is the
    // worst available answer: each binding believes it is registered, so
    // `available()` is true for both, and every completion goes to whichever
    // registered last, which reads the token's low bits as one of ITS OWN
    // connection ids. Refusing instead makes `available()` false for the
    // loser, so it keeps its fallback transport and nothing is misrouted.
    //
    // Re-registering the same sink stays idempotent, which is the documented
    // contract and what a binding whose module is initialised twice relies on.
    let held = SINKS[slot].load(Ordering::Acquire);
    if !held.is_null() && held != sink as *mut () {
        return false;
    }
    // Publish the allocator first: an accept completion needs it, and a sink
    // that is visible without one would have to refuse connections.
    ALLOCS[slot].store(alloc as *mut (), Ordering::Release);
    SINKS[slot].store(sink as *mut (), Ordering::Release);
    true
}

pub(super) fn emit(subsystem: u8, completion: NetCompletion) {
    let slot = subsystem as usize;
    if slot >= MAX_SUBSYSTEMS {
        return;
    }
    let p = SINKS[slot].load(Ordering::Acquire);
    if p.is_null() {
        return;
    }
    // SAFETY: the slot only ever holds a `SinkFn` stored by `register_sink`.
    let f: SinkFn = unsafe { std::mem::transmute(p) };
    f(&completion as *const NetCompletion);
}

pub(super) fn allocate_id(subsystem: u8) -> Option<i64> {
    let slot = subsystem as usize;
    if slot >= MAX_SUBSYSTEMS {
        return None;
    }
    let p = ALLOCS[slot].load(Ordering::Acquire);
    if p.is_null() {
        return None;
    }
    // SAFETY: the slot only ever holds an `AllocFn` stored by `register_sink`.
    let f: AllocFn = unsafe { std::mem::transmute(p) };
    let id = f();
    (id > 0).then_some(id)
}

/// Whether a sink is installed for `subsystem`. Test and diagnostic use: a
/// "turnloop handled this" claim is vacuous if nothing was listening.
pub fn sink_installed(subsystem: u8) -> bool {
    let slot = subsystem as usize;
    slot < MAX_SUBSYSTEMS && !SINKS[slot].load(Ordering::Acquire).is_null()
}
