//! turnloop P4 for wasm: there is no worker pool, so every job runs inline on
//! the calling thread and is delivered immediately — the same fallback the
//! native `submit_or_run_inline*` takes when the pool refuses a job. Only the
//! surface the stdlib's inline-capable callers use exists here; the
//! submit/cancel/turn entry points of the native module (long jobs, FFI
//! async work) are deliberately absent.

/// The outcome handed to `deliver`. Only `Done` is ever produced inline;
/// the other variants keep callers' `match` arms identical to native.
#[derive(Debug)]
pub enum Delivery<T> {
    Done(T),
    Cancelled,
    Failed(std::io::Error),
}

/// Run `work` inline and deliver its result. Always returns `false`
/// ("not accepted by a pool"), as the native refusal path does.
pub fn submit_or_run_inline<T, W, D>(work: W, deliver: D) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Delivery<T>) + 'static,
{
    deliver(Delivery::Done(work()));
    false
}

/// [`submit_or_run_inline`] with NaN-boxed roots handed back unchanged.
pub fn submit_or_run_inline_rooted<T, W, D>(roots: Vec<u64>, work: W, deliver: D) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(Delivery<T>, Vec<u64>) + 'static,
{
    deliver(Delivery::Done(work()), roots);
    false
}

/// No jobs are ever outstanding: everything completes inline.
pub fn has_pending_jobs() -> bool {
    false
}
