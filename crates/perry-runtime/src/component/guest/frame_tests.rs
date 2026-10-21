use super::*;

extern "C-unwind" fn returned_array(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(1) as i64)
}

#[test]
fn export_result_root_outlives_call_temporary_handles() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
    let previous = scope.root_nanbox_f64(super::super::maybe_registry());
    let world = serde_json::json!({"exports":[{"name":"", "functions":[
        {"name":"array", "params":[]}
    ]}]});
    let owner = scope.root_nanbox_f64(context::publish(world, undefined()));
    let exports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let callback = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind returned_array, 0),
        0,
    ) as i64);
    set_field(exports.get_nanbox_f64(), b"array", callback);
    unsafe {
        use crate::object::owned_state_bag::{self as bag, COMPONENT_EXPORTS, COMPONENT_GUEST};
        bag::store(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_EXPORTS,
            exports.get_nanbox_f64(),
        );
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            owner.get_nanbox_f64(),
        );
        let frame = perry_component_frame_new();
        let args = perry_component_frame_array(frame, 0);
        let result = perry_component_frame_call(frame, 0, 0, args);
        assert_eq!(perry_component_frame_failed(frame), 0);
        let before = super::frame(frame).get(result).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            super::frame(frame).get(result).to_bits(),
            before,
            "the exported result must move after its call scope has ended"
        );
        assert_eq!(perry_component_frame_array_len(frame, result), 1);
        perry_component_frame_drop(frame);
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            previous.get_nanbox_f64(),
        );
    }
}

#[test]
fn adapter_slots_are_rewritten_by_a_real_moving_collection() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    unsafe {
        let frame = perry_component_frame_new();
        let text = "A heap string that must survive collection 🌍";
        let string = perry_component_frame_string(frame, text.as_ptr(), text.len());
        let list = perry_component_frame_array(frame, 1);
        perry_component_frame_array_set(frame, list, 0, string);
        let before = super::frame(frame).get(list).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            super::frame(frame).get(list).to_bits(),
            before,
            "the subject array must move"
        );
        let value = perry_component_frame_array_get(frame, list, 0);
        let mut bytes = vec![0; perry_component_frame_string_len(frame, value)];
        perry_component_frame_string_copy(frame, value, bytes.as_mut_ptr(), bytes.len());
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), text);
        assert_eq!(perry_component_frame_failed(frame), 0);
        perry_component_frame_drop(frame);
    }
}

#[test]
fn failed_frame_keeps_the_original_heap_exception_rooted() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let mut frame = Frame::new();
    frame.guard::<()>(|_| {
        let thrown =
            crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(3) as i64);
        crate::exception::js_throw(thrown);
    });
    let before = frame.error().unwrap().to_bits();
    crate::gc::gc_collect_minor();
    let after = frame.error().unwrap();
    assert_ne!(
        after.to_bits(),
        before,
        "the original heap exception must move"
    );
    assert_eq!(crate::array::js_array_length(array(after)), 3);
}

#[test]
fn scalar_limits_and_errors_never_unwind_through_the_adapter() {
    unsafe {
        let frame = perry_component_frame_new();
        let unsigned = perry_component_frame_u64(frame, u64::MAX);
        let signed = perry_component_frame_s64(frame, i64::MIN);
        assert_eq!(perry_component_frame_get_u64(frame, unsigned), u64::MAX);
        assert_eq!(perry_component_frame_get_s64(frame, signed), i64::MIN);
        assert_eq!(perry_component_frame_failed(frame), 0);
        assert_eq!(perry_component_frame_get_s64(frame, unsigned), 0);
        assert_eq!(perry_component_frame_failed(frame), 1);
        let first_error = super::frame(frame).error().unwrap();
        // A failed frame is inert and retains the first error.
        assert_eq!(perry_component_frame_object(frame), 0);
        assert_eq!(
            super::frame(frame).error().unwrap().to_bits(),
            first_error.to_bits()
        );
        perry_component_frame_drop(frame);

        let frame = perry_component_frame_new();
        let number = perry_component_frame_number(frame, 256.0);
        assert_eq!(
            perry_component_frame_get_integer(frame, number, 0.0, 255.0),
            0.0
        );
        assert_eq!(perry_component_frame_failed(frame), 1);
        perry_component_frame_drop(frame);
    }
}

#[test]
fn adapter_copies_strings_bytes_and_unicode_scalars() {
    unsafe {
        let frame = perry_component_frame_new();
        let data = [0, 127, 255];
        let bytes = perry_component_frame_bytes(frame, data.as_ptr(), data.len());
        let mut out = [0; 3];
        assert_eq!(perry_component_frame_bytes_len(frame, bytes), 3);
        perry_component_frame_bytes_copy(frame, bytes, out.as_mut_ptr(), out.len());
        assert_eq!(out, data);
        let character = perry_component_frame_char(frame, 0x1f30d);
        assert_eq!(perry_component_frame_get_char(frame, character), 0x1f30d);
        assert_eq!(perry_component_frame_failed(frame), 0);
        // Invalid Unicode is an adapter error, not a replacement character.
        perry_component_frame_char(frame, 0xd800);
        assert_eq!(perry_component_frame_failed(frame), 1);
        perry_component_frame_drop(frame);
    }
}
