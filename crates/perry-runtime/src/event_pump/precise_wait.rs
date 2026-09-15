//! turnloop P0: the primary agent's park, on `Instant` deadlines.
//!
//! Replaces, for the primary agent only, the legacy tail of
//! `js_wait_for_event` that truncated every deadline to whole milliseconds
//! (`d as u64`), so a deadline 0.4 ms away read as "due now" and the loop
//! returned without waiting until it really was due — a spin that only the
//! #1114 throttle bounded. Here the deadline stays an `Instant` from the timer
//! queues to the OS wait: no truncation, no 1 ms floor.
//!
//! Wait selection (P0-transitional coexistence, deleted by P8):
//! - tokio-owned native work in flight (`native_inflight`, an O(1) predicate
//!   stdlib registers) → drive the legacy registered tick exactly as before
//!   (its whole-millisecond budget and 1 ms floor included);
//! - otherwise → one `Loop::turn(Timeout::Until(deadline))`.

use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use super::agent_loop;

/// stdlib's O(1) "tokio owns native work in flight" predicate.
static NATIVE_INFLIGHT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

pub(super) fn register_native_inflight(f: Option<extern "C" fn() -> i32>) {
    let ptr = f.map(|f| f as *mut ()).unwrap_or(std::ptr::null_mut());
    NATIVE_INFLIGHT.store(ptr, Ordering::Release);
}

#[inline]
pub(super) fn native_inflight() -> bool {
    let p = NATIVE_INFLIGHT.load(Ordering::Acquire);
    if p.is_null() {
        return false;
    }
    // SAFETY: the slot only ever holds an `extern "C" fn() -> i32` stored by
    // `js_register_native_inflight`; re-checked non-null right above.
    let f: extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
    f() != 0
}

/// The earliest wake across Perry's timer queues, the stdlib deadline
/// provider, the agent loop's own deadlines, and the idle cap.
pub(super) fn next_deadline(now: Instant) -> Instant {
    let mut deadline = now + Duration::from_millis(super::IDLE_CAP_MS);
    for at in [
        crate::timer::promise_timer_deadline(),
        crate::timer::callback_timer_deadline(),
        crate::timer::interval_timer_deadline(),
        agent_loop::loop_deadline(),
    ]
    .into_iter()
    .flatten()
    {
        deadline = deadline.min(at);
    }
    // The stdlib provider answers in fractional milliseconds relative to its
    // own clock read. Anchor it to a clock read taken AFTER it returned, so the
    // conversion can only land late (by nanoseconds), never early.
    let ms = crate::stdlib_pump::stdlib_next_wake_ms();
    if ms >= 0.0 {
        if let Ok(delay) = Duration::try_from_secs_f64(ms / 1000.0) {
            if let Some(at) = Instant::now().checked_add(delay) {
                deadline = deadline.min(at);
            }
        }
    }
    deadline
}

/// Park the primary agent. Returns `false` only when this thread could not
/// get a loop and nothing has happened yet, so the caller runs the legacy park.
pub(super) fn park() -> bool {
    let now = Instant::now();
    #[allow(unused_mut)]
    let mut deadline = next_deadline(now);
    #[cfg(test)]
    if super::TEST_FORCE_ZERO_BUDGET.load(Ordering::Acquire) {
        deadline = now;
    }
    if deadline <= now {
        // A deadline really is due: return to run it. With exact deadlines
        // this is no longer the sub-millisecond spin; see `zero_budget_return`
        // for why the #1114 throttle stays.
        super::zero_budget_return();
        return true;
    }
    if !agent_loop::ensure_loop() {
        return false;
    }
    let budget = deadline - now;
    // The idle-reclaim hook steps the collector in 4 ms slices. Below 1 ms
    // there is no room for a slice (and the legacy path never offered the hook
    // a zero budget either), so the loop parks straight to the deadline. The
    // verdict's remaining budget is always "the caller's deadline minus the
    // time the hook spent", which the absolute `deadline` already encodes.
    if budget >= Duration::from_millis(1) {
        if let crate::gc::ParkVerdict::Resume =
            crate::gc::idle_reclaim_park_hook(budget.as_millis() as u64)
        {
            return true;
        }
    }
    if native_inflight() {
        // P0-transitional: tokio still owns in-flight native work, and it only
        // advances inside its own tick. Drive that tick exactly as the legacy
        // driver did. P8 deletes this branch.
        let ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if super::wait_driver_sleep(ms) {
            if crate::promise::mt_profile_enabled() {
                super::PROFILE_WAIT_DRIVER_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            agent_loop::note_native_tick();
            super::spin_streak_reset();
            return true;
        }
    }
    match agent_loop::park_until(deadline) {
        agent_loop::Park::Waited => super::spin_streak_reset(),
        // A notify arrived after the fast path; like the fast path itself this
        // is not progress for the #1114 streak.
        agent_loop::Park::Notified => {}
        agent_loop::Park::Failed => {
            super::condvar_park(deadline.saturating_duration_since(Instant::now()));
        }
    }
    true
}
