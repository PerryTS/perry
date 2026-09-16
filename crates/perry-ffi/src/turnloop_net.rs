//! turnloop P1 networking, for native bindings (`node:net` and friends).
//!
//! A binding crate is a separately linked `staticlib` whose only Cargo
//! dependency is this one, so it cannot hold a `&mut turnloop::Loop`. The
//! runtime owns the agent's loop and exposes every submission as a C-ABI
//! function (`perry-runtime/src/turnloop_net/abi.rs`); this module is the safe
//! Rust face of that, exactly as [`crate::event_pump`] is for the wait driver.
//!
//! # The coexistence rule
//!
//! [`available`] answers whether this thread may use turnloop at all. It is
//! false on a `worker_threads` agent (which has no loop until P3/P4), and on
//! a host where loop creation failed. A binding that gets `false` must keep
//! its existing transport for that socket.
//! **A socket belongs to one transport for its whole life** — there is no
//! handover, so a binding must decide once, at creation.
//!
//! # Buffers and the GC
//!
//! [`write`] copies the caller's bytes before returning, and a read's bytes
//! are borrowed only for the duration of the sink call. Nothing the binding
//! owns — and in particular nothing on the JS heap — is retained by the
//! driver, so there is no buffer to root across a collection and no GC root
//! scanner to register for the I/O path. The binding's JS-side records
//! (listener closures, write callbacks) are unaffected and keep their existing
//! scanner.

#[cfg(any(not(test), feature = "runtime-link"))]
use std::sync::atomic::{AtomicBool, Ordering};

/// Completion kind: a client socket finished connecting.
pub const NET_CONNECT: i32 = 1;
/// Completion kind: a listener produced a connection (`conn` names it).
pub const NET_ACCEPT: i32 = 2;
/// Completion kind: bytes arrived.
pub const NET_DATA: i32 = 3;
/// Completion kind: the peer closed its write side (readable EOF).
pub const NET_EOF: i32 = 4;
/// Completion kind: a write completed.
pub const NET_WROTE: i32 = 5;
/// Completion kind: the write-side shutdown from `end()` completed.
pub const NET_SHUTDOWN: i32 = 6;
/// Completion kind: the handle's final completion.
pub const NET_CLOSED: i32 = 7;
/// Completion kind: an operation failed.
pub const NET_ERROR: i32 = 8;
/// Completion kind: a subsystem-owned deadline expired (P5); `id` names it.
pub const NET_TIMER: i32 = 9;

/// This module's view of the runtime's completion record.
///
/// Layout-checked against the runtime's own definition at
/// [`register_sink`] time, so the two cannot drift silently.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NetCompletion {
    /// One of the `NET_*` constants.
    pub kind: i32,
    /// Node's `err.errno` (negated OS code); zero when not an error.
    pub errno: i32,
    /// Nonzero when the operation that produced this will produce no more.
    pub terminal: i32,
    /// Layout padding; see the runtime's definition.
    pub _reserved: i32,
    /// The socket or listener this concerns.
    pub id: i64,
    /// For [`NET_ACCEPT`], the accepted connection's id; else zero.
    pub conn: i64,
    /// The write/end completion token the caller supplied; zero if none.
    pub user: u64,
    /// Bytes read or written.
    pub len: usize,
    /// Bytes still queued on the socket's write side.
    pub queued: usize,
    /// Read payload, valid only until the sink returns.
    pub data: *const u8,
    /// Node error code, not NUL-terminated.
    pub code: *const u8,
    /// Length of `code`.
    pub code_len: usize,
    /// Node `syscall` name, not NUL-terminated.
    pub syscall: *const u8,
    /// Length of `syscall`.
    pub syscall_len: usize,
}

impl NetCompletion {
    /// Borrow the read payload.
    ///
    /// # Safety
    /// Only valid inside the sink call that received this completion: the
    /// bytes live in a pooled buffer the runtime reclaims afterwards.
    pub unsafe fn bytes(&self) -> &[u8] {
        if self.data.is_null() || self.len == 0 {
            return &[];
        }
        // SAFETY: the caller promises to be inside the sink invocation.
        unsafe { std::slice::from_raw_parts(self.data, self.len) }
    }

