use crate::{
    backend::{Backend, Budget, Event, Filesystem, Operation, Outcome, Request},
    fs::FsOutput,
    table::Table,
    timer::DriverTimerQueue as TimerQueue,
    *,
};
use std::{
    collections::VecDeque,
    marker::PhantomData,
    net::SocketAddr,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, ThreadId},
    time::Duration,
};
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
const NATIVE_EVENTS: usize = 0;
const TIMER_EVENTS: usize = 1;
const POST_EVENTS: usize = 2;
#[derive(Clone, Copy)]
enum Kind {
    Socket,
    /// A typed file or directory handle served by the native pool service.
    File,
    Timer {
        op: Option<OpId>,
        repeat: Option<Duration>,
    },
}
struct Resource {
    kind: Kind,
    /// Reserved by an in-flight open; addressable only after `Opened`.
    hidden: bool,
    referenced: bool,
    pending: usize,
    closing: Option<Token>,
    closed_queued: bool,
    head: Option<OpId>,
    tail: Option<OpId>,
}
#[derive(Clone)]
struct Op {
    handle: Option<Handle>,
    token: Token,
    cancel: bool,
    timed_out: bool,
    stop: bool,
    external_wait: bool,
    /// A typed filesystem request (pool service or backend, per `B::FILESYSTEM`).
    fs: bool,
    /// Handle slots this operation holds against the ceiling, released when it
    /// retires. Non-zero only for single-shot operations whose completion
    /// creates a handle; a multishot accept draws on `multishot_reserved`.
    reserved_handles: usize,
    /// A multishot accept, counted in `multishot_armed`, whose connections the
    /// backend delivers against the poll's `Budget` (turnloop#77).
    multishot: bool,
    /// Counted in `native_pending`: a socket-handle operation or a request the
    /// backend accepted natively (such as WASI DNS). DESIGN §10 rule 3 keys
    /// queued-turn discovery on these operations.
    native: bool,
    /// Delivers through the loop's result ring (`WorkPort`), and so holds one
    /// of `PoolConfig::max_undelivered` credits until it retires.
    port: bool,
    job_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    previous: Option<OpId>,
    next: Option<OpId>,
}

struct Queued {
    completion: Completion,
    referenced: bool,
    event_class: Option<usize>,
}
impl std::ops::Deref for Queued {
    type Target = Completion;
    fn deref(&self) -> &Completion {
        &self.completion
    }
}

