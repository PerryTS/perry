//! The C ABI a separately linked net binding uses to drive the loop.
//!
//! `perry-ext-net` is a `staticlib` whose only Cargo dependency is `perry-ffi`;
//! it cannot hold a `&mut turnloop::Loop`, so every submission crosses this
//! boundary as primitives. The shape follows the event pump's existing
//! registration surface (`js_register_wake_callback`, `js_register_aux_pump`):
//! `#[no_mangle] extern "C"` functions, function pointers for callbacks, and
//! no Rust types in a signature.
//!
//! Every call must happen on the agent thread that owns the loop; each one
//! reports `PERRY_NET_ENOLOOP` rather than misbehaving if it does not. That is
//! the P1 coexistence contract: a worker agent has no loop until P3/P4, so its
//! binding keeps the tokio transport, and the two paths never share a socket.

use std::ffi::c_void;
use std::net::SocketAddr;
use std::path::PathBuf;

use super::sink::{AllocFn, NetCompletion, SinkFn};
use super::NodeError;

/// Success.
pub const PERRY_NET_OK: i32 = 0;
/// The operation failed; the `err` out-parameter, when supplied, says how.
pub const PERRY_NET_ERR: i32 = -1;
/// This thread has no turnloop loop: the caller must use its legacy transport.
pub const PERRY_NET_ENOLOOP: i32 = -2;

/// Out-parameter carrying Node's `code`/`errno`/`syscall` for a failed call.
///
/// `code` and `syscall` are static names, pointer + length, never NUL
/// terminated and never owned by the caller.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PerryNetError {
    /// Node's `err.code`, e.g. `"EADDRINUSE"`. Null when unset.
    pub code: *const u8,
    /// Length of `code`.
    pub code_len: usize,
    /// Node's `err.syscall`, e.g. `"listen"`. Null when unset.
    pub syscall: *const u8,
    /// Length of `syscall`.
    pub syscall_len: usize,
    /// Node's `err.errno` (negated OS code), zero when there was none.
    pub errno: i32,
}

impl PerryNetError {
    fn write(out: *mut PerryNetError, err: NodeError) {
        if out.is_null() {
            return;
        }
        let value = PerryNetError {
            code: err.code.as_ptr(),
            code_len: err.code.len(),
            syscall: err.syscall.as_ptr(),
            syscall_len: err.syscall.len(),
            errno: err.errno,
        };
        // SAFETY: the caller supplies a writable `PerryNetError`.
        // GC_STORE_AUDIT(POINTER_FREE): the destination is the CALLER's `PerryNetError`
        // out-param, not a GC slot, and every field is pointer-free with respect to the
        // heap: `code`/`syscall` are `&'static str` (see errors.rs:27,32), the lengths
        // are `usize`, `errno` is `i32`.
        unsafe { std::ptr::write(out, value) };
    }
}

fn finish(result: super::NetResult<()>, err: *mut PerryNetError) -> i32 {
    match result {
        Ok(()) => PERRY_NET_OK,
        Err(e) if e.code == "ENOTSUP" && e.errno == 0 && e.syscall.is_empty() => {
            PerryNetError::write(err, e);
            PERRY_NET_ENOLOOP
        }
        Err(e) => {
            PerryNetError::write(err, e);
            PERRY_NET_ERR
        }
    }
}

/// # Safety
/// `ptr`/`len` must describe a valid UTF-8 byte range, or `ptr` may be null
/// with `len` zero.
unsafe fn str_arg<'a>(ptr: *const u8, len: usize) -> &'a str {
    if ptr.is_null() || len == 0 {
        return "";
    }
    // SAFETY: the caller promises a readable range for `len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).unwrap_or("")
}

/// Revision of this ABI. Bumped whenever a signature or a struct field
/// changes; a binding compiled against a different revision is refused rather
/// than allowed to misread a completion.
///
/// 3: link routes (NET-TRANSPORT-DESIGN P0): `NetCompletion::flags`, the
/// `js_perry_net_link_*` family and the opaque `TransportCore` block.
pub const PERRY_NET_ABI_VERSION: u8 = 3;

