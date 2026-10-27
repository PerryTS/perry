//! Per-loop child and signal registrations. All completion storage is reserved.
use super::{
    poller::{Poller, last_error},
    signals::{self, Subscription},
    unix::Detached,
};
use crate::slots::{Slots, page_reserve};
use crate::{
    backend::{Event, Operation, Outcome, Request},
    sync::{AtomicBool, Ordering},
    *,
};
#[cfg(loom)]
use loom::sync::Mutex;
#[cfg(not(loom))]
use std::sync::Mutex;
use std::{
    collections::VecDeque,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::process::ExitStatusExt,
    },
    process::Child,
    sync::Arc,
};

// One coalesced readiness credit per live handle. The atomic empty fast path
// avoids both locking and table scans on idle turns. Dispatcher publication and
// release serialize on the mutex; release removes queued generations before reuse.
pub(super) struct ReadyQueue {
    pending: AtomicBool,
    state: Mutex<ReadyState>,
}

#[cfg(all(test, loom))]
mod readiness_models {
    use super::*;

    #[test]
    fn publication_coalescing_and_generation_reuse() {
        loom::model(|| {
            let ready = loom::sync::Arc::new(ReadyQueue::new(2));
            let a = Handle {
                owner: 1,
                key: 1 << 32,
            };
            let b = Handle {
                owner: 1,
                key: (1 << 32) | 1,
            };
            let producer = ready.clone();
            let thread = loom::thread::spawn(move || producer.push(a));
            ready.push(b);
            let first = ready.pop().expect("published local readiness");
            thread.join().expect("producer");
            let second = ready.pop().expect("published peer readiness");
            assert_ne!(first, second);
            assert!([first, second].contains(&a));
            assert!([first, second].contains(&b));
            assert!(!ready.has_work());
            ready.push(a);
            ready.push(a);
            assert_eq!(ready.pop(), Some(a));
            assert!(ready.pop().is_none(), "coalesced credit");
            ready.push(a);
            // Unsubscribe has joined dispatcher publication before removal.
            ready.remove(a);
            let reused = Handle {
                owner: 1,
                key: 2 << 32,
            };
            ready.push(reused);
            assert_eq!(ready.pop(), Some(reused));
            assert!(!ready.has_work());
        });
    }
}
struct ReadyState {
    queue: VecDeque<Handle>,
    /// Whether each handle is already in `queue`, so a repeat publication does
    /// not enqueue it twice. Vacant reads as "not queued".
    queued: Slots<()>,
}
impl ReadyQueue {
    fn new(capacity: usize) -> Self {
        let ready = Self {
            pending: AtomicBool::new(false),
            state: Mutex::new(ReadyState {
                queue: VecDeque::with_capacity(page_reserve(capacity)),
                queued: Slots::new(capacity),
            }),
        };
        // Initialize Darwin's lazily allocated pthread mutex during loop setup,
        // so first dispatcher publication also uses only reserved storage.
        drop(
            ready
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        ready
    }
    pub fn push(&self, h: Handle) {
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.queued[h.index()].is_none() {
            s.queued[h.index()] = Some(());
            s.queue.push_back(h);
            self.pending.store(true, Ordering::Release);
        }
    }
    fn pop(&self) -> Option<Handle> {
        if !self.has_work() {
            return None;
        }
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let h = s.queue.pop_front()?;
        s.queued[h.index()] = None;
        self.pending.store(!s.queue.is_empty(), Ordering::Release);
        Some(h)
    }
    fn remove(&self, h: Handle) {
        let mut s = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.queue.retain(|&queued| queued != h);
        s.queued[h.index()] = None;
        self.pending.store(!s.queue.is_empty(), Ordering::Release);
    }
    fn has_work(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}

struct ChildState {
    child: Child,
    status: Option<ExitStatus>,
    group: bool,
    pidfd: Option<OwnedFd>,
    fallback: Option<Subscription>,
}
impl ChildState {
    fn save_status(&mut self, status: std::process::ExitStatus) {
        self.status = Some(ExitStatus {
            code: status.code(),
            signal: status.signal(),
        });
    }
    fn reap(&mut self) -> Result<Option<ExitStatus>> {
        if self.status.is_none()
            && let Some(status) = self.child.try_wait().map_err(Error::from)?
        {
            self.save_status(status);
        }
        Ok(self.status)
    }
    fn kill(&mut self, signal: Signal, group: bool) -> Result<()> {
        if self.status.is_some() {
            return Err(Error::new(ErrorKind::NotFound));
        }
        if group && !self.group {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let pid = self.child.id() as i32;
        let signal = signals::number(signal)?;
        // SAFETY: child remains unreaped, so its positive PID cannot be reused;
        // negative PID is only permitted for the group we created for this child.
        if unsafe { libc::kill(if group { -pid } else { pid }, signal) } < 0 {
            return Err(last_error());
        }
        Ok(())
    }
}
impl Drop for ChildState {
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.kill(Signal::Kill, self.group);
            // Child::wait retries EINTR and reaps only this owned child, never others.
            let _ = self.child.wait();
        }
    }
}
enum Kind {
    Child(ChildState),
    Signal(Signal, Subscription),
}
struct Entry {
    handle: Handle,
    kind: Kind,
    op: Option<OpId>,
    cancelled: bool,
}
pub(super) struct Services {
    entries: Slots<Entry>,
    operations: Slots<Handle>,
    ready: Arc<ReadyQueue>,
    notifier: Option<Notifier>,
}
impl Services {
    pub fn new(config: &Config) -> Self {
        Self {
            entries: Slots::new(config.max_handles),
            operations: Slots::new(config.max_operations),
            ready: Arc::new(ReadyQueue::new(config.max_handles)),
            notifier: None,
        }
    }
    pub fn set_notifier(&mut self, notifier: Notifier) {
        self.notifier = Some(notifier);
    }
    fn notifier(&self) -> Result<Notifier> {
        self.notifier
            .clone()
            .ok_or(Error::new(ErrorKind::InvalidInput))
    }
    pub fn contains(&self, h: Handle) -> bool {
        self.entries
            .get(h.index())
            .and_then(Option::as_ref)
            .is_some_and(|e| e.handle == h)
    }
    pub fn signal(&mut self, h: Handle, signal: Signal) -> Result<()> {
        let ticket = signals::subscribe(signal, self.notifier()?, h, self.ready.clone())?;
        self.entries[h.index()] = Some(Entry {
            handle: h,
            kind: Kind::Signal(signal, ticket),
            op: None,
            cancelled: false,
        });
        Ok(())
    }
    pub fn child<P: Poller>(
        &mut self,
        h: Handle,
        child: Child,
        group: bool,
        poller: &mut P,
    ) -> Result<()> {
        self.register_child(h, child, group, |pid, key| poller.process(pid, key))
    }
    fn register_child(
        &mut self,
        h: Handle,
        child: Child,
        group: bool,
        register: impl FnOnce(u32, u64) -> Result<Option<OwnedFd>>,
    ) -> Result<()> {
        let mut state = ChildState {
            child,
            group,
            status: None,
            pidfd: None,
            fallback: None,
        };
        // NOTE_EXIT can precede waitpid visibility on a heavily loaded kqueue.
        // A shared SIGCHLD subscription supplies a later reaping opportunity,
        // without polling or a thread per child. Subscribe before the first check.
        state.fallback = Some(signals::subscribe(
            Signal::Chld,
            self.notifier()?,
            h,
            self.ready.clone(),
        )?);
        match register(state.child.id(), h.key()) {
            Ok(fd) => state.pidfd = fd,
            Err(e) => {
                if e.os == Some(libc::ESRCH) {
                    // Registration can see an exiting task before WNOHANG sees
                    // its wait status (notably XNU's EVFILT_PROC). It is already
                    // leaving: wait for this owned PID, retrying EINTR in std,
                    // and retain the normal exit for the submitted operation.
                    // This happens during spawn, never inside a loop turn.
                    let status = state.child.wait().map_err(Error::from)?;
                    state.save_status(status);
                }
                // A child can exit between spawn and kqueue registration. Reap
                // immediately before interpreting registration failure.
                if state.reap()?.is_none() {
                    #[cfg(turnloop_backend = "epoll")]
                    if matches!(e.os, Some(libc::ENOSYS | libc::EINVAL | libc::EPERM)) {
                        // The subscription above already covers the SIGCHLD
                        // fallback; recheck after the failed pidfd registration.
                        state.reap()?;
                    } else {
                        return Err(e);
                    }
                    #[cfg(turnloop_backend = "kqueue")]
                    return Err(e);
                }
            }
        }
        self.entries[h.index()] = Some(Entry {
            handle: h,
            kind: Kind::Child(state),
            op: None,
            cancelled: false,
        });
        Ok(())
    }
    pub fn submit(&mut self, request: &Request) -> Result<()> {
        let e = self
            .entries
            .get_mut(request.handle.index())
            .and_then(Option::as_mut)
            .filter(|e| e.handle == request.handle)
            .ok_or(Error::new(ErrorKind::NotFound))?;
        if e.op.is_some()
            || !matches!(
                (&e.kind, &request.operation),
                (Kind::Child(_), Operation::ProcessExit)
                    | (Kind::Signal(..), Operation::WatchSignal)
            )
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        e.op = Some(request.op);
        e.cancelled = false;
        self.operations[request.op.index()] = Some(request.handle);
        // The first child probe covers exit during successful registration.
        if matches!(e.kind, Kind::Child(_)) || matches!(&e.kind, Kind::Signal(_, t) if t.ready()) {
            self.ready.push(request.handle);
        }
        Ok(())
    }
    pub fn cancel(&mut self, op: OpId) -> bool {
        let Some(h) = self.operations.get(op.index()).copied().flatten() else {
            return false;
        };
        let e = self.entries[h.index()].as_mut().expect("service operation");
        if e.op != Some(op) {
            return false;
        }
        e.cancelled = true;
        self.ready.push(h);
        true
    }
    pub fn ready(&mut self, key: u64) -> bool {
        if let Some(e) = self
            .entries
            .get_mut(key as u32 as usize)
            .and_then(Option::as_mut)
            .filter(|e| e.handle.key() == key)
        {
            if e.op.is_some() {
                self.ready.push(e.handle);
            }
            return true;
        }
        false
    }
    pub fn has_work(&self) -> bool {
        self.ready.has_work()
    }
    pub fn poll(&mut self, events: &mut Vec<Event<Detached>>) {
        // Bound work even if a producer continuously replenishes readiness.
        for _ in 0..events.capacity() {
            if events.len() == events.capacity() {
                break;
            }
            let Some(h) = self.ready.pop() else {
                break;
            };
            let Some(e) = self.entries[h.index()]
                .as_mut()
                .filter(|e| e.handle == h && e.op.is_some())
            else {
                continue;
            };
            let (result, terminal) = match &mut e.kind {
                Kind::Signal(signal, ticket) => {
                    if e.cancelled {
                        (Ok(Outcome::Cancelled), true)
                    } else {
                        if !ticket.take() {
                            continue;
                        }
                        (Ok(Outcome::Signal(*signal)), false)
                    }
                }
                Kind::Child(c) => {
                    if let Some(ticket) = &c.fallback {
                        ticket.take();
                    }
                    match c.reap() {
                        Ok(Some(status)) => {
                            c.fallback = None;
                            (
                                Ok(if e.cancelled {
                                    Outcome::Cancelled
                                } else {
                                    Outcome::Exited(status)
                                }),
                                true,
                            )
                        }
                        Ok(None) => continue,
                        Err(error) => (Err(error), true),
                    }
                }
            };
            let op = e.op.expect("runnable operation");
            if terminal {
                e.op = None;
                self.operations[op.index()] = None;
            }
            events.push(Event {
                op,
                terminal,
                result,
            });
        }
    }
    pub fn prepare_close(&mut self, h: Handle) -> Result<()> {
        if let Some(e) = self
            .entries
            .get_mut(h.index())
            .and_then(Option::as_mut)
            .filter(|e| e.handle == h)
            && let Kind::Child(c) = &mut e.kind
        {
            if c.reap()?.is_none() {
                c.kill(Signal::Kill, c.group)?;
            }
            self.ready.push(h);
        }
        Ok(())
    }
    pub fn kill(&mut self, h: Handle, signal: Signal, group: bool) -> Result<()> {
        let e = self
            .entries
            .get_mut(h.index())
            .and_then(Option::as_mut)
            .filter(|e| e.handle == h)
            .ok_or(Error::new(ErrorKind::NotFound))?;
        if let Kind::Child(c) = &mut e.kind {
            c.kill(signal, group)
        } else {
            Err(Error::new(ErrorKind::InvalidInput))
        }
    }
    pub fn release<P: Poller>(&mut self, h: Handle, poller: &mut P) {
        if self.contains(h) {
            if let Some(Entry {
                kind: Kind::Child(c),
                ..
            }) = self.entries[h.index()].as_ref()
            {
                poller.remove_process(c.child.id(), c.pidfd.as_ref().map(AsRawFd::as_raw_fd));
            }
            self.entries[h.index()] = None;
        }
        // Also clean up failed registrations. Dropping their subscription first
        // waits for any dispatcher delivery before the handle slot can be reused.
        if self.ready.has_work() {
            self.ready.remove(h);
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn esrch_before_wait_status_is_visible_still_delivers_exit_once() {
        use std::{
            io::Write,
            process::{Command, Stdio},
            time::Duration,
        };
        let driver = crate::Loop::new(Config::default()).expect("notifier loop");
        let mut services = Services::new(&Config::default());
        services.set_notifier(driver.notifier());
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read value; exit 23"])
            .stdin(Stdio::piped())
            .spawn()
            .expect("waiting child");
        let mut input = child.stdin.take().expect("child input");
        let pid = child.id();
        let h = Handle {
            owner: 1,
            key: 1 << 32,
        };
        let op = OpId {
            owner: 1,
            key: 1 << 32,
        };
        let mut injected = 0;
        let mut release = None;
        let result = services.register_child(h, child, false, |registered, key| {
            assert_eq!((registered, key), (pid, h.key()));
            let mut status = 0;
            assert_eq!(
                // SAFETY: query this owned fixture PID only, without reaping a live child.
                unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
                0
            );
            injected += 1;
            // Force the registration error while the wait status is unavailable.
            // The delayed exit models the kernel gap after EVFILT_PROC/pidfd ESRCH.
            release = Some(std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                input.write_all(b"exit\n")
            }));
            Err(std::io::Error::from_raw_os_error(libc::ESRCH).into())
        });
        let released = release
            .expect("registration hook ran")
            .join()
            .expect("exit thread");
        assert_eq!(injected, 1);
        result.expect("ESRCH is an exiting owned child");
        released.expect("child exited normally, not killed during failed setup");
        services
            .submit(&Request {
                op,
                handle: h,
                operation: Operation::ProcessExit,
            })
            .expect("submit exit");
        let mut events = Vec::with_capacity(1);
        services.poll(&mut events);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].op, op);
        assert!(events[0].terminal);
        assert!(matches!(
            events[0].result,
            Ok(Outcome::Exited(ExitStatus {
                code: Some(23),
                signal: None
            }))
        ));
        events.clear();
        services.poll(&mut events);
        assert!(events.is_empty(), "no duplicate exit");
        let mut status = 0;
        assert_eq!(
            // SAFETY: WNOHANG query only of the fixture PID; ECHILD proves reaping.
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(last_error().os, Some(libc::ECHILD));
    }
}
