use super::*;
use serde_json::json;

extern "C" fn collect_during_tick_initialization(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) == "TickObject" {
        let scope = RuntimeHandleScope::new();
        let count = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
        crate::gc::gc_collect_minor();
        let pointer = array_pointer(count.get_nanbox_f64()).unwrap();
        let value = crate::array::js_array_get_f64(pointer, 0);
        crate::array::js_array_set_f64(pointer, 0, value + 1.0);
    }
    undefined()
}

#[test]
fn finalization_delivery_keeps_every_pending_job_rooted_across_init_hook_collections() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    // The isolation guard removes production scanners. This test delivers
    // real async hooks and queued jobs across collections, so their roots
    // must be installed too, not just this function's RuntimeHandles.
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(json!({}), undefined()));
    for token in ["pending:1", "pending:2", "pending:3"] {
        resource::wrap(owner.get_nanbox_f64(), token, true);
    }
    crate::gc::gc_collect_minor();
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 3);
    let count = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array_pointer(count.get_nanbox_f64()).unwrap(), 0, 0.0);
    let init = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(collect_during_tick_initialization, 4),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(init.get_raw_mut_ptr(), 0, count.get_nanbox_f64());
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(
        options.get_nanbox_f64(),
        b"init",
        crate::value::js_nanbox_pointer(
            init.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let hook = scope.root_nanbox_f64(crate::async_hooks::js_async_hooks_create_hook(
        options.get_nanbox_f64(),
    ));
    crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    crate::weakref::drain_pending_finalization_jobs();
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert_eq!(
        crate::array::js_array_get_f64(array_pointer(count.get_nanbox_f64()).unwrap(), 0),
        3.0,
        "each job must run the collecting init hook"
    );
    crate::promise::js_promise_run_microtasks();
    let pending = unsafe { &(*context::get(owner.get_nanbox_f64())).pending_collections };
    let mut tokens: Vec<_> = pending
        .iter()
        .map(|request| request["token"].as_str().unwrap())
        .collect();
    tokens.sort_unstable();
    assert_eq!(tokens, ["pending:1", "pending:2", "pending:3"]);
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
}

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
