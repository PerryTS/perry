//! Reusable regular-file jobs on the shared blocking pool; never file I/O in poll.
use super::{poller::last_error, unix::Detached};
use crate::slots::{Slots, page_reserve};
use crate::{
    backend::{Event, Operation, Outcome, Request},
    blocking::ReusableWork,
    queue::Queue,
    *,
};
use std::{
    collections::VecDeque,
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
struct Job {
    request: Request,
    fd: OwnedFd,
}
// SAFETY: accepted file requests contain only owned bytes/fds or host byte regions
// whose exclusive/immutable lifetime extends through completion or driver drop.
// No Rc, pool lease or thread-affine host object is moved to a worker.
unsafe impl Send for Job {}
enum FileOutcome {
    Read(usize),
    Wrote(usize),
    Eof,
    Cancelled,
}
struct FileEvent {
    op: OpId,
    result: Result<FileOutcome>,
}
struct Port {
    results: Queue<FileEvent>,
    notifier: Mutex<Option<Notifier>>,
    running: Mutex<usize>,
    quiescent: Condvar,
}
struct Slot {
    job: Mutex<Option<Job>>,
    cancel: AtomicBool,
    port: Arc<Port>,
}
impl ReusableWork for Slot {
    fn run(&self) {
        let job = self
            .job
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("submitted file job");
        let op = job.request.op;
        let result = if self.cancel.load(Ordering::Acquire) {
            Ok(FileOutcome::Cancelled)
        } else {
            execute(job)
        };
        // Kernel buffer access has ended before publication. Slot reuse is safe
        // after the terminal event is delivered, including cancellation races.
        assert!(
            self.port.results.push(FileEvent { op, result }).is_ok(),
            "reserved file completion credit"
        );
        if let Some(notifier) = self
            .port
            .notifier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            let _ = notifier.notify();
        }
        let mut running = self
            .port
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *running -= 1;
        self.port.quiescent.notify_all();
    }
}
struct Active {
    op: OpId,
    handle: Handle,
    next: Option<usize>,
    request: Option<Request>,
    fd: OwnedFd,
    submitted: bool,
    cancelled: bool,
    multishot: bool,
    awaiting_pool: bool,
}
pub(super) struct Files {
    /// Per-operation job slots, built when an operation index is first used.
    slots: Slots<Arc<Slot>>,
    active: Slots<Active>,
    heads: Slots<usize>,
    tails: Slots<usize>,
    port: Arc<Port>,
    config: PoolConfig,
    pool: BufferPool,
    leases: Slots<BufLease>,
    ready: VecDeque<usize>,
    awaiting_pool: VecDeque<usize>,
}
/// Darwin's std mutex allocates its pthread storage on first lock, so the slot
/// takes its own lock here: publishing a job later uses only reserved storage.
fn build_slot(port: &Arc<Port>) -> Arc<Slot> {
    let slot = Arc::new(Slot {
        job: Mutex::new(None),
        cancel: AtomicBool::new(false),
        port: port.clone(),
    });
    drop(
        slot.job
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    slot
}
impl Files {
    pub fn new(config: &Config, pool: BufferPool) -> Self {
        let port = Arc::new(Port {
            results: Queue::new(config.max_operations.max(2).next_power_of_two()),
            notifier: Mutex::new(None),
            running: Mutex::new(0),
            quiescent: Condvar::new(),
        });
        Self {
            slots: Slots::filled(config.max_operations, || build_slot(&port)),
            active: Slots::new(config.max_operations),
            heads: Slots::new(config.max_handles),
            tails: Slots::new(config.max_handles),
            port,
            config: config.blocking_pool,
            pool,
            leases: Slots::new(config.max_operations),
            ready: VecDeque::with_capacity(page_reserve(config.max_handles)),
            awaiting_pool: VecDeque::with_capacity(page_reserve(config.max_handles)),
        }
    }
    pub fn set_notifier(&mut self, notifier: Notifier) {
        *self
            .port
            .notifier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(notifier);
    }
    pub fn submit(&mut self, request: Request, fd: OwnedFd) -> Result<()> {
        let multishot = match request.operation {
            Operation::Read { multishot, .. } => multishot,
            Operation::Write(_) | Operation::Writev(_) => false,
            _ => return Err(Error::new(ErrorKind::Unsupported)),
        };
        if matches!(&request.operation, Operation::Read { buf: ReadBuf::Provided(b), .. } if b.is_empty())
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let op = request.op;
        let h = request.handle.index();
        if let Some(tail) = self.tails[h] {
            self.active[tail].as_mut().expect("file tail").next = Some(op.index());
        } else {
            self.heads[h] = Some(op.index());
            self.ready.push_back(op.index());
        }
        self.tails[h] = Some(op.index());
        self.active[op.index()] = Some(Active {
            op,
            handle: request.handle,
            next: None,
            request: Some(request),
            fd,
            submitted: false,
            cancelled: false,
            multishot,
            awaiting_pool: false,
        });
        Ok(())
    }
    /// This operation's job slot, built on first use.
    ///
    /// Darwin's std mutex allocates its pthread storage on first lock, so the
    /// slot takes its own lock here: a job publication from the loop thread only
    /// ever uses storage this reserved, including slots first reached later
    /// through backpressure or cancellation.
    fn slot(&mut self, i: usize) -> &Arc<Slot> {
        let port = self.port.clone();
        self.slots.get_or_insert_with(i, || build_slot(&port))
    }
    pub fn cancel(&mut self, op: OpId) -> bool {
        let Some(active) = self
            .active
            .get_mut(op.index())
            .and_then(Option::as_mut)
            .filter(|a| a.op == op)
        else {
            return false;
        };
        active.cancelled = true;
        if active.awaiting_pool {
            active.awaiting_pool = false;
            self.awaiting_pool.retain(|&i| i != op.index());
            self.ready.push_back(op.index());
        }
        if active.submitted {
            self.slots[op.index()]
                .as_ref()
                .expect("a submitted request built its slot")
                .cancel
                .store(true, Ordering::Release);
        }
        true
    }
    pub fn has_work(&self) -> bool {
        !self.port.results.is_empty()
            || !self.ready.is_empty()
            || (!self.awaiting_pool.is_empty() && self.pool.available())
    }
    fn start(&mut self) {
        // Retry only after a lease is available, never by scanning idle handles
        // or running jobs. At most one queue credit exists per file head.
        if !self.awaiting_pool.is_empty() && self.pool.available() {
            while let Some(i) = self.awaiting_pool.pop_front() {
                self.active[i].as_mut().expect("pool waiter").awaiting_pool = false;
                self.ready.push_back(i);
            }
        }
        while let Some(i) = self.ready.pop_front() {
            let slot = self.slot(i).clone();
            let active = self.active[i].as_mut().expect("file head");
            debug_assert!(!active.submitted);
            if active.cancelled {
                active.submitted = true;
                active.request = None;
                assert!(
                    self.port
                        .results
                        .push(FileEvent {
                            op: active.op,
                            result: Ok(FileOutcome::Cancelled)
                        })
                        .is_ok()
                );
                continue;
            }
            let needs_pool = matches!(
                active.request.as_ref().map(|r| &r.operation),
                Some(Operation::Read {
                    buf: ReadBuf::Pooled,
                    ..
                })
            );
            if needs_pool {
                let Some(mut lease) = self.pool.acquire() else {
                    active.awaiting_pool = true;
                    self.awaiting_pool.push_back(i);
                    continue;
                };
                let bytes = lease.writable();
                // SAFETY: exclusive lease is held by this loop until worker acknowledgement.
                let buf = unsafe { IoBufMut::from_raw_parts(bytes.as_mut_ptr(), bytes.len()) };
                active.request.as_mut().expect("pending request").operation = Operation::Read {
                    buf: ReadBuf::Provided(buf),
                    multishot: false,
                };
                self.leases[i] = Some(lease);
            }
            let result = active.fd.try_clone().map_err(Error::from).and_then(|fd| {
                slot.cancel.store(false, Ordering::Release);
                *slot
                    .job
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Job {
                    request: active.request.take().expect("pending file job"),
                    fd,
                });
                *self
                    .port
                    .running
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
                if let Err(e) = crate::blocking::reusable(self.config, slot.clone()) {
                    slot.job
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take();
                    *self
                        .port
                        .running
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) -= 1;
                    return Err(e);
                }
                Ok(())
            });
            active.submitted = true;
            if let Err(e) = result {
                assert!(
                    self.port
                        .results
                        .push(FileEvent {
                            op: active.op,
                            result: Err(e)
                        })
                        .is_ok()
                );
            }
        }
    }
    pub fn poll(&mut self, events: &mut Vec<Event<Detached>>) {
        self.start();
        while events.len() < events.capacity() {
            let Some(event) = self.port.results.pop() else {
                break;
            };
            let i = event.op.index();
            let active = self.active[i].as_mut().expect("file completion");
            let terminal = active.cancelled
                || !active.multishot
                || !matches!(event.result, Ok(FileOutcome::Read(_)));
            let mut lease = self.leases[i].take();
            let result = event.result.map(|r| match r {
                FileOutcome::Read(n) => {
                    if let Some(b) = &mut lease {
                        b.set_len(n);
                    }
                    Outcome::Read { n, lease }
                }
                FileOutcome::Wrote(n) => Outcome::Wrote(n),
                FileOutcome::Eof => Outcome::Eof,
                FileOutcome::Cancelled => Outcome::Cancelled,
            });
            if terminal {
                let active = self.active[i].take().expect("completed file job");
                self.heads[active.handle.index()] = active.next;
                if active.next.is_none() {
                    self.tails[active.handle.index()] = None;
                } else if let Some(next) = active.next {
                    self.ready.push_back(next);
                }
            } else {
                active.submitted = false;
                active.request = Some(Request {
                    op: event.op,
                    handle: active.handle,
                    operation: Operation::Read {
                        buf: ReadBuf::Pooled,
                        multishot: true,
                    },
                });
                self.ready.push_back(i);
            }
            events.push(Event {
                op: event.op,
                terminal,
                result,
            });
        }
        self.start();
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        for a in self.active.iter().flatten() {
            // A request that never started has no slot, and nothing to cancel.
            if let Some(slot) = self.slots[a.op.index()].as_ref() {
                slot.cancel.store(true, Ordering::Release);
            }
        }
        let mut running = self
            .port
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *running != 0 {
            running = self
                .port
                .quiescent
                .wait(running)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}
fn execute(mut job: Job) -> Result<FileOutcome> {
    let fd = job.fd.as_raw_fd();
    match &mut job.request.operation {
        Operation::Read { buf, .. } => {
            let ReadBuf::Provided(buf) = buf else {
                return Err(Error::new(ErrorKind::InvalidInput));
            };
            let (ptr, len) = (buf.as_mut_ptr(), buf.len());
            let n = loop {
                // SAFETY: operation owns stable writable memory until this job ends.
                let n = unsafe { libc::read(fd, ptr.cast(), len) };
                if n >= 0 {
                    break n as usize;
                }
                let e = last_error();
                if e.os != Some(libc::EINTR) {
                    return Err(e);
                }
            };
            if n == 0 {
                Ok(FileOutcome::Eof)
            } else {
                Ok(FileOutcome::Read(n))
            }
        }
        Operation::Write(buf) => write_all(fd, buf.as_slice()).map(FileOutcome::Wrote),
        Operation::Writev(bufs) => {
            let mut n = 0;
            for buf in bufs.bufs.iter().flatten() {
                n += write_all(fd, buf.as_slice())?;
            }
            Ok(FileOutcome::Wrote(n))
        }
        _ => Err(Error::new(ErrorKind::Unsupported)),
    }
}
fn write_all(fd: i32, mut bytes: &[u8]) -> Result<usize> {
    let length = bytes.len();
    while !bytes.is_empty() {
        // SAFETY: synchronous regular-file write from initialized owned/provided bytes.
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            let e = last_error();
            if e.os == Some(libc::EINTR) {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            return Err(Error::new(ErrorKind::BrokenPipe));
        }
        bytes = &bytes[n as usize..];
    }
    Ok(length)
}
