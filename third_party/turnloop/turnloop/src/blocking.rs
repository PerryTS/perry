//! Lazily started process-wide pool, with a completion port per loop.
//!
//! The pool serves two job classes that share a process and nothing else
//! (DESIGN D8). [`Occupancy::Bounded`] work is short and CPU-bound: it is
//! counted against the fixed worker set and its shared queue, and a full queue
//! is backpressure. [`Occupancy::Long`] work holds its thread for as long as a
//! connection, stream or nested runtime lives: it is served by a separately
//! accounted worker set that grows on demand to a ceiling.
//!
//! Neither class can exhaust the other, because they share no thread, no queue
//! and no reservation. A long job never occupies a bounded worker, so a host
//! may hold more threads than the bounded set has without refusing a single
//! short job; a bounded backlog never delays a long job, so a connection is
//! accepted while a hash queue is full.
use crate::{BlockingResult, Error, ErrorKind, Notifier, OpId, Payload, Result, queue::Queue};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::AtomicUsize;
use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Configuration fixed by the first process-wide blocking-pool submission.
pub struct PoolConfig {
    /// Number of lazily started process-wide blocking workers serving
    /// [`Occupancy::Bounded`] jobs. Started as one set on first use, whichever
    /// class asked for it.
    pub threads: usize,
    /// Maximum queued bounded jobs before ResourceLimit is returned. Long jobs
    /// never enter this queue and never consume its reservations.
    pub queue_capacity: usize,
    /// Ceiling on [`Occupancy::Long`] workers, which are started on demand and
    /// are not preallocated. A long submission that would need worker number
    /// `long_threads_max + 1` is refused with `ResourceLimit`; it is never
    /// queued behind another long job, so an accepted long job always has a
    /// thread of its own from the moment it is accepted.
    ///
    /// Matching tokio's `max_blocking_threads`, the default is 512. A host that
    /// hosts one connection per thread should raise it to its own connection
    /// ceiling and size [`Config::max_operations`](crate::Config::max_operations)
    /// to match, or lower it to place a hard bound on the process.
    pub long_threads_max: usize,
    /// How long an idle long worker waits for more long work before exiting.
    ///
    /// The long set shrinks back to zero threads after a burst, so a host pays
    /// for peak long occupancy only while it lasts. Reuse within the window
    /// costs no thread creation.
    pub long_idle_timeout: Duration,
    /// Ceiling on one loop's pool-delivered operations whose result has not
    /// been delivered yet (issue #88). Unlike the fields above, this one is
    /// **per loop**: it is not part of the process-wide configuration, and loops
    /// that differ only here share one pool.
    ///
    /// Every operation whose result a pool worker or helper thread hands back
    /// counts against it from acceptance until its completion is taken off the
    /// loop's result ring: [`Driver::blocking`](crate::Driver::blocking) and
    /// [`Driver::blocking_with`](crate::Driver::blocking_with) jobs of both
    /// occupancy classes, lookups served by the pool, typed filesystem requests
    /// on native targets and [`Driver::external_wait`](crate::Driver::external_wait)
    /// registrations. A submission that would exceed it is refused with
    /// [`ErrorKind::ResourceLimit`] before the work exists, the way a full
    /// [`queue_capacity`](Self::queue_capacity) refuses one.
    ///
    /// The loop's result ring is sized by this ceiling (capped at
    /// [`Config::max_operations`](crate::Config::max_operations), which bounds
    /// every operation anyway), not by `max_operations` itself. The ring must
    /// be able to hold every undelivered result at once — a worker that finds
    /// it full has nowhere to put a result — so this is the number that makes
    /// the ring safe, and a host that raises `max_operations` for I/O no longer
    /// pays for a result ring its pool work will never fill.
    ///
    /// `queue_capacity` does not bound the same thing: a bounded job leaves the
    /// queue when a worker takes it, long jobs never enter it, and a finished
    /// job's result can wait in the ring while the queue refills.
    pub max_undelivered: usize,
}
impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            threads: 4,
            queue_capacity: 1024,
            long_threads_max: 512,
            long_idle_timeout: Duration::from_secs(10),
            max_undelivered: 4096,
        }
    }
}
impl PoolConfig {
    /// The fields fixed process-wide by the first submission. `max_undelivered`
    /// belongs to each loop and is left out, so it never makes two loops'
    /// configurations disagree.
    #[cfg(not(target_arch = "wasm32"))]
    fn process_wide(self) -> Self {
        Self {
            max_undelivered: 0,
            ..self
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
/// How long a submitted job will hold the worker thread that runs it.
///
/// The class is a property of the job, not of the pool: one process-wide
/// configuration serves both, and a host does not have to prove a global thread
/// budget before it can host work of either shape.
pub enum Occupancy {
    /// Short CPU-bound work: hashing, compression, key derivation, a file
    /// request, a name lookup.
    ///
    /// Runs on the fixed worker set ([`PoolConfig::threads`]), queued behind
    /// other bounded work when every worker is busy. A full queue
    /// ([`PoolConfig::queue_capacity`]) refuses the submission with
    /// [`ErrorKind::ResourceLimit`] rather than growing.
    ///
    /// This is the default and the only class before this API existed;
    /// [`Driver::blocking`](crate::Driver::blocking) submits it.
    #[default]
    Bounded,
    /// Work that holds its thread for as long as a connection, stream or nested
    /// runtime lives, rather than for as long as a computation takes: an accept
    /// loop, a per-connection protocol runtime, a synchronous client library
    /// driven from its own thread.
    ///
    /// Served by a separate worker set that starts threads on demand up to
    /// [`PoolConfig::long_threads_max`] and retires them again after
    /// [`PoolConfig::long_idle_timeout`]. **The class has no queue:** a long
    /// submission is either given a thread immediately or refused with
    /// [`ErrorKind::ResourceLimit`], so an accepted long job never waits behind
    /// another long job and can never deadlock on one.
    ///
    /// A long job should observe [`Cancellation::requested`] and return when it
    /// is set; nothing can take a thread back from a job that does not.
    Long,
}
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
/// A snapshot of the process-wide pool, as it looked at some point during the
/// call (issue #44).
///
/// Deliberately **not** a consistent view: each field is read separately and
/// without taking the pool's lock, so the fields may belong to slightly
/// different instants and any of them may be stale the moment it is returned.
/// That is the useful trade for the question this answers — "is the pool near
/// saturation, should I shed load or run this inline?" — and a consistent
/// snapshot would cost a lock round-trip on the submitting thread. Do not build
/// an invariant on it; use the result of [`Driver::blocking`](crate::Driver::blocking)
/// for that, which is exact.
///
/// Every field is zero before the first submission starts the pool.
pub struct PoolStats {
    /// Bounded workers, once started: [`PoolConfig::threads`].
    pub threads: usize,
    /// Bounded workers currently running a job.
    pub busy: usize,
    /// Bounded jobs waiting for a worker.
    pub queued: usize,
    /// Queue slots promised to accepted operations that have not queued their
    /// work yet (typed filesystem requests). `queued + reserved` is what is
    /// measured against [`PoolConfig::queue_capacity`].
    pub reserved: usize,
    /// Live long workers, including idle ones that have not retired yet.
    pub long_threads: usize,
    /// Long workers currently running a job. Equal to the number of accepted
    /// long jobs that have not finished, because the class never queues.
    pub long_busy: usize,
}
/// Read the process-wide pool's [`PoolStats`]. Never blocks and never fails;
/// reports zeros before the first submission has started the pool.
pub fn pool_stats() -> PoolStats {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::stats()
    }
    #[cfg(target_arch = "wasm32")]
    {
        PoolStats::default()
    }
}
/// The cooperative stop signal of a running pool job.
///
/// Handed to every closure submitted through
/// [`Driver::blocking_with`](crate::Driver::blocking_with). Cancellation of a
/// job that has already started is cooperative on every platform: nothing
/// interrupts a running worker, so a job that never looks at this never stops.
pub struct Cancellation {
    cancel: Arc<AtomicBool>,
    port: Arc<WorkPort>,
}
impl Cancellation {
    pub(crate) fn new(cancel: Arc<AtomicBool>, port: Arc<WorkPort>) -> Self {
        Self { cancel, port }
    }
    /// Whether the host has asked this job to stop.
    ///
    /// True once [`Driver::cancel`](crate::Driver::cancel) was called for this
    /// job, or the loop that owns it was dropped. Returning promptly afterwards
    /// is what makes a long job's thread reusable and a host's shutdown finite.
    ///
    /// Delivery is unchanged by observing it: a job that returns after a cancel
    /// still delivers exactly one `Cancelled` completion, and one whose loop is
    /// gone delivers nothing to anybody, because there is no longer a host to
    /// deliver to.
    pub fn requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire) || self.port.is_closed()
    }
}
#[derive(Debug)]
/// Owned hostname and port for a pool-backed native address lookup.
pub struct DnsRequest {
    /// Hostname resolved by the native resolver.
    pub host: String,
    /// Port included in each resolved socket address.
    pub port: u16,
}
pub(crate) enum WorkOutput {
    ExternalWait(crate::WaitResult),
    #[cfg(not(target_arch = "wasm32"))]
    Fs(crate::fs::Reply),
    Blocking(Payload),
    Resolved(Vec<SocketAddr>),
}
pub(crate) struct WorkResult {
    pub op: OpId,
    pub result: Result<WorkOutput>,
}
pub(crate) struct WorkPort {
    queue: Queue<WorkResult>,
    notifier: Notifier,
    closed: AtomicBool,
}
impl WorkPort {
    pub fn new(capacity: usize, notifier: Notifier) -> Arc<Self> {
        Arc::new(Self {
            queue: Queue::new(capacity.max(2).next_power_of_two()),
            notifier,
            closed: AtomicBool::new(false),
        })
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    /// Slots in the result ring, for tests of its sizing.
    #[cfg(all(test, not(loom), not(target_arch = "wasm32")))]
    pub(crate) fn capacity(&self) -> usize {
        self.queue.capacity()
    }
    pub fn pop(&self) -> Option<WorkResult> {
        self.queue.pop()
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
    /// Whether the owning loop is gone, so a result pushed here is discarded.
    /// A running job reads this through [`Cancellation::requested`].
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    // The owning agent is already collecting this result. Do not leave a stale
    // notification that would turn its next future deadline into a Now poll.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn complete_during_turn(&self, result: WorkResult) {
        if !self.closed.load(Ordering::Acquire) {
            assert!(
                self.queue.push(result).is_ok(),
                "local completion credit invariant"
            );
        }
    }
    pub(crate) fn complete(&self, result: WorkResult) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        // Every producer here is an operation the driver admitted against
        // `PoolConfig::max_undelivered`, and it keeps that credit until its
        // result is popped from this ring, which is at least that large.
        // Thus the ring cannot overflow; a full ring is a broken invariant.
        assert!(
            self.queue.push(result).is_ok(),
            "blocking completion credit invariant"
        );
        let _ = self.notifier.notify();
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait ReusableWork: Send + Sync {
    fn run(&self);
}
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{
        collections::VecDeque,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Condvar, Mutex, OnceLock},
        thread,
    };
    pub(super) struct Job {
        op: OpId,
        cancel: Arc<AtomicBool>,
        port: Arc<WorkPort>,
        f: Box<dyn FnOnce() -> Result<WorkOutput> + Send>,
    }
    enum Task {
        Boxed(Job),
        Reusable(Arc<dyn ReusableWork>),
    }
    /// Counts a worker as busy for as long as it holds a task, restoring the
    /// count on an unwind that escapes the task itself.
    struct Busy<'a>(&'a AtomicUsize);
    impl Busy<'_> {
        fn start(counter: &AtomicUsize) -> Busy<'_> {
            counter.fetch_add(1, Ordering::Relaxed);
            Busy(counter)
        }
    }
    impl Drop for Busy<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }
    /// Run one job to its single completion, whichever class it belongs to.
    ///
    /// A cancel that arrived before the job started replaces the work; a panic
    /// inside it is caught and reported. Either way the port receives exactly
    /// one result for this operation.
    fn run(job: Job, busy: &AtomicUsize) {
        let _busy = Busy::start(busy);
        let result = if job.cancel.load(Ordering::Acquire) {
            Err(Error::new(ErrorKind::Cancelled))
        } else {
            catch_unwind(AssertUnwindSafe(job.f))
                .unwrap_or_else(|_| Err(Error::new(ErrorKind::Other)))
        };
        job.port.complete(WorkResult { op: job.op, result });
    }
    struct State {
        jobs: Mutex<VecDeque<Task>>,
        // Queue capacity promised to accepted operations that will push later.
        // Modified only while holding `jobs`: queued + reserved <= queue_capacity.
        reserved: AtomicUsize,
        // Lock-free mirrors of the queue length and of the workers running a
        // task, for PoolStats. Written under `jobs` (queued) or by the worker
        // that owns the task (busy); read without the lock and allowed to be stale.
        queued: AtomicUsize,
        busy: AtomicUsize,
        ready: Condvar,
        stopping: AtomicBool,
        /// Workers that finished thread startup; `start` returns only once all have.
        started: Mutex<usize>,
        started_changed: Condvar,
    }
    struct Pool {
        state: Arc<State>,
        long: Arc<LongSet>,
        config: PoolConfig,
    }
    static POOL: OnceLock<Result<Pool>> = OnceLock::new();
    fn start(config: PoolConfig) -> Result<Pool> {
        let state = Arc::new(State {
            jobs: Mutex::new(VecDeque::with_capacity(config.queue_capacity)),
            reserved: AtomicUsize::new(0),
            queued: AtomicUsize::new(0),
            busy: AtomicUsize::new(0),
            ready: Condvar::new(),
            stopping: AtomicBool::new(false),
            started: Mutex::new(0),
            started_changed: Condvar::new(),
        });
        let mut handles = Vec::with_capacity(config.threads);
        for i in 0..config.threads {
            let s = state.clone();
            let worker = thread::Builder::new()
                .name(format!("turnloop-blocking-{i}"))
                .spawn(move || {
                    // The runtime's per-thread setup (thread name, current-thread
                    // handle, TLS destructor lists; allocating on Windows) has run.
                    *s.started
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
                    s.started_changed.notify_all();
                    loop {
                        let job = {
                            let mut jobs = s
                                .jobs
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            while jobs.is_empty() && !s.stopping.load(Ordering::Acquire) {
                                jobs = s
                                    .ready
                                    .wait(jobs)
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                            }
                            if s.stopping.load(Ordering::Acquire) {
                                return;
                            }
                            let task = jobs.pop_front().expect("nonempty job queue");
                            s.queued.store(jobs.len(), Ordering::Relaxed);
                            task
                        };
                        match job {
                            Task::Boxed(job) => run(job, &s.busy),
                            Task::Reusable(work) => {
                                let _busy = Busy::start(&s.busy);
                                work.run();
                            }
                        }
                    }
                });
            match worker {
                Ok(handle) => handles.push(handle),
                Err(e) => {
                    state.stopping.store(true, Ordering::Release);
                    state.ready.notify_all();
                    for h in handles {
                        let _ = h.join();
                    }
                    return Err(e.into());
                }
            }
        }
        // Return only after every worker finished starting. Thread startup runs
        // asynchronously and allocates on some platforms; it must not overlap the
        // first steady-state jobs of any loop.
        let mut started = state
            .started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *started < config.threads {
            started = state
                .started_changed
                .wait(started)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(started);
        // Workers live for the process lifetime, shared by every submitting loop.
        Ok(Pool {
            state,
            long: LongSet::new(config.long_threads_max, config.long_idle_timeout),
            config,
        })
    }
    pub(super) fn submit(
        config: PoolConfig,
        occupancy: Occupancy,
        op: OpId,
        cancel: Arc<AtomicBool>,
        port: Arc<WorkPort>,
        f: Box<dyn FnOnce() -> Result<WorkOutput> + Send>,
    ) -> Result<()> {
        let pool = pool(config)?;
        let job = Job {
            op,
            cancel,
            port,
            f,
        };
        match occupancy {
            // A long job takes nothing from the bounded class: not a worker, not
            // a queue slot, not a reservation. Saturating one therefore cannot
            // refuse or delay the other, in either direction.
            Occupancy::Long => pool.long.submit(job),
            Occupancy::Bounded => {
                let mut jobs = lock(pool);
                if jobs.len() + pool.state.reserved.load(Ordering::Relaxed)
                    >= pool.config.queue_capacity
                {
                    return Err(Error::new(ErrorKind::ResourceLimit));
                }
                jobs.push_back(Task::Boxed(job));
                pool.state.queued.store(jobs.len(), Ordering::Relaxed);
                drop(jobs);
                pool.state.ready.notify_one();
                Ok(())
            }
        }
    }
    pub(super) fn stats() -> PoolStats {
        let Some(Ok(pool)) = POOL.get().map(|p| p.as_ref()) else {
            return PoolStats::default();
        };
        let (long_threads, long_busy) = pool.long.stats();
        PoolStats {
            threads: pool.config.threads,
            busy: pool.state.busy.load(Ordering::Relaxed),
            queued: pool.state.queued.load(Ordering::Relaxed),
            reserved: pool.state.reserved.load(Ordering::Relaxed),
            long_threads,
            long_busy,
        }
    }
    fn pool(config: PoolConfig) -> Result<&'static Pool> {
        if config.threads == 0 || config.queue_capacity == 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let pool = POOL
            .get_or_init(|| start(config))
            .as_ref()
            .map_err(|&e| e)?;
        if pool.config.process_wide() != config.process_wide() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        Ok(pool)
    }
    fn lock(pool: &Pool) -> std::sync::MutexGuard<'_, VecDeque<Task>> {
        pool.state
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    #[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
    pub(crate) fn reusable(config: PoolConfig, work: Arc<dyn ReusableWork>) -> Result<()> {
        let pool = pool(config)?;
        let mut jobs = lock(pool);
        if jobs.len() + pool.state.reserved.load(Ordering::Relaxed) >= pool.config.queue_capacity {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        jobs.push_back(Task::Reusable(work));
        pool.state.queued.store(jobs.len(), Ordering::Relaxed);
        pool.state.ready.notify_one();
        Ok(())
    }
    /// Promise one queue slot to an accepted operation (ResourceLimit when full).
    pub(crate) fn reserve(config: PoolConfig) -> Result<()> {
        let pool = pool(config)?;
        let jobs = lock(pool);
        let reserved = pool.state.reserved.load(Ordering::Relaxed);
        if jobs.len() + reserved >= pool.config.queue_capacity {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        pool.state.reserved.store(reserved + 1, Ordering::Relaxed);
        Ok(())
    }
    /// Return an unused reservation (the operation ended before it was queued).
    pub(crate) fn unreserve() {
        let pool = POOL
            .get()
            .and_then(|p| p.as_ref().ok())
            .expect("reserved pool");
        let _jobs = lock(pool);
        let reserved = pool.state.reserved.load(Ordering::Relaxed);
        pool.state.reserved.store(reserved - 1, Ordering::Relaxed);
    }
    /// Queue reusable work into its reservation. Never fails and never grows the
    /// preallocated queue, because the reservation already counted this slot.
    pub(crate) fn push_reserved(work: Arc<dyn ReusableWork>) {
        let pool = POOL
            .get()
            .and_then(|p| p.as_ref().ok())
            .expect("reserved pool");
        let mut jobs = lock(pool);
        let reserved = pool.state.reserved.load(Ordering::Relaxed);
        pool.state.reserved.store(reserved - 1, Ordering::Relaxed);
        debug_assert!(jobs.len() < pool.config.queue_capacity);
        jobs.push_back(Task::Reusable(work));
        pool.state.queued.store(jobs.len(), Ordering::Relaxed);
        drop(jobs);
        pool.state.ready.notify_one();
    }
    /// The [`Occupancy::Long`] worker set: no queue, threads on demand.
    ///
    /// A long job holds its thread for as long as a connection lives, so the
    /// two rules here are that a long job must never wait for another long job
    /// (that is a deadlock whenever the first waits on the second's progress),
    /// and that the set must still be bounded. Both follow from the same
    /// decision: **the class has no backlog.** A submission is handed to a
    /// parked worker, or starts one of its own, or is refused; there is no
    /// third outcome and nothing to queue behind.
    pub(super) struct LongSet {
        handoff: Mutex<Long>,
        ready: Condvar,
        /// Lock-free mirror of `Long::threads`, written under `handoff`, and the
        /// count of workers running a job. Both are read without the lock for
        /// PoolStats and are allowed to be stale.
        live: AtomicUsize,
        busy: AtomicUsize,
        max: usize,
        idle_timeout: Duration,
    }
    struct Long {
        /// Jobs given to a specific parked worker, never a backlog: a job is
        /// pushed only while `jobs.len() < idle` holds, so every pushed job has
        /// a parked worker that no other pushed job has been promised to.
        jobs: VecDeque<Job>,
        /// Live workers, counted from the moment one is decided on rather than
        /// from the moment its thread starts, so two submissions racing at the
        /// ceiling cannot both pass it.
        threads: usize,
        /// Parked workers not yet promised a job.
        idle: usize,
        /// Monotone worker ordinal, for thread names.
        started: usize,
    }
    impl LongSet {
        pub(super) fn new(max: usize, idle_timeout: Duration) -> Arc<Self> {
            Arc::new(Self {
                handoff: Mutex::new(Long {
                    jobs: VecDeque::new(),
                    threads: 0,
                    idle: 0,
                    started: 0,
                }),
                ready: Condvar::new(),
                live: AtomicUsize::new(0),
                busy: AtomicUsize::new(0),
                max,
                idle_timeout,
            })
        }
        fn lock(&self) -> std::sync::MutexGuard<'_, Long> {
            self.handoff
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
        /// Start `job` now on a long worker, or refuse it.
        ///
        /// Refusal is `ResourceLimit` at the ceiling, and the spawn's own error
        /// if the OS cannot start a thread. Both refuse the submission before
        /// it becomes a job, so the caller owes no completion either way.
        pub(super) fn submit(self: &Arc<Self>, job: Job) -> Result<()> {
            let mut long = self.lock();
            if long.jobs.len() < long.idle {
                long.jobs.push_back(job);
                drop(long);
                self.ready.notify_one();
                return Ok(());
            }
            if long.threads >= self.max {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            long.threads += 1;
            self.live.store(long.threads, Ordering::Relaxed);
            let ordinal = long.started;
            long.started += 1;
            drop(long);
            let set = self.clone();
            // The job is the new worker's first task rather than a queue entry:
            // a failed spawn then drops it here, with no window in which another
            // worker could have taken work this submission is about to disown.
            match thread::Builder::new()
                .name(format!("turnloop-long-{ordinal}"))
                .spawn(move || set.worker(job))
            {
                Ok(_detached) => Ok(()),
                Err(e) => {
                    let mut long = self.lock();
                    long.threads -= 1;
                    self.live.store(long.threads, Ordering::Relaxed);
                    Err(e.into())
                }
            }
        }
        /// Run `first`, then serve handed-off jobs until the idle timeout.
        fn worker(self: Arc<Self>, first: Job) {
            // Retiring under the same lock that admits work is what keeps the
            // ceiling honest through a panic that escapes a job's own guard.
            struct Retire<'a>(&'a LongSet);
            impl Drop for Retire<'_> {
                fn drop(&mut self) {
                    let mut long = self.0.lock();
                    long.threads -= 1;
                    self.0.live.store(long.threads, Ordering::Relaxed);
                }
            }
            let _retire = Retire(&self);
            let mut next = Some(first);
            while let Some(job) = next.take() {
                run(job, &self.busy);
                let mut long = self.lock();
                long.idle += 1;
                loop {
                    if let Some(job) = long.jobs.pop_front() {
                        long.idle -= 1;
                        next = Some(job);
                        break;
                    }
                    let (guard, wait) = self
                        .ready
                        .wait_timeout(long, self.idle_timeout)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    long = guard;
                    // A job that arrived during the wait is taken even if the
                    // wait also timed out: it was promised to this worker.
                    if wait.timed_out() && long.jobs.is_empty() {
                        long.idle -= 1;
                        // Explicitly, because `Retire` takes this same lock:
                        // scope-exit order already releases it first, and this
                        // keeps that true through any later edit.
                        drop(long);
                        return;
                    }
                }
            }
        }
        /// Live and busy long workers. Read without the lock, so the two may be
        /// a moment apart; `live` never lags a submission that returned `Ok`.
        pub(super) fn stats(&self) -> (usize, usize) {
            (
                self.live.load(Ordering::Relaxed),
                self.busy.load(Ordering::Relaxed),
            )
        }
        /// Parked workers, read under the lock. Tests use it to reach a state
        /// the stale counters cannot describe: a worker that is not merely done
        /// with its job but actually available to be handed the next one.
        #[cfg(all(test, not(loom)))]
        fn idle(&self) -> usize {
            self.lock().idle
        }
    }
    #[cfg(all(test, not(loom)))]
    mod long_tests {
        use super::*;
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };
        struct NoWake;
        impl crate::backend::Wake for NoWake {
            fn wake(&self) -> Result<()> {
                Ok(())
            }
            fn syscall_count(&self) -> u64 {
                0
            }
        }
        fn port() -> Arc<WorkPort> {
            WorkPort::new(64, Notifier::new(std::sync::Arc::new(NoWake)))
        }
        fn job(
            port: &Arc<WorkPort>,
            key: u64,
            cancel: Arc<AtomicBool>,
            f: impl FnOnce() -> Result<WorkOutput> + Send + 'static,
        ) -> Job {
            Job {
                op: OpId { owner: 1, key },
                cancel,
                port: port.clone(),
                f: Box::new(f),
            }
        }
        fn live() -> Arc<AtomicBool> {
            Arc::new(AtomicBool::new(false))
        }
        #[track_caller]
        fn until(what: &str, mut ready: impl FnMut() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ready() {
                assert!(Instant::now() < deadline, "{what}");
                thread::yield_now();
            }
        }
        #[track_caller]
        fn failure(result: Result<WorkOutput>) -> ErrorKind {
            match result {
                Ok(_) => panic!("expected a failed job"),
                Err(e) => e.kind,
            }
        }
        /// Poll for the next published result. `is_empty` is not a substitute:
        /// a producer that has reserved its slot is already non-empty, and the
        /// value is only readable once publication finishes.
        #[track_caller]
        fn take(port: &Arc<WorkPort>) -> WorkResult {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(result) = port.pop() {
                    return result;
                }
                assert!(Instant::now() < deadline, "a result arrived");
                thread::yield_now();
            }
        }
        /// One worker serves job after job, and the set shrinks back to nothing
        /// when the work stops: the ceiling bounds a peak, not a floor.
        #[test]
        fn long_workers_are_reused_and_retire_when_idle() {
            let set = LongSet::new(4, Duration::from_millis(50));
            let port = port();
            set.submit(job(&port, 1, live(), || {
                Ok(WorkOutput::Blocking(Payload::U64(1)))
            }))
            .expect("first long job");
            assert!(matches!(
                take(&port).result,
                Ok(WorkOutput::Blocking(Payload::U64(1)))
            ));
            assert_eq!(set.stats().0, 1, "one worker started for one long job");
            until("the worker parked", || set.idle() == 1);
            set.submit(job(&port, 2, live(), || {
                Ok(WorkOutput::Blocking(Payload::U64(2)))
            }))
            .expect("second long job");
            assert!(matches!(
                take(&port).result,
                Ok(WorkOutput::Blocking(Payload::U64(2)))
            ));
            assert_eq!(
                set.stats().0,
                1,
                "the parked worker served it instead of a new thread"
            );
            until("the idle worker retired", || set.stats() == (0, 0));
        }
        /// At the ceiling a long submission is refused, not queued: a long job
        /// that waits for another long job's progress would never finish.
        #[test]
        fn long_submissions_at_the_ceiling_are_refused_rather_than_queued() {
            let set = LongSet::new(2, Duration::from_secs(30));
            let port = port();
            let entered = Arc::new(AtomicUsize::new(0));
            let (release, held) = mpsc::channel::<()>();
            let held = Arc::new(Mutex::new(held));
            for key in 0..2 {
                let (entered, held) = (entered.clone(), held.clone());
                set.submit(job(&port, key, live(), move || {
                    entered.fetch_add(1, Ordering::Release);
                    let _ = held.lock().expect("held").recv();
                    Ok(WorkOutput::Blocking(Payload::U64(key)))
                }))
                .expect("within the ceiling");
            }
            until("both long jobs are running", || {
                entered.load(Ordering::Acquire) == 2
            });
            assert_eq!(set.stats(), (2, 2), "both workers are occupied");
            let refused = set
                .submit(job(&port, 9, live(), || {
                    unreachable!("a refused submission never runs")
                }))
                .expect_err("above the ceiling");
            assert_eq!(refused.kind, ErrorKind::ResourceLimit);
            assert!(
                port.is_empty(),
                "a refused submission owes no completion at all"
            );
            drop(release);
            assert!(matches!(take(&port).result, Ok(WorkOutput::Blocking(_))));
            assert!(matches!(take(&port).result, Ok(WorkOutput::Blocking(_))));
            // The ceiling bounds what runs at once, not what may ever run.
            until("a worker is free again", || set.idle() > 0);
            set.submit(job(&port, 10, live(), || {
                Ok(WorkOutput::Blocking(Payload::U64(10)))
            }))
            .expect("the released capacity is usable");
            assert!(matches!(
                take(&port).result,
                Ok(WorkOutput::Blocking(Payload::U64(10)))
            ));
        }
        /// Exactly one result per accepted long job, on both paths that do not
        /// return one from the closure.
        #[test]
        fn a_cancelled_or_panicking_long_job_delivers_exactly_one_result() {
            let set = LongSet::new(2, Duration::from_millis(50));
            let port = port();
            let ran = Arc::new(AtomicUsize::new(0));
            let counter = ran.clone();
            let cancelled = Arc::new(AtomicBool::new(true));
            set.submit(job(&port, 1, cancelled, move || {
                counter.fetch_add(1, Ordering::Release);
                Ok(WorkOutput::Blocking(Payload::U64(1)))
            }))
            .expect("accepted");
            let result = take(&port);
            assert_eq!(result.op.key, 1);
            assert_eq!(failure(result.result), ErrorKind::Cancelled);
            assert_eq!(ran.load(Ordering::Acquire), 0, "the work never started");
            set.submit(job(&port, 2, live(), || panic!("a job may panic")))
                .expect("accepted");
            let result = take(&port);
            assert_eq!(result.op.key, 2);
            assert_eq!(failure(result.result), ErrorKind::Other);
            // The worker survived the panic and still serves the class.
            set.submit(job(&port, 3, live(), || {
                Ok(WorkOutput::Blocking(Payload::U64(3)))
            }))
            .expect("accepted");
            let result = take(&port);
            assert_eq!(result.op.key, 3);
            assert!(matches!(
                result.result,
                Ok(WorkOutput::Blocking(Payload::U64(3)))
            ));
            assert!(port.is_empty(), "three jobs delivered three results");
        }
    }
}
pub(crate) fn submit(
    config: PoolConfig,
    occupancy: Occupancy,
    op: OpId,
    cancel: Arc<AtomicBool>,
    port: Arc<WorkPort>,
    f: Box<dyn FnOnce() -> Result<WorkOutput> + Send>,
) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::submit(config, occupancy, op, cancel, port, f)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (config, occupancy, op, cancel, port, f);
        Err(Error::new(ErrorKind::Unsupported))
    }
}
pub(crate) fn blocking(
    f: impl FnOnce() -> BlockingResult + Send + 'static,
) -> Box<dyn FnOnce() -> Result<WorkOutput> + Send> {
    Box::new(move || f().map(WorkOutput::Blocking))
}
/// A job that can observe its own cancellation. The signal is captured once,
/// when the job is built, so running it costs exactly what `blocking` costs.
pub(crate) fn blocking_cancellable(
    f: impl FnOnce(&Cancellation) -> BlockingResult + Send + 'static,
    cancel: Arc<AtomicBool>,
    port: Arc<WorkPort>,
) -> Box<dyn FnOnce() -> Result<WorkOutput> + Send> {
    let stop = Cancellation::new(cancel, port);
    Box::new(move || f(&stop).map(WorkOutput::Blocking))
}
pub(crate) fn resolve(request: DnsRequest) -> Box<dyn FnOnce() -> Result<WorkOutput> + Send> {
    Box::new(move || {
        // std's native ToSocketAddrs implementation uses getaddrinfo (Unix) or
        // GetAddrInfoW (Windows). Only owned Rust data runs on the pool thread.
        let addresses: Vec<_> = (request.host.as_str(), request.port)
            .to_socket_addrs()
            .map_err(Error::from)?
            .collect();
        if addresses.is_empty() {
            return Err(Error::new(ErrorKind::NotFound));
        }
        Ok(WorkOutput::Resolved(addresses))
    })
}

