//! Owner-link adapter for the WASI socket driver. Incarnations are fresh
//! driver ids, never OS descriptors (which can be reused after close).
use super::*;
use crate::native_payload::{self as np, OwnerLink};
use std::cell::Cell;
use std::ffi::c_void;

pub const TRANSPORT_CORE_WORDS: usize = 39;
#[repr(C, align(8))]
pub struct TransportCore {
    id: Option<i64>,
    timer: Option<i64>,
    route: u8,
    referenced: bool,
}
const _: () = assert!(std::mem::size_of::<TransportCore>() <= TRANSPORT_CORE_WORDS * 8);
/// A family payload that owns a transport: the core first, so the runtime
/// reads it at offset 0 of the payload a link names.
#[repr(C)]
pub struct TransportPayload<E> {
    pub core: TransportCore,
    pub ext: E,
}

pub const HANDLE_SNAPSHOT_WORDS: usize = 4;
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HandleSnapshot(i64);
const _: () = assert!(std::mem::size_of::<HandleSnapshot>() <= HANDLE_SNAPSHOT_WORDS * 8);

crate::perry_thread_local! { static NEXT_ID: Cell<i64> = const { Cell::new(1 << 48) }; }
fn next_id() -> i64 {
    NEXT_ID.with(|id| {
        let next = id.get();
        id.set(next.checked_add(1).expect("WASI socket ids exhausted"));
        next
    })
}

/// Pins the owner's stable cell until the driver resource or queued event is
/// retired. It does not hold a pointer into Perry's moving heap.
pub(super) struct Pin(pub OwnerLink);
impl Pin {
    unsafe fn new(link: OwnerLink) -> Self {
        np::link_ref(link);
        Self(link)
    }
}
impl Clone for Pin {
    fn clone(&self) -> Self {
        unsafe { Self::new(self.0) }
    }
}
impl Drop for Pin {
    fn drop(&mut self) {
        unsafe { np::link_unref(self.0) };
    }
}
pub(super) fn bad(syscall: &'static str) -> NodeError {
    errors::invalid_input(syscall)
}
unsafe fn bind<'a>(
    core: *mut TransportCore,
    link: OwnerLink,
    syscall: &'static str,
) -> NetResult<&'a mut TransportCore> {
    if core.is_null() || link.0 == 0 || np::link_open_payload(link) != core.cast::<c_void>() {
        return Err(bad(syscall));
    }
    let core = &mut *core;
    if !sink::is_link_route(core.route) {
        return Err(bad(syscall));
    }
    Ok(core)
}
unsafe fn current<'a>(link: OwnerLink) -> Option<&'a mut TransportCore> {
    np::link_open_payload(link).cast::<TransportCore>().as_mut()
}
fn idle(core: &TransportCore, syscall: &'static str) -> NetResult<()> {
    if core.id.is_some() {
        Err(bad(syscall))
    } else {
        Ok(())
    }
}
unsafe fn attach(core: &mut TransportCore, link: OwnerLink, id: i64) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let handle = state.handles.get_mut(&id).expect("installed WASI handle");
        handle.owner = Some(Pin::new(link));
        handle.referenced = core.referenced;
    });
    core.id = Some(id);
}
pub(super) unsafe fn core_init(core: *mut TransportCore, route: u8) {
    // GC_STORE_AUDIT(POINTER_FREE): TransportCore initializer writes id=None,timer=None,route u8,referenced bool; no owner link or pointer is installed.
    core.write(TransportCore {
        id: None,
        timer: None,
        route,
        referenced: true,
    });
}
pub(super) unsafe fn core_drop(core: *mut TransportCore) {
    std::ptr::drop_in_place(core);
}

