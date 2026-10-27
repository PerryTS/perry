use super::*;
use crate::{async_context as als, value::js_nanbox_pointer};

#[test]
fn late_origin_frames_keep_context_and_errors_after_callback_scope_unwinds() {
    for observed in [false, true] {
        check_batch(observed);
    }
}

fn check_batch(observed: bool) {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let token = als::AsyncLocalStoragePayload::default();
    let weak_first = scope.root_nanbox_f64(undefined());
    let weak_second = scope.root_nanbox_f64(undefined());
    let weak_error = scope.root_nanbox_f64(undefined());
    let owner = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64
    ));
    let batch = FrameBatch::new(&scope, owner.get_nanbox_f64());
    let first;
    let first_id;
    {
        let temporary = RuntimeHandleScope::new();
        als::set_store(token.token(), 11.0);
        let promise = temporary.root_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::js_promise_mark_internally_handled(promise.get_raw_mut_ptr());
        let value = js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64);
        if observed {
            crate::async_hooks::init_resource_with_trigger("PROMISE", value, true, 0);
        }
        first_id = unsafe { (*promise.get_raw_mut_ptr::<crate::promise::Promise>()).async_id };
        weak_first.set_nanbox_f64(js_nanbox_pointer(
            crate::weakref::js_weakref_new(value) as i64
        ));
        first = batch.for_promise(1, value);
    }
    let token_before = first.token();
    let snapshot_before = first.snapshot.get_nanbox_f64().to_bits();
    let owner_before = first.owner.get_nanbox_f64().to_bits();
    als::set_store(token.token(), 17.0);
    assert_eq!(
        first.run(|| {
            assert_eq!(als::get_store(token.token()), Some(11.0));
            assert_eq!(crate::async_hooks::execution_async_id_u64(), first_id);
            // These frames are constructed under both the async scope and an
            // exception landing pad. Their roots must not be slots in either.
            assert_eq!(
                crate::exception::catch_js_throw(|| {
                    let temporary = RuntimeHandleScope::new();
                    for origin in 2..66 {
                        als::set_store(token.token(), origin as f64);
                        let promise = temporary.root_raw_mut_ptr(crate::promise::js_promise_new());
                        crate::promise::js_promise_mark_internally_handled(
                            promise.get_raw_mut_ptr(),
                        );
                        let value = js_nanbox_pointer(
                            promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
                        );
                        if observed {
                            crate::async_hooks::init_resource_with_trigger(
                                "PROMISE", value, true, 0,
                            );
                        }
                        let late = batch.for_promise(origin, value);
                        assert_ne!(late.token(), token_before);
                        if origin == 2 {
                            weak_second.set_nanbox_f64(js_nanbox_pointer(
                                crate::weakref::js_weakref_new(value) as i64,
                            ));
                            let error = temporary.root_nanbox_f64(js_nanbox_pointer(
                                crate::object::js_object_alloc(0, 0) as i64,
                            ));
                            weak_error.set_nanbox_f64(js_nanbox_pointer(
                                crate::weakref::js_weakref_new(error.get_nanbox_f64()) as i64,
                            ));
                            late.capture(error.get_nanbox_f64());
                            late.capture(99.0);
                        }
                    }
                    crate::exception::js_throw(777.0);
                }),
                Err(777.0)
            );
            als::restore_context(first.context());
            crate::gc::gc_collect_minor();
            assert_eq!(als::get_store(token.token()), Some(11.0));
            let second = batch.for_promise(2, undefined());
            assert!(second.threw.get());
            assert_eq!(
                second.error.get_nanbox_f64().to_bits(),
                crate::weakref::js_weakref_deref(weak_error.get_nanbox_f64()).to_bits()
            );
            assert_eq!(
                second.run(|| {
                    assert_eq!(als::get_store(token.token()), Some(2.0));
                    crate::gc::gc_collect_minor();
                    crate::exception::js_throw(888.0);
                }),
                Err(888.0)
            );
            assert_eq!(als::get_store(token.token()), Some(11.0));
            first.capture(42.0);
            first.capture(43.0);
            crate::exception::js_throw(555.0);
        }),
        Err(555.0)
    );
    assert_eq!(als::get_store(token.token()), Some(17.0));
    let unretained_metadata;
    {
        let temporary = RuntimeHandleScope::new();
        let promise = temporary.root_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::js_promise_mark_internally_handled(promise.get_raw_mut_ptr());
        crate::async_hooks::init_resource_with_trigger(
            "PROMISE",
            js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64),
            true,
            0,
        );
        unretained_metadata =
            unsafe { (*promise.get_raw_mut_ptr::<crate::promise::Promise>()).async_id };
        assert!(crate::async_hooks::resource_state::get(unretained_metadata).is_some());
    }
    crate::gc::js_gc_collect();
    assert!(
        crate::async_hooks::resource_state::get(unretained_metadata).is_none(),
        "full collection must actually reclaim unrooted old metadata"
    );
    assert_eq!(
        crate::async_hooks::resource_state::get(first_id).is_some(),
        observed,
        "the frame must retain observed metadata after its Promise dies"
    );
    assert_eq!(
        first.run(|| {
            assert_eq!(als::get_store(token.token()), Some(11.0));
            assert_eq!(crate::async_hooks::execution_async_id_u64(), first_id);
            44.0
        }),
        Ok(44.0)
    );
    assert_eq!(batch.for_promise(1, undefined()).token(), token_before);
    assert_eq!(first.error.get_nanbox_f64(), 42.0);
    assert_ne!(
        first.snapshot.get_nanbox_f64().to_bits(),
        snapshot_before,
        "snapshot must actually move"
    );
    assert_ne!(
        first.owner.get_nanbox_f64().to_bits(),
        owner_before,
        "frame owner must actually move"
    );
    for weak in [weak_first, weak_second] {
        assert!(
            JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
                .is_undefined(),
            "frames must not retain public Promises"
        );
    }
    assert_eq!(
        first.run_cleanup(|| {
            assert_eq!(als::get_store(token.token()), Some(11.0));
            crate::gc::gc_collect_minor();
            crate::exception::js_throw(999.0);
        }),
        Err(999.0)
    );
    assert_eq!(als::get_store(token.token()), Some(17.0));
    assert_eq!(crate::async_hooks::execution_async_id_u64(), 0);
    assert_eq!(first.error.get_nanbox_f64(), 42.0);
    als::clear_store(token.token());
    crate::async_hooks::reset_for_tests();
}
