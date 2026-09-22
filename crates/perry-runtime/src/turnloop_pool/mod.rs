//! turnloop P4: Perry's blocking and CPU-bound work on turnloop's shared
//! bounded pool (DESIGN §12 "P4", D8, §9).
//!
//! P1 moved sockets onto the loop, P2 the child pipes, datagrams and signals,
//! P3 the timer heap. What is left of "work that must not run on the JS
//! thread" is the class libuv calls threadpool work: password hashing, image
//! processing, compression, key derivation, name resolution and an addon's
//! `napi_queue_async_work`. Perry ran each of those on a *different* mechanism
//! — tokio's blocking pool for bcrypt and every `perry_ffi::spawn_blocking`
//! caller, one fresh `std::thread` per N-API async work item, and, for argon2,
//! the KDFs, the zlib one-shots and `dns.lookup`, no offloading at all: the
//! work ran inline on the thread that owns the JS heap and only the *callback*
//! was deferred.
//!
//! This module is the one mechanism that replaces all of them:
//! [`Loop::blocking`](turnloop::Loop::blocking) submits an owned `Send`
//! closure to the process-wide bounded pool, and its result comes back as an
//! ordinary turnloop completion on the thread that submitted it.
//!
//! # The contract, and why it is shaped like this
//!
//! A job is two closures, and the split is the whole point:
//!
//! - `work` runs **on a pool thread**. It is `FnOnce() -> T + Send`, so the
//!   only thing that can cross is owned Rust data. It cannot touch the JS
//!   heap: perry-runtime's arena is thread-local, and a JSValue built on a
//!   pool thread lands in an arena the owning thread will never see (#1824).
//! - `deliver` runs **on the owning thread**, inside the completion dispatch
//!   that follows a turn. It receives the `T` the pool produced and is where
//!   JS values get built, promises settle and callbacks are queued. It is
//!   deliberately **not** `Send`.
//!
//! That is `spawn_for_promise_deferred`'s rule (DESIGN §9 "Thread-pool jobs
//! touching JS: never") expressed in the type system instead of in a comment.
//!
//! # Completion routing
//!
//! One token space, disjoint from P1's `1..=7`, P2's `0x10..=0x1F` and P3's
//! `TIMER_TOKEN` by construction: the top 8 bits are the operation class
//! (`0x20..=0x2F`), the low 56 the job id. [`owns`] is the range test
//! `agent_loop::dispatch_staged` routes on, so no module can be handed
//! another's completion and a stale token finds no entry and is dropped.
//!
//! # Exactly-once
//!
//! Every accepted job produces exactly one [`Delivery`] (DESIGN D4): `Done`
//! when the pool ran it, `Cancelled` when [`cancel`] won the race with the
//! pool thread or the loop shut down with the job still outstanding, `Failed`
//! when the job panicked on the pool thread (turnloop catches the unwind).
//! A submission the driver *refuses* never becomes a job at all and reports
//! through `submit`'s return value instead, so a caller never has to guess
//! whether its `deliver` will run.
//!
//! # GC
//!
//! **No JS heap memory reaches the pool**, the property P1 and P2 established:
//! `work` is `Send` and JS values are not, so the compiler rejects the mistake
//! rather than the reviewer having to catch it.
//!
//! What *is* new here is that a job has a lifetime, and a caller may need a JS
//! value to survive it — `zlib.gzip(buf, cb)` has to hold `cb` from submission
//! until the compression finishes. Such a value is parked in the job entry
//! through [`submit_rooted`] and visited by this module's registered
//! `gc_register_mutable_root_scanner`, so a moving collector rewrites it and
//! hands the rewritten value to `deliver`. A raw heap pointer held in a
//! runtime-side table is a GC root and the static checker cannot see it
//! (CLAUDE.md); registering the scanner in the same file as the holder is what
//! keeps that true.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use turnloop::{Completion, Error, ErrorKind, OpId, OpResult, Payload, Token};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

// ── Operation classes, in the top 8 bits of every submission token ──────────

/// Lowest operation class this module claims.
const CLASS_MIN: u64 = 0x20;
/// Highest operation class this module claims.
const CLASS_MAX: u64 = 0x2F;

/// A host blocking job.
const OP_JOB: u64 = 0x20;

