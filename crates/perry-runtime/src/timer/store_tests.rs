//! Unit tests for the agent timer store (turnloop P3).
//!
//! These exercise the structure directly — no JS, no callbacks — so a failure
//! names the heap, the id index or the keep-alive counters rather than a timer
//! that did not fire. Every test asserts its subject actually held entries; an
//! empty store would satisfy most of the ordering assertions vacuously.

use super::*;
use std::time::Duration;

fn entry_at(id: i64, class: Class, base: Instant, delay_ms: u64) -> Entry {
    Entry::callback(
        id,
        class,
        base + Duration::from_millis(delay_ms),
        delay_ms,
        0,
        Vec::new(),
        crate::async_context::AsyncContextSnapshot::default(),
        0,
        0,
        None,
    )
}

/// Draining the heap must yield deadline order regardless of insertion order,
/// with same-deadline entries in creation order — Node's timers-phase walk.
#[test]
fn heap_drains_in_deadline_then_creation_order() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        for (id, delay) in [(1, 10), (2, 5), (3, 1), (4, 3), (5, 3), (6, 3)] {
            timers.insert_timer(entry_at(id, Class::Timeout, base, delay));
        }
        assert_eq!(timers.refed_timers(), 6, "the subject must hold entries");

        let now = base + Duration::from_millis(50);
        let mut order = Vec::new();
        while let Some(entry) = timers.pop_due(now, u64::MAX) {
            order.push(entry.id);
        }
        assert_eq!(order, vec![3, 4, 5, 6, 2, 1]);
        assert_eq!(timers.refed_timers(), 0);
    });
}

/// An entry created during a phase carries a sequence number past the phase's
/// horizon and must not run in it, even when its deadline is already due. That
/// is Node's rule and it is also what stops a zero-delay timer scheduled from a
/// timer callback livelocking the phase loop.
#[test]
fn seq_horizon_defers_entries_created_during_the_phase() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Timeout, base, 0));
        let horizon = timers.seq_horizon();
        timers.insert_timer(entry_at(2, Class::Timeout, base, 0));

        let now = base + Duration::from_millis(5);
        assert_eq!(timers.pop_due(now, horizon).map(|e| e.id), Some(1));
        assert!(
            timers.pop_due(now, horizon).is_none(),
            "an entry created after the horizon waits for the next phase"
        );
        assert_eq!(
            timers.pop_due(now, u64::MAX).map(|e| e.id),
            Some(2),
            "and runs on the next one"
        );
    });
}

/// Cancellation removes the entry immediately (DESIGN D6), which is what lets a
/// callback cancel a sibling that was already due — measured Node behaviour.
#[test]
fn cancel_removes_immediately_and_keeps_the_heap_ordered() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        for (id, delay) in [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
            timers.insert_timer(entry_at(id, Class::Timeout, base, delay));
        }
        assert!(timers
            .remove_by_id(3, |class| class == Class::Timeout)
            .is_some());
        assert!(timers
            .remove_by_id(1, |class| class == Class::Timeout)
            .is_some());
        assert!(
            timers
                .remove_by_id(3, |class| class == Class::Timeout)
                .is_none(),
            "a second cancel of the same id finds nothing"
        );
        assert_eq!(timers.refed_timers(), 3);

        let now = base + Duration::from_millis(50);
        let mut order = Vec::new();
        while let Some(entry) = timers.pop_due(now, u64::MAX) {
            order.push(entry.id);
        }
        assert_eq!(order, vec![2, 4, 5]);
    });
}

/// `clearImmediate` must not cancel a Timeout and `clearTimeout` must not
/// cancel an Immediate: Node keeps the handle types distinct.
#[test]
fn cancel_respects_the_class_filter() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Timeout, base, 1));
        timers.insert_check(entry_at(2, Class::Immediate, base, 0));
        assert!(timers
            .remove_by_id(1, |class| class == Class::Immediate)
            .is_none());
        assert!(timers
            .remove_by_id(2, |class| matches!(class, Class::Timeout | Class::Interval))
            .is_none());
        assert!(timers
            .remove_by_id(1, |class| class == Class::Timeout)
            .is_some());
        assert!(timers
            .remove_by_id(2, |class| class == Class::Immediate)
            .is_some());
    });
}

