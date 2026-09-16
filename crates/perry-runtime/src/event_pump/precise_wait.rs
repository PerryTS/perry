//! turnloop P0: an agent's park, on `Instant` deadlines.
//!
//! Replaces, for every agent that owns a loop (P0: only the primary; P9: any),
//! the legacy tail of
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

/// How long the transitional tokio tick may block while turnloop also has
/// outstanding work. One millisecond is the legacy loop's own floor, so a
/// program with both transports live is no coarser than Perry was before P0.
const MIXED_TRANSPORT_SLICE_MS: u64 = 1;

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

/// The earliest wake across this agent's timer heap, the stdlib deadline
/// provider, the agent loop's own deadlines, and the idle cap.
///
/// P9 note: the JS timer component is already per-agent (`timer::store` keys
/// its partitions on `AgentId`), but the stdlib provider is one process-wide
/// hook. On a worker agent it can therefore report the PRIMARY agent's next
/// deadline, which only ever shortens this park — a spurious early wake, never
/// a missed one.
///
/// P3: the JS timer component is now one heap root (`next_timer_deadline`)
/// instead of a scan of three queues, and the loop's own `next_deadline()` also
/// carries it once armed — the two agree by construction (a unit test asserts
/// it). Keeping Perry's own read as well is what covers the threads that have
/// no loop: a worker agent, and the pump thread acting for the primary agent on
/// Android.
pub(super) fn next_deadline(now: Instant) -> Instant {
    let mut deadline = now + Duration::from_millis(super::IDLE_CAP_MS);
    for at in [
        crate::timer::next_timer_deadline(),
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

/// Park this agent. Returns `false` only when this thread could not get a loop
/// and nothing has happened yet, so the caller runs the legacy park.
pub(super) fn park() -> bool {
    // Node computes a zero poll timeout whenever the immediate queue is
    // non-empty, so a `setImmediate` queued by a check callback — or a native
    // completion callback awaiting its poll phase — runs on the very next turn
    // rather than after a park. The generated loop branches past its park for
    // the same reason; this covers every other caller of `js_wait_for_event`.
    //
    // It routes through the shared zero-budget return rather than returning
    // bare, so the #1114 throttle still bounds a caller that spins on
    // `js_wait_for_event` without ever running the check phase that would drain
    // the queue. That is the same safety net a due timer takes.
    if crate::timer::js_immediate_has_pending() != 0 {
        super::zero_budget_return();
        return true;
    }
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
        //
        // P1 added a second transport, and with it the case P0 could not have:
        // tokio-owned work AND turnloop-owned sockets live at once (a
        // `net.connect` client, which stays on tokio so `upgradeToTLS` keeps
        // working, talking to a turnloop-backed server in the same process).
        // The tick blocks inside tokio and nothing there knows about turnloop's
        // poller, so a full-budget tick would never return to collect a socket
        // completion — not a delay but a hang, since the completion is what
        // would have produced the notify that ends the tick.
        //
        // While both are live the tick therefore takes a bounded slice and the
        // loop is turned right after it, so neither transport waits on the
        // other for more than that slice. It costs a wakeup per slice on a
        // program that is idle in both, which is the price of running two event
        // loops at once; P2-P7 remove the second one, and the proper bridge
        // before then is to register turnloop's `Integration::Fd` /
        // `Integration::Event` inside the tick so it ends on turnloop readiness
        // instead of on a timer.
        let loop_work = agent_loop::has_outstanding_work();
        let ms = if loop_work {
            MIXED_TRANSPORT_SLICE_MS
        } else {
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64
        };
        if super::wait_driver_sleep(ms) {
            if crate::promise::mt_profile_enabled() {
                super::PROFILE_WAIT_DRIVER_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            agent_loop::note_native_tick();
            if loop_work {
                agent_loop::fast_turn();
            }
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
