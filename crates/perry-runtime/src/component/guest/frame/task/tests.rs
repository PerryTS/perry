use super::*;
use crate::object::owned_state_bag::{self as bag, COMPONENT_EXPORTS, COMPONENT_GUEST};

extern "C-unwind" fn finish(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let promise = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let token = crate::closure::js_closure_get_capture_f64(closure, 2) as i64;
    let store = scope.root_nanbox_f64(als::get_store(token).expect("task ALS store"));
    crate::gc::gc_collect_minor();
    let marker = crate::array::js_array_get_f64(array(store.get_nanbox_f64()), 0);
    let index = crate::array::js_array_length(array(log.get_nanbox_f64()));
    crate::array::js_array_set_f64_extend(array(log.get_nanbox_f64()), index, marker);
    let promise = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut();
    crate::promise::js_promise_resolve(promise, marker);
    undefined()
}

extern "C-unwind" fn start(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    marker: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let token = crate::closure::js_closure_get_capture_f64(closure, 1);
    let store = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array(store.get_nanbox_f64()), 0, marker);
    als::enter_with(token as i64, store.get_nanbox_f64());
    let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind finish, 0),
        3,
    ));
    crate::closure::js_closure_set_capture_f64(callback.get_raw_mut_ptr(), 0, log.get_nanbox_f64());
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

extern "C-unwind" fn read(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let token = crate::closure::js_closure_get_capture_f64(closure, 0) as i64;
    crate::array::js_array_get_f64(array(als::get_store(token).expect("saved task context")), 0)
}

#[test]
fn c_task_frames_restore_their_own_als_and_checkpoint_independently_after_moving_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
    let previous_registry = scope.root_nanbox_f64(super::super::super::maybe_registry());
    let mut previous = als::capture_context();
    let previous_roots = als::root_snapshot(&scope, &previous);
    let storage = als::AsyncLocalStoragePayload::default();
    als::enter_with(storage.token(), 9.0);
    let world = serde_json::json!({"exports":[{"name":"", "functions":[
        {"name":"start", "params":[{}]}, {"name":"read", "params":[]}
    ]}]});
    let registry = scope.root_nanbox_f64(context::publish(world, undefined()));
    let exports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(8) as i64,
    ));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind start, 1),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(callback.get_raw_mut_ptr(), 0, log.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        1,
        storage.token() as f64,
    );
    set_field(
        exports.get_nanbox_f64(),
        b"start",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind read, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        storage.token() as f64,
    );
    set_field(
        exports.get_nanbox_f64(),
        b"read",
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
        let depth = als::context_guard_depth();
        let a = perry_component_frame_task_new();
        let b = perry_component_frame_task_new();
        let mut promises = Vec::new();
        for (owner, marker) in [(a, 1.0), (b, 2.0)] {
            let args = perry_component_frame_array(owner, 1);
            let number = perry_component_frame_number(owner, marker);
            perry_component_frame_array_set(owner, args, 0, number);
            let result = perry_component_frame_task_call(owner, 0, 0, args);
            assert_eq!(perry_component_frame_failed(owner), 0);
            assert_eq!(als::get_store(storage.token()), Some(9.0));
            assert!(job_domain::current().is_none());
            assert_eq!(als::context_guard_depth(), depth);
            promises.push(result);
        }
        assert_eq!(
            crate::array::js_array_length(array(log.get_nanbox_f64())),
            0,
            "task calls must not run a global checkpoint"
        );
        crate::gc::gc_collect_minor();
        for (owner, marker) in [(a, 1.0), (b, 2.0)] {
            let args = perry_component_frame_array(owner, 0);
            let value = perry_component_frame_task_call(owner, 0, 1, args);
            assert_eq!(perry_component_frame_get_number(owner, value), marker);
            assert_eq!(perry_component_frame_task_ready(owner), 1);
            assert_eq!(
                perry_component_frame_task_ready(owner),
                1,
                "ready remains set while runnable jobs are queued"
            );
        }
        assert!(perry_component_frame_task_checkpoint(b) > 0);
        assert_eq!(
            crate::array::js_array_length(array(log.get_nanbox_f64())),
            1
        );
        assert_eq!(
            crate::array::js_array_get_f64(array(log.get_nanbox_f64()), 0),
            2.0
        );
        let mut result = u32::MAX;
        assert_eq!(
            perry_component_frame_promise_poll(a, promises[0], &mut result),
            0
        );
        assert_eq!(
            perry_component_frame_promise_poll(b, promises[1], &mut result),
            1
        );
        assert_eq!(perry_component_frame_get_number(b, result), 2.0);
        perry_component_frame_drop(b);
        crate::gc::gc_collect_minor();
        assert!(perry_component_frame_task_checkpoint(a) > 0);
        assert_eq!(
            perry_component_frame_promise_poll(a, promises[0], &mut result),
            1
        );
        assert_eq!(perry_component_frame_get_number(a, result), 1.0);
        assert_eq!(
            crate::array::js_array_get_f64(array(log.get_nanbox_f64()), 1),
            1.0
        );
        assert_eq!(als::get_store(storage.token()), Some(9.0));
        assert!(job_domain::current().is_none());
        assert_eq!(als::context_guard_depth(), depth);
        perry_component_frame_drop(a);
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            previous_registry.get_nanbox_f64(),
        );
    }
    als::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    als::restore_context(previous);
}