/// A driver belongs to its constructing thread. Backend is an internal extension
/// point used by platform lanes and the common contract suite.
pub struct Driver<B: Backend> {
    backend: B,
    notifier: Notifier,
    poster: Poster,
    external: bool,
    work_port: std::sync::Arc<crate::blocking::WorkPort>,
    #[cfg(not(target_arch = "wasm32"))]
    files: crate::fs::Service,
    metadata: crate::fs::MetadataPool,
    owner: u64,
    thread: ThreadId,
    handles: Table<Resource>,
    ops: Table<Op>,
    timers: TimerQueue,
    connect_deadlines: TimerQueue,
    queued: VecDeque<Queued>,
    /// Largest number of completions that may be queued at once. Was implicit in
    /// `queued`'s reserved capacity, which is now a page that grows on demand.
    completion_capacity: usize,
    buffered: [usize; 3],
    events: Vec<Event<B::Detached>>,
    refs: usize,
    outstanding: usize,
    /// Handle slots promised to accepts and handle receives that have been asked
    /// for but not yet delivered. See [`Driver::submit`].
    reserved_handles: usize,
    /// Armed multishot accepts, and the part of `reserved_handles` promised to
    /// them: one each, as far as the ceiling allows. The promises are pooled
    /// rather than owned per operation, because the poll budget is shared by
    /// every multishot accept, and any of them may spend any of these slots.
    multishot_armed: usize,
    multishot_reserved: usize,
    native_pending: usize,
    /// Operations delivering through `work_port` that have not retired: the
    /// ring's occupancy can never exceed this, and admission keeps it at or
    /// below `PoolConfig::max_undelivered`, which sizes the ring (issue #88).
    undelivered: usize,
    config: Config,
    _local: PhantomData<Rc<()>>,
}
impl<B: Backend> Driver<B> {
    /// Construct a loop on this thread and allocate its fixed operation and completion storage.
    pub fn new(config: Config) -> Result<Self> {
        if config.max_handles == 0
            || config.max_handles > u32::MAX as usize
            || config.max_operations == 0
            || config.max_operations > u32::MAX as usize
            || config.events_per_turn == 0
            || config.post_capacity == 0
            || config.pooled_buffer_size == 0
            || config.blocking_pool.max_undelivered == 0
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let completion_capacity = config
            .events_per_turn
            .checked_mul(3)
            .and_then(|n| n.checked_add(config.max_operations))
            .and_then(|n| n.checked_add(config.max_handles))
            .ok_or(Error::new(ErrorKind::InvalidInput))?;
        let buffers = BufferPool::new(config.pooled_buffers, config.pooled_buffer_size);
        let mut backend = B::new(&config, buffers.clone())?;
        let notifier = Notifier::new(backend.waker());
        backend.set_notifier(notifier.clone());
        let poster = Poster::new(config.post_capacity, notifier.clone());
        // Sized by the results that can be undelivered at once, not by every
        // operation the loop may hold: see `PoolConfig::max_undelivered`.
        let work_port = crate::blocking::WorkPort::new(
            config
                .blocking_pool
                .max_undelivered
                .min(config.max_operations),
            notifier.clone(),
        );
        let owner = loop {
            let current = NEXT_OWNER.load(Ordering::Relaxed);
            let next = current
                .checked_add(1)
                .ok_or(Error::new(ErrorKind::ResourceLimit))?;
            if NEXT_OWNER
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break current;
            }
        };
        Ok(Self {
            backend,
            notifier,
            poster,
            external: false,
            #[cfg(not(target_arch = "wasm32"))]
            files: crate::fs::Service::new(&config, work_port.clone(), buffers),
            metadata: crate::fs::MetadataPool::new(config.max_operations),
            work_port,
            owner,
            thread: thread::current().id(),
            handles: Table::new(config.max_handles),
            ops: Table::new(config.max_operations),
            timers: TimerQueue::new(config.max_handles),
            connect_deadlines: TimerQueue::new(config.max_operations),
            // Terminal operation and Closed credits live until delivery. Native
            // multishot events, repeating timers and posts each have their own
            // bounded reserve, so no source can consume cancellation capacity or
            // prevent another source from making progress with small host output.
            // The bound is `completion_capacity`, checked on every push; the
            // queue reserves a page of it and grows as a turn's backlog does.
            queued: VecDeque::with_capacity(crate::slots::page_reserve(completion_capacity)),
            completion_capacity,
            buffered: [0; 3],
            events: Vec::with_capacity(config.events_per_turn),
            refs: 0,
            outstanding: 0,
            reserved_handles: 0,
            multishot_armed: 0,
            multishot_reserved: 0,
            native_pending: 0,
            undelivered: 0,
            config,
            _local: PhantomData,
        })
    }
    /// Rebuild this loop from a new `config`, in place (issue #43).
    ///
    /// A loop's configuration is fixed when it is built, so raising a profile —
    /// a process that starts doing network I/O, say — means building a new loop.
    /// Dropping the old one to do that is a cancellation nobody asked for: a pool
    /// job it accepted still runs, and its result is discarded because its loop
    /// is gone, so the job's completion never arrives. This is the checked way
    /// to replace a loop, which refuses instead of losing anything.
    ///
    /// The loop is rebuilt only when it owns nothing a new loop could not
    /// deliver: no handle, no operation in flight (pool jobs, pool lookups,
    /// typed file requests and external waits included), no completion awaiting
    /// delivery and no queued post. Otherwise this is `WouldBlock` and nothing
    /// changes: turn the loop until that work has been delivered, then retry —
    /// the same contract as [`detach`](Self::detach). A `config` that
    /// [`Driver::new`] would reject fails the same way, also without changing
    /// anything.
    ///
    /// On success this is a new loop. Handles, operation ids, [`Notifier`]s,
    /// [`Poster`]s and an [`integration`](Self::integration) primitive obtained
    /// before belong to the old one and no longer reach it: fetch new ones.
    /// Quiesce other threads' posters first: a post that lands between the
    /// check and the rebuild goes down with the old loop, exactly as it would if
    /// the loop were dropped, and later ones are refused with `NotFound`.
    ///
    /// Growing the old loop in place instead is not offered: its capacities size
    /// structures that cannot grow under live use — the lock-free post and
    /// result rings that other threads are writing into, and the Windows
    /// `OVERLAPPED` slab whose addresses the kernel holds.
    pub fn rebuild(&mut self, config: Config) -> Result<()> {
        self.assert_owner();
        if self.outstanding != 0
            || self.handles.remaining() != self.config.max_handles
            || !self.queued.is_empty()
            || !self.poster.is_empty()
            || !self.work_port.is_empty()
        {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        debug_assert_eq!(self.undelivered, 0, "every pool operation is outstanding");
        *self = Self::new(config)?;
        Ok(())
    }
    /// Reserved slots are a subset of free ones.
    ///
    /// Every path that moves either side keeps this: a reserving submission
    /// checks it before promising, `new_handle` refuses to spend a promised
    /// slot, and a delivery releases its promise before taking the slot. Drift
    /// here means an armed accept can arrive with nowhere to put its connection,
    /// which is silent until a connection is destroyed for it.
    fn assert_reservations(&self) {
        debug_assert!(
            self.reserved_handles <= self.handles.remaining(),
            "{} handle slots promised but only {} free",
            self.reserved_handles,
            self.handles.remaining()
        );
    }
    fn assert_owner(&self) {
        debug_assert_eq!(
            self.thread,
            thread::current().id(),
            "completion on foreign thread"
        );
    }
    fn resource(&self, h: Handle) -> Result<&Resource> {
        if h.owner != self.owner {
            return Err(Error::new(ErrorKind::NotFound));
        }
        self.handles
            .get(h.key)
            .filter(|r| !r.hidden)
            .ok_or(Error::new(ErrorKind::NotFound))
    }
    /// Take a handle slot for a resource the host is creating.
    ///
    /// Slots promised to armed accepts are not available here: an accept that the
    /// kernel has been asked to perform must still have somewhere to put its
    /// connection when it arrives, so a host creating handles cannot spend the
    /// last one out from under it. The completion path consumes its own
    /// reservation first, in [`Driver::attach_reserved`].
    fn new_handle(&mut self, kind: Kind) -> Result<Handle> {
        if self.handles.remaining() <= self.reserved_handles {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        let key = self
            .handles
            .insert(Resource {
                kind,
                hidden: false,
                referenced: true,
                pending: 0,
                closing: None,
                closed_queued: false,
                head: None,
                tail: None,
            })
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        if matches!(kind, Kind::Socket | Kind::File) {
            self.refs += 1;
        }
        self.assert_reservations();
        Ok(Handle {
            owner: self.owner,
            key,
        })
    }
    fn new_op(&mut self, h: Option<Handle>, token: Token) -> Result<OpId> {
        if self.outstanding == self.config.max_operations {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        let previous = h.and_then(|h| self.handles.get(h.key).and_then(|r| r.tail));
        let native = h
            .and_then(|h| self.handles.get(h.key))
            .is_some_and(|r| matches!(r.kind, Kind::Socket));
        let key = self
            .ops
            .insert(Op {
                handle: h,
                token,
                cancel: false,
                timed_out: false,
                stop: false,
                job_cancel: None,
                external_wait: false,
                fs: false,
                reserved_handles: 0,
                multishot: false,
                native,
                port: false,
                previous,
                next: None,
            })
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        let id = OpId {
            owner: self.owner,
            key,
        };
        if let Some(previous) = previous {
            self.ops.get_mut(previous.key).expect("previous").next = Some(id);
        }
        if let Some(h) = h {
            let r = self.handles.get_mut(h.key).expect("validated handle");
            if r.head.is_none() {
                r.head = Some(id);
            }
            r.tail = Some(id);
            r.pending += 1;
            if r.referenced {
                self.refs += 1;
            }
        } else {
            self.refs += 1;
        }
        self.native_pending += usize::from(native);
        self.outstanding += 1;
        Ok(OpId {
            owner: self.owner,
            key,
        })
    }
    /// Refuse a submission whose result would have no room in the result ring.
    ///
    /// Checked before anything is created, like the pool's own queue limit, so
    /// a refused submission owes no completion. See `PoolConfig::max_undelivered`.
    fn admit_port(&self) -> Result<()> {
        if self.undelivered >= self.config.blocking_pool.max_undelivered {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        Ok(())
    }
    /// Charge an admitted operation to the result ring until it retires.
    fn charge_port(&mut self, op: OpId) {
        let op = self.ops.get_mut(op.key).expect("admitted op");
        debug_assert!(!op.port);
        op.port = true;
        self.undelivered += 1;
        debug_assert!(self.undelivered <= self.config.blocking_pool.max_undelivered);
    }
    fn retire(&mut self, id: OpId) -> Option<Op> {
        let op = self.ops.remove(id.key)?;
        self.connect_deadlines.cancel(id.key);
        self.reserved_handles -= op.reserved_handles;
        if op.multishot {
            self.multishot_armed -= 1;
            if self.multishot_reserved > self.multishot_armed {
                self.multishot_reserved -= 1;
                self.reserved_handles -= 1;
            }
        }
        if op.native {
            self.native_pending -= 1;
        }
        if op.port {
            self.undelivered -= 1;
        }
        if let Some(previous) = op.previous {
            self.ops.get_mut(previous.key).expect("previous").next = op.next;
        }
        if let Some(next) = op.next {
            self.ops.get_mut(next.key).expect("next").previous = op.previous;
        }
        if let Some(h) = op.handle {
            if let Some(r) = self.handles.get_mut(h.key) {
                if r.head == Some(id) {
                    r.head = op.next;
                }
                if r.tail == Some(id) {
                    r.tail = op.previous;
                }
                r.pending -= 1;
                if r.referenced {
                    self.refs -= 1;
                }
                if let Kind::Timer { op, .. } = &mut r.kind {
                    *op = None;
                }
            }
        } else {
            self.refs -= 1;
        }
        Some(op)
    }
    fn maybe_closed(&mut self, h: Handle) {
        if let Some(r) = self.handles.get_mut(h.key)
            && r.pending == 0
            && !r.closed_queued
            && let Some(token) = r.closing
        {
            r.closed_queued = true;
            self.enqueue(
                Completion {
                    token,
                    op: None,
                    handle: Some(h),
                    terminal: true,
                    result: OpResult::Closed,
                },
                false,
            );
        }
    }
    fn enqueue(&mut self, completion: Completion, referenced: bool) {
        let event_class = match completion.result {
            OpResult::Posted(_) => Some(POST_EVENTS),
            OpResult::Timer if !completion.terminal => Some(TIMER_EVENTS),
            _ if !completion.terminal => Some(NATIVE_EVENTS),
            _ => None,
        };
        if let Some(class) = event_class {
            debug_assert!(self.buffered[class] < self.config.events_per_turn);
            self.buffered[class] += 1;
        }
        debug_assert!(self.queued.len() < self.completion_capacity);
        self.refs += usize::from(referenced);
        self.queued.push_back(Queued {
            completion,
            referenced,
            event_class,
        });
    }
    fn finish(&mut self, id: OpId, result: OpResult, terminal: bool) {
        let Some(op) = self.ops.get(id.key).cloned() else {
            return;
        };
        let referenced = terminal
            && op
                .handle
                .is_none_or(|h| self.handles.get(h.key).is_some_and(|r| r.referenced));
        self.enqueue(
            Completion {
                token: op.token,
                op: Some(id),
                handle: op.handle,
                terminal,
                result,
            },
            referenced,
        );
        if terminal {
            self.retire(id);
            if let Some(h) = op.handle {
                self.maybe_closed(h);
            }
        }
    }
    /// Return O(1) reference-counted liveness, including undelivered terminal completions.
    pub fn alive(&self) -> bool {
        self.refs != 0
    }
    /// The backend monotonic clock; portable deadline construction starts here.
    pub fn now(&self) -> Instant {
        self.backend.now()
    }
    /// Return the earliest pending timer or connection deadline in the backend clock domain.
    pub fn next_deadline(&self) -> Option<Instant> {
        let deadline = match (
            self.timers.next_deadline(),
            self.connect_deadlines.next_deadline(),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        #[cfg(target_arch = "wasm32")]
        let deadline = match (deadline, crate::external_wait::deadline(self.owner)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        deadline
    }
    /// Include or exclude a handle and its pending/queued operations from loop liveness.
    pub fn set_ref(&mut self, h: Handle, referenced: bool) -> Result<()> {
        let r = self.resource(h)?;
        let weight = r.pending
            + usize::from(matches!(r.kind, Kind::Socket | Kind::File) || r.closing.is_some());
        if r.referenced != referenced {
            if referenced {
                self.refs += weight;
            } else {
                self.refs -= weight;
            }
            self.handles
                .get_mut(h.key)
                .expect("validated handle")
                .referenced = referenced;
            // Terminal results retain the operation reference through delivery,
            // including after the native operation has left the active table.
            for q in &mut self.queued {
                if q.handle == Some(h) && q.terminal && q.op.is_some() && q.referenced != referenced
                {
                    if referenced {
                        self.refs += 1;
                    } else {
                        self.refs -= 1;
                    }
                    q.referenced = referenced;
                }
            }
        }
        Ok(())
    }
    /// Create a timer and its operation; repeats coalesce missed intervals and must be nonzero.
    pub fn timer(&mut self, at: Instant, repeat: Option<Duration>, token: Token) -> Result<Handle> {
        if repeat.is_some_and(|r| r.is_zero()) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let h = self.new_handle(Kind::Timer { op: None, repeat })?;
        let op = match self.new_op(Some(h), token) {
            Ok(op) => op,
            Err(e) => {
                self.handles.remove(h.key);
                return Err(e);
            }
        };
        self.handles.get_mut(h.key).expect("inserted").kind = Kind::Timer {
            op: Some(op),
            repeat,
        };
        self.timers.insert(h.key, at);
        self.backend.deadline_changed(self.next_deadline());
        Ok(h)
    }
    /// Move an active timer deadline; returns false for closed, expired or foreign handles.
    pub fn timer_reset(&mut self, h: Handle, at: Instant) -> bool {
        let Ok(r) = self.resource(h) else {
            return false;
        };
        if r.closing.is_some() || !matches!(r.kind, Kind::Timer { op: Some(_), .. }) {
            return false;
        }
        self.timers.cancel(h.key);
        self.timers.insert(h.key, at);
        self.backend.deadline_changed(self.next_deadline());
        true
    }
    /// Return the operation identity of an active timer, for cancellation.
    pub fn timer_op(&self, h: Handle) -> Option<OpId> {
        match self.resource(h).ok()?.kind {
            Kind::Timer { op, .. } => op,
            _ => None,
        }
    }
    fn open(&mut self, spec: Open) -> Result<Handle> {
        let h = self.new_handle(Kind::Socket)?;
        if let Err(e) = self.backend.open(h, spec) {
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(e);
        }
        Ok(h)
    }
    /// Bind and listen for TCP connections; port zero chooses an ephemeral port.
    pub fn tcp_listen(&mut self, addr: SocketAddr, opts: &ListenOpts) -> Result<Handle> {
        self.open(Open::Listener { addr, opts: *opts })
    }
    /// Listen for local IPC streams. Unix socket path removal belongs to the host.
    pub fn pipe_listen(&mut self, name: &PipeName, opts: &ListenOpts) -> Result<Handle> {
        self.open(Open::PipeListener {
            name: name.clone(),
            opts: *opts,
        })
    }
    /// Connect a local stream, completing with Connected or an error. Busy named
    /// pipes wait asynchronously until available or cancelled by closing the handle.
    pub fn pipe_connect(&mut self, name: &PipeName, token: Token) -> Result<Handle> {
        self.pipe_connect_inner(name, None, token)
    }
    /// Connect a local stream by an absolute loop-clock deadline. Expiry cancels
    /// native I/O and reports TimedOut only after its cancellation acknowledgement.
    /// The deadline covers connection completion, not subsequent stream I/O.
    pub fn pipe_connect_until(
        &mut self,
        name: &PipeName,
        deadline: Instant,
        token: Token,
    ) -> Result<Handle> {
        self.pipe_connect_inner(name, Some(deadline), token)
    }
    fn pipe_connect_inner(
        &mut self,
        name: &PipeName,
        deadline: Option<Instant>,
        token: Token,
    ) -> Result<Handle> {
        let h = self.open(Open::Pipe(name.clone()))?;
        match self.submit(h, Operation::Connect, token) {
            Ok(op) => {
                if let Some(at) = deadline {
                    self.connect_deadlines.insert(op.key, at);
                    self.backend.deadline_changed(self.next_deadline());
                }
                Ok(h)
            }
            Err(e) => {
                self.backend.release(h);
                self.handles.remove(h.key);
                self.refs -= 1;
                Err(e)
            }
        }
    }
    fn expire_connects(&mut self, now: Instant) -> Result<()> {
        while let Some((key, at)) = self.connect_deadlines.pop_expired(now) {
            let op = OpId {
                owner: self.owner,
                key,
            };
            if let Err(error) = self.backend.cancel(op) {
                // Preserve both the deadline and original cancellation error;
                // no premature timeout may release in-flight kernel storage.
                self.connect_deadlines.insert(key, at);
                return Err(error);
            }
            let op = self.ops.get_mut(key).expect("cancelled connect");
            op.cancel = true;
            op.timed_out = true;
        }
        Ok(())
    }
    /// Duplicate standard input, output or error, classifying pipe/file/terminal.
    ///
    /// Unix duplicates share nonblocking flags and terminal settings. Overlapping
    /// managed standard-I/O transports, even on different loops, keep those
    /// settings until the last transport is released; then the original settings
    /// are restored. Do not replace or reconfigure process standard descriptors
    /// (or their aliases) while any managed standard-I/O transport is live.
    pub fn open_stdio(&mut self, which: Stdio) -> Result<Handle> {
        self.open(Open::Stdio(which))
    }
    /// Pass an independent reference to a socket over local IPC. The source stays
    /// owned by this loop; close or detach it explicitly when migration is desired.
    pub fn send_handle(&mut self, pipe: Handle, h: Handle, token: Token) -> Result<OpId> {
        if self.resource(h)?.closing.is_some() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.submit(pipe, Operation::SendHandle(h), token)
    }
    /// Receive and attach one passed socket. Use a dedicated IPC control stream;
    /// ordinary reads must not consume the handle-transfer framing byte.
    pub fn recv_handle(&mut self, pipe: Handle, token: Token) -> Result<OpId> {
        self.submit(pipe, Operation::RecvHandle, token)
    }
    /// Spawn a child and submit its exactly-once exit operation. Closing a live
    /// child initiates termination, then waits through ordinary turns for reaping
    /// before Cancelled and Closed; a new process group enables `kill_group`.
    ///
    /// A spec carrying [`ProcessSpec::extra`] descriptors is `InvalidInput` here,
    /// because their parent ends have nowhere to be reported; use
    /// [`Driver::spawn_extra`].
    pub fn spawn(&mut self, spec: &ProcessSpec, token: Token) -> Result<Process> {
        self.spawn_extra(spec, token, &mut [])
    }
    /// Spawn a child that also receives [`ProcessSpec::extra`] descriptors.
    ///
    /// `parents` must have exactly one slot per [`ChildFd`], in the same order.
    /// Each slot receives the parent end of that descriptor: a readable handle
    /// for [`ChildFdSource::Pipe`], a readable and writable one for
    /// [`ChildFdSource::Duplex`], and `None` for a source that has no parent end.
    /// Every returned handle belongs to this loop and is closed like any other.
    ///
    /// Descriptor numbers are the child's own, run from 3 to 255, and must not
    /// repeat. The child's view of them is a plain descriptor number on Unix and
    /// a C run-time descriptor on Windows, so a `NODE_CHANNEL_FD`-style handoff
    /// is the same environment entry on both: set it in [`ProcessSpec::env`].
    ///
    /// On failure nothing is created: no child, no handle, no completion.
    pub fn spawn_extra(
        &mut self,
        spec: &ProcessSpec,
        token: Token,
        parents: &mut [Option<Handle>],
    ) -> Result<Process> {
        if parents.len() != spec.extra.len() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        for (i, fd) in spec.extra.iter().enumerate() {
            if !(3..=MAX_CHILD_FD).contains(&fd.number)
                || spec.extra[..i].iter().any(|seen| seen.number == fd.number)
            {
                return Err(Error::new(ErrorKind::InvalidInput));
            }
        }
        if spec.controlling_terminal && !spec.detached {
            // TIOCSCTTY only succeeds for a session leader without a terminal.
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        parents.fill(None);
        let h = self.new_handle(Kind::Socket)?;
        let mut pipes = [None; 3];
        let result = self.spawn_inner(h, spec, token, &mut pipes, parents);
        match result {
            Ok(pid) => Ok(Process {
                handle: h,
                pid,
                stdin: pipes[0],
                stdout: pipes[1],
                stderr: pipes[2],
            }),
            Err(e) => {
                for handle in std::iter::once(h)
                    .chain(pipes.into_iter().flatten())
                    .chain(parents.iter().flatten().copied())
                {
                    self.backend.release(handle);
                    if self.handles.remove(handle.key).is_some() {
                        self.refs -= 1;
                    }
                }
                parents.fill(None);
                Err(e)
            }
        }
    }
    fn spawn_inner(
        &mut self,
        h: Handle,
        spec: &ProcessSpec,
        token: Token,
        pipes: &mut [Option<Handle>; 3],
        parents: &mut [Option<Handle>],
    ) -> Result<u32> {
        for (i, stdio) in spec.stdio.iter().enumerate() {
            if let ProcessStdio::Handle(source) = stdio {
                self.resource(*source)?;
            }
            if *stdio == ProcessStdio::Pipe {
                pipes[i] = Some(self.new_handle(Kind::Socket)?);
            }
        }
        for (slot, fd) in parents.iter_mut().zip(&spec.extra) {
            match fd.source {
                ChildFdSource::Handle(source) => {
                    self.resource(source)?;
                }
                ChildFdSource::Pipe | ChildFdSource::Duplex => {
                    *slot = Some(self.new_handle(Kind::Socket)?);
                }
                ChildFdSource::Null => {}
            }
        }
        // Reserve the terminal completion before creating an OS child.
        let op = self.new_op(Some(h), token)?;
        let result = self
            .backend
            .spawn(h, *pipes, parents, spec)
            .and_then(|pid| {
                self.backend.submit(Request {
                    op,
                    handle: h,
                    operation: Operation::ProcessExit,
                })?;
                Ok(pid)
            });
        if result.is_err() {
            self.retire(op);
            self.outstanding -= 1;
        }
        result
    }
    /// Signal a child still owned by this loop.
    pub fn kill(&mut self, process: Handle, signal: Signal) -> Result<()> {
        self.resource(process)?;
        self.backend.kill(process, signal, false)
    }
    /// Signal an isolated process group, including grandchildren.
    pub fn kill_group(&mut self, process: Handle, signal: Signal) -> Result<()> {
        self.resource(process)?;
        self.backend.kill(process, signal, true)
    }
    /// Subscribe this loop to a process-wide signal. Repeated deliveries may
    /// coalesce; every subscribed loop gets its own completion.
    pub fn signal_start(&mut self, signal: Signal, token: Token) -> Result<Handle> {
        let h = self.new_handle(Kind::Socket)?;
        if let Err(e) = self
            .backend
            .signal(h, signal)
            .and_then(|()| self.submit(h, Operation::WatchSignal, token).map(|_| ()))
        {
            self.backend.release(h);
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(e);
        }
        Ok(h)
    }
    /// Stop a signal subscription, delivering Stopped before the final Closed.
    pub fn signal_stop(&mut self, h: Handle, token: Token) -> Result<()> {
        let mut next = self.resource(h)?.head;
        while let Some(op) = next {
            next = self.ops.get(op.key).and_then(|op| op.next);
            self.stop(op);
        }
        self.close(h, token)
    }
    /// Set terminal mode; its original settings are restored on close or drop.
    pub fn tty_set_mode(&mut self, h: Handle, mode: TtyMode) -> Result<()> {
        self.resource(h)?;
        self.backend.tty_set_mode(h, mode)
    }
    /// Query terminal rows and columns.
    pub fn tty_window_size(&self, h: Handle) -> Result<WindowSize> {
        self.resource(h)?;
        self.backend.tty_window_size(h)
    }
    /// Subscribe to resize notifications after validating the terminal. On a
    /// Signal(WinCh) completion, query `tty_window_size` for its current size.
    pub fn tty_resize_start(&mut self, h: Handle, token: Token) -> Result<Handle> {
        self.tty_window_size(h)?;
        self.signal_start(Signal::WinCh, token)
    }
    /// Bind a UDP socket; port zero chooses an ephemeral port.
    pub fn udp_bind(&mut self, addr: SocketAddr, opts: &UdpOpts) -> Result<Handle> {
        self.open(Open::Udp { addr, opts: *opts })
    }
    /// Create a TCP socket and submit its connection operation with the supplied
    /// token. [`TcpOpts::connect_timeout`] is enforced exactly like the deadline
    /// of [`Driver::pipe_connect_until`].
    pub fn tcp_connect(
        &mut self,
        addr: SocketAddr,
        opts: &TcpOpts,
        token: Token,
    ) -> Result<Handle> {
        if opts.connect_timeout == Some(Duration::ZERO) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let h = self.open(Open::Tcp { addr, opts: *opts })?;
        self.start_tcp_connect(h, opts, token)
    }
    /// Connect an exclusively owned, already-bound native TCP socket.
    ///
    /// The binding (including an OS-assigned ephemeral port) and socket options
    /// survive adoption. This supports options which affect the handshake,
    /// such as the receive buffer size, and hosts which reserve a local port
    /// before choosing a peer. `nodelay: true` enables TCP_NODELAY; otherwise
    /// the existing setting is preserved. The peer must have the same address
    /// family. Connected sockets, listeners and unbound sockets are rejected.
    ///
    /// Unix and Windows native backends support this operation. Others report
    /// Unsupported. Ownership is consumed even on setup failure. Once accepted,
    /// completion, timeout and close/cancellation follow [`Self::tcp_connect`].
    pub fn tcp_connect_bound(
        &mut self,
        transport: B::Detached,
        addr: SocketAddr,
        opts: &TcpOpts,
        token: Token,
    ) -> Result<Handle> {
        if opts.connect_timeout == Some(Duration::ZERO) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let h = self.new_handle(Kind::Socket)?;
        if let Err(error) = self.backend.open_bound_tcp(h, transport, addr, *opts) {
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(error);
        }
        self.start_tcp_connect(h, opts, token)
    }
    fn start_tcp_connect(&mut self, h: Handle, opts: &TcpOpts, token: Token) -> Result<Handle> {
        match self.submit(h, Operation::Connect, token) {
            Ok(op) => {
                if let Some(at) = opts.connect_timeout.and_then(|t| self.now().checked_add(t)) {
                    self.connect_deadlines.insert(op.key, at);
                    self.backend.deadline_changed(self.next_deadline());
                }
                Ok(h)
            }
            Err(e) => {
                self.backend.release(h);
                self.handles.remove(h.key);
                self.refs -= 1;
                Err(e)
            }
        }
    }
    /// Adopt an explicitly bound, unconnected TCP socket and submit its connection.
    ///
    /// Preserves the socket's local endpoint and options. The socket must have
    /// a nonzero bound port and the same address family as the destination.
    /// Failure drops the rejected transport; success returns the owning handle
    /// and full operation identity for cancellation or completion matching.
    /// No host callback runs here. The ordinary turn/cancel/close rules apply.
    pub fn tcp_connect_bound(
        &mut self,
        transport: B::Detached,
        destination: SocketAddr,
        token: Token,
    ) -> Result<(Handle, OpId)> {
        self.tcp_connect_adopted(transport, destination, token, B::attach_tcp_connect)
    }
    /// Adopt an unbound TCP socket and submit its connection.
    ///
    /// Preserves its kernel socket options. The socket must be unbound and have
    /// the destination's address family. Unix leaves local endpoint allocation
    /// to connect; Windows binds inside adoption as required by ConnectEx.
    /// Failure drops the transport. Completion, cancellation and close obey the
    /// same ownership rules as [Driver::tcp_connect_bound].
    pub fn tcp_connect_unbound(
        &mut self,
        transport: B::Detached,
        destination: SocketAddr,
        token: Token,
    ) -> Result<(Handle, OpId)> {
        self.tcp_connect_adopted(transport, destination, token, B::attach_tcp_connect_unbound)
    }
    fn tcp_connect_adopted(
        &mut self,
        transport: B::Detached,
        destination: SocketAddr,
        token: Token,
        attach: fn(&mut B, Handle, B::Detached, SocketAddr) -> Result<()>,
    ) -> Result<(Handle, OpId)> {
        let h = self.new_handle(Kind::Socket)?;
        if let Err(error) = attach(&mut self.backend, h, transport, destination) {
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(error);
        }
        match self.submit(h, Operation::Connect, token) {
            Ok(op) => Ok((h, op)),
            Err(error) => {
                self.backend.release(h);
                self.handles.remove(h.key);
                self.refs -= 1;
                Err(error)
            }
        }
    }
    /// Return the local IP endpoint of a bound socket.
    pub fn local_addr(&self, h: Handle) -> Result<SocketAddr> {
        self.resource(h)?;
        self.backend.local_addr(h)
    }
    /// Apply a socket option to a live socket handle, including an accepted one.
    ///
    /// The option is applied immediately, inside this call, through the OS: no
    /// operation is submitted, no completion is produced, nothing is queued and
    /// nothing is allocated. A handle that is closing, or that is not a socket,
    /// is `InvalidInput`; a platform with no equivalent for the option is
    /// `Unsupported`. A backend never accepts an option it cannot apply.
    ///
    /// Bind-time-only options are not reachable here: `SO_REUSEPORT` and
    /// `SO_REUSEADDR` belong to [`ListenOpts`]/[`UdpOpts`], and a listener's
    /// per-connection defaults belong to [`AcceptDefaults`]. See [`SocketOption`].
    pub fn set_option(&mut self, h: Handle, option: SocketOption) -> Result<()> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.backend.set_option(h, option)
    }
    /// Read a socket option back from the OS.
    ///
    /// Always a fresh kernel query, never a cache of what was set, so a value the
    /// OS rounded, clamped or doubled is visible as the OS holds it.
    pub fn get_option(&self, h: Handle, kind: SocketOptionKind) -> Result<SocketOption> {
        let r = self.resource(h)?;
        if !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.backend.get_option(h, kind)
    }
    /// Set TCP keep-alive timing independently of its enable switch.
    ///
    /// Omitted fields remain unchanged. Values are validated before any write;
    /// OS failures are returned and may leave earlier fields applied. The socket
    /// remains enabled or disabled exactly as it was before this call.
    pub fn set_keepalive_parameters(&mut self, h: Handle, value: KeepAlive) -> Result<()> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.backend.set_keepalive_parameters(h, value)
    }
    /// Read the kernel's TCP keep-alive timing, including while probing is disabled.
    pub fn keepalive_parameters(&self, h: Handle) -> Result<KeepAlive> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.backend.keepalive_parameters(h)
    }
    /// Change the backlog of an already-listening TCP socket.
    ///
    /// Zero or an unrepresentable backlog is InvalidInput. This does not turn
    /// a merely bound socket into a listener. The kernel may cap the value.
    pub fn set_listen_backlog(&mut self, h: Handle, backlog: u32) -> Result<()> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if backlog == 0 || backlog > i32::MAX as u32 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.backend.set_listen_backlog(h, backlog)
    }
    /// Shut down the receive direction of a stream socket.
    ///
    /// The write direction remains open, including pending writes. Pending
    /// receive operations or queued completions return WouldBlock without
    /// changing the socket: first cancel reads as needed and consume their
    /// terminal acknowledgements.
    /// This synchronous control produces no completion of its own.
    pub fn shutdown_read(&mut self, h: Handle) -> Result<()> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if self.queued.iter().any(|c| c.handle == Some(h)) {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        self.backend.shutdown_read(h)
    }
    /// Report the native identity of a live transport handle, for the host's own
    /// bookkeeping. This is Node's `socket._handle.fd`.
    ///
    /// The loop keeps ownership. The value is **borrowed and reporting-only**,
    /// valid until this handle is closed, detached or its loop is dropped, after
    /// which the OS may reuse the number for something else. A host may print it,
    /// expose it to script, compare it, or pass it to a read-only query such as
    /// `getsockname`. A host must not do I/O on it, close or shut it down, change
    /// its blocking mode, register it with another poller or completion port, or
    /// give it to anything that takes ownership: every one of those breaks the
    /// exactly-once completion and buffer-ownership contracts, and turnloop cannot
    /// detect it. To take ownership, [`detach`](Self::detach) it and convert the
    /// resulting transport (`into_fd` on Unix, `into_socket`/`into_handle` on
    /// Windows), which is the only supported way for a descriptor to leave a loop.
    ///
    /// Timers, and any resource whose platform has no descriptor for it (WASI 0.2
    /// and 0.3 sockets are component-model resource handles; web resources are
    /// host objects), report `Unsupported`.
    pub fn raw_transport(&self, h: Handle) -> Result<crate::RawTransport> {
        let r = self.resource(h)?;
        if !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        self.backend.raw_transport(h)
    }
    /// Submit one operation on a socket handle.
    ///
    /// An operation whose completion creates a handle (an accept, a handle
    /// receive) reserves its handle slot here, before the backend is asked.
    /// Refusing at the ceiling is therefore a refusal to *ask*: the connection
    /// stays in the listener's backlog, which is the queue that is meant to
    /// absorb it. The alternative, which this replaces, was to let the kernel
    /// hand over a connection and then destroy it for want of a slot, so a host
    /// at its ceiling refused connections instead of deferring them.
    fn submit(&mut self, h: Handle, operation: Operation, token: Token) -> Result<OpId> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let reserve = usize::from(matches!(
            operation,
            Operation::Accept { .. } | Operation::RecvHandle
        ));
        let multishot = matches!(operation, Operation::Accept { multishot: true });
        // Only a reserving operation is gated: reads, writes and everything else
        // on an existing handle must still work at the ceiling.
        if reserve != 0 && self.handles.remaining() < self.reserved_handles + reserve {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        let op = self.new_op(Some(h), token)?;
        self.reserved_handles += reserve;
        let new = self.ops.get_mut(op.key).expect("new op");
        if multishot {
            new.multishot = true;
            self.multishot_armed += 1;
            self.multishot_reserved += 1;
        } else {
            new.reserved_handles = reserve;
        }
        self.assert_reservations();
        if let Err(e) = self.backend.submit(Request {
            op,
            handle: h,
            operation,
        }) {
            self.retire(op);
            self.outstanding -= 1;
            return Err(e);
        }
        Ok(op)
    }
    /// Accept one incoming TCP or local connection.
    pub fn accept(&mut self, h: Handle, token: Token) -> Result<OpId> {
        self.submit(h, Operation::Accept { multishot: false }, token)
    }
    /// Continuously accept connections until stopped or cancelled.
    pub fn accept_start(&mut self, h: Handle, token: Token) -> Result<OpId> {
        self.submit(h, Operation::Accept { multishot: true }, token)
    }
    /// Read once into stable provided memory or a pooled buffer.
    pub fn read(&mut self, h: Handle, buf: ReadBuf, token: Token) -> Result<OpId> {
        self.submit(
            h,
            Operation::Read {
                buf,
                multishot: false,
            },
            token,
        )
    }
    /// Read repeatedly into pooled leases until EOF, stop, cancellation or error.
    pub fn read_start(&mut self, h: Handle, token: Token) -> Result<OpId> {
        self.submit(
            h,
            Operation::Read {
                buf: ReadBuf::Pooled,
                multishot: true,
            },
            token,
        )
    }
    /// Write the complete buffer; internal partial writes preserve operation ordering.
    pub fn write(&mut self, h: Handle, buf: WriteBuf, token: Token) -> Result<OpId> {
        self.submit(h, Operation::Write(buf), token)
    }
    /// Write all inline segments in order without allocating descriptor storage.
    pub fn writev(&mut self, h: Handle, bufs: WriteVectored, token: Token) -> Result<OpId> {
        self.submit(h, Operation::Writev(bufs), token)
    }
    /// Send the buffer as one UDP datagram to the specified endpoint.
    pub fn send_to(
        &mut self,
        h: Handle,
        buf: WriteBuf,
        to: SocketAddr,
        token: Token,
    ) -> Result<OpId> {
        self.submit(h, Operation::SendTo { buf, to }, token)
    }
    /// Receive one UDP datagram into provided or pooled storage.
    pub fn recv(&mut self, h: Handle, buf: ReadBuf, token: Token) -> Result<OpId> {
        self.submit(h, Operation::RecvFrom(buf), token)
    }
    /// Queue stream write-side shutdown after earlier writes. The handle and its
    /// read direction stay open: reads continue until the peer's EOF.
    ///
    /// TCP half-closes on every native backend (`SHUT_WR`, `SD_SEND`, WASI
    /// `shutdown(send)` / send-stream close) and Unix-domain stream sockets on
    /// epoll/kqueue. Transports without an independent write direction fail with
    /// [`ErrorKind::Unsupported`]: Windows named pipes and synchronous handles,
    /// Unix ttys, FIFOs and regular files, and web fetches. WASI stdout/stderr
    /// close their output stream; a web WebSocket starts its close handshake.
    pub fn shutdown(&mut self, h: Handle, token: Token) -> Result<OpId> {
        self.submit(h, Operation::Shutdown, token)
    }
    /// Request native receive-side shutdown while preserving the send direction.
    ///
    /// This synchronous control supports TCP on Unix, Windows and WASI 0.2,
    /// and Unix-domain stream sockets. Other transports/backends return
    /// Unsupported. No completion is generated for the control itself.
    /// Outstanding reads still require terminal acknowledgement before their
    /// buffers can be reused; queued or already-completed data is not revoked.
    /// Use the executor adapter's `shutdown_receive` to also discard its pending
    /// read and buffered bytes and make all subsequent adapter reads return EOF.
    pub fn shutdown_receive(&mut self, h: Handle) -> Result<()> {
        let resource = self.resource(h)?;
        if resource.closing.is_some() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.backend.shutdown_receive(h)
    }
    /// Request cancellation; terminal acknowledgement must precede memory reuse.
    pub fn cancel(&mut self, id: OpId) -> bool {
        self.cancel_inner(id, false)
    }
    /// Stop a multishot operation with a final Stopped acknowledgement.
    pub fn stop(&mut self, id: OpId) -> bool {
        self.cancel_inner(id, true)
    }
    fn cancel_inner(&mut self, id: OpId, stop: bool) -> bool {
        if id.owner != self.owner {
            return false;
        }
        let Some(op) = self.ops.get(id.key).cloned() else {
            return false;
        };
        if op.cancel {
            return false;
        }
        let timer = op.handle.is_some_and(|h| {
            self.handles
                .get(h.key)
                .is_some_and(|r| matches!(r.kind, Kind::Timer { .. }))
        });
        if timer {
            self.timers.cancel(op.handle.expect("timer handle").key);
            self.backend.deadline_changed(self.next_deadline());
            self.finish(
                id,
                if stop {
                    OpResult::Stopped
                } else {
                    OpResult::Cancelled
                },
                true,
            );
        } else {
            if op.external_wait {
                crate::external_wait::cancel(id);
                #[cfg(target_arch = "wasm32")]
                self.backend.deadline_changed(self.next_deadline());
            } else if op.fs && B::FILESYSTEM == Filesystem::Pool {
                #[cfg(not(target_arch = "wasm32"))]
                self.files.cancel(id);
            } else if let Some(cancel) = op.job_cancel {
                cancel.store(true, Ordering::Release);
            } else if self.backend.cancel(id).is_err() {
                return false;
            }
            let op = self.ops.get_mut(id.key).expect("active op");
            op.cancel = true;
            op.stop = stop;
        }
        if self.connect_deadlines.cancel(id.key) {
            self.backend.deadline_changed(self.next_deadline());
        }
        true
    }
    /// Whether close has begun and physical release is still pending.
    pub fn is_closing(&self, h: Handle) -> bool {
        self.resource(h).is_ok_and(|r| r.closing.is_some())
    }
    /// Cancel pending operations, then deliver Closed. The resource is released
    /// only after Closed is appended to the host's output buffer.
    pub fn close(&mut self, h: Handle, token: Token) -> Result<()> {
        let r = self.resource(h)?;
        if r.closing.is_some() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if !matches!(r.kind, Kind::File) {
            self.backend.prepare_close(h)?;
        }
        let r = self.handles.get_mut(h.key).expect("validated");
        r.closing = Some(token);
        // Socket handles already carry a reference. Inactive timer handles do
        // not, so closing adds one through delivery of the final Closed result.
        if matches!(r.kind, Kind::Timer { .. }) && r.referenced {
            self.refs += 1;
        }
        let mut next = self.resource(h)?.head;
        while let Some(id) = next {
            next = self.ops.get(id.key).and_then(|op| op.next);
            self.cancel(id);
        }
        self.maybe_closed(h);
        Ok(())
    }
    /// Cancel pending operations and detach once acknowledgements are delivered; WouldBlock means turn and retry.
    ///
    /// Detaching is how a transport leaves a loop, whether it is going to another
    /// loop ([`attach`](Self::attach)) or to the host for good. It never cancels
    /// silently: operations still outstanding are cancelled, `WouldBlock` is
    /// returned while their terminal completions drain, and the host turns this
    /// loop and retries. On success this loop owns nothing of that transport —
    /// the handle is gone (a later call reports `NotFound`), it is unregistered
    /// from the poller or worker, no buffer is retained, and no completion for it
    /// will ever be produced again.
    ///
    /// To hand the descriptor itself to the host — Node's mid-stream
    /// `socket.upgradeToTLS`, which gives a live connected socket to a TLS layer
    /// — convert the returned transport: `into_fd` on Unix,
    /// `into_socket`/`into_handle` on Windows (where the completion-port rules in
    /// those methods' documentation apply). WASI 0.2, WASI 0.3 and web report
    /// `Unsupported`: their sockets are component-model resource handles or host
    /// objects, not descriptors, and neither platform can pass one out.
    ///
    /// [`raw_transport`](Self::raw_transport) is the borrowed, reporting-only
    /// counterpart for a socket the loop keeps.
    pub fn detach(&mut self, h: Handle) -> Result<B::Detached> {
        let r = self.resource(h)?;
        if r.closing.is_some() || !matches!(r.kind, Kind::Socket) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let mut next = self.resource(h)?.head;
        while let Some(id) = next {
            next = self.ops.get(id.key).and_then(|op| op.next);
            self.cancel(id);
        }
        if self.resource(h)?.pending != 0 || self.queued.iter().any(|c| c.handle == Some(h)) {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        let d = self.backend.detach(h)?;
        let r = self.handles.remove(h.key).expect("validated");
        if r.referenced {
            self.refs -= 1;
        }
        Ok(d)
    }
    /// House a transport an accept has already taken from the kernel, spending
    /// the slot that operation reserved when it was submitted.
    ///
    /// Releasing the reservation before creating the handle is what makes it
    /// real: `new_handle` refuses slots that are still promised, so the only
    /// thing that can spend this one is the accept that reserved it.
    ///
    /// A multishot accept spends one of the slots pooled for every multishot
    /// accept, if any is left, and the pool is then refilled as far as the
    /// ceiling allows. The backend delivered this connection within the poll's
    /// [`Budget`], which counts exactly the free slots not promised to
    /// single-shot operations, so there is room for it either way.
    fn attach_reserved(
        &mut self,
        transport: B::Detached,
        id: OpId,
        token: Token,
    ) -> Result<Handle> {
        let multishot = self.ops.get(id.key).is_some_and(|op| op.multishot);
        let attached = if multishot {
            if self.multishot_reserved != 0 {
                self.multishot_reserved -= 1;
                self.reserved_handles -= 1;
            }
            let attached = self.attach(transport, token);
            while self.multishot_reserved < self.multishot_armed
                && self.handles.remaining() > self.reserved_handles
            {
                self.multishot_reserved += 1;
                self.reserved_handles += 1;
            }
            attached
        } else {
            let held = self
                .ops
                .get_mut(id.key)
                .map_or(0, |op| std::mem::take(&mut op.reserved_handles));
            self.reserved_handles -= held;
            self.attach(transport, token)
        };
        self.assert_reservations();
        attached
    }
    /// What the backend may deliver in the next poll (turnloop#77): every free
    /// handle slot that no single-shot accept or handle receive is holding. A
    /// multishot accept's connections are counted against it, so a batch can
    /// never outrun the ceiling; nothing else is.
    fn budget(&self) -> Budget {
        Budget {
            accepts: self.handles.remaining() - (self.reserved_handles - self.multishot_reserved),
        }
    }
    /// Register an owning transport on this loop; failure drops the rejected transport.
    ///
    /// Reports `ResourceLimit` at the handle ceiling, and also when the only
    /// remaining slots are promised to accepts this loop has already armed.
    pub fn attach(&mut self, d: B::Detached, _token: Token) -> Result<Handle> {
        let h = self.new_handle(Kind::Socket)?;
        if let Err(e) = self.backend.attach(h, d) {
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(e);
        }
        Ok(h)
    }
    /// Register a host condition with the native helper or single-agent service. Registrations use
    /// preallocated storage; cancellation and completion follow normal OpId rules.
    pub fn external_wait(
        &mut self,
        condition: &WaitCondition,
        expected: u64,
        deadline: Option<Instant>,
        token: Token,
    ) -> Result<OpId> {
        self.admit_port()?;
        let op = self.new_op(None, token)?;
        self.ops.get_mut(op.key).expect("new wait").external_wait = true;
        self.charge_port(op);
        if let Err(e) =
            crate::external_wait::submit(op, self.work_port.clone(), condition, expected, deadline)
        {
            self.retire(op);
            self.outstanding -= 1;
            return Err(e);
        }
        #[cfg(target_arch = "wasm32")]
        self.backend.deadline_changed(self.next_deadline());
        Ok(op)
    }
    /// Submit a typed filesystem request; see [`FsRequest`] for ordering and
    /// ownership. Native targets run it on the shared blocking pool, where queue
    /// exhaustion rejects the request with ResourceLimit before acceptance. Requests
    /// naming a handle need a visible, open file handle that is not closing.
    pub fn fs(&mut self, request: FsRequest, token: Token) -> Result<OpId> {
        if B::FILESYSTEM == Filesystem::Unsupported {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        let kind = if B::FILESYSTEM == Filesystem::Pool {
            Kind::File
        } else {
            Kind::Socket
        };
        if B::FILESYSTEM == Filesystem::Pool {
            self.admit_port()?;
        }
        let target = request.handle();
        if let Some(h) = target {
            let r = self.resource(h)?;
            if r.closing.is_some()
                || std::mem::discriminant(&r.kind) != std::mem::discriminant(&kind)
            {
                return Err(Error::new(ErrorKind::InvalidInput));
            }
        }
        let opened = if request.opens() {
            let h = self.new_handle(kind)?;
            self.handles.get_mut(h.key).expect("new handle").hidden = true;
            Some(h)
        } else {
            None
        };
        let handle = opened.or(target);
        let op = match self.new_op(handle, token) {
            Ok(op) => op,
            Err(e) => {
                if let Some(h) = opened {
                    self.discard_hidden(h, false);
                }
                return Err(e);
            }
        };
        self.ops.get_mut(op.key).expect("new request").fs = true;
        if B::FILESYSTEM == Filesystem::Pool {
            self.charge_port(op);
        }
        let accepted = match B::FILESYSTEM {
            #[cfg(not(target_arch = "wasm32"))]
            Filesystem::Pool => self.files.submit(op, handle, request),
            _ => self.backend.fs(op, handle, request),
        };
        if let Err(e) = accepted {
            self.retire(op);
            self.outstanding -= 1;
            if let Some(h) = opened {
                self.discard_hidden(h, false);
            }
            return Err(e);
        }
        // DESIGN §10 rule 3: a request the backend executes needs its native step
        // even while posts are queued, like a natively accepted lookup. Requests on
        // backend handles are already counted; path requests are counted here. Pool
        // requests are not native: their results arrive as queued work.
        if B::FILESYSTEM == Filesystem::Backend {
            let op = self.ops.get_mut(op.key).expect("accepted request");
            if !op.native {
                op.native = true;
                self.native_pending += 1;
            }
        }
        Ok(op)
    }
    /// Whether a request waiting for a pooled lease can start now.
    fn files_waiting(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.files.has_work()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }
    /// Remove a handle that never became visible, releasing any native object.
    fn discard_hidden(&mut self, h: Handle, native: bool) {
        let r = self.handles.remove(h.key).expect("hidden handle");
        debug_assert!(r.hidden);
        if r.referenced {
            self.refs -= 1;
        }
        if native {
            match r.kind {
                #[cfg(not(target_arch = "wasm32"))]
                Kind::File => self.files.release(h),
                _ => self.backend.release(h),
            }
        }
    }
    /// Deliver a typed filesystem terminal result, publishing or discarding an
    /// opened handle. A failed or cancelled open never exposes its handle.
    fn finish_fs(&mut self, id: OpId, result: OpResult) {
        let Some(h) = self.ops.get(id.key).and_then(|op| op.handle) else {
            self.finish(id, result, true);
            return;
        };
        if !self.handles.get(h.key).is_some_and(|r| r.hidden) {
            self.finish(id, result, true);
            return;
        }
        if matches!(result, OpResult::Fs(FsResult::Opened(_))) {
            self.handles.get_mut(h.key).expect("hidden handle").hidden = false;
            self.finish(id, result, true);
            return;
        }
        self.finish(id, result, true);
        let q = self.queued.back_mut().expect("queued open result");
        debug_assert_eq!(q.completion.op, Some(id));
        q.completion.handle = None;
        self.discard_hidden(h, true);
    }
    /// Watch a file or directory with the platform's native API: inotify (Linux),
    /// FSEvents for directories and kqueue for files (macOS), kqueue (BSD/iOS),
    /// ReadDirectoryChangesW (Windows). Nonterminal `OpResult::Watch` batches follow
    /// until `fs_watch_stop` (Stopped, then Closed) or `close` (Cancelled, then Closed).
    /// WASI and the web return Unsupported. Node `watchFile` is a host timer plus `Stat`.
    pub fn fs_watch(
        &mut self,
        path: &FsPath,
        options: WatchOptions,
        token: Token,
    ) -> Result<Handle> {
        let h = self.new_handle(Kind::Socket)?;
        if let Err(e) = self
            .backend
            .fs_watch(h, path, options)
            .and_then(|()| self.submit(h, Operation::WatchFs, token).map(|_| ()))
        {
            self.backend.release(h);
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(e);
        }
        Ok(h)
    }
    /// Stop a filesystem watch, delivering Stopped before the final Closed.
    pub fn fs_watch_stop(&mut self, h: Handle, token: Token) -> Result<()> {
        self.signal_stop(h, token)
    }
    /// Submit an owned Send closure to the bounded shared blocking pool.
    ///
    /// Equivalent to [`blocking_with`](Self::blocking_with) at
    /// [`Occupancy::Bounded`], for work that does not need to observe its own
    /// cancellation.
    pub fn blocking<F: FnOnce() -> BlockingResult + Send + 'static>(
        &mut self,
        f: F,
        token: Token,
    ) -> Result<OpId> {
        self.submit_work(crate::blocking::blocking(f), token)
    }
    /// Submit an owned Send closure with an explicit [`Occupancy`] class, and
    /// hand it the [`Cancellation`] signal of its own job.
    ///
    /// [`Occupancy::Bounded`] behaves exactly like [`blocking`](Self::blocking):
    /// the fixed worker set, the shared queue, `ResourceLimit` when that queue
    /// is full. [`Occupancy::Long`] is for work that holds its thread for as
    /// long as a connection lives; it is served by a separate, on-demand worker
    /// set, so neither class can exhaust the other. The two share this loop's
    /// operation table, as every operation does, and nothing else.
    ///
    /// Delivery is the same single completion for both classes: `Blocking` with
    /// the payload, `Cancelled` if [`cancel`](Self::cancel) won, or `Err` if the
    /// job panicked. A refused submission returns here and never completes.
    ///
    /// ```no_run
    /// # use turnloop::*;
    /// # fn f(l: &mut Loop, accept_one: impl Fn() -> BlockingResult + Send + 'static) -> Result<()> {
    /// // An accept loop holds its thread until the host asks it to stop.
    /// l.blocking_with(
    ///     move |stop| {
    ///         while !stop.requested() {
    ///             accept_one()?;
    ///         }
    ///         Ok(Payload::U64(0))
    ///     },
    ///     Occupancy::Long,
    ///     Token(1),
    /// )?;
    /// # Ok(()) }
    /// ```
    pub fn blocking_with<F: FnOnce(&Cancellation) -> BlockingResult + Send + 'static>(
        &mut self,
        f: F,
        occupancy: Occupancy,
        token: Token,
    ) -> Result<OpId> {
        let port = self.work_port.clone();
        self.submit_job(
            move |cancel| crate::blocking::blocking_cancellable(f, cancel.clone(), port),
            occupancy,
            token,
        )
    }
    /// Resolve through a host resolver, or the shared native blocking pool.
    pub fn resolve(&mut self, request: crate::DnsRequest, token: Token) -> Result<OpId> {
        let op = self.new_op(None, token)?;
        match self.backend.resolve(op, &request) {
            Ok(()) => {
                // A natively accepted lookup needs backend discovery like socket I/O.
                self.ops.get_mut(op.key).expect("new lookup").native = true;
                self.native_pending += 1;
                Ok(op)
            }
            Err(e) => {
                self.retire(op);
                self.outstanding -= 1;
                if e.kind == ErrorKind::Unsupported {
                    self.submit_work(crate::blocking::resolve(request), token)
                } else {
                    Err(e)
                }
            }
        }
    }
    fn submit_work(
        &mut self,
        f: Box<dyn FnOnce() -> Result<crate::blocking::WorkOutput> + Send>,
        token: Token,
    ) -> Result<OpId> {
        self.submit_job(move |_| f, Occupancy::Bounded, token)
    }
    /// Take an operation slot, then hand the pool a job built around that
    /// operation's cancellation flag. A refused submission retires the slot, so
    /// a caller that sees an error is owed no completion.
    fn submit_job<M>(&mut self, make: M, occupancy: Occupancy, token: Token) -> Result<OpId>
    where
        M: FnOnce(
            &std::sync::Arc<std::sync::atomic::AtomicBool>,
        ) -> Box<dyn FnOnce() -> Result<crate::blocking::WorkOutput> + Send>,
    {
        self.admit_port()?;
        let op = self.new_op(None, token)?;
        self.charge_port(op);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.ops.get_mut(op.key).expect("new op").job_cancel = Some(cancel.clone());
        let f = make(&cancel);
        if let Err(e) = crate::blocking::submit(
            self.config.blocking_pool,
            occupancy,
            op,
            cancel,
            self.work_port.clone(),
            f,
        ) {
            self.retire(op);
            self.outstanding -= 1;
            return Err(e);
        }
        Ok(op)
    }
    /// Clone the thread-safe wake endpoint for this loop.
    pub fn notifier(&self) -> Notifier {
        self.notifier.clone()
    }
    /// Clone the bounded posting endpoint routed to this loop only.
    pub fn poster(&self) -> Poster {
        self.poster.clone()
    }
    /// Opt into external host waiting and return the borrowed integration primitive.
    pub fn integration(&mut self) -> Result<Integration> {
        let integration = self.backend.integration()?;
        self.external = true;
        self.notifier.external_park(
            !self.queued.is_empty()
                || !self.poster.is_empty()
                || !self.work_port.is_empty()
                || self.files_waiting()
                || self.backend.has_work(),
        )?;
        Ok(integration)
    }
    fn drain(&mut self, out: &mut Completions) {
        while out.len() < out.capacity() {
            let Some(Queued {
                completion: c,
                referenced,
                event_class,
            }) = self.queued.pop_front()
            else {
                break;
            };
            let closed = matches!(c.result, OpResult::Closed)
                .then_some(c.handle)
                .flatten();
            self.refs -= usize::from(referenced);
            if let Some(class) = event_class {
                self.buffered[class] -= 1;
            }
            if c.terminal && c.op.is_some() {
                self.outstanding -= 1;
            }
            out.entries.push(c);
            if let Some(h) = closed
                && let Some(r) = self.handles.remove(h.key)
            {
                match r.kind {
                    Kind::Socket => self.backend.release(h),
                    #[cfg(not(target_arch = "wasm32"))]
                    Kind::File => self.files.release(h),
                    _ => {}
                }
                if r.referenced {
                    self.refs -= 1;
                }
            }
        }
    }
    fn accept_event(&mut self, e: Event<B::Detached>) {
        if e.op.owner != self.owner {
            return;
        }
        let Some(op) = self.ops.get(e.op.key).cloned() else {
            return;
        };
        if op.fs && e.terminal {
            let result = if op.cancel {
                OpResult::Cancelled
            } else {
                match e.result {
                    Ok(Outcome::Fs { output, lease }) => {
                        fs_result(&self.metadata, op.handle, output, lease)
                    }
                    Err(e) => OpResult::Err(e),
                    Ok(_) => OpResult::Err(Error::new(ErrorKind::Other)),
                }
            };
            self.finish_fs(e.op, result);
            return;
        }
        let result = if op.cancel {
            if !e.terminal {
                return;
            }
            if op.timed_out {
                OpResult::Err(Error::new(ErrorKind::TimedOut))
            } else if op.stop {
                OpResult::Stopped
            } else {
                OpResult::Cancelled
            }
        } else {
            match e.result {
                Ok(Outcome::Resolved(addresses)) => OpResult::Resolved(addresses),
                Ok(Outcome::Exited(status)) => OpResult::Exited(status),
                Ok(Outcome::Signal(signal)) => OpResult::Signal(signal),
                Ok(Outcome::PipeAccepted(d)) => match self.attach_reserved(d, e.op, op.token) {
                    Ok(conn) => OpResult::PipeAccepted { conn },
                    Err(e) => OpResult::Err(e),
                },
                Ok(Outcome::HandleReceived(d)) => match self.attach_reserved(d, e.op, op.token) {
                    Ok(handle) => OpResult::HandleReceived { handle },
                    Err(e) => OpResult::Err(e),
                },
                Ok(Outcome::HandleSent) => OpResult::HandleSent,
                Err(e) => OpResult::Err(e),
                Ok(Outcome::Connected) => OpResult::Connected,
                Ok(Outcome::Accepted { transport, peer }) => {
                    match self.attach_reserved(transport, e.op, op.token) {
                        Ok(conn) => OpResult::Accepted { conn, peer },
                        Err(e) => OpResult::Err(e),
                    }
                }
                Ok(Outcome::Read { n, lease }) => OpResult::Read { n, lease },
                Ok(Outcome::Eof) => OpResult::Eof,
                Ok(Outcome::Wrote(n)) => OpResult::Wrote(n),
                Ok(Outcome::RecvFrom { n, from, lease }) => OpResult::RecvFrom { n, from, lease },
                Ok(Outcome::Shutdown) => OpResult::Shutdown,
                Ok(Outcome::Watch { events, overflow }) => OpResult::Watch { events, overflow },
                Ok(Outcome::Fs { output, lease }) => {
                    fs_result(&self.metadata, op.handle, output, lease)
                }
                Ok(Outcome::Cancelled) => OpResult::Cancelled,
            }
        };
        self.finish(e.op, result, e.terminal);
    }
    /// Collect bounded completions with at most one OS wait, never invoking host callbacks.
    pub fn turn(&mut self, timeout: Timeout, out: &mut Completions) -> Result<TurnInfo> {
        self.assert_owner();
        self.backend.validate_timeout(timeout)?;
        out.clear();
        // Leases released by the host since the last turn can start waiting requests.
        #[cfg(not(target_arch = "wasm32"))]
        self.files.retry();
        let notified = self.notifier.begin();
        let start = self.backend.now();
        self.expire_connects(start)?;
        #[cfg(target_arch = "wasm32")]
        crate::external_wait::poll(self.owner, start);
        let deadline = match (timeout.deadline(start), self.next_deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let queued =
            !self.queued.is_empty() || !self.poster.is_empty() || !self.work_port.is_empty();
        let mut waits = 0;
        let mut discovery_polls = 0;
        let mut zero_event_waits = 0;
        // DESIGN §10 rule 3. Queued work forbids a blocking wait (the timeout is
        // forced to zero below) and permits one zero-time native discovery poll
        // only with native operations pending and native output reserve free.
        // Without native operations, queued work makes no backend call: a native
        // backend could otherwise enter the OS after draining stale cached
        // readiness, so has_work() deliberately cannot force the call.
        #[cfg(not(turnloop_backend = "web"))]
        let native_step = !queued || self.native_pending != 0;
        // Web polling only drains host callbacks and Worker/condition rings and
        // never enters the OS. Keep draining that cached host work (for example a
        // Worker ring retained by a full poster) through sustained queued work.
        #[cfg(turnloop_backend = "web")]
        let native_step = !queued || self.native_pending != 0 || self.backend.has_work();
        if self.buffered[NATIVE_EVENTS] == 0 && native_step {
            let mut timeout = deadline.map(|d| d.saturating_duration_since(start));
            if timeout == Some(Duration::ZERO) || queued || notified || !self.notifier.park() {
                timeout = Some(Duration::ZERO);
            }
            let poll = self.backend.poll(timeout, self.budget(), &mut self.events);
            self.notifier.running();
            let poll = poll?;
            waits = poll.waits;
            discovery_polls = poll.discovery_polls;
            zero_event_waits = poll.zero_event_waits;
            // Taking/replacing the preallocated vector preserves storage and allows
            // completion handling to mutate the backend when accepting a socket.
            let mut events = std::mem::take(&mut self.events);
            for e in events.drain(..) {
                self.accept_event(e);
            }
            self.events = events;
        }
        let now = self.backend.now();
        self.expire_connects(now)?;
        #[cfg(target_arch = "wasm32")]
        crate::external_wait::poll(self.owner, now);
        for _ in 0..self.config.events_per_turn {
            if self.buffered[TIMER_EVENTS] == self.config.events_per_turn {
                break;
            }
            let Some((key, _at)) = self.timers.pop_expired(now) else {
                break;
            };
            let h = Handle {
                owner: self.owner,
                key,
            };
            let Some(r) = self.handles.get(key) else {
                continue;
            };
            if let Kind::Timer {
                op: Some(op),
                repeat,
            } = r.kind
            {
                if let Some(repeat) = repeat {
                    if let Some(at) = now.checked_add(repeat) {
                        self.timers.insert(h.key, at);
                        self.finish(op, OpResult::Timer, false);
                    } else {
                        self.finish(op, OpResult::Err(Error::new(ErrorKind::InvalidInput)), true);
                    }
                } else {
                    self.finish(op, OpResult::Timer, true);
                }
            }
        }
        self.backend.deadline_changed(self.next_deadline());
        for _ in 0..self.config.events_per_turn {
            let Some(work) = self.work_port.pop() else {
                break;
            };
            let Some(op) = self.ops.get(work.op.key) else {
                continue;
            };
            #[cfg(not(target_arch = "wasm32"))]
            if op.fs {
                let (cancel, handle) = (op.cancel, op.handle);
                let (lease, metadata) = self.files.complete(work.op);
                let result = match work.result {
                    _ if cancel => OpResult::Cancelled,
                    Ok(crate::blocking::WorkOutput::Fs(reply)) => {
                        fs_result(&self.metadata, handle, reply.output(metadata), lease)
                    }
                    Ok(_) => OpResult::Err(Error::new(ErrorKind::Other)),
                    Err(e) => OpResult::Err(e),
                };
                self.finish_fs(work.op, result);
                continue;
            }
            let result = if op.cancel {
                if op.stop {
                    OpResult::Stopped
                } else {
                    OpResult::Cancelled
                }
            } else {
                match work.result {
                    #[cfg(not(target_arch = "wasm32"))]
                    Ok(crate::blocking::WorkOutput::Fs(_)) => {
                        OpResult::Err(Error::new(ErrorKind::Other))
                    }
                    Ok(crate::blocking::WorkOutput::ExternalWait(r)) => OpResult::ExternalWait(r),
                    Ok(crate::blocking::WorkOutput::Blocking(p)) => OpResult::Blocking(p),
                    Ok(crate::blocking::WorkOutput::Resolved(a)) => OpResult::Resolved(a),
                    Err(e) => OpResult::Err(e),
                }
            };
            self.finish(work.op, result, true);
        }
        // The independent post reserve guarantees progress through a persistent
        // timer/I/O backlog without consuming terminal operation/Closed credits.
        for _ in 0..self.config.events_per_turn {
            if self.buffered[POST_EVENTS] == self.config.events_per_turn {
                break;
            }
            let Some(p) = self.poster.pop() else {
                break;
            };
            self.enqueue(
                Completion {
                    token: p.token,
                    op: None,
                    handle: None,
                    terminal: true,
                    result: OpResult::Posted(p.payload),
                },
                false,
            );
        }
        // Cancelled or failed reads may have returned leases during this turn.
        #[cfg(not(target_arch = "wasm32"))]
        self.files.retry();
        self.drain(out);
        if self.external {
            self.notifier.external_park(
                !self.queued.is_empty()
                    || !self.poster.is_empty()
                    || !self.work_port.is_empty()
                    || self.files_waiting()
                    || self.backend.has_work(),
            )?;
        }
        Ok(TurnInfo {
            completions: out.len(),
            waited: self.backend.now().saturating_duration_since(start),
            alive: self.alive(),
            os_waits: waits,
            discovery_polls,
            zero_event_waits,
        })
    }
}

/// Attach the lease and convert a worker or backend filesystem result.
fn fs_result(
    metadata: &crate::fs::MetadataPool,
    handle: Option<Handle>,
    output: FsOutput,
    mut lease: Option<BufLease>,
) -> OpResult {
    let mut fill = |n: usize| {
        if let Some(lease) = &mut lease {
            lease.set_len(n);
        }
    };
    OpResult::Fs(match output {
        FsOutput::Opened => FsResult::Opened(handle.expect("open request handle")),
        FsOutput::Read(n) => {
            fill(n);
            FsResult::Read { n, lease }
        }
        FsOutput::Wrote(n) => FsResult::Wrote(n),
        FsOutput::Metadata(m) => FsResult::Metadata(metadata.lease(m)),
        FsOutput::Directory { n, eof } => {
            fill(n);
            FsResult::Directory { n, lease, eof }
        }
        FsOutput::Bytes(n) => {
            fill(n);
            FsResult::Bytes { n, lease }
        }
        FsOutput::Done => FsResult::Done,
    })
}

impl<B: Backend> Drop for Driver<B> {
    fn drop(&mut self) {
        crate::external_wait::close(self.owner);
        self.work_port.close();
        self.poster.close();
        self.notifier.close();
    }
}

#[cfg(all(test, windows, not(loom)))]
mod iocp_failure_tests {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_HANDLE, WAIT_OBJECT_0},
        System::Threading::WaitForSingleObject,
    };

    #[test]
    fn pump_failure_repeats_before_queued_work_and_drop_joins_pending_io() {
        let (done, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut bytes = [0xa5; 32]; // remains pinned until driver destruction
            let mut driver = Loop::new(Config::default()).expect("loop");
            let listener = driver
                .tcp_listen(([127, 0, 0, 1], 0).into(), &ListenOpts::default())
                .expect("listener");
            let peer = std::net::TcpStream::connect(driver.local_addr(listener).expect("address"))
                .expect("peer");
            driver.accept(listener, Token(1)).expect("accept");
            let deadline = driver.now() + Duration::from_secs(5);
            let mut out = Completions::with_capacity(1);
            let stream = loop {
                assert!(driver.now() < deadline);
                driver
                    .turn(Timeout::Until(deadline), &mut out)
                    .expect("accept turn");
                if let Some(c) = out.drain().next() {
                    let OpResult::Accepted { conn, .. } = c.result else {
                        panic!("{c:?}")
                    };
                    break conn;
                }
            };
            // SAFETY: fixed exclusive bytes outlive driver on success and unwind.
            let buffer = unsafe { IoBufMut::from_raw_parts(bytes.as_mut_ptr(), bytes.len()) };
            let read = driver
                .read(stream, ReadBuf::Provided(buffer), Token(2))
                .expect("idle read");
            driver
                .turn(Timeout::Now, &mut out)
                .expect("start real pending I/O");
            assert!(out.is_empty());
            assert!(
                driver.backend.operation_waiting_for_test(read),
                "subject reached native wait"
            );
            let Integration::Event(event) = driver.integration().expect("helper") else {
                panic!("event")
            };
            let timer = driver
                .timer(driver.now(), None, Token(3))
                .expect("ready timer");
            driver
                .close(timer, Token(4))
                .expect("queued cancellation and close");
            assert!(!driver.queued.is_empty(), "core fast path subject");
            driver
                .backend
                .fail_event_for_test(ERROR_INVALID_HANDLE as i32);
            let expected: Error =
                std::io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32).into();
            for _ in 0..8 {
                assert_eq!(
                    driver
                        .turn(Timeout::Now, &mut out)
                        .expect_err("persistent pump error"),
                    expected
                );
                assert!(out.is_empty());
                assert_eq!(
                    driver.integration().expect_err("integration keeps failure"),
                    expected
                );
                assert_eq!(
                    // SAFETY: borrowed event is owned by the still-live loop.
                    unsafe { WaitForSingleObject(event as _, 1000) },
                    WAIT_OBJECT_0
                );
            }
            drop(driver); // joins pump, cancels and drains the live native read
            assert_eq!(bytes, [0xa5; 32]);
            bytes.fill(0x5a);
            drop(peer);
            assert_eq!(bytes, [0x5a; 32]);
            done.send(8).expect("completed teardown marker");
        });
        assert_eq!(
            receive
                .recv_timeout(Duration::from_secs(15))
                .expect("drop watchdog"),
            8
        );
        worker.join().expect("failure regression thread");
    }
}