/// `unref()` moves an entry out of the keep-alive heap but leaves it schedulable;
/// `ref()` puts it back. The deadline query hides unref'd entries when nothing
/// else keeps the loop alive, which is how an unref'd interval alone never ticks.
#[test]
fn ref_state_moves_between_heaps_and_gates_only_the_deadline() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Timeout, base, 10));
        timers.insert_timer(entry_at(2, Class::Timeout, base, 1));
        assert_eq!(timers.refed_timers(), 2);

        assert!(timers.set_ref(2, false));
        assert_eq!(timers.refed_timers(), 1);
        assert_eq!(
            timers.next_deadline(false),
            Some(base + Duration::from_millis(10)),
            "an unref'd entry contributes no deadline of its own"
        );
        assert_eq!(
            timers.next_deadline(true),
            Some(base + Duration::from_millis(1)),
            "but it does while something else keeps the loop alive"
        );

        // Firing is not gated on ref state: the phase only runs on a live loop.
        let now = base + Duration::from_millis(50);
        assert_eq!(timers.pop_due(now, u64::MAX).map(|e| e.id), Some(2));

        assert!(timers.set_ref(1, false));
        assert_eq!(timers.refed_timers(), 0);
        assert!(timers.set_ref(1, true));
        assert_eq!(timers.refed_timers(), 1);
    });
}

/// `Timeout.refresh()` re-arms at now + the original delay, sorts by the new
/// deadline, and leaves the handle's ref state alone (Node never re-refs on
/// refresh — measured: an unref'd handle still reports `hasRef() === false`).
#[test]
fn refresh_re_arms_at_the_original_delay() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Timeout, base, 10));
        timers.insert_timer(entry_at(2, Class::Timeout, base, 20));
        assert!(timers.refresh(1, base + Duration::from_millis(30)));
        assert_eq!(
            timers.next_deadline(true),
            Some(base + Duration::from_millis(20)),
            "the refreshed entry moved behind the one it used to precede"
        );
        assert!(!timers.refresh(99, base), "an unknown id finds nothing");

        assert!(timers.set_ref(2, false));
        assert_eq!(timers.refed_timers(), 1);
        assert!(timers.refresh(2, base + Duration::from_millis(40)));
        assert_eq!(
            timers.refed_timers(),
            1,
            "refresh must not re-ref an unref'd handle"
        );
    });
}

/// The check queue is FIFO, skips cancelled entries without disturbing the rest
/// (measured: `a` clearing `b` still lets `c` run), and defers entries queued
/// during the phase.
#[test]
fn check_queue_is_fifo_with_a_snapshot_boundary() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        for id in [1, 2, 3] {
            timers.insert_check(entry_at(id, Class::Immediate, base, 0));
        }
        assert!(timers.check_pending());
        assert_eq!(timers.refed_check(), 3);
        let horizon = timers.seq_horizon();
        timers.insert_check(entry_at(4, Class::Immediate, base, 0));

        assert!(timers
            .remove_by_id(2, |class| class == Class::Immediate)
            .is_some());
        assert_eq!(timers.pop_check(horizon).map(|e| e.id), Some(1));
        assert_eq!(
            timers.pop_check(horizon).map(|e| e.id),
            Some(3),
            "the cancelled entry is skipped, its neighbours are not"
        );
        assert!(
            timers.pop_check(horizon).is_none(),
            "an immediate queued during the phase waits for the next turn"
        );
        assert!(timers.check_pending(), "…but it is still queued");
        assert_eq!(timers.pop_check(u64::MAX).map(|e| e.id), Some(4));
        assert!(!timers.check_pending());
        assert_eq!(timers.refed_check(), 0);
    });
}

/// An interval re-arms one period past the PHASE's clock read, never past the
/// callback's completion time. That is libuv's `uv_timer_again` from
/// `loop->time`, and it is what makes a handler that overruns its period fire
/// once per iteration instead of catching up in a burst.
#[test]
fn interval_rearms_from_the_phase_clock_read() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Interval, base, 10));
        let phase_now = base + Duration::from_millis(10);
        let mut entry = timers
            .pop_due(phase_now, u64::MAX)
            .expect("the interval is due");
        timers.rearm_interval(entry.duplicate_for_rearm(), phase_now);
        assert_eq!(
            timers.next_deadline(true),
            Some(phase_now + Duration::from_millis(10))
        );
        assert!(
            timers.pop_due(phase_now, u64::MAX).is_none(),
            "a re-armed interval can never fire twice in one phase"
        );
        // And it keeps its id, so a later clearInterval still finds it.
        assert!(timers
            .remove_by_id(1, |class| class == Class::Interval)
            .is_some());
    });
}

/// The primary agent's O(1) counters are republished from the partition after
/// every mutation, so they cannot drift the way a pairwise increment can.
#[test]
fn primary_counters_track_the_partition() {
    reset_for_test();
    let base = Instant::now();
    assert!(!has_refed_timers());
    assert!(!has_refed_check());
    assert!(!any_pending());

    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Timeout, base, 5));
    });
    assert!(has_refed_timers());
    assert!(any_pending());
    assert!(!has_refed_check());

    with_current(|timers| {
        timers.insert_check(entry_at(2, Class::Immediate, base, 0));
    });
    assert!(has_refed_check());
    assert!(check_pending());

    with_current(|timers| {
        assert!(timers.set_ref(1, false));
    });
    assert!(
        !has_refed_timers(),
        "an unref'd timer stops keeping the loop alive"
    );
    assert!(any_pending(), "but it is still queued");

    with_current(|timers| {
        assert!(timers.remove_by_id(1, |_| true).is_some());
        assert!(timers.remove_by_id(2, |_| true).is_some());
    });
    assert!(!any_pending());
    assert!(!check_pending());
    reset_for_test();
}

