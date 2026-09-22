//! PERRY_LOOP_STATS wait-metric tests. They drive the real park and notify
//! entry points; every assertion is a delta against a snapshot, and each case
//! proves its wait actually ran.

use super::*;
use std::sync::atomic::Ordering;
use std::sync::PoisonError;
use std::time::Duration;

fn serial() -> std::sync::MutexGuard<'static, ()> {
    super::super::tests::SERIAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn wake_latency_buckets_split_at_50us_200us_1ms_5ms() {
    let _g = serial();
    force_enable_for_test();
    let before = snapshot();
    for ns in [
        1, 49_999, 50_000, 199_999, 200_000, 999_999, 1_000_000, 4_999_999, 5_000_000,
    ] {
        record_wake_latency(ns);
    }
    let after = snapshot();
    let delta: Vec<u64> = (0..5)
        .map(|i| after.wake_buckets[i] - before.wake_buckets[i])
        .collect();
    assert_eq!(delta, vec![2, 2, 2, 2, 1]);
    assert!(after.wake_max_ns >= 5_000_000);
}

/// A deliberate cross-thread notify into a parked condvar wait produces exactly
/// one wake-latency sample and one condvar wait.
#[test]
fn one_cross_thread_notify_into_a_condvar_park_is_one_wake_sample() {
    let _g = serial();
    force_enable_for_test();
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
    let before = snapshot();
    let waiter = std::thread::spawn(|| {
        let start = std::time::Instant::now();
        super::super::condvar_park(Duration::from_secs(30));
        start.elapsed()
    });
    let limit = std::time::Instant::now() + Duration::from_secs(10);
    while PARKED.load(Ordering::SeqCst) != WaitKind::Condvar as u8 {
        assert!(std::time::Instant::now() < limit, "waiter never parked");
        std::thread::yield_now();
    }
    super::super::js_notify_main_thread();
    let waited = waiter.join().unwrap();
    assert!(
        waited < Duration::from_secs(10),
        "the notify never woke the park"
    );
    let after = snapshot();
    assert_eq!(after.condvar.count - before.condvar.count, 1);
    assert_eq!(after.wake_samples() - before.wake_samples(), 1);
    assert!(after.condvar.total_ns > before.condvar.total_ns);
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
}

/// A wait that simply times out is measured but has no wake sample, and a
/// notify outside any wait adds no sample either.
#[test]
fn a_timed_out_wait_and_an_unparked_notify_add_no_wake_sample() {
    let _g = serial();
    force_enable_for_test();
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
    let before = snapshot();
    std::thread::spawn(|| super::super::condvar_park(Duration::from_millis(5)))
        .join()
        .unwrap();
    super::super::js_notify_main_thread();
    let after = snapshot();
    assert_eq!(after.condvar.count - before.condvar.count, 1);
    assert!(after.condvar.total_ns - before.condvar.total_ns >= 4_000_000);
    assert!(after.condvar.max_ns >= 4_000_000);
    assert_eq!(after.wake_samples(), before.wake_samples());
    assert_eq!(NOTIFY_AT_NS.load(Ordering::SeqCst), 0);
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
}

static FAKE_WOKEN: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
static FAKE_CVAR: std::sync::Condvar = std::sync::Condvar::new();

extern "C" fn fake_tick(budget_ms: u64) {
    let guard = FAKE_WOKEN.lock().unwrap();
    let (mut woken, _) = FAKE_CVAR
        .wait_timeout_while(guard, Duration::from_millis(budget_ms), |w| !*w)
        .unwrap();
    *woken = false;
}

extern "C" fn fake_wake() {
    *FAKE_WOKEN.lock().unwrap() = true;
    FAKE_CVAR.notify_all();
}

/// The tokio-tick kind: a notify while the registered tick is parked is one
/// wake sample and one tick (a stand-in tick with the same wake contract).
#[test]
fn one_notify_into_a_registered_tick_is_one_wake_sample() {
    let _g = serial();
    force_enable_for_test();
    *FAKE_WOKEN.lock().unwrap() = false;
    super::super::js_register_wait_driver(Some(fake_tick), None, Some(fake_wake));
    let before = snapshot();
    let waiter = std::thread::spawn(|| super::super::wait_driver_sleep(30_000));
    let limit = std::time::Instant::now() + Duration::from_secs(10);
    while PARKED.load(Ordering::SeqCst) != WaitKind::TokioTick as u8 {
        assert!(std::time::Instant::now() < limit, "tick never parked");
        std::thread::yield_now();
    }
    super::super::js_notify_main_thread();
    let ran = waiter.join().unwrap();
    super::super::js_register_wait_driver(None, None, None);
    assert!(ran, "the registered tick did not run");
    let after = snapshot();
    assert_eq!(after.tokio_tick.count - before.tokio_tick.count, 1);
    assert_eq!(after.wake_samples() - before.wake_samples(), 1);
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
}

#[test]
fn fast_drives_and_zero_budget_returns_are_counted() {
    let _g = serial();
    force_enable_for_test();
    let before = snapshot();
    let started = begin_fast_drive();
    assert_ne!(started, 0);
    std::thread::sleep(Duration::from_millis(1));
    end_fast_drive(started);
    end_fast_drive(0);
    note_zero_budget(false);
    note_zero_budget(true);
    let after = snapshot();
    assert_eq!(after.fast_drive.count - before.fast_drive.count, 1);
    assert!(after.fast_drive.total_ns - before.fast_drive.total_ns >= 900_000);
    assert_eq!(after.zero_budget - before.zero_budget, 2);
    assert_eq!(after.throttle_sleeps - before.throttle_sleeps, 1);
}

/// The zero-budget hook through the real `js_wait_for_event` entry.
#[test]
fn js_wait_for_event_zero_budget_path_is_counted() {
    let _g = serial();
    force_enable_for_test();
    super::super::NOTIFIED.store(false, Ordering::SeqCst);
    let before = snapshot();
    super::super::TEST_FORCE_ZERO_BUDGET.store(true, Ordering::SeqCst);
    std::thread::spawn(|| super::super::js_wait_for_event())
        .join()
        .unwrap();
    super::super::TEST_FORCE_ZERO_BUDGET.store(false, Ordering::SeqCst);
    assert_eq!(snapshot().zero_budget - before.zero_budget, 1);
}

/// Worker agents are not recorded, and the exit line carries every field.
#[test]
fn workers_are_not_recorded_and_the_line_names_every_metric() {
    force_enable_for_test();
    std::thread::spawn(|| {
        let agent = crate::agent::enter_worker_agent();
        assert_eq!(begin_wait(WaitKind::Condvar), None);
        assert_eq!(begin_fast_drive(), 0);
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
    let line = format_line("turnloop", &snapshot());
    for key in [
        "arm=turnloop",
        "turnloop_waits=",
        "turnloop_wait_ns=",
        "turnloop_wait_max_ns=",
        "tokio_ticks=",
        "tokio_tick_ns=",
        "tokio_tick_max_ns=",
        "condvar_waits=",
        "fast_drives=",
        "fast_drive_ns=",
        "zero_budget=",
        "throttle_sleeps=",
        "wake_samples=",
        "wake_lt50us=",
        "wake_lt200us=",
        "wake_lt1ms=",
        "wake_lt5ms=",
        "wake_ge5ms=",
        "wake_max_ns=",
    ] {
        assert!(line.contains(key), "missing {key} in {line}");
    }
}
