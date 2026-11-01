use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_EXPORTS, COMPONENT_GUEST};

extern "C-unwind" fn complete(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let retained = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let promise = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let token = crate::closure::js_closure_get_capture_f64(closure, 2) as i64;
    crate::gc::gc_collect_minor();
    let marker = crate::array::js_array_get_f64(array(retained.get_nanbox_f64()), 0);
    assert_eq!(als::get_store(token), Some(marker));
    crate::promise::js_promise_resolve(
        JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut(),
        marker,
    );
    undefined()
}

extern "C-unwind" fn ignore_and_capture(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _ignored: f64,
    retained: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let retained = scope.root_nanbox_f64(retained);
    let token = crate::closure::js_closure_get_capture_f64(closure, 0);
    let marker = crate::array::js_array_get_f64(array(retained.get_nanbox_f64()), 0);
    als::enter_with(token as i64, marker);
    let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind complete, 0),
        3,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        retained.get_nanbox_f64(),
    );
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        1,
        crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64),
    );
    crate::closure::js_closure_set_capture_f64(callback.get_raw_mut_ptr(), 2, token);
    crate::builtins::js_queue_microtask(
        callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    );
    crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64)
}

fn with_exports(body: impl FnOnce(&RuntimeHandleScope)) {
    let scope = RuntimeHandleScope::new();
    let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
    let previous_registry = scope.root_nanbox_f64(super::super::super::maybe_registry());
    let mut previous = als::capture_context();
    let previous_roots = als::root_snapshot(&scope, &previous);
    let storage = als::AsyncLocalStoragePayload::default();
    als::enter_with(storage.token(), 99.0);
    let registry = scope.root_nanbox_f64(context::publish(
        serde_json::json!({"exports":[{"name":"", "functions":[
            {"name":"start", "params":[{}, {}]}
        ]}]}),
        undefined(),
    ));
    let exports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind ignore_and_capture, 2),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        storage.token() as f64,
    );
    set_field(
        exports.get_nanbox_f64(),
        b"start",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    unsafe {
        bag::store(
            object_pointer(registry.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_EXPORTS,
            exports.get_nanbox_f64(),
        );
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            registry.get_nanbox_f64(),
        );
    }
    body(&scope);
    assert_eq!(als::get_store(storage.token()), Some(99.0));
    assert!(job_domain::current().is_none());
    unsafe {
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            previous_registry.get_nanbox_f64(),
        );
    }
    als::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    als::restore_context(previous);
}

#[test]
fn released_arguments_collect_but_js_captures_results_als_and_borrow_lifetimes_survive() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    with_exports(|scope| unsafe {
        let mut calls = Vec::new();
        for release in [false, true] {
            let owner = perry_component_frame_task_new();
            let args = perry_component_frame_array(owner, 2);
            let ignored = perry_component_frame_array(owner, 0);
            let retained = perry_component_frame_array(owner, 1);
            let marker = perry_component_frame_number(owner, if release { 2.0 } else { 1.0 });
            perry_component_frame_array_set(owner, retained, 0, marker);
            perry_component_frame_array_set(owner, args, 0, ignored);
            perry_component_frame_array_set(owner, args, 1, retained);
            let ignored_weak = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(frame(owner).get(ignored)) as i64,
            ));
            let retained_weak = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(frame(owner).get(retained)) as i64,
            ));
            let retained_before = frame(owner).get(retained).to_bits();
            let lifetime = scope.root_nanbox_f64(frame(owner).lifetime());
            let result = perry_component_frame_task_call(owner, 0, 0, args);
            assert_eq!(perry_component_frame_failed(owner), 0);
            if release {
                assert_eq!(
                    perry_component_frame_task_release_arguments(owner, result),
                    0
                );
                assert_eq!(
                    perry_component_frame_task_release_arguments(owner, result),
                    0
                );
                assert!(JSValue::from_bits(frame(owner).get(args).to_bits()).is_undefined());
            }
            calls.push((
                owner,
                result,
                ignored_weak,
                retained_weak,
                retained_before,
                lifetime,
            ));
        }
        crate::gc::gc_collect_minor();
        for (index, (owner, result, ignored, retained, before, lifetime)) in
            calls.iter().enumerate()
        {
            let ignored = crate::weakref::js_weakref_deref(ignored.get_nanbox_f64());
            assert_eq!(
                JSValue::from_bits(ignored.to_bits()).is_undefined(),
                index == 1
            );
            let kept = crate::weakref::js_weakref_deref(retained.get_nanbox_f64());
            assert!(!JSValue::from_bits(kept.to_bits()).is_undefined());
            assert_ne!(
                kept.to_bits(),
                *before,
                "captured argument must really move"
            );
            assert_eq!(
                crate::array::js_array_get_f64(array(lifetime.get_nanbox_f64()), 0),
                1.0
            );
            let mut value = u32::MAX;
            assert_eq!(
                perry_component_frame_promise_poll(*owner, *result, &mut value),
                0
            );
        }
        for (index, (owner, result, _, _, _, lifetime)) in calls.iter().enumerate().rev() {
            assert!(perry_component_frame_task_checkpoint(*owner) > 0);
            let mut value = u32::MAX;
            assert_eq!(
                perry_component_frame_promise_poll(*owner, *result, &mut value),
                1
            );
            assert_eq!(
                perry_component_frame_get_number(*owner, value),
                index as f64 + 1.0
            );
            assert_eq!(perry_component_frame_resource_scope_end(*owner), 0);
            assert_eq!(
                crate::array::js_array_get_f64(array(lifetime.get_nanbox_f64()), 0),
                0.0
            );
            perry_component_frame_drop(*owner);
        }
    });
}

#[test]
fn argument_release_validates_first_result_and_custody_before_clearing() {
    crate::gc::gc_init();
    with_exports(|_| unsafe {
        for pending in [false, true] {
            let owner = perry_component_frame_task_new();
            let args = perry_component_frame_array(owner, 2);
            let value = perry_component_frame_array(owner, 1);
            perry_component_frame_array_set(owner, args, 0, value);
            perry_component_frame_array_set(owner, args, 1, value);
            let result = perry_component_frame_task_call(owner, 0, 0, args);
            assert_eq!(perry_component_frame_failed(owner), 0);
            let before = frame(owner).get(args).to_bits();
            if pending {
                frame(owner).transfers.push(value);
            }
            assert_eq!(
                perry_component_frame_task_release_arguments(
                    owner,
                    if pending { result } else { result + 1 }
                ),
                -1
            );
            assert_eq!(frame(owner).get(args).to_bits(), before);
            frame(owner).transfers.clear();
            frame(owner).task.as_ref().unwrap().domain.cancel();
            crate::promise::discard_cancelled_guest_jobs();
            perry_component_frame_drop(owner);
        }
        let ordinary = perry_component_frame_new();
        assert_eq!(
            perry_component_frame_task_release_arguments(ordinary, 0),
            -1
        );
        perry_component_frame_drop(ordinary);
        let uncalled = perry_component_frame_task_new();
        assert_eq!(
            perry_component_frame_task_release_arguments(uncalled, 0),
            -1
        );
        perry_component_frame_drop(uncalled);
    });
}
