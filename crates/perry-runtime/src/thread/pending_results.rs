//! Cross-thread promise result queue and owner-agent settlement.

use super::*;

/// Queue a thread's result for resolution on the main thread.
///
/// Uses the stdlib's PENDING_DEFERRED mechanism. The converter function
/// runs on the main thread during `js_stdlib_process_pending()`, which
/// deserializes the value into the main thread's arena.
pub(super) fn queue_thread_result(
    owner: crate::agent::AgentId,
    promise_usize: usize,
    result: SerializedValue,
) {
    queue_thread_result_with_mode(owner, promise_usize, result, false);
}

fn queue_thread_result_with_mode(
    owner: crate::agent::AgentId,
    promise_usize: usize,
    result: SerializedValue,
    is_rejection: bool,
) {
    // We need to interact with perry-stdlib's deferred resolution queue.
    // Since perry-runtime cannot depend on perry-stdlib, we use the same
    // pattern as timer resolution: store the result and let the pump pick it up.
    //
    // Thread results are stored in a global Mutex queue. The main thread's
    // pump function (js_thread_process_pending) drains this queue and resolves
    // the promises.
    {
        let mut pending = match PENDING_THREAD_RESULTS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.push(PendingThreadResult {
            owner,
            promise_ptr: promise_usize,
            result,
            is_rejection,
        });
        PENDING_THREAD_RESULTS_LEN.store(pending.len(), Ordering::SeqCst);
    }
    ACTIVE_THREAD_JOBS.fetch_sub(1, Ordering::SeqCst);
    // Issue #84: wake the main thread so spawn()-returned promises
    // resolve as soon as the OS thread finishes, not at the next
    // event-loop quantum.
    crate::event_pump::js_notify_main_thread();
}

/// Register the start of a background job that will later resolve a promise on
/// the main thread via [`queue_promise_string_result`]. Keeps the event loop
/// alive until the result arrives (mirrors `spawn`'s job accounting). Used by
/// `Atomics.waitAsync` (#4913).
pub fn thread_job_begin() {
    ACTIVE_THREAD_JOBS.fetch_add(1, Ordering::SeqCst);
}

/// Resolve the promise at `promise_usize` with a UTF-8 string on the agent that
/// owns it. Routes through the same pending-result path `spawn` uses (which
/// unpins the promise, deserializes the value into that agent's arena,
/// decrements the active-job count, and wakes the event loop). Used by
/// `Atomics.waitAsync`.
///
/// `owner` must be captured on the thread that CREATED the promise (#6185). The
/// futex-waiter thread that calls this never runs JS and owns no heap, so it
/// cannot derive the right agent from itself.
pub fn queue_promise_string_result(
    owner: crate::agent::AgentId,
    promise_usize: usize,
    value: &str,
) {
    queue_thread_result(
        owner,
        promise_usize,
        SerializedValue::String(value.as_bytes().to_vec()),
    );
}

/// Reject a pinned cross-thread promise with a UTF-8 message on its owning
/// agent. This is the error-side companion to
/// [`queue_promise_string_result`], used by native async framework bridges
/// whose completion may arrive on an arbitrary OS thread (#5536).
pub fn queue_promise_string_rejection(
    owner: crate::agent::AgentId,
    promise_usize: usize,
    message: &str,
) {
    queue_thread_result_with_mode(
        owner,
        promise_usize,
        SerializedValue::String(message.as_bytes().to_vec()),
        true,
    );
}

/// A pending thread result waiting to be resolved on the agent that spawned it.
struct PendingThreadResult {
    /// #6185: the agent whose heap `promise_ptr` lives in — captured at spawn
    /// time from the *spawning* thread, not the worker. Only that agent may
    /// drain this entry; a worker pumping the global queue would otherwise
    /// resolve a foreign-heap promise with a pointer into its own arena, which
    /// is unmapped when it exits.
    owner: crate::agent::AgentId,
    promise_ptr: usize,
    result: SerializedValue,
    /// Settle through `reject` rather than `resolve` after deserialization.
    is_rejection: bool,
}

// Safety: SerializedValue is Send, usize is Send. `promise_ptr` is a raw
// pointer into `owner`'s arena; the `owner` tag plus the owner-filtered drain
// in `js_thread_process_pending` is what makes dereferencing it sound.
unsafe impl Send for PendingThreadResult {}

/// Global queue for pending thread results.
static PENDING_THREAD_RESULTS: std::sync::Mutex<Vec<PendingThreadResult>> =
    std::sync::Mutex::new(Vec::new());