    /// Borrow the Node error code (`"ECONNRESET"`), if this is an error.
    ///
    /// # Safety
    /// See [`NetCompletion::bytes`]; in practice this points at `'static`
    /// string data in the runtime image.
    pub unsafe fn code(&self) -> Option<&str> {
        if self.code.is_null() || self.code_len == 0 {
            return None;
        }
        // SAFETY: the runtime always builds this from a `&'static str`.
        unsafe { std::str::from_utf8(std::slice::from_raw_parts(self.code, self.code_len)).ok() }
    }

    /// Borrow the Node `syscall` name (`"read"`), if this is an error.
    ///
    /// # Safety
    /// See [`NetCompletion::code`].
    pub unsafe fn syscall(&self) -> Option<&str> {
        if self.syscall.is_null() || self.syscall_len == 0 {
            return None;
        }
        // SAFETY: the runtime always builds this from a `&'static str`.
        unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(self.syscall, self.syscall_len)).ok()
        }
    }
}

/// Out-parameter the runtime fills when a submission fails before any
/// completion exists.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RawNetError {
    code: *const u8,
    code_len: usize,
    syscall: *const u8,
    syscall_len: usize,
    errno: i32,
}

impl RawNetError {
    #[cfg(any(not(test), feature = "runtime-link"))]
    fn blank() -> Self {
        Self {
            code: std::ptr::null(),
            code_len: 0,
            syscall: std::ptr::null(),
            syscall_len: 0,
            errno: 0,
        }
    }
}

/// A failed submission, in the shape Node reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetError {
    /// Node's `err.code`, e.g. `"EADDRINUSE"`.
    pub code: String,
    /// Node's `err.syscall`, e.g. `"listen"`.
    pub syscall: String,
    /// Node's `err.errno` (negated OS code); zero when there was none.
    pub errno: i32,
    /// True when this thread simply has no turnloop loop, so the caller must
    /// fall back to its own transport rather than surface an error to JS.
    pub no_loop: bool,
}

impl NetError {
    /// `<syscall> <CODE>` — libuv's message shape, which Node keeps for socket
    /// errors.
    pub fn message(&self) -> String {
        if self.syscall.is_empty() {
            self.code.clone()
        } else {
            format!("{} {}", self.syscall, self.code)
        }
    }

    #[cfg(any(not(test), feature = "runtime-link"))]
    fn from_raw(raw: RawNetError, no_loop: bool) -> Self {
        // SAFETY: the runtime writes `'static` string data or nulls.
        let read = |ptr: *const u8, len: usize| -> String {
            if ptr.is_null() || len == 0 {
                return String::new();
            }
            unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) }
                .unwrap_or("")
                .to_string()
        };
        Self {
            code: read(raw.code, raw.code_len),
            syscall: read(raw.syscall, raw.syscall_len),
            errno: raw.errno,
            no_loop,
        }
    }
}

/// Called once per completion, on the agent thread that owns the loop.
pub type SinkFn = extern "C" fn(*const NetCompletion);
/// Allocates one id for an accepted connection, in the binding's handle space.
pub type AllocFn = extern "C" fn() -> i64;

