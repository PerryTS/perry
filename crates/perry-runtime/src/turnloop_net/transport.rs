//! Transport cores: turnloop sockets owned by native payloads
//! (NET-TRANSPORT-DESIGN P0, #11919).
//!
//! A link-routed socket keeps no entry in this module's id maps. Its state is a
//! [`TransportCore`] at offset 0 of the family's payload ([`TransportPayload`]),
//! and every submission's token names the payload *cell*, so a completion
//! finds its socket with no lookup:
//!
//! ```text
//! token = 0b11 (link tag) | route:4 | op:4 | (cell >> 3):54
//! dispatch: link = cell; owner = link_event_owner(link)?; core = link_open_payload(link)
//! ```
//!
//! The cell is malloc'd, never moves and outlives its payload, so the token
//! stays valid across every collection. Three rules make that safe:
//!
//! 1. **The token is the route.** It carries the cell, the operation class and
//!    the sink route; no map is consulted.
//! 2. **A live driver resource pins its cell.** Each terminal completion the
//!    driver still owes holds one `link_ref` from submission until it is
//!    dispatched: a socket or listener handle's `Closed`, a resolve's result, a
//!    deadline timer handle's `Closed`. Reads, accepts and writes need no ref:
//!    they finish before their handle's `Closed` (DESIGN D4). So the sweep never
//!    finalizes a payload that holds a driver handle, `Drop for T` only frees
//!    memory, and no completion can reach a freed or reused cell.
//! 3. **Release moves the handle into the close.** [`close`] takes the handle
//!    out of the core and submits the driver's close under the cell's token;
//!    the binding then releases the payload (`native_payload::close`). The
//!    driver owns the descriptor until its `Closed`, which is dispatched
//!    through `link_event_owner` (it answers for a CLOSED cell) and unrefs.
//!
//! **Incarnation** is the driver's generational handle: a completion mutates
//! the core only when `core.entry.handle == completion.handle` (the plan's
//! resolve op for a resolve, `core.deadline` for a timer). A stale `Closed`
//! still delivers the owner-level `close` and its unref; any other stale
//! result is dropped, and a stale accept closes the new connection.
//!
//! The id route (`super::dispatch`'s maps) stays as it is until each binding
//! converts; [`owns`] is the transitional fork.

use std::cell::Cell;
use std::ffi::c_void;
use std::net::SocketAddr;
use std::path::Path;

use super::*;
use crate::native_payload::{self as np, OwnerLink};

/// Bits 63..62 set: a link token. The id route's op classes are 1..=10, the
/// process, pool and posted-job spaces sit at 0x10..=0x3F in the top byte and
/// the JS timer is `u64::MAX`, which no link token equals (its cell bits would
/// name an address at 2^57 - 8, above `LINK_ADDRESS_LIMIT`).
const LINK_TAG: u64 = 0b11 << 62;
const ROUTE_SHIFT: u32 = 58;
const OP_SHIFT: u32 = 54;
const CELL_MASK: u64 = (1 << OP_SHIFT) - 1;
/// A link token naming no cell: for a close whose `Closed` nobody waits on (a
/// refused accepted connection, an expired timer's handle has its own token).
pub(super) const LINK_NULL: Token = Token(LINK_TAG);

/// Words of the opaque block perry-ffi reserves for a [`TransportCore`]
/// (`perry_ffi::turnloop_net::TransportCore`). Folded into the ABI digest
/// (`js_perry_net_abi_layout`), so a binding built against another block size
/// is refused at registration; the assertion below keeps the core inside it.
#[cfg(not(windows))]
pub const TRANSPORT_CORE_WORDS: usize = 39;
/// Windows' `Entry` carries the pipe drain and EOF-timer fields (48 bytes)
/// and a wider `PathBuf` (8): 46 words by construction, reserved with a
/// two-word margin because no Windows build checked it exactly; the
/// assertion below fails that build if the estimate is short.
#[cfg(windows)]
pub const TRANSPORT_CORE_WORDS: usize = 48;

const _: () = assert!(
    std::mem::size_of::<TransportCore>() <= TRANSPORT_CORE_WORDS * 8
        && std::mem::align_of::<TransportCore>() <= 8,
    "TransportCore outgrew the opaque block perry-ffi reserves: raise TRANSPORT_CORE_WORDS \
     here AND in perry-ffi (the ABI digest then refuses bindings built against the old size)"
);

/// One socket or listener's transport state, owned by its payload.
///
/// Plain Rust data: no JS value, no GC pointer, no thread-local, so dropping
/// it only frees memory (the payload rule). A driver `Handle` is a plain
/// generational id; while one is installed the cell is pinned (rule 2), so the
/// sweep never drops a core that names a live descriptor. Worker teardown may
/// drop one that still names a handle: by then `Loop::drop` released it.
pub struct TransportCore {
    /// The installed handle and its stream or listener state. `None` while
    /// idle, resolving, or after [`close`] took the handle.
    entry: Option<Entry>,
    /// Writes and an `end()` waiting for a connect or for the write in flight.
    backlog: Backlog,
    /// A client connect still choosing an address; `plan.op` is the pending
    /// resolve's incarnation.
    plan: Option<Box<ConnectPlan>>,
    /// The socket's one-shot deadline (server timeouts, `setTimeout`).
    deadline: Option<Handle>,
    /// The sink this socket's completions go to.
    route: u8,
    /// Node's `ref()` state, applied to every handle this core installs.
    referenced: bool,
}

impl TransportCore {
    /// An idle core whose completions go to the link sink `route`.
    pub fn new(route: u8) -> Self {
        Self {
            entry: None,
            backlog: Backlog::default(),
            plan: None,
            deadline: None,
            route,
            referenced: true,
        }
    }

    /// The handle this core holds now (its incarnation), if any.
    pub fn handle(&self) -> Option<Handle> {
        self.entry.as_ref().map(|e| e.handle)
    }

    #[cfg(test)]
    pub(crate) fn read_operation(&self) -> Option<OpId> {
        self.read_op()
    }

    /// The armed read operation, for transport identity witnesses. Submits
    /// no work and takes no ref.
    pub fn read_op(&self) -> Option<OpId> {
        self.entry.as_ref().and_then(|e| e.read_op)
    }

    /// The sink route.
    pub fn route(&self) -> u8 {
        self.route
    }

    /// Move the socket to another link sink (an HTTP upgrade hands the
    /// connection to `net`). Outstanding operations keep their tokens; every
    /// completion is delivered to the route current at dispatch.
    pub fn set_route(&mut self, route: u8) {
        self.route = route;
    }

    /// Heap capacity owned by this core, excluding the inline core and the
    /// driver's buffers. This query submits no work and takes no ref.
    fn retained_bytes(&self) -> usize {
        use std::mem::size_of;
        let mut bytes = self.backlog.bytes.capacity()
            + self.backlog.writes.capacity() * size_of::<PendingWrite>();
        if let Some(entry) = &self.entry {
            bytes += entry.writes.capacity() * size_of::<PendingWrite>();
            if let Some(path) = &entry.path {
                bytes += path.capacity();
            }
        }
        if let Some(plan) = &self.plan {
            bytes += size_of::<ConnectPlan>() + plan.remaining.capacity() * size_of::<SocketAddr>();
        }
        bytes
    }
}

