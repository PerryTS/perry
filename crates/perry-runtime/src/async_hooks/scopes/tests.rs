use super::*;
use crate::{async_context, gc::RuntimeHandleScope, value::JSValue};

#[test]
fn captured_promise_context_survives_promise_collection_and_restores_after_a_throw() {
    for metadata in [false, true] {
        check_context(metadata);
    }
}

fn check_context(with_metadata: bool) {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let token = async_context::AsyncLocalStoragePayload::default();
    let snapshot =
        scope.root_raw_mut_ptr::<async_context::AsyncSnapshotHolder>(std::ptr::null_mut());
    // AsyncResourceIds can be opaque GC-cell tokens when observed. Keep that
    // cell in traced state independently of the Promise; it owns a weak edge.
    let metadata = scope.root_nanbox_f64(TAG_UNDEFINED_F64);
    let weak_promise = scope.root_nanbox_f64(TAG_UNDEFINED_F64);
    let weak_store = scope.root_nanbox_f64(TAG_UNDEFINED_F64);
    let ids;
    {
        let temporary = RuntimeHandleScope::new();
        let store = temporary.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        weak_store.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(store.get_nanbox_f64()) as i64,
        ));
        async_context::set_store(token.token(), store.get_nanbox_f64());
        let promise = temporary.root_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::js_promise_mark_internally_handled(promise.get_raw_mut_ptr());
        if with_metadata {
            crate::async_hooks::init_resource_with_trigger(
                "PROMISE",
                crate::value::js_nanbox_pointer(
                    promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
                ),
                true,
                0,
            );
        }
        ids = unsafe {
            AsyncResourceIds {
                async_id: (*promise.get_raw_mut_ptr::<crate::promise::Promise>()).async_id,
                trigger_async_id: (*promise.get_raw_mut_ptr::<crate::promise::Promise>())
                    .trigger_async_id,
            }
        };
        assert_ne!(ids.async_id, 0);
        assert_eq!(
            crate::async_hooks::resource_tracked_for_test(ids.async_id),
            with_metadata
        );
        if with_metadata {
            metadata.set_nanbox_f64(crate::value::js_nanbox_pointer(ids.async_id as i64));
        }
        snapshot.set_raw_mut_ptr(async_context::alloc_snapshot_holder(
            crate::promise::context_for_promise(promise.get_raw_mut_ptr()),
        ));
        weak_promise.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(crate::value::js_nanbox_pointer(
                promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
            )) as i64,
        ));
    }
    async_context::set_store(token.token(), 17.0);
    let old_snapshot = snapshot.get_raw_mut_ptr::<async_context::AsyncSnapshotHolder>() as usize;
    crate::gc::gc_collect_minor();
    assert_ne!(
        snapshot.get_raw_mut_ptr::<async_context::AsyncSnapshotHolder>() as usize,
        old_snapshot,
        "the captured context holder must actually move"
    );
    assert!(
        JSValue::from_bits(
            crate::weakref::js_weakref_deref(weak_promise.get_nanbox_f64()).to_bits()
        )
        .is_undefined(),
        "the public Promise must be collectible independently of its background context"
    );
    let expected = scope.root_nanbox_f64(crate::weakref::js_weakref_deref(
        weak_store.get_nanbox_f64(),
    ));
    assert!(!JSValue::from_bits(expected.get_nanbox_f64().to_bits()).is_undefined());
    let previous = (
        crate::async_hooks::execution_async_id_u64(),
        crate::async_hooks::trigger_async_id_u64(),
    );
    for reject in [false, true] {
        let captured = unsafe {
            (*snapshot.get_raw_mut_ptr::<async_context::AsyncSnapshotHolder>())
                .snapshot
                .clone()
        };
        let result = crate::async_hooks::try_run_captured_promise_scope(ids, captured, || {
            assert_eq!(crate::async_hooks::execution_async_id_u64(), ids.async_id);
            assert_eq!(
                crate::async_hooks::trigger_async_id_u64(),
                ids.trigger_async_id
            );
            assert_eq!(
                async_context::get_store(token.token()).unwrap().to_bits(),
                expected.get_nanbox_f64().to_bits()
            );
            crate::gc::gc_collect_minor();
            assert_eq!(
                async_context::get_store(token.token()).unwrap().to_bits(),
                expected.get_nanbox_f64().to_bits()
            );
            if reject {
                crate::exception::js_throw(777.0);
            }
            33.0
        });
        assert_eq!(result, if reject { Err(777.0) } else { Ok(33.0) });
        assert_eq!(async_context::get_store(token.token()), Some(17.0));
        assert_eq!(
            (
                crate::async_hooks::execution_async_id_u64(),
                crate::async_hooks::trigger_async_id_u64()
            ),
            previous
        );
    }
    async_context::clear_store(token.token());
    crate::async_hooks::reset_for_tests();
}
