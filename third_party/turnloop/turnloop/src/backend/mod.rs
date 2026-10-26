//! Internal platform contract, revision 2 (empty-wait instrumentation and native services).
//!
//! Public only so the contract runner and independently developed backends can use
//! it; not a stable end-user extension API. All identifiers and buffers are core
//! types. No raw fd, OVERLAPPED pointer, pollable or browser object crosses here,
//! with one deliberate exception: `raw_transport` reports a `RawTransport` the core
//! passes straight through to the host and never acts on (§5a handle transfer).
//! Ownership still leaves through `detach`/`Detached`, never through that value.
//!
//! # Ownership and completion rules (D1–D4)
//!
//! * `open`/`attach` bind a resource to the supplied generational Handle. On error
//!   they retain nothing. Open::Tcp creates an unconnected resource; the Connect
//!   request performs the connection. Once validated/accepted, even a synchronous
//!   I/O error is queued as a terminal Event. Submit errors reject before acceptance
//!   (unsupported/invalid operation, capacity/resource setup failure).
//!   `submit` accepts ownership of a Request only on Ok; on
//!   Err it must have stopped all access to its buffers before returning.
//! * Every accepted Request yields one terminal Event, including cancellation and
//!   errors. Multishot accepts/reads may yield preceding nonterminal Events. Their
//!   terminal Event acknowledges that buffers and native op storage are quiescent.
//! * Native completions carry the **full OpId**, never a token or a reusable index
//!   alone. Delayed kernel events must not complete a later generation. IOCP must
//!   keep OVERLAPPED storage pinned until the final kernel acknowledgement.
//! * `cancel` initiates cancellation, never frees native buffers prematurely. It
//!   must eventually emit a terminal event even when cancel races with success.
//!   The core arbitrates the result and emits Cancelled if cancellation won.
//! * `prepare_close` starts nonblocking teardown before cancellation. Process
//!   backends terminate the owned child and retain its exit operation until reaped,
//!   including when cancellation is already pending. Other resources need no hook.
//! * The core calls `release` only after every associated operation terminated
//!   and the Closed completion was appended to host output. Drop must synchronously
//!   quiesce any remaining I/O before freeing buffers (native completion draining
//!   may be necessary). The driver never invokes user callbacks.
//!
//! # Turn and boundedness (D5–D7)
//!
//! * `poll` appends at most `events.capacity() - events.len()` Events; never grows
//!   the vector. A full buffer retains work for the next turn, without losing an
//!   edge. At most one OS wait is allowed (DESIGN §10 rule 3). Queued work
//!   prohibits blocking waits: the driver passes a zero timeout, and calls poll
//!   for that one discovery poll only with native operations pending and native
//!   output reserve. With no native operation pending, the driver skips poll for
//!   queued work, whatever `has_work` reports (web, whose poll never
//!   enters the OS, keeps draining cached host work). `None` means an unbounded
//!   wait. Durations must retain sub-millisecond precision.
//! * `Budget::accepts` bounds the connections multishot accepts deliver in one
//!   poll: nonterminal `Accepted`/`PipeAccepted` events (turnloop#77). It is the
//!   number of handles the core can still house for them, so each one past it
//!   would be accepted from the kernel and then destroyed. It throttles only
//!   those operations: every other source, single-shot accepts included, fills
//!   the vector exactly as before (DESIGN §10 rule 3 fairness). A multishot
//!   accept held back by it takes nothing further from the kernel (a readiness
//!   backend leaves the connection in the listener's backlog; a completion
//!   backend does not re-arm, and keeps an already completed one in its
//!   operation storage), and is neither runnable work for `has_work` nor a
//!   reason to end the wait early (rule 4a): the operation is resumed by the
//!   first poll whose budget is positive again.
//! * Readiness backends execute I/O in poll, cache readiness until EAGAIN, and
//!   requeue partially processed work fairly. Completion backends drain native
//!   completions. WASI 0.2 polls pollables; 0.3 drives a waitable set; web drains
//!   callback results and rejects blocking waits on the main thread.
//! * EINTR ends this poll early; it must not restart the timeout. `PollInfo.waits`
//!   counts positive-timeout/infinite waits; `discovery_polls` counts zero-time
//!   native polls. Their sum is at most one. `zero_event_waits`
//!   counts raw empty calls across both counters, including EINTR.
//!   Private timeout events count as zero-event waits, just like a timed OS wait
//!   returning zero; never infer this from user completions. Every backend that
//!   implements its timeout with a private wakeup source normalizes it: epoll's
//!   timerfd, the IOCP deadline packet, the WASI 0.2 deadline pollable and the
//!   WASI 0.3 deadline subtask. Real I/O or notifier events arriving in the same
//!   call still make it non-empty.
//! * Cached readiness ending in EAGAIN with no completion must retain the original
//!   timeout for the one permitted OS wait. It must not force a zero-timeout turn.
//! * `has_work` covers queued completions and cached runnable I/O. `wake` is called
//!   only after the notifier observed PARKED. It must be safe after loop drop:
//!   the wake object owns its native resource or detects closure, never uses a
//!   possibly reused raw handle. Errors cannot silently lose a wake.
//! * `integration` opts into external waiting. The core then leaves its notifier
//!   PARKED between turns; hosts call it again only while preparing external waits.
//!   This reconciles a readable integration fd with zero syscalls while RUNNING.
//!
//! # Transfer and platform constraints (§5a, §7)
//!
//! * `detach` is called only when all operations are quiescent. It unregisters the
//!   source loop before returning an owning Send transport. `attach` re-registers
//!   on the destination. Unsupported resources return Unsupported. For asynchronous
//!   cancellation, public detach can return WouldBlock while cancellation drains;
//!   the caller turns the source loop and retries. No blocking wait is hidden here.
//! * `Detached` may be an enum, not an integer: IOCP socket migration may require
//!   duplication; WASI/browser objects need not support transfer. `Resource` is
//!   intentionally absent: each backend owns its native tables keyed by Handle.
//! * A `Detached` is also how a descriptor leaves turnloop for good: the platform
//!   types expose `into_fd`/`into_socket`/`into_handle`, which restore whatever
//!   the backend changed on adoption (status flags, terminal or console mode) and
//!   then release ownership. Because `detach` already proved quiescence, the loop
//!   has no operation, buffer or registration left for that transport.
//! * `Wake` is Send + Sync on every target, but single-thread WASI/web may implement
//!   it as a flag/host scheduler token. No backend creates a loop-driving thread.
//!   Windows' explicitly requested Integration::Event helper is the D7 exception.
//! * Unsupported platforms/capabilities return errors; no fake successful I/O.
//!
//! # Socket options (§7.7)
//!
//! * `set_option`/`get_option` are synchronous and produce no Event. They run
//!   entirely inside the call, touch no operation storage, and must not allocate.
//! * A backend reports `Unsupported` for an option its platform lacks. It must
//!   never accept and ignore one: a host reads a lie back from `get_option`, and
//!   "the option is on" is a security- and latency-relevant claim.
//! * `ListenOpts::accept_defaults` is applied by the accepting backend to each
//!   accepted transport before its `Accepted`/`PipeAccepted` Outcome is produced,
//!   so the core hands the host an already-configured connection. A backend that
//!   cannot apply a requested default rejects the listener in `open`.
//!
//! # Filesystem (§7.6 Files)
//!
//! * `FILESYSTEM` selects where typed `FsRequest`s run. `Pool` (native default):
//!   the core runs them on the shared blocking pool and never calls the backend.
//!   `Backend` (WASI): `fs` accepts them under the ordinary Request rules, with a
//!   FIFO per handle, and yields `Outcome::Fs`. A request that opens binds its
//!   native object to the supplied new handle only on success; after a failed or
//!   cancelled open the core calls `release` for that never-visible handle.
//! * `fs_watch` binds a native watch to a new handle; the core then submits one
//!   multishot `Operation::WatchFs`. Each nonterminal `Outcome::Watch` carries a
//!   pooled lease of records (see `WatchEvents`). Pool exhaustion retains events
//!   in bounded per-watch storage; lost events set `overflow`. No backend thread
//!   is created: native watch sources are registered with the loop's own wait,
//!   except where the OS delivers on its own threads (FSEvents dispatch queues).
pub use crate::fs::FsOutput;
use crate::{
    BufLease, BufferPool, Config, Error, Handle, Integration, OpId, Open, ReadBuf, Result,
    WriteBuf, WriteVectored,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};

