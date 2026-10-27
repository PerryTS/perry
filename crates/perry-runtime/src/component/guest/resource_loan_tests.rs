use super::cleanup_tests::{method, record_drop, TestOps, COLLECTED};
use super::*;
use crate::async_context::job_domain;
use crate::component::guest::frame as abi;
use std::sync::atomic::Ordering;

#[test]
fn committed_loans_block_disposal_and_transfer_until_every_borrower_retires() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let _ops = TestOps::new(record_drop);
    let owner = scope.root_nanbox_f64(wrap(4, 1, 0, true, undefined(), undefined()));
    let alias = scope.root_nanbox_f64(method(owner.get_nanbox_f64(), b"borrow"));
    let mut borrowers = Vec::new();
    for _ in 0..2 {
        let mut frame = Frame::new();
        for resource in [owner.get_nanbox_f64(), alias.get_nanbox_f64()] {
            let slot = frame.add(resource);
            assert_eq!(frame.guard(|f| lower(f, slot, 4, false, false)), 1);
        }
        frame.guard(commit);
        frame.guard(commit); // A repeat commit cannot acquire another lease.
        assert!(frame.error().is_none());
        borrowers.push(frame);
    }
    assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 2.0);
    let before = owner.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(before, owner.get_nanbox_f64().to_bits());
    for remaining in [2.0, 1.0] {
        assert!(
            crate::exception::catch_js_throw(|| method(owner.get_nanbox_f64(), b"dispose"))
                .is_err()
        );
        let mut transfer = Frame::new();
        let slot = transfer.add(owner.get_nanbox_f64());
        transfer.guard(|f| lower(f, slot, 4, true, false));
        assert!(transfer.error().is_some());
        assert_eq!(get(data(owner.get_nanbox_f64()), STATE), OWNED);
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), remaining);
        assert_eq!(COLLECTED.load(Ordering::SeqCst), 0);
        drop(borrowers.pop());
    }
    assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 0.0);
    method(owner.get_nanbox_f64(), b"dispose");
    method(owner.get_nanbox_f64(), b"dispose");
    assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
}

#[test]
fn a_loan_started_after_lowering_prevents_partial_ownership_commit() {
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut transfer = Frame::new();
    let a = owned(&mut transfer, 4, 1);
    let b = owned(&mut transfer, 4, 2);
    for slot in [a, b] {
        transfer.guard(|f| lower(f, slot, 4, true, false));
    }
    let mut borrower = Frame::new();
    let slot = borrower.add(transfer.get(b));
    borrower.guard(|f| lower(f, slot, 4, false, false));
    borrower.guard(commit);
    transfer.guard(commit);
    assert!(transfer.error().is_some());
    assert_eq!(get(data(transfer.get(a)), STATE), OWNED);
    assert_eq!(get(data(transfer.get(b)), STATE), OWNED);
    drop(borrower);
    let mut retry = Frame::new();
    for original in [a, b] {
        let slot = retry.add(transfer.get(original));
        retry.guard(|f| lower(f, slot, 4, true, false));
    }
    retry.guard(commit);
    assert!(retry.error().is_none());
}