/// The low 56 bits of a token hold the job id.
const ID_BITS: u32 = 56;
const ID_MASK: u64 = (1 << ID_BITS) - 1;

/// Bits of the 56-bit token id reserved for the minting agent.
///
/// The tables that hold these ids are thread-local, so before P9 — when only
/// the primary agent could own a loop — a plain per-thread counter was enough:
/// there was one minter. Now every JS agent can own a loop, and two agents
/// counting from 1 would both own an id `1`. That is harmless while every
/// lookup is same-thread (each finds its own entry), and a **silent misroute**
/// the moment one is not.
///
/// So the id carries its agent: agent N mints from `(N & 0xFFFF) << ID_AGENT_SHIFT`,
/// leaving each agent 2^40 ids inside the 56-bit field. A foreign id then MISSES
/// the table rather than aliasing an entry, which turns a misroute into an
/// error the caller can see. The primary agent is unchanged (band 0, ids from
/// 1), so nothing about a single-agent program moves.
const ID_AGENT_SHIFT: u32 = 40;
const ID_AGENT_MASK: u64 = 0xFFFF;

/// The first id this agent may mint, minus one.
fn agent_id_band() -> u64 {
    (crate::agent::current_agent() & ID_AGENT_MASK) << ID_AGENT_SHIFT
}

/// Take the next job id for this thread's agent, seeding the band on first use.
fn mint_id(state: &mut PoolState) -> u64 {
    if state.next_id == 0 {
        state.next_id = agent_id_band();
    }
    state.next_id += 1;
    debug_assert!(state.next_id <= ID_MASK, "agent id band overflowed a token");
    state.next_id
}

fn token(op: u64, id: u64) -> Token {
    debug_assert!(id > 0 && id <= ID_MASK, "id {id} fits a token");
    debug_assert!((CLASS_MIN..=CLASS_MAX).contains(&op), "class {op} is P4's");
    Token((op << ID_BITS) | (id & ID_MASK))
}

fn token_parts(t: Token) -> (u64, u64) {
    (t.0 >> ID_BITS, t.0 & ID_MASK)
}

/// Whether this completion belongs to P4's blocking pool.
///
/// The router in `agent_loop::dispatch_staged` asks this and nothing else, so
/// the token spaces cannot overlap by accident.
#[inline]
pub fn owns(t: Token) -> bool {
    (CLASS_MIN..=CLASS_MAX).contains(&(t.0 >> ID_BITS))
}

// ── Public types ────────────────────────────────────────────────────────────

/// Identifies one accepted job, for [`cancel`].
///
/// Not `Copy`, and not reused: the id is a monotonic per-thread counter, so a
/// cancel naming a finished job finds nothing rather than cancelling whatever
/// took its slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobId(u64);

impl JobId {
    /// The raw id, for a caller that has to store it in a C-ABI-visible field.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Rebuild a job id from [`JobId::raw`].
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// The single outcome every accepted job produces, on the owning thread.
#[derive(Debug)]
pub enum Delivery<T> {
    /// The pool ran `work` and this is what it returned.
    Done(T),
    /// The job did not run: [`cancel`] won the race with the pool thread, or
    /// the loop was torn down while the job was still outstanding.
    Cancelled,
    /// The job panicked on the pool thread, or the driver reported a failure.
    Failed(Error),
}

/// Why a submission was refused. A refused submission never becomes a job, so
/// its `deliver` never runs and the caller keeps its own fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    /// This thread has no `turnloop::Loop`: a worker agent, or a host where
    /// loop creation failed.
    NoLoop,
    /// The pool queue or the loop's operation table is full. This is
    /// backpressure, not a failure: the caller may retry or run inline.
    Busy,
    /// The driver refused for another reason; the error carries the detail.
    Refused(Error),
}

impl SubmitError {
    fn from_driver(e: Error) -> Self {
        match e.kind {
            ErrorKind::ResourceLimit => SubmitError::Busy,
            _ => SubmitError::Refused(e),
        }
    }
}

// ── Process-wide counters ───────────────────────────────────────────────────
//
// Lifetime totals, not live counts: every live count is zero by the time a
// process exits, so a claim that a workload ran on the pool needs the totals
// (DESIGN §11 — a gate must assert its subject was live). Reported on the
// `PERRY_LOOP_STATS=1` exit line.

