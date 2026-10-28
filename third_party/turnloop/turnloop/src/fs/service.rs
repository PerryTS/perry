//! Native typed filesystem requests on the shared bounded blocking pool.
//!
//! Storage is reserved per operation slot and per handle when the loop starts, so
//! steady-state requests reuse it. Each accepted request reserves one pool queue
//! slot, which makes starting a queued FIFO successor infallible and bounded.
//! Only the head of a handle's FIFO is ever on the pool; a request needing a
//! pooled lease waits (without spinning) until one is available.
use super::{FileMetadata, FsOutput, FsRequest};
use crate::slots::{Slots, page_reserve};
use crate::{
    BufLease, BufferPool, Config, Error, ErrorKind, Handle, IoBufMut, OpId, PoolConfig, ReadBuf,
    Result,
    blocking::{ReusableWork, WorkOutput, WorkPort, WorkResult},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};
#[cfg(unix)]
#[path = "unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "windows.rs"]
mod sys;

pub(super) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Native object behind one file or directory handle.
#[derive(Default)]
pub(super) enum Object {
    /// Not opened yet, or released.
    #[default]
    Empty,
    /// Closed by an explicit Close request.
    Closed,
    File(sys::File),
    Dir(sys::Dir),
}

struct Job {
    op: OpId,
    request: FsRequest,
    object: Option<Arc<Mutex<Object>>>,
}
// SAFETY: a job carries prepared paths, owned bytes, handle state guarded by a
// mutex, and host buffers whose D3 contract keeps them valid and exclusively
// owned by this request until its result is published or the loop is dropped.
// `Service::drop` withdraws queued jobs and waits for running ones.
unsafe impl Send for Job {}