/// Only the notifier calls this method; implementations should expose syscall
/// counts here, counting attempts (including failures), not guessed notifications.
pub trait Wake: Send + Sync + 'static {
    /// Wake the native wait primitive; the notifier calls this only after observing PARKED.
    fn wake(&self) -> Result<()>;
    /// Return the number of native wake syscall attempts, including failures.
    fn syscall_count(&self) -> u64;
}

#[derive(Debug)]
/// One accepted backend operation, keyed by a full generational operation ID.
pub struct Request {
    /// Full generational operation identity, including its owning loop.
    pub op: OpId,
    /// Resource identity belonging to the submitting or receiving loop.
    pub handle: Handle,
    /// Requested operation and any memory it retains.
    pub operation: Operation,
}
#[derive(Debug)]
/// Backend requests with completion-shaped ownership and cancellation semantics.
pub enum Operation {
    /// Observe this process until its exit has been reaped.
    ProcessExit,
    /// Multishot subscription to this signal resource.
    WatchSignal,
    /// Duplicate a transport into the receiving process. The backend retains an
    /// independent native reference until its send completes or is cancelled.
    SendHandle(Handle),
    /// Receive one transport over a local IPC stream.
    RecvHandle,
    /// Establish the connection configured when the stream was opened.
    Connect,
    /// Accept one or repeatedly accept incoming stream connections.
    Accept {
        /// Continue producing nonterminal results until stopped, cancelled, EOF or error.
        multishot: bool,
    },
    /// Read bytes, with a byte count and optional pooled lease in the completion.
    Read {
        /// The owned/provided buffer for this operation.
        buf: ReadBuf,
        /// Continue producing nonterminal results until stopped, cancelled, EOF or error.
        multishot: bool,
    },
    /// Write the complete supplied byte buffer, permitting native partial writes internally.
    Write(WriteBuf),
    /// Write all inline segments in order.
    Writev(WriteVectored),
    /// Send one UDP datagram to the specified destination.
    SendTo {
        /// The owned/provided buffer for this operation.
        buf: WriteBuf,
        /// Destination UDP endpoint.
        to: SocketAddr,
    },
    /// Receive one UDP datagram and report its source address.
    RecvFrom(ReadBuf),
    /// Complete a stream write-side shutdown after preceding queued writes.
    Shutdown,
    /// Multishot delivery of the filesystem watch bound by `Backend::fs_watch`.
    WatchFs,
}
#[derive(Debug)]
/// Backend completion awaiting translation into a host-visible completion.
pub struct Event<D> {
    /// Full generational operation identity, including its owning loop.
    pub op: OpId,
    /// True once this operation will produce no further results or native buffer accesses.
    pub terminal: bool,
    /// The operation outcome or its original error.
    pub result: std::result::Result<Outcome<D>, Error>,
}
#[derive(Debug)]
/// Successful backend outcomes; accepted transports remain owned until attached.
pub enum Outcome<D> {
    /// Addresses from an asynchronous host resolver.
    Resolved(Vec<SocketAddr>),
    /// Reaped child termination.
    Exited(crate::ExitStatus),
    /// Coalesced signal delivery.
    Signal(crate::Signal),
    /// Accepted local connection (has no IP peer address).
    PipeAccepted(D),
    /// Received independent transport ownership.
    HandleReceived(D),
    /// One transport was passed successfully.
    HandleSent,
    /// The connection is established and ready for stream operations.
    Connected,
    /// An incoming TCP connection with its peer address.
    Accepted {
        /// Exclusive ownership of an unregistered accepted resource.
        transport: D,
        /// Remote TCP peer endpoint.
        peer: SocketAddr,
    },
    /// Read bytes, with a byte count and optional pooled lease in the completion.
    Read {
        /// Number of bytes transferred by this completion.
        n: usize,
        /// Owned pooled read bytes, present only when a pooled buffer was used.
        lease: Option<BufLease>,
    },
    /// The peer or file reached the end of its readable stream.
    Eof,
    /// Number of bytes successfully written.
    Wrote(usize),
    /// Receive one UDP datagram and report its source address.
    RecvFrom {
        /// Number of bytes transferred by this completion.
        n: usize,
        /// Source endpoint of the received datagram.
        from: SocketAddr,
        /// Owned pooled read bytes, present only when a pooled buffer was used.
        lease: Option<BufLease>,
    },
    /// Complete a stream write-side shutdown after preceding queued writes.
    Shutdown,
    /// A batch of filesystem watch records.
    Watch {
        /// Initialized `WatchEvents` records.
        events: BufLease,
        /// Native events were lost before this batch; rescan the watched scope.
        overflow: bool,
    },
    /// A typed filesystem result from a `Filesystem::Backend` backend.
    Fs {
        /// The typed result; `Opened` refers to the request's new handle.
        output: FsOutput,
        /// The pooled lease holding result bytes, when one was requested.
        lease: Option<BufLease>,
    },
    /// The operation was cancelled and its native buffer access has ended.
    Cancelled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Where typed filesystem requests execute (see the module documentation).
pub enum Filesystem {
    /// The core runs them on the shared native blocking pool.
    Pool,
    /// The backend accepts them through `Backend::fs`.
    Backend,
    /// No filesystem: `Loop::fs` returns Unsupported.
    Unsupported,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// What the core can house from one `poll` (see the module documentation).
pub struct Budget {
    /// Connections multishot accepts may deliver in this poll: the handle
    /// slots the core holds for them, which each such event spends.
    pub accepts: usize,
}
impl Budget {
    /// No limit on any source. For backend-level tests that bypass the core.
    pub const UNLIMITED: Self = Self {
        accepts: usize::MAX,
    };
    /// Whether this operation's deliveries are counted against `accepts`.
    pub fn throttles(operation: &Operation) -> bool {
        matches!(operation, Operation::Accept { multishot: true })
    }
    /// Whether this event spent one of `accepts`.
    pub fn spends<D>(event: &Event<D>) -> bool {
        !event.terminal
            && matches!(
                event.result,
                Ok(Outcome::Accepted { .. } | Outcome::PipeAccepted(_))
            )
    }
}
#[derive(Clone, Copy, Debug, Default)]
/// Instrumentation for the single bounded backend wait.
pub struct PollInfo {
    /// Blocking native wait invocations (positive timeout or infinite).
    pub waits: u32,
    /// Nonblocking native discovery invocations (zero timeout).
    /// `waits + discovery_polls` is at most one per poll.
    pub discovery_polls: u32,
    /// Native wait/discovery invocations with no I/O or notifier events, including EINTR.
    /// Bounded by `waits + discovery_polls`; retains revision-2 raw empty-poll accounting.
    /// Private timeout events are not native work: epoll's timerfd, the IOCP
    /// deadline packet, the WASI 0.2 deadline pollable and the WASI 0.3 deadline
    /// subtask all count as zero-event calls, while simultaneous I/O or notifier
    /// events keep the call non-empty.
    pub zero_event_waits: u32,
}

#[cfg(any(
    turnloop_backend = "epoll",
    turnloop_backend = "kqueue",
    turnloop_backend = "iocp",
    turnloop_backend = "wasi_p2",
    all(turnloop_backend = "wasi_p3", feature = "wasi-p3-experimental")
))]
impl PollInfo {
    /// Record one native wait/discovery call using its effective timeout.
    pub(crate) fn native(timeout: Option<Duration>, empty: bool) -> Self {
        let discovery = timeout == Some(Duration::ZERO);
        Self {
            waits: u32::from(!discovery),
            discovery_polls: u32::from(discovery),
            zero_event_waits: u32::from(empty),
        }
    }
}