static SUBMITTED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static CANCELLED: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);
per_test_global! {
    /// `per_test_global!` because TEST code reads it via `refused_total()`
    /// (#10944); `SUBMITTED` / `COMPLETED` / `CANCELLED` / `FAILED` above are
    /// not read from tests, which is why they stay bare. No `adopt` needed:
    /// `submit` increments this on the SUBMITTING thread and the assertion
    /// reads it on that same thread. Outside a test build the macro is the
    /// plain `static`, byte for byte, and `PerThread` derefs, so the
    /// `fetch_add` / `load` sites are unchanged.
    static REFUSED: AtomicU64 = AtomicU64::new(0);
}

per_test_global! {
    /// Jobs accepted and not yet delivered, across every thread.
    ///
    /// Process-wide on purpose: it answers the event loop's keep-alive question
    /// ("may this process exit?"), which is asked by the primary agent about the
    /// whole process, while the entry tables are per-thread. OUTSIDE a test build
    /// `per_test_global!` IS that plain `static`, byte for byte, so the keep-alive
    /// gate is unchanged in every shipped configuration.
    ///
    /// `per_test_global!` because tests assert on it through `has_pending_jobs()`
    /// (`tests.rs:180` "an accepted job keeps the loop alive", `:533` "the
    /// keep-alive gate is released") and `reset_for_test` writes it, so a bare
    /// static would let one test's residue decide another's verdict.
    ///
    /// No `adopt` needed, for the same reason as `REFUSED` above: every access
    /// is on the SUBMITTING thread. `submit` increments; the decrement rides
    /// `OutstandingGuard`, dropped inside `deliver`, which can only retire a job
    /// it found in the thread-local `POOL` table — the suite asserts exactly that
    /// ("the delivery must run on the submitting thread, where JS lives",
    /// `tests.rs:172`). `reset_for_test` likewise subtracts only this thread's
    /// leftover. Contrast `NOTIFY_AT_NS`, which a different thread writes and so
    /// needs `shared_key`/`adopt`.
    static OUTSTANDING: AtomicUsize = AtomicUsize::new(0);
}

/// Jobs submitted to the pool over this process's life.
pub fn submitted_total() -> u64 {
    SUBMITTED.load(Ordering::Relaxed)
}

/// Jobs the pool ran to completion.
pub fn completed_total() -> u64 {
    COMPLETED.load(Ordering::Relaxed)
}

/// Jobs delivered as [`Delivery::Cancelled`].
pub fn cancelled_total() -> u64 {
    CANCELLED.load(Ordering::Relaxed)
}

/// Jobs delivered as [`Delivery::Failed`].
pub fn failed_total() -> u64 {
    FAILED.load(Ordering::Relaxed)
}

/// Submissions the driver refused before they became jobs.
pub fn refused_total() -> u64 {
    REFUSED.load(Ordering::Relaxed)
}

/// Whether any accepted job is still outstanding anywhere in this process.
///
/// The event loop's keep-alive gate reads this: a program whose `main` has
/// returned while a bcrypt hash is still on a pool thread must not exit before
/// the hash's promise settles (the shape #591 fixed for the tokio pool).
#[inline]
pub fn has_pending_jobs() -> bool {
    OUTSTANDING.load(Ordering::Acquire) != 0
}

/// Jobs this thread has accepted and not yet delivered.
pub fn outstanding() -> usize {
    POOL.with(|state| state.borrow().jobs.len())
}

/// Whether this thread can take the turnloop pool path at all, asked without
/// creating a loop.
pub fn available() -> bool {
    crate::event_pump::net_loop_available()
}

// ── Per-thread job table ────────────────────────────────────────────────────

/// What the pool hands back, type-erased so one table holds every job shape.
type Erased = Box<dyn Any + Send>;

/// The owning thread's half of a job.
struct Job {
    /// The driver operation, for [`cancel`].
    op: OpId,
    /// Runs on the owning thread with the pool's result and the (rewritten)
    /// rooted values.
    deliver: Box<dyn FnOnce(Delivery<Erased>, Vec<u64>)>,
    /// NaN-boxed JS values the caller parked for the job's lifetime. Visited
    /// by [`scan_roots_mut`]; empty for every job submitted through [`submit`].
    roots: Vec<u64>,
}