/// turnloop P0: `PENDING_THREAD_RESULTS.len()`, republished under its lock
/// after every mutation. `js_thread_has_pending` runs on every event-loop
/// turn; an empty queue (the steady state) now answers without the lock.
static PENDING_THREAD_RESULTS_LEN: AtomicUsize = AtomicUsize::new(0);

/// Process pending thread results. Called from the main thread's event loop
/// (registered as a pump function, similar to js_stdlib_process_pending).
///
/// Drains the queue, deserializes each result into the main thread's arena,
/// and resolves or rejects the corresponding Promise.
///
/// # Returns
/// Number of results processed.
#[no_mangle]
pub extern "C" fn js_thread_process_pending() -> i32 {
    // #6185: take only the entries THIS agent owns. Every remaining entry names
    // a promise in another agent's arena; draining it here would resolve a
    // foreign-heap promise with a value deserialized into our arena (and, once
    // that agent exits, dereference freed memory). Leave them for their owner —
    // `retire_agent` purges any whose owner dies first.
    let mine: Vec<PendingThreadResult> = {
        let mut pending = match PENDING_THREAD_RESULTS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Order-preserving partition: results must settle in the order they
        // were queued (a `swap_remove` filter would reorder them).
        let (mine, theirs): (Vec<_>, Vec<_>) = std::mem::take(&mut *pending)
            .into_iter()
            .partition(|item| crate::agent::owns(item.owner));
        *pending = theirs;
        PENDING_THREAD_RESULTS_LEN.store(pending.len(), Ordering::SeqCst);
        mine
    };
    let count = mine.len() as i32;

    // The lock is released before we settle anything: `js_promise_resolve` runs
    // user `.then` callbacks, which can call `spawn` and re-enter
    // `queue_thread_result` (deadlock on a re-entrant lock of the same Mutex).
    for item in mine {
        unsafe {
            // #9552: the address crossed the thread boundary as a bare usize;
            // verify it still names a promise. The constructor's pin is
            // released by the settlement below, not here.
            let promise = crate::promise::native_promise_from_raw(
                item.promise_ptr,
                "perry/thread result drain",
            );

            // #6185: a worker that returned a non-transferable value (e.g.
            // `spawn(() => new Map())`) can't throw on its own thread (no
            // setjmp frame). The marker rode back in the serialized result;
            // reject the returned promise here on the main thread with a named
            // TypeError so `await`/`.catch` observes it instead of `undefined`.
            if let Some(name) = first_unsupported_transfer_type(&item.result) {
                let reason = make_unsupported_transfer_error(name);
                crate::promise::js_promise_reject(promise, reason);
                continue;
            }

            // Deserialize the result into the owning agent's arena and settle.
            let result_bits = deserialize_nanbox_on_current_thread(&item.result);
            if item.is_rejection {
                crate::promise::js_promise_reject(promise, f64::from_bits(result_bits));
            } else {
                crate::promise::js_promise_resolve(promise, f64::from_bits(result_bits));
            }
        }
    }

    count
}

/// Check if there are any pending thread results.
/// Used by the event loop to know whether to keep spinning.
#[no_mangle]
pub extern "C" fn js_thread_has_pending() -> i32 {
    if ACTIVE_THREAD_JOBS.load(Ordering::SeqCst) != 0 {
        return 1;
    }
    if PENDING_THREAD_RESULTS_LEN.load(Ordering::SeqCst) == 0 {
        return 0;
    }
    // #6185: only entries THIS agent can actually settle count as work keeping
    // its loop alive. Reporting a foreign entry here would spin the event loop
    // forever on a result the drain (correctly) refuses to touch.
    let pending = match PENDING_THREAD_RESULTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    i32::from(pending.iter().any(|item| crate::agent::owns(item.owner)))
}

/// Drop every queued result owned by `agent`. Called from
/// `agent::retire_agent` when a worker thread exits: those entries name
/// promises in an arena that is being unmapped, so no thread can ever settle
/// them, and leaving them would keep `js_thread_has_pending` honest but the
/// pointers dangling.
pub(crate) fn purge_agent_thread_results(agent: crate::agent::AgentId) {
    let mut pending = match PENDING_THREAD_RESULTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    pending.retain(|item| item.owner != agent);
    PENDING_THREAD_RESULTS_LEN.store(pending.len(), Ordering::SeqCst);
}
