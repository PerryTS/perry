//! Inline unit tests extracted from `timer.rs` (#8354 follow-up to #8328).
//!
//! `timer.rs` crossed the 2000-line cap enforced by
//! `scripts/check_file_size.sh` at 2001 lines. These are the same tests,
//! moved verbatim; the `#[path]` + `mod` declaration at the end of
//! `timer.rs` keeps them in the `crate::timer` module so every `super::`
//! and private-item reference still resolves.

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
        _scheduled: ref_states::ScheduledTimerId::unregistered(),
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
        async_id: 0,
        trigger_async_id: 0,
        cleared: false,
        _scheduled: ref_states::ScheduledTimerId::unregistered(),
    });
}

#[cfg(test)]
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
mod drain_expired_tests;

#[cfg(test)]
mod expired_batch_order_tests {
    use super::{order_expired_callback_batch, CallbackTimer, CallbackTimerKind};
    use std::time::{Duration, Instant};

    fn timer(id: i64, kind: CallbackTimerKind, base: Instant, delay_ms: u64) -> CallbackTimer {
        CallbackTimer {
            // #6185: test scaffolding runs on the primary agent.
            owner: crate::agent::current_agent(),
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
            _scheduled: crate::timer::ref_states::ScheduledTimerId::unregistered(),
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

#[cfg(test)]
mod mock_dispatch_own_pin_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    static SELF_ID: AtomicI64 = AtomicI64::new(0);
    static SAW_KNOWN: AtomicBool = AtomicBool::new(false);
    static SAW_HAS_REF: AtomicBool = AtomicBool::new(false);
    static RAN: AtomicBool = AtomicBool::new(false);

    /// A one-shot mock timer's own callback: churns more real one-shot timers
    /// than the registry's eviction cap, then checks its OWN id. If this
    /// timer's `_scheduled` pin already retired the moment it was popped off
    /// the mock queue for dispatch (the bug), it is the OLDEST retired id in
    /// the shared registry when the churn starts, so it is the very first one
    /// evicted once the churn passes the cap — and this callback observes its
    /// own eviction while it is still running.
    extern "C" fn churn_then_check_self(_closure: *const crate::closure::ClosureHeader) -> f64 {
        let id = SELF_ID.load(Ordering::SeqCst);
        for _ in 0..(ref_states::TIMER_REF_STATES_CAP + 2_000) {
            // #340/#341: the producer returns the handle object; `clearTimeout`
            // takes the id it carries.
            let handle = js_set_timeout_callback(0, 1_000.0);
            clearTimeout(
                crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(handle))
                    .expect("a timer producer must return a branded handle")
                    .0,
            );
        }
        SAW_KNOWN.store(is_known_timer_id(id), Ordering::SeqCst);
        SAW_HAS_REF.store(js_timer_has_ref(id) != 0, Ordering::SeqCst);
        RAN.store(true, Ordering::SeqCst);
        0.0
    }

    /// #10447 follow-up: `mock_timers_advance_to` used to pop a one-shot mock
    /// timer off the queue with `state.callbacks.remove(idx)` and destructure
    /// out `(id, callback, args, context)` — leaving the popped entry's
    /// `_scheduled: ScheduledTimerId` behind to drop, and retire the id, right
    /// there, before `call_timer_callback` had even run, let alone finished.
    /// A callback that then churned more timers than the eviction cap evicted
    /// its OWN handle mid-dispatch. The fix carries the pin into the dispatch
    /// action and drops it only after the callback returns.
    #[test]
    fn a_one_shot_mock_timers_own_pin_survives_its_own_dispatch() {
        let _serial = crate::gc::global_side_table_test_lock();
        SAW_KNOWN.store(false, Ordering::SeqCst);
        SAW_HAS_REF.store(false, Ordering::SeqCst);
        RAN.store(false, Ordering::SeqCst);
        js_mock_timers_reset();
        js_mock_timers_enable(MOCK_TIMERS_API_SET_TIMEOUT, 0.0);

        let closure = crate::closure::js_closure_alloc(churn_then_check_self as *const u8, 0);
        let id = schedule_mock_callback_timer(
            closure as i64,
            10.0,
            Vec::new(),
            CallbackTimerKind::Timeout,
        )
        .expect("mock setTimeout must be enabled for this API set");
        SELF_ID.store(id, Ordering::SeqCst);

        js_mock_timers_tick(10.0);

        assert!(RAN.load(Ordering::SeqCst), "the mock timer never fired");
        assert!(
            SAW_KNOWN.load(Ordering::SeqCst),
            "timer {id} was evicted from the registry by its own callback's churn"
        );
        assert!(
            SAW_HAS_REF.load(Ordering::SeqCst),
            "timer {id}'s ref state was lost to its own callback's churn"
        );
        js_mock_timers_reset();
    }
}

#[cfg(test)]
mod refresh_and_immediate_primitive_tests {
    use super::*;