#[cfg(any(not(test), feature = "runtime-link"))]
extern "C" {
    fn js_perry_net_available() -> i32;
    fn js_perry_net_abi_layout() -> u64;
    fn js_perry_net_register_sink(subsystem: i32, sink: SinkFn, alloc: AllocFn) -> i32;
    fn js_perry_net_sink_installed(subsystem: i32) -> i32;
    fn js_perry_net_error_from_os(
        os: i32,
        syscall: *const u8,
        syscall_len: usize,
        out: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_errno_for_code(code: *const u8, code_len: usize) -> i32;
    fn js_perry_net_tcp_listen(
        id: i64,
        subsystem: i32,
        host: *const u8,
        host_len: usize,
        port: u16,
        backlog: u32,
        reuse_port: i32,
        nodelay: i32,
        err: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_pipe_listen(
        id: i64,
        subsystem: i32,
        path: *const u8,
        path_len: usize,
        backlog: u32,
        err: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_accept_start(id: i64, err: *mut RawNetError) -> i32;
    fn js_perry_net_tcp_connect(
        id: i64,
        subsystem: i32,
        host: *const u8,
        host_len: usize,
        port: u16,
        nodelay: i32,
        err: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_pipe_connect(
        id: i64,
        subsystem: i32,
        path: *const u8,
        path_len: usize,
        err: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_read_start(id: i64, err: *mut RawNetError) -> i32;
    fn js_perry_net_adopt_stream(
        id: i64,
        subsystem: i32,
        socket: i64,
        err: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_timer_arm(id: i64, subsystem: i32, delay_ms: u64, err: *mut RawNetError)
        -> i32;
    fn js_perry_net_timer_cancel(id: i64, err: *mut RawNetError) -> i32;
    fn js_perry_net_timer_park(id: i64, err: *mut RawNetError) -> i32;
    fn js_perry_net_transfer(id: i64, subsystem: i32, err: *mut RawNetError) -> i32;
    fn js_perry_net_write(
        id: i64,
        bytes: *const u8,
        len: usize,
        user: u64,
        out_queued: *mut usize,
        err: *mut RawNetError,
    ) -> i32;
    fn js_perry_net_shutdown(id: i64, user: u64, err: *mut RawNetError) -> i32;
    fn js_perry_net_close(id: i64, err: *mut RawNetError) -> i32;
    fn js_perry_net_set_ref(id: i64, referenced: i32) -> i32;
    fn js_perry_net_queued_bytes(id: i64) -> usize;
    fn js_perry_net_is_live(id: i64) -> i32;
    fn js_perry_net_live_handles() -> usize;
    fn js_perry_net_local_address(
        id: i64,
        out: *mut u8,
        cap: usize,
        out_len: *mut usize,
        out_port: *mut u16,
        out_family: *mut i32,
    ) -> i32;
    fn js_perry_net_peer_address(
        id: i64,
        out: *mut u8,
        cap: usize,
        out_len: *mut usize,
        out_port: *mut u16,
        out_family: *mut i32,
    ) -> i32;
}

// Everything below is reached only when a runtime is linked: a standalone
// `cargo test -p perry-ffi` takes the `runtime_call!` fallback arm on every
// path. Gated so that build is warning-clean too — without this, an isolated
// `cargo check -p perry-ffi --all-targets` is red before anyone touches it,
// and a workspace build hides that because a binding crate's dev-dependency
// unifies `runtime-link` on.
#[cfg(any(not(test), feature = "runtime-link"))]
const OK: i32 = 0;
#[cfg(any(not(test), feature = "runtime-link"))]
const ENOLOOP: i32 = -2;

/// Revision of the ABI this file is written against; must match the runtime's.
const ABI_VERSION: u8 = 2;

fn layout_digest() -> u64 {
    use std::mem::{align_of, offset_of, size_of};
    (size_of::<NetCompletion>() as u64) << 48
        | (offset_of!(NetCompletion, id) as u64) << 40
        | (offset_of!(NetCompletion, data) as u64) << 32
        | (offset_of!(NetCompletion, code) as u64) << 24
        | (offset_of!(NetCompletion, syscall) as u64) << 16
        | (align_of::<NetCompletion>() as u64) << 8
        | ABI_VERSION as u64
}

#[cfg(any(not(test), feature = "runtime-link"))]
static REGISTERED: AtomicBool = AtomicBool::new(false);

#[cfg(any(not(test), feature = "runtime-link"))]
fn check(rc: i32, raw: RawNetError) -> Result<(), NetError> {
    match rc {
        OK => Ok(()),
        ENOLOOP => Err(NetError::from_raw(raw, true)),
        _ => Err(NetError::from_raw(raw, false)),
    }
}

/// Whether this thread may put sockets on turnloop for `subsystem`.
///
/// A binding calls this once per socket, at creation, and keeps its legacy
/// transport when it is false. Both halves matter: the thread must own a loop
/// (false on a worker agent), and *this* subsystem's sink must be installed
/// — a binding whose registration was refused by the layout check would
/// otherwise submit work whose completions nothing would deliver.
pub fn available(subsystem: u8) -> bool {
    #[cfg(any(not(test), feature = "runtime-link"))]
    {
        // SAFETY: both are plain predicates in the linked runtime.
        unsafe {
            js_perry_net_available() != 0
                && REGISTERED.load(Ordering::Acquire)
                && js_perry_net_sink_installed(subsystem as i32) != 0
        }
    }
    #[cfg(all(test, not(feature = "runtime-link")))]
    {
        let _ = subsystem;
        false
    }
}

/// Install this binding's completion sink and accepted-connection allocator.
///
/// Returns false — and leaves [`available`] false — when the runtime's
/// completion layout does not match this crate's, which is the drift check
/// described on [`NetCompletion`]. Idempotent.
pub fn register_sink(subsystem: u8, sink: SinkFn, alloc: AllocFn) -> bool {
    #[cfg(any(not(test), feature = "runtime-link"))]
    {
        // SAFETY: both are plain registration calls in the linked runtime.
        let ok = unsafe {
            if js_perry_net_abi_layout() != layout_digest() {
                return false;
            }
            js_perry_net_register_sink(subsystem as i32, sink, alloc) != 0
        };
        if ok {
            REGISTERED.store(true, Ordering::Release);
        }
        ok
    }
    #[cfg(all(test, not(feature = "runtime-link")))]
    {
        let _ = (subsystem, sink, alloc, layout_digest());
        false
    }
}

/// Whether a sink is installed for `subsystem`. A test uses this so a
/// "turnloop carried this" claim cannot pass with nothing listening.
pub fn sink_installed(subsystem: u8) -> bool {
    #[cfg(any(not(test), feature = "runtime-link"))]
    {
        // SAFETY: a plain predicate in the linked runtime.
        unsafe { js_perry_net_sink_installed(subsystem as i32) != 0 }
    }
    #[cfg(all(test, not(feature = "runtime-link")))]
    {
        let _ = subsystem;
        false
    }
}

macro_rules! runtime_call {
    ($body:block, $fallback:expr) => {{
        #[cfg(any(not(test), feature = "runtime-link"))]
        {
            $body
        }
        #[cfg(all(test, not(feature = "runtime-link")))]
        {
            $fallback
        }
    }};
}

/// The error a standalone `perry-ffi` unit test sees: there is no runtime
/// linked, so every submission reports "use your own transport".
#[cfg(all(test, not(feature = "runtime-link")))]
fn unavailable() -> NetError {
    NetError {
        code: "ENOTSUP".to_string(),
        syscall: String::new(),
        errno: 0,
        no_loop: true,
    }
}

/// Bind and listen on `host:port`. Synchronous: a bind failure is reported
/// here, not as a completion.
/// `nodelay` is applied by the accepting loop to **every** connection this
/// listener accepts, before its completion reaches the binding — which is where
/// Node applies `noDelay`, a server option rather than a per-socket one.
pub fn tcp_listen(
    id: i64,
    subsystem: u8,
    host: &str,
    port: u16,
    backlog: u32,
    reuse_port: bool,
    nodelay: bool,
) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `host` is a live UTF-8 slice; `raw` is writable.
            let rc = unsafe {
                js_perry_net_tcp_listen(
                    id,
                    subsystem as i32,
                    host.as_ptr(),
                    host.len(),
                    port,
                    backlog,
                    i32::from(reuse_port),
                    i32::from(nodelay),
                    &mut raw,
                )
            };
            check(rc, raw)
        },
        {
            let _ = (id, subsystem, host, port, backlog, reuse_port, nodelay);
            Err(unavailable())
        }
    )
}

/// Bind and listen on a Unix-domain socket path or a Windows named pipe.
pub fn pipe_listen(id: i64, subsystem: u8, path: &str, backlog: u32) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `path` is a live UTF-8 slice; `raw` is writable.
            let rc = unsafe {
                js_perry_net_pipe_listen(
                    id,
                    subsystem as i32,
                    path.as_ptr(),
                    path.len(),
                    backlog,
                    &mut raw,
                )
            };
            check(rc, raw)
        },
        {
            let _ = (id, subsystem, path, backlog);
            Err(unavailable())
        }
    )
}

