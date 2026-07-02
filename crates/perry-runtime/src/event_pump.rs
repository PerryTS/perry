//! Main-thread event pump wakeup primitive (issue #84).
//!
//! Replaces the old hard `js_sleep_ms(10.0)` in the generated event loop
//! and the `js_sleep_ms(1.0)` busy-wait inside `await`. The main thread
//! blocks on a `Condvar` until either:
//!
//! - a cross-thread event source (tokio worker, `std::thread::spawn`)
//!   calls `js_notify_main_thread` after pushing into a queue that the
//!   pump drains, or
//! - the next timer / interval deadline elapses, or
//! - a 1-second safety cap elapses (heartbeat).
//!
//! Result: cross-thread async-op latency on the event loop drops from
//! ~5 ms average (half of the old 10 ms quantum) to single-digit
//! microseconds — limited only by `Condvar::wait_timeout` wake latency.
//!
//! Producer/consumer protocol:
//!   producer (any thread):  push_to_queue();  js_notify_main_thread();
//!   consumer (main thread): drain_queues();   js_wait_for_event();
//!
//! The flag is what makes a notify sent **before** the consumer enters
//! `wait_timeout` survive — if we used a bare `Condvar::wait_timeout`
//! without a flag we would lose any notify that races the lock acquire.

use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::timer::{
    js_callback_timer_next_deadline, js_interval_timer_next_deadline, js_timer_next_deadline,
};

// ============================================================================
// #1088 — Host-embedding wake callback.
//
// Hosts that drive the event loop themselves (Rust + winit, Qt, GTK4, …)
// sleep on OS primitives that don't observe Perry's internal `Condvar`. They
// register `(cb, ctx)` once via `perry_set_wake_callback`; `js_notify_main_thread`
// then invokes it on top of the existing condvar path so the host wakes
// instantly instead of polling. The callback runs on whatever thread called
// `js_notify_main_thread` (any tokio worker, any std::thread::spawn), so the
// host's implementation must be thread-safe — typical use is
// `EventLoopProxy::send_event(())` which is.
// ============================================================================

/// Host wake callback. Stored as raw pointers so the C FFI surface stays
/// trivially `unsafe extern "C"`. Either-or-both can be null; the
/// `cb` slot being null disables the wake.
static WAKE_CALLBACK: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WAKE_CONTEXT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Register a host wake callback. Passing `cb = NULL` clears the previous
/// registration. The `ctx` pointer is opaque to Perry — the host owns its
/// lifetime; we just hand it back on each invocation.
///
/// Thread-safety: the registration store is atomic; the callback itself is
/// invoked from `js_notify_main_thread`, which any producer thread may
/// call. Hosts must therefore use a thread-safe wake primitive (winit's
/// `EventLoopProxy`, a self-pipe, an eventfd, etc.).
///
/// # Safety
/// `cb` must remain callable for as long as it is registered. `ctx` must
/// remain valid for the same window. Pass `cb = NULL` before dropping
/// the target context to avoid use-after-free from a concurrent notify.
#[no_mangle]
pub unsafe extern "C" fn perry_set_wake_callback(
    cb: Option<unsafe extern "C" fn(*mut c_void)>,
    ctx: *mut c_void,
) {
    // Order matters: store the context first so any racing notifier that
    // observes the new cb pointer also sees a fresh ctx.
    WAKE_CONTEXT.store(ctx, Ordering::Release);
    let cb_ptr = cb.map(|f| f as *mut ()).unwrap_or(std::ptr::null_mut());
    WAKE_CALLBACK.store(cb_ptr, Ordering::Release);
}

#[inline]
fn invoke_host_wake_callback() {
    let cb_ptr = WAKE_CALLBACK.load(Ordering::Acquire);
    if cb_ptr.is_null() {
        return;
    }
    let ctx = WAKE_CONTEXT.load(Ordering::Acquire);
    // SAFETY: `cb` was registered by a host that guaranteed it remains
    // callable until cleared. We re-check non-null right above the call.
    unsafe {
        let cb: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(cb_ptr);
        cb(ctx);
    }
}

