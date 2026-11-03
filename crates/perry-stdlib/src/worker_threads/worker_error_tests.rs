use super::*;
use perry_runtime::gc::RuntimeHandleScope;

extern "C" {
    fn js_shape_run_static_seed();
}

fn marked_error(value: f64) -> worker_error::WorkerErrorMessage {
    js_worker_threads_mark_as_uncloneable(value);
    assert!(clone::try_clone_message(value, js_undefined()).is_err());
    let result = worker_error_message(value);
    result
}

fn diagnostics(
    value: worker_error::WorkerErrorMessage,
) -> (
    Vec<u8>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<Box<SerializedValue>>,
) {
    match value.value {
        SerializedValue::Error {
            name,
            message,
            stack,
            cause,
        } => (name, message, stack, cause),
        _ => panic!("worker must deliver an Error"),
    }
}

#[test]
fn worker_error_fallback_retains_native_family_message_stack_and_cloneable_cause() {
    unsafe {
        js_shape_run_static_seed();
    }
    use perry_runtime::error::*;
    for (kind, name) in [
        (ERROR_KIND_ERROR, "Error"),
        (ERROR_KIND_TYPE_ERROR, "TypeError"),
        (ERROR_KIND_RANGE_ERROR, "RangeError"),
        (ERROR_KIND_REFERENCE_ERROR, "ReferenceError"),
        (ERROR_KIND_SYNTAX_ERROR, "SyntaxError"),
        (ERROR_KIND_EVAL_ERROR, "EvalError"),
        (ERROR_KIND_URI_ERROR, "URIError"),
    ] {
        let scope = RuntimeHandleScope::new();
        let error = js_error_new_kind_from_value(kind, string_value("details"));
        let error = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(error as i64));
        set_object_field_from_value_for_test(
            error.get_nanbox_f64(),
            "stack",
            string_value("saved stack"),
        );
        set_object_field_from_value_for_test(
            error.get_nanbox_f64(),
            "cause",
            string_value("inner"),
        );
        let (actual, message, stack, cause) = diagnostics(marked_error(error.get_nanbox_f64()));
        assert_eq!(actual, name.as_bytes());
        assert_eq!(message.as_deref(), Some(b"details".as_slice()));
        assert_eq!(stack.as_deref(), Some(b"saved stack".as_slice()));
        let cause = cause.expect("cloneable cause survives");
        let value = unsafe { deserialize_nanbox_on_current_thread(&cause) };
        assert_eq!(
            string_value_to_string(f64::from_bits(value)).as_deref(),
            Some("inner")
        );
    }
}

fn set_object_field_from_value_for_test(value: f64, name: &str, field: f64) {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let field = scope.root_nanbox_f64(field);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_set_field_by_name(
        perry_runtime::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as *mut _,
        key,
        field.get_nanbox_f64(),
    );
}

#[test]
fn worker_error_fallback_does_not_promote_a_non_error_with_diagnostic_fields() {
    unsafe {
        js_shape_run_static_seed();
    }
    let scope = RuntimeHandleScope::new();
    let object = perry_runtime::object::js_object_alloc(0, 0);
    let object = scope.root_nanbox_f64(object_value(object));
    set_object_field_from_value_for_test(
        object.get_nanbox_f64(),
        "name",
        string_value("TypeError"),
    );
    set_object_field_from_value_for_test(object.get_nanbox_f64(), "message", string_value("fake"));
    let (name, message, stack, cause) = diagnostics(marked_error(object.get_nanbox_f64()));
    assert_eq!(name, b"Error");
    assert_eq!(
        message.as_deref(),
        Some(b"the worker threw a value that could not be cloned".as_slice())
    );
    assert!(stack.is_none());
    assert!(cause.is_none());
}

extern "C" fn uncloneable_function(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    js_undefined()
}

#[test]
fn worker_error_fallback_discards_only_an_uncloneable_cause() {
    unsafe {
        js_shape_run_static_seed();
    }
    let scope = RuntimeHandleScope::new();
    let closure = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(uncloneable_function, 0; with_declared(0)),
        0,
    );
    let cause = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(closure as i64));
    let error = perry_runtime::error::js_error_new_with_cause_from_value(
        string_value("original"),
        cause.get_nanbox_f64(),
    );
    let error = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(error as i64));
    assert!(clone::try_clone_message(error.get_nanbox_f64(), js_undefined()).is_err());
    let (name, message, stack, cause) = diagnostics(worker_error_message(error.get_nanbox_f64()));
    assert_eq!(name, b"Error");
    assert_eq!(message.as_deref(), Some(b"original".as_slice()));
    assert!(stack.is_some());
    assert!(cause.is_none());
}

#[test]
fn worker_error_code_survives_clone_and_diagnostic_fallback() {
    unsafe {
        js_shape_run_static_seed();
    }
    for fallback in [false, true] {
        let scope = RuntimeHandleScope::new();
        let error = perry_runtime::error::js_error_new_kind_from_value(
            perry_runtime::error::ERROR_KIND_TYPE_ERROR,
            string_value("details"),
        );
        let error = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(error as i64));
        set_object_field_from_value_for_test(
            error.get_nanbox_f64(),
            "code",
            string_value("ERR_WORKER_PATH"),
        );
        let ordinary = clone::try_clone_message(error.get_nanbox_f64(), js_undefined()).unwrap();
        let message = if fallback {
            marked_error(error.get_nanbox_f64())
        } else {
            worker_error_message(error.get_nanbox_f64())
        };
        let restored = scope.root_nanbox_f64(message.deserialize());
        for (key, expected) in [
            ("name", "TypeError"),
            ("message", "details"),
            ("code", "ERR_WORKER_PATH"),
        ] {
            assert_eq!(
                string_value_to_string(get_object_field_from_value(restored.get_nanbox_f64(), key))
                    .as_deref(),
                Some(expected)
            );
        }
        let ordinary = f64::from_bits(unsafe { deserialize_nanbox_on_current_thread(&ordinary) });
        assert!(is_undefined(get_object_field_from_value(ordinary, "code")));
    }
}

#[test]
fn native_worker_diagnostic_code_survives_error_delivery() {
    unsafe {
        js_shape_run_static_seed();
    }
    for fallback in [false, true] {
        let scope = RuntimeHandleScope::new();
        let error = unsafe {
            perry_runtime::error::js_error_value_with_code(
                b"not compiled".as_ptr(),
                12,
                b"ERR_WORKER_NOT_COMPILED".as_ptr(),
                b"ERR_WORKER_NOT_COMPILED".len(),
                0,
            )
        };
        let error = scope.root_nanbox_f64(error);
        let message = if fallback {
            marked_error(error.get_nanbox_f64())
        } else {
            worker_error_message(error.get_nanbox_f64())
        };
        let restored = scope.root_nanbox_f64(message.deserialize());
        for (key, expected) in [
            ("name", "Error"),
            ("message", "not compiled"),
            ("code", "ERR_WORKER_NOT_COMPILED"),
        ] {
            assert_eq!(
                string_value_to_string(get_object_field_from_value(restored.get_nanbox_f64(), key))
                    .as_deref(),
                Some(expected)
            );
        }
    }
}
