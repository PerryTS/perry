//! #6084 / #10447: bounded id→handle-state registry for scheduled timers (ref
//! state and `Timeout`/`Immediate` kind), extracted from `timer.rs` to keep that
//! file under the 2000-line lint cap.

use super::CallbackTimerKind;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// What the registry knows about one timer id.
#[derive(Clone, Copy)]
struct TimerHandleState {
    has_ref: bool,
    /// `Timeout`/`Immediate` for `.constructor`; `None` for an id that only
    /// ever reached `ref()`/`unref()`.
    kind: Option<CallbackTimerKind>,
    /// Still has a queue entry: not fired, not cleared. Never evicted.
    scheduled: bool,
}

/// id → handle-state registry for timers. Entries are kept after the timer is
/// cleared or fires so post-clear `.hasRef()`/`.unref()`/`+timer`/`.constructor`
/// still route through timer dispatch (Node keeps the Timeout object alive).
/// They used to be inserted and *never* removed — a permanent per-id leak for a
/// process that creates unboundedly many timers (e.g. a `setTimeout` per
/// request, #6084). The bound that fixed it evicted the oldest ids whether or
/// not they were still scheduled, so 65,536 later timers silently undid a live
/// long-delay timer's `unref()` (a missing id reads as ref'd, and the process
/// stayed alive until the timer fired) and dropped it from `is_known_timer_id`
/// (#10447). Only RETIRED ids — fired or cleared, queued in `retired` — are
/// eviction candidates now; a scheduled id is pinned by its queue entry's
/// [`ScheduledTimerId`]. The map is bounded by live timers + `cap`.
#[derive(Default)]
pub(super) struct TimerRefStates {
    states: HashMap<i64, TimerHandleState>,
    /// Retired ids, oldest first — the only eviction candidates.
    retired: VecDeque<i64>,
}

pub(super) const TIMER_REF_STATES_CAP: usize = 65_536;

impl TimerRefStates {
    /// A newly scheduled timer: ref'd, pinned until [`Self::retire`].
    fn schedule(&mut self, id: i64, kind: CallbackTimerKind) {
        let state = TimerHandleState {
            has_ref: true,
            kind: Some(kind),
            scheduled: true,
        };
        self.states.insert(id, state);
    }

    /// `ref()`/`unref()`. An id the registry does not hold (never scheduled, or
    /// retired and since evicted) is recorded as retired, so it stays bounded.
    fn set_ref(&mut self, id: i64, has_ref: bool, cap: usize) {
        if let Some(state) = self.states.get_mut(&id) {
            state.has_ref = has_ref;
            return;
        }
        let state = TimerHandleState {
            has_ref,
            kind: None,
            scheduled: false,
        };
        self.states.insert(id, state);
        self.push_retired(id, cap);
    }

    /// The timer's queue entry is gone: keep its state for post-clear dispatch,
    /// but make it evictable. Idempotent, and a no-op for an unknown id.
    fn retire(&mut self, id: i64, cap: usize) {
        if let Some(state) = self.states.get_mut(&id) {
            if state.scheduled {
                state.scheduled = false;
                self.push_retired(id, cap);
            }
        }
    }

    fn push_retired(&mut self, id: i64, cap: usize) {
        self.retired.push_back(id);
        while self.retired.len() > cap {
            let Some(old) = self.retired.pop_front() else {
                break;
            };
            // Timer ids are monotonic, so a retired id is not rescheduled in
            // practice; the check still never lets eviction drop a live entry.
            if self.states.get(&old).is_some_and(|state| !state.scheduled) {
                self.states.remove(&old);
            }
        }
    }

    fn get(&self, id: i64) -> Option<TimerHandleState> {
        self.states.get(&id).copied()
    }
}

static TIMER_REF_STATES: Mutex<Option<TimerRefStates>> = Mutex::new(None);

/// Poison-tolerant: [`ScheduledTimerId`]'s drop takes this lock during unwinds.
fn lock_states() -> MutexGuard<'static, Option<TimerRefStates>> {
    TIMER_REF_STATES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A scheduled timer's pin on its registry entry. It lives IN the queue entry
