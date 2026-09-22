//! turnloop P10: run a host job on the loop of the agent this thread acts for.
//!
//! # The decline this exists to delete
//!
//! Every remaining tokio edge in a binding crate is held by the same sentence:
//! *"a thread that could not get a loop of its own"*. Since turnloop P9 every
//! JS agent has a loop, so that thread is no longer a worker — it is a **second
//! thread acting for an agent another thread already owns**. Android is the
//! shape: the `perry-native` thread runs the compiled TypeScript on the primary
//! heap while the UI thread pumps for the same agent, and whichever of the two
//! claims the route first leaves the other with no loop to submit on. Until now
//! the loser's only option was to keep a whole tokio driver alive for itself.
//!
//! [`event_pump::post_to_agent`](crate::event_pump::post_to_agent) removed that
//! constraint: any thread can hand work to the loop of the agent it is acting
//! for. This module is the part the *bindings* can reach, because a binding
//! crate depends on `perry-ffi` and not on this one — see [`abi`].
//!
//! # Why it is sound
//!
//! Both threads serve the **same agent's heap**. The job runs on the owner,
//! which is where that agent's JS values live, so a completion never crosses an
//! agent boundary — and the caller cannot make it, because the ABI takes no
//! agent argument. The runtime resolves [`crate::agent::current_agent`] itself,
//! which makes the unsound call unexpressible rather than merely discouraged.
//!
//! # The one rule a C caller must follow
//!
//! A job is an `extern "C" fn(*mut c_void)` plus an opaque context pointer, and
//! the context is *transferred* on a successful post. [`Posted`] encodes that
//! as a sign: **a non-negative code means the runtime owns the context and will
//! invoke the callback exactly once; a negative code means the context is
//! untouched and still the caller's.** There is no third state, and in
//! particular `QueuedUnwoken` is on the non-negative side because the post was
//! accepted — turnloop's `Poster::post` returns no payload for a wake failure
//! precisely so a caller cannot retry work that is already queued.

use std::any::Any;
use std::cell::Cell;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use turnloop::Token;

pub mod abi;

#[cfg(test)]
mod tests;

/// Token op class for a posted host job.
///
/// The router in `event_pump::agent_loop::dispatch_staged` is a sequence of
/// range tests over `token >> ID_BITS`: P1's net classes are 1..=7, P2's are
/// 0x10..=0x1F, P4's are 0x20..=0x2F, P3 owns `TIMER_TOKEN` (class 0xFF), and
/// anything unclaimed falls through to P1. So a new post class has to be both
/// outside every band above *and* explicitly branched on before that
/// fall-through, or its completions would be delivered to the net subsystem.
/// 0x30..=0x3F is the next free band; only [`CLASS_JOB`] is used today.
const CLASS_MIN: u64 = 0x30;
const CLASS_JOB: u64 = 0x30;
const CLASS_MAX: u64 = 0x3F;

/// Width of the id half of a token, matching P2's and P4's split.
const ID_BITS: u32 = 56;
const ID_MASK: u64 = (1 << ID_BITS) - 1;

/// Source of job ids. Process-wide rather than per-thread because the *posting*
/// thread builds the token and the *owning* thread reads it; a per-thread
/// counter would hand two threads the same token for different jobs, which a
/// trace could not tell apart.
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

crate::perry_thread_local! {
    /// Posted jobs this thread has run. The liveness counter: a test that
    /// claims "the owner carried this" must watch it move, because a post that
    /// silently went nowhere and a post that ran look identical from the
    /// caller's side.
    static DISPATCHED: Cell<u64> = const { Cell::new(0) };
}

fn job_token() -> Token {
    let id = NEXT_JOB.fetch_add(1, Ordering::Relaxed) & ID_MASK;
    // Id 0 would still route correctly (the class is what `owns` tests), but a
    // zero id reads as "unset" in a trace, so skip it on the wrap.
    let id = if id == 0 { 1 } else { id };
    Token((CLASS_JOB << ID_BITS) | id)
}

/// Whether this completion is a posted host job rather than P1/P2/P4 work.
///
/// The router asks this and nothing else, so the token spaces cannot overlap by
/// accident: a class outside this range is not this module's, by definition.
#[inline]
pub fn owns(t: Token) -> bool {
    (CLASS_MIN..=CLASS_MAX).contains(&(t.0 >> ID_BITS))
}

/// One host job in flight, as it crosses to the owning thread.
///
/// # Safety
///
/// The raw context pointer is what makes this `Send`, and nothing here can
/// check it. The promise is the ABI's: a caller that posts a context promises
/// the value behind it is safe to use from the agent's owner thread — which is
/// a thread serving the *same heap*, so the practical requirement is `Send`,
/// not `Sync`, and not thread-affinity to the poster.
struct HostJob {
    run: extern "C" fn(*mut c_void),
    ctx: *mut c_void,
}

// SAFETY: forwarded contract from `HostJob`'s own safety note — the caller of
// `post` promises the context may be used from the agent's owning thread. The
// box is moved, never shared, so one thread holds it at a time.
unsafe impl Send for HostJob {}

