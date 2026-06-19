//! `node:cluster` scheduling: primary-coordinated shared ports + SCHED_RR
//! fd-passing (#4962, follow-up to #4914).
//!
//! Two Node-fidelity behaviors live here, both riding the existing
//! `child_process.fork()` IPC socketpair (fd 3 in the worker):
//!
//! 1. **`listen(0)` shared ephemeral port.** A worker that binds port 0 (or any
//!    shared port) first asks the primary for the concrete port via a
//!    `queryServer` round-trip (`{cmd:"NODE_CLUSTER",act:"queryServer"}`). The
//!    primary binds/reserves the address once and replies with the resolved
//!    port (`act:"queryServerReply"`); every worker then ends up on the *same*
//!    port instead of N different OS-assigned ephemerals.
//!
//! 2. **`SCHED_RR` fd-passing.** Under round-robin scheduling the primary owns
//!    the listening socket, runs an accept loop, and hands each accepted
//!    connection's fd to the next worker over that worker's IPC socketpair with
//!    `SCM_RIGHTS` (tagged with a small binary frame so the worker reader can
//!    tell an fd from a JSON message). Workers under RR do not bind at all;
//!    they pull injected fds and serve them as if locally accepted.
//!
//! `SCHED_NONE` keeps the #4914 SO_REUSEPORT path (kernel-balanced) and just
//! layers the shared-port round-trip on top.

#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::collections::VecDeque;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
#[cfg(unix)]
use std::sync::{Condvar, Mutex};
#[cfg(unix)]
use std::time::Duration;

// Binary fd-frame on the IPC byte stream: a leading NUL (never present in the
// newline-delimited JSON the channel otherwise carries) followed by the 4-byte
// big-endian routing key id. The accompanying fd rides in the `SCM_RIGHTS`
// ancillary data of the same `sendmsg`.
#[cfg(unix)]
const FD_FRAME_TAG: u8 = 0x00;
#[cfg(unix)]
const FD_FRAME_LEN: usize = 5; // tag + u32 key id

/// Stable 32-bit routing key for an address. Both primary (fd-frame tag) and
/// worker (injection queue) derive it from the *resolved* address so they
/// always agree, even when the worker originally requested port 0.
pub fn compute_key_id(host: &str, port: u16, address_type: i32) -> u32 {
    // FNV-1a over "<addrType>:<host>:<port>" — small, stable, no_std-friendly.
    let mut hash: u32 = 0x811c_9dc5;
    let s = format!("{address_type}:{host}:{port}");
    for b in s.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// `reqKey` echoed in the queryServer round-trip so a worker can match a reply
/// to the exact (possibly port-0) listen it issued. The scheduling mode is part
/// of the key so an RR query never reuses a non-RR primary entry (which would
/// return a port but spawn no accept thread, hanging the RR worker).
#[cfg(unix)]
fn req_key(host: &str, req_port: i32, address_type: i32, rr: bool) -> String {
    let mode = if rr { "rr" } else { "n" };
    format!("{address_type}:{mode}:{host}:{req_port}")
}

// ---------------------------------------------------------------------------
// Worker side
// ---------------------------------------------------------------------------

/// Injected connection fds awaiting a worker accept loop, keyed by routing key
/// id, plus the channel-closed flag. Both live under one mutex (paired with
/// `fd_cv`) so `recv_fd`'s predicate check and condvar wait are atomic against
/// the close handler — a separate `closed` lock would let a notify slip between
/// the check and the wait and hang the waiter forever.
#[cfg(unix)]
#[derive(Default)]
struct FdInbox {
    queues: HashMap<u32, VecDeque<RawFd>>,
    closed: bool,
}

#[cfg(unix)]
struct WorkerState {
    /// In-flight queryServer round-trips: reqKey -> resolved port (None until
    /// the primary replies).
    queries: Mutex<HashMap<String, Option<i32>>>,
    query_cv: Condvar,
    fd_inbox: Mutex<FdInbox>,
    fd_cv: Condvar,
}

#[cfg(unix)]
impl WorkerState {
    fn new() -> Self {
        Self {
            queries: Mutex::new(HashMap::new()),
            query_cv: Condvar::new(),
            fd_inbox: Mutex::new(FdInbox::default()),
            fd_cv: Condvar::new(),
        }
    }
}

#[cfg(unix)]
fn worker_state() -> &'static WorkerState {
    use std::sync::OnceLock;
    static WS: OnceLock<WorkerState> = OnceLock::new();
    WS.get_or_init(WorkerState::new)
}

