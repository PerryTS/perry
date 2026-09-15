//! turnloop P0: O(1) timer keep-alive for the primary agent.
//!
//! The generated event loop asks "does a ref'd timer keep this agent alive?"
//! for all three queues on every turn — twice in its liveness disjunction, and
//! again from every tick and park-deadline computation through
//! `should_run_unref_*`. Each answer used to walk its whole queue under the
//! queue lock, and the callback/interval walks took the ref-state registry
//! lock once per entry.
//!
//! Each queue now carries a count of the entries that keep the PRIMARY agent
//! alive, maintained under the queue lock at every insert, removal and ref
//! change, so the primary agent's answer is one atomic load. Other agents
//! (`perry/thread` workers, which get their own loop in P3/P4) keep the exact
//! scan; the counter is only ever consulted for the primary agent.
//!
//! Invariant, per queue: `primary_live == #{entries e : e.keeps_primary_alive()}`
//! where `keeps_primary_alive` is `owner == PRIMARY_AGENT && has_ref` for
//! promise timers and `owner == PRIMARY_AGENT && !cleared && refed` for
//! callback and interval timers. Every mutation below is paired: an add for
//! each insert, a remove for each removal, a signed step for each ref change;
//! underflow is a debug assertion, and debug builds re-derive the count on each
//! O(1) read so a missed site fails loudly in `cargo test`.

use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{CallbackTimer, IntervalTimer, Timer};
use crate::agent::PRIMARY_AGENT;

/// A timer queue plus its primary-agent keep-alive count. Derefs to the queue's
/// mutex, so every `QUEUE.lock()` call site reads as before.
pub(super) struct TimerQueue<T> {
    entries: Mutex<Vec<T>>,
    primary_live: AtomicUsize,
}

impl<T> Deref for TimerQueue<T> {
    type Target = Mutex<Vec<T>>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

/// Whether one queue entry keeps the primary agent's event loop alive.
pub(super) trait KeepsPrimaryAlive {
    fn keeps_primary_alive(&self) -> bool;
}

impl KeepsPrimaryAlive for Timer {
    fn keeps_primary_alive(&self) -> bool {
        self.owner == PRIMARY_AGENT && self.has_ref
    }
}

impl KeepsPrimaryAlive for CallbackTimer {
    fn keeps_primary_alive(&self) -> bool {
        self.owner == PRIMARY_AGENT && !self.cleared && self.refed
    }
}

impl KeepsPrimaryAlive for IntervalTimer {
    fn keeps_primary_alive(&self) -> bool {
        self.owner == PRIMARY_AGENT && !self.cleared && self.refed
    }
}

impl<T: KeepsPrimaryAlive> TimerQueue<T> {
    pub(super) const fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            primary_live: AtomicUsize::new(0),
        }
    }

    /// Record that `entry` joined the queue. Call with the queue lock held.
    #[inline]
    pub(super) fn note_added(&self, entry: &T) {
        if entry.keeps_primary_alive() {
            self.primary_live.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Record that `entry` left the queue. Call with the queue lock held.
    #[inline]
    pub(super) fn note_removed(&self, entry: &T) {
        if entry.keeps_primary_alive() {
            let previous = self.primary_live.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "timer keep-alive count underflow");
        }
    }

    /// Record a change of one entry's liveness inputs (ref state, `cleared`).
    /// `before` is the entry's `keeps_primary_alive()` prior to the change.
    #[inline]
    pub(super) fn note_changed(&self, before: bool, entry: &T) {
        match (before, entry.keeps_primary_alive()) {
            (false, true) => {
                self.primary_live.fetch_add(1, Ordering::AcqRel);
            }
            (true, false) => {
                let previous = self.primary_live.fetch_sub(1, Ordering::AcqRel);
                debug_assert!(previous > 0, "timer keep-alive count underflow");
            }
            _ => {}
        }
    }

    /// Push an entry, counting it.
    pub(super) fn push_counted(&self, queue: &mut Vec<T>, entry: T) {
        self.note_added(&entry);
        queue.push(entry);
    }

    /// `Vec::retain` that counts every entry it removes.
    pub(super) fn retain_counted(&self, queue: &mut Vec<T>, mut keep: impl FnMut(&T) -> bool) {
        queue.retain(|entry| {
            let kept = keep(entry);
            if !kept {
                self.note_removed(entry);
            }
            kept
        });
    }

    /// Does any entry keep the CURRENT agent alive? O(1) for the primary agent.
    pub(super) fn has_live_for_current_agent(&self, scan: impl Fn(&T) -> bool) -> bool {
        if crate::agent::current_agent() != PRIMARY_AGENT {
            return self.entries.lock().unwrap().iter().any(scan);
        }
        #[cfg(debug_assertions)]
        self.debug_verify();
        self.primary_live.load(Ordering::Acquire) != 0
    }

    /// Re-derive the count from the queue (tests).
    #[cfg(test)]
    pub(super) fn recount(&self) -> usize {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.keeps_primary_alive())
            .count()
    }

    #[cfg(debug_assertions)]
    fn debug_verify(&self) {
        let queue = self.entries.lock().unwrap();
        let expected = queue.iter().filter(|e| e.keeps_primary_alive()).count();
        let counted = self.primary_live.load(Ordering::Acquire);
        drop(queue);
        debug_assert_eq!(
            counted, expected,
            "timer keep-alive count drifted from its queue (a mutation site is unpaired)"
        );
    }

    /// Test seeding that bypasses the counted mutators: re-derive the count.
    #[cfg(test)]
    pub(super) fn resync_for_test(&self) {
        let expected = self.recount();
        self.primary_live.store(expected, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn primary_live_for_test(&self) -> usize {
        self.primary_live.load(Ordering::Acquire)
    }
}

/// Apply a ref/unref to the queued callback or interval timer with `id`, if it
/// is still queued. Ids are monotonic and a ref change usually follows the
/// schedule closely, so the search runs from the newest entry.
pub(super) fn apply_ref_state_to_queues(id: i64, has_ref: bool) {
    {
        let mut timers = super::CALLBACK_TIMERS.lock().unwrap();
        if let Some(timer) = timers.iter_mut().rev().find(|t| t.id == id) {
            let before = timer.keeps_primary_alive();
            timer.refed = has_ref;
            super::CALLBACK_TIMERS.note_changed(before, timer);
            return;
        }
    }
    let mut intervals = super::INTERVAL_TIMERS.lock().unwrap();
    if let Some(timer) = intervals.iter_mut().rev().find(|t| t.id == id) {
        let before = timer.keeps_primary_alive();
        timer.refed = has_ref;
        super::INTERVAL_TIMERS.note_changed(before, timer);
    }
}

#[cfg(test)]
#[path = "liveness_tests.rs"]
mod tests;