/// A digest of [`NetCompletion`]'s layout, the opaque `TransportCore` block
/// a binding reserves ([`super::TRANSPORT_CORE_WORDS`]) and
/// [`PERRY_NET_ABI_VERSION`].
///
/// A binding declares its own `#[repr(C)]` copy of the completion struct — it
/// has no Cargo edge to this crate — so the two definitions can drift apart
/// silently, and the failure mode is reading a byte count out of a pointer
/// field. Both sides compute this from their own definition and compare once,
/// at registration, which turns that class of drift into a refused
/// registration instead of a corrupt read.
#[no_mangle]
pub extern "C" fn js_perry_net_abi_layout() -> u64 {
    net_abi_layout(super::TRANSPORT_CORE_WORDS)
}

/// The digest for a given core block size; perry-ffi computes the same
/// expression from its own struct and its own block constant. Fields, in bit
/// ranges: completion size 12 | `id` 6 | `data` 7 | `code` 7 | `syscall` 7 |
/// completion align 4 | core words 9 | core align 4 | version 8.
pub(crate) const fn net_abi_layout(core_words: usize) -> u64 {
    use std::mem::{align_of, offset_of, size_of};
    (size_of::<NetCompletion>() as u64 & 0xFFF) << 52
        | (offset_of!(NetCompletion, id) as u64 & 0x3F) << 46
        | (offset_of!(NetCompletion, data) as u64 & 0x7F) << 39
        | (offset_of!(NetCompletion, code) as u64 & 0x7F) << 32
        | (offset_of!(NetCompletion, syscall) as u64 & 0x7F) << 25
        | (align_of::<NetCompletion>() as u64 & 0xF) << 21
        | (core_words as u64 & 0x1FF) << 12
        | (align_of::<super::TransportCore>() as u64 & 0xF) << 8
        | PERRY_NET_ABI_VERSION as u64
}

/// Map an OS error code onto Node's `code`/`errno`/`syscall` triple.
///
/// Exists for the transports this phase did NOT move: they hold a
/// `std::io::Error` and still have to report the same triple, and duplicating
/// the table in a binding is how `code` and `errno` end up describing
/// different failures on different platforms.
///
/// # Safety
/// `syscall`/`syscall_len` must describe a readable UTF-8 range (`syscall` may
/// be null with length zero); `out` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_error_from_os(
    os: i32,
    syscall: *const u8,
    syscall_len: usize,
    out: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let name = unsafe { str_arg(syscall, syscall_len) };
    // The syscall name must outlive the call, and the caller owns the bytes it
    // passed in, so echo their pointer back rather than a borrowed local.
    let mapped = super::errors::from_os(os, "");
    if !out.is_null() {
        let value = PerryNetError {
            code: mapped.code.as_ptr(),
            code_len: mapped.code.len(),
            syscall,
            syscall_len,
            errno: mapped.errno,
        };
        // SAFETY: the caller supplies a writable `PerryNetError`.
        // GC_STORE_AUDIT(POINTER_FREE): same out-param as above. `code` is a `&'static
        // str`; `syscall` is the caller's OWN pointer echoed back, which is why it
        // outlives the call.
        unsafe { std::ptr::write(out, value) };
    }
    let _ = name;
    PERRY_NET_OK
}

/// libuv's `err.errno` for a Node error name. Zero when the name is unknown
/// to the table.
///
/// # Safety
/// `code`/`code_len` must describe a readable UTF-8 range.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_errno_for_code(code: *const u8, code_len: usize) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let name = unsafe { str_arg(code, code_len) };
    // NOT `-os`: on Windows libuv's errno is its own -4xxx value, not the
    // negated Winsock number, so the code name is what decides it.
    super::errors::libuv_errno(name, super::errors::os_code_for_name(name))
}

/// Nonzero when this thread can take the turnloop net path.
#[no_mangle]
pub extern "C" fn js_perry_net_available() -> i32 {
    i32::from(super::available())
}

/// Install a binding's completion sink and accepted-connection id allocator.
/// Returns nonzero on success.
#[no_mangle]
pub extern "C" fn js_perry_net_register_sink(subsystem: i32, sink: SinkFn, alloc: AllocFn) -> i32 {
    if subsystem < 0 {
        return 0;
    }
    i32::from(super::register_sink(subsystem as u8, sink, alloc))
}