/// True when round-robin scheduling is in effect for this worker (Node's
/// non-Windows default). The primary forwards its policy via
/// `NODE_CLUSTER_SCHED_POLICY` in the worker env.
pub fn worker_sched_is_rr() -> bool {
    std::env::var("NODE_CLUSTER_SCHED_POLICY")
        .map(|p| p != "none")
        .unwrap_or(true)
}

/// Ask the primary for the concrete port to bind `host:req_port`. Blocks on the
/// IPC reply (bounded timeout so an absent/slow primary degrades to the
/// caller's own bind instead of hanging). Returns the resolved port, or `None`
/// on timeout/failure. `rr` selects fd-passing (primary owns the socket) vs the
/// reuseport reservation used by SCHED_NONE and the non-injectable listeners.
#[cfg(unix)]
pub fn worker_query_listen(host: &str, req_port: i32, address_type: i32, rr: bool) -> Option<u16> {
    let key = req_key(host, req_port, address_type, rr);
    {
        let mut q = worker_state().queries.lock().unwrap();
        q.insert(key.clone(), None);
    }
    let json = format!(
        "{{\"cmd\":\"NODE_CLUSTER\",\"act\":\"queryServer\",\"reqKey\":\"{}\",\"host\":\"{}\",\"port\":{},\"addressType\":{},\"rr\":{}}}",
        key.replace('\\', "\\\\").replace('"', "\\\""),
        host.replace('\\', "\\\\").replace('"', "\\\""),
        req_port,
        address_type,
        if rr { 1 } else { 0 }
    );
    if !crate::process::ipc::process_ipc_send_raw_json(&json) {
        worker_state().queries.lock().unwrap().remove(&key);
        return None;
    }

    // Absolute deadline: spurious/early wakeups must not extend the total wait
    // past the 10s cap (a per-iteration `wait_timeout(10s)` would).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut q = worker_state().queries.lock().unwrap();
    loop {
        if let Some(Some(port)) = q.get(&key).copied() {
            q.remove(&key);
            return if (0..=u16::MAX as i32).contains(&port) {
                Some(port as u16)
            } else {
                None
            };
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            q.remove(&key);
            return None;
        }
        q = worker_state()
            .query_cv
            .wait_timeout(q, remaining)
            .unwrap()
            .0;
    }
}

/// Block until the primary passes a connection fd for `key_id` (SCHED_RR
/// accept loop). Returns the raw fd, or -1 once the IPC channel closes.
#[cfg(unix)]
pub fn worker_recv_fd(key_id: u32) -> RawFd {
    let mut inbox = worker_state().fd_inbox.lock().unwrap();
    loop {
        if let Some(fd) = inbox.queues.get_mut(&key_id).and_then(|q| q.pop_front()) {
            return fd;
        }
        if inbox.closed {
            return -1;
        }
        inbox = worker_state().fd_cv.wait(inbox).unwrap();
    }
}

