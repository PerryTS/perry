//! The async generator record lives in the object graph only: a suspended
//! generator stays alive exactly while something that can resume it (a
//! pending step promise's reaction, a queued request) is reachable.
use super::*;
use crate::closure::{js_closure_alloc, JsThis};
use crate::value::TAG_UNDEFINED;

crate::perry_thread_local! {
    /// The promise the first step is suspended on (the "awaited" promise a
    /// real body's step chain waits for); a registered global root.
    static AWAITED: std::cell::Cell<u64> = const { std::cell::Cell::new(TAG_UNDEFINED) };
    static STEPS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// First call: suspend on a fresh pending promise. Later calls: complete.
extern "C" fn suspend_then_finish(_: *const ClosureHeader, _: JsThis, _: f64, _: f64) -> f64 {
    let n = STEPS.with(|s| {
        s.set(s.get() + 1);
        s.get()
    });
    let promise = js_promise_new();
    if n == 1 {
        let bits = js_nanbox_pointer(promise as i64);
        AWAITED.with(|a| a.set(bits.to_bits()));
    } else {
        let done = unsafe {
            crate::iter_result::make_iter_result(JSValue::from_bits(TAG_UNDEFINED), true)
        };
        crate::promise::js_promise_resolve(promise, done);
    }
    js_nanbox_pointer(promise as i64)
}

/// Every call suspends on a pending promise nothing else holds.
extern "C" fn suspend_forever(_: *const ClosureHeader, _: JsThis, _: f64, _: f64) -> f64 {
    js_nanbox_pointer(js_promise_new() as i64)
}

fn born(info: *const crate::closure::JsFunctionInfo) -> f64 {
    let closure = js_closure_alloc(info, 0);
    js_async_generator_create(js_nanbox_pointer(closure as i64), std::ptr::null())
}

fn record_value(instance: f64) -> f64 {
    js_nanbox_pointer(async_generator_record(instance).expect("an async generator") as i64)
}

fn weak(value: f64) -> f64 {
    js_nanbox_pointer(crate::weakref::js_weakref_new(value) as i64)
}

fn alive(weak: f64) -> bool {
    crate::weakref::js_weakref_deref(weak).to_bits() != TAG_UNDEFINED
}

fn full_collections() {
    for _ in 0..3 {
        crate::gc::js_gc_collect();
    }
}

#[test]
fn suspended_generator_held_by_its_pending_step_stays_alive() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        AWAITED.with(|a| crate::gc::js_gc_register_global_root(a.as_ptr() as i64));
        let scope = crate::gc::RuntimeHandleScope::new();
        let (weak_record, queued) = {
            let inner = crate::gc::RuntimeHandleScope::new();
            let instance = inner.root_nanbox_f64(born(crate::fn_info!(suspend_then_finish, 2)));
            let record = record_value(instance.get_nanbox_f64());
            let weak_record = inner.root_nanbox_f64(weak(record));
            let arg = f64::from_bits(TAG_UNDEFINED);
            // The first request suspends the body; the second waits in the
            // record's queue. The consumer keeps only the queued promise.
            async_generator_request(
                async_generator_record(instance.get_nanbox_f64()).unwrap(),
                OP_NEXT,
                arg,
            );
            let queued = inner.root_nanbox_f64(async_generator_request(
                async_generator_record(instance.get_nanbox_f64()).unwrap(),
                OP_NEXT,
                arg,
            ));
            (weak_record.get_nanbox_f64(), queued.get_nanbox_f64())
        };
        // Nothing allocates between the inner scope's end and these roots.
        let weak_record = scope.root_nanbox_f64(weak_record);
        let queued = scope.root_nanbox_f64(queued);
        full_collections();
        assert!(
            alive(weak_record.get_nanbox_f64()),
            "a generator suspended on a reachable promise must stay alive"
        );
        let awaited = AWAITED.with(|a| a.get());
        let first = unsafe {
            crate::iter_result::make_iter_result(JSValue::from_bits(TAG_UNDEFINED), false)
        };
        crate::promise::js_promise_resolve(
            js_nanbox_get_pointer(f64::from_bits(awaited)) as *mut Promise,
            first,
        );
        crate::promise::js_promise_run_microtasks();
        let queued = js_nanbox_get_pointer(queued.get_nanbox_f64()) as *mut Promise;
        assert_eq!(
            crate::promise::js_promise_state(queued),
            1,
            "the surviving generator serves its queued request"
        );
        assert_eq!(STEPS.with(|s| s.get()), 2);
    })
    .join()
    .unwrap();
}

#[test]
fn abandoned_generators_are_collected() {
    std::thread::spawn(|| {
        crate::gc::gc_init();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        let scope = crate::gc::RuntimeHandleScope::new();
        let weaks = {
            let inner = crate::gc::RuntimeHandleScope::new();
            let unstarted = inner.root_nanbox_f64(born(crate::fn_info!(suspend_forever, 2)));
            let weak_unstarted = inner.root_nanbox_f64(weak(unstarted.get_nanbox_f64()));
            let suspended = inner.root_nanbox_f64(born(crate::fn_info!(suspend_forever, 2)));
            let weak_suspended = inner.root_nanbox_f64(weak(suspended.get_nanbox_f64()));
            let weak_record = inner.root_nanbox_f64(weak(record_value(suspended.get_nanbox_f64())));
            let arg = f64::from_bits(TAG_UNDEFINED);
            async_generator_request(
                async_generator_record(suspended.get_nanbox_f64()).unwrap(),
                OP_NEXT,
                arg,
            );
            async_generator_request(
                async_generator_record(suspended.get_nanbox_f64()).unwrap(),
                OP_NEXT,
                arg,
            );
            [
                weak_unstarted.get_nanbox_f64(),
                weak_suspended.get_nanbox_f64(),
                weak_record.get_nanbox_f64(),
            ]
        };
        // Nothing allocates between the inner scope's end and these roots.
        let weak_unstarted = scope.root_nanbox_f64(weaks[0]);
        let weak_suspended = scope.root_nanbox_f64(weaks[1]);
        let weak_record = scope.root_nanbox_f64(weaks[2]);
        full_collections();
        assert!(
            !alive(weak_unstarted.get_nanbox_f64()),
            "an unstarted generator is collected"
        );
        assert!(
            !alive(weak_suspended.get_nanbox_f64()),
            "a generator suspended on an unreachable promise is collected"
        );
        assert!(
            !alive(weak_record.get_nanbox_f64()),
            "its record and queue are collected with it"
        );
    })
    .join()
    .unwrap();
}
