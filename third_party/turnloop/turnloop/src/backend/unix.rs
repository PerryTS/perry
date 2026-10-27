//! Shared completion engine for kqueue and epoll. Each direction has an intrusive
//! FIFO of operations; readiness remains cached until an actual EAGAIN.
#[cfg(turnloop_backend = "epoll")]
use super::epoll::Epoll as SystemPoller;
#[cfg(turnloop_backend = "kqueue")]
use super::kqueue::Kqueue as SystemPoller;
use super::{
    poller::{last_error, Poller, Ready},
    socket::{self, Addr},
};
use crate::slots::{page_reserve, Slots};
use crate::{
    backend::{Backend, Budget, Event, Operation, Outcome, PollInfo, Request},
    *,
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    os::fd::{AsRawFd, OwnedFd, RawFd},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Tcp,
    Pipe,
    PipeListener,
    Stream,
    File,
    Listener,
    Udp,
}
/// Owns an unregistered socket and can be sent to another loop/thread.
pub struct Detached {
    pub(super) fd: OwnedFd,
    pub(super) kind: Kind,
    pub(super) original_flags: Option<i32>,
    original_mode: Option<libc::termios>,
    stdio_lease: Option<super::stdio::Lease>,
    /// A listener's per-connection defaults, applied to each socket it accepts.
    /// It travels with the listener across detach/attach, so a listener handed to
    /// another loop keeps configuring its connections there.
    accept_defaults: AcceptDefaults,
}
impl std::fmt::Debug for Detached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Detached")
            .field("fd", &self.fd)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl Detached {
    /// The descriptor this transport owns, for host reporting only.
    pub fn raw_transport(&self) -> crate::RawTransport {
        crate::RawTransport::Fd(self.fd.as_raw_fd())
    }
    /// Give the descriptor to the caller; turnloop never touches it again.
    ///
    /// The transport is already unregistered and quiescent: `Driver::detach`
    /// refuses a handle with an outstanding operation, so no loop, poller or
    /// buffer refers to this descriptor any more. Status flags and terminal
    /// settings captured when the descriptor was adopted are restored first,
    /// exactly as they would be on close, and then the close is *not* performed.
    ///
    /// For standard I/O, restoration waits until the last managed standard-I/O
    /// transport is released, including duplicates held by other loops. A handed
    /// off duplicate shares those flags and must not reconfigure them meanwhile.
    ///
    /// A descriptor turnloop created itself was created non-blocking with
    /// `FD_CLOEXEC`, and it is handed over that way: nothing is restored,
    /// because nothing was changed. Call `fcntl(F_SETFL)` (or
    /// `TcpStream::set_nonblocking(false)`) if the receiving code wants blocking
    /// I/O, which is what a synchronous TLS handshake on the descriptor needs.
    pub fn into_fd(mut self) -> OwnedFd {
        self.restore();
        drop(self.stdio_lease.take());
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: `this` is never dropped, so this move of the single owning
        // field cannot be observed twice; the lease was released above and
        // all other remaining fields are Copy/plain data whose Drop is a no-op.
        // Restoration and lease release have already run.
        unsafe { std::ptr::read(&this.fd) }
    }
    fn restore(&self) {
        if self.stdio_lease.is_some() {
            return;
        }
        if let Some(mode) = &self.original_mode {
            // SAFETY: descriptor is still owned; restore before OwnedFd drops.
            unsafe {
                libc::tcsetattr(self.fd.as_raw_fd(), libc::TCSANOW, mode);
            }
        }
        if let Some(flags) = self.original_flags {
            // SAFETY: descriptor is still owned and flags came from F_GETFL.
            unsafe {
                libc::fcntl(self.fd.as_raw_fd(), libc::F_SETFL, flags);
            }
        }
    }
    pub(super) fn new(fd: OwnedFd, kind: Kind) -> Self {
        // SAFETY: termios is plain C storage, filled by tcgetattr on a terminal.
        let mut mode: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: live fd and writable termios; ENOTTY means no restoration needed.
        let original_mode =
            (unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut mode) } == 0).then_some(mode);
        Self {
            fd,
            kind,
            original_flags: None,
            original_mode,
            stdio_lease: None,
            accept_defaults: AcceptDefaults::EMPTY,
        }
    }
    pub(super) fn share_stdio(&mut self, lease: super::stdio::Lease, mode: Option<libc::termios>) {
        self.original_flags = None;
        self.original_mode = mode;
        self.stdio_lease = Some(lease);
    }
    /// Adopt an owned Unix descriptor, classifying stream/file/TTY or socket.
    /// The descriptor must have no concurrent I/O users. Status flags and terminal
    /// settings are restored when this transport is closed or dropped.
    ///
    /// On kqueue platforms (macOS, the BSDs) a *listening* socket cannot be told
    /// apart from a connected one here, and is adopted as a stream; adopt a
    /// listener with [`from_listener_fd`](Self::from_listener_fd) instead.
    pub fn from_fd(fd: OwnedFd) -> Result<Self> {
        super::ipc::classify(fd)
    }
    /// Adopt an owned, already listening TCP or Unix stream socket, so the loop
    /// can `accept` on it - a listener a host bound itself or received from
    /// another process.
    ///
    /// This works on every Unix backend. [`from_fd`](Self::from_fd) recognises a
    /// listener only where the kernel reports `SO_ACCEPTCONN`, which macOS and
    /// the BSDs do not. A descriptor that is not a stream socket, or that has a
    /// peer, is refused with `InvalidInput` and closed. On kqueue platforms a
    /// socket that was bound but never `listen`ed cannot be detected; its first
    /// `accept` fails instead.
    pub fn from_listener_fd(fd: OwnedFd) -> Result<Self> {
        super::ipc::classify_listener(fd)
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
    connect: Option<Addr>,
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
    passed: Option<Detached>,
}
/// Completion engine shared by kqueue and epoll, with native process and signal services.
pub struct Unix {
    poller: SystemPoller,
    resources: Slots<Resource>,
    ops: Slots<Pending>,
    ready: VecDeque<Handle>,
    /// Listeners whose multishot accept is ready but out of budget. They are
    /// not runnable, so they neither count as work nor shorten a wait, and they
    /// return to `ready` at the first poll with a positive budget (turnloop#77).
    throttled: VecDeque<Handle>,
    /// Connections multishot accepts may still deliver in the current poll.
    accepts: usize,
    cancelled: VecDeque<OpId>,
    polled: Vec<Ready>,
    pool: BufferPool,
    services: super::services::Services,
    files: super::files::Files,
    watches: super::watch::Watches,
}
fn direction(op: &Operation) -> usize {
    usize::from(!matches!(
        op,
        Operation::Accept { .. }
            | Operation::Read { .. }
            | Operation::RecvFrom(_)
            | Operation::RecvHandle
    ))
}
impl Unix {
    fn get(&self, h: Handle) -> Result<&Resource> {
        self.resources
            .get(h.index())
            .and_then(Option::as_ref)
            .filter(|r| r.handle == h)
            .ok_or(Error::new(ErrorKind::NotFound))
    }
    /// The descriptor behind a socket handle. Process, signal and watch handles
    /// and adopted files/terminals are not sockets and say so.
    fn socket_fd(&self, h: Handle) -> Result<std::os::fd::RawFd> {
        if self.services.contains(h) || self.watches.contains(h) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        let r = self.get(h)?;
        if matches!(r.transport.kind, Kind::File | Kind::Stream) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        Ok(r.transport.fd.as_raw_fd())
    }
    fn install(&mut self, h: Handle, transport: Detached, connect: Option<Addr>) -> Result<()> {
        if self.resources.get(h.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if transport.kind != Kind::File {
            self.poller.register(transport.fd.as_raw_fd(), h.key())?;
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
                    // Out of budget: the connection stays in the backlog, and
                    // `schedule` below parks the listener until a later poll.
                    continue;
                }
                let p = self.ops[i].as_mut().expect("queued op");
                let result = execute(r, p, &self.pool);
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
                    Err(e) if e.os == Some(libc::EINTR) => None,
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
/// Whether the operation at the head of a direction is one the budget throttles.
fn holds_throttled(ops: &Slots<Pending>, head: Option<usize>) -> bool {
    head.and_then(|i| ops.get(i))
        .and_then(Option::as_ref)
        .is_some_and(|p| Budget::throttles(&p.request.operation))
}
// SAFETY: all I/O executes synchronously in poll; a terminal event removes its
// request, and owned descriptors/requests are dropped without outstanding native
// buffer access. Readiness events contain generation keys, never buffer pointers.
unsafe impl Backend for Unix {
    #[cfg(turnloop_backend = "kqueue")]
    type Wake = super::kqueue::KqueueWake;
    #[cfg(turnloop_backend = "epoll")]
    type Wake = super::epoll::EpollWake;
    type Detached = Detached;
    fn new(config: &Config, pool: BufferPool) -> Result<Self> {
        Ok(Self {
            poller: SystemPoller::new(config.events_per_turn)?,
            resources: Slots::new(config.max_handles),
            ops: Slots::new(config.max_operations),
            ready: VecDeque::with_capacity(page_reserve(config.max_handles)),
            throttled: VecDeque::with_capacity(page_reserve(config.max_handles)),
            accepts: usize::MAX,
            cancelled: VecDeque::with_capacity(page_reserve(config.max_operations)),
            polled: Vec::with_capacity(config.events_per_turn),
            files: super::files::Files::new(config, pool.clone()),
            watches: super::watch::Watches::new(config, pool.clone()),
            pool,
            services: super::services::Services::new(config),
        })
    }
    fn set_notifier(&mut self, notifier: Notifier) {
        self.watches.set_notifier(notifier.clone());
        self.files.set_notifier(notifier.clone());
        self.services.set_notifier(notifier);
    }
    fn signal(&mut self, h: Handle, signal: Signal) -> Result<()> {
        self.services.signal(h, signal)
    }
    fn fs_watch(&mut self, h: Handle, path: &FsPath, options: WatchOptions) -> Result<()> {
        if self.resources.get(h.index()).is_none_or(Option::is_some) || self.services.contains(h) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.watches.start(h, path, options, &mut self.poller)
    }
    fn prepare_close(&mut self, h: Handle) -> Result<()> {
        self.services.prepare_close(h)
    }
    fn kill(&mut self, h: Handle, signal: Signal, group: bool) -> Result<()> {
        self.services.kill(h, signal, group)
    }
    fn spawn(
        &mut self,
        h: Handle,
        pipes: [Option<Handle>; 3],
        extra: &[Option<Handle>],
        spec: &ProcessSpec,
    ) -> Result<u32> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio as ChildStdio};
        // Explicit SIG_IGN/NOCLDWAIT would auto-reap children behind our ownership.
        // Reject before launch instead of losing exit-before-registration status.
        // SAFETY: writable sigaction storage; null input only queries disposition.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: query a valid signal with initialized writable output storage.
        if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } < 0 {
            return Err(last_error());
        }
        if action.sa_sigaction == libc::SIG_IGN || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if spec.controlling_terminal && !spec.detached {
            // TIOCSCTTY succeeds only for a session leader without a terminal.
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        if spec.env_clear {
            command.env_clear();
        }
        command.envs(spec.env.iter().map(|(k, v)| (k, v)));
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        if let Some(uid) = spec.uid {
            command.uid(uid);
        }
        if let Some(gid) = spec.gid {
            command.gid(gid);
        }
        if !spec.detached && spec.new_process_group {
            command.process_group(0);
        }
        for (i, stdio) in spec.stdio.iter().enumerate() {
            let stream = match stdio {
                ProcessStdio::Inherit => ChildStdio::inherit(),
                ProcessStdio::Null => ChildStdio::null(),
                ProcessStdio::Pipe => ChildStdio::piped(),
                ProcessStdio::Handle(h) => ChildStdio::from(
                    self.get(*h)?
                        .transport
                        .fd
                        .try_clone()
                        .map_err(Error::from)?,
                ),
            };
            match i {
                0 => {
                    command.stdin(stream);
                }
                1 => {
                    command.stdout(stream);
                }
                _ => {
                    command.stderr(stream);
                }
            }
        }
        // Child ends of the extra descriptors, still at whatever numbers the OS
        // gave them. They are relocated above every target number first, so the
        // child hook's dup2 sequence cannot overwrite a source it has not used.
        let mut sources = Vec::with_capacity(spec.extra.len());
        let mut parent_ends = Vec::with_capacity(spec.extra.len());
        let floor = super::process::checked_floor(spec.extra.iter().map(|fd| fd.number))?;
        for fd in &spec.extra {
            let (child, parent) = match fd.source {
                ChildFdSource::Null => (super::process::null()?, None),
                ChildFdSource::Pipe => {
                    let (parent, child) = super::process::one_way()?;
                    (child, Some((parent, false)))
                }
                ChildFdSource::Duplex => {
                    let (parent, child) = super::process::stream_pair()?;
                    (child, Some((parent, true)))
                }
                ChildFdSource::Handle(source) => (
                    self.get(source)?
                        .transport
                        .fd
                        .try_clone()
                        .map_err(Error::from)?,
                    None,
                ),
            };
            sources.push((super::process::lift(child, floor)?, fd.number as RawFd));
            parent_ends.push(parent);
        }
        let dups: Vec<(RawFd, RawFd)> = sources
            .iter()
            .map(|(child, target)| (child.as_raw_fd(), *target))
            .collect();
        let reserved = match sources.first() {
            Some((donor, _)) => super::process::reserve(
                donor.as_raw_fd(),
                spec.extra.iter().map(|fd| fd.number as RawFd),
            )?,
            None => Vec::new(),
        };
        let (detached, terminal) = (spec.detached, spec.controlling_terminal);
        if detached || terminal || !dups.is_empty() {
            // SAFETY: the child hook calls only async-signal-safe setsid, ioctl
            // and dup2, reads a captured plain-integer list without allocating,
            // and builds an OS error from errno alone. std runs it after the
            // standard streams are in place and immediately before exec, so
            // descriptor 0 is already the child's stdin and every source is
            // above every target.
            unsafe {
                command.pre_exec(move || {
                    if detached && libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if terminal && libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    for &(source, target) in &dups {
                        // dup2 leaves the new descriptor without FD_CLOEXEC, so
                        // it is exactly this number that survives exec.
                        if libc::dup2(source, target) < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        let spawned = command.spawn();
        drop((sources, reserved));
        let mut child = spawned.map_err(Error::from)?;
        let pid = child.id();
        let stdio: [Option<OwnedFd>; 3] = [
            child.stdin.take().map(Into::into),
            child.stdout.take().map(Into::into),
            child.stderr.take().map(Into::into),
        ];
        let result = (|| {
            self.services.child(
                h,
                child,
                spec.new_process_group || spec.detached,
                &mut self.poller,
            )?;
            for (handle, fd) in pipes.into_iter().zip(stdio) {
                if let (Some(handle), Some(fd)) = (handle, fd) {
                    let transport = super::ipc::classify(fd)?;
                    self.install(handle, transport, None)?;
                }
            }
            for (handle, end) in extra.iter().zip(parent_ends.drain(..)) {
                if let (Some(handle), Some((fd, stream))) = (handle, end) {
                    let transport = if stream {
                        // A socket pair's own end, exactly as an accepted local
                        // connection is adopted: configured, not re-classified.
                        socket::configure(fd.as_raw_fd())?;
                        Detached::new(fd, Kind::Pipe)
                    } else {
                        super::ipc::classify(fd)?
                    };
                    self.install(*handle, transport, None)?;
                }
            }
            Ok(pid)
        })();
        if result.is_err() {
            for handle in std::iter::once(h)
                .chain(pipes.into_iter().flatten())
                .chain(extra.iter().flatten().copied())
            {
                self.release(handle);
            }
        }
        result
    }
    fn tty_set_mode(&mut self, h: Handle, mode: TtyMode) -> Result<()> {
        let r = self.get(h)?;
        let original = r
            .transport
            .original_mode
            .ok_or(Error::new(ErrorKind::InvalidInput))?;
        let mut termios = original;
        if mode != TtyMode::Normal {
            // SAFETY: initialized termios structure with exclusive local access.
            unsafe {
                libc::cfmakeraw(&mut termios);
            }
            if mode == TtyMode::Raw {
                termios.c_lflag |= libc::ISIG;
            }
        }
        // SAFETY: owned tty fd and valid termios; TCSANOW does not drain/block output.
        if unsafe { libc::tcsetattr(r.transport.fd.as_raw_fd(), libc::TCSANOW, &termios) } < 0 {
            return Err(last_error());
        }
        Ok(())
    }
    fn tty_window_size(&self, h: Handle) -> Result<WindowSize> {
        let r = self.get(h)?;
        if r.transport.original_mode.is_none() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        // SAFETY: winsize is plain C output storage.
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        // SAFETY: owned tty fd and writable winsize output.
        if unsafe { libc::ioctl(r.transport.fd.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } < 0 {
            return Err(last_error());
        }
        Ok(WindowSize {
            columns: size.ws_col,
            rows: size.ws_row,
        })
    }
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn waker(&self) -> Arc<Self::Wake> {
        self.poller.waker()
    }
    fn open(&mut self, h: Handle, spec: Open) -> Result<()> {
        let spec = match spec {
            Open::Pipe(name) => {
                let (transport, addr) = super::ipc::open(&name, None)?;
                return self.install(h, transport, Some(addr));
            }
            Open::PipeListener { name, opts } => {
                super::sockopt::validate_accept_defaults(opts.accept_defaults, false)?;
                let (transport, _) = super::ipc::open(&name, Some(opts))?;
                return self.install(h, transport, None);
            }
            Open::Stdio(which) => {
                let fd = match which {
                    Stdio::Stdin => 0,
                    Stdio::Stdout => 1,
                    Stdio::Stderr => 2,
                };
                let transport = super::ipc::stdio(fd)?;
                return self.install(h, transport, None);
            }
            other => other,
        };
        let (addr, kind, reuse, backlog, nodelay, accept_defaults) = match spec {
            Open::Tcp { addr, opts } => (
                addr,
                Kind::Tcp,
                ReusePort::No,
                0,
                opts.nodelay,
                AcceptDefaults::EMPTY,
            ),
            Open::Listener { addr, opts } => (
                addr,
                Kind::Listener,
                opts.reuse_port,
                opts.backlog,
                false,
                opts.accept_defaults,
            ),
            Open::Udp { addr, opts } => (
                addr,
                Kind::Udp,
                opts.reuse_port,
                0,
                false,
                AcceptDefaults::EMPTY,
            ),
            _ => unreachable!("native open handled above"),
        };
        super::sockopt::validate_accept_defaults(accept_defaults, kind == Kind::Listener)?;
        if backlog > i32::MAX as u32 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        // Resolved before the socket exists, so a request this platform cannot
        // honour never reaches the kernel at all.
        let reuse_option = super::socket::reuse_port_option(reuse)?;
        let fd = socket::create(addr, kind == Kind::Udp)?;
        if kind != Kind::Tcp {
            // TCP listeners need address reuse for TIME_WAIT. Default UDP binds
            // must stay exclusive: on Linux SO_REUSEADDR also permits two live
            // bind(:0) sockets to receive the same ephemeral endpoint.
            if kind == Kind::Listener || reuse.is_enabled() {
                socket::option(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_REUSEADDR, 1)?;
            }
            if let Some(option) = reuse_option {
                socket::option(fd.as_raw_fd(), libc::SOL_SOCKET, option, 1)?;
            }
            let a = Addr::new(addr);
            // SAFETY: sockaddr pointer and length refer to initialized storage.
            if unsafe { libc::bind(fd.as_raw_fd(), a.ptr(), a.len) } < 0 {
                return Err(last_error());
            }
            if kind == Kind::Listener {
                // SAFETY: this fd is a bound stream socket and backlog is checked.
                if unsafe { libc::listen(fd.as_raw_fd(), backlog as i32) } < 0 {
                    return Err(last_error());
                }
            }
        }
        if nodelay {
            socket::option(fd.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_NODELAY, 1)?;
        }
        let mut transport = Detached::new(fd, kind);
        transport.accept_defaults = accept_defaults;
        self.install(h, transport, (kind == Kind::Tcp).then(|| Addr::new(addr)))
    }
    fn local_addr(&self, h: Handle) -> Result<SocketAddr> {
        socket::local_addr(self.get(h)?.transport.fd.as_raw_fd())
    }
    fn open_bound_tcp(
        &mut self,
        h: Handle,
        transport: Detached,
        peer: SocketAddr,
        opts: TcpOpts,
    ) -> Result<()> {
        if transport.kind != Kind::Tcp {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let fd = transport.fd.as_raw_fd();
        let local = socket::local_addr(fd)?;
        if local.port() == 0 || local.is_ipv4() != peer.is_ipv4() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let mut address = Addr::empty();
        // SAFETY: exclusively owned socket and initialized writable address storage.
        if unsafe { libc::getpeername(fd, address.mut_ptr(), &mut address.len) } == 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let error = last_error();
        if error.os != Some(libc::ENOTCONN) {
            return Err(error);
        }
        if opts.nodelay {
            socket::option(fd, libc::IPPROTO_TCP, libc::TCP_NODELAY, 1)?;
        }
        self.install(h, transport, Some(Addr::new(peer)))
    }
    fn set_keepalive_parameters(&mut self, h: Handle, value: crate::KeepAlive) -> Result<()> {
        super::sockopt::set_keepalive_parameters(self.socket_fd(h)?, value)
    }
    fn keepalive_parameters(&self, h: Handle) -> Result<crate::KeepAlive> {
        super::sockopt::keepalive_parameters(self.socket_fd(h)?)
    }
    fn set_listen_backlog(&mut self, h: Handle, backlog: u32) -> Result<()> {
        if self.get(h)?.transport.kind != Kind::Listener {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        super::sockopt::set_listen_backlog(self.socket_fd(h)?, backlog)
    }
    fn shutdown_read(&mut self, h: Handle) -> Result<()> {
        let cancelled_read = self.cancelled.iter().any(|op| {
            self.ops
                .get(op.index())
                .and_then(Option::as_ref)
                .is_some_and(|p| p.request.handle == h && direction(&p.request.operation) == 0)
        });
        if self.get(h)?.heads[0].is_some() || cancelled_read {
            return Err(Error::new(crate::ErrorKind::WouldBlock));
        }
        super::sockopt::shutdown_read(self.socket_fd(h)?)
    }
    fn set_option(&mut self, h: Handle, option: SocketOption) -> Result<()> {
        if matches!(option, SocketOption::ListenBacklog(_))
            && self.get(h)?.transport.kind != Kind::Listener
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        super::sockopt::set(self.socket_fd(h)?, option)
    }
    fn get_option(&self, h: Handle, kind: SocketOptionKind) -> Result<SocketOption> {
        super::sockopt::get(self.socket_fd(h)?, kind)
    }
    fn shutdown_receive(&mut self, h: Handle) -> Result<()> {
        let transport = &self.get(h)?.transport;
        if !matches!(transport.kind, Kind::Tcp | Kind::Pipe) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        // SAFETY: live owned TCP/Unix stream socket; receive shutdown does not
        // close it or release any buffer held by accepted native requests.
        if unsafe { libc::shutdown(transport.fd.as_raw_fd(), libc::SHUT_RD) } < 0 {
            return Err(last_error());
        }
        Ok(())
    }
    fn submit(&mut self, request: Request) -> Result<()> {
        let h = request.handle;
        if self.services.contains(h) {
            return self.services.submit(&request);
        }
        if self.watches.contains(h) {
            return self.watches.submit(&request);
        }
        let r = self.get(h)?;
        if matches!(request.operation, Operation::Shutdown)
            && matches!(r.transport.kind, Kind::Stream | Kind::File)
        {
            // Only sockets have an independent write direction. A tty, FIFO or
            // regular file cannot half-close; closing it is the caller's choice.
            return Err(Error::new(ErrorKind::Unsupported));
        }
        if r.transport.kind == Kind::File {
            let fd = r.transport.fd.try_clone().map_err(Error::from)?;
            return self.files.submit(request, fd);
        }
        let valid = match &request.operation {
            Operation::Accept { .. } => {
                matches!(r.transport.kind, Kind::Listener | Kind::PipeListener)
            }
            Operation::SendHandle(_) | Operation::RecvHandle => r.transport.kind == Kind::Pipe,
            Operation::RecvFrom(_) | Operation::SendTo { .. } => r.transport.kind == Kind::Udp,
            Operation::Connect => {
                matches!(r.transport.kind, Kind::Tcp | Kind::Pipe) && r.connect.is_some()
            }
            _ => matches!(
                r.transport.kind,
                Kind::Tcp | Kind::Pipe | Kind::Stream | Kind::File
            ),
        };
        if !valid || self.ops.get(request.op.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if matches!(&request.operation, Operation::Read { buf: ReadBuf::Provided(b), .. } if b.is_empty())
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let passed = if let Operation::SendHandle(source) = request.operation {
            let source = self.get(source)?;
            if !matches!(
                source.transport.kind,
                Kind::Tcp | Kind::Listener | Kind::Udp | Kind::Pipe | Kind::PipeListener
            ) {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            Some(Detached::new(
                source.transport.fd.try_clone().map_err(Error::from)?,
                source.transport.kind,
            ))
        } else {
            None
        };
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
            passed,
        });
        self.schedule(h);
        Ok(())
    }
    fn cancel(&mut self, op: OpId) -> Result<()> {
        if self.files.cancel(op) {
            return Ok(());
        }
        if self.services.cancel(op) {
            return Ok(());
        }
        if self.watches.cancel(op) {
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
            || self.services.has_work()
            || self.files.has_work()
            || self.watches.has_work()
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
        self.files.poll(events);
        self.services.poll(events);
        self.watches.poll(events);
        self.run_ready(events);
        // Cached readiness can end in EAGAIN without producing a completion.
        // In that case use this turn's single OS wait with its exact timeout;
        // returning now would turn an idle socket into a timer polling loop.
        if self.has_work() || !events.is_empty() || events.len() == events.capacity() {
            return Ok(PollInfo::default());
        }
        self.polled.clear();
        let info = self.poller.wait(timeout, &mut self.polled)?;
        for i in 0..self.polled.len() {
            let e = self.polled[i];
            if self.services.ready(e.key) || self.watches.ready(e) {
                continue;
            }
            let Some(r) = self
                .resources
                .get_mut(e.key as u32 as usize)
                .and_then(Option::as_mut)
            else {
                continue;
            };
            if r.handle.key() != e.key {
                continue;
            }
            r.ready[0] |= e.read;
            r.ready[1] |= e.write;
            let h = r.handle;
            self.schedule(h);
        }
        self.files.poll(events);
        self.services.poll(events);
        self.watches.poll(events);
        self.run_ready(events);
        Ok(info)
    }
    fn release(&mut self, h: Handle) {
        self.services.release(h, &mut self.poller);
        self.watches.release(h, &mut self.poller);
        if let Ok(r) = self.get(h) {
            if r.transport.kind != Kind::File {
                let fd = r.transport.fd.as_raw_fd();
                let _ = self.poller.deregister(fd);
            }
            self.ready.retain(|&at| at != h);
            self.throttled.retain(|&at| at != h);
            self.resources[h.index()] = None;
        }
        // Closing the final descriptor removes its registration from epoll/kqueue.
    }
    fn raw_transport(&self, h: Handle) -> Result<crate::RawTransport> {
        if self.services.contains(h) || self.watches.contains(h) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        Ok(self.get(h)?.transport.raw_transport())
    }
    fn detach(&mut self, h: Handle) -> Result<Detached> {
        let r = self.get(h)?;
        if r.heads.iter().any(Option::is_some) {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        if r.transport.kind != Kind::File {
            self.poller.deregister(r.transport.fd.as_raw_fd())?;
        }
        // Remove a stale scheduling entry before the slot can be reused.
        self.ready.retain(|&at| at != h);
        self.throttled.retain(|&at| at != h);
        Ok(self.resources[h.index()]
            .take()
            .expect("validated")
            .transport)
    }
    fn attach(&mut self, h: Handle, transport: Detached) -> Result<()> {
        self.install(h, transport, None)
    }
    fn attach_tcp_connect(
        &mut self,
        h: Handle,
        transport: Detached,
        destination: SocketAddr,
    ) -> Result<()> {
        if transport.kind != Kind::Tcp {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let fd = transport.fd.as_raw_fd();
        let local = socket::local_addr(fd)?;
        if local.port() == 0 || local.is_ipv4() != destination.is_ipv4() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let mut peer = Addr::empty();
        // SAFETY: the transport owns fd; peer is correctly sized writable storage.
        let connected = unsafe { libc::getpeername(fd, peer.mut_ptr(), &mut peer.len) };
        if connected == 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOTCONN) {
            return Err(last_error());
        }
        let mut transport = transport;
        transport.kind = Kind::Tcp;
        self.install(h, transport, Some(Addr::new(destination)))
    }
    fn attach_tcp_connect_unbound(
        &mut self,
        h: Handle,
        transport: Detached,
        destination: SocketAddr,
    ) -> Result<()> {
        if transport.kind != Kind::Tcp {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let local = socket::local_addr(transport.fd.as_raw_fd())?;
        if local.port() != 0 || local.is_ipv4() != destination.is_ipv4() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        // The kernel chooses a local endpoint when Connect executes. An early
        // bind(0) would reserve a port before the destination is known.
        self.install(h, transport, Some(Addr::new(destination)))
    }
    fn integration(&mut self) -> Result<Integration> {
        Ok(Integration::Fd(self.poller.fd()))
    }
}

fn execute(
    r: &mut Resource,
    p: &mut Pending,
    pool: &BufferPool,
) -> Result<Option<(Outcome<Detached>, bool)>> {
    let fd = r.transport.fd.as_raw_fd();
    match &mut p.request.operation {
        Operation::ProcessExit | Operation::WatchSignal | Operation::WatchFs => {
            Err(Error::new(ErrorKind::InvalidInput))
        }
        Operation::Connect => {
            if !r.connecting {
                let a = r
                    .connect
                    .as_ref()
                    .ok_or(Error::new(ErrorKind::InvalidInput))?;
                // SAFETY: nonblocking socket and live initialized sockaddr.
                let n = unsafe { libc::connect(fd, a.ptr(), a.len) };
                if n < 0 {
                    let e = last_error();
                    if e.os == Some(libc::EINPROGRESS) {
                        r.connecting = true;
                        r.ready[1] = false;
                        return Ok(None);
                    }
                    return Err(e);
                }
            } else {
                let mut error: i32 = 0;
                let mut len = std::mem::size_of::<i32>() as libc::socklen_t;
                // SAFETY: initialized integer output and length match SO_ERROR's ABI.
                if unsafe {
                    libc::getsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_ERROR,
                        (&mut error as *mut i32).cast(),
                        &mut len,
                    )
                } < 0
                {
                    return Err(last_error());
                }
                if error != 0 {
                    return Err(std::io::Error::from_raw_os_error(error).into());
                }
            }
            r.connect = None;
            Ok(Some((Outcome::Connected, true)))
        }
        Operation::SendHandle(_) => {
            super::ipc::send(
                fd,
                p.passed
                    .as_ref()
                    .ok_or(Error::new(ErrorKind::InvalidInput))?,
            )?;
            Ok(Some((Outcome::HandleSent, true)))
        }
        Operation::RecvHandle => Ok(Some((
            Outcome::HandleReceived(super::ipc::receive(fd)?),
            true,
        ))),
        Operation::Accept { multishot } => {
            if r.transport.kind == Kind::PipeListener {
                return Ok(Some((
                    Outcome::PipeAccepted(super::ipc::accept(fd)?),
                    !*multishot,
                )));
            }
            let (fd, peer) = socket::accept(fd)?;
            // Before the connection becomes visible to the host. A rejected
            // default fails this accept and drops the socket; it is never
            // reported as an accepted-but-unconfigured connection.
            super::sockopt::apply_accept_defaults(fd.as_raw_fd(), r.transport.accept_defaults)?;
            Ok(Some((
                Outcome::Accepted {
                    transport: Detached::new(fd, Kind::Tcp),
                    peer,
                },
                !*multishot,
            )))
        }
        Operation::Read { buf, multishot } => receive(fd, buf, *multishot, false, pool),
        Operation::RecvFrom(buf) => receive(fd, buf, false, true, pool),
        Operation::Write(buf) => {
            let bytes = buf.as_slice();
            if p.offset == bytes.len() {
                return Ok(Some((Outcome::Wrote(p.offset), true)));
            }
            // SAFETY: WriteBuf guarantees stable initialized bytes until completion.
            let n = unsafe {
                if matches!(r.transport.kind, Kind::Stream | Kind::File) {
                    super::ipc::write(
                        fd,
                        bytes[p.offset..].as_ptr().cast(),
                        bytes.len() - p.offset,
                    )
                } else {
                    libc::send(
                        fd,
                        bytes[p.offset..].as_ptr().cast(),
                        bytes.len() - p.offset,
                        send_flags(),
                    )
                }
            };
            if n < 0 {
                return Err(last_error());
            }
            if n == 0 {
                return Err(Error::new(ErrorKind::BrokenPipe));
            }
            p.offset += n as usize;
            Ok((p.offset == bytes.len()).then_some((Outcome::Wrote(p.offset), true)))
        }
        Operation::Writev(bufs) => {
            let mut iov = [libc::iovec {
                iov_base: std::ptr::null_mut(),
                iov_len: 0,
            }; MAX_IOV];
            let mut skip = p.offset;
            let mut count = 0;
            for b in bufs.bufs.iter().flatten() {
                let bytes = b.as_slice();
                if skip >= bytes.len() {
                    skip -= bytes.len();
                    continue;
                }
                iov[count] = libc::iovec {
                    iov_base: bytes[skip..].as_ptr().cast_mut().cast(),
                    iov_len: bytes.len() - skip,
                };
                count += 1;
                skip = 0;
            }
            if count == 0 {
                return Ok(Some((Outcome::Wrote(p.offset), true)));
            }
            // SAFETY: all-zero msghdr is valid before assigning the iovec fields.
            let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
            msg.msg_iov = iov.as_mut_ptr();
            msg.msg_iovlen = count as _;
            // SAFETY: msghdr references initialized iovecs and stable buffer regions.
            let n = unsafe {
                if matches!(r.transport.kind, Kind::Stream | Kind::File) {
                    super::ipc::writev(fd, iov.as_ptr(), count as i32)
                } else {
                    libc::sendmsg(fd, &msg, send_flags())
                }
            };
            if n < 0 {
                return Err(last_error());
            }
            if n == 0 {
                return Err(Error::new(ErrorKind::BrokenPipe));
            }
            p.offset += n as usize;
            let len: usize = bufs.bufs.iter().flatten().map(|b| b.as_slice().len()).sum();
            Ok((p.offset == len).then_some((Outcome::Wrote(p.offset), true)))
        }
        Operation::SendTo { buf, to } => {
            let a = Addr::new(*to);
            let bytes = buf.as_slice();
            // SAFETY: initialized address and stable readable datagram payload.
            let n = unsafe {
                libc::sendto(
                    fd,
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    send_flags(),
                    a.ptr(),
                    a.len,
                )
            };
            if n < 0 {
                return Err(last_error());
            }
            Ok(Some((Outcome::Wrote(n as usize), true)))
        }
        Operation::Shutdown => {
            // SAFETY: fd is a live TCP or Unix-domain stream socket (submit rejects
            // other kinds); SHUT_WR is a valid shutdown direction.
            if unsafe { libc::shutdown(fd, libc::SHUT_WR) } < 0 {
                return Err(last_error());
            }
            Ok(Some((Outcome::Shutdown, true)))
        }
    }
}
fn send_flags() -> i32 {
    #[cfg(turnloop_backend = "epoll")]
    {
        libc::MSG_NOSIGNAL
    }
    #[cfg(turnloop_backend = "kqueue")]
    {
        0
    }
}
fn receive(
    fd: i32,
    buf: &mut ReadBuf,
    multishot: bool,
    udp: bool,
    pool: &BufferPool,
) -> Result<Option<(Outcome<Detached>, bool)>> {
    let mut lease = None;
    let (ptr, len) = match buf {
        ReadBuf::Provided(b) => (b.as_mut_ptr(), b.len()),
        ReadBuf::Pooled => {
            let Some(b) = pool.acquire() else {
                return Ok(None);
            };
            let b = lease.insert(b);
            let bytes = b.writable();
            (bytes.as_mut_ptr(), bytes.len())
        }
    };
    let mut a = Addr::empty();
    // SAFETY: provided memory obeys the exclusive writable-region contract, or is
    // exclusively held by this pool lease. Address output storage is valid.
    let n = unsafe {
        if udp {
            libc::recvfrom(fd, ptr.cast(), len, 0, a.mut_ptr(), &mut a.len)
        } else {
            libc::read(fd, ptr.cast(), len)
        }
    };
    if n < 0 {
        return Err(last_error());
    }
    let n = n as usize;
    if let Some(b) = &mut lease {
        b.set_len(n);
    }
    if udp {
        Ok(Some((
            Outcome::RecvFrom {
                n,
                from: a.decode()?,
                lease,
            },
            true,
        )))
    } else if n == 0 {
        Ok(Some((Outcome::Eof, true)))
    } else {
        Ok(Some((Outcome::Read { n, lease }, !multishot)))
    }
}

#[cfg(all(test, not(loom)))]
mod udp_tests {
    use super::*;

    const REBIND_ATTEMPTS: usize = 16;

    // Closing an exclusive socket releases its ephemeral port before the next
    // bind. Another thread/process can claim it in that gap. Retry the entire
    // fixture on EADDRINUSE only; successful iterations must run every assertion.
    fn retry_udp_rebind(mut iteration: impl FnMut() -> Result<()>) -> Result<()> {
        for attempt in 1..=REBIND_ATTEMPTS {
            match iteration() {
                Ok(()) => return Ok(()),
                Err(error) => {
                    eprintln!("UDP rebind attempt {attempt}/{REBIND_ATTEMPTS}: {error:?}");
                    if error.os != Some(libc::EADDRINUSE) || attempt == REBIND_ATTEMPTS {
                        return Err(error);
                    }
                }
            }
        }
        unreachable!("the final attempt always returns")
    }

    #[test]
    fn udp_rebind_retry_limit_and_errno_are_strict() {
        let busy = Error::from(std::io::Error::from_raw_os_error(libc::EADDRINUSE));
        let mut attempts = 0;
        retry_udp_rebind(|| {
            attempts += 1;
            if attempts < REBIND_ATTEMPTS {
                Err(busy)
            } else {
                Ok(())
            }
        })
        .expect("the final permitted attempt can succeed");
        assert_eq!(attempts, REBIND_ATTEMPTS);

        attempts = 0;
        let error = retry_udp_rebind(|| {
            attempts += 1;
            Err(busy)
        })
        .expect_err("exhaustion must fail, never skip the subject");
        assert_eq!(attempts, REBIND_ATTEMPTS);
        assert_eq!(error.os, Some(libc::EADDRINUSE));

        // Even the same portable kind cannot authorize a retry without the
        // exact OS code. Other failures must not get hidden by a later success.
        for fatal in [
            Error::from(std::io::Error::from_raw_os_error(libc::EADDRNOTAVAIL)),
            Error::new(busy.kind),
        ] {
            attempts = 0;
            let error = retry_udp_rebind(|| {
                attempts += 1;
                Err(fatal)
            })
            .expect_err("only EADDRINUSE can retry");
            assert_eq!(attempts, 1);
            assert_eq!(error.os, fatal.os);
            assert_eq!(error.kind, fatal.kind);
        }
    }

    #[test]
    fn default_udp_bind_does_not_enable_address_sharing() {
        let mut checked = 0;
        for addr in ["127.0.0.1:0", "[::1]:0"] {
            retry_udp_rebind(|| {
                let mut backend =
                    Unix::new(&Config::default(), BufferPool::new(2, 64)).expect("backend");
                let first = Handle {
                    owner: 1,
                    key: 1 << 32,
                };
                let second = Handle {
                    owner: 1,
                    key: (1 << 32) | 1,
                };
                backend
                    .open(
                        first,
                        Open::Udp {
                            addr: addr.parse().expect("address"),
                            opts: UdpOpts::default(),
                        },
                    )
                    .expect("first bind");
                let fd = backend
                    .get(first)
                    .expect("resource")
                    .transport
                    .fd
                    .as_raw_fd();
                let mut reuse = -1i32;
                let mut len = std::mem::size_of_val(&reuse) as libc::socklen_t;
                assert_eq!(
                    // SAFETY: live owned socket and correctly sized integer output.
                    unsafe {
                        libc::getsockopt(
                            fd,
                            libc::SOL_SOCKET,
                            libc::SO_REUSEADDR,
                            (&mut reuse as *mut i32).cast(),
                            &mut len,
                        )
                    },
                    0
                );
                assert_eq!(reuse, 0, "default UDP must reserve its endpoint: {addr}");
                let addr = backend.local_addr(first).expect("bound address");
                let error = backend
                    .open(
                        second,
                        Open::Udp {
                            addr,
                            opts: UdpOpts::default(),
                        },
                    )
                    .expect_err("a live default endpoint cannot be shared");
                assert_eq!(error.os, Some(libc::EADDRINUSE));
                backend.release(first);
                backend
                    .open(
                        second,
                        Open::Udp {
                            addr,
                            opts: UdpOpts::default(),
                        },
                    )
                    .inspect_err(|error| {
                        eprintln!("rebind released endpoint {addr}: {error:?}");
                    })?;
                assert_eq!(backend.local_addr(second).expect("rebound address"), addr);
                Ok(())
            })
            .expect("UDP rebind must succeed within 16 complete attempts");
            checked += 1;
        }
        assert_eq!(checked, 2, "IPv4 and IPv6 binding policies ran");
    }

    #[test]
    fn udp_port_sharing_requires_explicit_opt_in() {
        let mut checked = 0;
        for addr in ["127.0.0.1:0", "[::1]:0"] {
            let mut l = Loop::new(Config::default()).expect("loop");
            let opts = UdpOpts {
                reuse_port: ReusePort::Share,
            };
            let first = l
                .udp_bind(addr.parse().expect("address"), &opts)
                .expect("first bind");
            let addr = l.local_addr(first).expect("first address");
            let second = l.udp_bind(addr, &opts).expect("explicit shared bind");
            assert_eq!(l.local_addr(second).expect("second address"), addr);
            let error = l
                .udp_bind(addr, &UdpOpts::default())
                .expect_err("default cannot join");
            assert_eq!(error.os, Some(libc::EADDRINUSE));
            checked += 1;
        }
        assert_eq!(
            checked, 2,
            "both address families exercised explicit sharing"
        );
    }

    #[test]
    fn cancelled_udp_with_cached_events_survives_exact_fd_and_port_reuse() {
        let mut received = 0;
        for addr in ["127.0.0.1:0", "[::1]:0"] {
            retry_udp_rebind(|| {
                let mut backend =
                    Unix::new(&Config::default(), BufferPool::new(2, 64)).expect("backend");
                let old = Handle {
                    owner: 1,
                    key: 1 << 32,
                };
                let next = Handle {
                    owner: 1,
                    key: 2 << 32,
                };
                let old_op = OpId {
                    owner: 1,
                    key: 1 << 32,
                };
                let next_op = OpId {
                    owner: 1,
                    key: 2 << 32,
                };
                backend
                    .open(
                        old,
                        Open::Udp {
                            addr: addr.parse().expect("address"),
                            opts: UdpOpts::default(),
                        },
                    )
                    .expect("old socket");
                let endpoint = backend.local_addr(old).expect("old endpoint");
                let peer = std::net::UdpSocket::bind(addr).expect("peer");
                let from = peer.local_addr().expect("peer address");
                let mut events = Vec::with_capacity(1);
                backend
                    .submit(Request {
                        op: old_op,
                        handle: old,
                        operation: Operation::RecvFrom(ReadBuf::Pooled),
                    })
                    .expect("old receive");
                backend
                    .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
                    .expect("arm old receive");
                assert!(events.is_empty());
                assert_eq!(
                    peer.send_to(&[], endpoint).expect("old zero-byte packet"),
                    0
                );
                // Collect a real old-generation readiness event without consuming
                // its datagram; the next poll must discard this cached event batch.
                backend.polled.clear();
                backend
                    .poller
                    .wait(Some(Duration::from_secs(1)), &mut backend.polled)
                    .expect("old readiness");
                assert!(backend.polled.iter().any(|e| e.key == old.key() && e.read));
                backend.cancel(old_op).expect("cancel old receive");
                backend
                    .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
                    .expect("acknowledgement");
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].op, old_op);
                assert!(events[0].terminal);
                assert!(matches!(events[0].result, Ok(Outcome::Cancelled)));
                events.clear();
                let transport = backend.detach(old).expect("detach quiescent socket");
                let fd = transport.fd.as_raw_fd();
                let replacement = socket::create(endpoint, true).expect("replacement socket");
                assert_ne!(replacement.as_raw_fd(), fd);
                // Keep the destination descriptor owned throughout: dup2 atomically
                // closes the old socket (discarding its queued datagram) and reuses
                // exactly its fd, without racing other tests for an unowned fd slot.
                // SAFETY: distinct live owned fds; transport retains ownership of the
                // destination and replacement owns the source reference.
                assert_eq!(unsafe { libc::dup2(replacement.as_raw_fd(), fd) }, fd);
                drop(replacement);
                socket::configure(fd).expect("restore descriptor flags after dup2");
                let address = Addr::new(endpoint);
                // SAFETY: fd now owns a fresh UDP socket; address has initialized storage.
                if unsafe { libc::bind(fd, address.ptr(), address.len) } < 0 {
                    let error = last_error();
                    eprintln!("bind reused fd {fd} at {endpoint}: {error:?}");
                    return Err(error);
                }
                backend
                    .attach(next, transport)
                    .expect("attach next generation");
                assert_eq!(
                    backend
                        .get(next)
                        .expect("new resource")
                        .transport
                        .fd
                        .as_raw_fd(),
                    fd
                );
                assert_eq!(backend.local_addr(next).expect("reused port"), endpoint);
                assert_eq!(old.index(), next.index());
                assert_ne!(old.key(), next.key());
                backend
                    .submit(Request {
                        op: next_op,
                        handle: next,
                        operation: Operation::RecvFrom(ReadBuf::Pooled),
                    })
                    .expect("next receive");
                assert_eq!(old_op.index(), next_op.index());
                assert_eq!(
                    backend.cancel(old_op).expect_err("stale cancellation").kind,
                    ErrorKind::NotFound
                );
                backend.release(old); // A stale handle must not release the new fd.
                backend
                    .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
                    .expect("arm replacement receive");
                assert!(events.is_empty(), "old event batch crossed generations");
                // A stale event is not a completion, and must not cause a no-spin
                // violation after the new socket reports EAGAIN.
                let at = Instant::now() + Duration::from_millis(2);
                let info = backend
                    .poll(
                        Some(Duration::from_millis(2)),
                        Budget::UNLIMITED,
                        &mut events,
                    )
                    .expect("new empty socket waits");
                assert!(events.is_empty(), "old datagram crossed socket lifetime");
                assert_eq!(info.waits, 1);
                assert_eq!(info.discovery_polls, 0);
                assert!(Instant::now() >= at);
                assert_eq!(peer.send_to(b"new", endpoint).expect("new packet"), 3);
                backend
                    .poll(Some(Duration::from_secs(1)), Budget::UNLIMITED, &mut events)
                    .expect("new delivery");
                assert_eq!(events.len(), 1);
                let event = events.pop().expect("one completion");
                assert_eq!(event.op, next_op);
                assert!(event.terminal);
                match event.result {
                    Ok(Outcome::RecvFrom {
                        n,
                        from: actual,
                        lease: Some(lease),
                    }) => {
                        assert_eq!(actual, from);
                        assert_eq!(n, 3);
                        assert_eq!(lease.as_slice(), b"new");
                    }
                    other => panic!("unexpected reused-socket result: {other:?}"),
                }
                backend
                    .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
                    .expect("no duplicates");
                assert!(events.is_empty());
                Ok(())
            })
            .expect("UDP rebind must succeed within 16 complete attempts");
            received += 1;
        }
        assert_eq!(
            received, 2,
            "IPv4 and IPv6 fd/port reuse delivered new packets"
        );
    }
}

#[cfg(all(test, not(loom)))]
mod process_races {
    use super::*;
    #[test]
    fn child_exited_before_native_registration_is_reaped_once() {
        let mut backend = Unix::new(&Config::default(), BufferPool::new(2, 64)).expect("backend");
        backend.set_notifier(Notifier::new(backend.waker()));
        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 23"])
            .spawn()
            .expect("child");
        let pid = child.id();
        // SAFETY: initialized siginfo output and an owned child PID. WNOWAIT
        // proves exit occurred while deliberately preserving status for turnloop.
        let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            // SAFETY: valid child identity and writable output; wait only for its exit.
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as _,
                    &mut status,
                    libc::WEXITED | libc::WNOWAIT,
                )
            },
            0
        );
        let h = Handle {
            owner: 1,
            key: 1 << 32,
        };
        let op = OpId {
            owner: 1,
            key: 1 << 32,
        };
        backend
            .services
            .child(h, child, false, &mut backend.poller)
            .expect("register already exited child");
        backend
            .submit(Request {
                op,
                handle: h,
                operation: Operation::ProcessExit,
            })
            .expect("exit operation");
        let mut events = Vec::with_capacity(4);
        backend
            .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
            .expect("exit poll");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].result,
            Ok(Outcome::Exited(ExitStatus {
                code: Some(23),
                signal: None
            }))
        ));
        events.clear();
        backend
            .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
            .expect("duplicate check");
        assert!(events.is_empty());
        let mut code = 0;
        assert_eq!(
            // SAFETY: query only the fixture child's wait status, without blocking.
            unsafe { libc::waitpid(pid as i32, &mut code, libc::WNOHANG) },
            -1
        );
        assert_eq!(last_error().os, Some(libc::ECHILD));
    }
}

/// Construct an explicitly bound socket for the portable adoption contract.
#[cfg(test)]
pub(crate) fn bound_tcp_for_test(address: SocketAddr) -> (Detached, SocketAddr) {
    let fd = socket::create(address, false).unwrap();
    let addr = Addr::new(address);
    // SAFETY: owned socket and address storage valid for its recorded length.
    let result = unsafe { libc::bind(fd.as_raw_fd(), addr.ptr(), addr.len) };
    assert_eq!(result, 0);
    let local = socket::local_addr(fd.as_raw_fd()).unwrap();
    (Detached::from_fd(fd).unwrap(), local)
}

/// Unbound socket with a non-default kernel option for adoption tests.
#[cfg(test)]
pub(crate) fn unbound_tcp_for_test(address: SocketAddr) -> Detached {
    let stream = std::net::TcpStream::from(socket::create(address, false).unwrap());
    stream.set_nodelay(true).unwrap();
    Detached::from_fd(stream.into()).unwrap()
}