/// Start multishot accept on a listener.
pub fn accept_start(id: i64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_accept_start(id, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = id;
            Err(unavailable())
        }
    )
}

/// Connect a TCP client socket, resolving a hostname off the loop thread.
pub fn tcp_connect(
    id: i64,
    subsystem: u8,
    host: &str,
    port: u16,
    nodelay: bool,
) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `host` is a live UTF-8 slice; `raw` is writable.
            let rc = unsafe {
                js_perry_net_tcp_connect(
                    id,
                    subsystem as i32,
                    host.as_ptr(),
                    host.len(),
                    port,
                    i32::from(nodelay),
                    &mut raw,
                )
            };
            check(rc, raw)
        },
        {
            let _ = (id, subsystem, host, port, nodelay);
            Err(unavailable())
        }
    )
}

/// Connect to a Unix-domain socket path or a Windows named pipe.
pub fn pipe_connect(id: i64, subsystem: u8, path: &str) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `path` is a live UTF-8 slice; `raw` is writable.
            let rc = unsafe {
                js_perry_net_pipe_connect(id, subsystem as i32, path.as_ptr(), path.len(), &mut raw)
            };
            check(rc, raw)
        },
        {
            let _ = (id, subsystem, path);
            Err(unavailable())
        }
    )
}

/// An already-connected stream socket a binding hands to [`adopt_stream`]:
/// a file descriptor on Unix, a `SOCKET` on Windows.
#[cfg(unix)]
pub type AdoptedSocket = std::os::fd::OwnedFd;
/// See the Unix definition.
#[cfg(windows)]
pub type AdoptedSocket = std::os::windows::io::OwnedSocket;

