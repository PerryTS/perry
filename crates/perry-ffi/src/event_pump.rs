//! Event-pump surface — wakes the main thread when a tokio worker
//! (or other background thread) has produced an event that user
//! callbacks need to fire on.
//!
//! # Why
//!
//! Wrappers that expose `.on('event', cb)` semantics — `ws`, `http`,
//! `net`, `fastify` request/reply, `events.EventEmitter` (already
//! ported but in-process so no notify needed) — produce events on
//! tokio worker threads but must invoke user callbacks on the main
//! thread (perry-runtime's GC, NaN-boxing tag visibility, and
//! single-threaded JS semantics all assume main-thread callback
//! execution).
//!
//! The flow:
//!
//! ```text
//! tokio worker:        main thread:
//!   receive event        wait_for_event() →
//!   push onto queue          (woken by notify)
//!   notify_main_thread() → process_pending() drains queue
//!                          → JsClosure::call0/1/N for each listener
//! ```
//!
//! Wrappers manage their own per-module pending-events queue
//! (`Mutex<Vec<MyEvent>>` typical). They expose a
//! `js_<name>_process_pending() -> i32` extern that perry-codegen's
//! event-loop pump calls every tick to drain. The queue's producer
//! side calls `notify_main_thread()` after pushing so the main loop
//! wakes promptly instead of waiting on the 1-second heartbeat cap.
//!
//! # Today's surface (v0.5.x)
//!
//! Just `notify_main_thread`. The dispatch — `js_<name>_process_pending`
//! / `js_<name>_has_pending` — is wrapper-specific and doesn't need a
//! perry-ffi entry; codegen knows about each wrapper's tick symbols by
//! name, the same way it does for perry-stdlib's pumps (cron, http, ws,
//! readline, …).

extern "C" {
    /// Wake the main thread from `js_wait_for_event`.
    ///
    /// Safe to call from any thread, including the main thread
    /// itself. Multiple notifies between consumer waits collapse to
    /// one wake — the main-loop tick drains every queue each pass
    /// regardless.
    fn js_notify_main_thread();
}

/// Wake the main thread so it picks up a pending event the calling
/// (worker) thread just pushed onto its wrapper's queue.
///
/// ```ignore
/// // Inside a tokio worker that received a websocket message:
/// MY_PENDING_EVENTS.lock().unwrap().push(event);
/// perry_ffi::notify_main_thread();
/// ```
pub fn notify_main_thread() {
    // SAFETY: the runtime entry is `extern "C"` and takes no args;
    // it can be called from any thread.
    unsafe { js_notify_main_thread() };
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;

    #[test]
    fn notify_main_thread_does_not_panic() {
        // The pump is initialized lazily by perry-runtime; calling
        // notify before any consumer has waited is a no-op success.
        // We just want to confirm the extern symbol is wired.
        notify_main_thread();
    }
}