#[derive(Default)]
struct PoolState {
    jobs: HashMap<u64, Job>,
    next_id: u64,
}

crate::perry_thread_local! {
    /// Per agent, like the loop itself. A job belongs to the thread that
    /// submitted it; there is no cross-thread map to race on.
    static POOL: RefCell<PoolState> = RefCell::new(PoolState::default());
    /// Outside [`PoolState`] on purpose: registering a scanner pushes onto the
    /// scanner registry, and doing that while holding the table's borrow would
    /// be one more thing that has to be proven not to re-enter.
    static SCANNER_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Visit every JS value parked with an outstanding job on this thread.
///
/// Registered lazily, from the first [`submit_rooted`] on this thread, because
/// a program that never parks a JS value has nothing for this to walk. See the
/// module's GC note: this table holds raw heap pointers and the static
/// dominance checker cannot see it, so the registration lives beside it.
fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    POOL.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            // The only borrow a collection can land inside is a `deliver`
            // closure's: it runs JS, so it can allocate and collect. That job's
            // entry is already out of the table and its roots have moved into
            // the closure's own frame, which the ordinary stack scan reaches;
            // every other entry is untouched. The table's other borrows —
            // submit, cancel, the take in `deliver` — hold it across nothing
            // but `HashMap` operations, which allocate from the system
            // allocator and never reach a collection point. Skipping is
            // therefore correct, not a missed root.
            return;
        };
        for job in state.jobs.values_mut() {
            for root in job.roots.iter_mut() {
                visitor.visit_nanbox_u64_slot(root);
            }
        }
    });
}

fn ensure_scanner_registered() {
    SCANNER_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        crate::gc::gc_register_mutable_root_scanner_named("runtime:turnloop_pool", scan_roots_mut);
        registered.set(true);
    });
}

// ── Submission ──────────────────────────────────────────────────────────────

/// Run `work` on the shared blocking pool and `deliver` its result on this
/// thread.
///
/// `work` is `Send` and runs on a pool thread; `deliver` is not `Send` and
/// runs inside the completion dispatch that follows a turn on the submitting
/// thread, which is where JS values may be built. Exactly one [`Delivery`] is
/// produced for every accepted job.
///
/// Returns [`SubmitError`] when the job was *not* accepted, in which case
/// `deliver` never runs and the caller keeps whatever fallback it had.
pub fn submit<T, W, D>(work: W, deliver: D) -> Result<JobId, SubmitError>
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Delivery<T>) + 'static,
{
    submit_rooted(Vec::new(), work, move |delivery, _| deliver(delivery))
}

/// [`submit`], plus NaN-boxed JS values held alive for the job's lifetime.
///
/// `roots` are visited by this module's registered root scanner while the job
/// is outstanding, so a moving collector rewrites them, and the rewritten
/// values — not the ones the caller passed — are handed to `deliver`. Use this
/// whenever the completion needs a JS callback, a resource object or any other
/// heap value that nothing else roots for the duration.
pub fn submit_rooted<T, W, D>(roots: Vec<u64>, work: W, deliver: D) -> Result<JobId, SubmitError>
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Delivery<T>, Vec<u64>) + 'static,
{
    if !roots.is_empty() {
        ensure_scanner_registered();
    }
    let id = POOL.with(|state| mint_id(&mut state.borrow_mut()));
    let submitted = crate::event_pump::with_pool_driver(|driver| {
        driver.blocking(
            move || Ok(Payload::Boxed(Box::new(work()) as Erased)),
            token(OP_JOB, id),
        )
    });
    let op = match submitted {
        None => {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            return Err(SubmitError::NoLoop);
        }
        Some(Err(e)) => {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            return Err(SubmitError::from_driver(e));
        }
        Some(Ok(op)) => op,
    };
    let deliver: Box<dyn FnOnce(Delivery<Erased>, Vec<u64>)> = Box::new(move |delivery, roots| {
        let delivery = match delivery {
            // The id keys both halves, so the box can only hold the `T`
            // this job's own `work` produced.
            Delivery::Done(erased) => match erased.downcast::<T>() {
                Ok(value) => Delivery::Done(*value),
                Err(_) => Delivery::Failed(Error::new(ErrorKind::Other)),
            },
            Delivery::Cancelled => Delivery::Cancelled,
            Delivery::Failed(e) => Delivery::Failed(e),
        };
        deliver(delivery, roots);
    });
    POOL.with(|state| {
        state
            .borrow_mut()
            .jobs
            .insert(id, Job { op, deliver, roots })
    });
    SUBMITTED.fetch_add(1, Ordering::Relaxed);
    OUTSTANDING.fetch_add(1, Ordering::AcqRel);
    Ok(JobId(id))
}

