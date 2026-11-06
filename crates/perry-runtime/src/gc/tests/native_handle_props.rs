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
    {
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
    {
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

extern "C" fn collecting_event_target_value(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure as *mut crate::closure::ClosureHeader);
    if crate::closure::js_closure_get_capture_f64(closure.get_raw_mut_ptr(), 1) == 0.0 {
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        crate::closure::js_closure_set_capture_f64(closure.get_raw_mut_ptr(), 1, 1.0);
    }
    crate::closure::js_closure_get_capture_f64(closure.get_raw_mut_ptr(), 0)
}

extern "C" fn event_target_handler_identity(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _: f64,
) -> f64 {
    this.as_f64()
}

#[test]
fn event_target_setter_roots_handler_across_constructor_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    gc_register_mutable_root_scanner_named("native-frame-test", scan_runtime_handle_roots_mut);
    crate::event_target_hook_slots::install();
    crate::object::js_get_global_this();
    let scope = RuntimeHandleScope::new();
    let name = crate::string::js_string_from_bytes(b"events".as_ptr(), 6);
    let channel = scope.root_raw_mut_ptr(crate::messaging::broadcast_channel_object_with_id(
        0,
        crate::value::js_nanbox_string(name as i64),
    ));
    let constructor_name = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    let constructor = scope.root_nanbox_f64(crate::object::js_object_get_field_by_name_f64(
        channel.get_raw_mut_ptr(),
        constructor_name,
    ));
    let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(collecting_event_target_value, 0),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(
        getter.get_raw_mut_ptr(),
        0,
        constructor.get_nanbox_f64(),
    );
    crate::closure::js_closure_set_capture_f64(getter.get_raw_mut_ptr(), 1, 0.0);
    crate::object::js_object_define_getter(
        crate::value::js_nanbox_pointer(
            channel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
        ),
        crate::value::js_nanbox_string(constructor_name as i64),
        crate::value::js_nanbox_pointer(
            getter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let handler = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(event_target_handler_identity, 1),
        0,
    ));
    let before = handler.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize;
    let field = crate::string::js_string_from_bytes(b"onmessage".as_ptr(), 9);
    crate::object::js_object_set_field_by_name(
        channel.get_raw_mut_ptr(),
        field,
        crate::value::js_nanbox_pointer(before as i64),
    );
    assert_ne!(
        before,
        handler.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        "the setter must cross a moving collection"
    );
    let field = crate::string::js_string_from_bytes(b"onmessage".as_ptr(), 9);
    let actual = crate::object::js_object_get_field_by_name_f64(channel.get_raw_mut_ptr(), field);
    assert_eq!(
        actual.to_bits(),
        crate::value::js_nanbox_pointer(
            handler.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        )
        .to_bits()
    );
}

#[test]
fn event_target_wrapper_roots_receiver_across_handler_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    gc_register_mutable_root_scanner_named("native-frame-test", scan_runtime_handle_roots_mut);
    crate::event_target_hook_slots::install();
    crate::object::js_get_global_this();
    let scope = RuntimeHandleScope::new();
    let name = crate::string::js_string_from_bytes(b"events".as_ptr(), 6);
    let channel = scope.root_raw_mut_ptr(crate::messaging::broadcast_channel_object_with_id(
        0,
        crate::value::js_nanbox_string(name as i64),
    ));
    let handler = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(event_target_handler_identity, 1),
        0,
    ));
    let field = crate::string::js_string_from_bytes(b"onmessage".as_ptr(), 9);
    crate::object::js_object_set_field_by_name(
        channel.get_raw_mut_ptr(),
        field,
        crate::value::js_nanbox_pointer(
            handler.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let event_name = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let listeners = unsafe {
        crate::event_target::js_event_target_get_event_listeners(
            channel.get_raw_mut_ptr(),
            event_name,
        )
    };
    let wrapper = scope.root_nanbox_f64(f64::from_bits(
        crate::array::js_array_get(listeners, 0).bits(),
    ));
    let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(collecting_event_target_value, 0),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(
        getter.get_raw_mut_ptr(),
        0,
        crate::value::js_nanbox_pointer(
            handler.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    crate::closure::js_closure_set_capture_f64(getter.get_raw_mut_ptr(), 1, 0.0);
    let name = crate::string::js_string_from_bytes(b"handler".as_ptr(), 7);
    crate::object::js_object_define_getter(
        wrapper.get_nanbox_f64(),
        crate::value::js_nanbox_string(name as i64),
        crate::value::js_nanbox_pointer(
            getter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let before = channel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize;
    let result = unsafe {
        crate::closure::js_native_call_value(
            wrapper.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(crate::value::js_nanbox_pointer(before as i64)),
            [f64::from_bits(crate::value::TAG_UNDEFINED)].as_ptr(),
            1,
        )
    };
    assert_ne!(
        before,
        channel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize
    );
    assert_eq!(
        result.to_bits(),
        crate::value::js_nanbox_pointer(
            channel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
        )
        .to_bits()
    );
}
