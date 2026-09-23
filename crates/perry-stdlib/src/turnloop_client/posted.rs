//! turnloop P10: run an outbound request on the loop of the agent this thread
//! is acting for.
//!
//! # The decline this deleted
//!
//! `perry-stdlib`'s `reqwest` edge was held open by one sentence — *"a thread
//! that could not get a loop of its own"*. Since turnloop P9 gave every JS
//! agent a loop, that thread is not a worker: it is a **second thread acting
//! for an agent another thread already owns**. Android is the shape, with
//! `perry-native` running the compiled TypeScript on the primary heap while the
//! UI thread pumps for the same agent; whichever claims the route first leaves
//! the other unable to submit. Until P10 the loser's only answer was to run a
//! `reqwest` future on the tokio runtime for itself.
//!
//! [`try_submit`] is the other answer. The whole submission crosses to the
//! agent's **owner**, a thread serving the *same JS heap*, so the response is
//! built and the promise settled where that agent's values live.
//!
//! # What may be posted, and what must not
//!
//! Only [`Declined::NoLoop`] is a property of the *thread*. `Unsupported`,
//! `Proxy` and `NoTls` are properties of the *request* and of process-wide
//! configuration, so the owner would refuse them for the same reason — and by
//! then the caller has been told the engine took the request, so it could not
//! reject it with the refusal's own error. `submit` therefore prepares the request before it
//! chooses a transport, and only a thread-shaped decline reaches this module.
//!
//! # GC
//!
//! A posted job carries owned `String`/`Vec<u8>` and the `usize` address of a
//! promise created by `js_promise_new_cross_thread`, which pins it (#9552).
//! Exactly what the engine itself holds, and for the same reason: no JS value
//! and no heap pointer crosses a thread here, so this module registers no root
//! scanner.

use perry_ffi::agent_post::{self, AgentJob};

use super::{exchange::ClientError, Declined, Outcome, RequestSpec, Sink};

/// One submission handed to the thread that owns this agent's loop.
///
/// `Send` because every field is: `RequestSpec` is owned bytes and strings, and
/// `Sink` is a `usize` plus plain `fn` pointers — the reason P6 chose function
/// pointers over boxed closures pays for the crossing as well as for the
/// collector.
struct PostedRequest {
    spec: RequestSpec,
    sink: Sink,
}

impl AgentJob for PostedRequest {
    fn run(self: Box<Self>) {
        let PostedRequest { spec, sink } = *self;
        if super::submit_on_owner(spec, sink).is_err() {
            // We are on the owner and it refused anyway — it lost its route
            // between the post and this turn. There is no falling back from
            // here: the caller was told the engine took this request, so
            // nothing else will settle the promise. A promise nobody settles is the one outcome a caller
            // cannot recover from, so report the failure rather than drop it.
            (sink.on_done)(
                sink.ctx,
                Outcome::Err(ClientError {
                    code: "ECONNRESET",
                    message: "the agent's event loop went away before the request started"
                        .to_string(),
                    syscall: None,
                    aborted: false,
                }),
            );
        }
    }
}

/// One `AbortSignal` cancellation handed to the agent's owner.
struct PostedAbort {
    signal_ptr: usize,
}

impl AgentJob for PostedAbort {
    fn run(self: Box<Self>) {
        super::abort_signal_here(self.signal_ptr);
    }
}

/// Whether a post from this thread would reach a loop, asked without a job.
///
/// `submit` needs the answer before it prepares a request, because preparing
/// one is not free — the first `tls_config()` call loads the platform root
/// store.
pub(super) fn available() -> bool {
    agent_post::available()
}

/// Hand a submission to the thread that owns this agent's loop.
///
/// `Ok(())` keeps [`super::submit`]'s contract: the sink will be called exactly
/// once, over there. `Err(Declined::NoLoop)` means no loop exists for this
/// agent at all — the only case is a host where `Loop::new` failed — and the
/// caller must reject.
///
/// One qualification on "will be called", inherited from `agent_post`: a job
/// still queued when the owner's loop goes down is dropped without being
/// invoked, because turnloop's postbox is not drained at teardown. That is
/// bounded to agent teardown (a retiring Worker, or process exit), when the
/// heap holding the promise is going away regardless.
pub(super) fn try_submit(spec: RequestSpec, sink: Sink) -> Result<(), Declined> {
    match agent_post::post_job(Box::new(PostedRequest { spec, sink })) {
        Ok(()) => Ok(()),
        // `Again` is the owner between claiming its route and publishing its
        // loop, or a full postbox. Both read to the caller the same way
        // `NoRoute` does. They used to fall back to a reqwest future for this
        // one request; with no second transport they now reject it.
        Err(_) => Err(Declined::NoLoop),
    }
}

/// Hand an `AbortSignal` cancellation to the agent's owner.
///
/// Fire-and-forget: the request tables live over there, so how many requests
/// were actually cancelled is not knowable here. A refused post is a no-op —
/// the same outcome as a signal no in-flight request is bound to.
pub(super) fn try_abort(signal_ptr: usize) {
    let _ = agent_post::post_job(Box::new(PostedAbort { signal_ptr }));
}
