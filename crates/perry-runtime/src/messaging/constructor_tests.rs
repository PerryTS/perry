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