pub unsafe fn tcp_listen(
    core: *mut TransportCore,
    link: OwnerLink,
    addr: SocketAddr,
    backlog: u32,
    reuse_port: bool,
    nodelay: bool,
) -> NetResult<SocketAddr> {
    let core = bind(core, link, "listen")?;
    idle(core, "listen")?;
    let id = next_id();
    let local = super::tcp_listen(id, core.route, addr, backlog, reuse_port, nodelay)?;
    attach(core, link, id);
    Ok(local)
}
pub unsafe fn tcp_connect(
    core: *mut TransportCore,
    link: OwnerLink,
    host: &str,
    port: u16,
    nodelay: bool,
) -> NetResult<()> {
    let core = bind(core, link, "connect")?;
    idle(core, "connect")?;
    let id = next_id();
    super::tcp_connect_host(id, core.route, host, port, nodelay)?;
    attach(core, link, id);
    Ok(())
}
pub unsafe fn pipe_listen(
    core: *mut TransportCore,
    link: OwnerLink,
    _: &Path,
    _: u32,
) -> NetResult<()> {
    bind(core, link, "listen")?;
    Err(errors::unsupported("listen"))
}
pub unsafe fn pipe_connect(core: *mut TransportCore, link: OwnerLink, _: &Path) -> NetResult<()> {
    bind(core, link, "connect")?;
    Err(errors::unsupported("connect"))
}
pub unsafe fn adopt_stream(core: *mut TransportCore, link: OwnerLink, fd: i32) -> NetResult<()> {
    // Ownership transfers on every outcome, including invalid owner/core pairs.
    let handle = Handle::new(fd, 0);
    let core = bind(core, link, "adopt")?;
    idle(core, "adopt")?;
    let id = next_id();
    std::mem::forget(handle);
    super::adopt_fd(id, core.route, fd)?;
    attach(core, link, id);
    Ok(())
}
pub unsafe fn accept_start(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let id = bind(core, link, "accept")?
        .id
        .ok_or_else(|| bad("accept"))?;
    super::accept_start(id)
}
pub unsafe fn read_start(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let id = bind(core, link, "read")?.id.ok_or_else(|| bad("read"))?;
    super::read_start(id)
}
pub unsafe fn write(
    core: *mut TransportCore,
    link: OwnerLink,
    bytes: Vec<u8>,
    user: u64,
) -> NetResult<usize> {
    let id = bind(core, link, "write")?.id.ok_or_else(|| bad("write"))?;
    super::write(id, bytes, user)
}
pub unsafe fn shutdown(core: *mut TransportCore, link: OwnerLink, user: u64) -> NetResult<()> {
    let id = bind(core, link, "shutdown")?
        .id
        .ok_or_else(|| bad("shutdown"))?;
    super::shutdown(id, user)
}
pub unsafe fn close(core: *mut TransportCore, link: OwnerLink) -> NetResult<bool> {
    let core = bind(core, link, "close")?;
    if let Some(timer) = core.timer.take() {
        super::timer_cancel(timer)?;
    }
    let Some(id) = core.id.take() else {
        return Ok(false);
    };
    super::close(id)?;
    Ok(true)
}
pub unsafe fn set_ref(
    core: *mut TransportCore,
    link: OwnerLink,
    referenced: bool,
) -> NetResult<()> {
    let core = bind(core, link, "ref")?;
    core.referenced = referenced;
    if let Some(id) = core.id {
        super::set_ref(id, referenced)?;
    }
    Ok(())
}
pub unsafe fn set_route(core: *mut TransportCore, link: OwnerLink, route: u8) -> NetResult<()> {
    let core = bind(core, link, "route")?;
    if !sink::is_link_route(route) {
        return Err(bad("route"));
    }
    if let Some(id) = core.id {
        super::transfer(id, route)?;
    }
    core.route = route;
    Ok(())
}
pub unsafe fn queued_bytes(core: *mut TransportCore, link: OwnerLink) -> usize {
    bind(core, link, "write")
        .ok()
        .and_then(|c| c.id)
        .map_or(0, super::queued_bytes)
}
pub unsafe fn retained_bytes(core: *mut TransportCore, link: OwnerLink) -> usize {
    let Some(id) = bind(core, link, "memory").ok().and_then(|c| c.id) else {
        return 0;
    };
    STATE.with(|state| {
        state.borrow().handles.get(&id).map_or(0, |h| {
            h.writes.capacity() * std::mem::size_of::<Write>()
                + h.writes.iter().map(|w| w.bytes.capacity()).sum::<usize>()
        })
    })
}
pub unsafe fn local_addr(core: *mut TransportCore, link: OwnerLink) -> Option<SocketAddr> {
    bind(core, link, "address")
        .ok()?
        .id
        .and_then(super::local_addr)
}
pub unsafe fn peer_addr(core: *mut TransportCore, link: OwnerLink) -> Option<SocketAddr> {
    bind(core, link, "address")
        .ok()?
        .id
        .and_then(super::peer_addr)
}
pub unsafe fn snapshot_handle(core: *mut TransportCore, link: OwnerLink) -> Option<HandleSnapshot> {
    bind(core, link, "handle").ok()?.id.map(HandleSnapshot)
}
fn parts(id: i64) -> [u32; 4] {
    [0, 0, id as u32, (id as u64 >> 32) as u32]
}
pub unsafe fn snapshot_handle_parts(core: *mut TransportCore, link: OwnerLink) -> Option<[u32; 4]> {
    snapshot_handle(core, link).map(|s| parts(s.0))
}
pub unsafe fn handle_matches(
    core: *mut TransportCore,
    link: OwnerLink,
    snapshot: HandleSnapshot,
) -> bool {
    snapshot_handle(core, link) == Some(snapshot)
}
pub unsafe fn deadline_arm(
    core: *mut TransportCore,
    link: OwnerLink,
    delay_ms: u64,
) -> NetResult<()> {
    let core = bind(core, link, "timer")?;
    // A fresh id invalidates a timer already queued before a callback re-arms it.
    if let Some(old) = core.timer.take() {
        super::timer_cancel(old)?;
    }
    let id = next_id();
    super::timer_arm(id, core.route, delay_ms)?;
    STATE
        .with(|state| state.borrow_mut().timers.get_mut(&id).unwrap().owner = Some(Pin::new(link)));
    core.timer = Some(id);
    Ok(())
}
pub unsafe fn deadline_cancel(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    let core = bind(core, link, "timer")?;
    if let Some(id) = core.timer.take() {
        super::timer_cancel(id)?;
    }
    Ok(())
}
pub unsafe fn deadline_park(core: *mut TransportCore, link: OwnerLink) -> NetResult<()> {
    deadline_cancel(core, link)
}
pub unsafe fn dispatch_plaintext(
    core: *mut TransportCore,
    link: OwnerLink,
    bytes: &[u8],
    eof: bool,
) -> NetResult<()> {
    let route = bind(core, link, "plaintext")?.route;
    let pin = Pin::new(link);
    let mut event = if eof {
        NetCompletion::eof(link.0 as i64)
    } else {
        NetCompletion::data(link.0 as i64, bytes)
    }
    .linked();
    event.flags |= sink::NET_FLAG_PLAINTEXT;
    sink::emit(route, event);
    drop(pin);
    Ok(())
}