#[cfg(all(test, not(loom), not(turnloop_backend = "web")))]
mod clock_contract {
    use super::*;
    use crate::backend::{PollInfo, Wake};
    use std::sync::Arc;
    struct NoWake;
    impl Wake for NoWake {
        fn wake(&self) -> Result<()> {
            panic!("nonblocking test must not wake")
        }
        fn syscall_count(&self) -> u64 {
            0
        }
    }
    struct Host {
        now: Instant,
        deadline: Option<Instant>,
        changes: usize,
        polls: usize,
        cached_work: bool,
        lookup: Option<OpId>,
        lookup_ready: bool,
        file_request: Option<OpId>,
        accept_connect: bool,
        pending: Option<Request>,
        cancellation: Option<OpId>,
        cancel_error: Option<Error>,
        cancel_calls: usize,
        acknowledge: bool,
    }
    // SAFETY: this test backend accepts only buffer-free synthetic connects;
    // it performs no native I/O and owns no user buffers.
    unsafe impl Backend for Host {
        type Wake = NoWake;
        type Detached = ();
        const FILESYSTEM: Filesystem = Filesystem::Backend;
        fn fs(&mut self, op: OpId, handle: Option<Handle>, request: FsRequest) -> Result<()> {
            if handle.is_some() || !matches!(request, FsRequest::Stat { .. }) {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            assert!(self.file_request.replace(op).is_none());
            Ok(())
        }
        fn new(_: &Config, _: BufferPool) -> Result<Self> {
            Ok(Self {
                now: Instant::now(),
                deadline: None,
                changes: 0,
                polls: 0,
                cached_work: false,
                lookup: None,
                lookup_ready: false,
                file_request: None,
                accept_connect: false,
                pending: None,
                cancellation: None,
                cancel_error: None,
                cancel_calls: 0,
                acknowledge: false,
            })
        }
        fn now(&self) -> Instant {
            self.now
        }
        fn waker(&self) -> Arc<NoWake> {
            Arc::new(NoWake)
        }
        fn validate_timeout(&self, timeout: Timeout) -> Result<()> {
            if matches!(timeout, Timeout::Now) {
                Ok(())
            } else {
                Err(Error::new(ErrorKind::Unsupported))
            }
        }
        fn deadline_changed(&mut self, deadline: Option<Instant>) {
            self.deadline = deadline;
            self.changes += 1;
        }
        fn open(&mut self, _: Handle, spec: Open) -> Result<()> {
            if self.accept_connect && matches!(spec, Open::Pipe(_) | Open::Tcp { .. }) {
                Ok(())
            } else {
                Err(Error::new(ErrorKind::Unsupported))
            }
        }
        fn open_bound_tcp(&mut self, _: Handle, _: (), _: SocketAddr, _: TcpOpts) -> Result<()> {
            if self.accept_connect {
                Ok(())
            } else {
                Err(Error::new(ErrorKind::Unsupported))
            }
        }
        fn local_addr(&self, _: Handle) -> Result<SocketAddr> {
            Err(Error::new(ErrorKind::Unsupported))
        }
        fn submit(&mut self, request: Request) -> Result<()> {
            if self.accept_connect && matches!(request.operation, Operation::Connect) {
                assert!(self.pending.replace(request).is_none());
                Ok(())
            } else {
                Err(Error::new(ErrorKind::Unsupported))
            }
        }
        fn resolve(&mut self, op: OpId, _: &crate::DnsRequest) -> Result<()> {
            assert!(self.lookup.replace(op).is_none());
            Ok(())
        }
        fn cancel(&mut self, op: OpId) -> Result<()> {
            self.cancel_calls += 1;
            if let Some(error) = self.cancel_error {
                return Err(error);
            }
            if self
                .pending
                .as_ref()
                .is_some_and(|request| request.op == op)
            {
                self.cancellation = Some(op);
                Ok(())
            } else {
                Err(Error::new(ErrorKind::NotFound))
            }
        }
        fn has_work(&self) -> bool {
            self.cached_work
        }
        fn poll(
            &mut self,
            timeout: Option<Duration>,
            _budget: Budget,
            events: &mut Vec<Event<()>>,
        ) -> Result<PollInfo> {
            assert_eq!(timeout, Some(Duration::ZERO));
            self.polls += 1;
            // Like the WASI backends: accepted filesystem requests run in poll.
            if let Some(op) = self.file_request.take() {
                events.push(Event {
                    op,
                    terminal: true,
                    result: Ok(Outcome::Fs {
                        output: FsOutput::Done,
                        lease: None,
                    }),
                });
                return Ok(PollInfo::default());
            }
            if self.lookup_ready
                && let Some(op) = self.lookup.take()
            {
                events.push(Event {
                    op,
                    terminal: true,
                    result: Ok(Outcome::Resolved(vec![SocketAddr::from((
                        [127, 0, 0, 1],
                        80,
                    ))])),
                });
                return Ok(PollInfo {
                    discovery_polls: 1,
                    ..PollInfo::default()
                });
            }
            if self.acknowledge
                && let Some(op) = self.cancellation.take()
            {
                assert_eq!(self.pending.take().expect("pending connect").op, op);
                events.push(Event {
                    op,
                    terminal: true,
                    result: Ok(Outcome::Cancelled),
                });
            }
            Ok(PollInfo::default())
        }
        fn release(&mut self, _: Handle) {}
        fn detach(&mut self, _: Handle) -> Result<()> {
            Err(Error::new(ErrorKind::Unsupported))
        }
        fn attach(&mut self, _: Handle, _: ()) -> Result<()> {
            Err(Error::new(ErrorKind::Unsupported))
        }
        fn integration(&mut self) -> Result<Integration> {
            Ok(Integration::HostCallback)
        }
    }
    #[test]
    fn queued_core_work_skips_backend_even_with_cached_work() {
        let mut driver = Driver::<Host>::new(Config::default()).expect("host loop");
        driver.backend.cached_work = true;
        let mut out = Completions::with_capacity(1);
        let mut delivered = 0;
        for _ in 0..64 {
            driver
                .poster()
                .post(Token(1), Payload::U64(42))
                .expect("post");
            let info = driver.turn(Timeout::Now, &mut out).expect("queued turn");
            assert_eq!(info.completions, 1);
            assert_eq!(out[0].token, Token(1));
            assert!(matches!(out[0].result, OpResult::Posted(Payload::U64(42))));
            assert_eq!((info.os_waits, info.discovery_polls), (0, 0));
            delivered += 1;
        }
        assert_eq!(delivered, 64);
        assert_eq!(driver.backend.polls, 0, "backend must not be called");
    }

