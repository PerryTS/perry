//! perry-ffi async ABI **v2**: blocking work on turnloop's shared pool
//! (turnloop DESIGN §9, §12 "P4").
//!
//! # What v1 was, and why v2 exists
//!
//! v1 is [`crate::spawn_blocking`] / [`crate::spawn_blocking_with_reactor`] /
//! [`crate::spawn_async`] / [`crate::run_pending`]. All four assume an ambient
//! tokio runtime: the closure lands on tokio's blocking pool, and a binding
//! that needs to `await` runs `Handle::current().block_on` inside it. That
//! model has three problems Perry actually paid for:
//!
//! 1. **The result comes back on the wrong thread.** A v1 closure resolves its
//!    own promise, so every binding has to remember that building a JSValue on
//!    a pool thread allocates from an arena the main thread never sees
//!    (#1824). The rule lives in doc comments, and the compiler does not check
//!    it.
//! 2. **There is no completion.** v1 detaches. The caller cannot cancel, cannot
//!    tell "refused" from "running", and the event loop needs a separate
//!    in-flight counter (#591) to know the work exists at all.
//! 3. **It needs tokio**, which is what the turnloop migration removes.
//!
//! v2 is one call, [`submit`], and it fixes all three by splitting the job in
//! two:
//!
//! - `work` runs **on a pool thread**. `FnOnce() -> T + Send`, so only owned
//!   Rust data can cross — the #1824 rule becomes a trait bound instead of a
//!   comment.
//! - `deliver` runs **on the thread that submitted**, inside the loop's
//!   completion dispatch. That is where promises settle and JSValues are
//!   built. It is deliberately not `Send`.
//!
//! Exactly one [`Outcome`] is delivered per accepted job (turnloop DESIGN D4).
//! A submission the runtime refuses returns [`PoolError`] and delivers
//! nothing, so a binding never has to guess whether its completion will run.
//!
//! ```no_run
//! # use perry_ffi::{pool, JsPromise, JsValue};
//! # fn hash(password: String, cost: u32) -> Result<String, String> { unimplemented!() }
//! # let (password, cost) = (String::new(), 10u32);
//! let promise = JsPromise::new();
//! let job = pool::submit(
//!     // On a pool thread: owned Rust data only.
//!     move || hash(password, cost),
//!     // On the owning thread: JS values are legal here and nowhere else.
//!     move |outcome| match outcome {
//!         pool::Outcome::Done(Ok(digest)) => promise.resolve_string(&digest),
//!         pool::Outcome::Done(Err(e)) => promise.reject_string(&e),
//!         pool::Outcome::Cancelled => promise.reject_string("cancelled"),
//!         pool::Outcome::Failed => promise.reject_string("hashing panicked"),
//!     },
//! );
//! ```
//!
//! # Which v1 entry points remain, and why
//!
//! - [`crate::run_pending`] stays, and is now a **shim over v2**: it takes one
//!   bounded turnloop turn (so a pool completion is actually collected) before
//!   driving whatever tokio work is left. A synchronous binding's poll loop
//!   needs no change.
//! - [`crate::spawn_blocking`] and [`crate::spawn_blocking_with_reactor`] stay
//!   on tokio. They are **not** shimmed onto this pool, and that is a decision
//!   rather than an omission: their remaining callers (the `node:http2` accept
//!   loop, the HTTP/2 client and request runtimes, and every database binding
//!   that runs `Handle::current().block_on`) hold their thread for the lifetime
//!   of a *connection*, not of a job. turnloop's pool is bounded and fixed-size
//!   by design (DESIGN D8: four threads by default), so hosting an unbounded
//!   number of connection-lifetime occupants on it would deadlock under load.
//!   Those callers are rewritten by P5–P7, which replace the tokio I/O inside
//!   them; the shims go with tokio in P8.
//! - [`crate::spawn_async`] stays on tokio for the same reason: every current
//!   caller's future is tokio I/O (hyper, tokio-tungstenite, `TcpStream`), so a
//!   loop-executor v2 would today be an API with no caller — the kind of
//!   untested mode Perry's own kill-policy says not to ship.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