/// Bind and listen on `host:port`. Synchronous; a bind failure is reported
/// here, not as a completion.
///
/// # Safety
/// `host`/`host_len` must describe a readable UTF-8 range; `err` must be null
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_tcp_listen(
    id: i64,
    subsystem: i32,
    host: *const u8,
    host_len: usize,
    port: u16,
    backlog: u32,
    reuse_port: i32,
    nodelay: i32,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let host = unsafe { str_arg(host, host_len) };
    let host = if host.is_empty() { "0.0.0.0" } else { host };
    let Ok(addr) = parse_bind_addr(host, port) else {
        PerryNetError::write(
            err,
            NodeError {
                code: "EINVAL",
                errno: 0,
                syscall: "listen",
            },
        );
        return PERRY_NET_ERR;
    };
    match super::tcp_listen(
        id,
        subsystem.max(0) as u8,
        addr,
        backlog,
        reuse_port != 0,
        nodelay != 0,
    ) {
        Ok(_) => PERRY_NET_OK,
        Err(e) => finish(Err(e), err),
    }
}

fn parse_bind_addr(host: &str, port: u16) -> Result<SocketAddr, ()> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    // A bind host is a literal in every Node call path that reaches here
    // (`server.listen` resolves `host` first, or defaults to the wildcard);
    // keep the fallbacks to the two wildcards rather than blocking the loop in
    // `getaddrinfo`.
    match host {
        "localhost" => Ok(SocketAddr::from(([127, 0, 0, 1], port))),
        "" | "0.0.0.0" => Ok(SocketAddr::from(([0, 0, 0, 0], port))),
        "::" => Ok(SocketAddr::new(
            std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
            port,
        )),
        _ => Err(()),
    }
}

/// Bind and listen on a Unix-domain socket path or a Windows named pipe.
///
/// # Safety
/// `path`/`path_len` must describe a readable UTF-8 range; `err` must be null
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_pipe_listen(
    id: i64,
    subsystem: i32,
    path: *const u8,
    path_len: usize,
    backlog: u32,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let path = unsafe { str_arg(path, path_len) };
    finish(
        super::pipe_listen(id, subsystem.max(0) as u8, &PathBuf::from(path), backlog),
        err,
    )
}

/// Start multishot accept on a listener.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_accept_start(id: i64, err: *mut PerryNetError) -> i32 {
    finish(super::accept_start(id), err)
}

/// Connect a TCP client socket to `host:port`, resolving a hostname off the
/// loop thread when it is not an IP literal.
///
/// # Safety
/// `host`/`host_len` must describe a readable UTF-8 range; `err` must be null
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_tcp_connect(
    id: i64,
    subsystem: i32,
    host: *const u8,
    host_len: usize,
    port: u16,
    nodelay: i32,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let host = unsafe { str_arg(host, host_len) };
    let host = if host.is_empty() { "127.0.0.1" } else { host };
    finish(
        super::tcp_connect_host(id, subsystem.max(0) as u8, host, port, nodelay != 0),
        err,
    )
}

/// Connect to a Unix-domain socket path or a Windows named pipe.
///
/// # Safety
/// `path`/`path_len` must describe a readable UTF-8 range; `err` must be null
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_pipe_connect(
    id: i64,
    subsystem: i32,
    path: *const u8,
    path_len: usize,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let path = unsafe { str_arg(path, path_len) };
    finish(
        super::pipe_connect(id, subsystem.max(0) as u8, &PathBuf::from(path)),
        err,
    )
}

/// Adopt an already-connected stream socket as a connected socket on this
/// thread's loop (see [`super::adopt_stream`]).
///
/// `socket` is a file descriptor on Unix and a `SOCKET` on Windows. **It is
/// consumed on every outcome**, success or failure: a refusal closes it, so the
/// caller never owns it again after this call. A negative value is refused
/// (`EINVAL`) and nothing is closed.
///
/// # Safety
/// `socket` must be an open, connected stream socket that the caller owns and
/// that nothing else will use or close; `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_adopt_stream(
    id: i64,
    subsystem: i32,
    socket: i64,
    err: *mut PerryNetError,
) -> i32 {
    if socket < 0 {
        return finish(Err(super::errors::invalid_input("adopt")), err);
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: the caller transfers ownership of an open descriptor.
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(socket as i32) };
        finish(super::adopt_stream(id, subsystem.max(0) as u8, fd), err)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawSocket;
        // SAFETY: the caller transfers ownership of an open socket.
        let sock = unsafe { std::os::windows::io::OwnedSocket::from_raw_socket(socket as u64) };
        finish(super::adopt_stream(id, subsystem.max(0) as u8, sock), err)
    }
    #[cfg(target_os = "wasi")]
    {
        finish(
            super::adopt_fd(id, subsystem.max(0) as u8, socket as i32),
            err,
        )
    }
    #[cfg(not(any(unix, windows, target_os = "wasi")))]
    {
        let _ = (id, subsystem);
        finish(Err(super::errors::unsupported("adopt")), err)
    }
}

