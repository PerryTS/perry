//! Backend-neutral identifiers, errors, deadlines and socket options.
use crate::Instant;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
/// Opaque host routing value, returned unchanged with its completions.
pub struct Token(pub u64);

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        /// Loop-scoped generational identity; stale or foreign IDs never address a reused resource.
        pub struct $name {
            pub(crate) owner: u64,
            pub(crate) key: u64,
        }
        impl $name {
            /// Slot index, for paged backend operation/resource storage.
            pub fn index(self) -> usize {
                self.key as u32 as usize
            }
            /// Generation and index. Unique within a loop until u32 generation exhaustion.
            pub fn key(self) -> u64 {
                self.key
            }
            /// Unique identity of the creating loop. Never route on the token alone.
            pub fn owner(self) -> u64 {
                self.owner
            }
        }
    };
}
id!(Handle);
id!(OpId);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The native identity of a transport, for host reporting and for a descriptor
/// the host has taken ownership of (DESIGN §5a).
///
/// [`Driver::raw_transport`](crate::Driver::raw_transport) reports this for a
/// live, loop-owned handle; it is Node's `socket._handle.fd`. The value is
/// **reporting only**: see that method for the rules. Taking ownership is a
/// separate, owning step (`detach` then `Detached::into_fd`/`into_socket`/
/// `into_handle`).
pub enum RawTransport {
    /// A Unix file descriptor.
    Fd(i32),
    /// A Windows `SOCKET`.
    Socket(usize),
    /// A Windows `HANDLE`: a named-pipe instance, console or adopted stream.
    Handle(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Portable error categories independent of native numeric error codes.
pub enum ErrorKind {
    /// The operation was cancelled and its native buffer access has ended.
    Cancelled,
    /// This platform or resource does not support the requested capability.
    Unsupported,
    /// An argument or resource state is invalid for the requested operation.
    InvalidInput,
    /// The requested identity or resource does not exist or is no longer owned.
    NotFound,
    /// Progress requires a later turn or release of outstanding capacity.
    WouldBlock,
    /// The requested deadline expired.
    TimedOut,
    /// The remote endpoint refused the connection.
    ConnectionRefused,
    /// The established connection was reset or aborted.
    ConnectionReset,
    /// The write side has no reader or the connection cannot accept writes.
    BrokenPipe,
    /// A configured capacity or native resource limit was reached.
    ResourceLimit,
    /// Permissions or a capability boundary denied a filesystem operation.
    PermissionDenied,
    /// The target already exists (for example an exclusive create).
    AlreadyExists,
    /// A path component that must be a directory is not one.
    NotADirectory,
    /// A file operation targeted a directory.
    IsADirectory,
    /// A directory to be removed or replaced is not empty.
    DirectoryNotEmpty,
    /// An error without a more specific portable category.
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Portable error kind with an optional original operating-system code.
pub struct Error {
    /// Portable error category.
    pub kind: ErrorKind,
    /// Original OS error code, when one exists.
    pub os: Option<i32>,
}
impl Error {
    /// Create a portable error without an OS code.
    pub const fn new(kind: ErrorKind) -> Self {
        Self { kind, os: None }
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind as I;
        let kind = match e.kind() {
            I::Unsupported => ErrorKind::Unsupported,
            I::InvalidInput => ErrorKind::InvalidInput,
            I::NotFound => ErrorKind::NotFound,
            I::WouldBlock => ErrorKind::WouldBlock,
            I::TimedOut => ErrorKind::TimedOut,
            I::ConnectionRefused => ErrorKind::ConnectionRefused,
            I::ConnectionReset | I::ConnectionAborted => ErrorKind::ConnectionReset,
            I::BrokenPipe => ErrorKind::BrokenPipe,
            I::OutOfMemory => ErrorKind::ResourceLimit,
            I::PermissionDenied => ErrorKind::PermissionDenied,
            I::AlreadyExists => ErrorKind::AlreadyExists,
            I::NotADirectory => ErrorKind::NotADirectory,
            I::IsADirectory => ErrorKind::IsADirectory,
            I::DirectoryNotEmpty => ErrorKind::DirectoryNotEmpty,
            _ => ErrorKind::Other,
        };
        Self {
            kind,
            os: e.raw_os_error(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} (OS {:?})", self.kind, self.os)
    }
}
impl std::error::Error for Error {}
/// A fallible turnloop operation preserving portable and native error information.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug)]
/// Exact timeout budget for one host-driven turn.
pub enum Timeout {
    /// Collect immediately available work without blocking.
    Now,
    /// Wait at most this duration measured from turn entry.
    After(Duration),
    /// Wait until this absolute backend-clock deadline.
    Until(Instant),
    /// Wait without a host deadline, still bounded by pending timers and notifications.
    Forever,
}
impl Timeout {
    /// Absolute deadline used once at turn entry (retries never extend the budget).
    pub fn deadline(self, now: Instant) -> Option<Instant> {
        match self {
            Self::Now => Some(now),
            Self::After(d) => now.checked_add(d),
            Self::Until(t) => Some(t),
            Self::Forever => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// Fixed loop capacities and shared blocking-pool configuration.
pub struct Config {
    /// Maximum simultaneously allocated handles, including closed results awaiting delivery.
    ///
    /// A ceiling, not a reservation: slot storage is built in pages as the loop's
    /// high-water mark rises, so an idle loop costs the same whatever this is and
    /// a large value is affordable for a loop per agent. Reaching it refuses an
    /// accept at submission, before the kernel is asked for a connection, so
    /// pending connections wait in the listener's backlog rather than being
    /// accepted and destroyed.
    pub max_handles: usize,
    /// Maximum outstanding operations, including terminal results awaiting delivery.
    ///
    /// A ceiling paged like [`Config::max_handles`], except for the Windows
    /// `OVERLAPPED` slab, which stays contiguous.
    pub max_operations: usize,
    /// Native event budget and per-source completion reserve.
    pub events_per_turn: usize,
    /// Number of reusable pooled read buffers.
    pub pooled_buffers: usize,
    /// Bytes retained in each pooled read buffer.
    pub pooled_buffer_size: usize,
    /// Maximum queued cross-thread posts, rounded up to a power of two.
    pub post_capacity: usize,
    /// First pool submission fixes the process-wide configuration. Later loops
    /// must use the same configuration; mismatches are reported as InvalidInput.
    pub blocking_pool: crate::PoolConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_handles: 1024,
            max_operations: 4096,
            events_per_turn: 256,
            pooled_buffers: 256,
            pooled_buffer_size: 16 * 1024,
            post_capacity: 1024,
            blocking_pool: crate::PoolConfig::default(),
        }
    }
}
impl Config {
    /// Capacities for a short-lived loop that drives one outbound connection,
    /// one request at a time: a client that resolves a name, connects, writes a
    /// request, reads the response and drops the loop.
    ///
    /// [`Config::default`] sizes every table for a server. This preset keeps the
    /// defaults only where they are not per-loop: the pooled buffer size (one
    /// read still takes a full TLS record) and [`Config::blocking_pool`], which is
    /// process-wide and must match every other loop's, so shrinking it here would
    /// make the first DNS lookup fail with `InvalidInput` in a process that also
    /// runs default loops.
    ///
    /// # The floor, and what it covers
    ///
    /// A handle's slot is held from creation until its `Closed` completion is
    /// delivered, and an operation's until its terminal completion is. The worst
    /// moment for this caller is a reconnect (a redirect or a retry) that starts
    /// before the previous attempt's completions have been turned out:
    ///
    /// | Holding a slot at that moment | handles | operations |
    /// |---|---|---|
    /// | the old socket, closing, and its cancelled read, write and shutdown | 1 | 3 |
    /// | the new socket: its connect, then read, write and shutdown | 1 | 3 |
    /// | a request deadline timer, and the one it replaces | 2 | 2 |
    /// | a DNS lookup, and one other blocking-pool job | 0 | 2 |
    /// | **needed** | **4** | **10** |
    /// | **this preset** | **8** | **16** |
    ///
    /// `max_handles` and `max_operations` are the only fields a caller can size
    /// too small by guessing, and they fail loudly: a creation or submission past
    /// either ceiling is refused with `ResourceLimit`, nothing is dropped. The
    /// other fields only pace the loop: `events_per_turn` bounds how many native
    /// events one turn collects, `pooled_buffers` how many
    /// [`ReadBuf::Pooled`](crate::ReadBuf::Pooled) reads can hold data at once
    /// (a read waits for a free buffer rather than failing; two let the next read
    /// fill while the host still holds the last one), and `post_capacity` how
    /// many cross-thread posts can queue before `Poster::post` refuses one.
    ///
    /// Anything beyond the table, such as a second concurrent connection, a
    /// listener, child processes, signal subscriptions or filesystem requests,
    /// needs its own handles and operations on top. With the `executor`
    /// feature's `LocalExecutor`, pair this with
    /// `ExecutorConfig::single_connection`, whose operations fit inside these.
    pub fn single_connection() -> Self {
        Self {
            max_handles: 8,
            max_operations: 16,
            events_per_turn: 16,
            pooled_buffers: 2,
            pooled_buffer_size: 16 * 1024,
            post_capacity: 16,
            blocking_pool: crate::PoolConfig::default(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
/// Connect-time options for [`Loop::tcp_connect`].
///
/// This struct holds what must be decided before the connection attempt starts
/// and what the loop itself enforces. Every socket option that can be changed
/// on a live socket goes through [`Loop::set_option`] and [`SocketOption`]
/// instead, and can be applied to the handle `tcp_connect` returns, before the
/// `Connected` completion arrives, wherever the backend supports that option:
/// keep-alive, linger, TTL and buffer sizes. A
/// receive buffer set that way is applied after the handshake has started, so it
/// cannot change the window scale the peers negotiate.
///
/// To preserve a local address/port or pre-handshake options set by the host,
/// use [`crate::Driver::tcp_connect_bound`] with a bound native transport.
/// `IPV6_V6ONLY` is not needed for fresh connections here: their socket family
/// is the destination's.
///
/// [`Loop::tcp_connect`]: crate::Driver::tcp_connect
/// [`Loop::set_option`]: crate::Driver::set_option
pub struct TcpOpts {
    /// Disable the TCP Nagle algorithm for latency-sensitive small writes.
    pub nodelay: bool,
    /// Give up on the connection attempt after this long.
    ///
    /// Enforced by the loop on its own clock, identically on every backend: when
    /// the deadline passes, the pending connect is cancelled and completes once,
    /// with `Err` of kind `TimedOut`, only after the backend acknowledges the
    /// cancellation. It covers the attempt only, never later stream I/O, and it
    /// cannot outlast the operating system's own connect timeout, which still
    /// applies. The handle stays open after a timeout; close it as after any
    /// other failed connect. `None` (the default) leaves the attempt to the OS.
    /// A zero duration is `InvalidInput`.
    pub connect_timeout: Option<Duration>,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// What a second bind of the same address is allowed to do.
///
/// `SO_REUSEPORT` is spelled the same on Linux and on the BSDs and **does not
/// mean the same thing**. On Linux it both permits the duplicate bind and
/// distributes incoming connections across every listener holding the address.
/// On macOS and the other BSDs the same option permits the duplicate bind and
/// then hands new connections to the socket that bound *last*, so a second
/// listener silently takes the whole port and the first one accepts nothing.
///
/// A `bool` cannot express that difference, so it is not one. Each variant here
/// names the behaviour the caller is asking the kernel for, and a backend that
/// cannot provide it refuses the listener with `Unsupported` at creation rather
/// than binding a socket that will never be given any work.
///
/// The variants are ordered by strength, and a platform may satisfy a request
/// with something stronger: [`Share`](Self::Share) on Linux is the same
/// `setsockopt` as [`Distribute`](Self::Distribute) and does distribute. What a
/// variant guarantees is a floor, never a ceiling.
pub enum ReusePort {
    /// Exclusive bind: no other socket may hold this address (the default).
    #[default]
    No,
    /// Permit the duplicate bind, and promise nothing about delivery.
    ///
    /// Several sockets may hold the address at once; which one receives a given
    /// connection or datagram is the platform's business. This is the variant
    /// for the traditional BSD uses — receiving multicast or broadcast datagrams
    /// in several processes, and handing a port to a replacement process during
    /// a zero-downtime restart — where last-binder-wins is the desired effect
    /// rather than a defect.
    ///
    /// Supported wherever `SO_REUSEPORT` exists: Linux, Android, macOS and the
    /// BSDs. `Unsupported` on Windows, WASI and the web, and on Unix local
    /// (`AF_UNIX`) listeners.
    Share,
    /// Permit the duplicate bind **and** spread incoming connections across
    /// every listener holding the address.
    ///
    /// This is the kernel-balanced route of DESIGN §5a: N loops on N threads
    /// each bind the same port, and the kernel decides which loop accepts each
    /// connection, with no shared accept lock and no handoff.
    ///
    /// **It distributes by hash, not by load.** Linux selects the listener by
    /// hashing the connection's address 4-tuple; FreeBSD's `SO_REUSEPORT_LB`
    /// does the same. Neither asks how busy a listener is, so a loop whose agent
    /// is blocked in a long turn keeps being given its share of new connections
    /// and they wait in its queue. Even distribution of *connections* is not
    /// even distribution of *work*, and a host that needs the latter wants the
    /// handoff route ([`Loop::detach`]/[`Loop::attach`]), where the policy is
    /// the host's to write.
    ///
    /// Supported on Linux and Android (`SO_REUSEPORT`) and on FreeBSD
    /// (`SO_REUSEPORT_LB`, FreeBSD 12.0+). **`Unsupported` on macOS and other
    /// Apple platforms, on NetBSD, OpenBSD and DragonFly, on Windows, on WASI
    /// and on the web** — none of them has an option that distributes, and
    /// accepting the request by setting plain `SO_REUSEPORT` would produce
    /// exactly the silently-starved listener this variant exists to prevent.
    ///
    /// [`Loop::detach`]: crate::Driver::detach
    /// [`Loop::attach`]: crate::Driver::attach
    Distribute,
}
impl ReusePort {
    /// Whether this request needs a duplicate bind at all.
    pub const fn is_enabled(self) -> bool {
        !matches!(self, Self::No)
    }
}
#[derive(Clone, Copy, Debug)]
/// Listener backlog, kernel reuse-port configuration and accepted-socket defaults.
///
/// Address reuse is bind-time only and stays here rather than in
/// [`SocketOption`]: `SO_REUSEPORT` is `reuse_port`, and `SO_REUSEADDR` is applied
/// by the backend to every TCP listener it binds (TIME_WAIT rebinding). Neither
/// can be changed on a socket that is already bound, so neither is an option.
pub struct ListenOpts {
    /// What a second bind of this address may do; see [`ReusePort`].
    pub reuse_port: ReusePort,
    /// Maximum pending connection backlog requested from the OS.
    pub backlog: u32,
    /// Options applied to every connection this listener accepts.
    pub accept_defaults: AcceptDefaults,
}
impl Default for ListenOpts {
    fn default() -> Self {
        Self {
            reuse_port: ReusePort::No,
            backlog: 128,
            accept_defaults: AcceptDefaults::EMPTY,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Options a listener applies to every connection it accepts.
///
/// **When they are applied:** by the accepting loop, on the accepted socket,
/// after the OS accept succeeds and *before* the `Accepted` completion reaches
/// the host. A host therefore never observes an unconfigured connection, and
/// never needs a `set_option` round trip per connection.
///
/// **What "cheaply" means:** each field costs at most one `setsockopt` on the new
/// socket. Anything that would need a syscall per accepted byte, a kernel query
/// or a second handle is not a default and belongs in [`Loop::set_option`].
///
/// A backend that cannot apply a requested default rejects the *listener* when it
/// is created, rather than ignoring the request once per connection. If the OS
/// rejects a default while applying it to a live accepted socket, that accept
/// operation fails with the OS error and the connection is closed.
///
/// [`Loop::set_option`]: crate::Driver::set_option
pub struct AcceptDefaults {
    /// Disable Nagle coalescing on each accepted connection (`TCP_NODELAY`).
    pub nodelay: bool,
    /// Enable keep-alive probes on each accepted connection, with this schedule.
    pub keep_alive: Option<KeepAlive>,
}
impl AcceptDefaults {
    /// Apply nothing: every accepted socket keeps the platform defaults.
    pub const EMPTY: Self = Self {
        nodelay: false,
        keep_alive: None,
    };
    /// Whether any default would need to be applied to an accepted socket.
    pub const fn is_empty(self) -> bool {
        !self.nodelay && self.keep_alive.is_none()
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// TCP keep-alive probe schedule. Each `None` leaves that kernel setting unchanged.
///
/// Native platforms express the schedule in whole seconds, so a duration is
/// rounded **up** to the next whole second and a zero duration is rejected as
/// `InvalidInput`. WASI takes the duration unrounded.
pub struct KeepAlive {
    /// Connection idle time before the first probe is sent.
    pub idle: Option<Duration>,
    /// Interval between probes once probing has started.
    pub interval: Option<Duration>,
    /// Unanswered probes before the connection is dropped.
    pub count: Option<u32>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A multicast group and the local interface that should carry it.
pub struct MulticastGroup {
    /// Group address. Its family must match the socket's own family.
    pub group: IpAddr,
    /// Local interface index, or zero for the kernel's default interface.
    ///
    /// An IPv6 group accepts any index on every native platform. An IPv4 group
    /// accepts a nonzero index only on Linux (`ip_mreqn`); elsewhere a nonzero
    /// index is reported `Unsupported` rather than silently ignored.
    pub interface: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A socket option that can be changed after the socket exists.
///
/// Bind-time-only options are deliberately absent: `SO_REUSEPORT`/`SO_REUSEADDR`
/// live in [`ListenOpts`] and [`UdpOpts`], because the OS accepts them only on an
/// unbound socket. [`Ipv6Only`](Self::Ipv6Only) is the borderline case: it is
/// kept here so it can be *read* on any socket, but every platform rejects
/// setting it after bind, and turnloop binds listeners and UDP sockets when they
/// are created.
///
/// A backend with no equivalent for a variant reports `Unsupported`. It never
/// accepts the call and ignores it.
pub enum SocketOption {
    /// `TCP_NODELAY`: send small writes immediately instead of coalescing them.
    NoDelay(bool),
    /// `SO_KEEPALIVE` and its probe schedule; `None` disables probing.
    ///
    /// The switch and each schedule value are separate kernel settings, and every
    /// value is validated before any of them is written. If the OS still rejects
    /// one after the switch was set, the error is reported and the socket keeps
    /// whatever the OS left: read it back rather than assuming a rollback.
    /// Disabling clears the switch and leaves the schedule alone, as the OS does.
    KeepAlive(Option<KeepAlive>),
    /// Change only the keep-alive switch, preserving the kernel's probe schedule.
    KeepAliveEnabled(bool),
    /// Change the provided schedule fields without enabling/disabling probing.
    /// Omitted fields remain unchanged; getters return all three native values,
    /// including while probing is disabled. Validation precedes every write.
    KeepAliveSettings(KeepAlive),
    /// Update an existing TCP listener's backlog without replacing its socket.
    /// Zero, values above i32::MAX, and non-listener handles are InvalidInput.
    /// This option has no getter: kernels do not expose a portable backlog query.
    ListenBacklog(u32),
    /// `SO_LINGER`: `Some(d)` blocks the close until queued data is delivered or
    /// `d` elapses, and `Some(Duration::ZERO)` discards it and resets the
    /// connection. `None` restores the platform default (a graceful background
    /// close). The duration has one-second granularity, rounded up.
    Linger(Option<Duration>),
    /// `SO_RCVBUF`: requested receive-buffer bytes.
    ///
    /// **The final size is the kernel's choice, not the request.** The contract
    /// is only that the socket ends up with *at least* what was asked for, so
    /// read it back rather than assuming an exact value:
    ///
    /// * **Linux** doubles the request and clamps it to `net.core.rmem_max`
    ///   (asking 262144 on a stock kernel reports 524288).
    /// * **macOS/BSD** normally keep the request exactly, but start much higher
    ///   than Linux: an accepted loopback socket defaults to around 408300 bytes.
    /// * **Windows** rounds up to its own granularity and may keep an auto-tuned
    ///   receive window that is larger than the request.
    /// * **WASI** forwards to the host socket, so it inherits that host's policy.
    RecvBufferSize(u32),
    /// `SO_SNDBUF`: requested send-buffer bytes. The final size is the kernel's
    /// choice with the same per-platform rounding as
    /// [`RecvBufferSize`](Self::RecvBufferSize); read it back.
    SendBufferSize(u32),
    /// `IP_TTL` / `IPV6_UNICAST_HOPS`: hop limit for outgoing unicast packets.
    Ttl(u32),
    /// `IPV6_V6ONLY`: refuse IPv4-mapped peers on an IPv6 socket. Bind-time on
    /// every platform, so setting it on a bound socket fails.
    Ipv6Only(bool),
    /// `SO_BROADCAST`: permit datagrams addressed to a broadcast address.
    Broadcast(bool),
    /// `IP_MULTICAST_TTL` / `IPV6_MULTICAST_HOPS`: outgoing multicast hop limit.
    MulticastTtl(u32),
    /// `IP_MULTICAST_LOOP` / `IPV6_MULTICAST_LOOP`: deliver this socket's own
    /// multicast sends back to local members.
    MulticastLoop(bool),
    /// `IP_ADD_MEMBERSHIP` / `IPV6_JOIN_GROUP`: start receiving this group.
    MulticastJoin(MulticastGroup),
    /// `IP_DROP_MEMBERSHIP` / `IPV6_LEAVE_GROUP`: stop receiving this group.
    MulticastLeave(MulticastGroup),
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// Names a readable socket option for `Loop::get_option`.
///
/// Group membership has no getter: the OS exposes no per-socket membership
/// query, so [`SocketOption::MulticastJoin`] and
/// [`SocketOption::MulticastLeave`] have no kind here rather than a kind that
/// would have to answer `Unsupported` everywhere.
pub enum SocketOptionKind {
    /// Read [`SocketOption::NoDelay`].
    NoDelay,
    /// Read [`SocketOption::KeepAlive`], including the schedule the OS holds.
    KeepAlive,
    /// Read only the keep-alive switch.
    KeepAliveEnabled,
    /// Read the probe schedule, even when keep-alive is disabled.
    KeepAliveSettings,
    /// Read [`SocketOption::Linger`].
    Linger,
    /// Read [`SocketOption::RecvBufferSize`] as the OS actually kept it.
    RecvBufferSize,
    /// Read [`SocketOption::SendBufferSize`] as the OS actually kept it.
    SendBufferSize,
    /// Read [`SocketOption::Ttl`].
    Ttl,
    /// Read [`SocketOption::Ipv6Only`].
    Ipv6Only,
    /// Read [`SocketOption::Broadcast`].
    Broadcast,
    /// Read [`SocketOption::MulticastTtl`].
    MulticastTtl,
    /// Read [`SocketOption::MulticastLoop`].
    MulticastLoop,
}
#[derive(Clone, Copy, Debug, Default)]
/// UDP binding options. Reuse is bind-time only and stays here, not in
/// [`SocketOption`]; everything changeable on a live socket is an option.
pub struct UdpOpts {
    /// What a second bind of this address may do; see [`ReusePort`].
    ///
    /// Defaults to [`ReusePort::No`]: a live UDP endpoint cannot be shared by
    /// another bind. [`ReusePort::Share`] is the variant multicast and broadcast
    /// receivers want; [`ReusePort::Distribute`] spreads *datagrams* across the
    /// bound sockets on the platforms that can, and is refused elsewhere.
    pub reuse_port: ReusePort,
}

/// The fd/event is borrowed from the driver; it must never be closed by the host.
#[derive(Clone, Copy, Debug)]
pub enum Integration {
    /// Borrowed native poller descriptor for external host waiting.
    Fd(i32),
    /// Native waitable event, represented as bits without imposing Windows types.
    Event(usize),
    /// The host schedules nonblocking turns when work arrives.
    HostCallback,
    /// The component runtime owns scheduling and the wait primitive.
    RuntimeOwned,
}

#[derive(Debug)]
/// Backend-neutral resource creation requests; unsupported kinds must fail explicitly.
pub enum Open {
    #[cfg(turnloop_backend = "web")]
    /// Open a host HTTP fetch stream.
    Fetch {
        /// Host URL for the request.
        url: String,
    },
    #[cfg(turnloop_backend = "web")]
    /// Open a host WebSocket stream.
    WebSocket {
        /// Host URL for the connection.
        url: String,
    },
    /// Unconnected local stream.
    Pipe(crate::PipeName),
    /// Local stream listener.
    PipeListener {
        /// Local IPC address in the platform namespace.
        name: crate::PipeName,
        /// Options controlling resource creation.
        opts: ListenOpts,
    },
    /// Duplicate and classify a host standard stream.
    Stdio(crate::Stdio),
    /// Create an unconnected TCP socket for a subsequent Connect operation.
    Tcp {
        /// IP address and port used to bind or connect this resource.
        addr: SocketAddr,
        /// Options controlling resource creation.
        opts: TcpOpts,
    },
    /// Create and bind a listening TCP socket.
    Listener {
        /// IP address and port used to bind or connect this resource.
        addr: SocketAddr,
        /// Options controlling resource creation.
        opts: ListenOpts,
    },
    /// Create and bind a UDP socket.
    Udp {
        /// IP address and port used to bind or connect this resource.
        addr: SocketAddr,
        /// Options controlling resource creation.
        opts: UdpOpts,
    },
}
