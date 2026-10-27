use super::*;
extern "C-unwind" fn destructor(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::gc::gc_collect_minor();
    crate::exception::js_throw(error.get_nanbox_f64())
}
fn resource_with_error(component: f64, error: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let error = scope.root_nanbox_f64(error);
    let ty = scope.root_nanbox_f64(text("test:driver/clock#item"));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind destructor,1),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        error.get_nanbox_f64(),
    );
    host_resource::payload::js_component_host_resource_create(
        component.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        42.0,
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    )
}
#[test]
fn explicit_disposal_preserves_a_nonzero_origin_exception_before_local_cleanup_failure() {
    for historical_error in [false, true] {
        let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::gc_init();
        let _ops = host::TestOpsGuard::install(&OPS);
        let scope = RuntimeHandleScope::new();
        let component = scope.root_nanbox_f64(load(&scope, 42.0));
        let _lease = EngineLease(
            context::get(component.get_nanbox_f64()),
            engine(component.get_nanbox_f64()),
        );
        let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        unsafe {
            let world = &mut (*context::get(component.get_nanbox_f64())).world;
            world["imports"][0]["resources"] = json!({"item":"test:driver/clock#item"});
        }
        let transferred = scope.root_nanbox_f64(resource_with_error(
            component.get_nanbox_f64(),
            error.get_nanbox_f64(),
        ));
        let _local = scope.root_nanbox_f64(resource_with_error(component.get_nanbox_f64(), 99.0));
        let identity =
            host_resource::encode(transferred.get_nanbox_f64(), component.get_nanbox_f64())
                .unwrap();
        let reference = json!({"token":identity["$hostResource"],"type":identity["type"],"generation":identity["generation"]});
        host_resource::payload::event(
            component.get_nanbox_f64(),
            &json!({"kind":"lower","created":[reference],"transferred":[reference]}),
        );
        let result = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
        crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
        if historical_error {
            remember(component.get_nanbox_f64(), 1, 88.0);
        }
        unsafe {
            (*engine(component.get_nanbox_f64())).disposal_events =
                vec![(1, json!({"kind":"drop","resource":reference}))];
        }
        let outcome =
            crate::exception::catch_js_throw(|| js_component_dispose(component.get_nanbox_f64()));
        assert_eq!(
            outcome.unwrap_err().to_bits(),
            error.get_nanbox_f64().to_bits(),
            "a native callback's first exception precedes later local cleanup"
        );
        assert_eq!(
            crate::promise::js_promise_state(promise(result.get_nanbox_f64())),
            2
        );
        assert_eq!(
            crate::promise::js_promise_result(promise(result.get_nanbox_f64())).to_bits(),
            if historical_error {
                88.0f64.to_bits()
            } else {
                error.get_nanbox_f64().to_bits()
            }
        );
        assert!(!unsafe { (*context::get(component.get_nanbox_f64())).alive });
        assert_eq!(poll::poll_components(), 1);
    }
}

#[test]
fn retiring_a_queued_collector_does_not_throw_a_public_disposal_error() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let result = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
    unsafe {
        (*context::get(component.get_nanbox_f64()))
            .pending_collections
            .push(json!({"kind":"guest","token":"already-transferred"}));
    }
    cleanup::drain(component.get_nanbox_f64());
    assert_eq!(
        unsafe { (*state(component.get_nanbox_f64())).origins.len() },
        2,
        "both public call and internal collection must actually be queued"
    );
    assert!(
        crate::exception::catch_js_throw(|| js_component_dispose(component.get_nanbox_f64()))
            .is_ok(),
        "cancelling an internal collector is not an observable disposal failure"
    );
    assert_eq!(
        crate::promise::js_promise_state(promise(result.get_nanbox_f64())),
        2
    );
    assert_eq!(poll::poll_components(), 1);
    assert!(unsafe { (*state(component.get_nanbox_f64())).origins.is_empty() });
}

#[test]
fn native_retirement_cancels_collectors_without_masking_a_public_rejection() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let result = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
    unsafe {
        (*context::get(component.get_nanbox_f64()))
            .pending_collections
            .push(json!({"kind":"guest","token":"already-transferred"}));
    }
    cleanup::drain(component.get_nanbox_f64());
    remember(component.get_nanbox_f64(), 1, 77.0);
    unsafe {
        let native = &mut *engine(component.get_nanbox_f64());
        native.trap = true;
        native.ready = vec![1, 2];
    }
    assert_eq!(
        crate::exception::catch_js_throw(|| poll::poll_components()),
        Ok(1)
    );
    assert_eq!(
        crate::promise::js_promise_state(promise(result.get_nanbox_f64())),
        2
    );
    assert_eq!(
        crate::promise::js_promise_result(promise(result.get_nanbox_f64())),
        77.0
    );
    assert!(unsafe { (*state(component.get_nanbox_f64())).origins.is_empty() });
}
