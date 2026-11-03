use super::*;

#[test]
fn cancelled_jobs_and_late_reactions_never_migrate_to_siblings_or_unowned_drains() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(16) as i64,
    ));
    let a = JobDomain::new();
    let b = JobDomain::new();
    let token = a.token();
    let pending = scope.root_raw_mut_ptr(a.run(|| {
        crate::builtins::js_queue_next_tick(callback(log.get_nanbox_f64(), 101.0) as i64);
        crate::builtins::js_queue_microtask(callback(log.get_nanbox_f64(), 102.0) as i64);
        let p = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        let cb = scope.root_raw_mut_ptr(callback(log.get_nanbox_f64(), 103.0));
        crate::promise::js_promise_then(
            p.get_raw_mut_ptr(),
            cb.get_raw_const_ptr(),
            std::ptr::null(),
        );
        let ready = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        let cb = scope.root_raw_mut_ptr(callback(log.get_nanbox_f64(), 104.0));
        crate::promise::js_promise_then(
            ready.get_raw_mut_ptr(),
            cb.get_raw_const_ptr(),
            std::ptr::null(),
        );
        crate::promise::js_promise_resolve(ready.get_raw_mut_ptr(), 1.0);
        crate::node_submodules::diagnostics::schedule_uncaught(999.0);
        assert!(crate::promise::eligible_guest_jobs_pending());
        p.get_raw_mut_ptr::<crate::promise::Promise>()
    }));
    b.run(|| {
        crate::builtins::js_queue_next_tick(callback(log.get_nanbox_f64(), 201.0) as i64);
        crate::builtins::js_queue_microtask(callback(log.get_nanbox_f64(), 202.0) as i64);
    });
    a.cancel();
    a.cancel();
    crate::promise::discard_cancelled_guest_jobs();
    assert!(!a.take_ready());
    assert!(!eligible(&Some(token.clone())));
    drop(a);
    assert!(
        cancelled(&Some(token.clone())),
        "lease Drop must not turn CANCELLED into adoptable EXITED"
    );
    b.take_ready();
    let before = pending.get_raw_mut_ptr::<crate::promise::Promise>();
    crate::gc::gc_collect_minor();
    assert_ne!(before, pending.get_raw_mut_ptr());
    b.run(|| {
        crate::promise::js_promise_resolve(pending.get_raw_mut_ptr(), 1.0);
        assert!(
            !b.take_ready(),
            "late cancelled reactions must not adopt the settling sibling"
        );
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    assert_eq!(values(log.get_nanbox_f64()), [201.0, 202.0]);
    assert!(!crate::promise::eligible_guest_jobs_pending());
    assert!(current().is_none());
    let mut late = Some(token);
    b.run(|| assert!(!prepare_enqueue(&mut late)));
    assert!(cancelled(&late));
    assert!(!b.take_ready());
}

#[test]
fn cancelled_deferred_destroy_notifications_are_removed_and_rejected_at_enqueue() {
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let owner = JobDomain::new();
    let scope = RuntimeHandleScope::new();
    // Enabling a hook makes the Promise retain a real async resource state.
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(8) as i64,
    ));
    let options = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let cb = scope.root_raw_mut_ptr(callback(log.get_nanbox_f64(), 55.0));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"destroy".as_ptr(), 7));
    crate::object::js_object_set_field_by_name(
        options.get_raw_mut_ptr(),
        key.get_raw_const_ptr(),
        crate::value::js_nanbox_pointer(
            cb.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
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
    let p = scope.root_raw_mut_ptr(owner.run(|| crate::promise::js_promise_new()));
    let id = unsafe { (*p.get_raw_mut_ptr::<crate::promise::Promise>()).async_id };
    crate::async_hooks::enqueue_gc_destroy(id);
    owner.run(|| assert!(crate::async_hooks::gc_destroy_work_pending()));
    owner.cancel();
    crate::promise::discard_cancelled_guest_jobs();
    crate::async_hooks::enqueue_gc_destroy(id);
    assert_eq!(crate::async_hooks::drain_gc_destroy_queue(), 0);
    assert!(values(log.get_nanbox_f64()).is_empty());
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    crate::async_hooks::reset_for_tests();
}