/// Start multishot reading on a connected socket.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_read_start(id: i64, err: *mut PerryNetError) -> i32 {
    finish(super::read_start(id), err)
}

/// Queue `len` bytes for writing. The bytes are **copied** here, so the
/// caller's buffer may be reused or collected immediately; the copy is what
/// makes the write-side GC story trivial (module note in `turnloop_net`).
///
/// On success `out_queued`, when non-null, receives the socket's total queued
/// byte count — the input to `socket.write()`'s boolean return.
///
/// # Safety
/// `bytes`/`len` must describe a readable range; `out_queued` and `err` must
/// be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_write(
    id: i64,
    bytes: *const u8,
    len: usize,
    user: u64,
    out_queued: *mut usize,
    err: *mut PerryNetError,
) -> i32 {
    let owned = if bytes.is_null() || len == 0 {
        Vec::new()
    } else {
        // SAFETY: the caller promises a readable range for `len` bytes.
        unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec()
    };
    match super::write(id, owned, user) {
        Ok(queued) => {
            if !out_queued.is_null() {
                // SAFETY: the caller supplies a writable `usize`.
                // GC_STORE_AUDIT(POINTER_FREE): a `u32` queued-byte count into a caller
                // out-param.
                unsafe { std::ptr::write(out_queued, queued) };
            }
            PERRY_NET_OK
        }
        Err(e) => finish(Err(e), err),
    }
}

/// Half-close: shut down the write side once queued writes have gone out.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_shutdown(id: i64, user: u64, err: *mut PerryNetError) -> i32 {
    finish(super::shutdown(id, user), err)
}

/// Close the handle. The caller sees a [`super::sink::NET_CLOSED`] completion
/// when the descriptor is really gone.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_close(id: i64, err: *mut PerryNetError) -> i32 {
    finish(super::close(id), err)
}

/// Node's `ref()`/`unref()` for one handle.
#[no_mangle]
pub extern "C" fn js_perry_net_set_ref(id: i64, referenced: i32) -> i32 {
    match super::set_ref(id, referenced != 0) {
        Ok(()) => PERRY_NET_OK,
        Err(_) => PERRY_NET_ERR,
    }
}

/// Bytes handed to the driver and not yet reported written.
#[no_mangle]
pub extern "C" fn js_perry_net_queued_bytes(id: i64) -> usize {
    super::queued_bytes(id)
}

/// Arm — or move — a subsystem-owned one-shot deadline `delay_ms` from now
/// (P5). `id` is the caller's own id for the deadline, from the same shared
/// allocator socket ids come from, so it cannot collide with one.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_timer_arm(
    id: i64,
    subsystem: i32,
    delay_ms: u64,
    err: *mut PerryNetError,
) -> i32 {
    if subsystem < 0 || subsystem as usize >= super::MAX_SUBSYSTEMS {
        return finish(Err(super::errors::invalid_input("timer")), err);
    }
    finish(super::timer_arm(id, subsystem as u8, delay_ms), err)
}

/// Hand a live socket to another subsystem, keeping its id (P5).
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_transfer(
    id: i64,
    subsystem: i32,
    err: *mut PerryNetError,
) -> i32 {
    if subsystem < 0 {
        return finish(Err(super::errors::invalid_input("transfer")), err);
    }
    finish(super::transfer(id, subsystem as u8), err)
}

/// Cancel a deadline. Idempotent.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_timer_cancel(id: i64, err: *mut PerryNetError) -> i32 {
    finish(super::timer_cancel(id), err)
}

/// Disarm a deadline but keep its handle, so re-arming it costs no completion.
/// Idempotent.
///
/// # Safety
/// `err` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_timer_park(id: i64, err: *mut PerryNetError) -> i32 {
    finish(super::timer_park(id), err)
}

/// Nonzero when `id` names a live turnloop-backed handle on this thread.
#[no_mangle]
pub extern "C" fn js_perry_net_is_live(id: i64) -> i32 {
    i32::from(super::is_live(id))
}

/// Nonzero when a sink is installed for `subsystem`. A binding uses it to
/// confirm its own registration took; a test uses it so a "turnloop handled
/// this" claim cannot pass with nothing listening.
#[no_mangle]
pub extern "C" fn js_perry_net_sink_installed(subsystem: i32) -> i32 {
    i32::from(subsystem >= 0 && super::sink_installed(subsystem as u8))
}