/// (`CallbackTimer`, `IntervalTimer`, the mock-timer entries), so every path
/// that removes one — firing, `clearTimeout`/`clearInterval`/`clearImmediate`,
/// `purge_agent_timers`, a mock-timers reset — retires the id by dropping it,
/// and no removal site can forget to. Deliberately not `Clone`: a dropped copy
/// would retire a timer that is still queued.
///
/// Dropping takes the registry lock, so never drop a timer entry while holding
/// it (the only nesting is queue lock → registry lock).
pub(super) struct ScheduledTimerId(i64);

impl Drop for ScheduledTimerId {
    fn drop(&mut self) {
        if let Some(states) = lock_states().as_mut() {
            states.retire(self.0, TIMER_REF_STATES_CAP);
        }
    }
}

#[cfg(test)]
impl ScheduledTimerId {
    /// For test scaffolding entries whose ids were never registered.
    pub(super) fn unregistered() -> Self {
        Self(i64::MIN)
    }
}

/// Register a timer id as scheduled (ref'd, with its handle kind). Runs before
/// the id is observable — the async_hooks `init` hook already sees the handle.
pub(super) fn register_scheduled_timer(id: i64, kind: CallbackTimerKind) -> ScheduledTimerId {
    TIMER_IDS_NONEMPTY.arm();
    lock_states()
        .get_or_insert_with(TimerRefStates::default)
        .schedule(id, kind);
    ScheduledTimerId(id)
}

pub(super) fn set_timer_ref_state(id: i64, has_ref: bool) {
    TIMER_IDS_NONEMPTY.arm();
    lock_states()
        .get_or_insert_with(TimerRefStates::default)
        .set_ref(id, has_ref, TIMER_REF_STATES_CAP);
}

fn timer_handle_state(id: i64) -> Option<TimerHandleState> {
    lock_states().as_ref().and_then(|states| states.get(id))
}

/// Node's `hasRef()` default is `true`, so an id the registry does not hold
/// reads as ref'd. A scheduled timer's id is always held (#10447).
pub(super) fn timer_has_ref_state(id: i64) -> bool {
    timer_handle_state(id).map_or(true, |state| state.has_ref)
}

pub(super) fn timer_handle_kind(id: i64) -> Option<CallbackTimerKind> {
    timer_handle_state(id).and_then(|state| state.kind)
}

/// Read-only view for a whole-queue liveness scan, under ONE registry lock
/// instead of one per entry.
pub(super) struct RefStatesView<'a>(Option<&'a TimerRefStates>);

impl RefStatesView<'_> {
    pub(super) fn has_ref(&self, id: i64) -> bool {
        self.0
            .and_then(|states| states.get(id))
            .map_or(true, |state| state.has_ref)
    }
}

