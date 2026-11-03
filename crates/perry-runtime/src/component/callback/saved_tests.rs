//! A second poll must not require the first poll's frame or public Promise.
use super::*;
use crate::{async_context as als, value::js_nanbox_pointer};

#[test]
fn saved_invocation_context_outlives_poll_frames_and_public_promise() {
    for observed in [false, true] {
        check_saved(observed);
    }
}

fn check_saved(observed: bool) {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let token = als::AsyncLocalStoragePayload::default();
    let weak_promise = scope.root_nanbox_f64(undefined());
    let weak_store = scope.root_nanbox_f64(undefined());
    let owner = scope.root_nanbox_f64(context::publish(serde_json::json!({}), undefined()));
    let ids;
    let original_id;
    {
        let temporary = RuntimeHandleScope::new();
        let store = temporary.root_nanbox_f64(js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        als::set_store(token.token(), store.get_nanbox_f64());
        weak_store.set_nanbox_f64(js_nanbox_pointer(crate::weakref::js_weakref_new(
            store.get_nanbox_f64(),
        ) as i64));
        let promise = temporary.root_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::js_promise_mark_internally_handled(promise.get_raw_mut_ptr());
        let value = js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64);
        if observed {
            crate::async_hooks::init_resource_with_trigger("PROMISE", value, true, 0);
        }
        original_id = unsafe { (*promise.get_raw_mut_ptr::<crate::promise::Promise>()).async_id };
        weak_promise.set_nanbox_f64(js_nanbox_pointer(
            crate::weakref::js_weakref_new(value) as i64
        ));
        let (record, serials) = saved::capture(value);
        ids = serials;
        assert_eq!(
            ids.async_id,
            crate::async_hooks::resource_state::public_id(original_id)
        );
        // The component's ordinary private edge owns this context between
        // polls. No native structure caches the returned managed bits.
        context::set_imports(owner.get_nanbox_f64(), record);
    }
    als::set_store(token.token(), 17.0);
    {
        let poll = RuntimeHandleScope::new();
        let batch = FrameBatch::new(&poll, owner.get_nanbox_f64());
        let frame = batch.for_context(7, context::imports(owner.get_nanbox_f64()), ids);
        assert_eq!(
            frame.run(|| {
                assert_eq!(crate::async_hooks::execution_async_id_u64(), original_id);
                assert_eq!(
                    als::get_store(token.token()).unwrap().to_bits(),
                    crate::weakref::js_weakref_deref(weak_store.get_nanbox_f64()).to_bits()
                );
                31.0
            }),
            Ok(31.0)
        );
    }
    let before = context::imports(owner.get_nanbox_f64()).to_bits();
    // Both the first batch and all its runtime handles have gone away.
    crate::gc::gc_collect_minor();
    crate::gc::js_gc_collect();
    assert_ne!(
        context::imports(owner.get_nanbox_f64()).to_bits(),
        before,
        "saved context must actually move between polls"
    );
    assert!(JSValue::from_bits(
        crate::weakref::js_weakref_deref(weak_promise.get_nanbox_f64()).to_bits()
    )
    .is_undefined());
    assert!(!JSValue::from_bits(
        crate::weakref::js_weakref_deref(weak_store.get_nanbox_f64()).to_bits()
    )
    .is_undefined());
    assert_eq!(
        crate::async_hooks::resource_state::get(original_id).is_some(),
        observed
    );
    {
        let poll = RuntimeHandleScope::new();
        let batch = FrameBatch::new(&poll, owner.get_nanbox_f64());
        let frame = batch.for_context(7, context::imports(owner.get_nanbox_f64()), ids);
        assert_eq!(
            frame.run(|| {
                assert_eq!(crate::async_hooks::execution_async_id_u64(), original_id);
                crate::gc::gc_collect_minor();
                assert_eq!(
                    als::get_store(token.token()).unwrap().to_bits(),
                    crate::weakref::js_weakref_deref(weak_store.get_nanbox_f64()).to_bits()
                );
                crate::exception::js_throw(73.0);
            }),
            Err(73.0)
        );
    }
    assert_eq!(als::get_store(token.token()), Some(17.0));
    assert_eq!(crate::async_hooks::execution_async_id_u64(), 0);
    context::set_imports(owner.get_nanbox_f64(), undefined());
    als::clear_store(token.token());
    crate::async_hooks::reset_for_tests();
}
