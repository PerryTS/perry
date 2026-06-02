use perry_runtime::closure::ClosureHeader;

use super::{
    closure_value, js_undefined, js_worker_threads_on, js_worker_threads_post_message,
    set_object_field, string_value_to_string, worker_threads_noop0, CLOSE_CALLBACK,
    MESSAGE_CALLBACK,
};

pub(super) fn worker_parent_port_object() -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "postMessage",
        closure_value(worker_parent_port_post_message as *const u8, 1),
    );
    set_object_field(
        obj,
        "on",
        closure_value(worker_parent_port_on as *const u8, 2),
    );
    set_object_field(
        obj,
        "addListener",
        closure_value(worker_parent_port_on as *const u8, 2),
    );
    set_object_field(
        obj,
        "once",
        closure_value(worker_parent_port_on as *const u8, 2),
    );
    set_object_field(
        obj,
        "off",
        closure_value(worker_parent_port_off as *const u8, 2),
    );
    set_object_field(
        obj,
        "removeListener",
        closure_value(worker_parent_port_off as *const u8, 2),
    );
    set_object_field(
        obj,
        "ref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "unref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    obj
}

extern "C" fn worker_parent_port_post_message(_closure: *const ClosureHeader, value: f64) -> f64 {
    js_worker_threads_post_message(value)
}

extern "C" fn worker_parent_port_on(
    _closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let callback_ptr = perry_runtime::value::js_nanbox_get_pointer(callback) as i64;
    js_worker_threads_on(event.to_bits() as i64, callback_ptr)
}

extern "C" fn worker_parent_port_off(
    _closure: *const ClosureHeader,
    event: f64,
    _callback: f64,
) -> f64 {
    match string_value_to_string(event).unwrap_or_default().as_str() {
        "message" => MESSAGE_CALLBACK.with(|cb| *cb.borrow_mut() = None),
        "close" => CLOSE_CALLBACK.with(|cb| *cb.borrow_mut() = None),
        _ => {}
    }
    js_undefined()
}