/// [`submit`], with the caller's own thread as the fallback when the pool
/// refuses.
///
/// Returns true when the pool took the job. On false the work has **already
/// run, inline, on the calling thread**, and `deliver` has **already been
/// called** with `Delivery::Done` — so every caller settles exactly once
/// whichever path ran, and a thread with no loop (a `worker_threads` agent)
/// keeps working instead of failing.
///
/// This is the right fallback for CPU-bound work whose caller is *already*
/// waiting on the answer: blocking that thread is what the pool was avoiding,
/// and on a worker agent the thread being blocked is the worker's own. It is
/// the wrong fallback for work that must not run on the JS thread at all; such
/// a caller uses [`submit`] and keeps its own transport (that is what
/// `node_api_host`'s async work does).
///
/// The refusal is visible: `refused=` on the `PERRY_LOOP_STATS` line counts
/// exactly the jobs that took this path.
pub fn submit_or_run_inline<T, W, D>(work: W, deliver: D) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Delivery<T>) + 'static,
{
    submit_or_run_inline_rooted(Vec::new(), work, move |delivery, _| deliver(delivery))
}

/// [`submit_or_run_inline`], plus NaN-boxed JS values held alive for the job's
/// lifetime — see [`submit_rooted`].
///
/// On the inline path the roots are handed straight back, unchanged: nothing
/// could have collected between parking them and using them, because the work
/// ran without returning to the event loop.
pub fn submit_or_run_inline_rooted<T, W, D>(roots: Vec<u64>, work: W, deliver: D) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Delivery<T>, Vec<u64>) + 'static,
{
    // Both closures are consumed by a submission that succeeds and dropped by
    // one that does not, so the fallback needs them back. They are parked in
    // slots the pool path takes from: `work` behind a `Mutex` because it
    // crosses threads, `deliver` behind a `RefCell` because it must not.
    let work_slot = std::sync::Arc::new(std::sync::Mutex::new(Some(work)));
    let deliver_slot = std::rc::Rc::new(RefCell::new(Some(deliver)));
    let pool_work = work_slot.clone();
    let pool_deliver = deliver_slot.clone();
    let inline_roots = roots.clone();
    let accepted = submit_rooted(
        roots,
        move || {
            pool_work
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .map(|work| work())
        },
        move |delivery, roots| {
            let Some(deliver) = pool_deliver.borrow_mut().take() else {
                return;
            };
            deliver(
                match delivery {
                    Delivery::Done(Some(value)) => Delivery::Done(value),
                    // The slot is filled at construction and emptied exactly
                    // once, by this job; an empty slot would mean it ran twice.
                    Delivery::Done(None) => Delivery::Failed(Error::new(ErrorKind::Other)),
                    Delivery::Cancelled => Delivery::Cancelled,
                    Delivery::Failed(e) => Delivery::Failed(e),
                },
                roots,
            );
        },
    )
    .is_ok();
    if accepted {
        return true;
    }
    let work = work_slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let (Some(work), Some(deliver)) = (work, deliver_slot.borrow_mut().take()) else {
        return false;
    };
    deliver(Delivery::Done(work()), inline_roots);
    false
}

/// Ask the driver to cancel a job.
///
/// Best effort, exactly as DESIGN D8 specifies: a job the pool has already
/// started runs to the end, and whether it reports `Done` or `Cancelled`
/// depends on who won the race. Either way it produces exactly one delivery.
/// Returns false when the job is unknown — it has already been delivered, or
/// it belongs to another thread.
pub fn cancel(job: JobId) -> bool {
    let op = POOL.with(|state| state.borrow().jobs.get(&job.0).map(|entry| entry.op));
    let Some(op) = op else {
        return false;
    };
    crate::event_pump::with_pool_driver(|driver| driver.cancel(op)).unwrap_or(false)
}