/// Deliver TLS plaintext to this cell's current route. No driver operation:
/// no token, multishot read or ref changes. The core borrow ends before the
/// sink runs, which may close or reopen the rooted owner.
///
/// # Safety
/// core/link name an OPEN payload whose owner is rooted; bytes outlives the
/// sink call. The caller re-projects before any later core use.
pub(super) unsafe fn dispatch_plaintext(
    core: *mut TransportCore,
    link: OwnerLink,
    bytes: &[u8],
    eof: bool,
) -> NetResult<()> {
    let route = {
        let (core, _) = bind(core, link, "plaintext")?;
        core.route
    };
    let mut event = if eof {
        NetCompletion::eof(link.0 as i64)
    } else {
        NetCompletion::data(link.0 as i64, bytes)
    }
    .linked();
    if !sabotage("plaintext_flag") {
        event.flags |= sink::NET_FLAG_PLAINTEXT;
    }
    sink::emit(route, event);
    Ok(())
}

/// # Safety
/// As bind.
pub(super) unsafe fn retained_bytes(core: *mut TransportCore, link: OwnerLink) -> usize {
    bind(core, link, "retained_bytes")
        .map(|(core, _)| core.retained_bytes())
        .unwrap_or(0)
}

/// An ephemeral copy of the driver's capability for checking the incarnation
/// after JS re-entry. A hostname connect may hold only its resolve OpId.
/// Neither variant mints an identity or holds a ref.
#[derive(Clone, Copy)]
pub(super) enum HandleSnapshot {
    Handle(Handle),
    Resolve(OpId),
}

pub(super) const HANDLE_SNAPSHOT_WORDS: usize = 4;
const _: () = assert!(
    std::mem::size_of::<HandleSnapshot>() <= HANDLE_SNAPSHOT_WORDS * 8
        && std::mem::align_of::<HandleSnapshot>() <= 8
);

/// The driver's generational handle as four u32s (each exact as a JS
/// number). No identity is minted.
fn handle_parts(handle: Handle) -> [u32; 4] {
    let owner = handle.owner();
    let key = handle.key();
    [
        owner as u32,
        (owner >> 32) as u32,
        key as u32,
        (key >> 32) as u32,
    ]
}

/// # Safety
/// As bind.
pub(super) unsafe fn snapshot_handle_parts(
    core: *mut TransportCore,
    link: OwnerLink,
) -> Option<[u32; 4]> {
    let (core, _) = bind(core, link, "handle").ok()?;
    core.handle().map(handle_parts)
}

/// # Safety
/// As bind; the caller roots the owner across the JS call that follows.
pub(super) unsafe fn snapshot_handle(
    core: *mut TransportCore,
    link: OwnerLink,
) -> Option<HandleSnapshot> {
    let (core, _) = bind(core, link, "handle").ok()?;
    core.handle().map(HandleSnapshot::Handle).or_else(|| {
        core.plan
            .as_ref()
            .and_then(|plan| plan.op)
            .map(HandleSnapshot::Resolve)
    })
}

/// # Safety
/// As bind. Compare against the newly projected core, never an address saved
/// from an earlier payload.
pub(super) unsafe fn handle_matches(
    core: *mut TransportCore,
    link: OwnerLink,
    snapshot: HandleSnapshot,
) -> bool {
    if sabotage("continuation_handle") {
        return true;
    }
    bind(core, link, "handle").is_ok_and(|(core, _)| match snapshot {
        HandleSnapshot::Handle(handle) => core.handle() == Some(handle),
        HandleSnapshot::Resolve(op) => core.plan.as_ref().and_then(|plan| plan.op) == Some(op),
    })
}

/// A family payload that owns a transport: the core first, so the runtime
/// reads it at offset 0 of the payload a link names.
#[repr(C)]
pub struct TransportPayload<E> {
    pub core: TransportCore,
    pub ext: E,
}

/// An accepted connection in flight to the sink (`NetCompletion::conn`).
pub(super) struct AcceptSlot {
    conn: Option<Handle>,
    peer: Option<SocketAddr>,
}

crate::perry_thread_local! {
    /// Set by agent teardown: link completions are discarded, never
    /// dereferenced (LIFECYCLE-DESIGN L8). The thread's loop is dropped next
    /// and its heap teardown finalizes the cells.
    static TEARDOWN: Cell<bool> = const { Cell::new(false) };
    /// Driver resources this thread's link routes still hold a ref for:
    /// installed handles, pending resolves, deadline timers and pipe jobs.
    static LINK_RESOURCES: Cell<usize> = const { Cell::new(0) };
}

/// Whether `t` is a link token (the transitional fork in `super::dispatch`).
#[inline]
pub(super) fn owns(t: Token) -> bool {
    t.0 & LINK_TAG == LINK_TAG
}

#[inline]
pub(super) fn link_token(route: u8, op: u64, cell: usize) -> Token {
    debug_assert!(route < 16 && op < 16 && cell & 7 == 0);
    debug_assert!((cell as u64) < np::LINK_ADDRESS_LIMIT);
    Token(LINK_TAG | (route as u64) << ROUTE_SHIFT | op << OP_SHIFT | (cell as u64) >> 3)
}

#[inline]
fn link_parts(t: Token) -> (u8, u64, usize) {
    (
        ((t.0 >> ROUTE_SHIFT) & 0xF) as u8,
        (t.0 >> OP_SHIFT) & 0xF,
        ((t.0 & CELL_MASK) << 3) as usize,
    )
}

/// Driver resources held by link routes on this thread (`live_handles`).
pub(super) fn outstanding() -> usize {
    LINK_RESOURCES.with(Cell::get)
}

pub(super) fn begin_teardown() {
    TEARDOWN.with(|t| t.set(true));
}

#[cfg(test)]
pub(super) fn reset_for_test() {
    TEARDOWN.with(|t| t.set(false));
    LINK_RESOURCES.with(|n| n.set(0));
}