extern "C" {
    fn perry_ffi_pool_submit(
        ctx: *mut c_void,
        run_on_pool: extern "C" fn(*mut c_void),
        deliver_on_owner: extern "C" fn(*mut c_void, i32),
    ) -> u64;
    fn perry_ffi_pool_cancel(job: u64) -> i32;
    fn perry_ffi_pool_turn(budget_ms: u64);
}

/// Outcome codes on the C ABI. Kept as plain integers so the boundary carries
/// no Rust layout.
const OUTCOME_DONE: i32 = 0;
const OUTCOME_CANCELLED: i32 = 1;
const OUTCOME_FAILED: i32 = 2;

/// The single result of an accepted job, delivered on the submitting thread.
#[derive(Debug)]
pub enum Outcome<T> {
    /// The pool ran `work` and this is what it returned.
    Done(T),
    /// The job never ran: [`cancel`] won the race, or the loop shut down while
    /// the job was still queued.
    Cancelled,
    /// The job panicked on the pool thread. The panic is contained: it never
    /// crosses the C boundary and never reaches the JS thread.
    Failed,
}

/// Why a submission was refused. A refused job delivers nothing at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolError {
    /// This thread has no turnloop loop, or the pool queue is full. Either
    /// way the caller keeps whatever fallback it had — run the work inline,
    /// or report backpressure to JS.
    Unavailable,
}

/// A handle to an accepted job, for [`cancel`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Job(u64);

impl Job {
    /// The raw id, for a binding that stores it in a C-visible field.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Rebuild a job handle from [`Job::raw`].
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// Everything one job owns, allocated and freed on the submitting thread.
///
/// # Safety across the boundary
///
/// The address of this box crosses to a pool thread as a `usize`. Only `work`
/// (which is `Send`), `out` (`T: Send`) and `panicked` are touched there;
/// `deliver` is never read off the owning thread, which is what makes a
/// non-`Send` `deliver` sound. The handover in both directions is ordered by
/// turnloop's completion queue.
struct Ctx<T, W, D> {
    work: Option<W>,
    deliver: Option<D>,
    out: Option<T>,
    panicked: bool,
}

extern "C" fn run_on_pool<T, W, D>(ctx: *mut c_void)
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Outcome<T>) + 'static,
{
    // SAFETY: `ctx` is the `Box::into_raw` from `submit`, still alive because
    // the delivery that frees it cannot run before this returns.
    let ctx = unsafe { &mut *(ctx as *mut Ctx<T, W, D>) };
    let Some(work) = ctx.work.take() else {
        ctx.panicked = true;
        return;
    };
    // A panic must not cross an `extern "C"` frame. Containing it here rather
    // than relying on the host's unwind regime keeps this correct whichever
    // way perry-runtime is built (#8479).
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => ctx.out = Some(value),
        Err(_) => ctx.panicked = true,
    }
}

extern "C" fn deliver_on_owner<T, W, D>(ctx: *mut c_void, outcome: i32)
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Outcome<T>) + 'static,
{
    // SAFETY: the runtime calls this exactly once per accepted job, on the
    // thread that submitted it; taking ownership here is what frees the box.
    let ctx: Box<Ctx<T, W, D>> = unsafe { Box::from_raw(ctx as *mut Ctx<T, W, D>) };
    let Ctx {
        deliver,
        out,
        panicked,
        ..
    } = *ctx;
    let Some(deliver) = deliver else {
        return;
    };
    let outcome = match (outcome, out, panicked) {
        (OUTCOME_DONE, Some(value), false) => Outcome::Done(value),
        (OUTCOME_CANCELLED, _, _) => Outcome::Cancelled,
        (OUTCOME_FAILED, _, _) => Outcome::Failed,
        // A `Done` with no value, or a code this ABI version does not know:
        // the job did not produce a result, which is a failure, not a silent
        // success with a default value.
        _ => Outcome::Failed,
    };
    // Contained for the same reason as the pool side: a binding's delivery
    // panicking must not unwind through the runtime's dispatch loop.
    let _ = catch_unwind(AssertUnwindSafe(move || deliver(outcome)));
}