/// Number of live turnloop-backed handles on this thread. A test that claims
/// turnloop carried a workload must see this above zero while it runs.
#[no_mangle]
pub extern "C" fn js_perry_net_live_handles() -> usize {
    super::live_handles()
}

/// Write one endpoint into `out` as text, returning its port.
///
/// Returns [`PERRY_NET_ERR`] when the handle has no such endpoint. `out_len`
/// receives the written byte count; the address is truncated (never split
/// mid-UTF-8, since it is always ASCII) if `cap` is too small.
///
/// # Safety
/// `out` must be writable for `cap` bytes; `out_len`, `out_port` and
/// `out_family` must be null or writable.
unsafe fn write_addr(
    addr: Option<SocketAddr>,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_port: *mut u16,
    out_family: *mut i32,
) -> i32 {
    let Some(addr) = addr else {
        return PERRY_NET_ERR;
    };
    let text = addr.ip().to_string();
    let bytes = text.as_bytes();
    let n = bytes.len().min(cap);
    if !out.is_null() && n > 0 {
        // SAFETY: the caller promises `cap` writable bytes and `n <= cap`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, n) };
    }
    if !out_len.is_null() {
        // SAFETY: caller-supplied writable `usize`.
        // GC_STORE_AUDIT(POINTER_FREE): a length into a caller out-param.
        unsafe { std::ptr::write(out_len, n) };
    }
    if !out_port.is_null() {
        // SAFETY: caller-supplied writable `u16`.
        // GC_STORE_AUDIT(POINTER_FREE): a `u16` port into a caller out-param.
        unsafe { std::ptr::write(out_port, addr.port()) };
    }
    if !out_family.is_null() {
        // SAFETY: caller-supplied writable `i32`.
        // GC_STORE_AUDIT(POINTER_FREE): a `u8` address family into a caller out-param.
        unsafe { std::ptr::write(out_family, if addr.is_ipv6() { 6 } else { 4 }) };
    }
    PERRY_NET_OK
}

/// `server.address()` / `socket.localAddress` + `localPort` + `localFamily`.
///
/// # Safety
/// See [`write_addr`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_local_address(
    id: i64,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_port: *mut u16,
    out_family: *mut i32,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    unsafe {
        write_addr(
            super::local_addr(id),
            out,
            cap,
            out_len,
            out_port,
            out_family,
        )
    }
}

/// `socket.remoteAddress` + `remotePort` + `remoteFamily`.
///
/// # Safety
/// See [`write_addr`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_peer_address(
    id: i64,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_port: *mut u16,
    out_family: *mut i32,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    unsafe {
        write_addr(
            super::peer_addr(id),
            out,
            cap,
            out_len,
            out_port,
            out_family,
        )
    }
}

/// Borrow a completion's read payload. Exists so a binding written against
/// this ABI never has to reconstruct the slice itself.
///
/// # Safety
/// `completion` must be the pointer the sink was called with, and the call
/// must still be on the stack.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_completion_bytes(
    completion: *const NetCompletion,
    out_len: *mut usize,
) -> *const u8 {
    if completion.is_null() {
        if !out_len.is_null() {
            // SAFETY: caller-supplied writable `usize`.
            // GC_STORE_AUDIT(POINTER_FREE): a length into a caller out-param.
            unsafe { std::ptr::write(out_len, 0) };
        }
        return std::ptr::null();
    }
    // SAFETY: the caller promises a live completion pointer.
    let c = unsafe { &*completion };
    if !out_len.is_null() {
        // SAFETY: caller-supplied writable `usize`.
        // GC_STORE_AUDIT(POINTER_FREE): a length into a caller out-param.
        unsafe { std::ptr::write(out_len, c.len) };
    }
    c.data
}

// ── Link routes (NET-TRANSPORT-DESIGN P0) ───────────────────────────────────
//
// Every call takes the payload's `TransportCore` (offset 0 of the binding's
// `TransportPayload<E>`) and its owner link (`js_perry_payload_owner_link`)
// instead of an id. The runtime checks that the link's payload is OPEN on this
// thread and that `core` is its first field before touching either.

use super::transport::{self, TransportCore};
use crate::native_payload::OwnerLink;

