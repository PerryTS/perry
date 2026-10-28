//! WASI 0.2 completion backend. One poll import per turn, reusable canonical
//! lists, and synchronous nonblocking I/O with generational cancellation.
use crate::slots::{Slots, page_reserve};
mod abi;
mod fs;
mod sockopt;
use crate::{
    backend::{Backend, Budget, Event, Filesystem, Operation, Outcome, PollInfo, Request, Wake},
    *,
};
use std::{
    collections::VecDeque,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::Arc,
    time::Duration,
};
use wasip2::{
    clocks::monotonic_clock as clock,
    io::{
        poll::Pollable,
        streams::{InputStream, OutputStream, StreamError},
    },
    sockets::{
        instance_network::instance_network,
        ip_name_lookup::{ResolveAddressStream, resolve_addresses},
        network::{
            ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
            Ipv6SocketAddress,
        },
        tcp::TcpSocket,
        tcp_create_socket::create_tcp_socket,
        udp::{IncomingDatagramStream, OutgoingDatagramStream, UdpSocket},
        udp_create_socket::create_udp_socket,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Tcp,
    Listener,
    Udp,
    Stdio(Stdio),
}
// Declaration order matters: pollables must be dropped before their parents.
#[derive(Debug)]
struct Streams {
    read_poll: Option<Pollable>,
    write_poll: Option<Pollable>,
    input: Option<InputStream>,
    output: Option<OutputStream>,
}
#[derive(Debug)]
struct Datagrams {
    read_poll: Pollable,
    write_poll: Pollable,
    input: IncomingDatagramStream,
    output: OutgoingDatagramStream,
}
#[derive(Debug)]
enum Socket {
    Tcp(TcpSocket),
    Udp(UdpSocket),
    Stdio,
}
/// An accepted transport, owned by core until attached. Explicit WASI transfer
/// between loops is unsupported; no WASI resource is sent to an OS thread.
#[derive(Debug)]
pub struct Detached {
    streams: Option<Streams>,
    datagrams: Option<Datagrams>,
    poll: Option<Pollable>,
    socket: Socket,
    kind: Kind,
    /// A listener's per-connection defaults, applied to each socket it accepts.
    accept_defaults: AcceptDefaults,
}
struct Resource {
    handle: Handle,
    transport: Detached,
    connect: Option<SocketAddr>,
    connecting: bool,
    ready: [bool; 2],
    heads: [Option<usize>; 2],
    tails: [Option<usize>; 2],
    queued: bool,
    /// Listed in `throttled`: a ready multishot accept held back by the budget.
    throttled: bool,
}
struct Pending {
    request: Request,
    next: Option<usize>,
    offset: usize,
    flushing: bool,
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
// Children precede parents, including when cancellation drops a lookup.
struct Lookup {
    poll: Pollable,
    stream: ResolveAddressStream,
    op: OpId,
    port: u16,
    addresses: Vec<SocketAddr>,
    ready: bool,
}
#[derive(Clone, Copy)]
enum PollOwner {
    Socket(Handle, usize),
    Dns(usize),
}
/// WASI 0.2 pollable driver with retained canonical buffers.
pub struct WasiP2 {
    resources: Slots<Resource>,
    ops: Slots<Pending>,
    ready: VecDeque<Handle>,
    /// Listeners whose multishot accept is ready but out of budget: neither
    /// runnable nor subscribed, until a poll with a positive budget (#77).
    throttled: VecDeque<Handle>,
    /// Connections multishot accepts may still deliver in the current poll.
    accepts: usize,
    cancelled: VecDeque<OpId>,
    handles: Vec<u32>,
    owners: Vec<PollOwner>,
    lookups: Slots<Lookup>,
    indices: Vec<usize>,
    poll_storage: Vec<u32>,
    scratch: Vec<u32>,
    files: super::wasi_fs::Files<fs::P2>,
    pool: BufferPool,
    wake: Arc<WasiWake>,
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
        ErrorCode::WouldBlock => ErrorKind::WouldBlock,
        ErrorCode::NotSupported => ErrorKind::Unsupported,
        ErrorCode::InvalidArgument | ErrorCode::InvalidState => ErrorKind::InvalidInput,
        ErrorCode::ConnectionRefused => ErrorKind::ConnectionRefused,
        ErrorCode::ConnectionReset | ErrorCode::ConnectionAborted => ErrorKind::ConnectionReset,
        ErrorCode::Timeout => ErrorKind::TimedOut,
        ErrorCode::OutOfMemory | ErrorCode::NewSocketLimit => ErrorKind::ResourceLimit,
        _ => ErrorKind::Other,
    })
}
fn stream_error(e: StreamError) -> Error {
    Error::new(match e {
        StreamError::Closed => ErrorKind::BrokenPipe,
        _ => ErrorKind::Other,
    })
}
fn streams(input: InputStream, output: OutputStream) -> Streams {
    Streams {
        read_poll: Some(input.subscribe()),
        write_poll: Some(output.subscribe()),
        input: Some(input),
        output: Some(output),
    }
}
impl WasiP2 {
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
        self.resources[h.index()] = Some(Resource {
            handle: h,
            transport,
            connect,
            connecting: false,
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
                let result = execute(r, p, &self.pool, &mut self.scratch);
                let event = match result {
                    Ok(Some((result, terminal))) => Some(Event {
                        op: p.request.op,
                        result: Ok(result),
                        terminal,
                    }),
                    Ok(None) => None,
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

impl WasiP2 {
    fn run_dns(&mut self, events: &mut Vec<Event<Detached>>) {
        for slot in &mut self.lookups {
            if events.len() == events.capacity() {
                break;
            }
            let Some(l) = slot.as_mut().filter(|l| l.ready) else {
                continue;
            };
            // Bound address collection per turn; ready stays true only while
            // actual buffered resolver output may still be consumed.
            for _ in 0..64 {
                match l.stream.resolve_next_address() {
                    Ok(Some(ip)) => {
                        let ip = match ip {
                            IpAddress::Ipv4(a) => Ipv4Addr::new(a.0, a.1, a.2, a.3).into(),
                            IpAddress::Ipv6(a) => {
                                Ipv6Addr::new(a.0, a.1, a.2, a.3, a.4, a.5, a.6, a.7).into()
                            }
                        };
                        l.addresses.push(SocketAddr::new(ip, l.port));
                    }
                    Ok(None) => {
                        let result = if l.addresses.is_empty() {
                            Err(Error::new(ErrorKind::NotFound))
                        } else {
                            Ok(Outcome::Resolved(std::mem::take(&mut l.addresses)))
                        };
                        events.push(Event {
                            op: l.op,
                            terminal: true,
                            result,
                        });
                        *slot = None;
                        break;
                    }
                    Err(ErrorCode::WouldBlock) => {
                        l.ready = false;
                        break;
                    }
                    Err(e) => {
                        events.push(Event {
                            op: l.op,
                            terminal: true,
                            result: Err(error(e)),
                        });
                        *slot = None;
                        break;
                    }
                }
            }
        }
    }
}

// SAFETY: I/O imports synchronously copy buffers and never retain pointers.
// Terminal events retire requests. Resource drop order releases subscriptions
// before streams and sockets; accepted transports transfer ownership into core.
unsafe impl Backend for WasiP2 {
    type Wake = WasiWake;
    type Detached = Detached;
    const FILESYSTEM: Filesystem = Filesystem::Backend;
    fn fs(&mut self, op: OpId, handle: Option<Handle>, request: FsRequest) -> Result<()> {
        self.files.submit(op, handle, request)
    }
    fn new(config: &Config, pool: BufferPool) -> Result<Self> {
        // The most pollables this loop could ever subscribe at once. Still
        // computed, because an overflow here is a configuration this backend
        // cannot serve; the poll batch below reserves a page of it and grows
        // with the pollables actually subscribed, not with the ceiling.
        let polls = config
            .max_handles
            .checked_mul(2)
            .and_then(|n| n.checked_add(config.max_operations))
            .and_then(|n| n.checked_add(1))
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        let batch = page_reserve(polls);
        Ok(Self {
            resources: Slots::new(config.max_handles),
            ops: Slots::new(config.max_operations),
            ready: VecDeque::with_capacity(page_reserve(config.max_handles)),
            throttled: VecDeque::with_capacity(page_reserve(config.max_handles)),
            accepts: usize::MAX,
            cancelled: VecDeque::with_capacity(page_reserve(config.max_operations)),
            handles: Vec::with_capacity(batch),
            owners: Vec::with_capacity(batch),
            lookups: Slots::new(config.max_operations),
            indices: Vec::with_capacity(batch),
            poll_storage: vec![0; batch],
            scratch: vec![0; 16400],
            files: super::wasi_fs::Files::new(config, pool.clone()),
            pool,
            wake: Arc::new(WasiWake),
        })
    }
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn waker(&self) -> Arc<Self::Wake> {
        self.wake.clone()
    }
    fn open(&mut self, h: Handle, spec: Open) -> Result<()> {
        let (addr, kind, reuse, backlog, accept_defaults) = match spec {
            Open::Pipe(_) | Open::PipeListener { .. } => {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            Open::Stdio(which) => {
                let input = (which == Stdio::Stdin).then(wasip2::cli::stdin::get_stdin);
                let output = match which {
                    Stdio::Stdin => None,
                    Stdio::Stdout => Some(wasip2::cli::stdout::get_stdout()),
                    Stdio::Stderr => Some(wasip2::cli::stderr::get_stderr()),
                };
                return self.install(
                    h,
                    Detached {
                        streams: Some(Streams {
                            read_poll: input.as_ref().map(InputStream::subscribe),
                            write_poll: output.as_ref().map(OutputStream::subscribe),
                            input,
                            output,
                        }),
                        datagrams: None,
                        poll: None,
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
        let mut transport = if kind == Kind::Udp {
            let socket = create_udp_socket(family).map_err(error)?;
            socket
                .start_bind(&instance_network(), address(addr))
                .map_err(error)?;
            // open has no asynchronous completion in trait-v1. A host which defers
            // bind rejects with WouldBlock; no hidden wait or retained resource.
            socket.finish_bind().map_err(error)?;
            let (input, output) = socket.stream(None).map_err(error)?;
            let datagrams = Datagrams {
                read_poll: input.subscribe(),
                write_poll: output.subscribe(),
                input,
                output,
            };
            Detached {
                poll: Some(socket.subscribe()),
                streams: None,
                datagrams: Some(datagrams),
                socket: Socket::Udp(socket),
                kind,
                accept_defaults,
            }
        } else {
            let socket = create_tcp_socket(family).map_err(error)?;
            if kind == Kind::Listener {
                socket
                    .set_listen_backlog_size(u64::from(backlog))
                    .map_err(error)?;
                socket
                    .start_bind(&instance_network(), address(addr))
                    .map_err(error)?;
                socket.finish_bind().map_err(error)?;
                socket.start_listen().map_err(error)?;
                socket.finish_listen().map_err(error)?;
            }
            Detached {
                poll: Some(socket.subscribe()),
                streams: None,
                datagrams: None,
                socket: Socket::Tcp(socket),
                kind,
                accept_defaults,
            }
        };
        // Keep all child fields established before installation.
        transport.kind = kind;
        self.install(h, transport, (kind == Kind::Tcp).then_some(addr))
    }
    fn set_option(&mut self, h: Handle, option: SocketOption) -> Result<()> {
        if matches!(option, SocketOption::ListenBacklog(_))
            && self.get(h)?.transport.kind != Kind::Listener
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let socket = &self.get(h)?.transport.socket;
        sockopt::set(socket, option)
    }
    fn get_option(&self, h: Handle, kind: SocketOptionKind) -> Result<SocketOption> {
        sockopt::get(&self.get(h)?.transport.socket, kind)
    }
    fn shutdown_receive(&mut self, h: Handle) -> Result<()> {
        let transport = &self.get(h)?.transport;
        if transport.kind != Kind::Tcp {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        let Socket::Tcp(socket) = &transport.socket else {
            return Err(Error::new(ErrorKind::Unsupported));
        };
        socket
            .shutdown(wasip2::sockets::tcp::ShutdownType::Receive)
            .map_err(error)
    }
    fn local_addr(&self, h: Handle) -> Result<SocketAddr> {
        let a = match &self.get(h)?.transport.socket {
            Socket::Tcp(s) => s.local_address(),
            Socket::Udp(s) => s.local_address(),
            Socket::Stdio => return Err(Error::new(ErrorKind::Unsupported)),
        };
        a.map(native).map_err(error)
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
            flushing: false,
        });
        self.schedule(h);
        Ok(())
    }
    fn resolve(&mut self, op: OpId, request: &DnsRequest) -> Result<()> {
        let slot = self
            .lookups
            .get_mut(op.index())
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        if slot.is_some() || self.ops[op.index()].is_some() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let stream = resolve_addresses(&instance_network(), &request.host).map_err(error)?;
        *slot = Some(Lookup {
            poll: stream.subscribe(),
            stream,
            op,
            port: request.port,
            addresses: Vec::new(),
            ready: true,
        });
        Ok(())
    }
    fn cancel(&mut self, op: OpId) -> Result<()> {
        if self.files.cancel(op) {
            return Ok(());
        }
        if self
            .lookups
            .get(op.index())
            .and_then(Option::as_ref)
            .is_some_and(|l| l.op == op)
        {
            self.lookups[op.index()] = None;
            self.cancelled.push_back(op);
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
        self.unlink(h, op.index(), d);
        self.cancelled.push_back(op);
        self.schedule(h);
        Ok(())
    }
    fn has_work(&self) -> bool {
        !self.ready.is_empty()
            || !self.cancelled.is_empty()
            || self.files.has_work()
            || self.lookups.iter().flatten().any(|l| l.ready)
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
        self.run_dns(events);
        if self.has_work() || !events.is_empty() || events.len() == events.capacity() {
            return Ok(PollInfo::default());
        }
        self.handles.clear();
        self.owners.clear();
        self.indices.clear();
        for r in self.resources.iter().flatten() {
            for d in 0..2 {
                // A held listener is not subscribed: its pollable is level
                // triggered, and would end every wait at once (rule 4a).
                if r.heads[d].is_none() || (d == 0 && r.throttled) {
                    continue;
                }
                let t = &r.transport;
                let poll = if let Some(s) = &t.streams {
                    if d == 0 {
                        s.read_poll.as_ref().expect("readable")
                    } else {
                        s.write_poll.as_ref().expect("writable")
                    }
                } else if let Some(s) = &t.datagrams {
                    if d == 0 { &s.read_poll } else { &s.write_poll }
                } else {
                    t.poll.as_ref().expect("socket pollable")
                };
                self.handles.push(poll.handle());
                self.owners.push(PollOwner::Socket(r.handle, d));
            }
        }
        for (i, l) in self.lookups.iter().enumerate() {
            if let Some(l) = l {
                self.handles.push(l.poll.handle());
                self.owners.push(PollOwner::Dns(i));
            }
        }
        let deadline = timeout
            .map(|d| clock::subscribe_duration(d.as_nanos().min(u128::from(u64::MAX)) as u64));
        if let Some(p) = &deadline {
            self.handles.push(p.handle());
        }
        if self.handles.is_empty() {
            // No native source can ever wake a Forever wait on this single agent.
            return Err(Error::new(ErrorKind::Unsupported));
        }
        // `poll` lowers its result into `poll_storage` as the canonical return
        // arena, and returns at most one index per subscribed pollable, so the
        // arena has to cover exactly that many `u32`s. It grows with the poll
        // batch; at a steady set of pollables it is already large enough.
        if self.poll_storage.len() < self.handles.len() {
            self.poll_storage.resize(self.handles.len(), 0);
        }
        abi::poll(&self.handles, &mut self.poll_storage, &mut self.indices);
        // The private deadline pollable implements this wait's timeout, so its
        // readiness has the meaning of the poll returning nothing (see `PollInfo`
        // in backend/mod.rs, epoll's timerfd and the IOCP deadline packet): it is
        // not native work. It is the only handle without an `owners` entry, so
        // socket and DNS readiness alone counts, including when they arrive in
        // the same result as the expiry.
        let mut native_events = 0;
        for i in 0..self.indices.len() {
            match self.owners.get(self.indices[i]).copied() {
                Some(PollOwner::Socket(h, d)) => {
                    self.resources[h.index()]
                        .as_mut()
                        .expect("live poll owner")
                        .ready[d] = true;
                    self.schedule(h);
                    native_events += 1;
                }
                Some(PollOwner::Dns(i)) => {
                    self.lookups[i].as_mut().expect("live lookup").ready = true;
                    native_events += 1;
                }
                None => {}
            }
        }
        self.run_ready(events);
        self.run_dns(events);
        Ok(PollInfo::native(timeout, native_events == 0))
    }
    fn release(&mut self, h: Handle) {
        self.files.release(h);
        if self.get(h).is_ok() {
            self.ready.retain(|&at| at != h);
            self.throttled.retain(|&at| at != h);
            self.resources[h.index()] = None;
        }
    }
    fn detach(&mut self, _h: Handle) -> Result<Detached> {
        Err(Error::new(ErrorKind::Unsupported))
    }
    fn attach(&mut self, h: Handle, transport: Detached) -> Result<()> {
        self.install(h, transport, None)
    }
    fn integration(&mut self) -> Result<Integration> {
        Ok(Integration::RuntimeOwned)
    }
}
fn execute(
    r: &mut Resource,
    p: &mut Pending,
    pool: &BufferPool,
    scratch: &mut [u32],
) -> Result<Option<(Outcome<Detached>, bool)>> {
    let t = &mut r.transport;
    match &mut p.request.operation {
        Operation::ProcessExit
        | Operation::WatchSignal
        | Operation::WatchFs
        | Operation::SendHandle(_)
        | Operation::RecvHandle => Err(Error::new(ErrorKind::Unsupported)),
        Operation::Connect => {
            let Socket::Tcp(socket) = &t.socket else {
                unreachable!()
            };
            if !r.connecting {
                socket
                    .start_connect(
                        &instance_network(),
                        address(r.connect.ok_or(Error::new(ErrorKind::InvalidInput))?),
                    )
                    .map_err(error)?;
                r.connecting = true;
            }
            let (input, output) = socket.finish_connect().map_err(error)?;
            t.streams = Some(streams(input, output));
            r.connect = None;
            Ok(Some((Outcome::Connected, true)))
        }
        Operation::Accept { multishot } => {
            let Socket::Tcp(socket) = &t.socket else {
                unreachable!()
            };
            let (socket, input, output) = socket.accept().map_err(error)?;
            let peer = native(socket.remote_address().map_err(error)?);
            // Before the connection becomes visible to the host. A rejected
            // default fails this accept and drops the socket.
            sockopt::apply_accept_defaults(&socket, t.accept_defaults)?;
            let transport = Detached {
                streams: Some(streams(input, output)),
                datagrams: None,
                poll: Some(socket.subscribe()),
                socket: Socket::Tcp(socket),
                kind: Kind::Tcp,
                accept_defaults: AcceptDefaults::EMPTY,
            };
            Ok(Some((Outcome::Accepted { transport, peer }, !*multishot)))
        }
        Operation::Read { buf, multishot } => receive(t, buf, *multishot, pool, scratch),
        Operation::RecvFrom(buf) => receive(t, buf, false, pool, scratch),
        Operation::Write(buf) => write(t, buf.as_slice(), &mut p.offset, &mut p.flushing),
        Operation::Writev(bufs) => {
            let s = t
                .streams
                .as_ref()
                .ok_or(Error::new(ErrorKind::InvalidInput))?;
            let output = s.output.as_ref().ok_or(Error::new(ErrorKind::BrokenPipe))?;
            let permitted = output.check_write().map_err(stream_error)? as usize;
            if permitted == 0 {
                return Err(Error::new(ErrorKind::WouldBlock));
            }
            if p.flushing {
                return Ok(Some((Outcome::Wrote(p.offset), true)));
            }
            let mut skip = p.offset;
            for b in bufs.bufs.iter().flatten() {
                let bytes = b.as_slice();
                if skip >= bytes.len() {
                    skip -= bytes.len();
                    continue;
                }
                let n = (bytes.len() - skip).min(permitted);
                output.write(&bytes[skip..skip + n]).map_err(stream_error)?;
                p.offset += n;
                break;
            }
            let total: usize = bufs.bufs.iter().flatten().map(|b| b.as_slice().len()).sum();
            if p.offset == total {
                output.flush().map_err(stream_error)?;
                p.flushing = true;
            }
            Ok(None)
        }
        Operation::SendTo { buf, to } => {
            let s = t
                .datagrams
                .as_ref()
                .ok_or(Error::new(ErrorKind::InvalidInput))?;
            if s.output.check_send().map_err(error)? == 0 {
                return Err(Error::new(ErrorKind::WouldBlock));
            }
            let n = abi::send(&s.output, buf.as_slice(), *to)?;
            Ok(Some((Outcome::Wrote(n), true)))
        }
        Operation::Shutdown => {
            if matches!(t.kind, Kind::Stdio(_)) {
                let s = t.streams.as_mut().expect("stdio streams");
                s.write_poll.take();
                s.output.take();
                return Ok(Some((Outcome::Shutdown, true)));
            }
            let Socket::Tcp(socket) = &t.socket else {
                unreachable!()
            };
            socket
                .shutdown(wasip2::sockets::tcp::ShutdownType::Send)
                .map_err(error)?;
            Ok(Some((Outcome::Shutdown, true)))
        }
    }
}
fn write(
    t: &Detached,
    bytes: &[u8],
    offset: &mut usize,
    flushing: &mut bool,
) -> Result<Option<(Outcome<Detached>, bool)>> {
    let s = t
        .streams
        .as_ref()
        .ok_or(Error::new(ErrorKind::InvalidInput))?;
    let output = s.output.as_ref().ok_or(Error::new(ErrorKind::BrokenPipe))?;
    let permitted = output.check_write().map_err(stream_error)? as usize;
    if permitted == 0 {
        return Err(Error::new(ErrorKind::WouldBlock));
    }
    if *flushing {
        return Ok(Some((Outcome::Wrote(*offset), true)));
    }
    let n = (bytes.len() - *offset).min(permitted);
    if n > 0 {
        output
            .write(&bytes[*offset..*offset + n])
            .map_err(stream_error)?;
        *offset += n;
    }
    if *offset == bytes.len() {
        output.flush().map_err(stream_error)?;
        *flushing = true;
    }
    Ok(None)
}
fn receive(
    t: &Detached,
    buf: &mut ReadBuf,
    multishot: bool,
    pool: &BufferPool,
    scratch: &mut [u32],
) -> Result<Option<(Outcome<Detached>, bool)>> {
    let mut lease = match buf {
        ReadBuf::Pooled => {
            let Some(b) = pool.acquire() else {
                return Ok(None);
            };
            Some(b)
        }
        _ => None,
    };
    let output = match buf {
        ReadBuf::Provided(b) => {
            // SAFETY: Request owns the exclusive provided region until its terminal event.
            unsafe { std::slice::from_raw_parts_mut(b.as_mut_ptr(), b.len()) }
        }
        ReadBuf::Pooled => lease.as_mut().expect("acquired").writable(),
    };
    let (n, from) = if let Some(s) = &t.datagrams {
        let Some((n, from)) = abi::receive(&s.input, output, scratch)? else {
            return Err(Error::new(ErrorKind::WouldBlock));
        };
        (n, Some(from))
    } else {
        let s = t
            .streams
            .as_ref()
            .ok_or(Error::new(ErrorKind::InvalidInput))?;
        let limit = output.len().min(scratch.len() * 4);
        match abi::read(
            s.input.as_ref().expect("readable"),
            &mut output[..limit],
            scratch,
        ) {
            Ok(0) => return Err(Error::new(ErrorKind::WouldBlock)),
            Ok(n) => (n, None),
            Err(StreamError::Closed) => return Ok(Some((Outcome::Eof, true))),
            Err(e) => return Err(stream_error(e)),
        }
    };
    if let Some(b) = &mut lease {
        b.set_len(n);
    }
    Ok(Some((
        if let Some(from) = from {
            Outcome::RecvFrom { n, from, lease }
        } else {
            Outcome::Read { n, lease }
        },
        !multishot,
    )))
}

/// Whether the operation at the head of a direction is one the budget throttles.
fn holds_throttled(ops: &Slots<Pending>, head: Option<usize>) -> bool {
    head.and_then(|i| ops.get(i))
        .and_then(Option::as_ref)
        .is_some_and(|p| Budget::throttles(&p.request.operation))
}