/// Run `work` on Perry's shared bounded blocking pool and `deliver` its result
/// on this thread.
///
/// Returns [`PoolError::Unavailable`] when the job was not accepted — a thread
/// with no event loop (a `worker_threads` agent), or pool backpressure. In
/// that case `deliver` never runs and the caller keeps its own fallback.
pub fn submit<T, W, D>(work: W, deliver: D) -> Result<Job, PoolError>
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Outcome<T>) + 'static,
{
    let ctx = Box::into_raw(Box::new(Ctx {
        work: Some(work),
        deliver: Some(deliver),
        out: None::<T>,
        panicked: false,
    })) as *mut c_void;
    let job =
        unsafe { perry_ffi_pool_submit(ctx, run_on_pool::<T, W, D>, deliver_on_owner::<T, W, D>) };
    if job == 0 {
        // Refused: nothing will ever call the delivery trampoline, so this
        // side still owns the box and must free it. Dropping it runs neither
        // closure, which is exactly "the job never existed".
        // SAFETY: the pointer is the `Box::into_raw` above and the runtime
        // has not stored it.
        drop(unsafe { Box::from_raw(ctx as *mut Ctx<T, W, D>) });
        return Err(PoolError::Unavailable);
    }
    Ok(Job(job))
}

/// [`submit`], with the calling thread as the fallback when the pool refuses.
///
/// Returns true when the pool took the job. On **false** the work has already
/// run inline on this thread and `deliver` has already been called with
/// [`Outcome::Done`] — so a binding settles its promise exactly once either
/// way, and never has to carry a second code path for "no loop here".
///
/// This is the right shape for a binding whose caller is already waiting on
/// the answer (a hash, a compression, an image resize). A refusal means one of
/// two things, and both are better served by running than by failing: the
/// thread is a `worker_threads` agent with no event loop of its own, or the
/// pool is saturated and this is backpressure.
pub fn submit_or_run_inline<T, W, D>(work: W, deliver: D) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Outcome<T>) + 'static,
{
    // A submission consumes both closures whether or not it is accepted, so
    // the fallback needs them back: `work` behind a `Mutex` because it crosses
    // threads, `deliver` behind a `RefCell` because it must not.
    let work_slot = std::sync::Arc::new(std::sync::Mutex::new(Some(work)));
    let deliver_slot = std::rc::Rc::new(std::cell::RefCell::new(Some(deliver)));
    let pool_work = work_slot.clone();
    let pool_deliver = deliver_slot.clone();
    let accepted = submit(
        move || {
            pool_work
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .map(|work| work())
        },
        move |outcome| {
            let Some(deliver) = pool_deliver.borrow_mut().take() else {
                return;
            };
            deliver(match outcome {
                Outcome::Done(Some(value)) => Outcome::Done(value),
                // The slot is filled at construction and emptied exactly once.
                Outcome::Done(None) => Outcome::Failed,
                Outcome::Cancelled => Outcome::Cancelled,
                Outcome::Failed => Outcome::Failed,
            });
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
    deliver(Outcome::Done(work()));
    false
}

/// Fire-and-forget: run `work` on the shared pool, with no separate delivery.
///
/// This is v1 [`crate::spawn_blocking`]'s *shape* on v2's transport, for the
/// bindings whose closure already settles its own promise through a deferred
/// resolution (`JsPromise::resolve_with` / `reject_with`), which queues the JS
/// construction onto the owning thread by itself. Those callers get the tokio
/// removal without a rewrite.
///
/// The contract v1 did not state and this one does: **the job must be
/// bounded**. It occupies one of a small, fixed number of pool threads
/// (turnloop DESIGN D8) until it returns, so a closure that parks for the
/// lifetime of a connection belongs on [`crate::spawn_async`], not here.
///
/// Returns true when the pool took the job; on false the work has already run
/// inline on the calling thread.
pub fn run<W>(work: W) -> bool
where
    W: FnOnce() + Send + 'static,
{
    submit_or_run_inline(work, |_: Outcome<()>| {})
}

/// Ask the runtime to cancel an accepted job.
///
/// Best effort, as turnloop DESIGN D8 specifies: a job the pool has already
/// started runs to its end. Either way the job still produces exactly one
/// delivery. Returns false when the job is already delivered or unknown.
pub fn cancel(job: Job) -> bool {
    unsafe { perry_ffi_pool_cancel(job.0) != 0 }
}

/// Drive the event loop for at most `budget_ms` so ready pool completions are
/// delivered.
///
/// The v2 spelling of [`crate::run_pending`], for a *synchronous* binding that
/// blocks its thread waiting for a pool result. One bounded turn; it never
/// runs JS of its own.
pub fn turn(budget_ms: u64) {
    unsafe { perry_ffi_pool_turn(budget_ms) };
}

#[cfg(test)]
mod tests {
    use super::*;

    // The pool surface resolves against perry-stdlib's archive at link time,
    // which the perry-ffi unit-test binary does not have. End-to-end coverage
    // lives in perry-runtime's `turnloop_pool` suite (the runtime half) and in
    // the ext crates that call this (the ABI half). What IS checkable here is
    // the part with no `extern` in it: the outcome mapping, which is where a
    // silent "a cancelled job reported Done" bug would live.
    #[test]
    fn outcome_codes_map_one_to_one() {
        assert_eq!((OUTCOME_DONE, OUTCOME_CANCELLED, OUTCOME_FAILED), (0, 1, 2));
    }

    // Named fn items rather than closures: the trampolines are generic over
    // `D`, and nothing in a `*mut c_void` ties that parameter down, so a
    // closure would leave it uninferable.
    type Work = fn() -> u8;

    fn expect_failed(outcome: Outcome<u8>) {
        assert!(matches!(outcome, Outcome::Failed));
    }

    fn expect_cancelled(outcome: Outcome<u8>) {
        assert!(matches!(outcome, Outcome::Cancelled));
    }

    fn panicking_delivery(_outcome: Outcome<u8>) {
        panic!("binding delivery panicked");
    }

    fn ctx_for(out: Option<u8>, panicked: bool, deliver: fn(Outcome<u8>)) -> *mut c_void {
        Box::into_raw(Box::new(Ctx::<u8, Work, fn(Outcome<u8>)> {
            work: None,
            deliver: Some(deliver),
            out,
            panicked,
        })) as *mut c_void
    }

    #[test]
    fn a_delivery_reports_failed_when_the_pool_side_panicked() {
        let ctx = ctx_for(None, true, expect_failed);
        deliver_on_owner::<u8, Work, fn(Outcome<u8>)>(ctx, OUTCOME_DONE);
    }

    #[test]
    fn a_delivery_reports_cancelled_even_when_a_value_is_present() {
        // The runtime is the authority on whether the job ran: a cancel that
        // raced a finishing job must still read as Cancelled, or a binding
        // would settle a promise it had already rejected.
        let ctx = ctx_for(Some(9), false, expect_cancelled);
        deliver_on_owner::<u8, Work, fn(Outcome<u8>)>(ctx, OUTCOME_CANCELLED);
    }

    #[test]
    fn a_panicking_delivery_does_not_unwind_into_the_runtime() {
        let ctx = ctx_for(Some(1), false, panicking_delivery);
        // Must return normally: the panic is contained inside the trampoline.
        deliver_on_owner::<u8, Work, fn(Outcome<u8>)>(ctx, OUTCOME_DONE);
    }
}