/// Install a link-routed binding's completion sink (no id allocator: the sink
/// installs accepted connections itself). Returns nonzero on success.
#[no_mangle]
pub extern "C" fn js_perry_net_register_link_sink(subsystem: i32, sink: SinkFn) -> i32 {
    if subsystem < 0 || subsystem as usize >= super::MAX_LINK_ROUTES {
        return 0;
    }
    i32::from(super::register_link_sink(subsystem as u8, sink))
}

/// Construct an idle core for link route `route` in the binding's opaque
/// block. Returns [`PERRY_NET_ERR`] (and writes nothing) for a route outside
/// the sink table.
///
/// # Safety
/// `core` is writable for `TRANSPORT_CORE_WORDS` words and 8-aligned.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_core_init(core: *mut c_void, route: i32) -> i32 {
    if core.is_null() || route < 0 || route as usize >= super::MAX_LINK_ROUTES {
        return PERRY_NET_ERR;
    }
    // SAFETY: forwarded contract.
    unsafe { transport::core_init(core.cast::<TransportCore>(), route as u8) };
    PERRY_NET_OK
}

/// Drop a core in place. Frees memory only (it runs in a collection, at
/// release or at thread teardown): it never reaches the loop.
///
/// # Safety
/// `core` was initialized by [`js_perry_net_core_init`] and is dead after.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_core_drop(core: *mut c_void) {
    if !core.is_null() {
        // SAFETY: forwarded contract.
        unsafe { transport::core_drop(core.cast::<TransportCore>()) };
    }
}

fn link_arg(link: usize) -> OwnerLink {
    OwnerLink(link)
}

/// Link form of [`js_perry_net_tcp_listen`].
///
/// # Safety
/// `core`/`link` name one OPEN payload of this thread; `host` as there.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_tcp_listen(
    core: *mut c_void,
    link: usize,
    host: *const u8,
    host_len: usize,
    port: u16,
    backlog: u32,
    reuse_port: i32,
    nodelay: i32,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let host = unsafe { str_arg(host, host_len) };
    let host = if host.is_empty() { "0.0.0.0" } else { host };
    let Ok(addr) = parse_bind_addr(host, port) else {
        PerryNetError::write(
            err,
            NodeError {
                code: "EINVAL",
                errno: 0,
                syscall: "listen",
            },
        );
        return PERRY_NET_ERR;
    };
    // SAFETY: forwarded contract.
    let result = unsafe {
        transport::tcp_listen(
            core.cast(),
            link_arg(link),
            addr,
            backlog,
            reuse_port != 0,
            nodelay != 0,
        )
    };
    finish(result.map(|_| ()), err)
}

/// Link form of [`js_perry_net_pipe_listen`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_pipe_listen(
    core: *mut c_void,
    link: usize,
    path: *const u8,
    path_len: usize,
    backlog: u32,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract.
    let path = unsafe { str_arg(path, path_len) };
    finish(
        unsafe {
            transport::pipe_listen(core.cast(), link_arg(link), &PathBuf::from(path), backlog)
        },
        err,
    )
}

/// Link form of [`js_perry_net_accept_start`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_accept_start(
    core: *mut c_void,
    link: usize,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::accept_start(core.cast(), link_arg(link)) },
        err,
    )
}

/// Install the connection of the accept completion the sink is handling into
/// a fresh payload's core (+1 ref on that payload's cell). A connection the
/// sink does not install is closed by the runtime when the sink returns.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`]; `completion` is the sink's argument
/// and the sink call is still on the stack.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_install_accepted(
    core: *mut c_void,
    link: usize,
    completion: *const NetCompletion,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::install_accepted(core.cast(), link_arg(link), completion) },
        err,
    )
}

/// Link form of [`js_perry_net_tcp_connect`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_tcp_connect(
    core: *mut c_void,
    link: usize,
    host: *const u8,
    host_len: usize,
    port: u16,
    nodelay: i32,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract.
    let host = unsafe { str_arg(host, host_len) };
    let host = if host.is_empty() { "127.0.0.1" } else { host };
    finish(
        unsafe { transport::tcp_connect(core.cast(), link_arg(link), host, port, nodelay != 0) },
        err,
    )
}

/// Link form of [`js_perry_net_pipe_connect`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_pipe_connect(
    core: *mut c_void,
    link: usize,
    path: *const u8,
    path_len: usize,
    err: *mut PerryNetError,
) -> i32 {
    // SAFETY: forwarded contract.
    let path = unsafe { str_arg(path, path_len) };
    finish(
        unsafe { transport::pipe_connect(core.cast(), link_arg(link), &PathBuf::from(path)) },
        err,
    )
}

