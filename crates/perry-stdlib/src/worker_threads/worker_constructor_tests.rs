use super::*;
use perry_runtime::gc::RuntimeHandleScope;

extern "C" {
    fn js_shape_run_static_seed();
}

fn set_field(value: f64, name: &str, field: f64) {
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

fn assert_constructor_error(options: f64, name: &str, code: &str) {
    let scope = RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let result = perry_runtime::exception::catch_js_throw(|| {
        // Rejection must precede starting a thread or dereferencing an entry.
        js_worker_threads_worker_new(0, options.get_nanbox_f64())
    });
    let error = scope.root_nanbox_f64(result.expect_err("constructor must reject options"));
    for (field, expected) in [("name", name), ("code", code)] {
        let actual = get_object_field_from_value(error.get_nanbox_f64(), field);
        if field == "code" && code == "25" {
            assert_eq!(JSValue::from_bits(actual.to_bits()).as_number(), 25.0);
        } else {
            assert_eq!(string_value_to_string(actual).as_deref(), Some(expected));
        }
    }
}

#[test]
fn constructor_rejects_non_string_names_before_transferring_worker_data() {
    unsafe { js_shape_run_static_seed() };
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
    for name in [object.get_nanbox_f64(), js_bool(true), 42.0] {
        let options =
            scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
        let buffer = perry_runtime::buffer::js_array_buffer_new(8);
        let buffer = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(buffer as i64));
        let list = perry_runtime::array::js_array_alloc(1);
        let list = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(list as i64));
        perry_runtime::array::js_array_push_f64(
            array_ptr_from_value(list.get_nanbox_f64()).unwrap(),
            buffer.get_nanbox_f64(),
        );
        set_field(options.get_nanbox_f64(), "name", name);
        set_field(
            options.get_nanbox_f64(),
            "workerData",
            buffer.get_nanbox_f64(),
        );
        set_field(
            options.get_nanbox_f64(),
            "transferList",
            list.get_nanbox_f64(),
        );
        assert_constructor_error(
            options.get_nanbox_f64(),
            "TypeError",
            "ERR_INVALID_ARG_TYPE",
        );
        assert_eq!(
            perry_runtime::buffer::js_buffer_length(perry_runtime::value::js_nanbox_get_pointer(
                buffer.get_nanbox_f64()
            ) as *const _),
            8
        );
    }
}

#[test]
fn constructor_validates_transfer_list_without_worker_data() {
    unsafe { js_shape_run_static_seed() };
    // Dependency crates are not built with cfg(test), so install the DOM
    // exception provider normally selected by the generated executable.
    perry_runtime::feature_hooks::js_runtime_install_event_target();
    let scope = RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
    let list = perry_runtime::array::js_array_alloc(1);
    perry_runtime::array::js_array_push_f64(list, f64::from_bits(JSValue::null().bits()));
    set_field(
        options.get_nanbox_f64(),
        "transferList",
        perry_runtime::value::js_nanbox_pointer(list as i64),
    );
    assert_constructor_error(options.get_nanbox_f64(), "DataCloneError", "25");
}

#[test]
fn worker_options_keep_default_and_valid_string_names() {
    unsafe { js_shape_run_static_seed() };
    assert_eq!(WorkerOptions::from_value(js_undefined()).thread_name, "");
    let scope = RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
    assert_eq!(
        WorkerOptions::from_value(options.get_nanbox_f64()).thread_name,
        ""
    );
    for name in [
        js_undefined(),
        js_bool(false),
        0.0,
        f64::NAN,
        f64::from_bits(JSValue::null().bits()),
        string_value(""),
    ] {
        set_field(options.get_nanbox_f64(), "name", name);
        assert_eq!(
            WorkerOptions::from_value(options.get_nanbox_f64()).thread_name,
            ""
        );
    }
    set_field(options.get_nanbox_f64(), "name", string_value("worker"));
    assert_eq!(
        WorkerOptions::from_value(options.get_nanbox_f64()).thread_name,
        "worker"
    );
}