// ============================================================================
// #5779 — Idle-kick callback (tokio lost-unpark recovery).
//
// Perry runs JS on the main thread, which is NOT a tokio worker; it parks on
// the condvar below while the shared tokio runtime drives async I/O on its
// worker pool. A task scheduled from the main thread (the canonical case: an
// in-process HTTP server's per-request `oneshot::Sender::send` of the response,
// fired from `res.end()` inside the main-thread request pump) relies on tokio
// unparking a worker to run it. When *every* worker is parked and the I/O
// reactor is blocked in `kevent`/`epoll_wait`, that remote unpark can be lost
// under load — the woken task is enqueued but no worker is roused to drain it.
// The task is stranded forever: the response is never written, so the fetch
// client future never completes, and the main thread's 1-second idle re-check
// finds nothing to do (it already shipped the response) and re-parks. Permanent
// deadlock — exactly the in-process server-and-fetch hang in #5779.
//
// The fix: just before the main thread sleeps on its idle condvar, give it a
// chance to poke the runtime so a worker is roused to drain any
// enqueued-but-stranded task. perry-stdlib registers a callback here that
// schedules a task on the shared runtime whenever async client work is in
// flight. Because the kick fires right before the sleep, the drained task's own
// completion notify wakes the main thread immediately — so recovery latency is
// a worker hop, and even a fully missed wake self-heals within the 1s idle cap.
// (perry-stdlib's kick task does real work, not an empty `spawn(async {})`,
// which was measured not to reliably rouse a parked worker — see its
// `stdlib_idle_kick`.)
//
// Registration is opt-in and the slot is a single atomic load when unset, so
// embedders that never register pay nothing. The callback runs on the main
// thread only (from `js_wait_for_event`'s pre-sleep path, never the hot
// NOTIFIED fast path).
// ============================================================================