/// Adopt an already-connected stream socket — one some other transport
/// opened — as a connected socket on this thread's loop, under `id`.
///
/// Completions for it go to `subsystem`'s sink exactly as for a socket this
/// loop connected itself; the caller starts reading with [`read_start`].
/// `socket` is **consumed on every outcome**: a refusal (no loop on this
/// thread, a descriptor turnloop cannot adopt) closes it.
#[cfg(any(unix, windows))]
pub fn adopt_stream(id: i64, subsystem: u8, socket: AdoptedSocket) -> Result<(), NetError> {
    #[cfg(unix)]
    let raw = {
        use std::os::fd::IntoRawFd;
        socket.into_raw_fd() as i64
    };
    #[cfg(windows)]
    let raw = {
        use std::os::windows::io::IntoRawSocket;
        socket.into_raw_socket() as i64
    };
    runtime_call!(
        {
            let mut raw_err = RawNetError::blank();
            // SAFETY: `raw` is an owned socket whose ownership the runtime
            // takes on every outcome; `raw_err` is writable.
            let rc = unsafe { js_perry_net_adopt_stream(id, subsystem as i32, raw, &mut raw_err) };
            check(rc, raw_err)
        },
        {
            // No runtime to hand it to: honour the "consumed on every outcome"
            // contract by closing it here.
            // SAFETY (both arms): `raw` came from `into_raw_*` just above and
            // nothing else owns it, so this is its one reconstruction.
            #[cfg(unix)]
            drop(unsafe {
                <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(raw as i32)
            });
            #[cfg(windows)]
            drop(unsafe {
                <std::os::windows::io::OwnedSocket as std::os::windows::io::FromRawSocket>::from_raw_socket(raw as u64)
            });
            let _ = (id, subsystem);
            Err(unavailable())
        }
    )
}

/// Start multishot reading on a connected socket.
pub fn read_start(id: i64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_read_start(id, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = id;
            Err(unavailable())
        }
    )
}