    /// #340/#341: a producer hands back the JS-visible handle OBJECT while the
    /// `js_timer_*` entry points below still speak registry ids. Resolving once
    /// here keeps these tests about what they were about (ref state, eviction,
    /// kind) instead of about the representation.
    fn handle_id(handle: i64) -> i64 {
        crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(handle))
            .expect("a timer producer must return a branded handle")
            .0
    }

    /// #10541: `refresh()` reschedules a timer but must not touch its ref
    /// state -- neither re-ref an unref'd timer/interval nor unref a ref'd
    /// one. Before the fix `js_timer_refresh` unconditionally called
    /// `set_timer_ref_state(id, true)`.
    #[test]
    fn refresh_preserves_ref_state() {
        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();

        let unrefd = handle_id(js_set_timeout_callback(0, 50_000.0));
        js_timer_unref(unrefd);
        assert_eq!(js_timer_has_ref(unrefd), 0, "setup: unref() didn't take");
        js_timer_refresh(unrefd);
        assert_eq!(
            js_timer_has_ref(unrefd),
            0,
            "refresh() re-ref'd an unref'd timeout"
        );

        let refd = handle_id(js_set_timeout_callback(0, 50_000.0));
        assert_eq!(js_timer_has_ref(refd), 1, "setup: new timer isn't ref'd");
        js_timer_refresh(refd);
        assert_eq!(
            js_timer_has_ref(refd),
            1,
            "refresh() unref'd a ref'd timeout"
        );

        let unrefd_interval = handle_id(setInterval(0, 50_000.0));
        js_timer_unref(unrefd_interval);
        js_timer_refresh(unrefd_interval);
        assert_eq!(
            js_timer_has_ref(unrefd_interval),
            0,
            "refresh() re-ref'd an unref'd interval"
        );

        clearTimeout(unrefd);
        clearTimeout(refd);
        clearInterval(unrefd_interval);
    }

    /// #10542 / #340 / #341: a `setImmediate` handle is distinguished from a
    /// `setTimeout`/`setInterval` handle by kind. The kind used to live in the
    /// id-keyed ref-state registry so `js_number_coerce` could gate a
    /// Timeout-only numeric shortcut on it; it is a bit in the handle OBJECT's
    /// own state word now, and that shortcut is gone — `Symbol.toPrimitive` is
    /// installed on `Timeout.prototype` only, which is how node draws the same
    /// line. The distinction still has to be readable off the handle, and this
    /// is where that is asserted.
    #[test]
    fn immediate_kind_is_distinguished_from_timeout() {
        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();

        let timeout = js_set_timeout_callback(0, 50_000.0);
        let interval = setInterval(0, 50_000.0);
        let immediate = js_set_immediate_callback(0);

        let is_immediate = |handle: i64| {
            crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(handle))
                .expect("a timer producer must return a branded handle")
                .1
        };
        assert!(!is_immediate(timeout), "setTimeout is a Timeout");
        assert!(!is_immediate(interval), "setInterval is a Timeout");
        assert!(is_immediate(immediate), "setImmediate is an Immediate");

        clearTimeout(handle_id(timeout));
        clearInterval(handle_id(interval));
        clearImmediate(handle_id(immediate));
    }
}


/// #340/#341 — the representation itself. Everything else in this file is
/// about timer behaviour; these are about what a timer handle IS.
#[cfg(test)]
mod honest_tag_tests {
    use super::*;

    fn handle_value(handle: i64) -> f64 {
        crate::value::js_nanbox_pointer(handle)
    }