/// Worker IPC read loop for cluster workers. Replaces the plain
/// `BufReader::lines()` reader (which cannot receive `SCM_RIGHTS`) with a
/// `recvmsg` loop that splits the byte stream into JSON frames *and* routes
/// ancillary connection fds. `queryServerReply` frames are consumed here;
/// every other JSON line is handed to `on_message` exactly as before so normal
/// `process.on('message')` delivery is unaffected.
#[cfg(unix)]
pub fn worker_recv_loop(
    stream: std::os::unix::net::UnixStream,
    mut on_message: impl FnMut(String),
    on_closed: impl FnOnce(),
) {
    let fd = stream.as_raw_fd();
    // Keep the stream alive for the loop's lifetime; the fd is borrowed.
    let _owned = stream;

    // Data buffer == ancillary fd capacity guarantees a single recvmsg can
    // never carry more fd-frames than the cmsg space holds, so MSG_CTRUNC
    // cannot silently drop a passed fd (every fd-frame is >= 1 data byte).
    const BUF: usize = 1024;
    let mut data = [0u8; BUF];
    let mut acc: Vec<u8> = Vec::with_capacity(BUF * 2);
    let mut fd_queue: VecDeque<RawFd> = VecDeque::new();
    let cmsg_space =
        unsafe { libc::CMSG_SPACE((BUF * std::mem::size_of::<RawFd>()) as u32) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    loop {
        let mut iov = libc::iovec {
            iov_base: data.as_mut_ptr() as *mut libc::c_void,
            iov_len: data.len(),
        };
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = cmsg_buf.len() as _;

        let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
        if n <= 0 {
            break; // EOF or error
        }

        // Collect any passed fds (in order) before touching the data bytes.
        unsafe {
            let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data_ptr = libc::CMSG_DATA(cmsg) as *const RawFd;
                    let payload = (*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                    let count = payload / std::mem::size_of::<RawFd>();
                    for i in 0..count {
                        fd_queue.push_back(std::ptr::read_unaligned(data_ptr.add(i)));
                    }
                }
                cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
            }
        }

        acc.extend_from_slice(&data[..n as usize]);
        drain_frames(&mut acc, &mut fd_queue, &mut on_message);
    }

    // Channel closed: flag it + drain any already-routed-but-unpulled fds under
    // the inbox lock, then wake every worker blocked in `recv_fd`.
    {
        let mut inbox = worker_state().fd_inbox.lock().unwrap();
        inbox.closed = true;
        for q in inbox.queues.values_mut() {
            for fd in q.drain(..) {
                unsafe {
                    libc::close(fd);
                }
            }
        }
    }
    worker_state().fd_cv.notify_all();
    // Reader-local fds received but not yet routed to a queue.
    for fd in fd_queue.drain(..) {
        unsafe {
            libc::close(fd);
        }
    }
    on_closed();
}

/// Split `acc` into complete frames in place: NUL-led binary fd-frames route a
/// queued fd to its injection queue; newline-delimited JSON lines are either
/// consumed (queryServerReply) or forwarded to `on_message`.
#[cfg(unix)]
fn drain_frames(
    acc: &mut Vec<u8>,
    fd_queue: &mut VecDeque<RawFd>,
    on_message: &mut impl FnMut(String),
) {
    let mut pos = 0usize;
    loop {
        if pos >= acc.len() {
            break;
        }
        if acc[pos] == FD_FRAME_TAG {
            if acc.len() - pos < FD_FRAME_LEN {
                break; // partial fd-frame; wait for more bytes
            }
            let key_id =
                u32::from_be_bytes([acc[pos + 1], acc[pos + 2], acc[pos + 3], acc[pos + 4]]);
            // The fd for this frame arrived no later than its tag byte, so it
            // is already queued (FIFO matches frame order).
            if let Some(cfd) = fd_queue.pop_front() {
                let mut inbox = worker_state().fd_inbox.lock().unwrap();
                inbox.queues.entry(key_id).or_default().push_back(cfd);
                drop(inbox);
                worker_state().fd_cv.notify_all();
            }
            pos += FD_FRAME_LEN;
            continue;
        }
        // JSON line up to the next newline.
        match acc[pos..].iter().position(|&b| b == b'\n') {
            Some(rel) => {
                let line = &acc[pos..pos + rel];
                if !line.is_empty() && !try_consume_query_reply(line) {
                    if let Ok(s) = std::str::from_utf8(line) {
                        on_message(s.to_string());
                    }
                }
                pos += rel + 1;
            }
            None => break, // partial JSON line
        }
    }
    if pos > 0 {
        acc.drain(..pos);
    }
}