extern "C-unwind" fn throw_job(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    crate::exception::js_throw(crate::closure::js_closure_get_capture_f64(closure, 0))
}

#[test]
fn uncaught_task_jobs_preserve_the_original_error_without_exiting_the_host_or_failing_a_sibling() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let a = perry_component_frame_task_new();
        let b = perry_component_frame_task_new();
        let error = perry_component_frame_array(a, 1);
        let marker = perry_component_frame_number(a, 77.0);
        perry_component_frame_array_set(a, error, 0, marker);
        frame(a).task_guard(|owner| {
            let scope = RuntimeHandleScope::new();
            let error = scope.root_nanbox_f64(owner.get(error));
            let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(unwind throw_job, 0),
                1,
            ));
            crate::closure::js_closure_set_capture_f64(
                callback.get_raw_mut_ptr(),
                0,
                error.get_nanbox_f64(),
            );
            crate::builtins::js_queue_microtask(
                callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            );
        });
        assert_eq!(perry_component_frame_task_checkpoint(b), 0);
        assert_eq!(perry_component_frame_failed(b), 0);
        assert_eq!(perry_component_frame_task_checkpoint(a), -1);
        assert!(!crate::promise::microtasks::microtask_drain_active());
        assert!(!crate::promise::microtasks::microtask_job_drain_active());
        assert!(job_domain::current().is_none());
        crate::gc::gc_collect_minor();
        assert_eq!(
            frame(a).error().unwrap().to_bits(),
            frame(a).get(error).to_bits()
        );
        assert_eq!(
            crate::array::js_array_get_f64(array(frame(a).error().unwrap()), 0),
            77.0
        );
        assert_eq!(perry_component_frame_task_ready(a), -1);
        let log = perry_component_frame_array(b, 1);
        frame(b).task_guard(|owner| {
            let scope = RuntimeHandleScope::new();
            let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(unwind record_unowned_job, 0),
                1,
            ));
            crate::closure::js_closure_set_capture_f64(
                callback.get_raw_mut_ptr(),
                0,
                owner.get(log),
            );
            crate::builtins::js_queue_microtask(
                callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            );
        });
        assert!(perry_component_frame_task_checkpoint(b) > 0);
        assert_eq!(
            crate::array::js_array_get_f64(array(frame(b).get(log)), 0),
            42.0
        );
        assert!(!crate::promise::microtasks::microtask_drain_active());
        assert_eq!(perry_component_frame_failed(b), 0);
        perry_component_frame_drop(a);
        perry_component_frame_drop(b);
    }
}

pub(super) extern "C-unwind" fn record_unowned_job(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    assert!(job_domain::current().is_some());
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::gc::gc_collect_minor();
    crate::array::js_array_set_f64(array(log.get_nanbox_f64()), 0, 42.0);
    undefined()
}

#[test]
fn readiness_notices_unowned_jobs_without_an_owner_hint_and_delivers_them_once() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let a = perry_component_frame_task_new();
        let b = perry_component_frame_task_new();
        let log = perry_component_frame_array(a, 1);
        let scope = RuntimeHandleScope::new();
        let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(unwind record_unowned_job, 0),
            1,
        ));
        crate::closure::js_closure_set_capture_f64(
            callback.get_raw_mut_ptr(),
            0,
            frame(a).get(log),
        );
        assert!(job_domain::current().is_none());
        crate::builtins::js_queue_microtask(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        );
        assert!(!frame(a).task.as_ref().unwrap().domain.take_ready());
        assert!(!frame(b).task.as_ref().unwrap().domain.take_ready());
        assert_eq!(perry_component_frame_task_ready(a), 1);
        assert_eq!(perry_component_frame_task_ready(b), 1);
        assert!(perry_component_frame_task_checkpoint(b) > 0);
        assert_eq!(
            crate::array::js_array_get_f64(array(frame(a).get(log)), 0),
            42.0
        );
        assert_eq!(perry_component_frame_task_ready(a), 0);
        assert_eq!(perry_component_frame_task_ready(b), 0);
        assert_eq!(perry_component_frame_task_checkpoint(a), 0);
        assert!(job_domain::current().is_none());
        perry_component_frame_drop(a);
        perry_component_frame_drop(b);
    }
}

#[test]
fn task_entry_points_reject_ordinary_frames_inside_the_c_error_boundary() {
    crate::gc::gc_init();
    unsafe {
        for operation in 0..3 {
            let ordinary = perry_component_frame_new();
            match operation {
                0 => assert_eq!(perry_component_frame_task_ready(ordinary), -1),
                1 => assert_eq!(perry_component_frame_task_checkpoint(ordinary), -1),
                _ => assert_eq!(perry_component_frame_task_call(ordinary, 0, 0, 0), 0),
            }
            assert_eq!(perry_component_frame_failed(ordinary), 1);
            assert_eq!(perry_component_frame_task_checkpoint(ordinary), -1);
            assert!(job_domain::current().is_none());
            perry_component_frame_drop(ordinary);
        }
    }
}
