use super::*;
use crate::gc::RuntimeHandleScope;

fn text(value: &str) -> f64 {
    f64::from_bits(
        JSValue::string_ptr(crate::string::js_string_from_bytes(
            value.as_ptr(),
            value.len() as u32,
        ))
        .bits(),
    )
}

fn get(value: f64, key: &str) -> f64 {
    unsafe { crate::value::js_dynamic_object_get_property(value, key.as_ptr().cast(), key.len()) }
}

#[test]
fn error_cause_clone_retains_assigned_self_reference() {
    let scope = RuntimeHandleScope::new();
    let error = crate::error::js_error_new_from_value(text("payload"));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    let key = crate::string::js_string_from_bytes(b"cause".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(error.get_nanbox_f64()) as *mut _,
        key,
        error.get_nanbox_f64(),
    );
    let message = unsafe { serialize_message(error.get_nanbox_u64(), &[], None).unwrap() };
    let restored = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
    assert_eq!(
        get(restored.get_nanbox_f64(), "cause").to_bits(),
        restored.get_nanbox_u64()
    );
}

#[test]
fn error_cause_native_constructor_has_a_data_descriptor() {
    let scope = RuntimeHandleScope::new();
    let error = crate::error::js_error_new_with_cause_from_value(text("payload"), 23.0);
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    let descriptor = scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
        error.get_nanbox_f64(),
        text("cause"),
    ));
    assert_ne!(descriptor.get_nanbox_u64(), TAG_UNDEFINED);
    assert_eq!(get(descriptor.get_nanbox_f64(), "value"), 23.0);
    assert_eq!(
        get(descriptor.get_nanbox_f64(), "enumerable").to_bits(),
        JSValue::bool(false).bits()
    );
    let message = unsafe { serialize_message(error.get_nanbox_u64(), &[], None).unwrap() };
    let restored = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
    assert_eq!(get(restored.get_nanbox_f64(), "cause"), 23.0);
}

#[test]
fn error_cause_clone_retains_an_own_undefined_data_property() {
    let scope = RuntimeHandleScope::new();
    let error = crate::error::js_error_new_from_value(text("payload"));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    let key = crate::string::js_string_from_bytes(b"cause".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(error.get_nanbox_f64()) as *mut _,
        key,
        f64::from_bits(TAG_UNDEFINED),
    );
    let message = unsafe { serialize_message(error.get_nanbox_u64(), &[], None).unwrap() };
    let restored = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
    let descriptor = scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
        restored.get_nanbox_f64(),
        text("cause"),
    ));
    assert_ne!(descriptor.get_nanbox_u64(), TAG_UNDEFINED);
    assert_eq!(
        get(descriptor.get_nanbox_f64(), "value").to_bits(),
        TAG_UNDEFINED
    );
}

static CAUSE_GETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C-unwind" fn cause_getter(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    CAUSE_GETS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if crate::closure::js_closure_get_capture_f64(closure, 0) == 1.0 {
        crate::exception::test_throw(42.0);
    }
    23.0
}

#[test]
fn error_cause_clone_ignores_returning_and_throwing_accessors() {
    for throws in [false, true] {
        let scope = RuntimeHandleScope::new();
        let error = crate::error::js_error_new_from_value(text("payload"));
        let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
        let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let captures = [if throws { 1.0_f64 } else { 0.0_f64 }.to_bits()];
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc_init(
            crate::fn_info!(unwind cause_getter, 0; with_declared(0)),
            1,
            captures.as_ptr(),
        ));
        let key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            descriptor.get_raw_mut_ptr(),
            key,
            crate::value::js_nanbox_pointer(getter.get_raw_mut_ptr::<u8>() as i64),
        );
        crate::object::js_object_define_property(
            error.get_nanbox_f64(),
            text("cause"),
            crate::value::js_nanbox_pointer(descriptor.get_raw_mut_ptr::<u8>() as i64),
        );
        CAUSE_GETS.store(0, std::sync::atomic::Ordering::SeqCst);
        let message = crate::exception::catch_js_throw(|| unsafe {
            serialize_message(error.get_nanbox_u64(), &[], None).unwrap()
        })
        .expect("cause accessors are not read");
        let restored =
            scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
        assert_eq!(CAUSE_GETS.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            crate::object::js_object_get_own_property_descriptor(
                restored.get_nanbox_f64(),
                text("cause"),
            )
            .to_bits(),
            TAG_UNDEFINED
        );
    }
}

extern "C" fn replace_cause_and_collect(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(this.as_f64());
    let key = crate::string::js_string_from_bytes(b"cause".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(error.get_nanbox_f64()) as *mut _,
        key,
        99.0,
    );
    unsafe {
        crate::gc::gc_collect_minor();
    }
    text("stack")
}

#[test]
fn error_cause_clone_snapshots_data_before_a_collecting_stack_getter() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let original = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let error = crate::error::js_error_new_with_cause_from_value(
        text("payload"),
        crate::value::js_nanbox_pointer(original.get_raw_mut_ptr::<u8>() as i64),
    );
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(replace_cause_and_collect, 0; with_declared(0)),
        0,
    ));
    let key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
    crate::object::js_object_set_field_by_name(
        descriptor.get_raw_mut_ptr(),
        key,
        crate::value::js_nanbox_pointer(getter.get_raw_mut_ptr::<u8>() as i64),
    );
    crate::object::js_object_define_property(
        error.get_nanbox_f64(),
        text("stack"),
        crate::value::js_nanbox_pointer(descriptor.get_raw_mut_ptr::<u8>() as i64),
    );
    let before = crate::gc::moved_objects_total();
    let message = unsafe { serialize_message(error.get_nanbox_u64(), &[], None).unwrap() };
    assert!(
        crate::gc::moved_objects_total() > before,
        "the getter must collect live cells"
    );
    assert_eq!(get(error.get_nanbox_f64(), "cause"), 99.0);
    let restored = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
    let cause = get(restored.get_nanbox_f64(), "cause");
    assert!(
        JSValue::from_bits(cause.to_bits()).is_pointer(),
        "clone retains the original object cause"
    );
}