/// Resolve a `queryServerReply` line (lightweight scan — runs on the reader
/// thread, so no JS-runtime parsing). Returns true if the line was a reply.
#[cfg(unix)]
fn try_consume_query_reply(line: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(line) else {
        return false;
    };
    if !s.contains("\"act\":\"queryServerReply\"") {
        return false;
    }
    let Some(req_key) = json_str_field(s, "reqKey") else {
        return false;
    };
    let port = json_num_field(s, "port").unwrap_or(-1);
    let mut q = worker_state().queries.lock().unwrap();
    if let Some(slot) = q.get_mut(&req_key) {
        *slot = Some(port);
        drop(q);
        worker_state().query_cv.notify_all();
    }
    true
}

/// Extract `"name":"<value>"` from a flat JSON object string (no nested escapes
/// beyond the `\\`/`\"` our own senders produce).
#[cfg(unix)]
fn json_str_field(s: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\":\"");
    let start = s.find(&needle)? + needle.len();
    let rest = &s[start..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            _ => out.push(c),
        }
    }
    None
}

/// Extract `"name":<number>` from a flat JSON object string.
#[cfg(unix)]
fn json_num_field(s: &str, name: &str) -> Option<i32> {
    let needle = format!("\"{name}\":");
    let start = s.find(&needle)? + needle.len();
    let rest = &s[start..];
    let end = rest
        .find(|c: char| c != '-' && !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse::<i32>().ok()
}

// ---------------------------------------------------------------------------
// Primary side
// ---------------------------------------------------------------------------

#[cfg(unix)]
struct PrimaryKey {
    actual_port: u16,
    key_id: u32,
    #[allow(dead_code)] // retained for diagnostics / future per-key policy
    rr: bool,
    inner: Mutex<PrimaryKeyInner>,
}

#[cfg(unix)]
#[derive(Default)]
struct PrimaryKeyInner {
    workers: Vec<u64>,
    rr_index: usize,
}

#[cfg(unix)]
fn primary_keys() -> &'static Mutex<HashMap<String, std::sync::Arc<PrimaryKey>>> {
    use std::sync::OnceLock;
    static KEYS: OnceLock<Mutex<HashMap<String, std::sync::Arc<PrimaryKey>>>> = OnceLock::new();
    KEYS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Handle a worker's `queryServer`: bind/reserve the address once per key, add
/// the worker to its round-robin set, and return the resolved port the worker
/// should report. `None` means the primary could not bind (worker falls back).
#[cfg(unix)]
pub fn primary_handle_query(
    worker_handle: u64,
    host: &str,
    req_port: i32,
    address_type: i32,
    rr: bool,
) -> Option<u16> {
    let key = req_key(host, req_port, address_type, rr);
    let mut map = primary_keys().lock().unwrap();
    if let Some(existing) = map.get(&key) {
        let mut inner = existing.inner.lock().unwrap();
        if !inner.workers.contains(&worker_handle) {
            inner.workers.push(worker_handle);
        }
        return Some(existing.actual_port);
    }

    // Default the wildcard bind to the requested family — `0.0.0.0` would force
    // an IPv6 (`addressType == 6`) listen onto the wrong family.
    let bind_host = if !host.is_empty() {
        host
    } else if address_type == 6 {
        "::"
    } else {
        "0.0.0.0"
    };
    let bind_port = req_port.max(0) as u16;
    let listener = bind_primary_listener(bind_host, bind_port, address_type, rr)?;
    let actual_port = listener.local_addr().ok()?.port();
    let key_id = compute_key_id(host, actual_port, address_type);

    let entry = std::sync::Arc::new(PrimaryKey {
        actual_port,
        key_id,
        rr,
        inner: Mutex::new(PrimaryKeyInner {
            workers: vec![worker_handle],
            rr_index: 0,
        }),
    });

    if rr {
        // Primary owns the socket and distributes accepted fds round-robin.
        spawn_accept_thread(entry.clone(), listener);
    } else {
        // SCHED_NONE / non-injectable: the bind only served to *discover* a free
        // port (so `listen(0)` resolves to one shared port). Drop it — a bound
        // primary socket would otherwise sit in the kernel SO_REUSEPORT set and,
        // on macOS, swallow a share of connections it never accepts. The
        // workers' own SO_REUSEPORT listeners (#4914) own the port from here.
        drop(listener);
    }

    map.insert(key, entry);
    Some(actual_port)
}

/// Drop a dead worker from every key's rotation so the accept loop stops
/// targeting it. Called from the cluster `exit` path.
#[cfg(unix)]
pub fn primary_remove_worker(worker_handle: u64) {
    let map = primary_keys().lock().unwrap();
    for entry in map.values() {
        let mut inner = entry.inner.lock().unwrap();
        if let Some(pos) = inner.workers.iter().position(|h| *h == worker_handle) {
            inner.workers.remove(pos);
            if inner.rr_index >= inner.workers.len().max(1) {
                inner.rr_index = 0;
            }
        }
    }
}

#[cfg(unix)]
fn bind_primary_listener(
    host: &str,
    port: u16,
    address_type: i32,
    rr: bool,
) -> Option<std::net::TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::ToSocketAddrs;

    let addr = format!("{host}:{port}");
    let sock_addr = addr
        .to_socket_addrs()
        .ok()?
        .find(|a| {
            if address_type == 6 {
                a.is_ipv6()
            } else {
                a.is_ipv4()
            }
        })
        .or_else(|| addr.to_socket_addrs().ok()?.next())?;

    let socket = Socket::new(
        Domain::for_address(sock_addr),
        Type::STREAM,
        Some(Protocol::TCP),
    )
    .ok()?;
    socket.set_reuse_address(true).ok()?;
    if !rr {
        // Reserve the port in a way that coexists with the workers' own
        // SO_REUSEPORT binds.
        socket.set_reuse_port(true).ok()?;
    }
    socket.bind(&sock_addr.into()).ok()?;
    // RR: the primary owns the listening socket and accepts. SCHED_NONE: the
    // primary only *reserves* the port (bound, NOT listening) — the workers'
    // own SO_REUSEPORT listeners take every connection, so a listening primary
    // socket would join the kernel balance set and silently swallow a share.
    if rr {
        socket.listen(511).ok()?;
    }
    Some(socket.into())
}

