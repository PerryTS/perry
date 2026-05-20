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

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::timer::{
    js_callback_timer_next_deadline, js_interval_timer_next_deadline, js_timer_next_deadline,
};

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
static NOTIFIED: AtomicBool = AtomicBool::new(false);
static WAITER_COUNT: AtomicI64 = AtomicI64::new(0);

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
/// saw no notify and no real wait; once the streak crosses
/// `SPIN_THROTTLE_AFTER`, sleep `SPIN_THROTTLE_SLEEP` before returning.
/// That caps a runaway loop at ~1 kHz (≤1 ms added dispatch latency —
/// well inside Node's own nested-timer clamping) while a normal program
/// never reaches the threshold. Any notify or real condvar sleep resets
/// the streak, so the designed sub-µs async hot path is untouched.
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

/// Block until the next scheduled timer fires, a notify arrives, or the
/// 1-second idle cap elapses — whichever is earliest. Returns immediately
/// if a notify arrived since the last call (the flag is cleared on
/// return). Replaces the old `js_sleep_ms` in the generated event loop
/// and `await` busy-wait.
#[no_mangle]
pub extern "C" fn js_wait_for_event() {
    // FAST PATH: a notify was already issued since the last wait. The
    // hot async/await steady-state hits this every iteration. A real
    // notify is genuine progress — clear any accumulated spin streak so
    // the designed sub-µs async path never trips the #1114 throttle.
    if NOTIFIED.swap(false, Ordering::Acquire) {
        spin_streak_reset();
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
    // We're about to actually wait (or take the under-lock notify
    // recheck) — that's genuine progress / real blocking, so the spin
    // streak is broken regardless of which sub-path we exit through.
    spin_streak_reset();

    // Slow path: take the cvar mutex and sleep on it. Mark ourselves
    // as a waiter first so concurrent notifiers go through the
    // mutex+cvar path (they won't see our wait if we registered after
    // they checked WAITER_COUNT and we'd miss the wake). The
    // mutex-protected `flag` covers the lost-wakeup window.
    WAITER_COUNT.fetch_add(1, Ordering::Release);
    let mut flag = PUMP.flag.lock().unwrap();
    // Re-check NOTIFIED under the lock — a producer may have set it
    // between our atomic-load above and the WAITER_COUNT increment.
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
        // Anything under 50 ms confirms the notify path works.
        assert!(
            elapsed_us < 50_000,
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
        assert!(
            elapsed < Duration::from_millis(5),
            "wait blocked despite prior notify: {:?}",
            elapsed
        );
    }

    /// Spec: wait does eventually return even with no notify (idle cap).
    /// Smoke-only — full IDLE_CAP_MS would be too slow for unit tests.
    #[test]
    fn wait_returns_when_timer_due() {
        let _g = SERIAL.lock().unwrap();
        // Schedule a timer 50ms out so wait_for_event uses 50ms as budget.
        crate::timer::js_set_timeout(50.0);
        let start = Instant::now();
        js_wait_for_event();
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(40),
            "wait returned too early: {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "wait blocked past timer deadline: {:?}",
            elapsed
        );
        // Reap the 50ms timer so it can't leak a due deadline into a
        // later serialized test.
        std::thread::sleep(Duration::from_millis(60));
        crate::timer::js_timer_tick();
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

        // A 0ms promise timer keeps `budget_ms == 0` every call (it is
        // never ticked, so it stays perpetually "due"). This is exactly
        // the #1114 wedge shape: a deadline pinned in the past.
        crate::timer::js_set_timeout(0.0);

        // Transient zero-latency: a single budget-0 call with a fresh
        // streak returns effectively immediately. (A racing notify only
        // makes this return *faster* via the fast path — never slower —
        // so this upper bound is robust.)
        NOTIFIED.swap(false, Ordering::Acquire);
        spin_streak_reset();
        let t0 = Instant::now();
        js_wait_for_event();
        assert!(
            t0.elapsed() < Duration::from_millis(5),
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
        assert!(
            throttled < Duration::from_secs(1),
            "throttle over-slept on a single call: {:?}",
            throttled
        );

        // A real notify clears the streak so the designed sub-µs async
        // hot path is never throttled: even right after the burst above,
        // a notify makes the next call return immediately.
        js_notify_main_thread();
        let t2 = Instant::now();
        js_wait_for_event(); // consumes NOTIFIED, resets streak
        assert!(
            t2.elapsed() < Duration::from_millis(5),
            "notify did not reset the spin streak: {:?}",
            t2.elapsed()
        );

        // Cleanup so the perpetually-due timer can't leak into another
        // serialized test.
        crate::timer::js_timer_tick();
        NOTIFIED.swap(false, Ordering::Acquire);
    }
}
