use super::*;
use crate::{gc::RuntimeHandleScope, value::JSValue};

fn array(value: f64) -> *mut crate::array::ArrayHeader {
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::array::ArrayHeader>()
        .cast_mut()
}

fn promise(value: f64) -> *mut crate::promise::Promise {
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}

extern "C-unwind" fn append(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let marker = crate::closure::js_closure_get_capture_f64(closure, 1);
    crate::gc::gc_collect_minor();
    let length = crate::array::js_array_length(array(log.get_nanbox_f64()));
    crate::array::js_array_set_f64_extend(array(log.get_nanbox_f64()), length, marker);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn callback(log: f64, marker: f64) -> *mut crate::closure::ClosureHeader {
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(unwind append, 1), 2);
    crate::closure::js_closure_set_capture_f64(closure, 0, log);
    crate::closure::js_closure_set_capture_f64(closure, 1, marker);
    closure
}

fn values(log: f64) -> Vec<f64> {
    (0..crate::array::js_array_length(array(log)))
        .map(|i| crate::array::js_array_get_f64(array(log), i))
        .collect()
}

#[test]
fn als_snapshot_restores_values_without_stealing_the_calling_canonical_task() {
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let original = capture_context();
    let storage = AsyncLocalStoragePayload::default();
    let a = JobDomain::new();
    let b = JobDomain::new();
    let captured = scope.root_raw_mut_ptr(a.run(|| {
        enter_with(storage.token(), 11.0);
        crate::async_hooks::context_snapshots::snapshot_closure(capture_context())
    }));
    b.run(|| {
        enter_with(storage.token(), 22.0);
        crate::async_hooks::context_snapshots::run_with_context_snapshot(
            captured.get_raw_const_ptr(),
            || {
                assert!(b.is_current());
                assert!(!a.is_current());
                assert_eq!(get_store(storage.token()), Some(11.0));
                0.0
            },
        );
        assert!(b.is_current());
        assert_eq!(get_store(storage.token()), Some(22.0));
    });
    assert!(current().is_none());
    restore_context(original);
}

#[test]
fn nested_domain_scopes_restore_identity_and_guard_depth_after_a_js_throw() {
    let a = JobDomain::new();
    let b = JobDomain::new();
    let before = context_guard_depth();
    a.run(|| {
        let result = crate::exception::catch_js_throw(|| {
            b.run(|| {
                assert!(b.is_current());
                crate::exception::js_throw(123.0)
            })
        });
        assert_eq!(result, Err(123.0));
        assert!(a.is_current());
    });
    assert!(current().is_none());
    assert_eq!(context_guard_depth(), before);
}

#[test]
fn jobs_keep_live_owners_fifo_and_wake_their_owner_when_another_task_resolves_them() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(32) as i64,
    ));
    let a = JobDomain::new();
    let b = JobDomain::new();
    let pending = scope.root_nanbox_f64(a.run(|| {
        crate::builtins::js_queue_next_tick(callback(log.get_nanbox_f64(), 101.0) as i64);
        crate::builtins::js_queue_microtask(callback(log.get_nanbox_f64(), 102.0) as i64);
        let p = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        let cb = scope.root_raw_mut_ptr(callback(log.get_nanbox_f64(), 103.0));
        crate::promise::js_promise_then(
            p.get_raw_mut_ptr(),
            cb.get_raw_const_ptr(),
            std::ptr::null(),
        );
        crate::value::js_nanbox_pointer(p.get_raw_mut_ptr::<crate::promise::Promise>() as i64)
    }));
    b.run(|| {
        crate::builtins::js_queue_next_tick(callback(log.get_nanbox_f64(), 201.0) as i64);
        crate::builtins::js_queue_microtask(callback(log.get_nanbox_f64(), 202.0) as i64);
    });
    assert!(a.take_ready());
    assert!(b.take_ready());
    let before = pending.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(
        before,
        pending.get_nanbox_f64().to_bits(),
        "the pending Promise must really move"
    );
    b.run(|| crate::promise::js_promise_resolve(promise(pending.get_nanbox_f64()), 1.0));
    assert!(a.take_ready(), "settlement must wake the reaction's owner");
    assert!(!b.take_ready());
    crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    assert!(
        values(log.get_nanbox_f64()).is_empty(),
        "an unowned drain must leave live guest tasks parked"
    );
    b.run(|| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    assert_eq!(values(log.get_nanbox_f64()), [201.0, 202.0]);
    a.run(|| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    assert_eq!(
        values(log.get_nanbox_f64()),
        [201.0, 202.0, 101.0, 102.0, 103.0]
    );
    assert!(current().is_none());
}

#[test]
fn exited_owner_jobs_can_run_on_the_next_task_without_retaining_a_live_task() {
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(8) as i64,
    ));
    let a = JobDomain::new();
    a.run(|| crate::builtins::js_queue_microtask(callback(log.get_nanbox_f64(), 7.0) as i64));
    let b = JobDomain::new();
    b.run(|| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    assert!(values(log.get_nanbox_f64()).is_empty());
    drop(a);
    b.run(|| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    assert_eq!(values(log.get_nanbox_f64()), [7.0]);
}

#[test]
fn moving_snapshot_holder_transfers_and_releases_its_native_domain_lease_once() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let a = JobDomain::new();
    let weak = Arc::downgrade(&a.token.0);
    {
        let scope = RuntimeHandleScope::new();
        let snapshot = a.run(capture_context);
        assert!(
            !snapshot.is_empty(),
            "task ownership must survive an empty ALS context"
        );
        let holder = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(alloc_snapshot_holder(
            snapshot,
        ) as i64));
        drop(a);
        assert_eq!(weak.strong_count(), 1);
        let before = holder.get_nanbox_f64().to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(before, holder.get_nanbox_f64().to_bits());
        assert_eq!(
            weak.strong_count(),
            1,
            "moving the holder must neither clone nor release its native token"
        );
    }
    crate::gc::gc_collect_minor();
    assert_eq!(weak.strong_count(), 0);
}

#[test]
fn deferred_destroy_notifications_keep_native_ownership_after_the_resource_context_is_released() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(8) as i64,
    ));
    let options = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let callback = scope.root_raw_mut_ptr(callback(log.get_nanbox_f64(), 55.0));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"destroy".as_ptr(), 7));
    crate::object::js_object_set_field_by_name(
        options.get_raw_mut_ptr(),
        key.get_raw_const_ptr(),
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let hook = scope.root_nanbox_f64(crate::async_hooks::js_async_hooks_create_hook(
        crate::value::js_nanbox_pointer(
            options.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
        ),
    ));
    crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    let a = JobDomain::new();
    let b = JobDomain::new();
    let promise = scope.root_raw_mut_ptr(a.run(|| crate::promise::js_promise_new()));
    let id = unsafe { (*promise.get_raw_mut_ptr::<crate::promise::Promise>()).async_id };
    assert!(!a.take_ready());
    b.run(|| crate::async_hooks::enqueue_gc_destroy(id));
    assert!(a.take_ready());
    assert!(!b.take_ready());
    crate::gc::gc_collect_minor();
    b.run(|| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    assert!(values(log.get_nanbox_f64()).is_empty());
    a.run(|| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    assert_eq!(values(log.get_nanbox_f64()), [55.0]);
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    crate::async_hooks::reset_for_tests();
}