/// The outcome of a post, and with it the ownership of the caller's context.
///
/// Read the sign, not the variant, when deciding whether to free a context:
/// [`Posted::code`] is non-negative exactly when the runtime took ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posted {
    /// Accepted and the owner was woken. The callback runs exactly once.
    Accepted,
    /// Accepted, but waking the owner failed; the job stays queued and runs on
    /// the owner's next turn. The context is **consumed** — retrying would
    /// deliver it twice.
    QueuedUnwoken,
    /// No loop exists for this agent at all. The context is untouched and the
    /// caller must use its own fallback transport.
    NoRoute,
    /// Transient: the owner is between claiming its route and publishing its
    /// loop, or the postbox is full. The context is untouched; retry or fall
    /// back.
    Again,
}

impl Posted {
    /// The C return code. Non-negative means the context was consumed.
    pub const fn code(self) -> i32 {
        match self {
            Posted::Accepted => 0,
            Posted::QueuedUnwoken => 1,
            Posted::NoRoute => -2,
            Posted::Again => -3,
        }
    }

    /// Whether the runtime now owns the context the caller handed in.
    pub const fn consumed(self) -> bool {
        self.code() >= 0
    }
}

/// Whether a post from this thread would reach a loop, asked without a job.
///
/// A binding decides which transport a connection lives on at creation, before
/// it has anything to post, and [`post`] consumes its context on the way in —
/// so "would this land?" cannot be answered by trying it. Says nothing about
/// whether *this* thread owns that loop: a thread that does should submit
/// directly, and already knows so from `turnloop_net::available()`.
pub fn available() -> bool {
    crate::event_pump::has_route(crate::agent::current_agent())
    // A host where loop creation failed has no loop to post to, so the binding
    // keeps its legacy transport. Since the A/B baseline arm was deleted that
    // is the ONE decline reason posting cannot close — and unlike the arm, it
    // is not deliberate.
}

/// Hand `run(ctx)` to the loop of the agent this thread is acting for.
///
/// # The one case where an accepted job never runs
///
/// turnloop's `Loop` has no `Drop` that drains its postbox, so a job still
/// queued when the owner's loop goes down is dropped without being invoked.
/// That is the safe direction — the context leaks rather than being freed twice
/// — but a caller whose job was going to settle something (a `JsPromise`, say)
/// gets neither a completion nor an error. It is bounded to agent teardown:
/// the owner's loop is dropped at `shutdown_agent_loop` (a retiring Worker) or
/// at the process-exit funnel, when that agent's heap is going away regardless.
/// Named here rather than papered over, because "accepted" otherwise reads as
/// "will run" without qualification.
///
/// # Safety
///
/// `ctx` must be valid until `run` is invoked, and safe to use from the agent's
/// owning thread. On a non-negative outcome the runtime owns it and will invoke
/// `run` exactly once; on a negative one the caller still owns it.
pub unsafe fn post(run: extern "C" fn(*mut c_void), ctx: *mut c_void) -> Posted {
    use crate::event_pump::{post_to_agent, PostToAgentError};
    let job = HostJob { run, ctx };
    let payload = turnloop::Payload::Boxed(Box::new(job));
    match post_to_agent(crate::agent::current_agent(), job_token(), payload) {
        Ok(()) => Posted::Accepted,
        Err(PostToAgentError::NoRoute) => Posted::NoRoute,
        Err(PostToAgentError::NotPublished) => Posted::Again,
        // `payload: Some` is turnloop's "not accepted, take it back"; the
        // box is dropped here and the caller keeps its context. `None` is
        // the opposite and must NOT read as a failure the caller retries:
        // the job is queued and will run.
        Err(PostToAgentError::Refused { payload: Some(_) }) => Posted::Again,
        Err(PostToAgentError::Refused { payload: None }) => Posted::QueuedUnwoken,
    }
}

/// Run one posted job on the agent's owner.
///
/// Called from the router in `dispatch_staged`, which deliberately runs outside
/// the `AGENT_LOOP` borrow — a job legitimately submits new operations (the
/// whole point is that it does), and would otherwise re-enter a live borrow.
pub fn dispatch(completion: turnloop::Completion) {
    let turnloop::OpResult::Posted(payload) = completion.result else {
        // The class says "posted job", so anything else is a driver-side
        // mismatch rather than a caller error. Drop it rather than guess.
        return;
    };
    let turnloop::Payload::Boxed(boxed) = payload else {
        return;
    };
    let boxed: Box<dyn Any + Send> = boxed;
    let Ok(job) = boxed.downcast::<HostJob>() else {
        return;
    };
    DISPATCHED.with(|n| n.set(n.get().saturating_add(1)));
    // SAFETY: the context came from `post`, whose caller promised it is valid
    // until invoked and usable from this thread. This is the one invocation.
    (job.run)(job.ctx);
}

/// Posted jobs this thread has run.
///
/// A test that claims a workload was carried by the owner asserts this moved;
/// without it "the post was accepted" and "the job ran" are indistinguishable.
pub fn dispatched() -> u64 {
    DISPATCHED.with(Cell::get)
}