/// Link form of [`js_perry_net_adopt_stream`]: `socket` is consumed on every
/// outcome.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`] and [`js_perry_net_adopt_stream`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_adopt_stream(
    core: *mut c_void,
    link: usize,
    socket: i64,
    err: *mut PerryNetError,
) -> i32 {
    if socket < 0 {
        return finish(
            Err(super::map_error(
                turnloop::Error::new(turnloop::ErrorKind::InvalidInput),
                "adopt",
            )),
            err,
        );
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: the caller transfers ownership of an open descriptor.
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(socket as i32) };
        finish(
            unsafe { transport::adopt_stream(core.cast(), link_arg(link), fd) },
            err,
        )
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawSocket;
        // SAFETY: the caller transfers ownership of an open socket.
        let sock = unsafe { std::os::windows::io::OwnedSocket::from_raw_socket(socket as u64) };
        finish(
            unsafe { transport::adopt_stream(core.cast(), link_arg(link), sock) },
            err,
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (core, link);
        finish(
            Err(super::map_error(
                turnloop::Error::new(turnloop::ErrorKind::Unsupported),
                "adopt",
            )),
            err,
        )
    }
}

/// Link form of [`js_perry_net_read_start`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_read_start(
    core: *mut c_void,
    link: usize,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::read_start(core.cast(), link_arg(link)) },
        err,
    )
}

/// Link form of [`js_perry_net_write`] (the bytes are copied).
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`] and [`js_perry_net_write`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_write(
    core: *mut c_void,
    link: usize,
    bytes: *const u8,
    len: usize,
    user: u64,
    out_queued: *mut usize,
    err: *mut PerryNetError,
) -> i32 {
    let owned = if bytes.is_null() || len == 0 {
        Vec::new()
    } else {
        // SAFETY: the caller promises a readable range for `len` bytes.
        unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec()
    };
    match unsafe { transport::write(core.cast(), link_arg(link), owned, user) } {
        Ok(queued) => {
            if !out_queued.is_null() {
                // SAFETY: the caller supplies a writable `usize`.
                // GC_STORE_AUDIT(POINTER_FREE): a queued-byte count into a caller out-param.
                unsafe { std::ptr::write(out_queued, queued) };
            }
            PERRY_NET_OK
        }
        Err(e) => finish(Err(e), err),
    }
}

/// Link form of [`js_perry_net_shutdown`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_shutdown(
    core: *mut c_void,
    link: usize,
    user: u64,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::shutdown(core.cast(), link_arg(link), user) },
        err,
    )
}

/// Release the core's driver resources (the handle moves into the driver's
/// close). `*out_closed` is 1 when a `NET_CLOSED` completion for this link
/// will follow, 0 when there was no handle. The binding then releases the
/// payload.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`]; `out_closed` null or writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_close(
    core: *mut c_void,
    link: usize,
    out_closed: *mut i32,
    err: *mut PerryNetError,
) -> i32 {
    match unsafe { transport::close(core.cast(), link_arg(link)) } {
        Ok(closed) => {
            if !out_closed.is_null() {
                // SAFETY: caller-supplied writable `i32`.
                // GC_STORE_AUDIT(POINTER_FREE): a flag into a caller out-param.
                unsafe { std::ptr::write(out_closed, i32::from(closed)) };
            }
            PERRY_NET_OK
        }
        Err(e) => finish(Err(e), err),
    }
}

/// Link form of [`js_perry_net_set_ref`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_set_ref(
    core: *mut c_void,
    link: usize,
    referenced: i32,
) -> i32 {
    match unsafe { transport::set_ref(core.cast(), link_arg(link), referenced != 0) } {
        Ok(()) => PERRY_NET_OK,
        Err(_) => PERRY_NET_ERR,
    }
}

/// Link form of [`js_perry_net_queued_bytes`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_queued_bytes(core: *mut c_void, link: usize) -> usize {
    unsafe { transport::queued_bytes(core.cast(), link_arg(link)) }
}

/// Arm or move the socket's deadline (link form of [`js_perry_net_timer_arm`]).
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_deadline_arm(
    core: *mut c_void,
    link: usize,
    delay_ms: u64,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::deadline_arm(core.cast(), link_arg(link), delay_ms) },
        err,
    )
}

/// Park the socket's deadline (link form of [`js_perry_net_timer_park`]).
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_deadline_park(
    core: *mut c_void,
    link: usize,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::deadline_park(core.cast(), link_arg(link)) },
        err,
    )
}

