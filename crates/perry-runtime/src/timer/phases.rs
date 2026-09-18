//! turnloop P3: Node's event-loop phases, over the agent timer store.
//!
//! Node runs one loop iteration as
//! `timers → pending → poll → check → close`, draining the `nextTick` queue and
//! then the microtask queue after **every** callback in every phase. Perry ran
//! one iteration as `microtasks → (timeouts and immediates in one batch) →
//! nextTick → intervals → cron → all I/O pumps → park`
//! (`promise/microtasks.rs`), which is not that order. Two consequences were
//! visible from JS, both confirmed against Node 26.5.1:
//!
//! 1. **`setImmediate` ran before I/O, not after it.** Inside an I/O callback,
//!    Node always runs a `setImmediate` before a `setTimeout(…, 0)` — poll is
//!    followed by check in the same iteration, while the timeout waits for the
//!    next iteration's timers phase. Perry fired both from one pre-poll batch
//!    and so printed them the other way round.
//! 2. **An interval did not sort with timeouts.** Node's timers phase walks one
//!    expiry-ordered structure, so `setInterval(i, 3)` fires between
//!    `setTimeout(t1, 1)` and `setTimeout(t5, 5)`. Perry ran a whole callback
//!    queue and then a whole interval queue, printing `t1, t5, i`.
//!
//! This module implements the three phases Perry owns callbacks for:
//!
//! - [`run_timers_phase`] — promise timers, `setTimeout` and `setInterval`, in
//!   deadline order, one entry at a time;
//! - [`run_poll_callbacks`] — the native completion callbacks (`fs`, `dns`,
//!   `crypto`), which Node delivers in the poll phase, ahead of the same
//!   iteration's immediates;
//! - [`run_check_phase`] — `setImmediate`, FIFO.
//!
//! **One entry at a time, not a batch.** The old tick detached the whole expired
//! batch into a `Vec` before the first callback ran, which is why it needed
//! #8036's batch-wide rooting, and why `clearTimeout` of a sibling that was
//! already in the batch could not stop it — Node *does* stop it (measured). The
//! phase loop here pops one entry, runs it, and pops the next, so a cancel from
//! inside a callback removes its target from the heap before it is ever popped.
//!
//! **Both phases take a snapshot boundary** (`seq_horizon`): an entry scheduled
//! *by* a callback in this phase runs in the next iteration's phase, matching
//! Node's `processImmediate` and `uv__run_timers`. It is also what makes the
//! loops terminate: without it, a `setTimeout(f, 0)` scheduled from a timer
//! callback could be due at the phase's own clock read.

use std::time::Instant;

use super::store::{self, Class, Entry};
use super::{
    call_timer_callback_entry, enter_timer_callback_dispatch, in_timer_callback_dispatch,
    leave_timer_callback_dispatch, PROFILE_CALLBACK_TIMERS_FIRED, PROFILE_INTERVAL_TIMERS_FIRED,
    PROFILE_PROMISE_TIMERS_FIRED,
};

/// Node's **timers** phase: every entry whose deadline has passed, in deadline
/// order, with same-deadline entries in creation order.
///
/// Returns how many entries ran.
pub(crate) fn run_timers_phase() -> i32 {
    // First turn of the codegen event loop — `nodeTiming.loopStart` stops being
    // the "not started" sentinel here.
    crate::perf_hooks::note_event_loop_start();
    if in_timer_callback_dispatch() {
        return 0;
    }
    // libuv reads the clock once per iteration and runs the timers that are due
    // at *that* instant; a callback that takes 25 ms does not drag later timers
    // into the same phase. Perry's interval re-arm depends on the same read.
    let phase_now = Instant::now();
    let horizon = store::with_current(|timers| timers.seq_horizon());

    let mut fired = 0;
    let mut promise_fired = 0u64;
    let mut timeout_fired = 0u64;
    let mut interval_fired = 0u64;
    loop {
        let Some(entry) = store::with_current(|timers| {
            let mut entry = timers.pop_due(phase_now, horizon)?;
            // libuv re-arms a repeating timer BEFORE calling its callback
            // (`uv_timer_again` then `timer_cb`), which is what lets the
            // callback's own `clearInterval` cancel it. Re-arming afterwards
            // would resurrect an interval the callback had just cleared.
            if entry.class == Class::Interval {
                // `entry` is mutable so the #10447 pin can MOVE into the re-armed
                // copy: that copy is the live timer from here on.
                timers.rearm_interval(entry.duplicate_for_rearm(), phase_now);
            }
            Some(entry)
        }) else {
            break;
        };
        match entry.class {
            Class::Promise => {
                promise_fired += 1;
                resolve_promise_entry(entry);
            }
            Class::Timeout => {
                timeout_fired += 1;
                run_callback_entry(entry);
            }
            Class::Interval => {
                interval_fired += 1;
                run_callback_entry(entry);
            }
            // `pop_due` only ever returns timers-phase classes.
            Class::Immediate | Class::Pending => {
                unreachable!("a non-timer entry reached the timers heap")
            }
        }
        fired += 1;
    }

    if crate::promise::mt_profile_enabled() {
        use std::sync::atomic::Ordering;
        PROFILE_PROMISE_TIMERS_FIRED.fetch_add(promise_fired, Ordering::Relaxed);
        PROFILE_CALLBACK_TIMERS_FIRED.fetch_add(timeout_fired, Ordering::Relaxed);
        PROFILE_INTERVAL_TIMERS_FIRED.fetch_add(interval_fired, Ordering::Relaxed);
    }
    super::sync_loop_timer();
    fired
}

