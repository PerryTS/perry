//! Run a job on the loop of the agent this thread is acting for (turnloop P10).
//!
//! # The decline this deletes
//!
//! Every binding in this repo that still links tokio keeps it for one sentence:
//! *"a thread that could not get a loop of its own"*. Since turnloop P9 gave
//! every JS agent a loop, that thread is not a worker — it is a **second thread
//! acting for an agent another thread already owns**. Android is the shape: the
//! `perry-native` thread runs the compiled TypeScript on the primary heap while
//! the UI thread pumps for the same agent, and whichever claims the route first
//! leaves the other unable to submit. Until now the loser's only answer was to
//! keep a whole async runtime alive for itself.
//!
//! [`post_job`] is the other answer. The work runs on the agent's **owner**,
//! which is a thread serving the *same JS heap*, so the reply is built where
//! that agent's values live — the #1824 rule the `spawn_blocking` paths had to
//! obey by hand, now structural.
//!
//! # What a binding does with it
//!
//! ```ignore
//! // At creation, once, the way `turnloop_net::available` is asked:
//! let transport = if turnloop_net::available(SUBSYSTEM) {
//!     Transport::Direct          // this thread owns the loop: submit here
//! } else if agent_post::available() {
//!     Transport::Posted          // the owner does the I/O for us
//! } else {
//!     Transport::Legacy          // no loop anywhere: keep the old driver
//! };
//! ```
//!
//! # Ownership
//!
//! A job is *transferred*. [`post_job`] takes a `Box` and gives it back only
//! when the post did not land, so the "did the runtime take this?" question the
//! raw C ABI answers with the sign of an `i32` is a `Result` here and cannot be
//! got wrong. There is no way to hold a reference to a job in flight.

use std::os::raw::c_void;

// Gated the way every call site below is, so a standalone `cargo test -p
// perry-ffi` — which links no runtime and therefore calls none of these — does
// not carry three dead declarations.
#[cfg(any(not(test), feature = "runtime-link"))]
extern "C" {
    fn js_perry_agent_post_available() -> i32;
    fn js_perry_agent_post(run: Option<extern "C" fn(*mut c_void)>, ctx: *mut c_void) -> i32;
    fn js_perry_agent_post_dispatched() -> u64;
}

/// Work a binding hands to the loop of the agent it is acting for.
///
/// `Send` because the job crosses to another thread; `'static` because the
/// runtime holds it for an unbounded time. It does **not** need `Sync`: one
/// thread owns the box at a time, and ownership moves with it.
pub trait AgentJob: Send + 'static {
    /// Run on the agent's owner thread. Called exactly once, and only for a
    /// job [`post_job`] accepted.
    fn run(self: Box<Self>);
}

/// A post that did not land, with the caller's job handed back.
///
/// The two cases call for different answers, which is why they are not one
/// variant: `NoRoute` means this agent has no loop at all and the binding's own
/// fallback transport is the correct behaviour, while `Again` is transient.
#[derive(Debug)]
pub enum Rejected<J> {
    /// No loop exists for this agent. Use your legacy transport.
    NoRoute(Box<J>),
    /// The owner is between claiming its route and publishing its loop, or its
    /// postbox is full. Retry, or fall back.
    Again(Box<J>),
}

impl<J> Rejected<J> {
    /// Take the job back, whichever refusal this was.
    pub fn into_job(self) -> Box<J> {
        match self {
            Rejected::NoRoute(job) | Rejected::Again(job) => job,
        }
    }

    /// Whether this agent has no loop at all, so retrying cannot help.
    pub fn is_permanent(&self) -> bool {
        matches!(self, Rejected::NoRoute(_))
    }
}

/// Whether a post from this thread would reach a loop.
///
/// Asked once, when a binding decides which transport a connection lives on —
/// [`post_job`] consumes its job on the way in, so "would this land?" cannot be
/// answered by trying. False on a host where loop creation failed — the only
/// case where the legacy transport must stay.
///
/// Says nothing about whether *this* thread owns that loop. A thread that does
/// should submit directly and already knows so from
/// [`crate::turnloop_net::available`].
pub fn available() -> bool {
    #[cfg(any(not(test), feature = "runtime-link"))]
    {
        // SAFETY: a plain predicate in the linked runtime.
        unsafe { js_perry_agent_post_available() != 0 }
    }
    #[cfg(all(test, not(feature = "runtime-link")))]
    {
        false
    }
}

/// The trampoline the runtime calls. Monomorphised per job type, so the
/// reconstituted box has the type it was posted with and no extra allocation
/// or type-erasure step is needed.
extern "C" fn run_job<J: AgentJob>(ctx: *mut c_void) {
    // SAFETY: `post_job` passed `Box::into_raw` of exactly this type, the
    // runtime invokes a job exactly once, and this is that invocation.
    let job: Box<J> = unsafe { Box::from_raw(ctx.cast::<J>()) };
    job.run();
}

