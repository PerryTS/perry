//! Test scaffolding other modules reach as `crate::timer::…`, plus the inline
//! unit tests extracted from `timer.rs` (#8354).
//!
//! turnloop P3 replaced the three queues these helpers used to seed with the
//! per-agent store, so the seeding goes through the store's own API. The GC
//! root-scanner tests that consume them are unchanged: they ask for a timeout,
//! a callback timer and an interval whose slots the collector must visit.

use super::store::{self, Class, Entry};
use super::*;

pub(crate) const TEST_CALLBACK_TIMER_ID: i64 = i64::MIN + 101;
pub(crate) const TEST_INTERVAL_TIMER_ID: i64 = i64::MIN + 102;

#[derive(Debug, Default)]
pub(crate) struct TestTimerScannerSnapshot {
    pub timeout_promise_ptr: usize,
    pub timeout_value_bits: u64,
    pub callback_ptr: usize,
    pub callback_arg_bits: u64,
    pub callback_context_store_bits: u64,
    pub interval_callback_ptr: usize,
    pub interval_context_store_bits: u64,
}

fn far_future() -> Instant {
    Instant::now() + Duration::from_secs(86_400)
}

pub(crate) fn test_seed_timer_scanner_roots(
    promise: *mut Promise,
    value: f64,
    callback: i64,
    arg: f64,
    context_store: f64,
) {
    let context = crate::async_context::test_snapshot_with_store(context_store);
    let deadline = far_future();
    store::with_current(|timers| {
        timers.insert_timer(Entry::promise(deadline, promise, value, true));
        timers.insert_timer(Entry::callback(
            TEST_CALLBACK_TIMER_ID,
            Class::Timeout,
            deadline,
            86_400_000,
            callback,
            vec![arg],
            context.clone(),
            0,
            0,
        ));
        timers.insert_timer(Entry::callback(
            TEST_INTERVAL_TIMER_ID,
            Class::Interval,
            deadline,
            86_400_000,
            callback,
            Vec::new(),
            context.clone(),
            0,
            0,
        ));
    });
}

pub(crate) fn test_seed_many_timeout_roots(values: &[f64]) {
    let deadline = far_future();
    store::with_current(|timers| {
        timers.test_clear();
        for &value in values {
            timers.insert_timer(Entry::promise(deadline, std::ptr::null_mut(), value, true));
        }
    });
}

pub(crate) fn test_clear_all_timer_scanner_roots() {
    store::with_current(|timers| timers.test_clear());
}

pub(crate) fn test_timer_scanner_snapshot() -> TestTimerScannerSnapshot {
    let mut snapshot = TestTimerScannerSnapshot::default();
    store::with_current(|timers| {
        if let Some(entry) = timers.test_last_of_class(Class::Promise) {
            snapshot.timeout_promise_ptr = entry.promise as usize;
            snapshot.timeout_value_bits = entry.value.to_bits();
        }
        if let Some(entry) = timers.test_find_by_id(TEST_CALLBACK_TIMER_ID) {
            snapshot.callback_ptr = entry.callback as usize;
            snapshot.callback_arg_bits = entry.args.first().copied().map(f64::to_bits).unwrap_or(0);
            snapshot.callback_context_store_bits =
                crate::async_context::test_snapshot_first_store(&entry.context)
                    .map(f64::to_bits)
                    .unwrap_or(0);
        }
        if let Some(entry) = timers.test_find_by_id(TEST_INTERVAL_TIMER_ID) {
            snapshot.interval_callback_ptr = entry.callback as usize;
            snapshot.interval_context_store_bits =
                crate::async_context::test_snapshot_first_store(&entry.context)
                    .map(f64::to_bits)
                    .unwrap_or(0);
        }
    });
    snapshot
}

pub(crate) fn test_callback_timer_snapshot(timer_id: i64) -> Option<(usize, u64)> {
    store::with_current(|timers| {
        timers.test_find_by_id(timer_id).map(|entry| {
            (
                entry.callback as usize,
                entry.args.first().copied().map(f64::to_bits).unwrap_or(0),
            )
        })
    })
}

pub(crate) fn test_clear_timer_scanner_roots(promise_before: usize, promise_after: usize) {
    store::with_current(|timers| {
        timers.test_retain(|entry| match entry.class {
            Class::Promise => {
                let promise = entry.promise as usize;
                promise != promise_before && promise != promise_after
            }
            _ => entry.id != TEST_CALLBACK_TIMER_ID && entry.id != TEST_INTERVAL_TIMER_ID,
        });
    });
}
