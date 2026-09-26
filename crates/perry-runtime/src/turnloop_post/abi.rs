//! The C ABI a separately linked binding uses to run a job on its agent's loop.
//!
//! A binding crate is a `staticlib` whose only Cargo dependency is `perry-ffi`,
//! so it cannot name a `turnloop::Payload` or hold a Rust closure. The C-safe
//! shape of "run this later, over there" is a function pointer plus an opaque
//! context, which [`js_perry_agent_post`] boxes for the owner to invoke.
//!
//! Three functions, and one rule that governs all of them: **a non-negative
//! return code means the runtime took the context.** See [`super::Posted`].
//!
//! No layout digest here, deliberately. `perry-ffi::turnloop_net` needs one
//! because both sides declare their own copy of a shared struct; this ABI
//! shares no struct at all — a function pointer, a `void*` and an `i32` — so
//! there is nothing that can drift silently and a digest would only be a second
//! thing to keep in step.

use std::os::raw::c_void;

/// Nonzero when a post from this thread would reach a loop.
///
/// Asked once, when a binding decides which transport a connection lives on.
/// False on a host where loop creation failed — the only case where the
/// binding must keep its legacy transport.
#[no_mangle]
pub extern "C" fn js_perry_agent_post_available() -> i32 {
    i32::from(super::available())
}

/// Hand `run(ctx)` to the loop of the agent this thread is acting for.
///
/// Returns `0` accepted, `1` accepted but the owner's wake failed (it runs on
/// the next turn), `-2` no loop exists for this agent, `-3` transient. **`>= 0`
/// means the runtime owns `ctx` and will invoke `run` exactly once; `< 0` means
/// `ctx` is untouched and still the caller's.**
///
/// # Safety
///
/// `ctx` must stay valid until `run` is invoked, and must be safe to use from
/// the agent's owning thread — a thread serving the same JS heap, so the
/// requirement is `Send`, not `Sync`. Passing a null `run` is refused rather
/// than called.
#[no_mangle]
pub unsafe extern "C" fn js_perry_agent_post(
    run: Option<extern "C" fn(*mut c_void)>,
    ctx: *mut c_void,
) -> i32 {
    let Some(run) = run else {
        // Nothing was taken, so the caller keeps its context: report the
        // "use your fallback" code rather than a success that never runs.
        return super::Posted::NoRoute.code();
    };
    // SAFETY: forwarded contract from this function's own safety note.
    unsafe { super::post(run, ctx) }.code()
}

/// How many posted jobs this thread has run.
///
/// The liveness counter a test asserts on: a post that went nowhere and a post
/// that ran are indistinguishable from the caller's side, so a "the owner
/// carried this" claim that does not watch this move cannot fail.
#[no_mangle]
pub extern "C" fn js_perry_agent_post_dispatched() -> u64 {
    super::dispatched()
}

/// The agent the calling thread acts for ([`crate::agent::current_agent`]):
/// its own if it is a `worker_threads` worker, otherwise the primary agent.
///
/// For a binding that keeps a queue of work produced by several agents' loops
/// and must hand each agent only its own (#11340): a socket event produced on
/// a worker's loop names objects on the worker's heap, so the primary thread
/// must never drain it.
#[no_mangle]
pub extern "C" fn js_perry_agent_current() -> u64 {
    crate::agent::current_agent()
}
