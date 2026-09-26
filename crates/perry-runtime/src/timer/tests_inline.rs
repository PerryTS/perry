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
            None,
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
            None,
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
                crate::native_class_ids::IMMEDIATE,
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
            let keys = unsafe { crate::object::object_keys(obj).arr() };
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
        // `js_timer_has_pending`, not `js_callback_timer_has_pending`. turnloop
        // P3 split the three liveness entry points across the per-agent store's
        // phases: `js_timer_has_pending` and `js_interval_timer_has_pending`
        // both answer `has_refed_timers()` (the Timeout/Interval classes) while
        // `js_callback_timer_has_pending` answers `has_refed_check()` -- the
        // CHECK phase, i.e. `setImmediate`. The generated loop's liveness
        // disjunction asks all three, so the union is unchanged, but this
        // subject is a `setTimeout` and belongs to the first accessor's
        // question, not the third's.
        assert_eq!(
            js_timer_has_pending(),
            1,
            "setup: the timer must be pending"
        );
        js_clear_timeout_value(handle_value(handle));
        assert_eq!(
            js_timer_has_pending(),
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
        assert!(
            crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(plain as i64))
                .is_none()
        );
        // …and a bare small id, the OLD representation, is not one either.
        assert!(crate::timer::timer_handle_parts(crate::value::js_nanbox_pointer(1)).is_none());
    }
}

/// #10836 follow-up: the first timer of a program must not drag the realm
/// global's bootstrap in with it.
///
/// `build_timer_prototypes` installs `Timeout.prototype` / `Immediate.prototype`
/// with `install_proto_method`, and that install records spec property
/// descriptors. The descriptor bookkeeping asks "is this receiver
/// `Object.prototype`?", which used to be answered by *materializing*
/// `globalThis` — `populate_global_this_builtins`, measured at 4–6 ms by its own
/// `[gc-globalthis-bootstrap]` diagnostic — so the whole bootstrap landed inside
/// the first `setTimeout` call.
///
/// That is not merely slow. A timer's deadline is `now + delay`, taken per call,
/// so 6 ms spent inside call #1 pushes call #2's deadline 6 ms later and a
/// `setTimeout(…, 10)` written before a `setTimeout(…, 5)` fires FIRST — the
/// `test_gap_6287_timer_batch_order` failure that blocked merge train 248.
///
/// The property is only observable on a thread that has not yet built its realm
/// global, which is why the subject runs on its own thread: libtest may run unit
/// tests on a thread earlier tests already used, and `THREAD_GLOBAL_THIS` is
/// per-thread. The precondition is asserted rather than assumed, so a future
/// harness change that shares the thread turns this test RED instead of making
/// its verdict vacuous.
#[cfg(test)]
mod first_timer_cost_tests {
    use super::*;

    #[test]
    fn the_first_timer_handle_does_not_bootstrap_the_realm_global() {
        let _serial = crate::gc::global_side_table_test_lock();
        std::thread::spawn(|| {
            crate::gc::ensure_gc_initialized();
            assert!(
                !crate::object::global_this_is_materialized(),
                "fixture precondition: a fresh thread must start with no realm global, \
                 or every verdict below is vacuous"
            );

            // The subject: the allocator the `js_set_*` entry points call, on the
            // first timer of this realm — so it is the call that builds both
            // prototypes.
            let handle = timer_object(1, CallbackTimerKind::Timeout);
            assert_ne!(
                handle, 0,
                "the handle must have been built, or nothing was measured"
            );
            assert_ne!(
                handle_object::TIMEOUT_PROTOTYPE_PTR.load(Ordering::Acquire),
                0,
                "`Timeout.prototype` must have been installed by that call, or the \
                 verdict below is about a path that never ran"
            );

            assert!(
                !crate::object::global_this_is_materialized(),
                "building the timer prototypes materialized `globalThis`: \
                 `populate_global_this_builtins` (~5 ms) now runs inside the first \
                 `setTimeout`, which moves the second timer's deadline and reorders \
                 the batch (#10836)"
            );
        })
        .join()
        .expect("the probe thread must not panic");
    }
}

/// #10522: `t.unref()` & co. skip the dispatch tower only while the call
/// provably resolves to the family's own native method. Each override a
/// program can make must send the call back to the tower, or the fast path
/// would run the native method where JS says something else runs.
#[cfg(test)]
mod method_fast_path_tests {
    use super::*;
    use crate::timer::try_timer_method_fast_dispatch as fast;

