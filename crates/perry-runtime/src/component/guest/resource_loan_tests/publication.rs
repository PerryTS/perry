use super::*;

struct Hook<'a>(crate::gc::RuntimeHandle<'a>);
impl Drop for Hook<'_> {
    fn drop(&mut self) {
        crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
            self.0.get_nanbox_f64(),
        ));
        crate::async_hooks::reset_for_tests();
    }
}
extern "C-unwind" fn canonical_loan_promise_init(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) != "PROMISE" {
        return undefined();
    }
    let scope = RuntimeHandleScope::new();
    let mode = crate::closure::js_closure_get_capture_f64(closure, 0);
    let value = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    if mode == 1.0 {
        method(value.get_nanbox_f64(), b"dispose");
    }
    crate::gc::gc_collect_minor();
    if mode == 0.0 {
        crate::exception::js_throw(value.get_nanbox_f64());
    }
    undefined()
}
fn install_loan_init_hook<'a>(scope: &'a RuntimeHandleScope, mode: f64, value: f64) -> Hook<'a> {
    let value = scope.root_nanbox_f64(value);
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind canonical_loan_promise_init, 4),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(callback.get_raw_mut_ptr(), 0, mode);
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        1,
        value.get_nanbox_f64(),
    );
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(
        options.get_nanbox_f64(),
        b"init",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let hook = scope.root_nanbox_f64(crate::async_hooks::js_async_hooks_create_hook(
        options.get_nanbox_f64(),
    ));
    crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    Hook(hook)
}

#[test]
fn throwing_promise_init_preserves_staged_owners_original_error_and_runtime_custody() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let mut child = Box::new(Frame::new());
    let owner = owned(&mut child, 4, 1);
    child.guard(|f| lower(f, owner, 4, true, false));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array(error.get_nanbox_f64()), 0, 77.0);
    let old_error = error.get_nanbox_f64().to_bits();
    let enabled = install_loan_init_hook(&scope, 0.0, error.get_nanbox_f64());
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_begin(pointer)),
            0
        );
    }
    assert_ne!(old_error, error.get_nanbox_f64().to_bits());
    assert_eq!(
        child.error().unwrap().to_bits(),
        error.get_nanbox_f64().to_bits()
    );
    assert!(!child.import_is_deferred());
    assert_eq!(get(data(child.get(owner)), STATE), OWNED);
    drop(enabled);
    let alive = child.weak_anchor();
    child.release_import();
    assert!(
        alive.upgrade().is_none(),
        "failed begin must never hand the frame to C"
    );
}

#[test]
fn disposal_in_promise_init_prevents_partial_transfer_and_deferral_publication() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let _ops = TestOps::new(record_drop);
    let domain = job_domain::JobDomain::new();
    let mut child = Box::new(Frame::new());
    let a = owned(&mut child, 4, 1);
    let b = owned(&mut child, 4, 2);
    for slot in [a, b] {
        child.guard(|f| lower(f, slot, 4, true, false));
    }
    let before = child.get(a).to_bits();
    let enabled = install_loan_init_hook(&scope, 1.0, child.get(b));
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_begin(pointer)),
            0
        );
    }
    assert!(child.error().is_some());
    assert!(!child.import_is_deferred());
    assert_ne!(before, child.get(a).to_bits());
    assert_eq!(get(data(child.get(a)), STATE), OWNED);
    assert_eq!(get(data(child.get(b)), STATE), DISPOSED);
    assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 16);
    drop(enabled);
    let mut retry = Frame::new();
    let slot = retry.add(child.get(a));
    retry.guard(|f| lower(f, slot, 4, true, false));
    retry.guard(commit);
    assert!(retry.error().is_none());
    let alive = child.weak_anchor();
    child.release_import();
    assert!(alive.upgrade().is_none());
}

#[test]
fn successful_begin_commits_owned_and_borrowed_arguments_after_collecting_init_hooks() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let mut child = Box::new(Frame::new());
    let a = owned(&mut child, 4, 1);
    let b = owned(&mut child, 4, 2);
    child.guard(|f| lower(f, a, 4, true, false));
    child.guard(|f| lower(f, b, 4, false, false));
    let before = child.get(b).to_bits();
    let enabled = install_loan_init_hook(&scope, 2.0, undefined());
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert!(domain.run(|| abi::perry_component_frame_import_begin(pointer)) > 0);
    }
    assert!(child.error().is_none());
    assert!(child.import_is_deferred());
    assert_ne!(before, child.get(b).to_bits());
    assert_eq!(get(data(child.get(a)), STATE), TRANSFERRED);
    let borrowed = scope.root_nanbox_f64(child.get(b));
    assert_eq!(get(data(borrowed.get_nanbox_f64()), BORROW_COUNT), 1.0);
    drop(enabled);
    child.guard(commit);
    assert_eq!(get(data(borrowed.get_nanbox_f64()), BORROW_COUNT), 1.0);
    drop(child);
    assert_eq!(get(data(borrowed.get_nanbox_f64()), BORROW_COUNT), 0.0);
    let mut transfer = Frame::new();
    let slot = transfer.add(borrowed.get_nanbox_f64());
    transfer.guard(|f| lower(f, slot, 4, true, false));
    transfer.guard(commit);
    assert!(transfer.error().is_none());
}
