use super::*;
use serde_json::json;

#[test]
fn a_live_borrow_keeps_its_owner_and_collection_queues_only_identity_after_moving_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(json!({}), undefined()));
    let borrowed = scope.root_nanbox_f64(undefined());
    let weak = scope.root_nanbox_f64(undefined());
    let original;
    {
        let temporary = RuntimeHandleScope::new();
        let resource =
            temporary.root_nanbox_f64(resource::wrap(owner.get_nanbox_f64(), "test:1", true));
        original = resource.get_nanbox_f64().to_bits();
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(resource.get_nanbox_f64()) as i64,
        ));
        borrowed.set_nanbox_f64(unsafe {
            crate::closure::js_native_call_value(
                field(resource.get_nanbox_f64(), b"borrow"),
                crate::closure::plain_call_receiver(),
                std::ptr::null(),
                0,
            )
        });
    }
    crate::gc::gc_collect_minor();
    let moved = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
    assert!(!JSValue::from_bits(moved.to_bits()).is_undefined());
    assert_ne!(
        moved.to_bits(),
        original,
        "owned wrapper must actually move"
    );
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 0);
    borrowed.set_nanbox_f64(undefined());
    crate::gc::gc_collect_minor();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined()
    );
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 1);
    // Collection itself never enters the component. Delivery is a normal
    // nextTick job; this not-yet-initialized context retains the request.
    assert!(unsafe {
        (*context::get(owner.get_nanbox_f64()))
            .pending_collections
            .is_empty()
    });
    crate::promise::js_promise_run_microtasks();
    assert_eq!(
        unsafe { &(*context::get(owner.get_nanbox_f64())).pending_collections },
        &[json!({"kind":"guest","token":"test:1"})]
    );
    crate::promise::js_promise_run_microtasks();
    assert_eq!(
        unsafe {
            (*context::get(owner.get_nanbox_f64()))
                .pending_collections
                .len()
        },
        1
    );
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
    assert!(unsafe {
        (*context::get(owner.get_nanbox_f64()))
            .pending_collections
            .is_empty()
    });
}

#[test]
fn finalization_registry_does_not_keep_an_unreachable_component_cycle_alive() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let weak = scope.root_nanbox_f64(undefined());
    {
        let temporary = RuntimeHandleScope::new();
        let owner = temporary.root_nanbox_f64(context::publish(json!({}), undefined()));
        resource::wrap(owner.get_nanbox_f64(), "test:2", true);
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(owner.get_nanbox_f64()) as i64,
        ));
    }
    crate::gc::gc_collect_minor();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined()
    );
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        0,
        "dead registries must not resurrect component callbacks"
    );
}
