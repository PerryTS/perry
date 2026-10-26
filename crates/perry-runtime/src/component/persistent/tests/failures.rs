//! A report's global retirement summary must not replace its observer error.
use super::*;
extern "C-unwind" fn throws(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::gc::gc_collect_minor();
    crate::exception::js_throw(error.get_nanbox_f64())
}
#[test]
fn synchronous_callback_exception_keeps_its_identity_when_the_native_driver_retires() {
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
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind throws,0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        error.get_nanbox_f64(),
    );
    set_field(
        context::imports(component.get_nanbox_f64()),
        b"wait",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    unsafe {
        let world = &mut (*context::get(component.get_nanbox_f64())).world;
        world["imports"][0]["functions"][0]["async"] = json!(false);
        (*engine(component.get_nanbox_f64())).imports = vec![(1, false)];
    }
    let outcome = crate::exception::catch_js_throw(|| call(component.get_nanbox_f64(), "sync"));
    assert_eq!(
        outcome.unwrap_err().to_bits(),
        error.get_nanbox_f64().to_bits()
    );
    assert!(!unsafe { (*context::get(component.get_nanbox_f64())).alive });
    assert_eq!(poll::poll_components(), 1);
}
#[test]
fn async_rejection_is_delivered_once_without_throwing_the_native_retirement_summary() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let result = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
    remember(component.get_nanbox_f64(), 1, 77.0);
    unsafe {
        (*engine(component.get_nanbox_f64())).trap = true;
        (*engine(component.get_nanbox_f64())).ready = vec![1];
    }
    assert_eq!(
        crate::exception::catch_js_throw(|| poll::poll_components()),
        Ok(1),
        "native retirement is already delivered to the rejected observer"
    );
    assert_eq!(
        crate::promise::js_promise_state(promise(result.get_nanbox_f64())),
        2
    );
    assert_eq!(
        crate::promise::js_promise_result(promise(result.get_nanbox_f64())),
        77.0
    );
}
