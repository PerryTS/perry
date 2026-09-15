//! turnloop P0: every keep-alive increment has exactly one decrement.
//!
//! Each case drives the real timer entry points (schedule, fire, clear,
//! ref/unref, refresh, agent purge) and checks both that the O(1) count moved
//! — so the subject ran — and that it equals a fresh recount of its queue
//! before returning to zero.

use super::super::*;
use std::time::Duration;

fn counts() -> (usize, usize, usize) {
    (
        TIMER_QUEUE.primary_live_for_test(),
        CALLBACK_TIMERS.primary_live_for_test(),
        INTERVAL_TIMERS.primary_live_for_test(),
    )
}

fn assert_paired() {
    assert_eq!(TIMER_QUEUE.primary_live_for_test(), TIMER_QUEUE.recount());
    assert_eq!(
        CALLBACK_TIMERS.primary_live_for_test(),
        CALLBACK_TIMERS.recount()
    );
    assert_eq!(
        INTERVAL_TIMERS.primary_live_for_test(),
        INTERVAL_TIMERS.recount()
    );
}

extern "C" fn noop_timer_callback(_closure: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Each case runs on a fresh thread: the queues are `per_test_global!`, so the
/// thread starts from empty queues and zero counts.
fn on_fresh_queues(case: fn()) {
    std::thread::spawn(move || {
        assert_eq!(counts(), (0, 0, 0), "fresh queues must start at zero");
        case();
        assert_paired();
        assert_eq!(counts(), (0, 0, 0), "a keep-alive reference leaked");
    })
    .join()
    .unwrap();
}

#[test]
fn promise_timers_balance_across_fire_and_unref() {
    on_fresh_queues(|| {
        let _refed = js_set_timeout(1.0);
        let _unrefed = js_set_timeout_value_ref(1.0, 0.0, 0);
        assert_eq!(
            TIMER_QUEUE.primary_live_for_test(),
            1,
            "only the ref'd one counts"
        );
        assert_eq!(js_timer_has_pending(), 1);
        assert_paired();
        std::thread::sleep(Duration::from_millis(5));
        assert!(js_timer_tick() >= 1, "the fire path never ran");
        assert_paired();
        // An unref'd promise timer that did not fire alongside the ref'd one
        // holds no count; drain it by firing with nothing else alive.
        let _ = js_timer_tick();
        TIMER_QUEUE.lock().unwrap().clear();
        TIMER_QUEUE.resync_for_test();
        assert_eq!(js_timer_has_pending(), 0);
    });
}

#[test]
fn callback_timers_balance_across_cancel_unref_ref_and_refresh() {
    on_fresh_queues(|| {
        let timeout = js_set_timeout_callback(0, 60_000.0);
        let immediate = js_set_immediate_callback(0);
        assert_eq!(CALLBACK_TIMERS.primary_live_for_test(), 2);
        js_timer_unref(timeout);
        js_timer_unref(timeout);
        assert_eq!(
            CALLBACK_TIMERS.primary_live_for_test(),
            1,
            "unref is idempotent"
        );
        js_timer_ref(timeout);
        assert_eq!(CALLBACK_TIMERS.primary_live_for_test(), 2);
        js_timer_unref(timeout);
        js_timer_refresh(timeout);
        assert_eq!(
            CALLBACK_TIMERS.primary_live_for_test(),
            2,
            "refresh re-refs"
        );
        assert_paired();
        clearImmediate(immediate);
        clearTimeout(timeout);
        // Ref changes after removal touch only the hasRef() registry.
        js_timer_unref(timeout);
        js_timer_ref(timeout);
        assert_eq!(js_callback_timer_has_pending(), 0);
    });
}

#[test]
fn callback_timers_balance_across_the_fire_path() {
    on_fresh_queues(|| {
        let callback = crate::closure::js_closure_alloc(noop_timer_callback as *const u8, 0) as i64;
        let _fires = js_set_timeout_callback(callback, 1.0);
        let unrefed = js_set_timeout_callback(callback, 1.0);
        js_timer_unref(unrefed);
        assert_eq!(CALLBACK_TIMERS.primary_live_for_test(), 1);
        std::thread::sleep(Duration::from_millis(5));
        assert!(js_callback_timer_tick() >= 1, "the fire path never ran");
        assert_paired();
        clearTimeout(unrefed);
    });
}

#[test]
fn interval_timers_balance_across_fire_unref_and_both_clear_spellings() {
    on_fresh_queues(|| {
        let callback = crate::closure::js_closure_alloc(noop_timer_callback as *const u8, 0) as i64;
        let interval = setInterval(callback, 1.0);
        assert_eq!(INTERVAL_TIMERS.primary_live_for_test(), 1);
        std::thread::sleep(Duration::from_millis(5));
        assert!(js_interval_timer_tick() >= 1, "the fire path never ran");
        assert_eq!(
            INTERVAL_TIMERS.primary_live_for_test(),
            1,
            "an interval stays queued"
        );
        js_timer_unref(interval);
        assert_eq!(js_interval_timer_has_pending(), 0);
        js_timer_ref(interval);
        assert_eq!(js_interval_timer_has_pending(), 1);
        clearInterval(interval);
        let second = setInterval(0, 60_000.0);
        assert_eq!(INTERVAL_TIMERS.primary_live_for_test(), 1);
        clearTimeout(second);
    });
}

/// A worker agent's timers never enter the primary count, and retiring the
/// worker purges them without disturbing it (the agent-exit cancel path).
#[test]
fn worker_timers_on_a_shared_queue_never_touch_the_primary_count() {
    on_fresh_queues(|| {
        let primary = js_set_timeout_callback(0, 60_000.0);
        let keys = crate::timer::test_shared_queues::test_shared_queue_keys();
        std::thread::spawn(move || {
            let agent = crate::agent::enter_worker_agent();
            crate::timer::test_shared_queues::test_adopt_queues(keys);
            let _worker_timeout = js_set_timeout_callback(0, 60_000.0);
            let _worker_interval = setInterval(0, 60_000.0);
            assert_eq!(CALLBACK_TIMERS.primary_live_for_test(), 1);
            assert_eq!(INTERVAL_TIMERS.primary_live_for_test(), 0);
            // The worker's own answer still comes from the exact scan.
            assert_eq!(js_callback_timer_has_pending(), 1);
            crate::agent::retire_agent(agent);
            assert_eq!(js_callback_timer_has_pending(), 0);
        })
        .join()
        .unwrap();
        assert_paired();
        assert_eq!(CALLBACK_TIMERS.primary_live_for_test(), 1);
        clearTimeout(primary);
    });
}