    #[test]
    fn natively_accepted_lookup_is_a_pending_native_operation() {
        let mut driver = Driver::<Host>::new(Config::default()).expect("host loop");
        let request = crate::DnsRequest {
            host: "localhost".into(),
            port: 80,
        };
        let lookup = driver.resolve(request, Token(1)).expect("native lookup");
        let mut out = Completions::with_capacity(1);
        let (mut posts, mut resolved) = (0, 0);
        for turn in 0..8 {
            driver
                .poster()
                .post(Token(2), Payload::U64(turn))
                .expect("post");
            // Discovery runs through queued posts while the lookup is pending.
            driver.backend.lookup_ready = turn == 3;
            let polls = driver.backend.polls;
            let info = driver.turn(Timeout::Now, &mut out).expect("turn");
            assert_eq!(info.os_waits, 0);
            let expected = usize::from(turn <= 3);
            assert_eq!(driver.backend.polls - polls, expected, "turn {turn}");
            for c in out.drain() {
                match c.result {
                    OpResult::Resolved(addresses) => {
                        assert_eq!(c.op, Some(lookup));
                        assert_eq!(addresses, [SocketAddr::from(([127, 0, 0, 1], 80))]);
                        resolved += 1;
                    }
                    OpResult::Posted(_) => posts += 1,
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
        assert_eq!((resolved, posts), (1, 7));
        assert!(driver.backend.lookup.is_none());
    }

    #[test]
    fn backend_path_request_is_a_pending_native_operation() {
        let mut driver = Driver::<Host>::new(Config::default()).expect("host loop");
        let path = FsPath::new("/synthetic").expect("path");
        let mut out = Completions::with_capacity(1);
        // Sustained posts: without the native flag, every turn has queued work and
        // no native operation, so the backend (where the request runs) is skipped.
        driver
            .poster()
            .post(Token(2), Payload::U64(0))
            .expect("post");
        let op = driver
            .fs(
                FsRequest::Stat {
                    path,
                    follow_symlinks: true,
                },
                Token(1),
            )
            .expect("backend accepts the path request");
        assert_eq!(driver.native_pending, 1, "path request counted as native");
        let (mut done, mut posts) = (0, 0);
        for turn in 0..4 {
            let polls = driver.backend.polls;
            let info = driver.turn(Timeout::Now, &mut out).expect("turn");
            assert_eq!(info.os_waits, 0, "queued turns never block");
            assert!(info.os_waits + info.discovery_polls <= 1);
            assert_eq!(
                driver.backend.polls - polls,
                usize::from(turn == 0),
                "one native step while the request is pending, none after"
            );
            for c in out.drain() {
                match c.result {
                    OpResult::Fs(FsResult::Done) => {
                        assert_eq!(c.op, Some(op));
                        done += 1;
                    }
                    OpResult::Posted(_) => posts += 1,
                    other => panic!("unexpected {other:?}"),
                }
            }
            driver
                .poster()
                .post(Token(2), Payload::U64(turn))
                .expect("replenish");
        }
        assert_eq!(done, 1, "the request completed despite queued posts");
        assert_eq!(
            driver.native_pending, 0,
            "terminal completion retires the flag"
        );
        assert!(posts >= 3);
    }

    #[test]
    fn connection_deadline_waits_for_acknowledgement_and_retains_cancellation_errors() {
        for kind in 0..3 {
            let mut driver = Driver::<Host>::new(Config::default()).expect("host loop");
            driver.backend.accept_connect = true;
            let at = driver.now() + Duration::from_millis(10);
            let opts = TcpOpts {
                connect_timeout: Some(Duration::from_millis(10)),
                ..TcpOpts::default()
            };
            let peer = "127.0.0.1:9".parse().unwrap();
            let h = match kind {
                0 => driver.pipe_connect_until(&PipeName("synthetic".into()), at, Token(1)),
                1 => driver.tcp_connect(peer, &opts, Token(1)),
                _ => driver.tcp_connect_bound((), peer, &opts, Token(1)),
            }
            .expect("connect");
            let op = driver
                .backend
                .pending
                .as_ref()
                .expect("submitted connect")
                .op;
            let mut out = Completions::with_capacity(1);
            driver
                .turn(Timeout::Now, &mut out)
                .expect("before deadline");
            assert!(out.is_empty());
            assert_eq!(driver.backend.cancel_calls, 0);
            assert_eq!(driver.next_deadline(), Some(at));
            let failure = Error {
                kind: ErrorKind::Other,
                os: Some(12345),
            };
            driver.backend.cancel_error = Some(failure);
            driver.backend.now = at;
            assert_eq!(
                driver
                    .turn(Timeout::Now, &mut out)
                    .expect_err("injected cancellation error"),
                failure
            );
            assert_eq!(driver.backend.cancel_calls, 1);
            assert_eq!(driver.next_deadline(), Some(at));
            assert!(out.is_empty() && driver.backend.pending.is_some());
            driver.backend.cancel_error = None;
            driver
                .turn(Timeout::Now, &mut out)
                .expect("retry cancellation");
            assert_eq!(driver.backend.cancel_calls, 2);
            assert_eq!(driver.next_deadline(), None);
            assert!(
                out.is_empty() && driver.backend.pending.is_some(),
                "timeout cannot retire before native acknowledgement"
            );
            driver.backend.acknowledge = true;
            driver
                .turn(Timeout::Now, &mut out)
                .expect("acknowledge timeout");
            assert_eq!(out.len(), 1);
            assert_eq!(
                (out[0].handle, out[0].op, out[0].token, out[0].terminal),
                (Some(h), Some(op), Token(1), true)
            );
            assert!(matches!(
                out[0].result,
                OpResult::Err(Error {
                    kind: ErrorKind::TimedOut,
                    ..
                })
            ));
            assert!(driver.backend.pending.is_none());
            assert_eq!(driver.backend.cancel_calls, 2);
            driver.close(h, Token(2)).expect("close timed-out stream");
            driver.turn(Timeout::Now, &mut out).expect("closed");
            assert_eq!(out.len(), 1);
            assert!(matches!(out[0].result, OpResult::Closed));
            driver
                .turn(Timeout::Now, &mut out)
                .expect("no duplicate timeout");
            assert!(out.is_empty() && !driver.alive());
        }
    }
    #[test]
    fn optional_native_capabilities_reject_without_leaking_core_reservations() {
        let mut l = Driver::<Host>::new(Config::default()).expect("host loop");
        let mut spec = ProcessSpec::new("unused-native-program");
        spec.stdio = [ProcessStdio::Pipe; 3];
        assert_eq!(
            l.spawn(&spec, Token(1))
                .expect_err("unsupported spawn")
                .kind,
            ErrorKind::Unsupported
        );
        assert_eq!(
            l.signal_start(Signal::Int, Token(2))
                .expect_err("unsupported signal")
                .kind,
            ErrorKind::Unsupported
        );
        assert_eq!(
            l.open_stdio(Stdio::Stdin)
                .expect_err("unsupported stdio")
                .kind,
            ErrorKind::Unsupported
        );
        assert_eq!(
            l.pipe_listen(&PipeName("unused".into()), &ListenOpts::default())
                .expect_err("unsupported pipe")
                .kind,
            ErrorKind::Unsupported
        );
        assert!(
            !l.alive(),
            "failed native setup must roll back every reservation"
        );
        let h = l
            .timer(l.now(), None, Token(3))
            .expect("core timer still works");
        assert_eq!(
            l.kill(h, Signal::Kill).expect_err("unsupported kill").kind,
            ErrorKind::Unsupported
        );
        assert_eq!(
            l.tty_set_mode(h, TtyMode::Raw)
                .expect_err("unsupported TTY")
                .kind,
            ErrorKind::Unsupported
        );
        assert_eq!(
            l.tty_window_size(h)
                .expect_err("unsupported dimensions")
                .kind,
            ErrorKind::Unsupported
        );
        let mut out = Completions::default();
        l.turn(Timeout::Now, &mut out).expect("core turn");
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].result, OpResult::Timer));
        l.close(h, Token(4)).expect("timer close");
        l.turn(Timeout::Now, &mut out).expect("closed turn");
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].result, OpResult::Closed));
        assert!(!l.alive());
    }
    #[test]
    fn host_clock_arms_deadline_and_validates_queued_turns() {
        let mut l = Driver::<Host>::new(Config::default()).expect("host loop");
        let at = l.now() + Duration::from_micros(50);
        let h = l.timer(at, None, Token(1)).expect("timer");
        assert_eq!(l.backend.deadline, Some(at));
        assert!(l.backend.changes > 0);
        let mut out = Completions::default();
        l.turn(Timeout::Now, &mut out).expect("early turn");
        assert!(out.is_empty());
        assert_eq!(l.backend.polls, 1);
        l.backend.now = at;
        l.turn(Timeout::Now, &mut out).expect("deadline turn");
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].result, OpResult::Timer));
        assert_eq!(l.backend.deadline, None);
        l.close(h, Token(2)).expect("close");
        assert!(
            matches!(
                l.turn(Timeout::Forever, &mut out),
                Err(Error {
                    kind: ErrorKind::Unsupported,
                    ..
                })
            ),
            "queued completions do not bypass timeout validation"
        );
        l.turn(Timeout::Now, &mut out).expect("queued close");
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].result, OpResult::Closed));
        let h = l
            .timer(at + Duration::from_secs(1), None, Token(3))
            .expect("timer");
        assert!(l.cancel(l.timer_op(h).expect("op")));
        assert_eq!(l.backend.deadline, None);
    }
}