/// # Safety
/// Implementers must uphold the buffer-lifetime and native-quiescence guarantees
/// above: no native I/O may access a buffer after its terminal Event or Drop.
/// Core lifetime and handle-release safety relies on those guarantees.
pub unsafe trait Backend: Sized + 'static {
    /// Thread-safe native wake endpoint type.
    type Wake: Wake;
    /// Owning transferable representation of an unregistered resource.
    type Detached: Send + 'static;
    /// Where typed filesystem requests run for this backend.
    const FILESYSTEM: Filesystem = if cfg!(target_arch = "wasm32") {
        Filesystem::Unsupported
    } else {
        Filesystem::Pool
    };
    /// Construct backend storage and native wait resources with the supplied shared read pool.
    fn new(config: &Config, pool: BufferPool) -> Result<Self>;
    /// Adopt an exclusively owned, bound but unconnected TCP transport and
    /// configure its peer before a Connect request. Preserve the local binding
    /// and existing socket options; apply nodelay when requested. On error,
    /// retain no resource or native registration. Backends without native
    /// transport adoption return Unsupported.
    fn open_bound_tcp(
        &mut self,
        _handle: Handle,
        _transport: Self::Detached,
        _peer: SocketAddr,
        _opts: crate::TcpOpts,
    ) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Accept a typed filesystem request (only with `Filesystem::Backend`). `handle`
    /// is the request's file handle, or the new handle for an open.
    fn fs(&mut self, _op: OpId, _handle: Option<Handle>, _request: crate::FsRequest) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Bind a native filesystem watch to a new handle, retaining nothing on error.
    fn fs_watch(
        &mut self,
        _handle: Handle,
        _path: &crate::FsPath,
        _options: crate::WatchOptions,
    ) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Give process-wide services this loop's parking-aware notification endpoint.
    fn set_notifier(&mut self, _notifier: crate::Notifier) {}
    /// Spawn and bind the child and requested parent pipe handles atomically.
    /// Failure must release all supplied handles and reap any created child.
    ///
    /// `extra` is parallel to `spec.extra`: a handle is supplied for every entry
    /// whose source has a parent end, and `None` for the rest. The core has
    /// already checked descriptor numbers and duplicates.
    fn spawn(
        &mut self,
        _handle: Handle,
        _pipes: [Option<Handle>; 3],
        _extra: &[Option<Handle>],
        _spec: &crate::ProcessSpec,
    ) -> Result<u32> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Begin nonblocking resource teardown before the core cancels its operations.
    /// Process backends terminate the owned child/tree here and defer its terminal
    /// acknowledgement until exit/reaping, so release never blocks a normal turn.
    /// Other resources need no action. An error leaves the core handle open.
    fn prepare_close(&mut self, _handle: Handle) -> Result<()> {
        Ok(())
    }
    /// Signal an owned child, or its explicitly isolated process group.
    fn kill(&mut self, _handle: Handle, _signal: crate::Signal, _group: bool) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Bind a process-wide signal subscription to a new core handle.
    fn signal(&mut self, _handle: Handle, _signal: crate::Signal) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Set a terminal mode, retaining the original settings until release/drop.
    fn tty_set_mode(&mut self, _handle: Handle, _mode: crate::TtyMode) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Apply one socket option to a live resource, synchronously, through the OS.
    /// No Request is accepted and no Event is produced. A platform without an
    /// equivalent returns Unsupported; silently ignoring an option is forbidden,
    /// because a host cannot tell an ignored option from an applied one.
    fn set_option(&mut self, _handle: Handle, _option: crate::SocketOption) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Synchronously request native receive-side shutdown without closing the
    /// send direction. Existing accepted requests retain their normal buffer
    /// ownership and terminal acknowledgement requirements. Unsupported transports
    /// must fail without changing either direction.
    fn shutdown_receive(&mut self, _handle: Handle) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Read one socket option from the OS. Backends never answer from a cache of
    /// what was set: the kernel may round, clamp or double a request, and the
    /// caller is entitled to the value the kernel actually holds.
    fn get_option(
        &self,
        _handle: Handle,
        _kind: crate::SocketOptionKind,
    ) -> Result<crate::SocketOption> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Report the native identity of a live transport, for the host only.
    ///
    /// The core never acts on the value and never stores it: it reads it out of
    /// the backend's own table and returns it, so a host can implement Node's
    /// `socket._handle.fd`. Backends return Unsupported for a resource with no
    /// descriptor identity of its own (timers, processes, signals, watches, and
    /// every WASI/web resource). See `Driver::raw_transport` for the host rules.
    fn raw_transport(&self, _handle: Handle) -> Result<crate::RawTransport> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Query current terminal dimensions.
    fn tty_window_size(&self, _handle: Handle) -> Result<crate::WindowSize> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Monotonic, nondecreasing time in a consistent domain across calls. Native
    /// adapters return std::time::Instant::now(). Web adapters construct
    /// crate::Instant::from_duration from their monotonic host clock. This method
    /// must not schedule work, block, or invoke a user callback.
    fn now(&self) -> crate::Instant;
    /// Validate the original request even when queued completions eliminate the
    /// effective wait. Browser main-thread adapters reject every timeout but Now;
    /// a worker adapter may support blocking. Native adapters use this default.
    fn validate_timeout(&self, _timeout: crate::Timeout) -> Result<()> {
        Ok(())
    }
    /// The core owns timer semantics. Callback-driven adapters arm one host timer
    /// for this earliest deadline, whose callback schedules a turn. None disarms
    /// it. Native pollers need no action: poll receives the effective timeout.
    /// This is a scheduling import, never a user callback. If host scheduling
    /// fails, preserve the failure and return it from poll; never fake expiry.
    fn deadline_changed(&mut self, _deadline: Option<crate::Instant>) {}
    /// Clone the lifetime-safe native wake endpoint for this backend.
    fn waker(&self) -> Arc<Self::Wake>;
    /// Bind a new resource to the supplied handle, retaining nothing on error.
    fn open(&mut self, handle: Handle, spec: Open) -> Result<()>;
    /// Return the local IP endpoint of a bound socket.
    fn local_addr(&self, handle: Handle) -> Result<SocketAddr>;
    /// Accept one operation or reject it before retaining native buffer access.
    fn submit(&mut self, request: Request) -> Result<()>;
    /// Start a handle-free host name lookup. The default uses no resources and
    /// lets the core fall back to its native blocking pool. Accepted requests
    /// obey the same terminal/cancellation and no-spin rules as socket I/O.
    fn resolve(&mut self, _op: OpId, _request: &crate::DnsRequest) -> Result<()> {
        Err(Error::new(crate::ErrorKind::Unsupported))
    }
    /// Request cancellation; terminal acknowledgement must precede memory reuse.
    fn cancel(&mut self, op: OpId) -> Result<()>;
    /// Whether completions or immediately runnable cached operations are available.
    fn has_work(&self) -> bool;
    /// Append bounded completions, performing at most one OS wait with the
    /// supplied exact timeout, and delivering no more multishot connections
    /// than `budget` allows.
    fn poll(
        &mut self,
        timeout: Option<Duration>,
        budget: Budget,
        events: &mut Vec<Event<Self::Detached>>,
    ) -> Result<PollInfo>;
    /// Release a quiescent resource after Closed was appended to host output.
    fn release(&mut self, handle: Handle);
    /// Unregister a quiescent resource and return its owning transferable representation.
    fn detach(&mut self, handle: Handle) -> Result<Self::Detached>;
    /// Register an owning transport on this loop; failure drops the rejected transport.
    fn attach(&mut self, handle: Handle, transport: Self::Detached) -> Result<()>;
    /// Opt into external host waiting and return the borrowed integration primitive.
    fn integration(&mut self) -> Result<Integration>;
}

