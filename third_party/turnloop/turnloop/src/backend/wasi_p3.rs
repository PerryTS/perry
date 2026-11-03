//! Experimental direct WASI 0.3 component async backend. A persistent wait-set
//! and fixed request return areas avoid a fresh block_on or executor per turn.
//! Experimental: host-yield boundedness is unproven. Allocation gates require
//! release on the pinned p3 compiler; see docs/upstream/wasi-p3-wait.md.
use crate::slots::{Slots, page_reserve};
mod abi;
mod fs;
mod return_storage;
mod sockopt;
mod wait_set;
use crate::{
    backend::{Backend, Budget, Event, Filesystem, Operation, Outcome, PollInfo, Request, Wake},
    *,
};
use std::{
    collections::VecDeque,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use wait_set::WaitSet;
use wasip3::{
    clocks::monotonic_clock as clock,
    sockets::types::{
        ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress,
        TcpSocket, UdpSocket,
    },
    wit_future::FuturePayload,
    wit_stream::StreamPayload,
};
type SocketResult = std::result::Result<(), ErrorCode>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Tcp,
    Listener,
    Udp,
    Stdio(Stdio),
}
#[derive(Debug)]
enum Socket {
    Tcp(TcpSocket),
    Udp(UdpSocket),
    Stdio,
}
#[derive(Debug)]
struct ResultFuture(u32, bool);
type CliResult = std::result::Result<(), wasip3::cli::types::ErrorCode>;
impl ResultFuture {
    unsafe fn read(&self, area: *mut u8) -> u32 {
        // SAFETY: caller pins a correctly aligned result area until acknowledgement.
        unsafe {
            if self.1 {
                (CliResult::VTABLE.start_read)(self.0, area)
            } else {
                (SocketResult::VTABLE.start_read)(self.0, area)
            }
        }
    }
}
impl Drop for ResultFuture {
    fn drop(&mut self) {
        // SAFETY: owned future is dropped only after any raw read has been cancelled.
        unsafe {
            if self.1 {
                (CliResult::VTABLE.drop_readable)(self.0);
            } else {
                (SocketResult::VTABLE.drop_readable)(self.0);
            }
        }
    }
}
#[derive(Debug)]
struct Streams {
    reader: Option<wasip3::wit_bindgen::rt::async_support::StreamReader<u8>>,
    writer: Option<wasip3::wit_bindgen::rt::async_support::StreamWriter<u8>>,
    read_done: Option<ResultFuture>,
    write_done: Option<ResultFuture>,
    read_closed: bool,
    read_result: Option<Result<()>>,
    write_result: Option<Result<()>>,
}
/// Owning accepted transport, attached only on its original WASI agent.
#[derive(Debug)]
pub struct Detached {
    streams: Option<Streams>,
    incoming: Option<wasip3::wit_bindgen::rt::async_support::StreamReader<TcpSocket>>,
    socket: Socket,
    kind: Kind,
    /// A listener's per-connection defaults, applied to each socket it accepts.
    accept_defaults: AcceptDefaults,
}
struct Resource {
    _udp_return: Option<return_storage::Reservation>,
    handle: Handle,
    transport: Detached,
    connect: Option<SocketAddr>,
    ready: [bool; 2],
    heads: [Option<usize>; 2],
    tails: [Option<usize>; 2],
    queued: bool,
    /// Listed in `throttled`: a ready multishot accept held back by the budget.
    throttled: bool,
}
#[derive(Clone, Copy)]
enum WaitKind {
    Connect,
    Accept,
    Read,
    Write,
    ReadDone,
    WriteDone,
    Send,
    Receive,
}
struct Pending {
    request: Request,
    next: Option<usize>,
    offset: usize,
    wait: Option<(u32, WaitKind)>,
    code: Option<u32>,
    area: [u32; 32],
    lease: Option<BufLease>,
}
/// Same-agent notification endpoint; WASI has no OS threads.
#[derive(Default)]
pub struct WasiWake;
impl Wake for WasiWake {
    fn wake(&self) -> Result<()> {
        Ok(())
    }
    fn syscall_count(&self) -> u64 {
        0
    }
}
/// Experimental WASI 0.3 driver with a persistent waitable set.
pub struct WasiP3 {
    resources: Slots<Resource>,
    ops: Slots<Pending>,
    ready: VecDeque<Handle>,
    /// Listeners whose multishot accept is ready but out of budget: neither
    /// runnable nor subscribed, until a poll with a positive budget (#77).
    throttled: VecDeque<Handle>,
    /// Connections multishot accepts may still deliver in the current poll.
    accepts: usize,
    cancelled: VecDeque<OpId>,
    pool: BufferPool,
    wake: Arc<WasiWake>,
    wait_set: WaitSet,
    returns: Rc<return_storage::Arena>,
    files: super::wasi_fs::Files<fs::P3>,
}
fn direction(op: &Operation) -> usize {
    usize::from(!matches!(
        op,
        Operation::Accept { .. } | Operation::Read { .. } | Operation::RecvFrom(_)
    ))
}
fn address(a: SocketAddr) -> IpSocketAddress {
    match a {
        SocketAddr::V4(a) => {
            let b = a.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: a.port(),
                address: (b[0], b[1], b[2], b[3]),
            })
        }
        SocketAddr::V6(a) => {
            let b = a.ip().segments();
            IpSocketAddress::Ipv6(Ipv6SocketAddress {
                port: a.port(),
                flow_info: a.flowinfo(),
                scope_id: a.scope_id(),
                address: (b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]),
            })
        }
    }
}
fn native(a: IpSocketAddress) -> SocketAddr {
    match a {
        IpSocketAddress::Ipv4(a) => (
            Ipv4Addr::new(a.address.0, a.address.1, a.address.2, a.address.3),
            a.port,
        )
            .into(),
        IpSocketAddress::Ipv6(a) => {
            let b = a.address;
            SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::new(b.0, b.1, b.2, b.3, b.4, b.5, b.6, b.7),
                a.port,
                a.flow_info,
                a.scope_id,
            ))
        }
    }
}

