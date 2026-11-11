use super::*;
use crate::gc::RuntimeHandleScope;
use std::sync::atomic::{AtomicUsize, Ordering};

static GETS: AtomicUsize = AtomicUsize::new(0);

fn text(value: &str) -> f64 {
    f64::from_bits(
        JSValue::string_ptr(crate::string::js_string_from_bytes(
            value.as_ptr(),
            value.len() as u32,
        ))
        .bits(),
    )
}

extern "C-unwind" fn stack_getter(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    GETS.fetch_add(1, Ordering::SeqCst);
    if crate::closure::js_closure_get_capture_f64(closure, 0) == 1.0 {
        crate::exception::test_throw(42.0);
    }
    text("getter-stack")
}

fn error_with_stack(scope: &RuntimeHandleScope, getter: Option<bool>) -> f64 {
    let error = crate::error::js_error_new_from_value(text("payload"));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let (name, value) = if let Some(throws) = getter {
        let capture = [if throws { 1.0_f64 } else { 0.0_f64 }.to_bits()];
        let closure = crate::closure::js_closure_alloc_init(
            crate::fn_info!(unwind stack_getter, 0; with_declared(0)),
            1,
            capture.as_ptr(),
        );
        ("get", crate::value::js_nanbox_pointer(closure as i64))
    } else {
        ("value", f64::from_bits(TAG_UNDEFINED))
    };
    crate::object::js_object_set_field_by_name(
        descriptor.get_raw_mut_ptr(),
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32),
        value,
    );
    crate::object::js_object_define_property(
        error.get_nanbox_f64(),
        text("stack"),
        crate::value::js_nanbox_pointer(descriptor.get_raw_mut_ptr::<u8>() as i64),
    );
    error.get_nanbox_f64()
}

fn cloned_stack(error: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let message = unsafe { serialize_message(error.to_bits(), &[], None).unwrap() };
    let restored = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
    let descriptor = scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
        restored.get_nanbox_f64(),
        text("stack"),
    ));
    let enumerable = unsafe {
        crate::value::js_dynamic_object_get_property(
            descriptor.get_nanbox_f64(),
            b"enumerable".as_ptr().cast(),
            10,
        )
    };
    assert_eq!(enumerable.to_bits(), JSValue::bool(false).bits());
    unsafe {
        crate::value::js_dynamic_object_get_property(
            restored.get_nanbox_f64(),
            b"stack".as_ptr().cast(),
            5,
        )
    }
}

#[test]
fn error_stack_clone_reads_a_non_enumerable_getter_once() {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error_with_stack(&scope, Some(false)));
    GETS.store(0, Ordering::SeqCst);
    let stack = cloned_stack(error.get_nanbox_f64());
    assert_eq!(GETS.load(Ordering::SeqCst), 1);
    let ptr =
        crate::value::js_get_string_pointer_unified(stack) as *const crate::string::StringHeader;
    let bytes = unsafe {
        std::slice::from_raw_parts(
            ptr.cast::<u8>()
                .add(std::mem::size_of::<crate::string::StringHeader>()),
            (*ptr).byte_len as usize,
        )
    };
    assert_eq!(bytes, b"getter-stack");
}

#[test]
fn error_stack_clone_propagates_the_original_getter_exception() {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error_with_stack(&scope, Some(true)));
    GETS.store(0, Ordering::SeqCst);
    let result = crate::exception::catch_js_throw(|| unsafe {
        serialize_message(error.get_nanbox_u64(), &[], None)
    });
    assert!(matches!(result, Err(value) if value == 42.0));
    assert_eq!(GETS.load(Ordering::SeqCst), 1);
}

#[test]
fn error_stack_clone_preserves_an_explicitly_undefined_stack() {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error_with_stack(&scope, None));
    assert_eq!(
        cloned_stack(error.get_nanbox_f64()).to_bits(),
        TAG_UNDEFINED
    );
}