/// Dispatch without retaining a payload borrow across the sink's JS calls.
pub(super) fn emit(route: u8, mut event: NetCompletion, pin: Option<&Pin>) {
    let Some(pin) = pin else {
        sink::emit(route, event);
        return;
    };
    let id = event.id;
    let mut route = route;
    let mut stale = false;
    unsafe {
        if let Some(core) = current(pin.0) {
            route = core.route;
            if event.kind == sink::NET_TIMER {
                if core.timer != Some(id) {
                    return;
                }
                core.timer = None;
            } else if event.kind == sink::NET_CLOSED {
                stale = core.id.is_some_and(|current| current != id);
            } else if core.id != Some(id) {
                return;
            }
        } else if event.kind != sink::NET_CLOSED {
            return;
        }
    }
    event.id = pin.0 .0 as i64;
    event = event.linked();
    let parts = parts(id);
    if event.kind == sink::NET_CLOSED {
        event.flags |= sink::NET_FLAG_HANDLE_PARTS;
        event.data = parts.as_ptr().cast();
        event.len = std::mem::size_of_val(&parts);
        if stale {
            event.flags |= sink::NET_FLAG_STALE;
        }
    }
    sink::emit(route, event);
}

struct AcceptSlot(Option<Handle>);
pub(super) fn accepted(_route: u8, id: i64, handle: Handle, pin: &Pin) {
    let Some(core) = (unsafe { current(pin.0) }) else {
        return;
    };
    if core.id != Some(id) {
        return;
    }
    let route = core.route;
    let mut slot = AcceptSlot(Some(handle));
    let event =
        NetCompletion::accept(pin.0 .0 as i64, &mut slot as *mut AcceptSlot as i64, None).linked();
    sink::emit(route, event); // An unclaimed Handle is closed by Drop.
}
pub unsafe fn install_accepted(
    core: *mut TransportCore,
    link: OwnerLink,
    completion: *const NetCompletion,
) -> NetResult<()> {
    let core = bind(core, link, "accept")?;
    idle(core, "accept")?;
    let event = completion.as_ref().ok_or_else(|| bad("accept"))?;
    if event.kind != sink::NET_ACCEPT || event.flags & sink::NET_FLAG_LINK == 0 || event.conn == 0 {
        return Err(bad("accept"));
    }
    // The slot is lent by accepted() for exactly this sink invocation.
    let mut handle = (*(event.conn as *mut AcceptSlot))
        .0
        .take()
        .ok_or_else(|| bad("accept"))?;
    handle.subsystem = core.route;
    let id = next_id();
    STATE.with(|state| state.borrow_mut().handles.insert(id, handle));
    attach(core, link, id);
    Ok(())
}
