use super::publication::install_loan_init_hook;
use super::*;

#[test]
fn after_return_import_rejects_incoming_borrows_before_any_custody_commits() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let mut source = Frame::allocate_guest_frame();
    let lifetime = source.lifetime();
    let mut child = Box::new(Frame::allocate_guest_frame());
    let incoming = child.add(wrap(4, 7, 0, false, lifetime, undefined()));
    let owner = owned(&mut child, 4, 8);
    let loan = owned(&mut child, 4, 9);
    child.guard(|f| lower(f, owner, 4, true, false));
    child.guard(|f| lower(f, loan, 4, false, false));
    child.guard(|f| lower(f, incoming, 4, false, false));
    let before = child.get(incoming).to_bits();
    let enabled = install_loan_init_hook(&scope, 2.0, undefined());
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_begin_after_return(pointer)),
            0
        );
    }
    assert_ne!(before, child.get(incoming).to_bits());
    assert!(child.error().is_some());
    assert!(!child.import_is_deferred());
    assert_eq!(get(data(child.get(owner)), STATE), OWNED);
    assert_eq!(get(data(child.get(loan)), BORROW_COUNT), 0.0);
    assert_eq!(get(data(child.get(incoming)), BORROW_COUNT), 0.0);
    drop(enabled);
    let alive = child.weak_anchor();
    child.release_import();
    assert!(alive.upgrade().is_none());
}

#[test]
fn after_return_import_revalidates_borrow_expiry_after_collecting_promise_hooks() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let mut source = Frame::allocate_guest_frame();
    let lifetime = scope.root_nanbox_f64(source.lifetime());
    let mut child = Box::new(Frame::allocate_guest_frame());
    let incoming = child.add(wrap(4, 7, 0, false, lifetime.get_nanbox_f64(), undefined()));
    let owner = owned(&mut child, 4, 8);
    child.guard(|f| lower(f, owner, 4, true, false));
    child.guard(|f| lower(f, incoming, 4, false, false));
    let before = lifetime.get_nanbox_f64().to_bits();
    let enabled = install_loan_init_hook(&scope, 3.0, lifetime.get_nanbox_f64());
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_begin_after_return(pointer)),
            0
        );
    }
    assert_ne!(before, lifetime.get_nanbox_f64().to_bits());
    assert_eq!(get(lifetime.get_nanbox_f64(), 0), 0.0);
    assert!(child.error().is_some());
    assert!(!child.import_is_deferred());
    assert_eq!(get(data(child.get(owner)), STATE), OWNED);
    assert_eq!(get(data(child.get(incoming)), BORROW_COUNT), 0.0);
    drop(enabled);
}

#[test]
fn after_return_import_keeps_guest_owned_alias_loans_through_settlement_and_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let _ops = TestOps::new(record_drop);
    let domain = job_domain::JobDomain::new();
    let mut source = Frame::allocate_guest_frame();
    let owner = scope.root_nanbox_f64(wrap(4, 1, 0, true, undefined(), undefined()));
    let alias = scope.root_nanbox_f64(method(owner.get_nanbox_f64(), b"borrow"));
    let mut child = Box::new(Frame::allocate_guest_frame());
    let input = child.add(alias.get_nanbox_f64());
    let transferred = owned(&mut child, 4, 2);
    child.guard(|f| lower(f, input, 4, false, false));
    child.guard(|f| lower(f, transferred, 4, true, false));
    let before = owner.get_nanbox_f64().to_bits();
    let enabled = install_loan_init_hook(&scope, 2.0, undefined());
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert!(domain.run(|| abi::perry_component_frame_import_begin_after_return(pointer)) > 0);
        assert!(child.error().is_none());
        assert_eq!(get(data(child.get(transferred)), STATE), TRANSFERRED);
        assert_ne!(before, owner.get_nanbox_f64().to_bits());
        drop(enabled);
        assert_eq!(
            abi::perry_component_frame_resource_scope_end((&raw mut source).cast()),
            0
        );
        drop(source);
        crate::gc::gc_collect_minor();
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 1.0);
        assert!(
            crate::exception::catch_js_throw(|| method(owner.get_nanbox_f64(), b"dispose"))
                .is_err()
        );
        let result = child.add(42.0);
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_resolve(pointer, result)),
            0
        );
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 1.0);
        child.release_import();
        abi::perry_component_frame_drop(pointer);
    }
    assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 0.0);
    method(owner.get_nanbox_f64(), b"dispose");
    method(owner.get_nanbox_f64(), b"dispose");
    assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
}

#[test]
fn throwing_after_return_promise_init_preserves_owner_and_original_relocated_error() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let mut child = Box::new(Frame::allocate_guest_frame());
    let owner = owned(&mut child, 4, 1);
    child.guard(|f| lower(f, owner, 4, true, false));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    let before = error.get_nanbox_f64().to_bits();
    let enabled = install_loan_init_hook(&scope, 0.0, error.get_nanbox_f64());
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_begin_after_return(pointer)),
            0
        );
    }
    assert_ne!(before, error.get_nanbox_f64().to_bits());
    assert_eq!(
        child.error().unwrap().to_bits(),
        error.get_nanbox_f64().to_bits()
    );
    assert!(!child.import_is_deferred());
    assert_eq!(get(data(child.get(owner)), STATE), OWNED);
    drop(enabled);
}