/// Idle-kick callback, stored as a raw fn pointer for a trivial C FFI surface.
/// Null disables the kick.
static IDLE_KICK_CALLBACK: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Register the idle-kick callback (see module-level #5779 note). Passing
/// `cb = NULL` clears it. Invoked on the main thread immediately before it
/// blocks on the idle condvar; the implementation must be cheap and must not
/// re-enter `js_wait_for_event`.
#[no_mangle]
pub extern "C" fn js_register_idle_kick(cb: Option<extern "C" fn()>) {
    let cb_ptr = cb.map(|f| f as *mut ()).unwrap_or(std::ptr::null_mut());
    IDLE_KICK_CALLBACK.store(cb_ptr, Ordering::Release);
}

#[inline]
fn invoke_idle_kick() {
    let cb_ptr = IDLE_KICK_CALLBACK.load(Ordering::Acquire);
    if cb_ptr.is_null() {
        return;
    }
    // SAFETY: the slot only ever holds a `extern "C" fn()` installed by
    // `js_register_idle_kick`; re-checked non-null right above.
    let cb: extern "C" fn() = unsafe { std::mem::transmute(cb_ptr) };
    cb();
}

struct Pump {
    /// `true` iff a producer notified since the last consumer reset.
    flag: Mutex<bool>,
    cvar: Condvar,
}

static PUMP: Pump = Pump {
    flag: Mutex::new(false),
    cvar: Condvar::new(),
};

/// Lock-free fast-path flag for `js_notify_main_thread`.
///
/// The hot path is a single-threaded async benchmark with millions of
/// promise resolutions per second — every one of which used to take
/// the `PUMP.flag` mutex (a syscall on contention, an atomic CAS even
/// uncontended). Profile of `benchmarks/app-patterns/kernels/promise_all_chains.ts`
/// showed ~5% of total runtime in `<std::sync::Mutex as MutexGuard>::new` /
/// `parking_lot_core::deadlock::*`.
///
/// New protocol:
///   - `WAITER_COUNT` is incremented by the consumer just before entering
///     `cvar.wait_timeout` and decremented immediately after.
///   - `js_notify_main_thread` does a relaxed-load of `WAITER_COUNT`. If
///     it's zero (the consumer is busy draining queues, not waiting)
///     just store-true to `NOTIFIED` and return — no mutex, no syscall.
///   - When `WAITER_COUNT > 0`, fall through to the mutex+cvar path so
///     `notify_one` actually wakes the sleeping thread.
///
/// `js_wait_for_event` reads `NOTIFIED` first; if true, it consumes it
/// and returns immediately. Otherwise it takes the mutex + cvar path.
///
/// **#1114 nuance**: the NOTIFIED fast-path is **not** treated as "real
/// progress" for the spin-throttle below — every `js_promise_resolve`,
/// `js_async_step_chain`, and net/ws/http event push calls
/// `js_notify_main_thread`, so a hot async tick that does any internal
/// promise work flips NOTIFIED on every iteration. Counting those as
/// progress would mean the streak counter can never accumulate, and the
/// throttle becomes a no-op exactly when it's needed. So the fast-path
/// leaves the streak untouched (neither increments nor resets it); only
/// an actual `cvar.wait_timeout` sleep counts as progress.
static NOTIFIED: AtomicBool = AtomicBool::new(false);
static WAITER_COUNT: AtomicI64 = AtomicI64::new(0);
#[cfg(test)]
static TEST_FORCE_ZERO_BUDGET: AtomicBool = AtomicBool::new(false);

/// Idle-cap: even if every notify path were silent, the consumer
/// re-checks every second. Acts as a safety net only — the design
/// target is 0 unmatched notifies on the hot path.
const IDLE_CAP_MS: u64 = 1000;

/// #1114: adaptive spin-throttle.
///
/// The generated event loop (and the inline `await` poll loop) call
/// `js_wait_for_event` every iteration. The condvar fast paths
/// (`NOTIFIED`, or a real `wait_timeout` sleep) bound CPU to near-zero
/// in the common case. But there is a third exit — `budget_ms == 0`
/// ("a timer reads as due now") — that returns *immediately without
/// sleeping*. If anything keeps a timer/interval deadline pinned in the
/// past, or a tokio source re-arms a 0 ms-budget condition every
/// iteration, the loop spins at ~100 % CPU forever. That starves the
/// fastify request pump (it only gets one slice per loop iteration but
/// the loop never yields the core), so every HTTP route times out even
/// though TCP still accepts — exactly the #1114 wedge signature, with
/// GC `madvise` hot from the per-iteration allocation churn.
///
/// Transient `budget_ms == 0` is legitimate and must stay zero-latency
/// (a real 0 ms `setTimeout`/`setImmediate`, or a just-due timer the
/// loop body reaps within an iteration or two). So we only throttle a
/// *sustained* spin: count consecutive immediate budget-0 returns that
/// were not separated by a real condvar sleep; once the streak crosses
/// `SPIN_THROTTLE_AFTER`, sleep `SPIN_THROTTLE_SLEEP` before returning.
/// That caps a runaway loop at ~1 kHz (≤1 ms added dispatch latency —
/// well inside Node's own nested-timer clamping) while a normal program
/// never reaches the threshold. A real `cvar.wait_timeout` sleep resets
/// the streak; the NOTIFIED-fast-path return does **not** (see comment
/// on `NOTIFIED`), because hot async work flips NOTIFIED every
/// iteration and would otherwise mask a true wedge.
///
/// Escape hatch: `PERRY_SPIN_THROTTLE=0` (or `off`/`false`) restores the
/// old pure-spin behaviour for bisection, mirroring `PERRY_GEN_GC` etc.
const SPIN_THROTTLE_AFTER: u64 = 1024;
const SPIN_THROTTLE_SLEEP: Duration = Duration::from_millis(1);

fn spin_throttle_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_SPIN_THROTTLE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

thread_local! {
    /// Consecutive `budget_ms == 0` immediate returns with no intervening
    /// notify / real wait. Reset to 0 on any genuine progress.
    static SPIN_STREAK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[inline]
fn spin_streak_reset() {
    SPIN_STREAK.with(|s| s.set(0));
}

/// Wake the main thread from `js_wait_for_event` (or a future call).
///
/// Safe to call from any thread, including the main thread itself.
/// Multiple notifies between consumer waits collapse to one wake — the
/// consumer drains the entire queue each pass anyway.
#[no_mangle]
pub extern "C" fn js_notify_main_thread() {
    // Mark notification visible to the consumer regardless of which
    // path it took (Release so subsequent producer side-effects are
    // visible).
    NOTIFIED.store(true, Ordering::Release);
    // #1088 — fan the wake out to the host-registered callback (if any)
    // BEFORE the WAITER_COUNT fast-path return. The host may be sleeping
    // on an OS primitive (winit's `EventLoopProxy`, an eventfd, …) that
    // Perry's condvar doesn't observe; without this hook a hot tokio
    // worker pushing fetch results would only wake the host on the next
    // OS-event tick. Registration is opt-in — `invoke_host_wake_callback`
    // is a single atomic-load when no host is listening, so callers that
    // never register pay essentially nothing.
    invoke_host_wake_callback();
    // Hot path: no consumer is currently in `cvar.wait_timeout`, so
    // we don't need to take the mutex or signal the cvar — the next
    // call to `js_wait_for_event` will see `NOTIFIED == true` on the
    // atomic-load fast path and return immediately. This skips a
    // mutex acquire+release per call (= ~10 ns saved on uncontended
    // x86, more under load), which for 200k microtasks/await dominates
    // the per-await fixed cost.
    if WAITER_COUNT.load(Ordering::Acquire) == 0 {
        return;
    }
    // Slow path: a consumer is sleeping in `cvar.wait_timeout`. Take
    // the mutex to publish the flag under the lock (the cvar protocol
    // requires this), then signal. The mutex is contended only for the
    // brief duration the consumer holds it — uncontended in steady
    // state.
    let mut flag = PUMP.flag.lock().unwrap();
    *flag = true;
    drop(flag);
    PUMP.cvar.notify_one();
}

// ============================================================================
// #1088 — Unified Event Loop FFI facade for host embedding.
//
// The internal pump surface (`js_promise_run_microtasks`, `js_run_stdlib_pump`,
// `js_microtasks_pending`, `js_*_timer_next_deadline`, the various
// `js_*_has_active_handles` shims) is correct but easy to mis-wire from a
// host: forgetting `js_run_stdlib_pump` silently hangs `fetch`; relying only
// on `js_microtasks_pending` to gate sleep ignores timers and stdlib I/O.
//
// The three functions below collapse the foot-guns into one obvious surface:
//
//   * `perry_poll()`           — drains microtasks + stdlib
//   * `perry_has_work()`       — true while anything is pending (microtasks,
//                                timers across all 3 queues, stdlib handles)
//   * `perry_next_wake_ms()`   — minimum across the 3 timer queues, or -1
//
// Pair with `perry_set_wake_callback` for polling-free integration.
// ============================================================================

extern "C" {
    fn js_promise_run_microtasks() -> i32;
    fn js_run_stdlib_pump();
    fn js_microtasks_pending() -> i32;
    fn js_stdlib_has_active_handles() -> i32;
}

/// Drain everything Perry is currently holding ready: microtask queue and
/// the stdlib pump (fetch / fs / ws / fastify / timers). Returns the number
/// of microtasks executed by `js_promise_run_microtasks`. The stdlib pump
/// doesn't report task counts, so the return value is a lower-bound proxy
/// for "did anything observable happen this tick".
///
/// Safe to call from the host's event-loop tick. Idempotent at zero cost
/// when there's no work — the stdlib pump trampoline bails immediately
/// when nothing is registered.
#[no_mangle]
pub extern "C" fn perry_poll() -> i32 {
    // SAFETY: every call site below is a Perry C FFI surface declared with
    // `extern "C"` linkage and stable across host builds; no thread-safety
    // invariants beyond what each individual function already documents.
    unsafe {
        let microtasks = js_promise_run_microtasks();
        js_run_stdlib_pump();
        microtasks
    }
}

/// Returns 1 if the host should keep the event loop alive — anything
/// pending across all of Perry's internal queues. Use as the gate for
/// `ControlFlow::Wait` vs `ControlFlow::Poll` in winit, or the equivalent
/// in other event-loop frameworks.
///
/// Checks (any positive answer ⇒ has work):
///   * `js_microtasks_pending()`           — promise microtasks
///   * any of the 3 timer queues has a deadline ≥ 0
///   * `js_stdlib_has_active_handles()`    — fetch / ws / fastify / timers
#[no_mangle]
pub extern "C" fn perry_has_work() -> i32 {
    // SAFETY: same trampoline surface as `perry_poll`.
    let pending_microtasks = unsafe { js_microtasks_pending() };
    if pending_microtasks > 0 {
        return 1;
    }
    let has_timer = js_timer_next_deadline() >= 0.0
        || js_callback_timer_next_deadline() >= 0.0
        || js_interval_timer_next_deadline() >= 0.0;
    if has_timer {
        return 1;
    }
    if unsafe { js_stdlib_has_active_handles() } != 0 {
        return 1;
    }
    0
}

/// Returns the closest pending wake-up across all 3 timer queues, in
/// milliseconds from now. Returns -1.0 when no timers are scheduled —
/// the host can then sleep indefinitely (or until an OS event / a wake
/// callback fires).
///
/// NaN is *not* returned — keeps the return shape printable and avoids
/// surprising hosts that compare with `<`.
#[no_mangle]
pub extern "C" fn perry_next_wake_ms() -> f64 {
    let mut best: f64 = -1.0;
    for d in [
        js_timer_next_deadline(),
        js_callback_timer_next_deadline(),
        js_interval_timer_next_deadline(),
    ] {
        if d < 0.0 {
            continue;
        }
        if best < 0.0 || d < best {
            best = d;
        }
    }
    best
}

/// Block until the next scheduled timer fires, a notify arrives, or the
/// 1-second idle cap elapses — whichever is earliest. Returns immediately
/// if a notify arrived since the last call (the flag is cleared on
/// return). Replaces the old `js_sleep_ms` in the generated event loop
/// and `await` busy-wait.
#[no_mangle]
pub extern "C" fn js_wait_for_event() {
    // FAST PATH: a notify was already issued since the last wait. The
    // hot async/await steady-state hits this every iteration.
    //
    // #1114: we deliberately do **not** reset the spin streak here.
    // `js_notify_main_thread` is called from inside every promise
    // resolution and every async-step chain, so a tight JobLoop tick
    // that does any internal async work flips NOTIFIED on essentially
    // every iteration of the event loop — resetting the streak here
    // means the throttle can never accumulate enough consecutive
    // budget==0 returns to fire, and the wedge it's meant to catch
    // (timer deadline pinned in the past + hot notifies) silently
    // pegs a core. Only `cvar.wait_timeout` actually sleeping counts
    // as "progress" for streak-reset purposes.
    if NOTIFIED.swap(false, Ordering::Acquire) {
        return;
    }

    let mut budget_ms: u64 = IDLE_CAP_MS;
    for d in [
        js_timer_next_deadline(),
        js_callback_timer_next_deadline(),
        js_interval_timer_next_deadline(),
    ] {
        if d >= 0.0 {
            let d_ms = d as u64;
            if d_ms < budget_ms {
                budget_ms = d_ms;
            }
        }
    }
    #[cfg(test)]
    if TEST_FORCE_ZERO_BUDGET.load(Ordering::Acquire) {
        budget_ms = 0;
    }

    if budget_ms == 0 {
        // A timer reads as due now — don't block. Transient hits stay
        // zero-latency; only a *sustained* budget-0 spin (the #1114
        // wedge) gets throttled so it can't peg a core and starve the
        // request pump. See `SPIN_THROTTLE_AFTER`.
        if spin_throttle_enabled() {
            let streak = SPIN_STREAK.with(|s| {
                let n = s.get().saturating_add(1);
                s.set(n);
                n
            });
            if streak > SPIN_THROTTLE_AFTER {
                std::thread::sleep(SPIN_THROTTLE_SLEEP);
            }
        }
        return;
    }
    // #5779: we are about to sleep on the idle condvar. If async client work is
    // in flight, kick the tokio runtime so a worker is roused to drain any task
    // that was scheduled but whose worker-unpark was lost while every worker was
    // parked. Done *before* registering as a waiter / sleeping so the kicked
    // task's completion notify still wakes us via the NOTIFIED re-check below or
    // the cvar — no lost wake. The callback no-ops cheaply when nothing is in
    // flight (and is absent until perry-stdlib registers it).
    invoke_idle_kick();
    // Slow path: take the cvar mutex and sleep on it. Mark ourselves
    // as a waiter first so concurrent notifiers go through the
    // mutex+cvar path (they won't see our wait if we registered after
    // they checked WAITER_COUNT and we'd miss the wake). The
    // mutex-protected `flag` covers the lost-wakeup window.
    WAITER_COUNT.fetch_add(1, Ordering::Release);
    let mut flag = PUMP.flag.lock().unwrap();
    // Re-check NOTIFIED under the lock — a producer may have set it
    // between our atomic-load above and the WAITER_COUNT increment.
    // This is equivalent to the fast-path return at the top of the
    // function (just under the mutex), so — like the fast path — it
    // does **not** reset the spin streak. #1114.
    if NOTIFIED.swap(false, Ordering::Acquire) || *flag {
        *flag = false;
        WAITER_COUNT.fetch_sub(1, Ordering::Release);
        return;
    }
    let (mut new_flag, _) = PUMP
        .cvar
        .wait_timeout(flag, Duration::from_millis(budget_ms))
        .unwrap();
    *new_flag = false;
    WAITER_COUNT.fetch_sub(1, Ordering::Release);
    NOTIFIED.store(false, Ordering::Release);
    // We actually slept on the cvar (even if the timeout was short or a
    // spurious wakeup fired) — that's the one path that yielded the
    // core, so it's the only one allowed to reset the streak.
    spin_streak_reset();
}

/// Exit like Node does when top-level module evaluation is still pending but
/// the event loop has no refed work left to drive it.
#[no_mangle]
pub extern "C" fn js_unsettled_top_level_await_exit() {
    const MESSAGE: &[u8] = b"Warning: Detected unsettled top-level await\n";

    #[cfg(unix)]
    unsafe {
        libc::write(
            libc::STDERR_FILENO,
            MESSAGE.as_ptr() as *const _,
            MESSAGE.len(),
        );
        libc::_exit(13);
    }

    #[cfg(windows)]
    {
        eprint!("{}", std::str::from_utf8(MESSAGE).unwrap_or(""));
        extern "system" {
            fn ExitProcess(uExitCode: u32);
        }
        unsafe {
            ExitProcess(13);
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        eprint!("{}", std::str::from_utf8(MESSAGE).unwrap_or(""));
        std::process::exit(13);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::thread;
    use std::time::Instant;

    /// Serializes tests that mutate the global timer queues so a
    /// transiently-due timer from one can't change another's wait
    /// budget. (`js_wait_for_event`'s budget is computed from global
    /// timer state — there is no per-thread injection point.)
    static SERIAL: StdMutex<()> = StdMutex::new(());

    struct ForcedZeroBudgetGuard;

    impl ForcedZeroBudgetGuard {
        fn new() -> Self {
            TEST_FORCE_ZERO_BUDGET.store(true, Ordering::Release);
            Self
        }
    }

    impl Drop for ForcedZeroBudgetGuard {
        fn drop(&mut self) {
            TEST_FORCE_ZERO_BUDGET.store(false, Ordering::Release);
        }
    }

    /// Spec: wait returns within microseconds of a notify, well below the
    /// idle cap (1 s).
    #[test]
    fn notify_wakes_within_5ms() {
        let _g = SERIAL.lock().unwrap();
        // Consume any prior pending notify so this test starts clean.
        js_wait_for_event();

        let woken_at = Arc::new(AtomicU64::new(0));
        let woken_at_clone = woken_at.clone();
        let consumer = thread::spawn(move || {
            let start = Instant::now();
            js_wait_for_event();
            woken_at_clone.store(start.elapsed().as_micros() as u64, Ordering::Relaxed);
        });

        // Give consumer time to enter wait_timeout.
        thread::sleep(Duration::from_millis(10));
        js_notify_main_thread();
        consumer.join().unwrap();

        let elapsed_us = woken_at.load(Ordering::Relaxed);
        // Consumer slept ~10 ms before notify, then woke up. Total elapsed
        // since consumer start should be ~10 ms + tiny wake latency.
        // #1444: a broken notify path blocks until the 1 s idle cap, so any
        // sub-cap return confirms the notify works. The old 50 ms bound
        // measured wake *latency* and false-failed when an overloaded runner
        // oversleeps the 10 ms producer sleep or delays the consumer wake;
        // 500 ms is robust and still an order of magnitude under the 1 s
        // lost-notify floor.
        assert!(
            elapsed_us < 500_000,
            "wake took {} us — notify path broken",
            elapsed_us
        );
    }

    /// Spec: a notify sent BEFORE the consumer waits is not lost.
    #[test]
    fn notify_before_wait_is_preserved() {
        let _g = SERIAL.lock().unwrap();
        // Drain.
        js_wait_for_event();

        js_notify_main_thread();
        let start = Instant::now();
        js_wait_for_event(); // should return immediately
        let elapsed = start.elapsed();
        // #1444: a preserved notify returns essentially instantly; a *lost*
        // notify (the bug) blocks for the whole `IDLE_CAP_MS` (1 s) budget
        // since no timer is queued. The original `< 5 ms` bound asserted
        // wake *latency*, not the spec, and false-failed on overloaded CI
        // runners where scheduler preemption between `Instant::now()` and the
        // return alone exceeds 5 ms. Assert well under the 1 s lost-notify
        // floor instead — `IDLE_CAP_MS / 2` keeps a 500 ms margin in both
        // directions and stays deterministic under load.
        assert!(
            elapsed < Duration::from_millis(IDLE_CAP_MS / 2),
            "wait blocked despite prior notify: {:?}",
            elapsed
        );
    }

    /// Spec: wait does eventually return even with no notify (idle cap).
    /// Smoke-only — full IDLE_CAP_MS would be too slow for unit tests.
    ///
    /// `js_wait_for_event`'s budget is derived from the **process-global**
    /// timer queue, and it returns early when the **process-global**
    /// `NOTIFIED` flag is set. The `SERIAL` lock only serializes the other
    /// event_pump tests — it does not stop a parallel (non-event_pump) test
    /// from calling `js_notify_main_thread` or scheduling a sooner timer
    /// mid-wait, either of which wakes our wait before the 50ms budget and
    /// made this assertion flaky under load. So we don't assert a single
    /// timed wait blocks ~50ms; instead we re-arm and re-measure until we
    /// observe one clean, uninterrupted window (the common case is the first
    /// attempt). Each attempt clears the stale notify, drains any past-due
    /// timers polluting the budget, and reaps its own timer afterward.
    #[test]
    fn wait_returns_when_timer_due() {
        let _g = SERIAL.lock().unwrap();
        let mut last_elapsed = Duration::ZERO;
        let mut blocked_for_timer = false;
        for _ in 0..40 {
            // Drain any already-expired timer (left by a parallel test or a
            // prior attempt) so it can't pin the budget at 0, and consume any
            // stale notify so the wait blocks on our timer rather than a
            // leftover flag.
            crate::timer::js_timer_tick();
            NOTIFIED.store(false, Ordering::Release);
            // Schedule a timer 50ms out so wait_for_event uses 50ms as budget.
            crate::timer::js_set_timeout(50.0);
            // js_set_timeout / the drain above can flip NOTIFIED via promise
            // resolution; clear it once more immediately before the wait.
            NOTIFIED.store(false, Ordering::Release);
            let start = Instant::now();
            js_wait_for_event();
            last_elapsed = start.elapsed();
            // Reap our 50ms timer so it can't leak a due deadline into the
            // next attempt or a later serialized test.
            std::thread::sleep(Duration::from_millis(60));
            crate::timer::js_timer_tick();
            // #1444: the lower bound (≥40ms) is the real spec — the wait
            // *blocked* for the ~50ms timer budget rather than spinning or
            // returning instantly. The upper bound only guards against a wait
            // that never returns; the old 500ms cap false-failed on
            // overloaded runners where a 50ms `wait_timeout` oversleeps well
            // past 500ms (the 1016s-vs-451s slow-runner signature in #1444),
            // so every attempt landed "too late" and the retry loop exhausted.
            // 5s tolerates that oversleep while still catching a truly stuck
            // wait.
            if (Duration::from_millis(40)..Duration::from_secs(5)).contains(&last_elapsed) {
                blocked_for_timer = true;
                break;
            }
            // Woken early (concurrent notify / sooner parallel timer) or
            // absurdly late (>5s stall) — retry for a clean window.
        }
        assert!(
            blocked_for_timer,
            "wait never blocked for the ~50ms timer budget across retries; last: {:?}",
            last_elapsed
        );
    }

    /// #1114 spec: a *transient* budget-0 return stays zero-latency, but
    /// a *sustained* budget-0 spin is throttled so it can't peg a core.
    ///
    /// `NOTIFIED` is process-global, so any parallel test calling
    /// `js_notify_main_thread` resets this thread's streak. We can't
    /// prevent that across test binaries, so the throttle check is a
    /// retry-until-clean *single-call* measurement: a working 1 ms
    /// throttle yields ≥1 attempt with a ≥700 µs budget-0 return; a
    /// broken (or disabled) throttle can NEVER produce one, regardless
    /// of resets. That makes it deterministic, not flaky.
    #[test]
    fn sustained_budget_zero_spin_is_throttled() {
        let _g = SERIAL.lock().unwrap();
        assert!(
            spin_throttle_enabled(),
            "throttle must be on by default for this test"
        );

        // Force the event-pump budget to zero without depending on the
        // process-global timer queue. Other runtime tests may clear that
        // queue in parallel, turning this warm-up into 1,025 idle-cap waits.
        let _budget = ForcedZeroBudgetGuard::new();

        // Transient zero-latency: a single budget-0 call with a fresh
        // streak returns effectively immediately. (A racing notify only
        // makes this return *faster* via the fast path — never slower —
        // so this upper bound is robust.)
        NOTIFIED.swap(false, Ordering::Acquire);
        spin_streak_reset();
        let t0 = Instant::now();
        js_wait_for_event();
        // #1444: a fresh streak does not throttle, so this returns without the
        // throttle's ~1ms sleep. The bound only needs to sit below the sleep's
        // order of magnitude scaled for an overloaded runner — 5ms preemption
        // alone tripped it under CI load; 200ms is robust and still catches an
        // erroneously-throttled transient call.
        assert!(
            t0.elapsed() < Duration::from_millis(200),
            "transient budget-0 must stay zero-latency, took {:?}",
            t0.elapsed()
        );

        // Sustained spin is throttled: push past the threshold, then
        // measure ONE call. If a parallel notify reset the streak mid
        // warm-up the measured call is cheap — retry. A genuinely
        // working throttle produces a ≥700 µs call within a few clean
        // attempts; a broken one never does.
        let mut throttled = Duration::ZERO;
        for _ in 0..8 {
            NOTIFIED.swap(false, Ordering::Acquire);
            spin_streak_reset();
            for _ in 0..=SPIN_THROTTLE_AFTER {
                js_wait_for_event();
            }
            let t = Instant::now();
            js_wait_for_event();
            let d = t.elapsed();
            if d > throttled {
                throttled = d;
            }
            if throttled >= Duration::from_micros(700) {
                break;
            }
        }
        assert!(
            throttled >= Duration::from_micros(700),
            "sustained budget-0 spin not throttled: best post-threshold \
             call was {:?} (a working 1ms throttle yields ≥700µs)",
            throttled
        );
        // #1444: guards against a throttle that sleeps grossly too long (the
        // configured delay is ~1ms). The old 1s cap could false-fail when an
        // overloaded runner adds hundreds of ms of scheduler latency on top of
        // the 1ms sleep; 5s still catches a seconds-scale over-sleep bug.
        assert!(
            throttled < Duration::from_secs(5),
            "throttle over-slept on a single call: {:?}",
            throttled
        );

        // A pending notify still returns immediately via the fast path
        // — the sub-µs async hot path is preserved when there's actual
        // work to drain — but the streak intentionally persists across
        // it so an interleaved notify-then-budget==0 wedge can't mask
        // itself (see `notified_interleave_does_not_mask_wedge` below).
        js_notify_main_thread();
        let t2 = Instant::now();
        js_wait_for_event(); // consumes NOTIFIED, returns immediately
                             // #1444: 5ms was scheduler-preemption-tight under CI load; 200ms is
                             // robust and still well below any budget-blocking behavior.
        assert!(
            t2.elapsed() < Duration::from_millis(200),
            "notify fast-path was not zero-latency: {:?}",
            t2.elapsed()
        );

        NOTIFIED.swap(false, Ordering::Acquire);
    }

    /// #1114 regression: the JobLoop-shape wedge interleaves
    /// `js_notify_main_thread` (from promise resolutions / async-step
    /// chains during a busy tick) with `budget_ms == 0` returns (from
    /// a timer/interval deadline that doesn't advance). The original
    /// throttle reset the streak on every notify fast-path hit, so the
    /// budget-0 counter could never accumulate to the threshold and
    /// the throttle was structurally bypassed — CPU pegged at 99 % and
    /// every HTTP route timed out.
    ///
    /// This test alternates notify + budget-0 calls past the threshold
    /// and asserts that the throttle still fires. With the bug, no
    /// single post-threshold call ever takes more than a few µs (the
    /// notify path keeps resetting). With the fix, at least one of the
    /// budget-0 calls after the warm-up sleeps for the throttle delay.
    #[test]
    fn notified_interleave_does_not_mask_wedge() {
        let _g = SERIAL.lock().unwrap();
        assert!(
            spin_throttle_enabled(),
            "throttle must be on by default for this test"
        );

        // Force the same budget-0 shape as a perpetually-due timer while
        // staying isolated from parallel tests that mutate the timer queue.
        let _budget = ForcedZeroBudgetGuard::new();

        let mut throttled = Duration::ZERO;
        // Retry loop guards against a parallel test pushing a notify
        // through in the gap between our notify and our wait — a
        // working throttle yields a ≥700 µs call within a few attempts,
        // a broken one never does (the streak never accumulates).
        for _ in 0..8 {
            NOTIFIED.swap(false, Ordering::Acquire);
            spin_streak_reset();
            // Warm-up: alternate notify and wait past the threshold.
            // Under the original code each notify reset the streak so
            // the threshold was never crossed; under the fix the
            // budget-0 streak accumulates uninterrupted.
            for _ in 0..=SPIN_THROTTLE_AFTER {
                js_notify_main_thread();
                js_wait_for_event(); // notify fast-path
                js_wait_for_event(); // budget==0 path
            }
            // One more notify+wait pair, then a measured budget-0 wait.
            js_notify_main_thread();
            js_wait_for_event();
            let t = Instant::now();
            js_wait_for_event(); // measured budget==0 wait
            let d = t.elapsed();
            if d > throttled {
                throttled = d;
            }
            if throttled >= Duration::from_micros(700) {
                break;
            }
        }

        assert!(
            throttled >= Duration::from_micros(700),
            "notify-interleaved budget-0 spin was not throttled: best \
             post-threshold call {:?} — the throttle is bypassed by \
             the notify fast-path, exactly the #1114 wedge",
            throttled
        );

        NOTIFIED.swap(false, Ordering::Acquire);
    }

    static KICK_COUNT: AtomicU64 = AtomicU64::new(0);

    /// Idle-kick test stub: count invocations and immediately notify so the
    /// pending `wait_timeout` shortcuts instead of sleeping the full idle cap
    /// — mirrors the real callback's effect (a drained task's completion notify
    /// wakes the main thread), keeping the test fast and deterministic.
    extern "C" fn counting_kick() {
        KICK_COUNT.fetch_add(1, Ordering::Release);
        js_notify_main_thread();
    }

    /// #5779: the idle kick must run on the pre-sleep slow path so a parked
    /// main thread can rouse a stranded tokio worker before blocking. Forcing
    /// a real (budget > 0) wait with no prior notify must invoke the callback.
    #[test]
    fn idle_kick_fires_before_blocking_sleep() {
        let _g = SERIAL.lock().unwrap();
        // Drain any stale notify so the first attempt below can reach the
        // slow path rather than returning via the fast path.
        js_wait_for_event();
        js_register_idle_kick(Some(counting_kick));

        // Retry to stay robust against a parallel (non-event_pump) test that
        // sets NOTIFIED or schedules a sooner timer — either of which would
        // divert this call off the slow path. A working wiring fires on the
        // first clean attempt; a broken one never does.
        let mut fired = false;
        for _ in 0..50 {
            crate::timer::js_timer_tick();
            NOTIFIED.store(false, Ordering::Release);
            KICK_COUNT.store(0, Ordering::Release);
            js_wait_for_event();
            if KICK_COUNT.load(Ordering::Acquire) >= 1 {
                fired = true;
                break;
            }
        }

        js_register_idle_kick(None);
        NOTIFIED.swap(false, Ordering::Acquire);
        assert!(
            fired,
            "idle kick was never invoked on the pre-sleep path — a stranded \
             tokio worker would never be roused (the #5779 deadlock)"
        );
    }

    /// #5779: the kick must NOT run on the NOTIFIED fast path — there is work
    /// to drain, the main thread is not about to block, and kicking the
    /// runtime on every hot async tick would be pure overhead.
    #[test]
    fn idle_kick_skipped_on_notified_fast_path() {
        let _g = SERIAL.lock().unwrap();
        js_wait_for_event(); // drain
        js_register_idle_kick(Some(counting_kick));

        // A pre-set NOTIFIED makes the next wait return via the fast path. A
        // racing parallel notify could only *add* a fast-path return, never
        // force the slow path here, so observing at least one clean fast-path
        // return with no kick is deterministic across retries.
        let mut clean_fast_path = false;
        for _ in 0..50 {
            KICK_COUNT.store(0, Ordering::Release);
            js_notify_main_thread();
            js_wait_for_event();
            if KICK_COUNT.load(Ordering::Acquire) == 0 {
                clean_fast_path = true;
                break;
            }
        }

        js_register_idle_kick(None);
        NOTIFIED.swap(false, Ordering::Acquire);
        assert!(
            clean_fast_path,
            "idle kick fired on the NOTIFIED fast path — it must only run \
             when the main thread is about to block"
        );
    }
}