/// Arm — or move — a one-shot deadline `delay_ms` from now, delivered as a
/// [`NET_TIMER`] completion naming `id` (P5).
///
/// Perry's server timeouts (`keepAliveTimeout`, `headersTimeout`,
/// `requestTimeout`, a TLS handshake deadline) are per-connection deadlines,
/// and a binding has no way to create a JS timer. Arming one here puts it in
/// the loop's `next_deadline()`, so a park that has nothing but an idle
/// keep-alive connection still ends on time. The deadline is unreferenced: it
/// never keeps the process alive by itself.
pub fn timer_arm(id: i64, subsystem: u8, delay_ms: u64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_timer_arm(id, subsystem as i32, delay_ms, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = (id, subsystem, delay_ms);
            Err(unavailable())
        }
    )
}

/// Hand a live socket to another subsystem, keeping its id and every
/// outstanding operation (P5).
///
/// This is how an HTTP `'upgrade'` becomes a raw `net.Socket`: the multishot
/// read is not cancelled, so the next byte is delivered straight to the new
/// owner. The old owner hands over whatever it had already buffered itself.
pub fn transfer(id: i64, subsystem: u8) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_transfer(id, subsystem as i32, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = (id, subsystem);
            Err(unavailable())
        }
    )
}

/// Disarm a deadline while keeping its handle, so the next `timer_arm` for the
/// same id moves the deadline in place instead of building a handle. Idempotent.
pub fn timer_park(id: i64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_timer_park(id, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = id;
            Err(unavailable())
        }
    )
}

/// Cancel a deadline. Idempotent: an id with no deadline is not an error.
pub fn timer_cancel(id: i64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_timer_cancel(id, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = id;
            Err(unavailable())
        }
    )
}

/// Queue bytes for writing; they are copied before this returns.
///
/// `user` is echoed back on the [`NET_WROTE`] completion, for the binding's
/// write callback. The returned value is the socket's total queued byte count
/// — Node's `writableLength`, and what decides `socket.write()`'s boolean.
pub fn write(id: i64, bytes: &[u8], user: u64) -> Result<usize, NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            let mut queued: usize = 0;
            // SAFETY: `bytes` is a live slice; both out-params are writable.
            let rc = unsafe {
                js_perry_net_write(id, bytes.as_ptr(), bytes.len(), user, &mut queued, &mut raw)
            };
            check(rc, raw).map(|()| queued)
        },
        {
            let _ = (id, bytes, user);
            Err(unavailable())
        }
    )
}

/// Half-close: shut the write side down after queued writes have gone out.
pub fn shutdown(id: i64, user: u64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_shutdown(id, user, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = (id, user);
            Err(unavailable())
        }
    )
}

/// Close the handle; a [`NET_CLOSED`] completion follows.
pub fn close(id: i64) -> Result<(), NetError> {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `raw` is writable.
            let rc = unsafe { js_perry_net_close(id, &mut raw) };
            check(rc, raw)
        },
        {
            let _ = id;
            Err(unavailable())
        }
    )
}

/// Node's `ref()`/`unref()` for one handle.
pub fn set_ref(id: i64, referenced: bool) -> bool {
    runtime_call!(
        {
            // SAFETY: a plain setter in the linked runtime.
            unsafe { js_perry_net_set_ref(id, i32::from(referenced)) == OK }
        },
        {
            let _ = (id, referenced);
            false
        }
    )
}

/// Bytes handed to the driver and not yet reported written.
pub fn queued_bytes(id: i64) -> usize {
    runtime_call!(
        {
            // SAFETY: a plain getter in the linked runtime.
            unsafe { js_perry_net_queued_bytes(id) }
        },
        {
            let _ = id;
            0
        }
    )
}

/// Whether `id` names a live turnloop-backed handle on this thread.
pub fn is_live(id: i64) -> bool {
    runtime_call!(
        {
            // SAFETY: a plain predicate in the linked runtime.
            unsafe { js_perry_net_is_live(id) != 0 }
        },
        {
            let _ = id;
            false
        }
    )
}

/// Live turnloop-backed handles on this thread. A test that claims turnloop
/// carried a workload must see this above zero while the workload runs.
pub fn live_handles() -> usize {
    runtime_call!(
        {
            // SAFETY: a plain getter in the linked runtime.
            unsafe { js_perry_net_live_handles() }
        },
        0
    )
}

