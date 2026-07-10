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