#[cfg(all(test, loom))]
mod models {
    use super::*;
    use crate::{
        backend::Wake,
        sync::{AtomicUsize, Ordering as ModelOrdering},
    };
    struct WakeCounter(AtomicUsize);
    impl Wake for WakeCounter {
        fn wake(&self) -> Result<()> {
            self.0.fetch_add(1, ModelOrdering::Relaxed);
            Ok(())
        }
        fn syscall_count(&self) -> u64 {
            self.0.load(ModelOrdering::Relaxed) as u64
        }
    }
    #[test]
    fn pool_result_publication_races_loop_parking() {
        loom::model(|| {
            let wake = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let notifier = Notifier::new(wake.clone());
            let port = WorkPort::new(2, notifier.clone());
            let worker = port.clone();
            let op = OpId {
                owner: 7,
                key: 1 << 32,
            };
            let t = loom::thread::spawn(move || {
                worker.complete(WorkResult {
                    op,
                    result: Ok(WorkOutput::Blocking(Payload::U64(42))),
                })
            });
            let parked = notifier.park();
            t.join().expect("pool worker");
            let completion = port.pop().expect("published completion");
            assert_eq!(completion.op, op);
            assert!(matches!(
                completion.result,
                Ok(WorkOutput::Blocking(Payload::U64(42)))
            ));
            assert!(port.pop().is_none());
            if parked {
                assert_eq!(wake.syscall_count(), 1);
            } else {
                assert!(notifier.begin());
            }
        });
    }
}

#[cfg(any(turnloop_backend = "kqueue", turnloop_backend = "epoll"))]
pub(crate) use native::reusable;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{push_reserved, reserve, unreserve};
