//! Moving-GC regression for the runtime's bound-method value builder (#8036).

use super::super::super::*;
use super::super::support::*;

#[test]
fn bound_method_builder_reloads_closure_and_receiver_after_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    let receiver = crate::object::js_object_alloc(0, 0);
    assert!(!receiver.is_null());
    let receiver_before = receiver as usize;
    let receiver_value = crate::value::js_nanbox_pointer(receiver as i64);

    crate::object::test_collect_bound_method_after_capture_init();
    let cycles_before = copying_minor_cycles();
    let method =
        crate::object::build_bound_method_closure(receiver_value, b"reflectGet".as_ptr(), 10);
    let cycles_after = copying_minor_cycles();
    let method_after = (method.to_bits() & crate::value::POINTER_MASK) as usize;
    let (method_before, method_during) = crate::object::test_take_bound_method_move();

    assert!(
        cycles_after > cycles_before,
        "the test hook must run a copying minor inside the builder"
    );
    assert_ne!(
        method_before, method_during,
        "the collection must move the just-created method closure"
    );
    assert_eq!(
        method_after, method_during,
        "the builder must return the handle's post-collection closure address"
    );

    let captured_receiver = crate::closure::js_closure_get_capture_f64(
        method_after as *const crate::closure::ClosureHeader,
        0,
    );
    let receiver_after = (captured_receiver.to_bits() & crate::value::POINTER_MASK) as usize;
    assert_ne!(
        receiver_before, receiver_after,
        "the test must also move the captured receiver"
    );
    assert!(
        crate::value::addr_class::is_valid_obj_ptr(receiver_after as *const u8),
        "the method must capture the receiver's post-collection address"
    );
}