/// Hand `job` to the loop of the agent this thread is acting for.
///
/// `Ok(())` means the owner will run it exactly once. `Err` hands the job back
/// unrun, and says whether retrying could help.
///
/// One qualification on "will run": a job still queued when the owner's loop
/// goes down is dropped without being invoked — turnloop's postbox is not
/// drained at teardown. The job's `Drop` runs, so a binding that must settle
/// something can do it there; nothing is freed twice. This is bounded to agent
/// teardown (a retiring Worker, or process exit), when that agent's heap is
/// going away anyway.
pub fn post_job<J: AgentJob>(job: Box<J>) -> Result<(), Rejected<J>> {
    #[cfg(any(not(test), feature = "runtime-link"))]
    {
        let ctx = Box::into_raw(job);
        // SAFETY: `ctx` is a live box of exactly the type `run_job::<J>`
        // reconstitutes. On a non-negative code the runtime owns it and will
        // invoke the trampoline once; on a negative one it was not taken and
        // the `Box::from_raw` below is the only claim on it.
        let code = unsafe { js_perry_agent_post(Some(run_job::<J>), ctx.cast()) };
        if code >= 0 {
            // Accepted (0) or accepted-but-the-wake-failed (1). Both mean the
            // job is queued and will run: turnloop returns no payload for a
            // wake failure precisely so a caller cannot deliver it twice.
            return Ok(());
        }
        // SAFETY: a negative code means the runtime took nothing, so this box
        // is still ours and unaliased.
        let job = unsafe { Box::from_raw(ctx) };
        Err(if code == NO_ROUTE {
            Rejected::NoRoute(job)
        } else {
            Rejected::Again(job)
        })
    }
    #[cfg(all(test, not(feature = "runtime-link")))]
    {
        // No runtime is linked, so there is no loop to post to — the same
        // answer a host whose loop creation failed gets.
        let _ = run_job::<J>;
        Err(Rejected::NoRoute(job))
    }
}

/// The runtime's code for "no loop exists for this agent".
#[cfg(any(not(test), feature = "runtime-link"))]
const NO_ROUTE: i32 = -2;

/// How many posted jobs *this* thread has run.
///
/// The liveness counter. A post that silently went nowhere and a post that ran
/// are indistinguishable from the caller's side, so a test claiming "the owner
/// carried this" must watch this move — exactly what
/// [`crate::turnloop_net::sink_installed`] is for on the net path.
pub fn dispatched() -> u64 {
    #[cfg(any(not(test), feature = "runtime-link"))]
    {
        // SAFETY: a plain counter read in the linked runtime.
        unsafe { js_perry_agent_post_dispatched() }
    }
    #[cfg(all(test, not(feature = "runtime-link")))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static RAN: AtomicUsize = AtomicUsize::new(0);
    static DROPPED: AtomicUsize = AtomicUsize::new(0);

    struct Probe(u64);
    impl Drop for Probe {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl AgentJob for Probe {
        fn run(self: Box<Self>) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// With no runtime linked there is no loop, which is the `NoRoute` case —
    /// and the binding can only take its fallback if it gets its job back. The
    /// point is the reclaim, not the refusal: assert the job is returned intact
    /// and that nothing ran it.
    #[test]
    fn a_refused_post_hands_the_job_back_unrun() {
        assert!(
            !available(),
            "no runtime is linked, so nothing can be posted"
        );
        let before_dropped = DROPPED.load(Ordering::SeqCst);

        let rejected = post_job(Box::new(Probe(7))).expect_err("nothing to post to");
        assert!(rejected.is_permanent(), "no loop at all is not transient");
        let job = rejected.into_job();
        assert_eq!(job.0, 7, "the job came back intact");
        assert_eq!(
            RAN.load(Ordering::SeqCst),
            0,
            "a refused job must never be run"
        );
        assert_eq!(
            DROPPED.load(Ordering::SeqCst),
            before_dropped,
            "the runtime must not drop a job it refused"
        );

        drop(job);
        assert_eq!(
            DROPPED.load(Ordering::SeqCst),
            before_dropped + 1,
            "the caller reclaimed and dropped it exactly once"
        );
    }

    /// `Rejected` must keep the two refusals apart: one says "use your fallback
    /// forever", the other "try again". Collapsing them would make a binding
    /// either spin on a dead agent or abandon a live one.
    ///
    /// Deliberately carries a plain `u64` rather than a `Probe`: the drop
    /// counter above is process-global, and `cargo test` runs these in
    /// parallel, so a second test that dropped a `Probe` would make the first
    /// one's "nothing was dropped" assertion race.
    #[test]
    fn the_two_refusals_stay_distinguishable() {
        assert!(Rejected::NoRoute(Box::new(1u64)).is_permanent());
        assert!(!Rejected::Again(Box::new(2u64)).is_permanent());
        assert_eq!(*Rejected::Again(Box::new(3u64)).into_job(), 3);
    }
}