struct Shared {
    work: Arc<WorkPort>,
    running: Mutex<usize>,
    idle: Condvar,
}
/// Compact worker result for the loop's completion queue. Metadata stays in the
/// operation slot, keeping every queued completion small.
pub(crate) enum Reply {
    Opened,
    Read(usize),
    Wrote(usize),
    Metadata,
    Directory { n: usize, eof: bool },
    Bytes(usize),
    Done,
}
impl Reply {
    pub fn output(self, metadata: Option<FileMetadata>) -> FsOutput {
        match self {
            Self::Opened => FsOutput::Opened,
            Self::Read(n) => FsOutput::Read(n),
            Self::Wrote(n) => FsOutput::Wrote(n),
            Self::Metadata => FsOutput::Metadata(metadata.expect("published metadata")),
            Self::Directory { n, eof } => FsOutput::Directory { n, eof },
            Self::Bytes(n) => FsOutput::Bytes(n),
            Self::Done => FsOutput::Done,
        }
    }
}
struct Slot {
    job: Mutex<Option<Job>>,
    metadata: Mutex<Option<FileMetadata>>,
    cancel: AtomicBool,
    shared: Arc<Shared>,
}
impl ReusableWork for Slot {
    fn run(&self) {
        // A dropped loop withdraws queued jobs; the queue entry then does nothing.
        let Some(job) = lock(&self.job).take() else {
            return;
        };
        let op = job.op;
        let result = if self.cancel.load(Ordering::Acquire) {
            drop(job);
            Err(Error::new(ErrorKind::Cancelled))
        } else {
            execute(job)
        };
        let result = result.map(|output| match output {
            FsOutput::Opened => Reply::Opened,
            FsOutput::Read(n) => Reply::Read(n),
            FsOutput::Wrote(n) => Reply::Wrote(n),
            FsOutput::Metadata(metadata) => {
                *lock(&self.metadata) = Some(metadata);
                Reply::Metadata
            }
            FsOutput::Directory { n, eof } => Reply::Directory { n, eof },
            FsOutput::Bytes(n) => Reply::Bytes(n),
            FsOutput::Done => Reply::Done,
        });
        // The job and its buffers were consumed above: no access after publication.
        self.shared.work.complete(WorkResult {
            op,
            result: result.map(WorkOutput::Fs),
        });
        let mut running = lock(&self.shared.running);
        *running -= 1;
        self.shared.idle.notify_all();
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Stage {
    /// Accepted behind the handle's FIFO head.
    Queued,
    /// FIFO head waiting for a pooled lease.
    Waiting,
    /// Handed to the pool.
    Running,
    /// Cancelled before starting; the terminal result is already published.
    Withdrawn,
}
struct Active {
    op: OpId,
    handle: Option<Handle>,
    previous: Option<usize>,
    next: Option<usize>,
    stage: Stage,
    request: Option<FsRequest>,
    lease: Option<BufLease>,
}

pub(crate) struct Service {
    /// Per-operation job slots, built when an operation index is first used.
    slots: Slots<Arc<Slot>>,
    /// Per-handle object cells, built when a handle index is first used.
    objects: Slots<Arc<Mutex<Object>>>,
    active: Slots<Active>,
    heads: Slots<usize>,
    tails: Slots<usize>,
    waiting: VecDeque<usize>,
    shared: Arc<Shared>,
    pool: BufferPool,
    config: PoolConfig,
}
/// Darwin's std mutex allocates its storage on first lock, so both locks are
/// taken here: publishing a job later uses only storage reserved with the slot.
fn build_slot(shared: &Arc<Shared>) -> Arc<Slot> {
    let slot = Arc::new(Slot {
        job: Mutex::new(None),
        metadata: Mutex::new(None),
        cancel: AtomicBool::new(false),
        shared: shared.clone(),
    });
    drop(lock(&slot.job));
    drop(lock(&slot.metadata));
    slot
}
/// See [`build_slot`]: the cell's lock storage is reserved with the cell.
fn build_object() -> Arc<Mutex<Object>> {
    let object = Arc::new(Mutex::new(Object::Empty));
    drop(lock(&object));
    object
}
impl Service {
    pub fn new(config: &Config, work: Arc<WorkPort>, pool: BufferPool) -> Self {
        let shared = Arc::new(Shared {
            work,
            running: Mutex::new(0),
            idle: Condvar::new(),
        });
        // Darwin's std mutex allocates its storage on first lock: do it at setup.
        drop(lock(&shared.running));
        Self {
            slots: Slots::filled(config.max_operations, || build_slot(&shared)),
            objects: Slots::filled(config.max_handles, build_object),
            active: Slots::new(config.max_operations),
            heads: Slots::new(config.max_handles),
            tails: Slots::new(config.max_handles),
            waiting: VecDeque::with_capacity(page_reserve(config.max_operations)),
            shared,
            pool,
            config: config.blocking_pool,
        }
    }
    /// This operation's job slot, built on first use.
    ///
    /// Darwin's std mutex allocates its storage on first lock, so both locks are
    /// taken here: the slot's storage is reserved when the slot is built, never
    /// on a later publication from the loop thread.
    fn slot(&mut self, i: usize) -> &Arc<Slot> {
        let shared = self.shared.clone();
        self.slots.get_or_insert_with(i, || build_slot(&shared))
    }
    /// This handle's object cell, built on first use. See [`Service::slot`].
    fn object(&mut self, i: usize) -> &Arc<Mutex<Object>> {
        self.objects.get_or_insert_with(i, build_object)
    }
    /// Accept a request, or reject it without retaining or touching its buffers.
    pub fn submit(&mut self, op: OpId, handle: Option<Handle>, request: FsRequest) -> Result<()> {
        crate::blocking::reserve(self.config)?;
        let i = op.index();
        debug_assert!(self.active[i].is_none());
        let previous = handle.and_then(|h| self.tails[h.index()]);
        self.active[i] = Some(Active {
            op,
            handle,
            previous,
            next: None,
            stage: Stage::Queued,
            request: Some(request),
            lease: None,
        });
        if let Some(h) = handle {
            match previous {
                Some(tail) => self.active[tail].as_mut().expect("FIFO tail").next = Some(i),
                None => self.heads[h.index()] = Some(i),
            }
            self.tails[h.index()] = Some(i);
        }
        if previous.is_none() {
            self.start(i);
        }
        Ok(())
    }
    fn start(&mut self, i: usize) {
        let active = self.active[i].as_mut().expect("startable request");
        debug_assert!(matches!(active.stage, Stage::Queued | Stage::Waiting));
        let request = active.request.as_mut().expect("unstarted request");
        if let Some(buffer) = request.read_buffer()
            && matches!(buffer, ReadBuf::Pooled)
        {
            let Some(mut lease) = self.pool.acquire() else {
                if active.stage != Stage::Waiting {
                    active.stage = Stage::Waiting;
                    self.waiting.push_back(i);
                }
                return;
            };
            let bytes = lease.writable();
            // SAFETY: the lease stays in `Active` until the loop thread consumes the
            // published result, so this region outlives the worker's access.
            *buffer = ReadBuf::Provided(unsafe {
                IoBufMut::from_raw_parts(bytes.as_mut_ptr(), bytes.len())
            });
            active.lease = Some(lease);
        }
        active.stage = Stage::Running;
        let (op, request, handle) = (
            active.op,
            active.request.take().expect("unstarted request"),
            active.handle,
        );
        let object = handle.map(|h| self.object(h.index()).clone());
        let slot = self.slot(i).clone();
        slot.cancel.store(false, Ordering::Release);
        *lock(&slot.job) = Some(Job {
            op,
            request,
            object,
        });
        *lock(&self.shared.running) += 1;
        crate::blocking::push_reserved(slot.clone());
    }
    /// Start requests that were waiting for a pooled lease.
    pub fn retry(&mut self) {
        while self.pool.available() {
            let Some(i) = self.waiting.pop_front() else {
                break;
            };
            self.start(i);
        }
    }
    /// Whether `retry` can make progress now.
    pub fn has_work(&self) -> bool {
        !self.waiting.is_empty() && self.pool.available()
    }
    fn unlink(&mut self, i: usize) {
        let active = self.active[i].as_mut().expect("linked request");
        let (previous, next, handle) = (active.previous.take(), active.next.take(), active.handle);
        let Some(h) = handle else {
            return;
        };
        match previous {
            Some(p) => self.active[p].as_mut().expect("FIFO predecessor").next = next,
            None => self.heads[h.index()] = next,
        }
        match next {
            Some(n) => self.active[n].as_mut().expect("FIFO successor").previous = previous,
            None => self.tails[h.index()] = previous,
        }
    }
    /// Begin cancellation. Unstarted requests publish Cancelled at once; running
    /// ones finish (or observe the flag first) and are reported as Cancelled.
    pub fn cancel(&mut self, op: OpId) {
        let i = op.index();
        let Some(active) = self.active[i].as_mut().filter(|a| a.op == op) else {
            return;
        };
        match active.stage {
            Stage::Running => self.slot(i).cancel.store(true, Ordering::Release),
            Stage::Withdrawn => {}
            Stage::Queued | Stage::Waiting => {
                let was_head = active.previous.is_none();
                if active.stage == Stage::Waiting {
                    self.waiting.retain(|&w| w != i);
                }
                active.stage = Stage::Withdrawn;
                active.request = None;
                let next = active.next;
                self.unlink(i);
                crate::blocking::unreserve();
                self.shared.work.complete(WorkResult {
                    op,
                    result: Err(Error::new(ErrorKind::Cancelled)),
                });
                if was_head && let Some(next) = next {
                    self.start(next);
                }
            }
        }
    }
    /// Consume the published terminal result: advance the FIFO and return the
    /// lease and any metadata the worker stored in the slot.
    pub fn complete(&mut self, op: OpId) -> (Option<BufLease>, Option<FileMetadata>) {
        let i = op.index();
        let Some(active) = self.active[i].as_ref().filter(|a| a.op == op) else {
            return (None, None);
        };
        if active.stage == Stage::Running {
            let next = active.next;
            self.unlink(i);
            if let Some(next) = next {
                self.start(next);
            }
        }
        let metadata = lock(&self.slot(i).metadata).take();
        (self.active[i].take().and_then(|a| a.lease), metadata)
    }
    /// Reset a released handle's state; a still-open descriptor closes here.
    pub fn release(&mut self, h: Handle) {
        debug_assert!(self.heads[h.index()].is_none());
        let object = std::mem::take(&mut *lock(self.object(h.index())));
        drop(object);
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let mut withdrawn = 0;
        for (i, active) in self.active.iter().enumerate() {
            let Some(active) = active else { continue };
            match active.stage {
                Stage::Running => {
                    let slot = self.slots[i]
                        .as_ref()
                        .expect("a running request built its slot");
                    slot.cancel.store(true, Ordering::Release);
                    if lock(&slot.job).take().is_some() {
                        withdrawn += 1;
                    }
                }
                Stage::Queued | Stage::Waiting => crate::blocking::unreserve(),
                Stage::Withdrawn => {}
            }
        }
        let mut running = lock(&self.shared.running);
        *running -= withdrawn;
        while *running != 0 {
            running = self
                .shared
                .idle
                .wait(running)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

pub(super) fn output(buffer: &mut ReadBuf) -> &mut [u8] {
    let ReadBuf::Provided(buffer) = buffer else {
        unreachable!("pooled buffers are provided before a request starts")
    };
    // SAFETY: an accepted request exclusively owns its output region until publication.
    unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len()) }
}
fn execute(job: Job) -> Result<FsOutput> {
    let Job {
        request, object, ..
    } = job;
    let mut object = object.as_ref().map(|o| lock(o));
    match request {
        FsRequest::Open { path, options } => {
            let object = object.as_mut().expect("open handle state");
            **object = Object::File(sys::open(&path, &options)?);
            Ok(FsOutput::Opened)
        }
        FsRequest::OpenDir { path } => {
            let object = object.as_mut().expect("open handle state");
            **object = Object::Dir(sys::open_dir(&path)?);
            Ok(FsOutput::Opened)
        }
        FsRequest::Close { .. } => {
            let object = object.as_mut().expect("close handle state");
            match std::mem::replace(&mut **object, Object::Closed) {
                Object::File(file) => sys::close(file).map(|()| FsOutput::Done),
                Object::Dir(dir) => {
                    drop(dir);
                    Ok(FsOutput::Done)
                }
                previous => {
                    **object = previous;
                    Err(sys::bad_descriptor())
                }
            }
        }
        request => sys::execute(request, object.as_deref_mut()),
    }
}
