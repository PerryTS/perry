use super::*;

fn assert_rejected(action: impl FnOnce() -> f64, code: Option<&str>, message: &str) {
    let thrown = crate::exception::catch_js_throw(action).expect_err("constructor must reject");
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(thrown);
    let ptr = JSValue::from_bits(error.get_nanbox_f64().to_bits())
        .as_pointer::<crate::error::ErrorHeader>()
        .cast_mut();
    assert_eq!(
        crate::node_submodules::diagnostics::error_code_for_error(ptr),
        code
    );
    assert_eq!(
        crate::string::string_as_str(crate::error::js_error_get_name(ptr)),
        "TypeError"
    );
    assert_eq!(
        crate::string::string_as_str(crate::error::js_error_get_message(ptr)),
        message
    );
}

#[test]
fn constructor_rejections_preserve_distinct_code_and_message_policies() {
    crate::gc::gc_init();
    let this = crate::closure::JsThis::from_f64(js_undefined());
    assert_rejected(
        || js_message_channel_constructor_call_error(std::ptr::null(), this),
        Some("ERR_CONSTRUCT_CALL_REQUIRED"),
        "Cannot call constructor without `new`",
    );
    for construct in [false, true] {
        assert_rejected(
            || {
                if construct {
                    js_message_port_constructor_error()
                } else {
                    js_message_port_constructor_call_error(std::ptr::null(), this)
                }
            },
            Some("ERR_CONSTRUCT_CALL_INVALID"),
            "Constructor cannot be called",
        );
    }
    assert_rejected(
        || js_broadcast_channel_constructor_call_error(std::ptr::null(), this, js_undefined()),
        None,
        "Class constructor BroadcastChannel cannot be invoked without 'new'",
    );
}

#[test]
fn broadcast_constructor_distinguishes_missing_name_and_implicit_symbol_conversion() {
    crate::gc::gc_init();
    assert_rejected(
        || js_broadcast_channel_new_counted(js_undefined(), 0),
        Some("ERR_MISSING_ARGS"),
        "The \"name\" argument must be specified",
    );
    let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
    assert!(crate::exception::catch_js_throw(|| js_broadcast_channel_new(symbol)).is_err());
    let value = js_broadcast_channel_new_counted(js_undefined(), 1);
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = js_string_from_bytes(b"name".as_ptr(), 4);
    let name = object::js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as *const _,
        key,
    );
    assert!(
        JSValue::from_bits(name.to_bits()).is_string(),
        "name must be a string"
    );
    assert_eq!(
        crate::string::string_as_str(
            crate::value::js_get_string_pointer_unified(name) as *const StringHeader,
        ),
        "undefined"
    );
}
