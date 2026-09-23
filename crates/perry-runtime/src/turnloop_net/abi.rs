//! The C ABI a separately linked net binding uses to drive the loop.
//!
//! `perry-ext-net` is a `staticlib` whose only Cargo dependency is `perry-ffi`;
//! it cannot hold a `&mut turnloop::Loop`, so every submission crosses this
//! boundary as primitives. The shape follows the event pump's existing
//! registration surface (`js_register_wait_driver`, `js_register_aux_pump`):
//! `#[no_mangle] extern "C"` functions, function pointers for callbacks, and
//! no Rust types in a signature.
//!
//! Every call must happen on the agent thread that owns the loop; each one
//! reports `PERRY_NET_ENOLOOP` rather than misbehaving if it does not. That is
//! the P1 coexistence contract: a worker agent has no loop until P3/P4, so its
//! binding keeps the tokio transport, and the two paths never share a socket.

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
pub const PERRY_NET_ABI_VERSION: u8 = 2;

/// A digest of [`NetCompletion`]'s layout plus [`PERRY_NET_ABI_VERSION`].
///
/// A binding declares its own `#[repr(C)]` copy of the completion struct — it
/// has no Cargo edge to this crate — so the two definitions can drift apart
/// silently, and the failure mode is reading a byte count out of a pointer
/// field. Both sides compute this from their own definition and compare once,
/// at registration, which turns that class of drift into a refused
/// registration instead of a corrupt read.
#[no_mangle]
pub extern "C" fn js_perry_net_abi_layout() -> u64 {
    use std::mem::{align_of, offset_of, size_of};
    (size_of::<NetCompletion>() as u64) << 48
        | (offset_of!(NetCompletion, id) as u64) << 40
        | (offset_of!(NetCompletion, data) as u64) << 32
        | (offset_of!(NetCompletion, code) as u64) << 24
        | (offset_of!(NetCompletion, syscall) as u64) << 16
        | (align_of::<NetCompletion>() as u64) << 8
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
    let error = turnloop::Error {
        kind: turnloop::ErrorKind::Other,
        os: (os != 0).then_some(os),
    };
    let mapped = super::map_error(error, "");
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

/// The host OS code for a Node error name, negated the way libuv reports
/// `err.errno`. Zero when the name is unknown to the table.
///
/// # Safety
/// `code`/`code_len` must describe a readable UTF-8 range.
#[no_mangle]
pub unsafe extern "C" fn js_perry_net_errno_for_code(code: *const u8, code_len: usize) -> i32 {
    // SAFETY: forwarded contract from this function's own safety note.
    let name = unsafe { str_arg(code, code_len) };
    super::errors::os_code_for_name(name).map_or(0, |os| -os)
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
        finish(super::adopt_stream(id, subsystem.max(0) as u8, fd), err)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawSocket;
        // SAFETY: the caller transfers ownership of an open socket.
        let sock = unsafe { std::os::windows::io::OwnedSocket::from_raw_socket(socket as u64) };
        finish(super::adopt_stream(id, subsystem.max(0) as u8, sock), err)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (id, subsystem);
        finish(
            Err(super::map_error(
                turnloop::Error::new(turnloop::ErrorKind::Unsupported),
                "adopt",
            )),
            err,
        )
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
        return finish(
            Err(super::map_error(
                turnloop::Error::new(turnloop::ErrorKind::InvalidInput),
                "timer",
            )),
            err,
        );
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
        return finish(
            Err(super::map_error(
                turnloop::Error::new(turnloop::ErrorKind::InvalidInput),
                "transfer",
            )),
            err,
        );
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