/// `f` must not drop a timer entry (see [`ScheduledTimerId`]).
pub(super) fn with_ref_states<R>(f: impl FnOnce(&RefStatesView<'_>) -> R) -> R {
    let guard = lock_states();
    f(&RefStatesView(guard.as_ref()))
}

/// `PERRY_GC_CENSUS` row for the registry.
pub(super) fn ref_states_census() -> crate::gc::census::SideTableRow {
    let guard = lock_states();
    let (len, bytes) = guard.as_ref().map_or((0, 0), |states| {
        let deque = states.retired.capacity() * std::mem::size_of::<i64>();
        let map = crate::gc::census::map_bytes(&states.states);
        (states.states.len(), map + deque)
    });
    ("timer.ref_states", len, bytes)
}

#[cfg(test)]
pub(crate) fn test_ref_state_counts() -> (usize, usize) {
    lock_states().as_ref().map_or((0, 0), |states| {
        let scheduled = states.states.values().filter(|s| s.scheduled).count();
        (states.states.len(), scheduled)
    })
}

#[cfg(test)]
mod tests {
    use super::{CallbackTimerKind, TimerRefStates, TIMER_REF_STATES_CAP};

    fn scheduled_then_retired(
        s: &mut TimerRefStates,
        ids: std::ops::RangeInclusive<i64>,
        cap: usize,
    ) {
        for id in ids {
            s.schedule(id, CallbackTimerKind::Timeout);
            s.retire(id, cap);
        }
    }

    /// #6084: retired ids stay bounded, evicting the oldest while retaining
    /// recent ones (so post-clear `.hasRef()` keeps working for a handle held
    /// for any realistic duration).
    #[test]
    fn retired_ids_evict_oldest_and_cap_size() {
        let mut s = TimerRefStates::default();
        let cap = 4;
        for id in 1..=10i64 {
            s.schedule(id, CallbackTimerKind::Timeout);
            s.set_ref(id, id % 2 == 0, cap);
            s.retire(id, cap);
        }
        assert_eq!(s.states.len(), cap);
        assert_eq!(s.retired.len(), cap);
        for id in 1..=6i64 {
            assert!(s.get(id).is_none(), "id {id} should be evicted");
        }
        for id in 7..=10i64 {
            assert_eq!(s.get(id).map(|st| st.has_ref), Some(id % 2 == 0));
        }
    }

    /// #10447: a still-scheduled id is never an eviction candidate, however
    /// many later timers come and go — its `unref()` and its kind survive.
    #[test]
    fn a_scheduled_id_survives_any_number_of_later_timers() {
        let mut s = TimerRefStates::default();
        let cap = 16;
        s.schedule(1, CallbackTimerKind::Timeout);
        s.set_ref(1, false, cap);
        s.schedule(2, CallbackTimerKind::Immediate);
        scheduled_then_retired(&mut s, 3..=10_000, cap);
        let keep = s.get(1).expect("scheduled id 1 was evicted");
        assert!(!keep.has_ref, "id 1's unref() was forgotten");
        assert!(matches!(keep.kind, Some(CallbackTimerKind::Timeout)));
        assert!(matches!(
            s.get(2).and_then(|st| st.kind),
            Some(CallbackTimerKind::Immediate)
        ));
        assert_eq!(s.states.len(), cap + 2);
        // Once it retires it is an ordinary eviction candidate again.
        s.retire(1, cap);
        s.retire(2, cap);
        scheduled_then_retired(&mut s, 10_001..=10_000 + cap as i64, cap);
        assert!(s.get(1).is_none() && s.get(2).is_none());
        assert_eq!(s.states.len(), cap);
    }

    /// More live timers than the cap: all of them stay, and churn around them
    /// still only keeps `cap` retired ids.
    #[test]
    fn live_timers_beyond_the_cap_are_all_kept() {
        let mut s = TimerRefStates::default();
        let cap = 8;
        for id in 1..=100i64 {
            s.schedule(id, CallbackTimerKind::Timeout);
        }
        scheduled_then_retired(&mut s, 101..=1_000, cap);
        assert!((1..=100i64).all(|id| s.get(id).is_some()));
        assert_eq!(s.states.len(), 100 + cap);
        assert_eq!(s.retired.len(), cap);
    }

    /// #6084's leak must stay fixed: a million set+clear cycles at the real
    /// cap leave the registry at the cap, not at a million entries.
    #[test]
    fn a_million_set_clear_cycles_stay_bounded() {
        let mut s = TimerRefStates::default();
        let cap = TIMER_REF_STATES_CAP;
        scheduled_then_retired(&mut s, 1..=1_000_000, cap);
        assert_eq!(s.states.len(), cap);
        assert_eq!(s.retired.len(), cap);
        // `ref()`/`unref()` on ids the registry never held is bounded too.
        for id in 2_000_000..2_000_000 + 3 * cap as i64 {
            s.set_ref(id, false, cap);
        }
        assert_eq!(s.states.len(), cap);
        assert_eq!(s.retired.len(), cap);
    }

    #[test]
    fn ref_unref_and_repeated_retire_do_not_grow_the_queue() {
        let mut s = TimerRefStates::default();
        let cap = 100;
        s.schedule(42, CallbackTimerKind::Timeout);
        s.set_ref(42, false, cap);
        s.set_ref(42, true, cap);
        assert_eq!(
            s.retired.len(),
            0,
            "a scheduled id is not queued for eviction"
        );
        s.retire(42, cap);
        s.retire(42, cap);
        s.set_ref(42, false, cap);
        assert_eq!(s.retired.len(), 1);
        assert_eq!(s.states.len(), 1);
        assert_eq!(s.get(42).map(|st| st.has_ref), Some(false));
        s.retire(7, cap);
        assert_eq!(s.states.len(), 1, "retiring an unknown id is a no-op");
    }

    /// End to end through the real queues: a live unref'd `setTimeout`, an
    /// unref'd `setInterval` and a `setImmediate` keep their state across more
    /// than `TIMER_REF_STATES_CAP` later set+clear cycles, and do not keep the
    /// event loop alive. Before #10447 all three ids were evicted, so the
    /// loop saw two ref'd timers and `is_known_timer_id` rejected all three.
    #[test]
    fn live_timers_keep_ref_state_across_timer_churn() {
        use crate::timer::*;
        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();
        let timeout = js_set_timeout_callback(0, 50_000.0);
        js_timer_unref(timeout);
        let interval = setInterval(0, 50_000.0);
        js_timer_unref(interval);
        let immediate = js_set_immediate_callback(0);
        let recent = js_set_timeout_callback(0, 50_000.0);
        js_timer_unref(recent);

        for _ in 0..TIMER_REF_STATES_CAP + 1_000 {
            clearTimeout(js_set_timeout_callback(0, 1_000.0));
        }
        clearTimeout(recent);
        for _ in 0..1_000 {
            clearTimeout(js_set_timeout_callback(0, 1_000.0));
        }

        for id in [timeout, interval, immediate, recent] {
            assert!(
                is_known_timer_id(id),
                "timer {id} dropped from the registry"
            );
        }
        assert_eq!(js_timer_has_ref(timeout), 0);
        assert_eq!(js_timer_has_ref(interval), 0);
        assert_eq!(js_timer_has_ref(immediate), 1);
        assert_eq!(
            js_timer_has_ref(recent),
            0,
            "post-clear hasRef of a recent handle"
        );
        assert!(matches!(
            super::timer_handle_kind(timeout),
            Some(CallbackTimerKind::Timeout)
        ));
        assert!(matches!(
            super::timer_handle_kind(interval),
            Some(CallbackTimerKind::Timeout)
        ));
        assert!(matches!(
            super::timer_handle_kind(immediate),
            Some(CallbackTimerKind::Immediate)
        ));
        assert_eq!(
            js_interval_timer_has_pending(),
            0,
            "unref'd interval kept the loop alive"
        );

        // Only the ref'd immediate keeps the loop alive; once it is gone,
        // nothing does — and re-`ref()`ing the timeout re-arms it.
        clearImmediate(immediate);
        assert_eq!(
            js_callback_timer_has_pending(),
            0,
            "unref'd timeout kept the loop alive"
        );
        js_timer_ref(timeout);
        assert_eq!(
            js_callback_timer_has_pending(),
            1,
            "ref() after churn did not re-arm"
        );

        // The registry stayed bounded: every entry beyond the cap is scheduled.
        let (len, scheduled) = super::test_ref_state_counts();
        assert!(
            len <= TIMER_REF_STATES_CAP + scheduled,
            "{len} entries, {scheduled} scheduled"
        );

        clearTimeout(timeout);
        clearInterval(interval);
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
/// Armed by `register_scheduled_timer` / `set_timer_ref_state`, which run
/// before any id becomes observable, per `registry_latch`'s ordering rule.
pub(crate) static TIMER_IDS_NONEMPTY: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

/// Whether `id` corresponds to a timer that was scheduled by this runtime
/// (active or already cleared). Used by the small-handle method/property
/// fast paths in `object/*.rs` and by `js_number_coerce` to decide whether
/// to apply Timeout-shaped semantics to a NaN-boxed small pointer. Without
/// this gate, any small handle (UI widget, drizzle, etc.) would accidentally
/// route through timer dispatch.
///
/// A scheduled timer's id is always registered. After `clearTimeout` or
/// firing it stays registered — so post-clear `.hasRef()` / `+timer` /
/// `.unref()` still route through timer dispatch (Node keeps the Timeout
/// object alive and its methods still work) — until 65,536 later timers have
/// also retired.
#[inline]
pub fn is_known_timer_id(id: i64) -> bool {
    if id <= 0 || TIMER_IDS_NONEMPTY.is_idle() {
        return false;
    }
    is_known_timer_id_slow(id)
}

#[inline(never)]
fn is_known_timer_id_slow(id: i64) -> bool {
    lock_states()
        .as_ref()
        .is_some_and(|states| states.states.contains_key(&id))
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
