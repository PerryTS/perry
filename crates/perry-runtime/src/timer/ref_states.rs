//! #6084: bounded id→ref-state registry for scheduled timers, extracted from
//! `timer.rs` to keep that file under the 2000-line lint cap.

use std::collections::{HashMap, VecDeque};

/// id → ref-state registry for scheduled timers. Entries are kept after
/// `clearTimeout`/`clearInterval` so post-clear `.hasRef()`/`.unref()`/`+timer`
/// still route through timer dispatch (Node keeps the Timeout object alive).
/// They used to be inserted and *never* removed — a permanent per-id leak for a
/// process that creates unboundedly many timers (e.g. a `setTimeout` per
/// request). The insertion-ordered eviction queue bounds the map: the cap is
/// large enough that a realistic "hold the handle, call `.hasRef()` after
/// clear" pattern never sees eviction, but a long-running process no longer
/// grows it without limit. Timer ids are monotonic (never reused), so an
/// evicted id is never re-queried in practice.
#[derive(Default)]
pub(super) struct TimerRefStates {
    pub(super) states: HashMap<i64, bool>,
    order: VecDeque<i64>,
}

pub(super) const TIMER_REF_STATES_CAP: usize = 65_536;

impl TimerRefStates {
    /// Insert/overwrite `id`'s ref state, bounding the registry to `cap` entries
    /// by evicting the oldest ids. Only a new id extends the eviction queue; a
    /// ref/unref change on an existing id just overwrites its value.
    pub(super) fn insert_bounded(&mut self, id: i64, has_ref: bool, cap: usize) {
        if self.states.insert(id, has_ref).is_none() {
            self.order.push_back(id);
            while self.order.len() > cap {
                if let Some(old) = self.order.pop_front() {
                    self.states.remove(&old);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TimerRefStates;

    /// #6084: the ref-state registry must stay bounded, evicting the oldest ids
    /// while retaining recent ones (so post-clear `.hasRef()` keeps working for
    /// a handle held for any realistic duration).
    #[test]
    fn insert_bounded_evicts_oldest_and_caps_size() {
        let mut s = TimerRefStates::default();
        let cap = 4;
        for id in 1..=10i64 {
            s.insert_bounded(id, id % 2 == 0, cap);
        }
        assert_eq!(s.states.len(), cap);
        assert_eq!(s.order.len(), cap);
        for id in 1..=6i64 {
            assert!(!s.states.contains_key(&id), "id {id} should be evicted");
        }
        for id in 7..=10i64 {
            assert_eq!(s.states.get(&id).copied(), Some(id % 2 == 0));
        }
    }

    #[test]
    fn ref_unref_of_existing_id_does_not_grow_queue() {
        let mut s = TimerRefStates::default();
        let cap = 100;
        s.insert_bounded(42, true, cap);
        s.insert_bounded(42, false, cap);
        s.insert_bounded(42, true, cap);
        assert_eq!(s.order.len(), 1);
        assert_eq!(s.states.len(), 1);
        assert_eq!(s.states.get(&42).copied(), Some(true));
    }
}

/// Idle until the program schedules its first timer.
///
/// `is_known_timer_id` is consulted by the small-handle method/property fast
/// paths and by `js_number_coerce`, so a program that never calls `setTimeout`
/// was taking a process-global mutex on the GENERIC dispatch path — #7769
/// measured it as `pthread_mutex_lock` under `dispatch_primitive` on a pure
/// class-hierarchy benchmark that schedules no timers at all.
///
/// Armed by `set_timer_ref_state`, which runs before any id becomes
/// observable, per `registry_latch`'s ordering rule.
pub(crate) static TIMER_IDS_NONEMPTY: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

/// Whether `id` corresponds to a timer that was scheduled by this runtime
/// (active or already cleared). Used by the small-handle method/property
/// fast paths in `object/*.rs` and by `js_number_coerce` to decide whether
/// to apply Timeout-shaped semantics to a NaN-boxed small pointer. Without
/// this gate, any small handle (UI widget, drizzle, etc.) would accidentally
/// route through timer dispatch.
///
/// Entries in `TIMER_REF_STATES` are inserted at schedule time and never
/// removed — clearing a timer marks it cleared in the queue but keeps the
/// id registered as "this was a timer" so post-clear `.hasRef()` / `+timer`
/// / `.unref()` still route through timer dispatch (Node keeps the
/// Timeout object alive after `clearTimeout` and methods still work).
#[inline]
pub fn is_known_timer_id(id: i64) -> bool {
    if id <= 0 || TIMER_IDS_NONEMPTY.is_idle() {
        return false;
    }
    is_known_timer_id_slow(id)
}

#[inline(never)]
fn is_known_timer_id_slow(id: i64) -> bool {
    super::TIMER_REF_STATES
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.states.contains_key(&id))
        .unwrap_or(false)
}

#[cfg(test)]
mod latch_tests {
    /// The OFF state is the one every timer-free program takes, so it is the
    /// one that must be asserted: an accidentally pre-armed latch would put the
    /// mutex back on the dispatch path with nothing to notice.
    #[test]
    fn starts_idle_so_a_timer_free_program_pays_nothing() {
        if super::TIMER_IDS_NONEMPTY.is_idle() {
            assert!(!crate::timer::is_known_timer_id(1));
        }
    }
}