/// A native completion callback is passed over by the poll phase in flight and
/// runs in the next one. That is the turn of latency Perry's eager syscall
/// removed: on Node a `setImmediate` queued beside a top-level `fs.readFile`
/// runs first, 10/10 runs, in either registration order.
#[test]
fn poll_callbacks_skip_the_poll_phase_in_flight() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_pending(entry_at(1, Class::Pending, base, 0));
        assert!(timers.poll_pending(), "the subject must be queued");

        // The poll phase in flight: drain what is eligible, then promote.
        assert!(
            timers.pop_poll().is_none(),
            "an entry queued before this phase is not eligible IN this phase"
        );
        timers.promote_pending();
        assert!(timers.poll_pending());

        // The next poll phase runs it.
        assert_eq!(timers.pop_poll().map(|e| e.id), Some(1));
        assert!(timers.pop_poll().is_none());
        timers.promote_pending();
        assert!(!timers.poll_pending());
    });
}

/// A cancelled native completion is skipped without disturbing its neighbours,
/// and a poll-phase entry is not a timers-phase entry.
#[test]
fn poll_queue_cancels_and_stays_out_of_the_heap() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        for id in [1, 2, 3] {
            timers.insert_pending(entry_at(id, Class::Pending, base, 0));
        }
        timers.promote_pending();
        assert!(timers
            .remove_by_id(2, |class| class == Class::Pending)
            .is_some());
        assert_eq!(timers.pop_poll().map(|e| e.id), Some(1));
        assert_eq!(timers.pop_poll().map(|e| e.id), Some(3));
        assert!(timers.pop_poll().is_none());
        assert_eq!(timers.refed_timers(), 0, "nothing reached the timers heap");
        assert!(timers
            .pop_due(base + Duration::from_secs(1), u64::MAX)
            .is_none());
    });
}

/// A queued native completion keeps the loop alive and blocks the park, because
/// dropping one would lose the completion outright.
#[test]
fn poll_callbacks_keep_the_loop_alive_and_block_the_park() {
    reset_for_test();
    let base = Instant::now();
    assert!(!has_refed_check());
    with_current(|timers| {
        timers.insert_pending(entry_at(1, Class::Pending, base, 0));
    });
    assert!(
        has_refed_check(),
        "a pending completion keeps the loop alive"
    );
    assert!(check_pending(), "and the loop must not park before it runs");
    with_current(|timers| {
        timers.promote_pending();
        assert_eq!(timers.pop_poll().map(|e| e.id), Some(1));
    });
    assert!(!has_refed_check());
    assert!(!check_pending());
    reset_for_test();
}

/// A retired agent's partition goes away wholesale — its arena is unmapped, so
/// nothing in it could ever legally run again.
#[test]
fn purging_an_agent_drops_its_partition() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        timers.insert_timer(entry_at(1, Class::Timeout, base, 5));
    });
    assert!(any_pending());
    purge_agent(crate::agent::current_agent());
    assert!(!any_pending());
    assert!(with_current_existing(|t| t.any_pending()).is_none());
}

/// A thousand inserts and cancels in mixed order must leave the heap a valid
/// min-heap: the drain order is the sorted order. This is the invariant a
/// hand-written sift-up/sift-down most easily breaks.
#[test]
fn heap_survives_mixed_insert_and_cancel_churn() {
    reset_for_test();
    let base = Instant::now();
    with_current(|timers| {
        // Deterministic pseudo-random delays; no dependency on a RNG crate.
        let mut x: u64 = 0x9E3779B97F4A7C15;
        let mut expected: Vec<(u64, i64)> = Vec::new();
        for id in 1..=1000i64 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let delay = x % 500;
            timers.insert_timer(entry_at(id, Class::Timeout, base, delay));
            expected.push((delay, id));
        }
        for id in (1..=1000i64).step_by(3) {
            assert!(timers.remove_by_id(id, |_| true).is_some());
        }
        expected.retain(|(_, id)| (id - 1) % 3 != 0);
        expected.sort();
        assert_eq!(timers.refed_timers(), expected.len());
        assert!(expected.len() > 600, "the subject must still be populated");

        let now = base + Duration::from_millis(10_000);
        let mut drained = Vec::new();
        while let Some(entry) = timers.pop_due(now, u64::MAX) {
            drained.push(entry.delay_ms);
        }
        let mut sorted = drained.clone();
        sorted.sort();
        assert_eq!(drained, sorted, "the drain order must be the sorted order");
        assert_eq!(drained.len(), expected.len());
    });
    reset_for_test();
}
