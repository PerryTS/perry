//! Test-only seeding and snapshot helpers for the timer root scanners.
//!
//! Split out of `timer.rs` to keep that file under the 2000-line cap
//! (`scripts/check_file_size.sh`). These are `#[cfg(test)]` support routines
//! reached as `crate::timer::<name>` from the GC root-scanner tests; the
//! parent re-exports each one by name.

use super::*;

const TEST_CALLBACK_TIMER_ID: i64 = i64::MIN + 101;
const TEST_INTERVAL_TIMER_ID: i64 = i64::MIN + 102;

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

pub(crate) fn test_seed_timer_scanner_roots(
    promise: *mut Promise,
    value: f64,
    callback: i64,
    arg: f64,
    context_store: f64,
) {
    let context = crate::async_context::test_snapshot_with_store(context_store);
    let deadline = Instant::now() + Duration::from_secs(86_400);
    TIMER_QUEUE.lock().unwrap().push(Timer {
        // #6185: test scaffolding runs on the primary agent.
        owner: crate::agent::current_agent(),
        deadline,
        promise,
        value,
        has_ref: true,
    });
    CALLBACK_TIMERS.lock().unwrap().push(CallbackTimer {
        // #6185: test scaffolding runs on the primary agent.
        owner: crate::agent::current_agent(),
        id: TEST_CALLBACK_TIMER_ID,
        kind: CallbackTimerKind::Timeout,
        deadline,
        delay_ms: 86_400_000,
        callback,
        args: vec![arg],
        context: context.clone(),
        async_id: 0,
        trigger_async_id: 0,
        cleared: false,
    });
    INTERVAL_TIMERS.lock().unwrap().push(IntervalTimer {
        // #6185: test scaffolding runs on the primary agent.
        owner: crate::agent::current_agent(),
        id: TEST_INTERVAL_TIMER_ID,
        callback,
        interval_ms: 86_400_000,
        next_deadline: deadline,
        args: Vec::new(),
        context,
        cleared: false,
    });
}

pub(crate) fn test_seed_many_timeout_roots(values: &[f64]) {
    let deadline = Instant::now() + Duration::from_secs(86_400);
    let mut q = TIMER_QUEUE.lock().unwrap();
    q.clear();
    for &value in values {
        q.push(Timer {
            // #6185: test scaffolding runs on the primary agent.
            owner: crate::agent::current_agent(),
            deadline,
            promise: std::ptr::null_mut(),
            value,
            has_ref: true,
        });
    }
}

pub(crate) fn test_clear_all_timer_scanner_roots() {
    TIMER_QUEUE.lock().unwrap().clear();
    CALLBACK_TIMERS.lock().unwrap().clear();
    INTERVAL_TIMERS.lock().unwrap().clear();
}

pub(crate) fn test_timer_scanner_snapshot() -> TestTimerScannerSnapshot {
    let mut snapshot = TestTimerScannerSnapshot::default();
    if let Some(timer) = TIMER_QUEUE.lock().unwrap().last() {
        snapshot.timeout_promise_ptr = timer.promise as usize;
        snapshot.timeout_value_bits = timer.value.to_bits();
    }
    if let Some(timer) = CALLBACK_TIMERS
        .lock()
        .unwrap()
        .iter()
        .find(|timer| timer.id == TEST_CALLBACK_TIMER_ID)
    {
        snapshot.callback_ptr = timer.callback as usize;
        snapshot.callback_arg_bits = timer.args.first().copied().map(f64::to_bits).unwrap_or(0);
        snapshot.callback_context_store_bits =
            crate::async_context::test_snapshot_first_store(&timer.context)
                .map(f64::to_bits)
                .unwrap_or(0);
    }
    if let Some(timer) = INTERVAL_TIMERS
        .lock()
        .unwrap()
        .iter()
        .find(|timer| timer.id == TEST_INTERVAL_TIMER_ID)
    {
        snapshot.interval_callback_ptr = timer.callback as usize;
        snapshot.interval_context_store_bits =
            crate::async_context::test_snapshot_first_store(&timer.context)
                .map(f64::to_bits)
                .unwrap_or(0);
    }
    snapshot
}

pub(crate) fn test_callback_timer_snapshot(timer_id: i64) -> Option<(usize, u64)> {
    CALLBACK_TIMERS
        .lock()
        .unwrap()
        .iter()
        .find(|timer| timer.id == timer_id)
        .map(|timer| {
            (
                timer.callback as usize,
                timer.args.first().copied().map(f64::to_bits).unwrap_or(0),
            )
        })
}

pub(crate) fn test_clear_timer_scanner_roots(promise_before: usize, promise_after: usize) {
    TIMER_QUEUE.lock().unwrap().retain(|timer| {
        let promise = timer.promise as usize;
        promise != promise_before && promise != promise_after
    });
    CALLBACK_TIMERS
        .lock()
        .unwrap()
        .retain(|timer| timer.id != TEST_CALLBACK_TIMER_ID);
    INTERVAL_TIMERS
        .lock()
        .unwrap()
        .retain(|timer| timer.id != TEST_INTERVAL_TIMER_ID);
}