/// Round-robin accept loop: pass each accepted connection's fd to the next live
/// worker over its IPC socketpair.
#[cfg(unix)]
fn spawn_accept_thread(entry: std::sync::Arc<PrimaryKey>, listener: std::net::TcpListener) {
    std::thread::spawn(move || {
        let key_id = entry.key_id;
        for accepted in listener.incoming() {
            let stream = match accepted {
                Ok(s) => s,
                Err(_) => continue,
            };
            let fd = stream.into_raw_fd();
            // SCM_RIGHTS dups the fd into the receiver, so the primary always
            // closes its own copy after the send (whether or not a worker took
            // it) — the connection lives on through the worker's dup.
            dispatch_fd_round_robin(&entry, key_id, fd);
            unsafe {
                libc::close(fd);
            }
        }
    });
}

/// Pass the fd to the next live worker in rotation. Returns whether any worker
/// took it.
#[cfg(unix)]
fn dispatch_fd_round_robin(entry: &PrimaryKey, key_id: u32, fd: RawFd) -> bool {
    let mut inner = entry.inner.lock().unwrap();
    let n = inner.workers.len();
    if n == 0 {
        return false;
    }
    let start = inner.rr_index % n;
    for off in 0..n {
        let idx = (start + off) % n;
        let handle = inner.workers[idx];
        if crate::child_process::reactor::cp_ipc_send_fd(handle, key_id, fd) {
            inner.rr_index = (idx + 1) % n;
            return true;
        }
    }
    false
}