    fn key(name: &str) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    /// Timer prototypes are per-thread singletons, and a test that mutates one
    /// must not leak that into later tests on a reused thread.
    fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
        let _serial = crate::gc::global_side_table_test_lock();
        std::thread::spawn(move || {
            crate::gc::ensure_gc_initialized();
            body();
        })
        .join()
        .expect("the probe thread must not panic");
    }

    fn new_timeout() -> (f64, i64) {
        let value = crate::value::js_nanbox_pointer(js_set_timeout_callback(0, 50_000.0));
        let (id, _) = crate::timer::timer_handle_parts(value).expect("branded handle");
        (value, id)
    }

    fn timeout_proto() -> *mut crate::object::ObjectHeader {
        handle_object::TIMEOUT_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut _
    }

    #[test]
    fn a_pristine_handle_takes_the_fast_path_with_the_thunks_answers() {
        on_fresh_thread(|| unsafe {
            let (t, id) = new_timeout();
            assert_eq!(fast(t, b"unref").map(f64::to_bits), Some(t.to_bits()));
            assert_eq!(js_timer_has_ref(id), 0, "unref() did not take");
            let has_ref = fast(t, b"hasRef").expect("hasRef on the fast path");
            assert_eq!(has_ref.to_bits(), crate::value::JSValue::bool(false).bits());
            assert_eq!(fast(t, b"ref").map(f64::to_bits), Some(t.to_bits()));
            assert_eq!(js_timer_has_ref(id), 1, "ref() did not take");
            assert_eq!(fast(t, b"refresh").map(f64::to_bits), Some(t.to_bits()));
            // Not a fast-path name: the tower answers it.
            assert!(fast(t, b"close").is_none());
            // An Immediate has no `refresh` in node; the tower throws for it.
            let imm = crate::value::js_nanbox_pointer(js_set_immediate_callback(0));
            assert!(fast(imm, b"refresh").is_none());
            assert!(fast(imm, b"unref").is_some());
            // And the generic entry point reaches it: `t.unref()` as emitted.
            let result = crate::object::js_native_call_method(
                t,
                b"unref".as_ptr() as *const i8,
                5,
                std::ptr::null(),
                0,
            );
            assert_eq!(result.to_bits(), t.to_bits());
            assert_eq!(js_timer_has_ref(id), 0);
            clearTimeout(id);
        });
    }

    #[test]
    fn an_own_property_on_the_handle_is_not_bypassed() {
        on_fresh_thread(|| unsafe {
            let (t, id) = new_timeout();
            let obj =
                (t.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
            crate::object::js_object_set_field_by_name(obj, key("unref"), 1.0);
            assert!(
                fast(t, b"unref").is_none(),
                "an own `unref` shadows the prototype"
            );
            // Any own key at all leaves the pristine shape; stay conservative.
            let (t2, id2) = new_timeout();
            let obj2 =
                (t2.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
            crate::object::js_object_set_field_by_name(obj2, key("tag"), 1.0);
            assert!(fast(t2, b"unref").is_none());
            clearTimeout(id);
            clearTimeout(id2);
        });
    }

    #[test]
    fn a_replaced_prototype_method_is_not_bypassed() {
        on_fresh_thread(|| unsafe {
            let (t, id) = new_timeout();
            let proto = timeout_proto();
            let original = crate::object::js_object_get_field_by_name(proto, key("unref"));
            let ref_method = crate::object::js_object_get_field_by_name(proto, key("ref"));
            // `Timeout.prototype.unref = Timeout.prototype.ref`: a closure, and
            // a native one, but not THIS method's thunk.
            crate::object::js_object_set_field_by_name(
                proto,
                key("unref"),
                f64::from_bits(ref_method.bits()),
            );
            assert!(fast(t, b"unref").is_none());
            crate::object::js_object_set_field_by_name(
                proto,
                key("unref"),
                f64::from_bits(original.bits()),
            );
            assert!(
                fast(t, b"unref").is_some(),
                "restoring the method restores the path"
            );
            clearTimeout(id);
        });
    }

    #[test]
    fn a_prototype_getter_is_not_bypassed() {
        on_fresh_thread(|| unsafe {
            let (t, id) = new_timeout();
            let proto = timeout_proto();
            let getter = crate::object::js_object_get_field_by_name(proto, key("ref"));
            crate::object::js_object_define_getter(
                crate::value::js_nanbox_pointer(proto as i64),
                crate::value::js_nanbox_string(key("unref") as i64),
                f64::from_bits(getter.bits()),
            );
            assert!(
                fast(t, b"unref").is_none(),
                "an accessor must run, not the thunk"
            );
            clearTimeout(id);
        });
    }

    #[test]
    fn a_replaced_handle_prototype_is_not_bypassed() {
        on_fresh_thread(|| unsafe {
            let (t, id) = new_timeout();
            let other = crate::object::js_object_alloc(0, 0);
            crate::object::prototype_chain::object_set_user_prototype(
                (t.to_bits() & crate::value::POINTER_MASK) as usize,
                crate::value::js_nanbox_pointer(other as i64).to_bits(),
            );
            assert!(fast(t, b"unref").is_none());
            clearTimeout(id);
        });
    }
}