    /// GATE B, and the invariant the whole migration is for: the value JS
    /// receives is a real heap object with the family class id, ABOVE the
    /// small-handle band, carrying zero own keys. The band assertion is what
    /// covers statically lowered reads, which the receiver-repr ledger (gate A)
    /// cannot see.
    #[test]
    fn a_timer_handle_is_an_ordinary_object_outside_the_handle_band() {
        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();

        for (handle, class_id, immediate) in [
            (
                js_set_timeout_callback(0, 50_000.0),
                crate::timer::TIMEOUT_CLASS_ID,
                false,
            ),
            (
                setInterval(0, 50_000.0),
                crate::timer::TIMEOUT_CLASS_ID,
                false,
            ),
            (
                js_set_immediate_callback(0),
                crate::timer::IMMEDIATE_CLASS_ID,
                true,
            ),
        ] {
            let addr = handle as usize;
            assert!(
                !crate::value::addr_class::is_handle_band(addr),
                "gate B: a producer handed back a small band id ({addr:#x})"
            );
            let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }
                .expect("a timer handle carries a GcHeader");
            assert_eq!(header.obj_type, crate::gc::GC_TYPE_OBJECT);
            let obj = addr as *mut crate::object::ObjectHeader;
            assert_eq!(unsafe { (*obj).class_id }, class_id);
            let keys = unsafe { crate::object::object_keys_array(obj) };
            let key_count = if keys.is_null() {
                0
            } else {
                unsafe { (*keys).length }
            };
            assert_eq!(key_count, 0, "a timer handle must have no own keys");
            let (id, is_immediate) = crate::timer::timer_handle_parts(handle_value(handle))
                .expect("a timer handle must be branded");
            assert_eq!(is_immediate, immediate);
            assert!(is_known_timer_id(id), "the handle must name a live timer");
            clearTimeout(id);
            clearInterval(id);
            clearImmediate(id);
        }
    }

    /// Two timers are two objects, and each names its own id. Under the old
    /// representation this held by accident (two ids are two values); it holds
    /// by construction now, and it is what `Map`/`Set`/`WeakMap` keys need.
    #[test]
    fn two_timers_are_two_objects() {
        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();

        let a = js_set_timeout_callback(0, 50_000.0);
        let b = js_set_timeout_callback(0, 50_000.0);
        assert_ne!(a, b, "two constructions must be two objects");
        let (ida, _) = crate::timer::timer_handle_parts(handle_value(a)).expect("branded");
        let (idb, _) = crate::timer::timer_handle_parts(handle_value(b)).expect("branded");
        assert_ne!(ida, idb, "two timers must be two ids");
        clearTimeout(ida);
        clearTimeout(idb);
    }

    /// `clearTimeout(t)` takes the HANDLE — the dual-accepting resolver in
    /// `arg_to_timer_id`. Asserted by behaviour (the timer stops being
    /// pending), not by the resolver's return value.
    #[test]
    fn clear_accepts_the_handle_object() {
        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();

        let handle = js_set_timeout_callback(0, 50_000.0);
        assert_eq!(
            js_callback_timer_has_pending(),
            1,
            "setup: the timer must be pending"
        );
        js_clear_timeout_value(handle_value(handle));
        assert_eq!(
            js_callback_timer_has_pending(),
            0,
            "clearTimeout(handleObject) did not clear the timer"
        );
    }

    /// The COMPILED read path. An emitted `t.unref` value read is a per-site
    /// inline cache whose miss edge calls `js_object_get_field_ic_slow` with the
    /// receiver's 48-bit payload — not `js_object_get_field_by_name`. The
    /// prototype method has to resolve through THAT entry too; measured on the
    /// text family, a program whose reads took this edge answered `undefined`
    /// while every by-name test passed.
    #[test]
    fn a_method_value_read_resolves_through_the_ic_miss_entry() {
        use crate::object::{PicCache, PicCacheSlot, PIC_CACHE_WORDS};
        use std::sync::atomic::AtomicU64;

        let _serial = crate::gc::global_side_table_test_lock();
        test_clear_all_timer_scanner_roots();

        let handle = js_set_timeout_callback(0, 50_000.0);
        for name in ["ref", "unref", "hasRef", "refresh", "close"] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let mut cache: PicCache = [0; PIC_CACHE_WORDS];
            let mut slot: PicCacheSlot = &mut cache;
            let packed = AtomicU64::new(0);
            let value = crate::object::js_object_get_field_ic_slow(
                (handle as u64 & crate::value::POINTER_MASK) as i64,
                key,
                &mut slot,
                &packed,
            );
            let ptr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
            assert!(
                crate::value::JSValue::from_bits(value.to_bits()).is_pointer()
                    && crate::closure::is_closure_ptr(ptr),
                "{name}: the IC miss edge must answer the prototype method, got {:#018x}",
                value.to_bits()
            );
        }
        let (id, _) = crate::timer::timer_handle_parts(handle_value(handle)).expect("branded");
        clearTimeout(id);
    }

    /// A foreign receiver is refused rather than misread — the brand check the
    /// prototype thunks make, matching node's
    /// `Timeout.prototype.unref.call({})`.
    #[test]
    fn a_foreign_receiver_is_not_a_timer_handle() {
        let _serial = crate::gc::global_side_table_test_lock();
        let plain = crate::object::js_object_alloc(0, 0);
        assert!(crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(plain as i64))
            .is_none());
        // …and a bare small id, the OLD representation, is not one either.
        assert!(crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(1)).is_none());
    }
}