fn error(e: ErrorCode) -> Error {
    Error::new(match e {
        ErrorCode::NotSupported => ErrorKind::Unsupported,
        ErrorCode::InvalidArgument | ErrorCode::InvalidState => ErrorKind::InvalidInput,
        ErrorCode::ConnectionRefused => ErrorKind::ConnectionRefused,
        ErrorCode::ConnectionReset | ErrorCode::ConnectionAborted => ErrorKind::ConnectionReset,
        ErrorCode::ConnectionBroken => ErrorKind::BrokenPipe,
        ErrorCode::Timeout => ErrorKind::TimedOut,
        ErrorCode::OutOfMemory => ErrorKind::ResourceLimit,
        _ => ErrorKind::Other,
    })
}
fn streams(socket: &TcpSocket) -> Streams {
    let (reader, read_done) = socket.receive();
    let (writer, send) = wasip3::wit_stream::new::<u8>();
    let write_done = socket.send(send);
    Streams {
        reader: Some(reader),
        writer: Some(writer),
        read_done: Some(ResultFuture(read_done.take_handle(), false)),
        write_done: Some(ResultFuture(write_done.take_handle(), false)),
        read_closed: false,
        read_result: None,
        write_result: None,
    }
}
impl WasiP3 {
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
        transport: Detached,
        connect: Option<SocketAddr>,
    ) -> Result<()> {
        if self.resources.get(h.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let udp_return = (transport.kind == Kind::Udp).then(|| self.returns.reserve());
        self.resources[h.index()] = Some(Resource {
            _udp_return: udp_return,
            handle: h,
            transport,
            connect,
            ready: [true; 2],
            heads: [None; 2],
            tails: [None; 2],
            queued: false,
            throttled: false,
        });
        Ok(())
    }
    fn schedule(&mut self, h: Handle) {
        let Some(r) = self.resources.get_mut(h.index()).and_then(Option::as_mut) else {
            return;
        };
        if r.handle != h || r.queued {
            return;
        }
        let held = self.accepts == 0 && r.ready[0] && holds_throttled(&self.ops, r.heads[0]);
        if (0..2).any(|d| r.ready[d] && r.heads[d].is_some() && !(d == 0 && held)) {
            r.queued = true;
            self.ready.push_back(h);
        } else if held && !r.throttled {
            r.throttled = true;
            self.throttled.push_back(h);
            // A parked accept whose read already fired keeps its outcome in
            // `code`, which `execute` consumes once the budget resumes it; only
            // `finish_wait` would otherwise take the waitable out of the set.
            // Leave it there and every wait while the ceiling holds could return
            // it again at once, a spin that produces nothing (rule 4a). Removing
            // it runs no side effect and loses no connection; `finish_wait`
            // removes it again harmlessly, since `join(_, 0)` is idempotent.
            if let Some(p) = r.heads[0].and_then(|i| self.ops[i].as_ref())
                && let Some((waitable, _)) = p.wait
                && p.code.is_some()
                && waitable != 0
            {
                self.wait_set.remove(waitable);
            }
        }
    }
    /// Start a poll with `budget`: a positive one resumes every held listener.
    fn begin(&mut self, budget: Budget) {
        self.accepts = budget.accepts;
        if self.accepts == 0 {
            return;
        }
        while let Some(h) = self.throttled.pop_front() {
            if let Some(r) = self
                .resources
                .get_mut(h.index())
                .and_then(Option::as_mut)
                .filter(|r| r.handle == h)
            {
                r.throttled = false;
                self.schedule(h);
            }
        }
    }
    fn unlink(&mut self, h: Handle, i: usize, d: usize) {
        let r = self.resources[h.index()]
            .as_mut()
            .expect("registered resource");
        let next = self.ops[i].as_ref().expect("pending op").next;
        if r.heads[d] == Some(i) {
            r.heads[d] = next;
        } else {
            let mut at = r.heads[d];
            while let Some(p) = at {
                let previous = self.ops[p].as_mut().expect("queued op");
                if previous.next == Some(i) {
                    previous.next = next;
                    if r.tails[d] == Some(i) {
                        r.tails[d] = Some(p);
                    }
                    break;
                }
                at = previous.next;
            }
        }
        if r.tails[d] == Some(i) {
            r.tails[d] = None;
        }
    }
    fn run_ready(&mut self, events: &mut Vec<Event<Detached>>) {
        let budget = events.capacity().saturating_sub(events.len());
        for _ in 0..budget {
            if events.len() == events.capacity() {
                break;
            }
            let Some(h) = self.ready.pop_front() else {
                break;
            };
            let Some(r) = self.resources.get_mut(h.index()).and_then(Option::as_mut) else {
                continue;
            };
            if r.handle != h {
                continue;
            }
            r.queued = false;
            for d in 0..2 {
                if events.len() == events.capacity() {
                    break;
                }
                let r = self.resources[h.index()].as_mut().expect("resource");
                if !r.ready[d] {
                    continue;
                }
                let Some(i) = r.heads[d] else {
                    continue;
                };
                if self.accepts == 0 && holds_throttled(&self.ops, Some(i)) {
                    // Out of budget: `schedule` below parks the listener.
                    continue;
                }
                let p = self.ops[i].as_mut().expect("queued op");
                let result = execute(r, p, &self.pool, &self.wait_set);
                let event = match result {
                    Ok(Some((result, terminal))) => Some(Event {
                        op: p.request.op,
                        result: Ok(result),
                        terminal,
                    }),
                    Ok(None) => {
                        if p.wait.is_some() && p.code.is_none() {
                            r.ready[d] = false;
                        }
                        None
                    }
                    Err(e) if e.kind == ErrorKind::WouldBlock => {
                        r.ready[d] = false;
                        None
                    }
                    Err(e) => Some(Event {
                        op: p.request.op,
                        result: Err(e),
                        terminal: true,
                    }),
                };
                if let Some(e) = event {
                    if Budget::spends(&e) {
                        self.accepts -= 1;
                    }
                    if e.terminal {
                        self.unlink(h, i, d);
                        self.ops[i] = None;
                    }
                    events.push(e);
                }
            }
            self.schedule(h);
        }
    }
}

