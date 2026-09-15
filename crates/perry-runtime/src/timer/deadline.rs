//! turnloop P0: exact `Instant` timer deadlines for the primary agent's precise
//! park, and the legacy whole-millisecond C shape derived from them.

use super::{
    should_run_unref_callback_interval_timers, should_run_unref_promise_timers, CALLBACK_TIMERS,
    INTERVAL_TIMERS, TIMER_QUEUE,
};
use std::time::Instant;

/// turnloop P0: the earliest promise-timer deadline as an exact `Instant`
/// (same filter as `js_timer_next_deadline`, no millisecond truncation).
pub(crate) fn promise_timer_deadline() -> Option<Instant> {
    let allow_unref = should_run_unref_promise_timers();
    TIMER_QUEUE
        .lock()
        .unwrap()
        .iter()
        .filter(|t| (t.has_ref || allow_unref) && crate::agent::owns(t.owner))
        .map(|t| t.deadline)
        .min()
}

/// The legacy C deadline shape: whole milliseconds until `at` (0 when due), or
/// -1 when there is none. Truncation commutes with `min`, so this equals the
/// per-timer truncate-then-min it replaced. Kept for embedders and the legacy
/// park; the primary agent's precise park reads the `Instant` directly.
pub(super) fn whole_ms_until(at: Option<Instant>, now: Instant) -> f64 {
    match at {
        None => -1.0,
        Some(at) if at <= now => 0.0,
        Some(at) => (at - now).as_millis() as f64,
    }
}

/// turnloop P0: exact `Instant` form of `js_callback_timer_next_deadline`.
pub(crate) fn callback_timer_deadline() -> Option<Instant> {
    let allow_unref = should_run_unref_callback_interval_timers();
    CALLBACK_TIMERS
        .lock()
        .unwrap()
        .iter()
        .filter(|t| !t.cleared && crate::agent::owns(t.owner) && (t.refed || allow_unref))
        .map(|t| t.deadline)
        .min()
}

/// turnloop P0: exact `Instant` form of `js_interval_timer_next_deadline`.
pub(crate) fn interval_timer_deadline() -> Option<Instant> {
    let allow_unref = should_run_unref_callback_interval_timers();
    INTERVAL_TIMERS
        .lock()
        .unwrap()
        .iter()
        .filter(|t| !t.cleared && crate::agent::owns(t.owner) && (t.refed || allow_unref))
        .map(|t| t.next_deadline)
        .min()
}
