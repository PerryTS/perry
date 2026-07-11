//! Test-only helpers and unit tests for the timer subsystem.
//!
//! Split out of `timer.rs` so the parent stays under the 2 000-line file gate
//! (`scripts/check_file_size.sh`). The `pub(crate)` seed/snapshot helpers are
//! re-exported from `timer.rs`, so out-of-module call sites keep using
//! `crate::timer::test_*` unchanged.

use super::*;

#[cfg(test)]
const TEST_CALLBACK_TIMER_ID: i64 = i64::MIN + 101;
#[cfg(test)]
const TEST_INTERVAL_TIMER_ID: i64 = i64::MIN + 102;

#[cfg(test)]
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

#[cfg(test)]
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
        deadline,
        promise,
        value,
        has_ref: true,
    });
    CALLBACK_TIMERS.lock().unwrap().push(CallbackTimer {
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
        id: TEST_INTERVAL_TIMER_ID,
        callback,
        interval_ms: 86_400_000,
        next_deadline: deadline,
        args: Vec::new(),
        context,
        cleared: false,
    });
}

#[cfg(test)]
pub(crate) fn test_seed_many_timeout_roots(values: &[f64]) {
    let deadline = Instant::now() + Duration::from_secs(86_400);
    let mut q = TIMER_QUEUE.lock().unwrap();
    q.clear();
    for &value in values {
        q.push(Timer {
            deadline,
            promise: std::ptr::null_mut(),
            value,
            has_ref: true,
        });
    }
}

#[cfg(test)]
pub(crate) fn test_clear_all_timer_scanner_roots() {
    TIMER_QUEUE.lock().unwrap().clear();
    CALLBACK_TIMERS.lock().unwrap().clear();
    INTERVAL_TIMERS.lock().unwrap().clear();
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
mod drain_expired_tests {
    use super::drain_expired_timers;

    /// The single-pass partition must preserve the original order of BOTH
    /// halves: expired entries fire in creation order (same-deadline Node
    /// semantics) and survivors keep queue order for the next tick.
    #[test]
    fn timer_drain_partition_preserves_order() {
        // (id, cleared, expired)
        let mut queue = vec![
            (1, false, true),
            (2, false, false),
            (3, true, true),
            (4, false, true),
            (5, false, false),
            (6, true, false),
            (7, false, true),
        ];
        let expired = drain_expired_timers(&mut queue, |t| t.1, |t| t.2);
        let expired_ids: Vec<i32> = expired.iter().map(|t| t.0).collect();
        let kept_ids: Vec<i32> = queue.iter().map(|t| t.0).collect();
        assert_eq!(expired_ids, vec![1, 4, 7], "expired keep creation order");
        assert_eq!(kept_ids, vec![2, 5], "survivors keep queue order");
    }

    #[test]
    fn timer_drain_partition_empty_and_all_expired() {
        let mut empty: Vec<i32> = Vec::new();
        assert!(drain_expired_timers(&mut empty, |_| false, |_| true).is_empty());

        let mut queue = vec![10, 20, 30];
        let expired = drain_expired_timers(&mut queue, |_| false, |_| true);
        assert_eq!(expired, vec![10, 20, 30]);
        assert!(queue.is_empty());
    }
}

#[cfg(test)]
mod expired_batch_order_tests {
    use super::{order_expired_callback_batch, CallbackTimer, CallbackTimerKind};
    use std::time::{Duration, Instant};

    fn timer(id: i64, kind: CallbackTimerKind, base: Instant, delay_ms: u64) -> CallbackTimer {
        CallbackTimer {
            id,
            kind,
            deadline: base + Duration::from_millis(delay_ms),
            delay_ms,
            callback: 0,
            args: Vec::new(),
            context: crate::async_context::AsyncContextSnapshot::default(),
            async_id: 0,
            trigger_async_id: 0,
            cleared: false,
        }
    }

    /// #6287 case 1: the batch fires in DEADLINE order, not creation order —
    /// a 5 ms timer created after a 10 ms one still fires first. Ground truth
    /// from node: `setTimeout(f,10); setTimeout(g,5)` runs g then f.
    #[test]
    fn expired_timeouts_fire_in_deadline_order() {
        let base = Instant::now();
        let mut batch = vec![
            timer(1, CallbackTimerKind::Timeout, base, 10),
            timer(2, CallbackTimerKind::Timeout, base, 5),
            timer(3, CallbackTimerKind::Timeout, base, 1),
        ];
        order_expired_callback_batch(&mut batch);
        let ids: Vec<i64> = batch.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![3, 2, 1], "earliest deadline first");
    }

    /// Same-deadline timers must STILL fire in creation order — the ordering
    /// Perry already got right, preserved by the sort being stable.
    #[test]
    fn same_deadline_timeouts_keep_creation_order() {
        let base = Instant::now();
        let mut batch = vec![
            timer(1, CallbackTimerKind::Timeout, base, 3),
            timer(2, CallbackTimerKind::Timeout, base, 3),
            timer(3, CallbackTimerKind::Timeout, base, 3),
        ];
        order_expired_callback_batch(&mut batch);
        let ids: Vec<i64> = batch.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![1, 2, 3], "stable sort keeps creation order");
    }

    /// #6287 case 2: setImmediate runs in the CHECK phase, so an expired
    /// setTimeout fires ahead of an immediate scheduled earlier — and this is
    /// exactly why a naive sort by deadline alone is wrong (an immediate's
    /// deadline is ~now, so it would sort ahead of the timeout). Immediates
    /// keep FIFO order among themselves.
    #[test]
    fn expired_timeouts_precede_immediates_which_stay_fifo() {
        let base = Instant::now();
        let mut batch = vec![
            timer(1, CallbackTimerKind::Immediate, base, 0),
            timer(2, CallbackTimerKind::Timeout, base, 5),
            timer(3, CallbackTimerKind::Immediate, base, 0),
            timer(4, CallbackTimerKind::Timeout, base, 1),
        ];
        order_expired_callback_batch(&mut batch);
        let ids: Vec<i64> = batch.iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            vec![4, 2, 1, 3],
            "timeouts by deadline (4 then 2), then immediates FIFO (1 then 3)"
        );
    }
}
