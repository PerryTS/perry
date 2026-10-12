//! A C-frame native owner retains and rewrites only its own bag slot.
use super::super::*;
use super::support::*;
#[test]
fn native_handle_property_bag_moves_and_releases_its_values() {
    check(false);
}
#[test]
#[ignore = "negative control: remove the native owner's bag slot"]
fn negative_control_objtables_native_handle_props() {
    check(true);
}
fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    gc_register_mutable_root_scanner_named("native-frame-test", scan_runtime_handle_roots_mut);
    crate::weakref::test_support::clear_weak_holders();
    let native = crate::object::handle_expando::test_support::TestNativeOwner::new();
    let value = crate::object::js_object_alloc(0, 1);
    unsafe {
        crate::object::js_object_set_field_f64(value, 0, 73.0);
    }
    let boxed = f64::from_bits(ptr_bits(value as usize));
    crate::object::handle_expando::handle_expando_set(native.handle(), "payload", boxed);
    let before = crate::object::handle_expando::handle_property_bag(native.handle());
    let weak = crate::weakref::js_weakref_new(boxed);
    js_shadow_slot_set(0, ptr_bits(weak as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(
        before,
        crate::object::handle_expando::handle_property_bag(native.handle())
    );
    if erase {
        native.clear();
    }
    js_gc_collect();
    let retained = crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(0)));
    assert_ne!(
        retained.to_bits(),
        crate::value::TAG_UNDEFINED,
        "native bag is the only strong edge"
    );
    let stored =
        crate::object::handle_expando::handle_expando_get(native.handle(), "payload").unwrap();
    assert_eq!(stored.to_bits(), retained.to_bits());
    unsafe {
        assert_eq!(
            crate::object::js_object_get_field_f64(
                crate::value::js_nanbox_get_pointer(stored) as *mut crate::object::ObjectHeader,
                0
            ),
            73.0
        );
    }
    native.clear();
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(0))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