/// The callback half of Node's **poll** phase: the native completion callbacks
/// whose syscall Perry already performed.
///
/// Draining happens before the staged queue is promoted, which is what gives
/// an eagerly-completed operation the one turn of latency a real threadpool
/// round trip has (`store::insert_pending`). The phase runs BEFORE the check
/// phase, so an immediate queued by one of these callbacks runs in the same
/// iteration — measured on Node 26.5.1, stable 5/5, delta exactly one turn.
pub(crate) fn run_poll_callbacks() -> i32 {
    if in_timer_callback_dispatch() {
        return 0;
    }
    let mut fired = 0;
    loop {
        let Some(entry) = store::with_current(|timers| timers.pop_poll()) else {
            break;
        };
        run_callback_entry(entry);
        fired += 1;
    }
    store::with_current(|timers| timers.promote_pending());
    fired
}

/// Node's **check** phase: the `setImmediate` queue, in scheduling order,
/// bounded by the snapshot taken on entry.
///
/// Runs after the poll phase, which is the whole point: a `setImmediate`
/// scheduled inside an I/O callback runs in the same iteration, ahead of any
/// `setTimeout` that callback also scheduled.
pub(crate) fn run_check_phase() -> i32 {
    if in_timer_callback_dispatch() {
        return 0;
    }
    let horizon = store::with_current(|timers| timers.seq_horizon());
    let mut fired = 0;
    loop {
        let Some(entry) = store::with_current(|timers| timers.pop_check(horizon)) else {
            break;
        };
        run_callback_entry(entry);
        fired += 1;
    }
    // #input: dispatch buffered keyboard input on `process.stdin` at the same
    // safe event-loop point the callback phase always used. The reader thread
    // notifies on each keypress; this is a cheap empty-buffer check otherwise.
    crate::os::pump_process_stdin();
    if fired != 0 && crate::promise::mt_profile_enabled() {
        use std::sync::atomic::Ordering;
        PROFILE_CALLBACK_TIMERS_FIRED.fetch_add(fired as u64, Ordering::Relaxed);
    }
    super::sync_loop_timer();
    fired
}

/// Settle a promise timer. No user callback runs here, so there is no async
/// context to enter and no microtask checkpoint of its own: the settle queues a
/// reaction that the surrounding pump drains.
fn resolve_promise_entry(entry: Entry) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(entry.promise);
    let value_handle = scope.root_nanbox_f64(entry.value);
    promise_handle.with_mut_ptr::<crate::promise::Promise, _>(|promise| {
        crate::promise::js_promise_resolve(promise, value_handle.get_nanbox_f64());
    });
}

/// Run one timer/immediate callback the way Node runs a macrotask: enter its
/// captured `AsyncLocalStorage` context and async-hooks resource, install the
/// handle as `this`, call it under the uncaught-exception trap, then run the
/// `nextTick` + microtask checkpoint before the next entry is even popped
/// (#3870).
fn run_callback_entry(entry: Entry) {
    let Entry {
        id,
        class,
        callback,
        args,
        mut context,
        async_id,
        trigger_async_id,
        ..
    } = entry;

    // The entry left the store, so the collector no longer scans its slots:
    // everything the dispatch touches is rooted here for the whole dispatch.
    let scope = crate::gc::RuntimeHandleScope::new();
    let context_roots = crate::async_context::root_snapshot(&scope, &context);
    crate::async_context::refresh_snapshot_from_roots(&mut context, &context_roots);
    let previous = crate::async_context::enter_context(&context);
    let mut previous = previous;
    let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
    crate::async_hooks::before(async_id, trigger_async_id);

    enter_timer_callback_dispatch();
    call_timer_callback_entry(&scope, id, callback, &args);
    // Node runs a microtask checkpoint after EACH macrotask callback, so a
    // `queueMicrotask`/`Promise.then` queued inside this callback runs before
    // the next timer fires.
    crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    leave_timer_callback_dispatch();

    crate::async_hooks::after(async_id);
    // An interval keeps its resource alive across ticks; a one-shot timer's is
    // destroyed once it has run.
    if class != Class::Interval {
        crate::async_hooks::destroy(async_id);
    }
    crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    crate::async_context::restore_context(previous);
}