#[cfg(turnloop_backend = "web")]
impl Driver<crate::backend::web::Web> {
    /// Configure the host dispatcher. It is scheduled asynchronously and should
    /// call turn(Now); no user callback runs inside a turnloop method.
    pub fn set_schedule_turn(&mut self, schedule: &js_sys::Function) -> Result<()> {
        self.backend.configure(schedule)?;
        self.integration()?;
        Ok(())
    }
    /// Number of coalesced host turn requests.
    pub fn schedule_count(&self) -> u32 {
        self.backend.schedule_count()
    }
    /// Attach a bounded Worker Poster. Returns a JS descriptor with `buffer`
    /// (SharedArrayBuffer), `capacity`, and `producerSource` (SharedPoster class).
    /// Construct that class in a Worker with the transferred descriptor; post
    /// returns false on contention/full/closed, retaining the caller's values.
    /// Browsers require COOP/COEP isolation and Atomics.waitAsync; see docs/wasm.md.
    #[cfg(feature = "web-worker")]
    pub fn worker_poster(&mut self, capacity: u32) -> Result<wasm_bindgen::JsValue> {
        self.backend.worker_poster(capacity, self.poster())
    }
    /// Expose a condition to Workers through a bounded Atomics-backed queue.
    /// The descriptor's producerSource class offers store(u64) and notify(). Both
    /// return false on contention/full/closed; retry without losing the update.
    /// Accepted updates apply on the owning agent in queue order. A descriptor
    /// lives until this driver drops; the condition can notify waits on any local loop.
    #[cfg(feature = "web-worker")]
    pub fn worker_wait_condition(
        &mut self,
        condition: &WaitCondition,
        capacity: u32,
    ) -> Result<wasm_bindgen::JsValue> {
        self.backend.worker_condition(condition.clone(), capacity)
    }
    /// Fetch one complete response into a provided or pooled buffer. Responses
    /// larger than that buffer complete with ResourceLimit, never truncated data.
    pub fn fetch(&mut self, url: &str, buf: ReadBuf, token: Token) -> Result<(Handle, OpId)> {
        let h = self.open(Open::Fetch { url: url.into() })?;
        match self.read(h, buf, token) {
            Ok(op) => Ok((h, op)),
            Err(e) => {
                self.backend.release(h);
                self.handles.remove(h.key);
                self.refs -= 1;
                Err(e)
            }
        }
    }
    /// Connect a browser WebSocket; subsequent read/write operations exchange
    /// whole binary messages. Oversize messages fail with ResourceLimit.
    pub fn websocket(&mut self, url: &str, token: Token) -> Result<Handle> {
        let h = self.open(Open::WebSocket { url: url.into() })?;
        if let Err(e) = self.submit(h, Operation::Connect, token) {
            self.backend.release(h);
            self.handles.remove(h.key);
            self.refs -= 1;
            return Err(e);
        }
        Ok(h)
    }
}

#[cfg(all(test, not(loom), not(target_arch = "wasm32")))]
mod result_ring {
    use super::*;
    /// The result ring holds every result that may be undelivered at once, and
    /// no more: `max_undelivered`, capped by `max_operations` (issue #88).
    #[test]
    fn is_sized_by_undelivered_results_not_by_operations() {
        let pool = PoolConfig {
            max_undelivered: 100,
            ..PoolConfig::default()
        };
        let l = Loop::new(Config {
            max_operations: 32_768,
            blocking_pool: pool,
            ..Config::default()
        })
        .expect("loop");
        assert_eq!(
            l.work_port.capacity(),
            128,
            "a large I/O ceiling is not paid for"
        );
        let l = Loop::new(Config {
            max_operations: 16,
            ..Config::default()
        })
        .expect("loop");
        assert_eq!(
            l.work_port.capacity(),
            16,
            "never more than every operation"
        );
        let none = PoolConfig {
            max_undelivered: 0,
            ..PoolConfig::default()
        };
        let refused = Loop::new(Config {
            blocking_pool: none,
            ..Config::default()
        });
        assert_eq!(refused.err().map(|e| e.kind), Some(ErrorKind::InvalidInput));
    }
}