// SAFETY: fixed Vec storage pins every canonical return area until its native
// acknowledgement. Cancellation removes wait-set membership, synchronously
// cancels buffer access, then retires. Drop quiesces operations before resources.
unsafe impl Backend for WasiP3 {
    type Wake = WasiWake;
    type Detached = Detached;
    const FILESYSTEM: Filesystem = Filesystem::Backend;
    fn fs(&mut self, op: OpId, handle: Option<Handle>, request: FsRequest) -> Result<()> {
        self.files.submit(op, handle, request)
    }
    fn new(config: &Config, pool: BufferPool) -> Result<Self> {
        Ok(Self {
            resources: Slots::new(config.max_handles),
            ops: Slots::new(config.max_operations),
            ready: VecDeque::with_capacity(page_reserve(config.max_handles)),
            throttled: VecDeque::with_capacity(page_reserve(config.max_handles)),
            accepts: usize::MAX,
            cancelled: VecDeque::with_capacity(page_reserve(config.max_operations)),
            files: super::wasi_fs::Files::new(config, pool.clone()),
            pool,
            wake: Arc::new(WasiWake),
            wait_set: WaitSet::new(),
            returns: return_storage::Arena::shared(),
        })
    }
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn waker(&self) -> Arc<WasiWake> {
        self.wake.clone()
    }
    fn open(&mut self, h: Handle, spec: Open) -> Result<()> {
        let (addr, kind, reuse, backlog, accept_defaults) = match spec {
            Open::Pipe(_) | Open::PipeListener { .. } => {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            Open::Stdio(which) => {
                let (reader, read_done, writer, write_done) = if which == Stdio::Stdin {
                    let (reader, done) = wasip3::cli::stdin::read_via_stream();
                    (
                        Some(reader),
                        Some(ResultFuture(done.take_handle(), true)),
                        None,
                        None,
                    )
                } else {
                    let (writer, reader) = wasip3::wit_stream::new::<u8>();
                    let done = if which == Stdio::Stdout {
                        wasip3::cli::stdout::write_via_stream(reader)
                    } else {
                        wasip3::cli::stderr::write_via_stream(reader)
                    };
                    (
                        None,
                        None,
                        Some(writer),
                        Some(ResultFuture(done.take_handle(), true)),
                    )
                };
                return self.install(
                    h,
                    Detached {
                        streams: Some(Streams {
                            reader,
                            writer,
                            read_done,
                            write_done,
                            read_closed: false,
                            read_result: None,
                            write_result: None,
                        }),
                        incoming: None,
                        socket: Socket::Stdio,
                        kind: Kind::Stdio(which),
                        accept_defaults: AcceptDefaults::EMPTY,
                    },
                    None,
                );
            }
            Open::Tcp { addr, opts } => {
                // wasi:sockets has no Nagle control. Reporting it is the only
                // honest answer; a dropped hint is indistinguishable from an
                // applied one (see Backend::set_option).
                if opts.nodelay {
                    return Err(Error::new(ErrorKind::Unsupported));
                }
                (addr, Kind::Tcp, ReusePort::No, 0, AcceptDefaults::EMPTY)
            }
            Open::Listener { addr, opts } => (
                addr,
                Kind::Listener,
                opts.reuse_port,
                opts.backlog,
                opts.accept_defaults,
            ),
            Open::Udp { addr, opts } => {
                (addr, Kind::Udp, opts.reuse_port, 0, AcceptDefaults::EMPTY)
            }
        };
        // wasi:sockets has no address-reuse interface at all, so neither
        // Share nor Distribute can be honoured. Both are refused.
        if reuse.is_enabled() {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        sockopt::validate_accept_defaults(accept_defaults)?;
        let family = if addr.is_ipv4() {
            IpAddressFamily::Ipv4
        } else {
            IpAddressFamily::Ipv6
        };
        let transport = if kind == Kind::Udp {
            let s = UdpSocket::create(family).map_err(error)?;
            s.bind(address(addr)).map_err(error)?;
            Detached {
                streams: None,
                incoming: None,
                socket: Socket::Udp(s),
                kind,
                accept_defaults,
            }
        } else {
            let s = TcpSocket::create(family).map_err(error)?;
            let incoming = if kind == Kind::Listener {
                s.set_listen_backlog_size(u64::from(backlog))
                    .map_err(error)?;
                s.bind(address(addr)).map_err(error)?;
                Some(s.listen().map_err(error)?)
            } else {
                None
            };
            Detached {
                streams: None,
                incoming,
                socket: Socket::Tcp(s),
                kind,
                accept_defaults,
            }
        };
        self.install(h, transport, (kind == Kind::Tcp).then_some(addr))
    }
    fn set_option(&mut self, h: Handle, option: SocketOption) -> Result<()> {
        if matches!(option, SocketOption::ListenBacklog(_))
            && self.get(h)?.transport.kind != Kind::Listener
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        sockopt::set(&self.get(h)?.transport.socket, option)
    }
    fn get_option(&self, h: Handle, kind: SocketOptionKind) -> Result<SocketOption> {
        sockopt::get(&self.get(h)?.transport.socket, kind)
    }
    fn local_addr(&self, h: Handle) -> Result<SocketAddr> {
        match &self.get(h)?.transport.socket {
            Socket::Tcp(s) => s.get_local_address(),
            Socket::Udp(s) => s.get_local_address(),
            Socket::Stdio => return Err(Error::new(ErrorKind::Unsupported)),
        }
        .map(native)
        .map_err(error)
    }
    fn submit(&mut self, request: Request) -> Result<()> {
        if matches!(
            request.operation,
            Operation::ProcessExit
                | Operation::WatchSignal
                | Operation::WatchFs
                | Operation::SendHandle(_)
                | Operation::RecvHandle
        ) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        let h = request.handle;
        let r = self.get(h)?;
        let valid = match &request.operation {
            Operation::Accept { .. } => r.transport.kind == Kind::Listener,
            Operation::RecvFrom(_) | Operation::SendTo { .. } => r.transport.kind == Kind::Udp,
            Operation::Connect => r.transport.kind == Kind::Tcp && r.connect.is_some(),
            Operation::Read { .. } => {
                matches!(r.transport.kind, Kind::Tcp | Kind::Stdio(Stdio::Stdin))
            }
            Operation::Write(_) | Operation::Writev(_) | Operation::Shutdown => matches!(
                r.transport.kind,
                Kind::Tcp | Kind::Stdio(Stdio::Stdout | Stdio::Stderr)
            ),
            _ => false,
        };
        if !valid || self.ops.get(request.op.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if matches!(&request.operation, Operation::Read { buf: ReadBuf::Provided(b), .. } if b.is_empty())
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let d = direction(&request.operation);
        let i = request.op.index();
        let r = self.resources[h.index()].as_mut().expect("validated");
        if let Some(tail) = r.tails[d] {
            self.ops[tail].as_mut().expect("tail").next = Some(i);
        } else {
            r.heads[d] = Some(i);
        }
        r.tails[d] = Some(i);
        self.ops[i] = Some(Pending {
            request,
            next: None,
            offset: 0,
            wait: None,
            code: None,
            area: [0; 32],
            lease: None,
        });
        self.schedule(h);
        Ok(())
    }
    fn cancel(&mut self, op: OpId) -> Result<()> {
        if self.files.cancel(op) {
            return Ok(());
        }
        let p = self
            .ops
            .get(op.index())
            .and_then(Option::as_ref)
            .filter(|p| p.request.op == op)
            .ok_or(Error::new(ErrorKind::NotFound))?;
        let h = p.request.handle;
        let d = direction(&p.request.operation);
        quiesce(
            self.ops[op.index()].as_mut().expect("pending"),
            &self.wait_set,
            self.resources[h.index()].as_mut().expect("owner"),
        );
        let head = self.resources[h.index()].as_ref().expect("owner").heads[d] == Some(op.index());
        self.unlink(h, op.index(), d);
        if head {
            self.resources[h.index()].as_mut().expect("owner").ready[d] = true;
        }
        self.cancelled.push_back(op);
        self.schedule(h);
        Ok(())
    }
    fn has_work(&self) -> bool {
        !self.ready.is_empty() || !self.cancelled.is_empty() || self.files.has_work()
    }

    fn poll(
        &mut self,
        timeout: Option<Duration>,
        budget: Budget,
        events: &mut Vec<Event<Detached>>,
    ) -> Result<PollInfo> {
        self.begin(budget);
        while events.len() < events.capacity() {
            let Some(op) = self.cancelled.pop_front() else {
                break;
            };
            self.ops[op.index()] = None;
            events.push(Event {
                op,
                terminal: true,
                result: Ok(Outcome::Cancelled),
            });
        }
        self.files.run(events);
        self.run_ready(events);
        if self.has_work() || !events.is_empty() || events.len() == events.capacity() {
            return Ok(PollInfo::default());
        }
        let deadline = timeout.filter(|d| !d.is_zero()).map(|d| {
            let at = clock::now().saturating_add(d.as_nanos().min(u128::from(u64::MAX)) as u64);
            // SAFETY: scalar async import, no guest buffer can outlive this turn.
            let packed = unsafe { abi::deadline(at) };
            let task = packed >> 4;
            if task != 0 && packed & 15 < 2 {
                self.wait_set.join(task);
            }
            (task, packed & 15)
        });
        let blocking =
            timeout != Some(Duration::ZERO) && deadline.is_none_or(|(_, status)| status < 2);
        if deadline.is_none()
            && timeout.is_none()
            && !self.ops.iter().flatten().any(|p| p.wait.is_some())
        {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        if !blocking && self.ops.iter().flatten().any(|p| p.wait.is_some()) {
            // A nonblocking wait-set poll does not yield the component to its
            // host. One cooperative yield lets socket subtasks progress even
            // when the embedding host repeatedly calls turn(Now).
            if !abi::preserving_stack(wasip3::wit_bindgen::rt::async_support::yield_blocking) {
                return Err(Error::new(ErrorKind::Cancelled));
            }
        }
        let (kind, waitable, code) = self.wait_set.step(blocking);
        // The private deadline subtask implements this wait's timeout, so its
        // completion has the meaning of the step returning no event (see
        // `PollInfo` in backend/mod.rs, epoll's timerfd and the IOCP deadline
        // packet): it is not native work. Socket subtask events still count, and
        // a step that returns one leaves any simultaneous expiry for the timer
        // wheel, which reads the clock rather than this event.
        let deadline_event =
            kind != 0 && deadline.is_some_and(|(task, _)| task != 0 && waitable == task);
        if let Some((task, initial)) = deadline
            && task != 0
        {
            self.wait_set.remove(task);
            if initial < 2 && !(deadline_event && code >= 2) {
                // SAFETY: synchronously cancel this turn-owned deadline subtask before dropping it.
                unsafe {
                    wait_set::subtask_cancel(task);
                }
            }
            // SAFETY: deadline returned or cancellation acknowledged, and membership removed.
            unsafe {
                wait_set::subtask_drop(task);
            }
        }
        if kind != 0 && !deadline_event {
            // The materialised prefix, not the ceiling: an occupied slot is
            // always below the high-water mark, and this runs per event.
            for i in 0..self.ops.materialised() {
                let Some(p) = &mut self.ops[i] else {
                    continue;
                };
                if p.wait.is_some_and(|(w, _)| w == waitable) {
                    p.code = Some(code);
                    let h = p.request.handle;
                    let d = direction(&p.request.operation);
                    self.resources[h.index()].as_mut().expect("owner").ready[d] = true;
                    self.schedule(h);
                    break;
                }
            }
        }
        self.run_ready(events);
        Ok(PollInfo::native(
            if blocking { None } else { Some(Duration::ZERO) },
            kind == 0 || deadline_event,
        ))
    }
    fn release(&mut self, h: Handle) {
        self.files.release(h);
        if self.get(h).is_ok() {
            self.ready.retain(|&at| at != h);
            self.throttled.retain(|&at| at != h);
            self.resources[h.index()] = None;
        }
    }
    fn detach(&mut self, _: Handle) -> Result<Detached> {
        Err(Error::new(ErrorKind::Unsupported))
    }
    fn attach(&mut self, h: Handle, transport: Detached) -> Result<()> {
        self.install(h, transport, None)
    }
    fn integration(&mut self) -> Result<Integration> {
        Ok(Integration::RuntimeOwned)
    }
}
impl Drop for WasiP3 {
    fn drop(&mut self) {
        for p in self.ops.iter_mut().flatten() {
            let r = self.resources[p.request.handle.index()]
                .as_mut()
                .expect("owner");
            quiesce(p, &self.wait_set, r);
        }
    }
}
fn is_subtask(kind: WaitKind) -> bool {
    matches!(kind, WaitKind::Connect | WaitKind::Send | WaitKind::Receive)
}
fn finish_wait(p: &mut Pending, set: &WaitSet) -> Option<(WaitKind, u32)> {
    let (waitable, kind) = p.wait?;
    let code = p.code.take()?;
    if is_subtask(kind) && code < 2 {
        return None;
    }
    if waitable != 0 {
        set.remove(waitable);
    }
    p.wait = None;
    if is_subtask(kind) && waitable != 0 {
        // SAFETY: this subtask returned; its fixed return area remains live.
        unsafe {
            wait_set::subtask_drop(waitable);
        }
    }
    Some((kind, code))
}
fn start(p: &mut Pending, set: &WaitSet, kind: WaitKind, waitable: u32, code: u32) {
    p.wait = Some((waitable, kind));
    if (is_subtask(kind) && code < 2) || code == u32::MAX {
        p.code = None;
        set.join(waitable);
    } else {
        p.code = Some(code);
    }
}
fn result(p: &mut Pending) -> Result<()> {
    socket_result(&mut p.area[16..21])
}
fn stream_result(r: &Resource, p: &mut Pending) -> Result<()> {
    if matches!(r.transport.kind, Kind::Stdio(_)) {
        if p.area[16] & 255 == 0 {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::Other))
        }
    } else {
        result(p)
    }
}
fn socket_result(area: &mut [u32]) -> Result<()> {
    // Other(Some(string)) owns a canonical list. Consume it ourselves because
    // it may be a retained slot; generated lifting would free it as a String.
    if area[0] & 255 == 1 && area[1] & 255 == 14 {
        if area[2] & 255 == 1 {
            // SAFETY: completed SocketResult initialized the owned error text.
            drop(unsafe { return_storage::ReturnBytes::take(area[3], area[4] as usize) });
        }
        return Err(Error::new(ErrorKind::Other));
    }
    // SAFETY: completed SocketResult at the start of area; remaining variants own no lists.
    unsafe { (SocketResult::VTABLE.lift)(area.as_mut_ptr().cast()) }.map_err(error)
}
fn read_outcome(p: &mut Pending, n: usize) -> (Outcome<Detached>, bool) {
    if let Some(b) = &mut p.lease {
        b.set_len(n);
    }
    let multishot = matches!(
        p.request.operation,
        Operation::Read {
            multishot: true,
            ..
        }
    );
    (
        Outcome::Read {
            n,
            lease: p.lease.take(),
        },
        !multishot,
    )
}
fn execute(
    r: &mut Resource,
    p: &mut Pending,
    pool: &BufferPool,
    set: &WaitSet,
) -> Result<Option<(Outcome<Detached>, bool)>> {
    if let Some((kind, code)) = finish_wait(p, set) {
        match kind {
            WaitKind::Connect => {
                result(p)?;
                let Socket::Tcp(socket) = &r.transport.socket else {
                    unreachable!()
                };
                r.transport.streams = Some(streams(socket));
                r.connect = None;
                return Ok(Some((Outcome::Connected, true)));
            }
            WaitKind::Accept => {
                if code & 15 != 0 {
                    return Err(Error::new(ErrorKind::Other));
                }
                assert_eq!(code >> 4, 1);
                // SAFETY: one canonical owned socket handle was transferred into area[0].
                let socket = unsafe { TcpSocket::from_handle(p.area[0]) };
                let peer = native(socket.get_remote_address().map_err(error)?);
                // Before the connection becomes visible to the host. A rejected
                // default fails this accept and drops the socket.
                sockopt::apply_accept_defaults(&socket, r.transport.accept_defaults)?;
                let transport = Detached {
                    streams: Some(streams(&socket)),
                    incoming: None,
                    socket: Socket::Tcp(socket),
                    kind: Kind::Tcp,
                    accept_defaults: AcceptDefaults::EMPTY,
                };
                let multishot =
                    matches!(p.request.operation, Operation::Accept { multishot: true });
                return Ok(Some((Outcome::Accepted { transport, peer }, !multishot)));
            }
            WaitKind::Read => {
                let n = (code >> 4) as usize;
                if n > 0 {
                    return Ok(Some(read_outcome(p, n)));
                }
                if code & 15 == 1 {
                    let s = r.transport.streams.as_mut().expect("connected");
                    s.read_closed = true;
                    let f = s.read_done.as_ref().expect("read result").0;
                    // SAFETY: this future is read once at EOF into the pinned result area.
                    let code = unsafe {
                        s.read_done
                            .as_ref()
                            .expect("read result")
                            .read(p.area.as_mut_ptr().add(16).cast())
                    };
                    start(p, set, WaitKind::ReadDone, f, code);
                    return Ok(None);
                }
            }
            WaitKind::ReadDone => {
                let result = stream_result(r, p);
                r.transport.streams.as_mut().expect("connected").read_result = Some(result);
                result?;
                return Ok(Some((Outcome::Eof, true)));
            }
            WaitKind::Write => {
                if code & 15 != 0 {
                    return Err(Error::new(ErrorKind::BrokenPipe));
                }
                p.offset += (code >> 4) as usize;
            }
            WaitKind::WriteDone => {
                let result = stream_result(r, p);
                r.transport
                    .streams
                    .as_mut()
                    .expect("connected")
                    .write_result = Some(result);
                result?;
                return Ok(Some((Outcome::Shutdown, true)));
            }
            WaitKind::Send => {
                result(p)?;
                return Ok(Some((Outcome::Wrote(p.offset), true)));
            }
            WaitKind::Receive => {
                if p.area[0] & 255 != 0 {
                    p.area.copy_within(0..5, 16);
                    result(p)?;
                    unreachable!();
                }
                let len = p.area[2] as usize;
                // SAFETY: canonical return transfers one owned byte list with len/cap len.
                let bytes = unsafe { return_storage::ReturnBytes::take(p.area[1], len) };
                let from = abi::decode_addr(&p.area[3..11]);
                let output = read_buffer(p, pool).ok_or(Error::new(ErrorKind::ResourceLimit))?;
                let n = output.len().min(bytes.len());
                output[..n].copy_from_slice(&bytes[..n]);
                if let Some(b) = &mut p.lease {
                    b.set_len(n);
                }
                return Ok(Some((
                    Outcome::RecvFrom {
                        n,
                        from,
                        lease: p.lease.take(),
                    },
                    true,
                )));
            }
        }
    } else if p.wait.is_some() {
        return Ok(None);
    }
    let t = &mut r.transport;
    match &p.request.operation {
        Operation::ProcessExit
        | Operation::WatchSignal
        | Operation::WatchFs
        | Operation::SendHandle(_)
        | Operation::RecvHandle => return Err(Error::new(ErrorKind::Unsupported)),
        Operation::Connect => {
            let Socket::Tcp(s) = &t.socket else {
                unreachable!()
            };
            p.area[0] = s.handle();
            abi::encode_addr(
                &mut p.area[1..9],
                r.connect.ok_or(Error::new(ErrorKind::InvalidInput))?,
            );
            // SAFETY: pinned initialized parameters and separate pinned SocketResult
            // area survive until this subtask returns or synchronous cancellation.
            let packed = unsafe { abi::connect(p.area.as_mut_ptr(), p.area.as_mut_ptr().add(16)) };
            let task = packed >> 4;
            if task == 0 {
                result(p)?;
                t.streams = Some(streams(s));
                r.connect = None;
                return Ok(Some((Outcome::Connected, true)));
            }
            start(p, set, WaitKind::Connect, task, packed & 15);
        }
        Operation::Accept { .. } => {
            let stream = t.incoming.as_ref().expect("listener").handle();
            // SAFETY: accepts one owned handle into this pinned four-byte slot.
            let code =
                unsafe { (TcpSocket::VTABLE.start_read)(stream, p.area.as_mut_ptr().cast(), 1) };
            start(p, set, WaitKind::Accept, stream, code);
        }
        Operation::Read { .. } => {
            let s = t
                .streams
                .as_ref()
                .ok_or(Error::new(ErrorKind::InvalidInput))?;
            if let Some(result) = s.read_result {
                result?;
                return Ok(Some((Outcome::Eof, true)));
            }
            if s.read_closed {
                let f = s.read_done.as_ref().expect("read result").0;
                // SAFETY: an earlier cancelled future read did not consume its result;
                // the new request owns its fixed return area until acknowledgement.
                let code = unsafe {
                    s.read_done
                        .as_ref()
                        .expect("read result")
                        .read(p.area.as_mut_ptr().add(16).cast())
                };
                start(p, set, WaitKind::ReadDone, f, code);
                return Ok(None);
            }
            let stream = t
                .streams
                .as_ref()
                .ok_or(Error::new(ErrorKind::InvalidInput))?
                .reader
                .as_ref()
                .expect("readable")
                .handle();
            let Some(output) = read_buffer(p, pool) else {
                return Ok(None);
            };
            // SAFETY: provided memory follows the core lifetime contract, or the
            // lease remains owned by this pinned request until acknowledgement.
            let code = unsafe {
                (u8::VTABLE.start_read)(
                    stream,
                    output.as_mut_ptr(),
                    output.len().min((1 << 28) - 1),
                )
            };
            start(p, set, WaitKind::Read, stream, code);
        }
        Operation::Write(_) | Operation::Writev(_) => {
            let bytes = write_slice(&p.request.operation, p.offset);
            if bytes.is_empty() {
                return Ok(Some((Outcome::Wrote(p.offset), true)));
            }
            let stream = t
                .streams
                .as_ref()
                .ok_or(Error::new(ErrorKind::InvalidInput))?
                .writer
                .as_ref()
                .ok_or(Error::new(ErrorKind::BrokenPipe))?
                .handle();
            // SAFETY: the accepted request owns stable bytes until terminal delivery;
            // partial writes retain offset and all buffers, including across cancel.
            let code = unsafe {
                (u8::VTABLE.start_write)(stream, bytes.as_ptr(), bytes.len().min((1 << 28) - 1))
            };
            start(p, set, WaitKind::Write, stream, code);
        }
        Operation::Shutdown => {
            let s = t
                .streams
                .as_mut()
                .ok_or(Error::new(ErrorKind::InvalidInput))?;
            if let Some(result) = s.write_result {
                result?;
                return Ok(Some((Outcome::Shutdown, true)));
            }
            s.writer.take();
            let future = s.write_done.as_ref().expect("write result").0;
            // SAFETY: close send stream once, then wait for its actual send-result
            // acknowledgement before returning Shutdown. Area stays pinned.
            let code = unsafe {
                s.write_done
                    .as_ref()
                    .expect("write result")
                    .read(p.area.as_mut_ptr().add(16).cast())
            };
            start(p, set, WaitKind::WriteDone, future, code);
        }
        Operation::SendTo { buf, to } => {
            let Socket::Udp(s) = &t.socket else {
                unreachable!()
            };
            let bytes = buf.as_slice();
            if bytes.len() > 65535 {
                return Err(Error::new(ErrorKind::InvalidInput));
            }
            p.offset = bytes.len();
            p.area[0] = s.handle();
            p.area[1] = bytes.as_ptr() as u32;
            p.area[2] = bytes.len() as u32;
            p.area[3] = 1;
            abi::encode_addr(&mut p.area[4..12], *to);
            // SAFETY: canonical borrowed list and address parameters remain valid
            // until this async import returns; no generated Vec ownership transfer.
            let packed = unsafe { abi::send(p.area.as_mut_ptr(), p.area.as_mut_ptr().add(16)) };
            if packed >> 4 == 0 {
                result(p)?;
                return Ok(Some((Outcome::Wrote(p.offset), true)));
            }
            start(p, set, WaitKind::Send, packed >> 4, packed & 15);
        }
        Operation::RecvFrom(_) => {
            let Socket::Udp(s) = &t.socket else {
                unreachable!()
            };
            if matches!(p.request.operation, Operation::RecvFrom(ReadBuf::Pooled))
                && p.lease.is_none()
            {
                let Some(b) = pool.acquire() else {
                    return Ok(None);
                };
                p.lease = Some(b);
            }
            // SAFETY: pinned 44-byte return record survives through completion.
            let packed = unsafe { abi::receive(s.handle(), p.area.as_mut_ptr()) };
            start(p, set, WaitKind::Receive, packed >> 4, packed & 15);
        }
    }
    Ok(None)
}
fn read_buffer<'a>(p: &'a mut Pending, pool: &BufferPool) -> Option<&'a mut [u8]> {
    let buf = match &p.request.operation {
        Operation::Read { buf, .. } | Operation::RecvFrom(buf) => buf,
        _ => unreachable!(),
    };
    match buf {
        ReadBuf::Provided(b) => {
            // SAFETY: core retains exclusive provided memory until acknowledgement.
            Some(unsafe { std::slice::from_raw_parts_mut(b.as_mut_ptr(), b.len()) })
        }
        ReadBuf::Pooled => {
            if p.lease.is_none() {
                p.lease = pool.acquire();
            }
            p.lease.as_mut().map(BufLease::writable)
        }
    }
}
fn write_slice(op: &Operation, mut offset: usize) -> &[u8] {
    match op {
        Operation::Write(b) => &b.as_slice()[offset..],
        Operation::Writev(bufs) => {
            for b in bufs.bufs.iter().flatten() {
                let bytes = b.as_slice();
                if offset < bytes.len() {
                    return &bytes[offset..];
                }
                offset -= bytes.len();
            }
            &[]
        }
        _ => unreachable!(),
    }
}
fn quiesce(p: &mut Pending, set: &WaitSet, r: &mut Resource) {
    let Some((waitable, kind)) = p.wait.take() else {
        return;
    };
    if waitable != 0 {
        set.remove(waitable);
    }
    let code = if let Some(code) = p.code.take().filter(|&c| !is_subtask(kind) || c >= 2) {
        code
    } else {
        // SAFETY: matching cancellation intrinsic stops all native access to this
        // request's pinned return area and buffers before returning its status.
        unsafe {
            match kind {
                WaitKind::Connect | WaitKind::Send | WaitKind::Receive => {
                    wait_set::subtask_cancel(waitable)
                }
                WaitKind::Accept => (TcpSocket::VTABLE.cancel_read)(waitable),
                WaitKind::Read => (u8::VTABLE.cancel_read)(waitable),
                WaitKind::Write => (u8::VTABLE.cancel_write)(waitable),
                WaitKind::ReadDone | WaitKind::WriteDone => {
                    if matches!(r.transport.kind, Kind::Stdio(_)) {
                        (CliResult::VTABLE.cancel_read)(waitable)
                    } else {
                        (SocketResult::VTABLE.cancel_read)(waitable)
                    }
                }
            }
        }
    };
    match kind {
        WaitKind::Accept if code >> 4 == 1 => {
            // SAFETY: cancellation raced a successful accept; discard its owned socket.
            drop(unsafe { TcpSocket::from_handle(p.area[0]) });
        }
        WaitKind::Connect | WaitKind::Send if code == 2 => {
            let _ = result(p);
        }
        WaitKind::Read if code & 15 == 1 => {
            r.transport.streams.as_mut().expect("connected").read_closed = true;
        }
        WaitKind::ReadDone | WaitKind::WriteDone if code & 15 == 0 => {
            let result = stream_result(r, p);
            let s = r.transport.streams.as_mut().expect("connected");
            if matches!(kind, WaitKind::ReadDone) {
                s.read_result = Some(result);
            } else {
                s.write_result = Some(result);
            }
        }
        WaitKind::Receive if code == 2 => {
            if p.area[0] & 255 == 0 {
                let len = p.area[2] as usize;
                // SAFETY: returned owned canonical list must be freed even when cancel wins.
                drop(unsafe { return_storage::ReturnBytes::take(p.area[1], len) });
            } else {
                p.area.copy_within(0..5, 16);
                let _ = result(p);
            }
        }
        _ => {}
    }
    if is_subtask(kind) && waitable != 0 {
        // SAFETY: return/cancellation acknowledged; membership already removed.
        unsafe {
            wait_set::subtask_drop(waitable);
        }
    }
}

/// Whether the operation at the head of a direction is one the budget throttles.
fn holds_throttled(ops: &Slots<Pending>, head: Option<usize>) -> bool {
    head.and_then(|i| ops.get(i))
        .and_then(Option::as_ref)
        .is_some_and(|p| Budget::throttles(&p.request.operation))
}