#[cfg(turnloop_backend = "epoll")]
mod epoll;
#[cfg(turnloop_backend = "kqueue")]
mod kqueue;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod poller;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod socket;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod sockopt;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
pub mod unix;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
pub use unix::Unix as Platform;

#[cfg(turnloop_backend = "iocp")]
pub mod iocp;
#[cfg(turnloop_backend = "iocp")]
pub use iocp::Iocp as Platform;

#[cfg(any(
    turnloop_backend = "wasi_p2",
    all(turnloop_backend = "wasi_p3", feature = "wasi-p3-experimental")
))]
mod wasi_fs;
#[cfg(turnloop_backend = "wasi_p2")]
pub mod wasi_p2;
#[cfg(turnloop_backend = "wasi_p2")]
pub use wasi_p2::WasiP2 as Platform;

#[cfg(turnloop_backend = "web")]
pub mod web;
#[cfg(turnloop_backend = "web")]
pub use web::Web as Platform;

#[cfg(all(turnloop_backend = "wasi_p3", feature = "wasi-p3-experimental"))]
pub mod wasi_p3;
#[cfg(all(turnloop_backend = "wasi_p3", feature = "wasi-p3-experimental"))]
pub use wasi_p3::WasiP3 as Platform;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod ipc;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod process;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod stdio;

#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod services;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod signals;

#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod files;
#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
mod watch;