/// Cancel the socket's deadline (link form of [`js_perry_net_timer_cancel`]).
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_deadline_cancel(
    core: *mut c_void,
    link: usize,
    err: *mut PerryNetError,
) -> i32 {
    finish(
        unsafe { transport::deadline_cancel(core.cast(), link_arg(link)) },
        err,
    )
}

/// Link form of [`js_perry_net_local_address`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`] and [`write_addr`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_local_address(
    core: *mut c_void,
    link: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_port: *mut u16,
    out_family: *mut i32,
) -> i32 {
    // SAFETY: forwarded contract.
    unsafe {
        write_addr(
            transport::local_addr(core.cast(), link_arg(link)),
            out,
            cap,
            out_len,
            out_port,
            out_family,
        )
    }
}

/// Link form of [`js_perry_net_peer_address`].
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`] and [`write_addr`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_peer_address(
    core: *mut c_void,
    link: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_port: *mut u16,
    out_family: *mut i32,
) -> i32 {
    // SAFETY: forwarded contract.
    unsafe {
        write_addr(
            transport::peer_addr(core.cast(), link_arg(link)),
            out,
            cap,
            out_len,
            out_port,
            out_family,
        )
    }
}

/// Change the link sink without cancelling or replacing outstanding I/O.
/// # Safety
/// As js_perry_net_link_tcp_listen; route names a registered link sink.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_set_route(
    core: *mut c_void,
    link: usize,
    route: i32,
    err: *mut PerryNetError,
) -> i32 {
    if !(0..super::MAX_LINK_ROUTES as i32).contains(&route) {
        return finish(Err(super::transport::bad("route")), err);
    }
    finish(
        super::transport::set_route(core.cast(), link_arg(link), route as u8),
        err,
    )
}

/// The installed handle as four u32s; 0 when the core holds none.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`]; `out` names four writable u32s.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_handle_parts(
    core: *mut c_void,
    link: usize,
    out: *mut u32,
) -> i32 {
    match unsafe { transport::snapshot_handle_parts(core.cast(), link_arg(link)) } {
        Some(parts) if !out.is_null() => {
            unsafe { std::ptr::copy_nonoverlapping(parts.as_ptr(), out, 4) };
            1
        }
        _ => 0,
    }
}

/// Copy the core's current capability (handle or pending resolve) into the
/// caller's opaque [`transport::HANDLE_SNAPSHOT_WORDS`]-word block.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`]; `out` names the opaque block.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_snapshot_handle(
    core: *mut c_void,
    link: usize,
    out: *mut c_void,
) -> i32 {
    match unsafe { transport::snapshot_handle(core.cast(), link_arg(link)) } {
        Some(snapshot) if !out.is_null() => {
            // GC_STORE_AUDIT(POINTER_FREE): caller-owned opaque capability snapshot;
            // numeric driver handle/generation/resolve ids, no GC pointers or JS values.
            unsafe { std::ptr::write(out.cast::<transport::HandleSnapshot>(), snapshot) };
            1
        }
        _ => 0,
    }
}

/// Whether the core still holds the snapshot's handle or resolve.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`]; `snapshot` came from
/// [`js_perry_net_link_snapshot_handle`] returning 1.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_handle_matches(
    core: *mut c_void,
    link: usize,
    snapshot: *const c_void,
) -> i32 {
    if snapshot.is_null() {
        return 0;
    }
    let snapshot = unsafe { std::ptr::read(snapshot.cast::<transport::HandleSnapshot>()) };
    i32::from(unsafe { transport::handle_matches(core.cast(), link_arg(link), snapshot) })
}

/// Heap bytes the core owns (backlog, write records, plan, pipe path).
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_retained_bytes(core: *mut c_void, link: usize) -> usize {
    unsafe { transport::retained_bytes(core.cast(), link_arg(link)) }
}

/// Deliver decoded TLS plaintext (or its EOF) on the core's current route.
///
/// # Safety
/// As [`js_perry_net_link_tcp_listen`]; `bytes` is readable for `len`.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_link_dispatch_plaintext(
    core: *mut c_void,
    link: usize,
    bytes: *const u8,
    len: usize,
    eof: i32,
    err: *mut PerryNetError,
) -> i32 {
    let bytes = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    finish(
        unsafe { transport::dispatch_plaintext(core.cast(), link_arg(link), bytes, eof != 0) },
        err,
    )
}