/// Low-level `sendmsg` of one fd with its routing tag. Used by the reactor
/// (under the live-child lock, so writes to the socket stay serialized).
#[cfg(unix)]
pub fn send_fd_over(sock_fd: RawFd, key_id: u32, payload_fd: RawFd) -> bool {
    let mut frame = [0u8; FD_FRAME_LEN];
    frame[0] = FD_FRAME_TAG;
    frame[1..5].copy_from_slice(&key_id.to_be_bytes());

    let mut iov = libc::iovec {
        iov_base: frame.as_ptr() as *mut libc::c_void,
        iov_len: frame.len(),
    };
    let cmsg_space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg_buf.len() as _;

    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return false;
        }
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
        // GC_STORE_AUDIT(POINTER_FREE): writes a raw i32 fd into a stack/heap
        // ancillary-data buffer (never a GC heap slot, never a NaN-boxed ptr).
        std::ptr::write_unaligned(libc::CMSG_DATA(cmsg) as *mut RawFd, payload_fd);
        let sent = libc::sendmsg(sock_fd, &msg, 0);
        sent == frame.len() as isize
    }
}

/// Reconstruct a `std::net::TcpStream` from a passed fd (worker side).
#[cfg(unix)]
pub fn tcp_stream_from_fd(fd: RawFd) -> std::net::TcpStream {
    unsafe { std::net::TcpStream::from_raw_fd(fd) }
}

// ---------------------------------------------------------------------------
// C ABI surface for the out-of-crate listen sites (perry-ext-http-server has no
// Cargo dep on perry-runtime; these resolve at final link like perry-ffi's
// helpers and `perry_cluster_worker_listening`).
// ---------------------------------------------------------------------------

#[cfg(unix)]
unsafe fn read_host(ptr: *const u8, len: u32) -> String {
    if ptr.is_null() || len == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len as usize)).into_owned()
    }
}

/// 1 when this worker uses SCHED_RR, else 0. Always 0 off Unix.
#[no_mangle]
pub extern "C" fn perry_cluster_worker_sched_is_rr() -> i32 {
    #[cfg(unix)]
    {
        if crate::cluster::is_cluster_worker() && worker_sched_is_rr() {
            return 1;
        }
    }
    0
}

/// queryServer round-trip → resolved port, or -1 on timeout/failure/non-unix.
#[no_mangle]
pub extern "C" fn perry_cluster_worker_query_listen(
    host_ptr: *const u8,
    host_len: u32,
    port: i32,
    address_type: i32,
    rr: i32,
) -> i32 {
    #[cfg(unix)]
    {
        if !crate::cluster::is_cluster_worker() {
            return -1;
        }
        let host = unsafe { read_host(host_ptr, host_len) };
        match worker_query_listen(&host, port, address_type, rr != 0) {
            Some(p) => p as i32,
            None => -1,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (host_ptr, host_len, port, address_type, rr);
        -1
    }
}

/// Block for the next SCHED_RR connection fd for `key_id`; -1 on channel close.
#[no_mangle]
pub extern "C" fn perry_cluster_worker_recv_fd(key_id: u32) -> i32 {
    #[cfg(unix)]
    {
        worker_recv_fd(key_id) as i32
    }
    #[cfg(not(unix))]
    {
        let _ = key_id;
        -1
    }
}

/// Stable routing key id for a resolved address (matches the primary's tag).
#[no_mangle]
pub extern "C" fn perry_cluster_compute_key_id(
    host_ptr: *const u8,
    host_len: u32,
    port: i32,
    address_type: i32,
) -> u32 {
    #[cfg(unix)]
    {
        let host = unsafe { read_host(host_ptr, host_len) };
        compute_key_id(&host, port.max(0) as u16, address_type)
    }
    #[cfg(not(unix))]
    {
        let _ = (host_ptr, host_len, port, address_type);
        0
    }
}