// Test-only: a guard under which any driver access panics. A payload `Drop`
// runs inside a collection or at thread teardown and must never reach the
// loop (N7); the witness drops payloads under this guard.
#[cfg(test)]
crate::perry_thread_local! {
    static DRIVER_FORBIDDEN: Cell<bool> = const { Cell::new(false) };
    /// Link completions the teardown rule discarded: the witness asserts its
    /// subject ran.
    static DISCARDED: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn discarded_for_test() -> usize {
    DISCARDED.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn begin_teardown_for_test() {
    begin_teardown();
}

#[cfg(test)]
pub(super) fn assert_driver_access_allowed() {
    assert!(
        !DRIVER_FORBIDDEN.with(Cell::get),
        "the turnloop driver was reached where it is forbidden (a payload Drop)"
    );
}

#[cfg(test)]
pub(crate) struct ForbidDriver;

#[cfg(test)]
impl ForbidDriver {
    pub(crate) fn new() -> Self {
        DRIVER_FORBIDDEN.with(|f| f.set(true));
        ForbidDriver
    }
}

#[cfg(test)]
impl Drop for ForbidDriver {
    fn drop(&mut self) {
        DRIVER_FORBIDDEN.with(|f| f.set(false));
    }
}

/// Sabotage hooks for the P0 witnesses, compiled only into test binaries.
#[inline]
pub(super) fn sabotage(fault: &str) -> bool {
    #[cfg(test)]
    {
        std::env::var("PERRY_TEST_NET_SABOTAGE").as_deref() == Ok(fault)
    }
    #[cfg(not(test))]
    {
        let _ = fault;
        false
    }
}

/// Take one ref for a driver resource that will end in one terminal
/// completion on this cell.
unsafe fn hold(link: OwnerLink) {
    if sabotage("skip_ref") {
        return;
    }
    np::link_ref(link);
    LINK_RESOURCES.with(|n| n.set(n.get() + 1));
}

/// Drop the ref of a resource whose terminal completion was just dispatched,
/// or that will never produce one.
unsafe fn release(link: OwnerLink) {
    np::link_unref(link);
    LINK_RESOURCES.with(|n| n.set(n.get().saturating_sub(1)));
}

pub(super) fn bad(syscall: &'static str) -> NodeError {
    map_error(Error::new(ErrorKind::InvalidInput), syscall)
}

/// Validate a submission's `(core, link)` pair: the link's payload is OPEN on
/// this thread and `core` is its first field, and the core's route is a link
/// sink.
///
/// # Safety
/// `link` came from `owner_link` on this thread and its cell is alive (the
/// caller holds the owner).
unsafe fn bind<'a>(
    core: *mut TransportCore,
    link: OwnerLink,
    syscall: &'static str,
) -> NetResult<(&'a mut TransportCore, Name)> {
    if core.is_null() || link.0 == 0 || np::link_open_payload(link) != core.cast::<c_void>() {
        return Err(bad(syscall));
    }
    let c = &mut *core;
    if !sink::is_link_route(c.route) {
        return Err(bad(syscall));
    }
    let name = Name::Link {
        route: c.route,
        cell: link.0,
    };
    Ok((c, name))
}

/// The OPEN core behind `link`, for dispatch.
///
/// # Safety
/// The cell is alive (a ref is held for the completion being dispatched).
unsafe fn core_of<'a>(link: OwnerLink) -> Option<&'a mut TransportCore> {
    let p = np::link_open_payload(link);
    (!p.is_null()).then(|| &mut *(p as *mut TransportCore))
}

/// The OPEN core behind `link` when it still holds `handle`: the incarnation
/// that submitted the operation now completing.
unsafe fn current<'a>(link: OwnerLink, handle: Option<Handle>) -> Option<&'a mut TransportCore> {
    let c = core_of(link)?;
    let matches = c.entry.as_ref().is_some_and(|e| Some(e.handle) == handle);
    (matches || (c.entry.is_some() && sabotage("handle_check"))).then_some(c)
}

/// Change only the route; existing multishot tokens keep naming this cell.
/// # Safety
/// Same live payload/link contract as tcp_listen.
pub unsafe fn set_route(core: *mut TransportCore, link: OwnerLink, route: u8) -> NetResult<()> {
    let (c, _) = bind(core, link, "route")?;
    if !sink::is_link_route(route) {
        return Err(bad("route"));
    }
    c.set_route(route);
    #[cfg(test)]
    if sabotage("route_resubmit") {
        let old = c.entry.as_mut().and_then(|e| e.read_op.take());
        if let Some(old) = old {
            let _ = with_driver(|driver| driver.cancel(old));
            read_start(core, link)?;
        }
    }
    Ok(())
}

/// Install a handle the driver just created for this core (+1 ref).
unsafe fn install(
    driver: &mut turnloop::Loop,
    c: &mut TransportCore,
    link: OwnerLink,
    mut entry: Entry,
) {
    if !c.referenced {
        let _ = driver.set_ref(entry.handle, false);
        entry.referenced = false;
    }
    c.entry = Some(entry);
    hold(link);
}

fn idle_or(c: &TransportCore, syscall: &'static str) -> NetResult<()> {
    if c.entry.is_some() || c.plan.is_some() {
        return Err(bad(syscall));
    }
    Ok(())
}

// ── Submission ──────────────────────────────────────────────────────────────

/// Bind and listen on a TCP address; returns the actual local address.
///
/// # Safety
/// See [`bind`].
pub unsafe fn tcp_listen(
    core: *mut TransportCore,
    link: OwnerLink,
    addr: SocketAddr,
    backlog: u32,
    reuse_port: bool,
    nodelay: bool,
) -> NetResult<SocketAddr> {
    let (c, _) = bind(core, link, "listen")?;
    idle_or(c, "listen")?;
    with_driver(|driver| {
        let opts = listen_opts(backlog, reuse_port, nodelay);
        let handle = driver
            .tcp_listen(addr, &opts)
            .map_err(|e| map_error(e, "listen"))?;
        let local = driver.local_addr(handle).unwrap_or(addr);
        let mut entry = Entry::new(handle, c.route, true);
        entry.local = Some(local);
        install(driver, c, link, entry);
        Ok(local)
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Bind and listen on a Unix-domain socket path or a Windows named pipe.
/// Unlinking a stale or closed socket file is the binding's job, done
/// synchronously at close (a deferred unlink would delete a reopened
/// listener's path).
///
/// # Safety
/// See [`bind`].
pub unsafe fn pipe_listen(
    core: *mut TransportCore,
    link: OwnerLink,
    path: &Path,
    backlog: u32,
) -> NetResult<()> {
    let (c, _) = bind(core, link, "listen")?;
    idle_or(c, "listen")?;
    with_driver(|driver| {
        let opts = ListenOpts {
            reuse_port: ReusePort::No,
            backlog,
            ..ListenOpts::default()
        };
        let handle = driver
            .pipe_listen(&PipeName(path.to_path_buf()), &opts)
            .map_err(|e| map_error(e, "listen"))?;
        let mut entry = Entry::new(handle, c.route, true);
        entry.path = Some(path.to_path_buf());
        install(driver, c, link, entry);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Start the listener's multishot accept.
///
/// # Safety
/// See [`bind`].
pub unsafe fn accept_start(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let (c, name) = bind(core, link, "accept")?;
    with_driver(|driver| {
        let entry = c
            .entry
            .as_mut()
            .filter(|e| e.listener && !e.closing)
            .ok_or_else(|| not_found("accept"))?;
        if entry.accept_op.is_some() {
            return Ok(());
        }
        let op = driver
            .accept_start(entry.handle, name.token(OP_ACCEPT))
            .map_err(|e| map_error(e, "accept"))?;
        entry.accept_op = Some(op);
        census::note_submit(OP_ACCEPT);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Install the connection an accept completion carries into a fresh payload's
/// core. Valid only inside the sink call that received `completion`; a
/// connection the sink does not install is closed by the runtime after the
/// sink returns.
///
/// # Safety
/// See [`bind`]; `completion` is the pointer the sink was called with.
pub unsafe fn install_accepted(
    core: *mut TransportCore,
    link: OwnerLink,
    completion: *const NetCompletion,
) -> NetResult<()> {
    let (c, _) = bind(core, link, "accept")?;
    idle_or(c, "accept")?;
    let Some(completion) = completion.as_ref() else {
        return Err(bad("accept"));
    };
    if completion.kind != sink::NET_ACCEPT
        || completion.flags & sink::NET_FLAG_LINK == 0
        || completion.conn == 0
    {
        return Err(bad("accept"));
    }
    let slot = &mut *(completion.conn as *mut AcceptSlot);
    let Some(conn) = slot.conn.take() else {
        return Err(bad("accept"));
    };
    let peer = slot.peer;
    with_driver(|driver| {
        let mut entry = Entry::new(conn, c.route, false);
        entry.local = driver.local_addr(conn).ok();
        entry.peer = peer;
        install(driver, c, link, entry);
    })
    .ok_or_else(no_loop)
}

/// Connect a TCP client to `host:port`, resolving a hostname off the loop
/// thread first (a resolve holds its own ref until its result is dispatched).
///
/// # Safety
/// See [`bind`].
pub unsafe fn tcp_connect(
    core: *mut TransportCore,
    link: OwnerLink,
    host: &str,
    port: u16,
    nodelay: bool,
) -> NetResult<()> {
    let (c, name) = bind(core, link, "connect")?;
    idle_or(c, "connect")?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return with_driver(|driver| {
            connect_addr(driver, c, link, SocketAddr::new(ip, port), nodelay)
        })
        .unwrap_or_else(|| Err(no_loop()));
    }
    with_driver(|driver| {
        let request = turnloop::DnsRequest {
            host: host.to_string(),
            port,
        };
        let op = driver
            .resolve(request, name.token(OP_RESOLVE))
            .map_err(|e| map_error(e, "getaddrinfo"))?;
        census::note_submit(OP_RESOLVE);
        c.plan = Some(Box::new(ConnectPlan {
            subsystem: c.route,
            nodelay,
            remaining: std::collections::VecDeque::new(),
            retrying: false,
            last_error: None,
            op: Some(op),
        }));
        hold(link);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// One connect attempt: a fresh handle, installed (+1 ref).
unsafe fn connect_addr(
    driver: &mut turnloop::Loop,
    c: &mut TransportCore,
    link: OwnerLink,
    addr: SocketAddr,
    nodelay: bool,
) -> NetResult<()> {
    let name = Name::Link {
        route: c.route,
        cell: link.0,
    };
    let handle = driver
        .tcp_connect(
            addr,
            &TcpOpts {
                nodelay,
                ..TcpOpts::default()
            },
            name.token(OP_CONNECT),
        )
        .map_err(|e| map_error(e, "connect"))?;
    let mut entry = Entry::new(handle, c.route, false);
    entry.peer = Some(addr);
    entry.connecting = true;
    install(driver, c, link, entry);
    Ok(())
}

/// Connect to a Unix-domain socket or a Windows named pipe.
///
/// # Safety
/// See [`bind`].
pub unsafe fn pipe_connect(
    core: *mut TransportCore,
    link: OwnerLink,
    path: &Path,
) -> NetResult<()> {
    let (c, name) = bind(core, link, "connect")?;
    idle_or(c, "connect")?;
    with_driver(|driver| {
        let handle = driver
            .pipe_connect(&PipeName(path.to_path_buf()), name.token(OP_CONNECT))
            .map_err(|e| map_error(e, "connect"))?;
        let mut entry = Entry::new(handle, c.route, false);
        entry.path = Some(path.to_path_buf());
        entry.connecting = true;
        install(driver, c, link, entry);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Adopt an already-connected stream socket. `socket` is consumed on every
/// outcome, as on the id route.
///
/// # Safety
/// See [`bind`].
#[cfg(any(unix, windows))]
pub unsafe fn adopt_stream(
    core: *mut TransportCore,
    link: OwnerLink,
    socket: AdoptedSocket,
) -> NetResult<()> {
    let (c, name) = bind(core, link, "adopt")?;
    idle_or(c, "adopt")?;
    let (local, peer) = {
        let sock = socket2::SockRef::from(&socket);
        (
            sock.local_addr().ok().and_then(|a| a.as_socket()),
            sock.peer_addr().ok().and_then(|a| a.as_socket()),
        )
    };
    with_driver(move |driver| {
        #[cfg(unix)]
        let detached = turnloop::Detached::from_fd(socket);
        #[cfg(windows)]
        let detached = turnloop::Detached::from_socket(socket);
        let detached = detached.map_err(|e| map_error(e, "adopt"))?;
        let handle = driver
            .attach(detached, name.token(OP_READ))
            .map_err(|e| map_error(e, "adopt"))?;
        let mut entry = Entry::new(handle, c.route, false);
        entry.local = local;
        entry.peer = peer;
        install(driver, c, link, entry);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Start the multishot read.
///
/// # Safety
/// See [`bind`].
pub unsafe fn read_start(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let (c, name) = bind(core, link, "read")?;
    with_driver(|driver| {
        let entry = c.entry.as_mut().ok_or_else(|| not_found("read"))?;
        if entry.read_op.is_some() || entry.closing {
            return Ok(());
        }
        let op = driver
            .read_start(entry.handle, name.token(OP_READ))
            .map_err(|e| map_error(e, "read"))?;
        entry.read_op = Some(op);
        census::note_submit(OP_READ);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Queue `bytes`; returns the socket's total queued bytes (`writableLength`).
/// The same queue discipline as the id route (`write_queue`): one driver write
/// in flight, the rest coalesced in the backlog, writes before `Connected`
/// held for the attempt that succeeds.
///
/// # Safety
/// See [`bind`].
pub unsafe fn write(
    core: *mut TransportCore,
    link: OwnerLink,
    bytes: Vec<u8>,
    user: u64,
) -> NetResult<usize> {
    let (c, name) = bind(core, link, "write")?;
    with_driver(|driver| accept_write(driver, c, name, bytes, user))
        .unwrap_or_else(|| Err(no_loop()))
}

/// `end()`: shut the write side down behind every write accepted before it.
///
/// # Safety
/// See [`bind`].
pub unsafe fn shutdown(core: *mut TransportCore, link: OwnerLink, user: u64) -> NetResult<()> {
    let (c, name) = bind(core, link, "shutdown")?;
    with_driver(|driver| accept_shutdown(driver, c, name, link, user))
        .unwrap_or_else(|| Err(no_loop()))
}

/// Release the core's driver resources (rule 3). Takes the handle out and
/// submits its close under the cell's token, closes the deadline, cancels a
/// pending resolve and drops the backlog. Returns whether a `Closed` (a
/// `NET_CLOSED` completion for this link) will follow; when it is `false`
/// there was no handle, and a binding that owes a `close` event emits it
/// itself. The binding releases the payload afterwards.
///
/// # Safety
/// See [`bind`].
pub unsafe fn close(core: *mut TransportCore, link: OwnerLink) -> NetResult<bool> {
    let (c, name) = bind(core, link, "close")?;
    with_driver(|driver| {
        if let Some(timer) = c.deadline.take() {
            if driver.close(timer, name.token(OP_TIMER)).is_err() {
                release(link);
            }
        }
        if let Some(plan) = c.plan.take() {
            if let Some(op) = plan.op {
                // Its terminal (`Cancelled`) result carries the resolve's ref.
                driver.cancel(op);
            }
        }
        c.backlog = Backlog::default();
        let Some(entry) = c.entry.take() else {
            return Ok(false);
        };
        #[cfg(windows)]
        let mut entry = entry;
        #[cfg(windows)]
        {
            windows_pipe::cancel_eof(driver, name, &mut entry);
            if let Some(op) = entry.pipe_drain.take() {
                driver.cancel(op);
            }
        }
        if entry.closing {
            // A failed connect attempt's close is already in flight; its
            // `Closed` no longer matches the core and is delivered as this
            // socket's close.
            return Ok(true);
        }
        census::note_submit(OP_CLOSE);
        match driver.close(entry.handle, name.token(OP_CLOSE)) {
            Ok(()) => Ok(true),
            Err(e) => {
                // The handle is still live: keep it (and its ref) so no
                // completion can outlive the cell.
                c.entry = Some(entry);
                Err(map_error(e, "close"))
            }
        }
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Node's `ref()`/`unref()`: remembered on the core, so a handle installed
/// later inherits it.
///
/// # Safety
/// See [`bind`].
pub unsafe fn set_ref(
    core: *mut TransportCore,
    link: OwnerLink,
    referenced: bool,
) -> NetResult<()> {
    let (c, _) = bind(core, link, "")?;
    c.referenced = referenced;
    let Some(entry) = c.entry.as_mut() else {
        return Ok(());
    };
    if entry.referenced == referenced {
        return Ok(());
    }
    entry.referenced = referenced;
    let handle = entry.handle;
    with_driver(|driver| {
        driver
            .set_ref(handle, referenced)
            .map_err(|e| map_error(e, ""))
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Bytes accepted and not yet reported written.
///
/// # Safety
/// See [`bind`].
pub unsafe fn queued_bytes(core: *mut TransportCore, link: OwnerLink) -> usize {
    bind(core, link, "").map_or(0, |(c, _)| total_queued(c))
}

/// The local endpoint (`server.address()`, `socket.localAddress`).
///
/// # Safety
/// See [`bind`].
pub unsafe fn local_addr(core: *mut TransportCore, link: OwnerLink) -> Option<SocketAddr> {
    let (c, _) = bind(core, link, "").ok()?;
    c.entry.as_ref().and_then(|e| e.local)
}

/// The peer endpoint (`socket.remoteAddress`).
///
/// # Safety
/// See [`bind`].
pub unsafe fn peer_addr(core: *mut TransportCore, link: OwnerLink) -> Option<SocketAddr> {
    let (c, _) = bind(core, link, "").ok()?;
    c.entry.as_ref().and_then(|e| e.peer)
}

/// Arm, or move, the socket's one-shot deadline `delay_ms` from now. Never
/// keeps the loop alive on its own. Its expiry is delivered as `NET_TIMER`.
///
/// # Safety
/// See [`bind`].
pub unsafe fn deadline_arm(
    core: *mut TransportCore,
    link: OwnerLink,
    delay_ms: u64,
) -> NetResult<()> {
    let (c, name) = bind(core, link, "timer")?;
    with_driver(|driver| {
        let at = driver.now() + std::time::Duration::from_millis(delay_ms);
        if let Some(timer) = c.deadline {
            if driver.timer_reset(timer, at) {
                census::note_timer_reset();
                return Ok(());
            }
            // Fired (its expiry not yet dispatched) or closing: replace it.
            // Its `Closed` releases its ref; a pending expiry no longer
            // matches `deadline` and is dropped.
            c.deadline = None;
            if driver.close(timer, name.token(OP_TIMER)).is_err() {
                release(link);
            }
        }
        let timer = driver
            .timer(at, None, name.token(OP_TIMER))
            .map_err(|e| map_error(e, "timer"))?;
        census::note_timer_create();
        let _ = driver.set_ref(timer, false);
        c.deadline = Some(timer);
        hold(link);
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Disarm the deadline but keep its handle (see the id route's `timer_park`).
///
/// # Safety
/// See [`bind`].
pub unsafe fn deadline_park(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let (c, name) = bind(core, link, "timer")?;
    let Some(timer) = c.deadline else {
        return Ok(());
    };
    with_driver(|driver| {
        if let Some(at) = driver.now().checked_add(PARK_AHEAD) {
            if driver.timer_reset(timer, at) {
                census::note_timer_park();
                return Ok(());
            }
        }
        c.deadline = None;
        census::note_submit(OP_TIMER);
        if driver.close(timer, name.token(OP_TIMER)).is_err() {
            release(link);
        }
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Cancel the deadline. Idempotent.
///
/// # Safety
/// See [`bind`].
pub unsafe fn deadline_cancel(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let (c, name) = bind(core, link, "timer")?;
    let Some(timer) = c.deadline.take() else {
        return Ok(());
    };
    with_driver(|driver| {
        census::note_submit(OP_TIMER);
        if driver.close(timer, name.token(OP_TIMER)).is_err() {
            release(link);
        }
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

// ── Write queue on a core (the id route's `write_queue`, per socket) ────────

fn total_queued(c: &TransportCore) -> usize {
    c.entry.as_ref().map_or(0, |e| e.queued) + c.backlog.bytes.len()
}

/// Caller writes a backlog flush could not hand to the driver.
struct Failure {
    error: NodeError,
    users: Vec<u64>,
}

fn accept_write(
    driver: &mut turnloop::Loop,
    c: &mut TransportCore,
    name: Name,
    bytes: Vec<u8>,
    user: u64,
) -> NetResult<usize> {
    let retrying = c.plan.as_ref().is_some_and(|p| p.retrying);
    match c.entry.as_mut() {
        Some(entry) if !retrying => {
            if entry.listener || entry.closing {
                return Err(bad("write"));
            }
            let waiting = !c.backlog.is_idle();
            if !entry.connecting && !waiting && entry.inflight < MAX_INFLIGHT_WRITES {
                let len = bytes.len();
                write_queue::submit_bytes(driver, name, entry, bytes)
                    .map_err(|e| map_error(e, "write"))?;
                entry.writes.push_back(PendingWrite {
                    user,
                    len,
                    last: true,
                });
            } else {
                if c.backlog.shutdown.is_some() {
                    return Err(bad("write"));
                }
                c.backlog.push(bytes, user);
            }
        }
        // Resolving, or between two attempts of a plan.
        _ if c.plan.is_some() => {
            if c.backlog.shutdown.is_some() {
                return Err(bad("write"));
            }
            c.backlog.push(bytes, user);
        }
        _ => return Err(not_found("write")),
    }
    Ok(total_queued(c))
}

unsafe fn accept_shutdown(
    driver: &mut turnloop::Loop,
    c: &mut TransportCore,
    name: Name,
    link: OwnerLink,
    user: u64,
) -> NetResult<()> {
    let retrying = c.plan.as_ref().is_some_and(|p| p.retrying);
    let deferred = match c.entry.as_ref() {
        Some(_) if retrying => true,
        Some(entry) if entry.listener || entry.closing => return Err(bad("shutdown")),
        Some(entry) => entry.connecting,
        None if c.plan.is_some() => true,
        None => return Err(not_found("shutdown")),
    };
    if c.backlog.shutdown.is_some() {
        return Err(bad("shutdown"));
    }
    c.backlog.shutdown = Some(user);
    if deferred {
        return Ok(());
    }
    match flush(driver, c, name, link, true) {
        Some(failure) => Err(failure.error),
        None => Ok(()),
    }
}

/// Hand the backlog (and a deferred shutdown) to the driver if the socket can
/// take it now; `force` ignores the in-flight cap.
unsafe fn flush(
    driver: &mut turnloop::Loop,
    c: &mut TransportCore,
    name: Name,
    link: OwnerLink,
    force: bool,
) -> Option<Failure> {
    let entry = c.entry.as_mut()?;
    if entry.connecting || entry.closing {
        return None;
    }
    let backlog = &mut c.backlog;
    let mut failure = None;
    if !backlog.writes.is_empty() {
        if !force && entry.inflight >= MAX_INFLIGHT_WRITES {
            return None;
        }
        let bytes = std::mem::take(&mut backlog.bytes);
        let mut writes = std::mem::take(&mut backlog.writes);
        match write_queue::submit_bytes(driver, name, entry, bytes) {
            Ok(()) => {
                if let Some(tail) = writes.last_mut() {
                    tail.last = true;
                }
                entry.writes.extend(writes);
            }
            Err(e) => {
                let mut users: Vec<u64> = writes.iter().map(|w| w.user).collect();
                users.extend(backlog.shutdown.take());
                failure = Some(Failure {
                    error: map_error(e, "write"),
                    users,
                });
            }
        }
    }
    #[cfg(windows)]
    if failure.is_none()
        && backlog.shutdown.is_some()
        && entry.inflight > 0
        && windows_pipe::is_pipe(driver, entry)
    {
        return None;
    }
    if failure.is_none() {
        if let Some(user) = backlog.shutdown.take() {
            match write_queue::submit_shutdown(driver, name, entry, user) {
                // A Windows pipe drain is a job, not a handle operation: it
                // holds its own ref until its terminal result.
                Ok(true) => hold(link),
                Ok(false) => {}
                Err(e) => {
                    failure = Some(Failure {
                        error: map_error(e, "shutdown"),
                        users: vec![user],
                    })
                }
            }
        }
    }
    failure
}

/// [`flush`] from a completion; a refused flush is reported like a failed
/// write.
unsafe fn flush_after(link: OwnerLink, force: bool) {
    let Some(c) = core_of(link) else {
        return;
    };
    let route = c.route;
    let name = Name::Link {
        route,
        cell: link.0,
    };
    let failure = with_driver(|driver| flush(driver, c, name, link, force)).flatten();
    if let Some(failure) = failure {
        let queued = core_of(link).map_or(0, |c| total_queued(c));
        report_error(route, link, &failure.users, queued, failure.error, false);
    }
}

/// Retire the caller writes one driver write (or shutdown) covered.
fn retire(c: &mut TransportCore, op_class: u64) -> Vec<(u64, usize, usize)> {
    let waiting = c.backlog.bytes.len();
    let Some(entry) = c.entry.as_mut() else {
        return Vec::new();
    };
    if op_class == OP_WRITE {
        entry.inflight = entry.inflight.saturating_sub(1);
    }
    let mut retired = Vec::new();
    while let Some(w) = entry.writes.pop_front() {
        entry.queued = entry.queued.saturating_sub(w.len);
        retired.push((w.user, w.len, entry.queued + waiting));
        if w.last {
            break;
        }
    }
    retired
}

fn report_error(
    route: u8,
    link: OwnerLink,
    users: &[u64],
    queued: usize,
    error: NodeError,
    terminal: bool,
) {
    let id = link.0 as i64;
    let mut reported = false;
    for &user in users.iter().filter(|&&user| user != 0) {
        sink::emit(
            route,
            NetCompletion::error(id, user, queued, error, terminal).linked(),
        );
        reported = true;
    }
    if !reported {
        sink::emit(
            route,
            NetCompletion::error(id, 0, queued, error, terminal).linked(),
        );
    }
}

// ── Dispatch ────────────────────────────────────────────────────────────────

/// Route one link-token completion. Called by `super::dispatch` after the
/// turn, outside every loop borrow, so a sink may run JS and submit.
pub(super) fn dispatch(completion: Completion) {
    let (route, op, cell) = link_parts(completion.token);
    census::note_completion(op);
    if cell == 0 {
        // LINK_NULL: a close nobody waits on.
        return;
    }
    if TEARDOWN.with(Cell::get) && !sabotage("teardown_dispatch") {
        // Never dereference a token once the thread is going away.
        #[cfg(test)]
        DISCARDED.with(|n| n.set(n.get() + 1));
        return;
    }
    let link = OwnerLink(cell);
    // SAFETY: a ref held for this completion's resource keeps the cell alive
    // (rule 2); a finalized cell answers None and is never touched again.
    if unsafe { np::link_event_owner(link) }.is_none() {
        census::note_no_entry();
        return;
    }
    let Completion {
        result,
        terminal,
        op: op_id,
        handle,
        ..
    } = completion;
    // SAFETY: as above, for every helper below.
    unsafe {
        match op {
            OP_TIMER => dispatch_timer(link, handle, result),
            OP_RESOLVE => dispatch_resolve(link, op_id, terminal, result),
            #[cfg(windows)]
            OP_PIPE_EOF => dispatch_pipe_eof(link, handle, result),
            #[cfg(windows)]
            OP_PIPE_DRAIN => dispatch_pipe_drain(link, op_id, terminal, result),
            _ => dispatch_stream(route, op, link, handle, terminal, result),
        }
    }
}

unsafe fn dispatch_timer(link: OwnerLink, handle: Option<Handle>, result: OpResult) {
    census::note_timer_result(&result);
    match result {
        OpResult::Timer => {
            let Some(c) = core_of(link) else {
                return;
            };
            let (Some(timer), true) = (handle, c.deadline == handle) else {
                // Replaced or cancelled before this expiry was dispatched;
                // whoever did so closed the handle.
                return;
            };
            c.deadline = None;
            let route = c.route;
            let name = Name::Link {
                route,
                cell: link.0,
            };
            // A fired one-shot keeps its handle until closed; the `Closed`
            // releases the deadline's ref.
            let closed = with_driver(|driver| driver.close(timer, name.token(OP_TIMER)).is_ok());
            if closed != Some(true) {
                release(link);
            }
            sink::emit(route, NetCompletion::timer(link.0 as i64).linked());
        }
        OpResult::Closed => release(link),
        _ => {}
    }
}

unsafe fn dispatch_resolve(link: OwnerLink, op_id: Option<OpId>, terminal: bool, result: OpResult) {
    let current = core_of(link).filter(|c| {
        c.plan
            .as_ref()
            .is_some_and(|p| p.op.is_some() && p.op == op_id)
            || (c.plan.is_some() && sabotage("plan_check"))
    });
    match result {
        OpResult::Resolved(addresses) => {
            if let Some(c) = current {
                if let Some(plan) = c.plan.as_mut() {
                    plan.op = None;
                    plan.remaining = addresses.into_iter().collect();
                }
                attempt_next_address(link);
            }
        }
        OpResult::Err(err) => {
            if let Some(c) = current {
                c.plan = None;
                c.backlog = Backlog::default();
                let route = c.route;
                let mut mapped = map_error(err, "getaddrinfo");
                // libuv (and so node) reports any failed lookup as ENOTFOUND.
                mapped.code = "ENOTFOUND";
                sink::emit(
                    route,
                    NetCompletion::error(link.0 as i64, 0, 0, mapped, true).linked(),
                );
            }
        }
        OpResult::Cancelled | OpResult::Stopped => {
            if let Some(c) = current {
                c.plan = None;
                c.backlog = Backlog::default();
            }
        }
        _ => {}
    }
    if terminal {
        release(link);
    }
}

/// Inspect a resolve incarnation in acceptance tests.
///
/// # Safety
/// `cell` is a live transport payload cell on this thread.
#[cfg(any(test, feature = "native-payload-test-census"))]
pub unsafe fn pending_resolve_for_test(cell: usize) -> Option<OpId> {
    core_of(OwnerLink(cell)).and_then(|core| core.plan.as_ref().and_then(|plan| plan.op))
}

/// Replay a resolve completion in acceptance tests. Nonterminal: the real
/// driver completion still owns its ref.
///
/// # Safety
/// As [`pending_resolve_for_test`].
#[cfg(any(test, feature = "native-payload-test-census"))]
pub unsafe fn replay_resolve_for_test(cell: usize, op: OpId, addresses: Vec<SocketAddr>) {
    dispatch_resolve(
        OwnerLink(cell),
        Some(op),
        false,
        OpResult::Resolved(addresses),
    );
}

/// Start the plan's next address, or report its final failure.
unsafe fn attempt_next_address(link: OwnerLink) {
    loop {
        let Some(c) = core_of(link) else {
            return;
        };
        let route = c.route;
        let Some(plan) = c.plan.as_mut() else {
            return;
        };
        let (nodelay, next, last_error) =
            (plan.nodelay, plan.remaining.pop_front(), plan.last_error);
        let Some(addr) = next else {
            c.plan = None;
            c.backlog = Backlog::default();
            let err = last_error.unwrap_or(NodeError {
                code: "ECONNREFUSED",
                errno: 0,
                syscall: "connect",
            });
            sink::emit(
                route,
                NetCompletion::error(link.0 as i64, 0, 0, err, true).linked(),
            );
            return;
        };
        let submitted = with_driver(|driver| connect_addr(driver, c, link, addr, nodelay))
            .unwrap_or_else(|| Err(no_loop()));
        match submitted {
            Ok(()) => return,
            Err(err) => {
                // Submission itself failed: record it and try the next one.
                if let Some(plan) = core_of(link).and_then(|c| c.plan.as_mut()) {
                    plan.last_error = Some(err);
                }
            }
        }
    }
}

/// A connect attempt failed: true when absorbed into a retry. The attempt's
/// handle is closed here and its `Closed` starts the next address.
unsafe fn connect_failed(c: &mut TransportCore, link: OwnerLink, err: NodeError) -> bool {
    let Some(plan) = c.plan.as_mut() else {
        return false;
    };
    plan.last_error = Some(err);
    if plan.remaining.is_empty() {
        c.plan = None;
        return false;
    }
    plan.retrying = true;
    let name = Name::Link {
        route: c.route,
        cell: link.0,
    };
    if let Some(entry) = c.entry.as_mut() {
        if !entry.closing {
            entry.closing = true;
            census::note_submit(OP_CLOSE);
            let handle = entry.handle;
            let _ = with_driver(|driver| driver.close(handle, name.token(OP_CLOSE)));
        }
    }
    true
}

fn clear_op(c: &mut TransportCore, op_class: u64) {
    if let Some(entry) = c.entry.as_mut() {
        match op_class {
            OP_ACCEPT => entry.accept_op = None,
            OP_READ => entry.read_op = None,
            _ => {}
        }
    }
}

fn close_null(conn: Handle) {
    let _ = with_driver(|driver| driver.close(conn, LINK_NULL));
}

unsafe fn dispatch_stream(
    route: u8,
    op: u64,
    link: OwnerLink,
    handle: Option<Handle>,
    terminal: bool,
    result: OpResult,
) {
    let id = link.0 as i64;
    match result {
        OpResult::Closed => {
            let mut route = route;
            let mut retry = false;
            let mut stale = false;
            if let Some(c) = current(link, handle) {
                route = c.route;
                c.entry = None;
                match c.plan.as_mut() {
                    Some(plan) if plan.retrying => {
                        plan.retrying = false;
                        retry = true;
                    }
                    _ => {
                        c.plan = None;
                        c.backlog = Backlog::default();
                    }
                }
            } else if let Some(c) = core_of(link) {
                route = c.route;
                stale = true;
            }
            if retry {
                // A failed attempt's handle, not the socket the caller sees.
                attempt_next_address(link);
            } else {
                let mut event = NetCompletion::closed(id).linked();
                // Callback context belongs to the closing incarnation even if
                // the owner already has a new payload. Lend the handle's
                // parts for the sink call only.
                let parts = handle.map(handle_parts);
                if let Some(parts) = parts.as_ref() {
                    event.flags |= sink::NET_FLAG_HANDLE_PARTS;
                    event.data = parts.as_ptr().cast();
                    event.len = std::mem::size_of_val(parts);
                }
                if stale && !sabotage("stale_closed_flag") {
                    event.flags |= sink::NET_FLAG_STALE;
                }
                sink::emit(route, event);
            }
            // The handle's ref, taken at install. Last: the sink ran with the
            // cell pinned.
            release(link);
        }
        OpResult::Accepted { conn, peer } => accepted(link, handle, conn, Some(peer)),
        OpResult::PipeAccepted { conn } => accepted(link, handle, conn, None),
        OpResult::Connected => {
            let Some(c) = current(link, handle) else {
                return;
            };
            let route = c.route;
            let local =
                handle.and_then(|h| with_driver(|driver| driver.local_addr(h).ok()).flatten());
            if let Some(entry) = c.entry.as_mut() {
                entry.local = local;
                entry.connecting = false;
            }
            c.plan = None;
            sink::emit(route, NetCompletion::connect(id).linked());
            // Writes (and an `end()`) issued while connecting go out now.
            flush_after(link, true);
        }
        OpResult::Read { n, lease } => {
            let Some(c) = current(link, handle) else {
                return;
            };
            let route = c.route;
            #[cfg(windows)]
            {
                let name = Name::Link {
                    route,
                    cell: link.0,
                };
                if let Some(entry) = c.entry.as_mut() {
                    if entry.pipe_eof_timer.is_some() {
                        let created =
                            with_driver(|driver| windows_pipe::arm_eof(driver, name, entry));
                        if matches!(created, Some(Ok(true))) {
                            hold(link);
                        }
                    }
                }
            }
            let bytes = lease.as_ref().map(|l| l.as_slice()).unwrap_or(&[]);
            debug_assert!(bytes.len() == n || lease.is_none());
            sink::emit(route, NetCompletion::data(id, bytes).linked());
            drop(lease);
        }
        OpResult::Eof => {
            let Some(c) = current(link, handle) else {
                return;
            };
            let route = c.route;
            if let Some(entry) = c.entry.as_mut() {
                #[cfg(windows)]
                {
                    let name = Name::Link {
                        route,
                        cell: link.0,
                    };
                    let _ = with_driver(|driver| windows_pipe::cancel_eof(driver, name, entry));
                }
                entry.read_op = None;
            }
            sink::emit(route, NetCompletion::eof(id).linked());
        }
        OpResult::Wrote(n) => {
            let Some(c) = current(link, handle) else {
                return;
            };
            let route = c.route;
            let retired = retire(c, OP_WRITE);
            debug_assert!(
                retired.is_empty() || retired.iter().map(|r| r.1).sum::<usize>() == n,
                "a write completes its whole buffer"
            );
            flush_after(link, false);
            for (user, len, queued) in retired {
                sink::emit(route, NetCompletion::wrote(id, user, len, queued).linked());
            }
        }
        OpResult::Shutdown => {
            let Some(c) = current(link, handle) else {
                return;
            };
            let route = c.route;
            let user = retire(c, OP_SHUTDOWN).first().map_or(0, |r| r.0);
            sink::emit(route, NetCompletion::shutdown(id, user).linked());
        }
        OpResult::Err(err) => {
            let Some(c) = current(link, handle) else {
                return;
            };
            let route = c.route;
            let users: Vec<u64> = if op == OP_WRITE || op == OP_SHUTDOWN {
                retire(c, op).into_iter().map(|r| r.0).collect()
            } else {
                Vec::new()
            };
            let queued = total_queued(c);
            if terminal {
                clear_op(c, op);
            }
            let mapped = map_error(err, syscall_for(op));
            if op == OP_CONNECT && connect_failed(c, link, mapped) {
                return;
            }
            report_error(route, link, &users, queued, mapped, terminal);
        }
        OpResult::Cancelled | OpResult::Stopped => {
            if let Some(c) = current(link, handle) {
                clear_op(c, op);
                if op == OP_WRITE || op == OP_SHUTDOWN {
                    retire(c, op);
                }
            }
        }
        _ => {}
    }
}

unsafe fn accepted(
    link: OwnerLink,
    handle: Option<Handle>,
    conn: Handle,
    peer: Option<SocketAddr>,
) {
    let Some(c) = current(link, handle) else {
        // A stale listener's connection: nobody can own it.
        close_null(conn);
        return;
    };
    let route = c.route;
    let mut slot = AcceptSlot {
        conn: Some(conn),
        peer,
    };
    let completion =
        NetCompletion::accept(link.0 as i64, &mut slot as *mut AcceptSlot as i64, peer);
    sink::emit(route, completion.linked());
    if let Some(conn) = slot.conn.take() {
        // The sink refused (or could not allocate) the connection.
        close_null(conn);
    }
}

#[cfg(windows)]
unsafe fn dispatch_pipe_eof(link: OwnerLink, handle: Option<Handle>, result: OpResult) {
    match result {
        OpResult::Timer => {
            let Some(c) = core_of(link) else {
                return;
            };
            let live = c
                .entry
                .as_ref()
                .is_some_and(|e| !e.closing && e.pipe_eof_timer == handle);
            if !live {
                return;
            }
            let route = c.route;
            let name = Name::Link {
                route,
                cell: link.0,
            };
            sink::emit(route, NetCompletion::eof(link.0 as i64).linked());
            close_stream(link, name);
        }
        // The EOF timer handle's own `Closed`: its ref.
        OpResult::Closed => release(link),
        _ => {}
    }
}

#[cfg(windows)]
unsafe fn dispatch_pipe_drain(
    link: OwnerLink,
    op_id: Option<OpId>,
    terminal: bool,
    result: OpResult,
) {
    let live = core_of(link).and_then(|c| {
        let route = c.route;
        c.entry
            .as_ref()
            .is_some_and(|e| !e.closing && e.pipe_drain == op_id)
            .then_some(route)
    });
    if let Some(route) = live {
        let name = Name::Link {
            route,
            cell: link.0,
        };
        match result {
            OpResult::Blocking(_) => {
                let Some(c) = core_of(link) else {
                    return;
                };
                let user = retire(c, OP_SHUTDOWN).first().map_or(0, |r| r.0);
                sink::emit(route, NetCompletion::shutdown(link.0 as i64, user).linked());
                let armed = core_of(link)
                    .and_then(|c| c.entry.as_mut())
                    .and_then(|entry| {
                        with_driver(|driver| windows_pipe::arm_eof(driver, name, entry))
                    });
                match armed {
                    Some(Ok(true)) => hold(link),
                    Some(Ok(false)) => {}
                    _ => {
                        sink::emit(route, NetCompletion::eof(link.0 as i64).linked());
                        close_stream(link, name);
                    }
                }
            }
            OpResult::Err(error) => {
                let users: Vec<u64> = core_of(link)
                    .map(|c| retire(c, OP_SHUTDOWN).into_iter().map(|r| r.0).collect())
                    .unwrap_or_default();
                report_error(route, link, &users, 0, map_error(error, "shutdown"), true);
                close_stream(link, name);
            }
            _ => {}
        }
    }
    if terminal {
        release(link);
    }
}

/// The runtime's own close of a stream (a Windows pipe's EOF): the handle
/// stays in the core, closing, and its `Closed` is delivered as the close.
#[cfg(windows)]
unsafe fn close_stream(link: OwnerLink, name: Name) {
    let Some(entry) = core_of(link).and_then(|c| c.entry.as_mut()) else {
        return;
    };
    if entry.closing {
        return;
    }
    entry.closing = true;
    let _ = with_driver(|driver| windows_pipe::cancel_eof(driver, name, entry));
    if let Some(op) = entry.pipe_drain.take() {
        let _ = with_driver(|driver| driver.cancel(op));
    }
    census::note_submit(OP_CLOSE);
    let handle = entry.handle;
    let _ = with_driver(|driver| driver.close(handle, name.token(OP_CLOSE)));
}

// ── The core as an ABI value ────────────────────────────────────────────────

/// Construct a core in place (perry-ffi's `TransportCore::new`).
///
/// # Safety
/// `core` is writable for `TRANSPORT_CORE_WORDS` words, 8-aligned.
pub(super) unsafe fn core_init(core: *mut TransportCore, route: u8) {
    // GC_STORE_AUDIT(POINTER_FREE): a binding's payload memory on the Rust
    // heap (or its stack); a TransportCore holds no GC reference.
    std::ptr::write(core, TransportCore::new(route));
}

/// Drop a core in place (perry-ffi's `Drop for TransportCore`).
///
/// # Safety
/// `core` was initialized by [`core_init`] and is not used afterwards.
pub(super) unsafe fn core_drop(core: *mut TransportCore) {
    std::ptr::drop_in_place(core);
}

/// Frees memory only: a core is dropped inside a collection, at release, or
/// at thread teardown, so it never reaches the loop or any other thread-local
/// (rule 2 guarantees a swept core holds no live handle). The body exists for
/// the N7 sabotage alone and is empty in a production build.
impl Drop for TransportCore {
    fn drop(&mut self) {
        if sabotage("drop_closes") {
            // The defect the rule forbids: a Drop that releases its handle.
            let handle = self.handle();
            let _ = with_driver(|driver| {
                if let Some(handle) = handle {
                    let _ = driver.close(handle, LINK_NULL);
                }
            });
        }
    }
}
