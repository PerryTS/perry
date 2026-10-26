//! Native IOCP completion backend. Kernel storage is pinned separately from Rust
//! operation metadata and survives cancellation until the OS acknowledgement.
use crate::slots::{Slots, page_reserve};
mod bridge;
mod integration;
mod ipc;
mod pipes;
mod port;
mod process;
mod services;
mod signals;
mod socket;
mod sockopt;
mod sync_io;
mod timer;
mod watch;

use crate::{
    backend::{Backend, Budget, Event, Operation, Outcome, PollInfo, Request},
    *,
};
use integration::EventIntegration;
use port::{Entry, Port, TIMER, WAKE, Wait};
use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    mem::size_of,
    net::SocketAddr,
    os::windows::io::{AsRawHandle, AsRawSocket, OwnedHandle, OwnedSocket},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Networking::WinSock::*,
    Storage::FileSystem::{ReadFile, SetFileCompletionNotificationModes, WriteFile},
    System::IO::{CancelIoEx, OVERLAPPED},
};

fn invalid() -> Error {
    Error::new(ErrorKind::InvalidInput)
}
fn unsupported() -> Error {
    Error::new(ErrorKind::Unsupported)
}
fn os_error() -> Error {
    std::io::Error::last_os_error().into()
}
fn bool_result(value: i32) -> Result<()> {
    if value == 0 { Err(os_error()) } else { Ok(()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Tcp,
    Listener,
    Udp,
    Pipe,
    PipeConnecting,
    PipeListener,
    Sync,
}
#[derive(Debug)]
enum Native {
    Socket(OwnedSocket),
    Handle(OwnedHandle),
    PipeListener,
}
impl Native {
    fn raw(&self) -> HANDLE {
        match self {
            Self::Socket(s) => s.as_raw_socket() as HANDLE,
            Self::Handle(h) => h.as_raw_handle(),
            Self::PipeListener => ptr::null_mut(),
        }
    }
}
/// Owning, quiescent Windows transport. Transfer preserves independent ownership;
/// an association with a different IOCP is routed through overlapped events.
#[derive(Debug)]
pub struct Detached {
    native: Native,
    kind: Kind,
    routed: bool,
    port: Option<Arc<Port>>,
    mode: Option<u32>,
    console_input: bool,
    /// A listener's per-connection defaults, applied to each socket it accepts.
    /// It travels with the listener across detach/attach, so a listener handed to
    /// another loop keeps configuring its connections there.
    accept_defaults: AcceptDefaults,
}
impl Detached {
    fn new(native: Native, kind: Kind, routed: bool) -> Self {
        Self {
            native,
            kind,
            routed,
            port: None,
            mode: None,
            console_input: false,
            accept_defaults: AcceptDefaults::EMPTY,
        }
    }
    /// The socket or handle this transport owns, for host reporting only.
    /// A pipe listener owns no instance of its own and reports Unsupported.
    pub fn raw_transport(&self) -> Result<crate::RawTransport> {
        match &self.native {
            Native::Socket(s) => Ok(crate::RawTransport::Socket(s.as_raw_socket() as usize)),
            Native::Handle(h) => Ok(crate::RawTransport::Handle(h.as_raw_handle() as usize)),
            Native::PipeListener => Err(unsupported()),
        }
    }
    /// Give the socket to the caller; turnloop never touches it again.
    ///
    /// `Driver::detach` already proved quiescence, so no operation, kernel
    /// storage or worker of this loop refers to the socket. It is handed over
    /// exactly as the loop held it: non-blocking (`FIONBIO`), and with
    /// `FILE_SKIP_COMPLETION_PORT_ON_SUCCESS` still set if the provider is an
    /// IFS provider. Call `ioctlsocket(FIONBIO, 0)` (or
    /// `TcpStream::set_nonblocking(false)`) for blocking I/O, which is what a
    /// synchronous TLS handshake on the socket needs.
    ///
    /// **The IOCP association is permanent and travels with the socket.** Windows
    /// has no way to dissociate a handle from a completion port, and rejects a
    /// second `CreateIoCompletionPort` with `ERROR_INVALID_PARAMETER`. Nor is
    /// there a way around it: the association belongs to the underlying socket,
    /// and `WSADuplicateSocketW` only produces another descriptor *for that same
    /// socket*, so a duplicate is refused too (this is asserted, not assumed).
    ///
    /// It is inert, though. `Driver::detach` proved the transport quiescent, so
    /// no completion packet will ever be posted for it. The receiving host has
    /// two ways to drive it, and one route back:
    ///
    /// * **Synchronous or non-blocking Winsock calls** — `recv`/`send`/`select`
    ///   and `WSARecv`/`WSASend` without an `OVERLAPPED`. These never touch a
    ///   completion port and are the supported way to use a handed-over socket.
    /// * **Overlapped calls with `hEvent` tagged** — set the low-order bit of
    ///   `OVERLAPPED.hEvent` (`hEvent | 1`). Windows then skips queueing the
    ///   completion packet, and the host waits on its own event.
    /// * **Back onto a loop** — `Detached::from_socket` and `Driver::attach`.
    ///   That is what an imported association is for: the backend detects it and
    ///   routes completions through overlapped events instead of the port.
    ///
    /// Issuing an untagged overlapped call is the one thing that is not allowed:
    /// its completion packet would arrive on the source loop's port carrying an
    /// `OVERLAPPED` that loop does not own, and that loop's next `turn` reports
    /// `InvalidInput` rather than dereferencing it.
    pub fn into_socket(self) -> Result<OwnedSocket> {
        if !matches!(self.native, Native::Socket(_)) {
            return Err(invalid());
        }
        match self.take_native() {
            Native::Socket(socket) => Ok(socket),
            _ => Err(invalid()),
        }
    }
    /// Give the named-pipe instance, console or adopted stream handle to the
    /// caller; turnloop never touches it again. A socket is `InvalidInput`
    /// (use [`into_socket`](Self::into_socket)) and a pipe listener, which owns
    /// no instance of its own, is `Unsupported`.
    ///
    /// A captured console mode is restored first, exactly as on close. Every
    /// other property is handed over unchanged, including
    /// `FILE_FLAG_OVERLAPPED` on a pipe instance: the receiving host must supply
    /// an `OVERLAPPED` for every `ReadFile`/`WriteFile`, and the IOCP rules in
    /// [`into_socket`](Self::into_socket) apply unchanged — `DuplicateHandle`
    /// shares the file object and therefore the association, exactly as
    /// `WSADuplicateSocketW` does for a socket. So tagging `OVERLAPPED.hEvent`
    /// with its low-order bit (`hEvent | 1`) is how a host drives one, and it is
    /// enough: the host waits on its own event and the source loop's port never
    /// sees a packet.
    pub fn into_handle(self) -> Result<OwnedHandle> {
        match self.native {
            Native::Handle(_) => {}
            Native::Socket(_) => return Err(invalid()),
            Native::PipeListener => return Err(unsupported()),
        }
        match self.take_native() {
            Native::Handle(handle) => Ok(handle),
            _ => Err(invalid()),
        }
    }
    /// Restore adopted settings, then move the native resource out. What is left
    /// behind owns nothing, so the usual Drop releases the rest of the transport.
    fn take_native(mut self) -> Native {
        self.restore();
        self.mode = None;
        std::mem::replace(&mut self.native, Native::PipeListener)
    }
    fn restore(&self) {
        if let Some(mode) = self.mode {
            // SAFETY: owned console handle is still live; mode was captured on adoption.
            unsafe {
                windows_sys::Win32::System::Console::SetConsoleMode(self.native.raw(), mode);
            }
        }
    }
}
impl Drop for Detached {
    fn drop(&mut self) {
        self.restore();
    }
}
struct Resource {
    handle: Handle,
    transport: Detached,
    connect: Option<SocketAddr>,
    pipe_connect: Option<pipes::Connect>,
    listener: Option<pipes::Listener>,
    skip: bool,
    heads: [Option<usize>; 2],
    tails: [Option<usize>; 2],
}
#[repr(C)]
struct Kernel {
    overlapped: OVERLAPPED,
    addr: SOCKADDR_STORAGE,
    addr_len: i32,
    flags: u32,
    accept_addr: [u8; 288],
    wire: [u32; 160],
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Start,
    Io,
    IdleRead,
}
struct Pending {
    request: Request,
    next: Option<usize>,
    stage: Stage,
    waiting: bool,
    queued: bool,
    pool_wait: bool,
    listener_wait: bool,
    /// Listed in `throttled`: a multishot accept held back by the budget. It
    /// is not re-armed, and a connection it already completed stays here.
    throttled: bool,
    cancelled: bool,
    completion: Option<Result<u32>>,
    offset: usize,
    lease: Option<BufLease>,
    accepted: Option<Detached>,
    passed: Option<OwnedSocket>,
    direction: usize,
}
fn direction(operation: &Operation) -> usize {
    usize::from(!matches!(
        operation,
        Operation::Read { .. }
            | Operation::RecvFrom(_)
            | Operation::Accept { .. }
            | Operation::RecvHandle
    ))
}
/// Lifetime-safe, parking-aware IOCP wake endpoint.
pub struct IocpWake {
    port: Arc<Port>,
    calls: AtomicU64,
}
impl crate::backend::Wake for IocpWake {
    fn wake(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.port.post(WAKE, 0).map_err(Into::into)
    }
    fn syscall_count(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
}
/// One IOCP per host-driven loop, with fixed operation storage and an opt-in GUI helper.
pub struct Iocp {
    resources: Slots<Resource>,
    ops: Slots<Pending>,
    kernel: Box<[UnsafeCell<Kernel>]>,
    /// One per `kernel` slot, so it is sized with that slab rather than paged:
    /// a bridge holds its slot's pinned address, and the reverse map from an
    /// `OVERLAPPED` pointer back to an operation index needs `kernel` contiguous.
    bridges: Vec<Option<bridge::Bridge>>,
    ready: VecDeque<usize>,
    /// Multishot accepts out of budget: not runnable, so they neither count
    /// as work nor shorten a wait, until a poll with a positive budget (#77).
    throttled: VecDeque<usize>,
    /// Connections multishot accepts may still deliver in the current poll.
    accepts: usize,
    pool_waiting: VecDeque<OpId>,
    pool: BufferPool,
    port: Arc<Port>,
    wake: Arc<IocpWake>,
    timer: timer::PacketTimer,
    timer_pending: bool,
    event: Option<EventIntegration>,
    notifier: Option<Notifier>,
    services: services::Services,
    workers: Slots<[sync_io::Worker; 2]>,
    deadline: Option<Instant>,
    failure: Option<Error>,
    next_listener_key: usize,
    watches: watch::Watches,
}
impl Iocp {
    #[cfg(test)]
    pub(crate) fn fail_event_for_test(&self, code: i32) {
        self.event
            .as_ref()
            .expect("active helper")
            .fail_and_wait(code);
    }
    #[cfg(test)]
    pub(crate) fn operation_waiting_for_test(&self, op: OpId) -> bool {
        self.ops[op.index()]
            .as_ref()
            .is_some_and(|p| p.request.op == op && p.waiting)
    }
    /// The Winsock socket behind a handle. Process, signal and watch handles, named
    /// pipes and adopted files/consoles are not sockets and say so.
    fn socket_of(&self, h: Handle) -> Result<usize> {
        if self.services.contains(h) || self.watches.contains(h) {
            return Err(unsupported());
        }
        let r = self.get(h)?;
        match r.transport.native {
            Native::Socket(_) => Ok(r.transport.native.raw() as usize),
            _ => Err(unsupported()),
        }
    }
    fn get(&self, h: Handle) -> Result<&Resource> {
        self.resources
            .get(h.index())
            .and_then(Option::as_ref)
            .filter(|r| r.handle == h)
            .ok_or(Error::new(ErrorKind::NotFound))
    }
    fn install(
        &mut self,
        h: Handle,
        mut transport: Detached,
        connect: Option<SocketAddr>,
        pipe_connect: Option<pipes::Connect>,
    ) -> Result<()> {
        if self.resources.get(h.index()).is_none_or(Option::is_some) {
            return Err(invalid());
        }
        if let Some(port) = &transport.port {
            transport.routed = !Arc::ptr_eq(port, &self.port);
        }
        let raw = transport.native.raw();
        let mut skip = false;
        if let Native::Socket(_) = transport.native {
            socket::nonblocking(raw as usize)?;
        }
        if !matches!(transport.kind, Kind::Sync | Kind::PipeListener) && transport.port.is_none() {
            // SAFETY: exclusively owned quiescent overlapped handle. An
            // unknown imported association may reject reassociation; no I/O
            // was submitted and no pointer is retained by this setup call.
            match unsafe { self.port.associate(raw, 1) } {
                Ok(()) => {
                    transport.routed = false;
                    transport.port = Some(Arc::clone(&self.port));
                }
                Err(error)
                    if transport.routed
                        && error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) =>
                {
                    // A validated imported overlapped handle already belongs
                    // to another port. Its immutable association needs routing.
                }
                Err(error) => return Err(error.into()),
            }
        }
        if !matches!(transport.kind, Kind::Sync | Kind::PipeListener) && !transport.routed {
            let ifs = if matches!(transport.native, Native::Socket(_)) {
                socket::ifs(raw as usize)?
            } else {
                true
            };
            if ifs {
                // SAFETY: live overlapped IFS handle. Unsupported skip mode retains
                // normal completion packets, including synchronous successes.
                skip = unsafe { SetFileCompletionNotificationModes(raw, 1) } != 0;
            }
        }
        if transport.kind == Kind::Sync {
            // Reserve both directions and classify once, while still quiescent.
            // A worker must not query console mode behind the other's idle I/O.
            self.workers[h.index()] = Some(sync_io::Worker::pair(
                raw,
                transport.mode.is_some(),
                Arc::clone(&self.port),
            )?);
        }
        self.resources[h.index()] = Some(Resource {
            handle: h,
            transport,
            connect,
            pipe_connect,
            listener: None,
            skip,
            heads: [None; 2],
            tails: [None; 2],
        });
        Ok(())
    }
    fn schedule(&mut self, i: usize) {
        if let Some(p) = &mut self.ops[i]
            && !p.queued
            && !p.waiting
            && !p.listener_wait
            && (!p.throttled || p.cancelled)
        {
            p.queued = true;
            self.ready.push_back(i);
        }
    }
    fn unlink(&mut self, i: usize, p: &Pending) {
        let h = p.request.handle;
        let d = p.direction;
        let r = self.resources[h.index()].as_mut().expect("live resource");
        if r.heads[d] == Some(i) {
            r.heads[d] = p.next;
            if r.tails[d] == Some(i) {
                r.tails[d] = None;
            }
        } else {
            let mut at = r.heads[d];
            while let Some(j) = at {
                let previous = self.ops[j].as_mut().expect("queued operation");
                if previous.next == Some(i) {
                    previous.next = p.next;
                    if r.tails[d] == Some(i) {
                        r.tails[d] = Some(j);
                    }
                    break;
                }
                at = previous.next;
            }
        }
        if let Some(next) = r.heads[d] {
            self.schedule(next);
        }
    }
    fn run_ready(&mut self, events: &mut Vec<Event<Detached>>) {
        if self.pool.available() {
            while let Some(op) = self.pool_waiting.pop_front() {
                self.ops[op.index()]
                    .as_mut()
                    .expect("pool waiter")
                    .pool_wait = false;
                self.schedule(op.index());
            }
        }
        let budget = self.ready.len();
        for _ in 0..budget {
            if events.len() == events.capacity() {
                break;
            }
            let i = self.ready.pop_front().expect("ready operation");
            let Some(mut p) = self.ops[i].take() else {
                continue;
            };
            p.queued = false;
            if !p.cancelled && self.accepts == 0 && Budget::throttles(&p.request.operation) {
                // Out of budget: take nothing more from the kernel, and keep a
                // connection that has already completed in `p` until a poll can
                // house it.
                if !p.throttled {
                    p.throttled = true;
                    self.throttled.push_back(i);
                }
                self.ops[i] = Some(p);
                continue;
            }
            let result = if p.cancelled {
                Ok(Some((Outcome::Cancelled, true)))
            } else {
                self.execute(i, &mut p)
            };
            let result = match result {
                Ok(value) => value,
                Err(error) => {
                    self.unlink(i, &p);
                    events.push(Event {
                        op: p.request.op,
                        terminal: true,
                        result: Err(error),
                    });
                    continue;
                }
            };
            if let Some((result, terminal)) = result {
                if terminal {
                    self.unlink(i, &p);
                }
                let event = Event {
                    op: p.request.op,
                    terminal,
                    result: Ok(result),
                };
                if Budget::spends(&event) {
                    self.accepts -= 1;
                }
                events.push(event);
                if terminal {
                    continue;
                }
                p.stage = Stage::Start;
                p.completion = None;
            }
            let runnable = !p.waiting && !p.pool_wait && !p.listener_wait;
            if p.pool_wait {
                self.pool_waiting.push_back(p.request.op);
            }
            self.ops[i] = Some(p);
            if runnable {
                self.schedule(i);
            }
        }
    }
    fn kernel_ptr(&self, i: usize) -> *mut OVERLAPPED {
        self.kernel[i].get().cast()
    }
    fn io_raw(&self, p: &Pending) -> HANDLE {
        if matches!(p.request.operation, Operation::Accept { .. })
            && let Some(accepted) = &p.accepted
            && accepted.kind == Kind::Pipe
        {
            return accepted.native.raw();
        }
        self.resources[p.request.handle.index()]
            .as_ref()
            .expect("resource")
            .transport
            .native
            .raw()
    }
    fn prepare(&mut self, i: usize, p: &Pending) -> Result<*mut Kernel> {
        let raw = self.kernel[i].get();
        // SAFETY: caller only prepares after the previous native completion and
        // registered-wait callback have both quiesced. Slab storage never moves.
        unsafe {
            ptr::write(raw, std::mem::zeroed());
        }
        let resource = self.get(p.request.handle)?;
        if resource.transport.routed {
            if self.bridges[i].is_none() {
                self.bridges[i] = Some(bridge::Bridge::new(
                    Arc::clone(&self.port),
                    self.kernel_ptr(i),
                )?);
            }
            let handle = self.io_raw(p);
            let event = self.bridges[i].as_mut().expect("bridge").prepare(handle)?;
            // SAFETY: inactive kernel slot. Low event bit suppresses the old IOCP
            // association, including when socket duplication retained that association.
            unsafe {
                (*raw).overlapped.hEvent = (event as usize | 1) as HANDLE;
            }
        }
        Ok(raw)
    }
    fn submitted(
        &mut self,
        i: usize,
        p: &mut Pending,
        success: bool,
        bytes: u32,
        error: Error,
        stage: Stage,
    ) -> Result<()> {
        let r = self.get(p.request.handle)?;
        p.stage = stage;
        if success && (r.skip || r.transport.routed) {
            p.completion = Some(Ok(bytes));
        } else if success || error.os == Some(ERROR_IO_PENDING as i32) {
            p.waiting = true;
            if r.transport.routed {
                // A failed wait registration cannot release an in-flight request.
                // Bridge::start cancels and synchronously drains on setup failure.
                if let Err(error) = self.bridges[i].as_mut().expect("bridge").start() {
                    p.waiting = false;
                    return Err(error);
                }
            }
        } else {
            p.completion = Some(Err(error));
        }
        Ok(())
    }
    fn execute(&mut self, i: usize, p: &mut Pending) -> Result<Option<(Outcome<Detached>, bool)>> {
        if matches!(
            p.request.operation,
            Operation::SendHandle(_) | Operation::RecvHandle
        ) {
            return self.ipc_step(i, p);
        }
        let r = self.get(p.request.handle)?;
        if r.transport.kind == Kind::PipeConnecting {
            return self.pipe_connect_step(i, p);
        }
        let raw = r.transport.native.raw();
        let kind = r.transport.kind;
        let defaults = r.transport.accept_defaults;
        if let Some(result) = p.completion.take() {
            let bytes = match result {
                Ok(bytes) => bytes as usize,
                Err(error)
                    if matches!(p.request.operation, Operation::Read { .. })
                        && matches!(error.os, Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_HANDLE_EOF as i32) =>
                {
                    return Ok(Some((Outcome::Eof, true)));
                }
                Err(error) => return Err(error),
            };
            if p.stage == Stage::IdleRead {
                return self.read_ready(p, raw as usize);
            }
            match &p.request.operation {
                Operation::Connect => {
                    if kind == Kind::Tcp {
                        // SAFETY: successful ConnectEx requires context update before use.
                        socket::check(unsafe {
                            setsockopt(
                                raw as usize,
                                SOL_SOCKET,
                                SO_UPDATE_CONNECT_CONTEXT,
                                ptr::null(),
                                0,
                            )
                        })?;
                    }
                    return Ok(Some((Outcome::Connected, true)));
                }
                Operation::Accept { multishot } => {
                    let transport = p.accepted.take().expect("accept transport");
                    let outcome = if kind == Kind::Listener {
                        let listener = raw as usize;
                        // SAFETY: AcceptEx completed; option consumes the live listener value.
                        socket::check(unsafe {
                            setsockopt(
                                transport.native.raw() as usize,
                                SOL_SOCKET,
                                SO_UPDATE_ACCEPT_CONTEXT,
                                ptr::from_ref(&listener).cast(),
                                size_of::<usize>() as i32,
                            )
                        })?;
                        // Before the connection becomes visible to the host, and
                        // after the accept context makes the socket queryable. A
                        // rejected default fails this accept and drops the socket.
                        sockopt::apply_accept_defaults(transport.native.raw() as usize, defaults)?;
                        let peer = socket::address(transport.native.raw() as usize, true)?;
                        Outcome::Accepted { transport, peer }
                    } else {
                        Outcome::PipeAccepted(transport)
                    };
                    return Ok(Some((outcome, !multishot)));
                }
                Operation::Read { multishot, .. } => {
                    if bytes == 0 {
                        return Ok(Some((Outcome::Eof, true)));
                    }
                    let mut lease = p.lease.take();
                    if let Some(lease) = &mut lease {
                        lease.set_len(bytes);
                    }
                    return Ok(Some((Outcome::Read { n: bytes, lease }, !multishot)));
                }
                Operation::RecvFrom(_) => {
                    // SAFETY: native receive has completed, so the address output is quiescent.
                    let k = unsafe { &*self.kernel[i].get() };
                    let from = socket::decode(&k.addr, k.addr_len)?;
                    let mut lease = p.lease.take();
                    if let Some(lease) = &mut lease {
                        lease.set_len(bytes);
                    }
                    return Ok(Some((
                        Outcome::RecvFrom {
                            n: bytes,
                            from,
                            lease,
                        },
                        true,
                    )));
                }
                Operation::SendTo { .. } => return Ok(Some((Outcome::Wrote(bytes), true))),
                Operation::Write(_) | Operation::Writev(_) => {
                    if bytes == 0 && write_length(&p.request.operation) != p.offset {
                        return Err(Error::new(ErrorKind::BrokenPipe));
                    }
                    p.offset += bytes;
                    if p.offset == write_length(&p.request.operation) {
                        return Ok(Some((Outcome::Wrote(p.offset), true)));
                    }
                    p.stage = Stage::Start;
                }
                _ => return Err(unsupported()),
            }
        }
        let r = self.get(p.request.handle)?;
        if kind == Kind::Sync {
            let write = matches!(
                p.request.operation,
                Operation::Write(_) | Operation::Writev(_)
            );
            let (buffer, len) = if write {
                let mut buffers = [WSABUF {
                    len: 0,
                    buf: ptr::null_mut(),
                }; MAX_IOV];
                if write_buffers(&p.request.operation, p.offset, &mut buffers) == 0 {
                    return Ok(Some((Outcome::Wrote(p.offset), true)));
                }
                (buffers[0].buf, buffers[0].len)
            } else {
                match read_buffer(p, &self.pool) {
                    Some(buf) => buf,
                    None => {
                        p.pool_wait = true;
                        return Ok(None);
                    }
                }
            };
            let index = p.request.handle.index();
            self.workers[index].as_ref().expect("stdio workers")[p.direction].start(
                buffer,
                len,
                self.kernel_ptr(i),
            );
            p.stage = Stage::Io;
            p.waiting = true;
            return Ok(None);
        }
        match &p.request.operation {
            Operation::Shutdown => {
                if matches!(kind, Kind::Tcp) {
                    // SAFETY: live stream; sends ahead of shutdown have completed in FIFO order.
                    socket::check(unsafe { shutdown(raw as usize, SD_SEND) })?;
                } else {
                    return Err(unsupported());
                }
                return Ok(Some((Outcome::Shutdown, true)));
            }
            Operation::Connect if kind == Kind::Pipe => {
                return Ok(Some((Outcome::Connected, true)));
            }
            Operation::Read {
                buf: ReadBuf::Pooled,
                ..
            } if kind == Kind::Tcp => {
                let k = self.prepare(i, p)?;
                let buf = WSABUF {
                    len: 0,
                    buf: ptr::null_mut(),
                };
                let mut bytes = 0;
                // SAFETY: pinned OVERLAPPED/flags, descriptor captured synchronously;
                // zero-byte receive retains no payload or pool lease while idle.
                let ok = unsafe {
                    WSARecv(
                        raw as usize,
                        &buf,
                        1,
                        &mut bytes,
                        ptr::addr_of_mut!((*k).flags),
                        k.cast(),
                        None,
                    )
                } == 0;
                self.submitted(i, p, ok, bytes, socket::last_error(), Stage::IdleRead)?;
            }
            Operation::Read { .. } | Operation::RecvFrom(_) => {
                let (buf, len) = match read_buffer(p, &self.pool) {
                    Some(buf) => buf,
                    None => {
                        p.pool_wait = true;
                        return Ok(None);
                    }
                };
                let k = self.prepare(i, p)?;
                let mut bytes = 0;
                let (ok, error) = if matches!(kind, Kind::Tcp | Kind::Udp) {
                    let buf = WSABUF { len, buf };
                    // SAFETY: stable provided/leased storage; kernel output lives in
                    // the pinned slab through acknowledgement. WSABUF is captured.
                    let code = unsafe {
                        if kind == Kind::Udp {
                            (*k).addr_len = size_of::<SOCKADDR_STORAGE>() as i32;
                            WSARecvFrom(
                                raw as usize,
                                &buf,
                                1,
                                &mut bytes,
                                ptr::addr_of_mut!((*k).flags),
                                ptr::addr_of_mut!((*k).addr).cast(),
                                ptr::addr_of_mut!((*k).addr_len),
                                k.cast(),
                                None,
                            )
                        } else {
                            WSARecv(
                                raw as usize,
                                &buf,
                                1,
                                &mut bytes,
                                ptr::addr_of_mut!((*k).flags),
                                k.cast(),
                                None,
                            )
                        }
                    };
                    (code == 0, socket::last_error())
                } else {
                    // SAFETY: live overlapped pipe and stable exclusive buffer until completion.
                    let ok = unsafe { ReadFile(raw, buf, len, &mut bytes, k.cast()) } != 0;
                    (ok, os_error())
                };
                self.submitted(i, p, ok, bytes, error, Stage::Io)?;
            }
            Operation::Write(_) | Operation::Writev(_) | Operation::SendTo { .. } => {
                let k = self.prepare(i, p)?;
                let mut buffers = [WSABUF {
                    len: 0,
                    buf: ptr::null_mut(),
                }; MAX_IOV];
                let n = write_buffers(&p.request.operation, p.offset, &mut buffers);
                if n == 0 && !matches!(p.request.operation, Operation::SendTo { .. }) {
                    return Ok(Some((Outcome::Wrote(p.offset), true)));
                }
                let mut bytes = 0;
                let (ok, error) = if matches!(kind, Kind::Tcp | Kind::Udp) {
                    // SAFETY: request retains immutable payload until completion;
                    // Winsock captures stack descriptors and destination synchronously.
                    let code = unsafe {
                        if let Operation::SendTo { to, .. } = p.request.operation {
                            let addr = socket::Addr::new(to);
                            WSASendTo(
                                raw as usize,
                                buffers.as_ptr(),
                                n.max(1) as u32,
                                &mut bytes,
                                0,
                                addr.as_ptr(),
                                addr.len,
                                k.cast(),
                                None,
                            )
                        } else {
                            WSASend(
                                raw as usize,
                                buffers.as_ptr(),
                                n as u32,
                                &mut bytes,
                                0,
                                k.cast(),
                                None,
                            )
                        }
                    };
                    (code == 0, socket::last_error())
                } else {
                    // SAFETY: overlapped pipe; first remaining segment retained until completion.
                    let ok = unsafe {
                        WriteFile(raw, buffers[0].buf, buffers[0].len, &mut bytes, k.cast())
                    } != 0;
                    (ok, os_error())
                };
                self.submitted(i, p, ok, bytes, error, Stage::Io)?;
            }
            Operation::Connect => {
                let addr = socket::Addr::new(r.connect.ok_or_else(invalid)?);
                // SAFETY: nullable ConnectEx pointer matches the provider GUID.
                let connect =
                    unsafe { socket::extension::<LPFN_CONNECTEX>(raw as usize, &WSAID_CONNECTEX) }?
                        .ok_or_else(unsupported)?;
                let k = self.prepare(i, p)?;
                let mut bytes = 0;
                // SAFETY: explicitly bound socket; address captured; pinned OVERLAPPED.
                let ok = unsafe {
                    connect(
                        raw as usize,
                        addr.as_ptr(),
                        addr.len,
                        ptr::null(),
                        0,
                        &mut bytes,
                        k.cast(),
                    )
                } != 0;
                self.submitted(i, p, ok, bytes, socket::last_error(), Stage::Io)?;
            }
            Operation::Accept { .. } if kind == Kind::Listener => {
                let v6 = socket::address(raw as usize, false)?.is_ipv6();
                p.accepted = Some(Detached::new(
                    Native::Socket(socket::create(v6, false)?),
                    Kind::Tcp,
                    false,
                ));
                // SAFETY: nullable AcceptEx pointer matches the provider GUID.
                let accept =
                    unsafe { socket::extension::<LPFN_ACCEPTEX>(raw as usize, &WSAID_ACCEPTEX) }?
                        .ok_or_else(unsupported)?;
                let k = self.prepare(i, p)?;
                let accepted = p.accepted.as_ref().expect("socket").native.raw() as usize;
                let mut bytes = 0;
                // SAFETY: listener and secondary socket stay owned until completion;
                // address buffer is pinned and has two sockaddr_storage + 16 regions.
                let ok = unsafe {
                    accept(
                        raw as usize,
                        accepted,
                        ptr::addr_of_mut!((*k).accept_addr).cast(),
                        0,
                        144,
                        144,
                        &mut bytes,
                        k.cast(),
                    )
                } != 0;
                self.submitted(i, p, ok, bytes, socket::last_error(), Stage::Io)?;
            }
            Operation::Accept { multishot } if kind == Kind::PipeListener => {
                let listener = self.resources[p.request.handle.index()]
                    .as_mut()
                    .expect("listener")
                    .listener
                    .as_mut()
                    .expect("pipe backlog");
                if let Some(transport) = listener.accept()? {
                    return Ok(Some((Outcome::PipeAccepted(transport), !multishot)));
                }
                // Listener instances own their kernel operations. This request
                // waits for a backlog entry, and cancellation needs no native I/O.
                p.listener_wait = true;
            }
            _ => return Err(unsupported()),
        }
        Ok(None)
    }
    fn pipe_connect_step(
        &mut self,
        i: usize,
        p: &mut Pending,
    ) -> Result<Option<(Outcome<Detached>, bool)>> {
        if let Some(result) = p.completion.take() {
            result?;
        }
        let r = self.get(p.request.handle)?;
        match pipes::connect(&r.pipe_connect.as_ref().expect("connect state").name) {
            Ok(mut transport) => {
                // SAFETY: newly opened overlapped client, no existing association.
                unsafe { self.port.associate(transport.native.raw(), 1) }?;
                // SAFETY: live, uniquely owned named pipe; failure keeps normal packets.
                let skip =
                    unsafe { SetFileCompletionNotificationModes(transport.native.raw(), 1) } != 0;
                transport.port = Some(Arc::clone(&self.port));
                let r = self.resources[p.request.handle.index()]
                    .as_mut()
                    .expect("client");
                r.transport = transport;
                r.skip = skip;
                r.pipe_connect = None;
                Ok(Some((Outcome::Connected, true)))
            }
            Err(error) if error.os == Some(ERROR_PIPE_BUSY as i32) => {
                let k = self.prepare(i, p)?;
                let r = self.get(p.request.handle)?;
                let state = r.pipe_connect.as_ref().expect("connect state");
                // SAFETY: pinned operation; state owns event and immutable wait buffer
                // through completion/cancellation. Only availability causes retries.
                let status = unsafe { state.wait(r.transport.native.raw(), k.cast()) };
                let ok = status >= 0 && status != STATUS_PENDING;
                // SAFETY: pure conversion of the native result, including pending.
                let error = socket::error(unsafe { RtlNtStatusToDosError(status) } as i32);
                self.submitted(i, p, ok, 0, error, Stage::Io)?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    fn read_ready(
        &mut self,
        p: &mut Pending,
        raw: usize,
    ) -> Result<Option<(Outcome<Detached>, bool)>> {
        let Some(mut lease) = self.pool.acquire() else {
            p.pool_wait = true;
            p.completion = Some(Ok(0));
            return Ok(None);
        };
        let buf = lease.writable();
        // SAFETY: nonblocking socket and exclusive lease; synchronous recv retains no pointer.
        let n = unsafe {
            recv(
                raw,
                buf.as_mut_ptr(),
                buf.len().min(i32::MAX as usize) as i32,
                0,
            )
        };
        if n == SOCKET_ERROR {
            let error = socket::last_error();
            if error.kind == ErrorKind::WouldBlock {
                p.stage = Stage::Start;
                return Ok(None);
            }
            return Err(error);
        }
        if n == 0 {
            return Ok(Some((Outcome::Eof, true)));
        }
        lease.set_len(n as usize);
        let multishot = matches!(
            p.request.operation,
            Operation::Read {
                multishot: true,
                ..
            }
        );
        Ok(Some((
            Outcome::Read {
                n: n as usize,
                lease: Some(lease),
            },
            !multishot,
        )))
    }
    fn entries(&mut self, entries: &[Entry]) -> Result<()> {
        let mut result = Ok(());
        for entry in entries {
            // Every dequeued packet must be applied even after an earlier error.
            // Keeping the first error must not strand later native operations.
            let applied = self.entry(entry);
            result = result.and(applied);
        }
        result
    }
    fn entry(&mut self, entry: &Entry) -> Result<()> {
        if entry.key == WAKE || entry.key == port::STOP {
            return Ok(());
        }
        if entry.key == TIMER {
            // SAFETY: this port has exactly one associated deadline timer.
            if unsafe { self.timer.dequeued(entry.overlapped) } {
                self.timer_pending = false;
            }
            if self.event.is_some()
                && self
                    .deadline
                    .is_some_and(|deadline| deadline > Instant::now())
            {
                self.arm_event_deadline()?;
            }
            return Ok(());
        }
        let base = self.kernel.as_ptr() as usize;
        let end = base + std::mem::size_of_val(&*self.kernel);
        if entry.key >= pipes::FIRST_KEY
            && entry.key < port::STOP
            && !(base..end).contains(&entry.overlapped)
        {
            if self.watches.completed(entry)? {
                return Ok(());
            }
            // Listener keys are never reused in this loop. Cancellation packets
            // can arrive after release; ignore those without dereferencing them.
            for resource in self.resources.iter_mut().flatten() {
                if let Some(listener) = &mut resource.listener
                    && listener.key() == entry.key
                {
                    listener.completed(entry)?;
                    if let Some(i) = resource.heads[0] {
                        self.ops[i].as_mut().expect("accept").listener_wait = false;
                        self.schedule(i);
                    }
                    break;
                }
            }
            return Ok(());
        }
        let stride = size_of::<UnsafeCell<Kernel>>();
        let offset = entry.overlapped.checked_sub(base).ok_or_else(invalid)?;
        if offset % stride != 0 || offset / stride >= self.ops.len() {
            return Err(invalid());
        }
        let i = offset / stride;
        let p = self.ops[i].as_ref().ok_or_else(invalid)?;
        if !p.waiting {
            return Err(invalid());
        }
        let bridge_result = if entry.key == sync_io::KEY {
            Some(
                self.workers[p.request.handle.index()]
                    .as_ref()
                    .ok_or_else(invalid)?[p.direction]
                    .finish(),
            )
        } else if entry.key == bridge::KEY {
            Some(self.bridges[i].as_mut().ok_or_else(invalid)?.finish()?)
        } else {
            None
        };
        let p = self.ops[i].as_mut().ok_or_else(invalid)?;
        p.waiting = false;
        p.completion = Some(bridge_result.unwrap_or_else(|| {
            if entry.status < 0 {
                // SAFETY: pure NTSTATUS conversion, no pointers.
                Err(socket::error(
                    unsafe { RtlNtStatusToDosError(entry.status) } as i32,
                ))
            } else {
                Ok(entry.bytes)
            }
        }));
        self.schedule(i);
        Ok(())
    }
}
// SAFETY: accepted requests and pinned kernel storage stay owned until terminal
// acknowledgement. Cancel never frees pending storage. Drop cancels and drains
// native I/O and joins routed completion callbacks before releasing any buffers.
unsafe impl Backend for Iocp {
    type Wake = IocpWake;
    type Detached = Detached;
    fn new(config: &Config, pool: BufferPool) -> Result<Self> {
        socket::startup()?;
        let port = Arc::new(Port::new()?);
        let kernel: Box<[UnsafeCell<Kernel>]> = (0..config.max_operations)
            .map(|_| {
                // SAFETY: inactive all-zero kernel I/O storage is valid.
                UnsafeCell::new(unsafe { std::mem::zeroed() })
            })
            .collect();
        let bridges = kernel
            .iter()
            .map(|slot| bridge::Bridge::new(Arc::clone(&port), slot.get().cast()).map(Some))
            .collect::<Result<Vec<_>>>()?;
        let watches = watch::Watches::new(config, pool.clone(), Arc::clone(&port));
        Ok(Self {
            resources: Slots::new(config.max_handles),
            ops: Slots::new(config.max_operations),
            kernel,
            bridges,
            ready: VecDeque::with_capacity(page_reserve(config.max_operations)),
            throttled: VecDeque::with_capacity(page_reserve(config.max_operations)),
            accepts: usize::MAX,
            pool_waiting: VecDeque::with_capacity(page_reserve(config.max_operations)),
            pool,
            wake: Arc::new(IocpWake {
                port: Arc::clone(&port),
                calls: AtomicU64::new(0),
            }),
            timer: timer::PacketTimer::new(Arc::clone(&port))?,
            timer_pending: false,
            port,
            event: None,
            notifier: None,
            services: services::Services::new(config.max_handles),
            workers: Slots::new(config.max_handles),
            deadline: None,
            failure: None,
            next_listener_key: pipes::FIRST_KEY,
            watches,
        })
    }
    fn set_notifier(&mut self, notifier: Notifier) {
        self.notifier = Some(notifier);
    }
    fn fs_watch(&mut self, handle: Handle, path: &FsPath, options: WatchOptions) -> Result<()> {
        if self
            .resources
            .get(handle.index())
            .is_none_or(Option::is_some)
            || self.services.contains(handle)
        {
            return Err(invalid());
        }
        let key = self.next_listener_key;
        self.next_listener_key = key
            .checked_add(1)
            .filter(|key| *key < port::STOP)
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        self.watches.start(handle, path, options.recursive, key)
    }
    fn spawn(
        &mut self,
        handle: Handle,
        pipes: [Option<Handle>; 3],
        extra: &[Option<Handle>],
        spec: &ProcessSpec,
    ) -> Result<u32> {
        let mut existing = [None; 3];
        for (i, option) in spec.stdio.iter().enumerate() {
            if let ProcessStdio::Handle(h) = option {
                existing[i] = Some(self.get(*h)?.transport.native.raw());
            }
        }
        let mut sources = Vec::with_capacity(spec.extra.len());
        for fd in &spec.extra {
            sources.push(match fd.source {
                crate::ChildFdSource::Handle(h) => Some(self.get(h)?.transport.native.raw()),
                _ => None,
            });
        }
        let (child, parents, extra_parents) = process::spawn(
            spec,
            existing,
            &sources,
            self.notifier.clone().ok_or_else(invalid)?,
        )?;
        let pid = child.pid;
        let result = (|| {
            for (h, parent) in pipes.into_iter().zip(parents) {
                if let (Some(h), Some(parent)) = (h, parent) {
                    self.install(h, parent, None, None)?;
                }
            }
            for (h, parent) in extra.iter().zip(extra_parents) {
                if let (Some(h), Some(parent)) = (h, parent) {
                    self.install(*h, parent, None, None)?;
                }
            }
            child.resume()
        })();
        if let Err(error) = result {
            for h in pipes
                .into_iter()
                .flatten()
                .chain(extra.iter().flatten().copied())
            {
                self.release(h);
            }
            return Err(error);
        }
        self.services.child(handle, child);
        Ok(pid)
    }
    fn prepare_close(&mut self, handle: Handle) -> Result<()> {
        self.services.close(handle)
    }
    fn kill(&mut self, handle: Handle, signal: Signal, group: bool) -> Result<()> {
        self.services.kill(handle, signal, group)
    }
    fn signal(&mut self, handle: Handle, signal: Signal) -> Result<()> {
        self.services
            .signal(handle, signal, self.notifier.clone().ok_or_else(invalid)?)
    }
    fn tty_set_mode(&mut self, handle: Handle, mode: TtyMode) -> Result<()> {
        use windows_sys::Win32::System::Console::*;
        let r = self.get(handle)?;
        let original = r.transport.mode.ok_or_else(unsupported)?;
        let raw = r.transport.native.raw();
        let flags = match mode {
            TtyMode::Normal => original,
            TtyMode::Raw | TtyMode::Io if r.transport.console_input => {
                let mut flags = (original
                    | ENABLE_WINDOW_INPUT
                    | ENABLE_VIRTUAL_TERMINAL_INPUT
                    | ENABLE_EXTENDED_FLAGS)
                    & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_QUICK_EDIT_MODE);
                if mode == TtyMode::Io {
                    flags &= !ENABLE_PROCESSED_INPUT;
                }
                flags
            }
            TtyMode::Raw | TtyMode::Io => {
                original | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING
            }
        };
        // SAFETY: mode was captured from this live console handle.
        bool_result(unsafe { SetConsoleMode(raw, flags) })
    }
    fn tty_window_size(&self, handle: Handle) -> Result<WindowSize> {
        use windows_sys::Win32::System::Console::*;
        let r = self.get(handle)?;
        if r.transport.mode.is_none() {
            return Err(unsupported());
        }
        // SAFETY: plain writable console screen-buffer information.
        let mut info = unsafe { std::mem::zeroed() };
        // SAFETY: live owned console screen-buffer handle and correct output.
        let output = if r.transport.console_input {
            use windows_sys::Win32::Storage::FileSystem::*;
            let name = [67u16, 79, 78, 79, 85, 84, 36, 0]; // CONOUT$
            // SAFETY: explicit console device, exclusively owned temporary query handle.
            Some(unsafe {
                port::owned(CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                ))
            }?)
        } else {
            None
        };
        let raw = output
            .as_ref()
            .map_or(r.transport.native.raw(), AsRawHandle::as_raw_handle);
        // SAFETY: owned console screen-buffer handle and correct writable output.
        bool_result(unsafe { GetConsoleScreenBufferInfo(raw, &mut info) })?;
        Ok(WindowSize {
            columns: (info.srWindow.Right - info.srWindow.Left + 1) as u16,
            rows: (info.srWindow.Bottom - info.srWindow.Top + 1) as u16,
        })
    }
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn validate_timeout(&self, timeout: Timeout) -> Result<()> {
        if let Some(event) = &self.event {
            // Check before the core can deliver already-queued timers/posts.
            event.check()?;
        }
        if self.event.is_some() && !matches!(timeout, Timeout::Now) {
            return Err(invalid());
        }
        Ok(())
    }
    fn deadline_changed(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
        if self.event.is_some()
            && let Err(error) = self.arm_event_deadline()
        {
            self.failure = Some(error);
        }
    }
    fn waker(&self) -> Arc<Self::Wake> {
        Arc::clone(&self.wake)
    }
    fn open(&mut self, h: Handle, spec: Open) -> Result<()> {
        let (transport, connect, pipe_connect) = match spec {
            Open::Tcp { addr, opts } => {
                let socket = socket::create(addr.is_ipv6(), false)?;
                let raw = socket.as_raw_socket() as usize;
                socket::bind_to(
                    raw,
                    if addr.is_ipv6() {
                        ([0u16; 8], 0).into()
                    } else {
                        ([0u8; 4], 0).into()
                    },
                )?;
                if addr.ip().is_loopback() {
                    socket::loopback_connect(raw);
                }
                if opts.nodelay {
                    let value = 1i32;
                    // SAFETY: TCP_NODELAY takes an initialized integer value.
                    socket::check(unsafe {
                        setsockopt(
                            raw,
                            IPPROTO_TCP,
                            TCP_NODELAY,
                            ptr::from_ref(&value).cast(),
                            4,
                        )
                    })?;
                }
                (
                    Detached::new(Native::Socket(socket), Kind::Tcp, false),
                    Some(addr),
                    None,
                )
            }
            Open::Listener { addr, opts } => {
                if opts.reuse_port.is_enabled() {
                    return Err(unsupported());
                }
                sockopt::validate_accept_defaults(opts.accept_defaults, Kind::Listener)?;
                let socket = socket::create(addr.is_ipv6(), false)?;
                let raw = socket.as_raw_socket() as usize;
                socket::option(raw, SO_EXCLUSIVEADDRUSE, 1)?;
                socket::bind_to(raw, addr)?;
                // SAFETY: bound socket; backlog is bounded to the signed API range.
                socket::check(unsafe { listen(raw, opts.backlog.min(i32::MAX as u32) as i32) })?;
                let mut transport = Detached::new(Native::Socket(socket), Kind::Listener, false);
                transport.accept_defaults = opts.accept_defaults;
                (transport, None, None)
            }
            Open::Udp { addr, opts } => {
                if opts.reuse_port.is_enabled() {
                    return Err(unsupported());
                }
                let socket = socket::create(addr.is_ipv6(), true)?;
                socket::bind_to(socket.as_raw_socket() as usize, addr)?;
                (
                    Detached::new(Native::Socket(socket), Kind::Udp, false),
                    None,
                    None,
                )
            }
            Open::PipeListener { name, opts } => {
                if opts.reuse_port.is_enabled() {
                    return Err(unsupported());
                }
                sockopt::validate_accept_defaults(opts.accept_defaults, Kind::PipeListener)?;
                let name = pipes::name(&name)?;
                let key = self.next_listener_key;
                self.next_listener_key = key
                    .checked_add(1)
                    .filter(|key| *key < port::STOP)
                    .ok_or(Error::new(ErrorKind::ResourceLimit))?;
                let listener =
                    pipes::Listener::new(name, opts.backlog, Arc::clone(&self.port), key)?;
                self.install(
                    h,
                    Detached::new(Native::PipeListener, Kind::PipeListener, false),
                    None,
                    None,
                )?;
                self.resources[h.index()]
                    .as_mut()
                    .expect("listener")
                    .listener = Some(listener);
                return Ok(());
            }
            Open::Pipe(name) => {
                let (transport, state) = pipes::open(pipes::name(&name)?)?;
                (transport, None, state)
            }
            Open::Stdio(which) => {
                let i = match which {
                    Stdio::Stdin => 0,
                    Stdio::Stdout => 1,
                    Stdio::Stderr => 2,
                };
                (
                    Detached::from_handle(process::stdio(i, false)?)?,
                    None,
                    None,
                )
            }
        };
        self.install(h, transport, connect, pipe_connect)
    }
    fn local_addr(&self, h: Handle) -> Result<SocketAddr> {
        let r = self.get(h)?;
        if !matches!(r.transport.native, Native::Socket(_)) {
            return Err(unsupported());
        }
        socket::address(r.transport.native.raw() as usize, false)
    }
    fn set_option(&mut self, h: Handle, option: SocketOption) -> Result<()> {
        if matches!(option, SocketOption::ListenBacklog(_))
            && self.get(h)?.transport.kind != Kind::Listener
        {
            return Err(invalid());
        }
        sockopt::set(self.socket_of(h)?, option)
    }
    fn open_bound_tcp(
        &mut self,
        h: Handle,
        transport: Detached,
        peer: SocketAddr,
        opts: TcpOpts,
    ) -> Result<()> {
        if transport.kind != Kind::Tcp {
            return Err(invalid());
        }
        let Native::Socket(socket) = &transport.native else {
            return Err(invalid());
        };
        let raw = socket.as_raw_socket() as usize;
        let local = socket::address(raw, false)?;
        if local.port() == 0 || local.is_ipv4() != peer.is_ipv4() {
            return Err(invalid());
        }
        match socket::address(raw, true) {
            Ok(_) => return Err(invalid()),
            Err(error) if error.os == Some(WSAENOTCONN) => {}
            Err(error) => return Err(error),
        }
        if opts.nodelay {
            sockopt::set(raw, SocketOption::NoDelay(true))?;
        }
        if peer.ip().is_loopback() {
            socket::loopback_connect(raw);
        }
        self.install(h, transport, Some(peer), None)
    }
    fn get_option(&self, h: Handle, kind: SocketOptionKind) -> Result<SocketOption> {
        sockopt::get(self.socket_of(h)?, kind)
    }
    fn shutdown_receive(&mut self, h: Handle) -> Result<()> {
        let transport = &self.get(h)?.transport;
        if transport.kind != Kind::Tcp {
            return Err(unsupported());
        }
        // SAFETY: live owned TCP socket. SD_RECEIVE preserves sends and does
        // not release OVERLAPPED storage; outstanding receives still complete.
        socket::check(unsafe { shutdown(transport.native.raw() as usize, SD_RECEIVE) })
    }
    fn submit(&mut self, request: Request) -> Result<()> {
        if self.services.contains(request.handle) {
            return self.services.submit(request);
        }
        if self.watches.contains(request.handle) {
            return self.watches.submit(&request);
        }
        let r = self.get(request.handle)?;
        let kind = r.transport.kind;
        let valid = match &request.operation {
            Operation::Connect => matches!(kind, Kind::Tcp | Kind::Pipe | Kind::PipeConnecting),
            Operation::Accept { .. } => matches!(kind, Kind::Listener | Kind::PipeListener),
            Operation::Read { .. } | Operation::Write(_) | Operation::Writev(_) => {
                matches!(kind, Kind::Tcp | Kind::Pipe | Kind::Sync)
            }
            Operation::SendTo { .. } | Operation::RecvFrom(_) => kind == Kind::Udp,
            Operation::Shutdown => kind == Kind::Tcp,
            Operation::SendHandle(_) | Operation::RecvHandle => kind == Kind::Pipe,
            _ => false,
        };
        if !valid {
            return Err(unsupported());
        }
        let passed = if let Operation::SendHandle(source) = request.operation {
            let Native::Socket(socket) = &self.get(source)?.transport.native else {
                return Err(unsupported());
            };
            Some(socket::duplicate(socket)?)
        } else {
            None
        };
        let i = request.op.index();
        if self.ops.get(i).is_none_or(Option::is_some) {
            return Err(invalid());
        }
        let d = direction(&request.operation);
        let r = self.resources[request.handle.index()]
            .as_mut()
            .expect("resource");
        let previous = r.tails[d];
        r.tails[d] = Some(i);
        if previous.is_none() {
            r.heads[d] = Some(i);
        }
        if let Some(previous) = previous {
            self.ops[previous].as_mut().expect("tail").next = Some(i);
        }
        self.ops[i] = Some(Pending {
            request,
            next: None,
            stage: Stage::Start,
            waiting: false,
            queued: false,
            pool_wait: false,
            listener_wait: false,
            throttled: false,
            cancelled: false,
            completion: None,
            offset: 0,
            lease: None,
            accepted: None,
            passed,
            direction: d,
        });
        if previous.is_none() {
            self.schedule(i);
        }
        Ok(())
    }
    fn cancel(&mut self, op: OpId) -> Result<()> {
        if self.services.cancel(op) || self.watches.cancel(op)? {
            return Ok(());
        }
        let Some(p) = self
            .ops
            .get(op.index())
            .and_then(Option::as_ref)
            .filter(|p| p.request.op == op)
        else {
            return Ok(());
        };
        if p.waiting {
            if let Some(workers) = &self.workers[p.request.handle.index()] {
                workers[p.direction].cancel();
            } else {
                // SAFETY: exact live native request; acknowledgement is still required.
                if unsafe { CancelIoEx(self.io_raw(p), self.kernel_ptr(op.index())) } == 0 {
                    let error = os_error();
                    if error.os != Some(ERROR_NOT_FOUND as i32) {
                        return Err(error);
                    }
                }
            }
        }
        self.ops[op.index()].as_mut().expect("op").cancelled = true;
        self.ops[op.index()].as_mut().expect("op").listener_wait = false;
        if self.ops[op.index()].as_ref().expect("op").pool_wait {
            self.pool_waiting.retain(|queued| *queued != op);
            self.ops[op.index()].as_mut().expect("op").pool_wait = false;
        }
        self.schedule(op.index());
        Ok(())
    }
    fn has_work(&self) -> bool {
        self.failure.is_some()
            || !self.ready.is_empty()
            || self.services.has_work()
            || self.watches.has_work()
            || (self.pool.available() && !self.pool_waiting.is_empty())
    }
    fn poll(
        &mut self,
        timeout: Option<Duration>,
        budget: Budget,
        events: &mut Vec<Event<Detached>>,
    ) -> Result<PollInfo> {
        self.accepts = budget.accepts;
        if self.accepts != 0 {
            while let Some(i) = self.throttled.pop_front() {
                if let Some(p) = self.ops[i].as_mut().filter(|p| p.throttled) {
                    p.throttled = false;
                    self.schedule(i);
                }
            }
        }
        if let Some(event) = &self.event {
            event.check()?;
        }
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        let before = events.len();
        self.services.collect(events)?;
        self.watches.collect(events);
        self.run_ready(events);
        if events.len() != before || events.len() == events.capacity() || !self.ready.is_empty() {
            return Ok(PollInfo::default());
        }
        let mut entries = [Entry::default(); 64];
        let mut cancel_result = Ok(());
        let (n, info) = if let Some(event) = &mut self.event {
            // The helper owns the blocking port wait on its own thread.
            // This turn only drains its queue: no native discovery call.
            (event.drain(&mut entries)?, PollInfo::default())
        } else {
            let armed = timeout.is_some_and(|d| !d.is_zero());
            if let Some(delay) = timeout.filter(|d| !d.is_zero())
                && !self.timer_pending
            {
                self.timer.arm(delay)?;
            }
            // GQCSEx's millisecond timeout may expire at an earlier system tick.
            // Once armed, the exact NT timer is the only deadline wake source.
            let result = self
                .port
                .wait(if armed { None } else { timeout }, false, &mut entries);
            if armed {
                cancel_result = self.cancel_timer().map(|_| ());
            }
            match result? {
                // The private deadline timer packet has the meaning of a timed wait
                // returning zero (see `PollInfo` in backend/mod.rs, and epoll's timerfd):
                // it is not native work. Notifier and I/O packets still count.
                Wait::Entries(n) => {
                    let timer_only = entries[..n].iter().all(|entry| entry.key == TIMER);
                    (n, PollInfo::native(timeout, timer_only))
                }
                Wait::Timeout | Wait::Apc => (0, PollInfo::native(timeout, true)),
            }
        };
        let entries_result = self.entries(&entries[..n]);
        cancel_result?;
        entries_result?;
        self.services.collect(events)?;
        self.watches.collect(events);
        self.run_ready(events);
        Ok(info)
    }
    fn release(&mut self, h: Handle) {
        self.services.release(h);
        self.watches.release(h);
        if self.get(h).is_ok() {
            self.workers[h.index()] = None;
            self.resources[h.index()] = None;
        }
    }
    fn raw_transport(&self, h: Handle) -> Result<crate::RawTransport> {
        if self.services.contains(h) || self.watches.contains(h) {
            return Err(unsupported());
        }
        self.get(h)?.transport.raw_transport()
    }
    fn detach(&mut self, h: Handle) -> Result<Detached> {
        let r = self.get(h)?;
        if matches!(r.transport.kind, Kind::PipeListener | Kind::PipeConnecting) {
            return Err(unsupported());
        }
        if r.heads.iter().any(Option::is_some) {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        let mut r = self.resources[h.index()].take().expect("resource");
        self.workers[h.index()] = None;
        r.transport.routed = true;
        Ok(r.transport)
    }
    fn attach(&mut self, h: Handle, transport: Detached) -> Result<()> {
        self.install(h, transport, None, None)
    }
    fn integration(&mut self) -> Result<Integration> {
        if let Some(event) = &self.event {
            event.check()?;
        }
        if self.event.is_none() {
            self.event = Some(EventIntegration::new(Arc::clone(&self.port))?);
            self.arm_event_deadline()?;
        }
        if self.has_work() {
            self.port.post(WAKE, 0)?;
        }
        Ok(Integration::Event(
            self.event.as_ref().expect("event").event() as usize,
        ))
    }
}
impl Iocp {
    fn cancel_timer(&mut self) -> Result<bool> {
        if !self.timer_pending {
            self.timer_pending = self.timer.cancel()? == 0x103;
        }
        // STATUS_PENDING retains ownership until entry() sees this generation.
        Ok(!self.timer_pending)
    }
    fn arm_event_deadline(&mut self) -> Result<()> {
        if !self.cancel_timer()? {
            return Ok(());
        } // drain the in-flight packet before rearming
        if let Some(deadline) = self.deadline {
            let now = Instant::now();
            if deadline <= now {
                self.port.post(WAKE, 0)?;
            } else {
                self.timer.arm(deadline.saturating_duration_since(now))?;
            }
        }
        Ok(())
    }
}
impl Drop for Iocp {
    fn drop(&mut self) {
        // Join the sole consumer and reclaim every forwarded packet, then use
        // blocking port waits for cancellation. Never repeatedly poll an empty
        // helper queue (including after its worker has failed).
        if let Some(mut event) = self.event.take() {
            if event.shutdown().is_err() {
                std::process::abort();
            }
            let mut entries = [Entry::default(); 64];
            loop {
                let Ok(n) = event.drain_retained(&mut entries) else {
                    std::process::abort();
                };
                if n == 0 {
                    break;
                }
                if self.entries(&entries[..n]).is_err() {
                    std::process::abort();
                }
            }
        }
        self.failure = None; // teardown no longer arms host deadlines
        for i in 0..self.ops.materialised() {
            if let Some(p) = &self.ops[i] {
                let op = p.request.op;
                if self.cancel(op).is_err() {
                    std::process::abort();
                }
            }
        }
        self.watches.shutdown();
        let mut events = Vec::with_capacity(64);
        while self.ops.iter().any(Option::is_some) || self.watches.pending() {
            if self.poll(None, Budget::UNLIMITED, &mut events).is_err() {
                std::process::abort();
            }
            events.clear();
        }
    }
}
fn read_buffer(p: &mut Pending, pool: &BufferPool) -> Option<(*mut u8, u32)> {
    let buf = match &p.request.operation {
        Operation::Read { buf, .. } | Operation::RecvFrom(buf) => buf,
        _ => unreachable!(),
    };
    match buf {
        ReadBuf::Provided(buf) => Some((buf.as_mut_ptr(), buf.len().min(u32::MAX as usize) as u32)),
        ReadBuf::Pooled => {
            if p.lease.is_none() {
                p.lease = Some(pool.acquire()?);
            }
            let bytes = p.lease.as_mut().expect("lease").writable();
            Some((
                bytes.as_mut_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
            ))
        }
    }
}
fn write_length(op: &Operation) -> usize {
    match op {
        Operation::Write(buf) | Operation::SendTo { buf, .. } => buf.as_slice().len(),
        Operation::Writev(bufs) => bufs.bufs.iter().flatten().map(|b| b.as_slice().len()).sum(),
        _ => 0,
    }
}
fn write_buffers(op: &Operation, mut offset: usize, out: &mut [WSABUF; MAX_IOV]) -> usize {
    let mut n = 0;
    let mut add = |buf: &WriteBuf| {
        let bytes = buf.as_slice();
        if offset >= bytes.len() {
            offset -= bytes.len();
            return;
        }
        let bytes = &bytes[offset..];
        offset = 0;
        out[n] = WSABUF {
            len: bytes.len().min(u32::MAX as usize) as u32,
            buf: bytes.as_ptr().cast_mut(),
        };
        n += 1;
    };
    match op {
        Operation::Write(buf) | Operation::SendTo { buf, .. } => add(buf),
        Operation::Writev(bufs) => {
            for buf in bufs.bufs.iter().flatten() {
                add(buf);
            }
        }
        _ => {}
    }
    n
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn direct_discovery_and_event_queue_draining_have_distinct_counts() {
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
        };
        let mut backend = Iocp::new(&Config::default(), BufferPool::new(2, 64)).expect("backend");
        let mut events = Vec::with_capacity(1);
        for wake in [false, true] {
            if wake {
                backend.port.post(WAKE, 0).expect("wake");
            }
            let info = backend
                .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
                .expect("discovery");
            assert_eq!(
                (info.waits, info.discovery_polls, info.zero_event_waits),
                (0, 1, u32::from(!wake))
            );
            assert!(events.is_empty());
        }
        let Integration::Event(event) = backend.integration().expect("event helper") else {
            panic!("Event required");
        };
        backend.port.post(WAKE, 0).expect("helper packet");
        // SAFETY: backend owns the event throughout this bounded external wait.
        let result = unsafe { WaitForSingleObject(event as _, 2000) };
        assert_eq!(result, WAIT_OBJECT_0, "helper must forward a real packet");
        let info = backend
            .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
            .expect("helper drain");
        assert_eq!(
            (info.waits, info.discovery_polls, info.zero_event_waits),
            (0, 0, 0)
        );
        assert!(events.is_empty());
    }

    // These operations model already-dequeued kernel completions. No native I/O
    // is submitted, so unwind must discard their metadata rather than wait on it.
    pub(super) struct Synthetic {
        pub backend: Iocp,
        pub entries: [Entry; 2],
        pub ops: [OpId; 2],
    }
    impl Synthetic {
        pub fn new() -> Self {
            let config = Config {
                max_handles: 2,
                max_operations: 2,
                ..Config::default()
            };
            let mut backend = Iocp::new(&config, BufferPool::new(2, 64)).expect("backend");
            let handle = Handle { owner: 1, key: 0 };
            backend
                .open(
                    handle,
                    Open::Tcp {
                        addr: ([127, 0, 0, 1], 9).into(),
                        opts: TcpOpts::default(),
                    },
                )
                .expect("socket resource");
            let ops = [OpId { owner: 1, key: 0 }, OpId { owner: 1, key: 1 }];
            let entries = std::array::from_fn(|i| {
                backend
                    .submit(Request {
                        op: ops[i],
                        handle,
                        operation: Operation::Write(WriteBuf::Owned(vec![0; i + 1])),
                    })
                    .expect("synthetic request");
                let pending = backend.ops[i].as_mut().expect("op");
                pending.waiting = true;
                pending.queued = false;
                pending.stage = Stage::Io;
                Entry {
                    key: 1,
                    overlapped: backend.kernel_ptr(i) as usize,
                    bytes: (i + 1) as u32,
                    status: 0,
                }
            });
            backend.ready.clear();
            Self {
                backend,
                entries,
                ops,
            }
        }
        pub fn assert_delivered(&mut self) {
            let mut out = Vec::with_capacity(1);
            for (i, op) in self.ops.into_iter().enumerate() {
                let info = self
                    .backend
                    .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut out)
                    .expect("drain valid packet");
                assert_eq!((info.waits, info.discovery_polls), (0, 0));
                assert_eq!(out.len(), 1);
                assert_eq!(out[0].op, op);
                assert!(out[0].terminal);
                assert!(matches!(out[0].result, Ok(Outcome::Wrote(n)) if n == i + 1));
                out.clear();
            }
            assert!(self.backend.ops.iter().all(Option::is_none));
            assert!(!self.backend.has_work());
        }
    }
    impl Drop for Synthetic {
        fn drop(&mut self) {
            self.backend.ops.iter_mut().for_each(|op| *op = None);
            self.backend.ready.clear();
        }
    }
    #[test]
    fn invalid_entry_does_not_discard_later_completions() {
        let mut fixture = Synthetic::new();
        let batch = [fixture.entries[0], Entry::default(), fixture.entries[1]];
        assert_eq!(
            fixture
                .backend
                .entries(&batch)
                .expect_err("invalid packet")
                .kind,
            ErrorKind::InvalidInput
        );
        fixture.assert_delivered();
    }
}