/// Drive this thread's loop for at most `budget_ms` so pool completions that
/// are ready (or about to be) are delivered.
///
/// DESIGN §9's v2 `run_pending`: a synchronous native API that blocks the JS
/// thread waiting for a pool result has to turn the loop itself, because a
/// turn is the only thing that collects a completion. Bounded by construction
/// — one turn, one OS wait at most (DESIGN §10 rule 3).
pub fn turn(budget_ms: u64) {
    crate::event_pump::js_loop_turn_bounded(budget_ms);
}

// ── Dispatch ────────────────────────────────────────────────────────────────

/// Route one pool completion to the job that produced it.
///
/// Called by `agent_loop::dispatch_staged` **after** `turn` has returned, out
/// of a staging buffer with no borrow held on the loop (DESIGN D1), so a
/// `deliver` closure may run JS, allocate, collect and submit more work.
pub fn dispatch(completion: Completion) {
    let (op, id) = token_parts(completion.token);
    if op != OP_JOB {
        return;
    }
    let delivery = match completion.result {
        OpResult::Blocking(Payload::Boxed(value)) => Delivery::Done(value),
        OpResult::Cancelled | OpResult::Stopped => Delivery::Cancelled,
        OpResult::Err(e) => Delivery::Failed(e),
        // turnloop only ever produces the three above for a `blocking`
        // submission. Anything else is a driver contract break, not a job
        // result, and must not be reported as one.
        _ => Delivery::Failed(Error::new(ErrorKind::Other)),
    };
    deliver(id, delivery);
}

/// Take a job out of the table and run its `deliver`, accounting for exactly
/// one outcome. A stale id (a completion for a job already delivered at
/// shutdown) is dropped.
fn deliver(id: u64, delivery: Delivery<Erased>) {
    let Some(job) = POOL.with(|state| state.borrow_mut().jobs.remove(&id)) else {
        return;
    };
    match &delivery {
        Delivery::Done(_) => COMPLETED.fetch_add(1, Ordering::Relaxed),
        Delivery::Cancelled => CANCELLED.fetch_add(1, Ordering::Relaxed),
        Delivery::Failed(_) => FAILED.fetch_add(1, Ordering::Relaxed),
    };
    let Job { deliver, roots, .. } = job;
    // Released on drop, so a `deliver` that unwinds cannot leave the process
    // pinned alive by a job that has already been delivered.
    let _outstanding = OutstandingGuard;
    // Outside the table borrow: `deliver` runs JS, which can collect (and so
    // re-enter `scan_roots_mut`) and can submit another job.
    deliver(delivery, roots);
}

/// Holds the one `OUTSTANDING` reference a delivery is retiring, and releases
/// it on drop — including on an unwind out of the delivery.
struct OutstandingGuard;

impl Drop for OutstandingGuard {
    fn drop(&mut self) {
        let previous = OUTSTANDING.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "turnloop_pool OUTSTANDING underflow");
    }
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

/// Settle every outstanding job on this thread as `Cancelled`, because the
/// loop is going away and nothing will complete into it any more.
///
/// A job whose work is still running on a pool thread will finish and try to
/// push its result into a `WorkPort` the dropped loop has closed, which
/// discards it. Without this the awaiting promise would simply never settle
/// and the addon's `complete` callback would never run — the caller would have
/// been told "accepted" and then handed nothing, breaking the exactly-once
/// contract on the one path where it matters least to the program and most to
/// the accounting. Called from `event_pump::agent_loop::shutdown_current_thread`.
pub fn shutdown_current_thread() {
    // Snapshot first: a `deliver` closure may legitimately submit another job
    // (a settlement that kicks off follow-up work), and draining "whatever is
    // in the table now" would then never terminate. Anything submitted during
    // the drain is left for the loop's own teardown, which is about to free it.
    let ids: Vec<u64> = POOL.with(|state| state.borrow().jobs.keys().copied().collect());
    for id in ids {
        deliver(id, Delivery::Cancelled);
    }
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    let leftover = POOL.with(|state| {
        let mut state = state.borrow_mut();
        state.next_id = 0;
        state.jobs.drain().count()
    });
    if leftover > 0 {
        OUTSTANDING.fetch_sub(leftover, Ordering::AcqRel);
    }
}