/// Map an OS error code onto Node's `code`/`errno`/`syscall` triple.
///
/// For the transports P1 did not move: they hold a `std::io::Error` and still
/// have to report the same triple, and a second copy of the table in a binding
/// is how `code` and `errno` come to describe different failures on different
/// platforms.
pub fn error_from_os(os: Option<i32>, syscall: &str) -> NetError {
    runtime_call!(
        {
            let mut raw = RawNetError::blank();
            // SAFETY: `syscall` is a live UTF-8 slice; `raw` is writable.
            unsafe {
                js_perry_net_error_from_os(
                    os.unwrap_or(0),
                    syscall.as_ptr(),
                    syscall.len(),
                    &mut raw,
                )
            };
            NetError::from_raw(raw, false)
        },
        {
            let _ = (os, syscall);
            NetError {
                code: "UNKNOWN".to_string(),
                syscall: syscall.to_string(),
                errno: 0,
                no_loop: false,
            }
        }
    )
}

/// libuv's `err.errno` for a Node error name: the negated OS code on Linux
/// and macOS, libuv's own `-4xxx` number on Windows. Zero when the name is not
/// one this table knows.
pub fn errno_for_code(code: &str) -> i32 {
    runtime_call!(
        {
            // SAFETY: `code` is a live UTF-8 slice.
            unsafe { js_perry_net_errno_for_code(code.as_ptr(), code.len()) }
        },
        {
            let _ = code;
            0
        }
    )
}

/// One endpoint of a socket, as Node reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Textual IP address, or the empty string for a local (pipe) socket.
    pub address: String,
    /// TCP port, zero for a local socket.
    pub port: u16,
    /// 4 or 6.
    pub family: i32,
}

#[cfg(any(not(test), feature = "runtime-link"))]
fn endpoint(
    id: i64,
    f: unsafe extern "C" fn(i64, *mut u8, usize, *mut usize, *mut u16, *mut i32) -> i32,
) -> Option<Endpoint> {
    // 45 bytes covers the longest textual IPv6 form, plus room to spare.
    let mut buf = [0u8; 64];
    let mut len: usize = 0;
    let mut port: u16 = 0;
    let mut family: i32 = 4;
    // SAFETY: `buf` is writable for its own length and every out-param is a
    // live local.
    let rc = unsafe {
        f(
            id,
            buf.as_mut_ptr(),
            buf.len(),
            &mut len,
            &mut port,
            &mut family,
        )
    };
    if rc != OK {
        return None;
    }
    Some(Endpoint {
        address: String::from_utf8_lossy(&buf[..len]).into_owned(),
        port,
        family,
    })
}

/// `server.address()` / `socket.localAddress`.
pub fn local_address(id: i64) -> Option<Endpoint> {
    runtime_call!({ endpoint(id, js_perry_net_local_address) }, {
        let _ = id;
        None
    })
}

/// `socket.remoteAddress`.
pub fn peer_address(id: i64) -> Option<Endpoint> {
    runtime_call!({ endpoint(id, js_perry_net_peer_address) }, {
        let _ = id;
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_digest_changes_when_the_struct_does() {
        // Not a tautology: it pins the field offsets this crate's decoder and
        // the runtime's encoder both depend on. A reordering that keeps the
        // size identical still moves `data`, and that is the drift this digest
        // is here to catch.
        let digest = layout_digest();
        assert_eq!(digest as u8, ABI_VERSION);
        assert_eq!(
            (digest >> 48) as usize,
            std::mem::size_of::<NetCompletion>()
        );
        assert_eq!(
            ((digest >> 32) & 0xff) as usize,
            std::mem::offset_of!(NetCompletion, data)
        );
    }

    #[test]
    fn an_endpoint_decodes_a_truncated_address_without_panicking() {
        // `write_addr` truncates rather than failing when the buffer is short;
        // the decoder must stay lossy-safe rather than assume valid UTF-8.
        let raw = b"127.0.0.1";
        assert_eq!(String::from_utf8_lossy(&raw[..5]), "127.0");
    }
}