#[test]
fn cancelling_the_parent_and_failing_the_import_do_not_release_unacknowledged_loans() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let owner = scope.root_nanbox_f64(wrap(4, 1, 0, true, undefined(), undefined()));
    unsafe extern "C" fn begin(data: *mut c_void) {
        let (pointer, promise) = &mut *data.cast::<(*mut c_void, u32)>();
        *promise = abi::perry_component_frame_import_begin(*pointer);
    }
    unsafe {
        let parent = abi::perry_component_frame_task_new();
        let mut child = Box::new(Frame::new());
        let resource = child.add(owner.get_nanbox_f64());
        child.guard(|f| lower(f, resource, 4, false, false));
        let pointer = (&raw mut *child).cast();
        let mut args = (pointer, 0u32);
        assert_eq!(
            abi::perry_component_frame_task_resume(parent, Some(begin), (&raw mut args).cast()),
            0
        );
        assert!(args.1 > 0);
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 1.0);
        child.release_import();
        assert_eq!(abi::perry_component_frame_task_cancel(parent), 0);
        abi::perry_component_frame_drop(parent);
        crate::gc::gc_collect_minor();
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 1.0);
        let value = abi::perry_component_frame_number(pointer, 42.0);
        assert_eq!(
            abi::perry_component_frame_import_resolve(pointer, value),
            -1
        );
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 1.0);
        let promise = JSValue::from_bits(abi::frame(pointer).get(args.1).to_bits())
            .as_pointer::<crate::promise::Promise>();
        assert_eq!((*promise).state, crate::promise::PromiseState::Pending);
        // C owns the failed frame until its native child acknowledges. Drop
        // must release the lease even though the frame error boundary is inert.
        abi::perry_component_frame_drop(pointer);
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 0.0);
        let mut transfer = Frame::new();
        let slot = transfer.add(owner.get_nanbox_f64());
        transfer.guard(|f| lower(f, slot, 4, true, false));
        transfer.guard(commit);
        assert!(transfer.error().is_none());
    }
}

#[test]
fn promise_settlement_keeps_the_native_loan_until_frame_retirement() {
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let domain = job_domain::JobDomain::new();
    let mut child = Box::new(Frame::new());
    let resource = owned(&mut child, 4, 1);
    child.guard(|f| lower(f, resource, 4, false, false));
    unsafe {
        let pointer = (&raw mut *child).cast();
        assert!(domain.run(|| abi::perry_component_frame_import_begin(pointer)) > 0);
        let owner = scope.root_nanbox_f64(child.get(resource));
        let value = abi::perry_component_frame_number(pointer, 42.0);
        assert_eq!(
            domain.run(|| abi::perry_component_frame_import_resolve(pointer, value)),
            0
        );
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 1.0);
        child.release_import();
        abi::perry_component_frame_drop(pointer);
        assert_eq!(get(data(owner.get_nanbox_f64()), BORROW_COUNT), 0.0);
        let mut transfer = Frame::new();
        let slot = transfer.add(owner.get_nanbox_f64());
        transfer.guard(|f| lower(f, slot, 4, true, false));
        transfer.guard(commit);
        assert!(transfer.error().is_none());
    }
}

#[test]
fn loan_retirement_uses_rooted_private_state_after_the_current_registry_changes() {
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut borrower = Frame::new();
    let slot = owned(&mut borrower, 4, 1);
    borrower.guard(|f| lower(f, slot, 4, false, false));
    borrower.guard(commit);
    let fields = scope.root_nanbox_f64(data(borrower.get(slot)));
    assert_eq!(get(fields.get_nanbox_f64(), BORROW_COUNT), 1.0);
    let _other = Registry::new(&scope);
    drop(borrower);
    assert_eq!(get(fields.get_nanbox_f64(), BORROW_COUNT), 0.0);
}

#[test]
fn a_full_loan_counter_rejects_the_whole_commit_before_consuming_an_owner() {
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut frame = Frame::new();
    let a = owned(&mut frame, 4, 1);
    let b = owned(&mut frame, 4, 2);
    frame.guard(|f| lower(f, a, 4, true, false));
    frame.guard(|f| lower(f, b, 4, false, false));
    set(data(frame.get(b)), BORROW_COUNT, u32::MAX as f64);
    frame.guard(commit);
    assert!(frame.error().is_some());
    assert_eq!(get(data(frame.get(a)), STATE), OWNED);
    assert_eq!(get(data(frame.get(b)), BORROW_COUNT), u32::MAX as f64);
    set(data(frame.get(b)), BORROW_COUNT, 0.0);
}

#[path = "resource_loan_tests/publication.rs"]
mod publication;
