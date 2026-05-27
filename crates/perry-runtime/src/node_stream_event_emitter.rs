use crate::closure::ClosureHeader;
use crate::value::JSValue;

pub(super) extern "C" fn ns_set_max_listeners(closure: *const ClosureHeader, value: f64) -> f64 {
    set_stream_max_listeners(super::this_value(closure), value)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_set_max_listeners(stream_handle: i64, value: f64) -> f64 {
    set_stream_max_listeners(super::stream_value_from_handle(stream_handle), value)
}

fn set_stream_max_listeners(stream: f64, value: f64) -> f64 {
    super::set_hidden_value(stream, super::hidden_max_listeners_key(), value);
    stream
}

pub(super) extern "C" fn ns_get_max_listeners(closure: *const ClosureHeader) -> f64 {
    stream_max_listeners(super::this_value(closure))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_get_max_listeners(stream_handle: i64) -> f64 {
    stream_max_listeners(super::stream_value_from_handle(stream_handle))
}

fn stream_max_listeners(stream: f64) -> f64 {
    super::get_hidden_value(stream, super::hidden_max_listeners_key()).unwrap_or(10.0)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_on(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    super::add_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    js_node_stream_method_on(stream_handle, event, cb)
}

fn listener_key_for_event(event: f64) -> Option<*mut crate::string::StringHeader> {
    if super::string_value_eq(event, b"data") {
        Some(super::hidden_data_listeners_key())
    } else if super::string_value_eq(event, b"end") {
        Some(super::hidden_end_listeners_key())
    } else {
        None
    }
}

pub(super) extern "C" fn ns_listener_count(closure: *const ClosureHeader, event: f64) -> f64 {
    let stream = super::this_value(closure);
    listener_count_for_event(stream, event)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listener_count(stream_handle: i64, event: f64) -> f64 {
    listener_count_for_event(super::stream_value_from_handle(stream_handle), event)
}

fn listener_count_for_event(stream: f64, event: f64) -> f64 {
    listener_key_for_event(event)
        .map(|key| super::listener_snapshot(stream, key).len() as f64)
        .unwrap_or(0.0)
}

pub(super) extern "C" fn ns_event_names(closure: *const ClosureHeader) -> f64 {
    let stream = super::this_value(closure);
    f64::from_bits(JSValue::pointer(event_names_array(stream) as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_event_names(stream_handle: i64) -> i64 {
    event_names_array(super::stream_value_from_handle(stream_handle)) as i64
}

fn event_names_array(stream: f64) -> *mut crate::array::ArrayHeader {
    let mut arr = crate::array::js_array_alloc(0);
    if !super::listener_snapshot(stream, super::hidden_data_listeners_key()).is_empty() {
        arr = crate::array::js_array_push_f64(arr, super::string_value(b"data"));
    }
    if !super::listener_snapshot(stream, super::hidden_end_listeners_key()).is_empty() {
        arr = crate::array::js_array_push_f64(arr, super::string_value(b"end"));
    }
    arr
}

pub(super) extern "C" fn ns_listeners(closure: *const ClosureHeader, event: f64) -> f64 {
    let stream = super::this_value(closure);
    f64::from_bits(JSValue::pointer(listeners_array_for_event(stream, event) as *const u8).bits())
}

pub(super) extern "C" fn ns_raw_listeners(closure: *const ClosureHeader, event: f64) -> f64 {
    ns_listeners(closure, event)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listeners(stream_handle: i64, event: f64) -> i64 {
    listeners_array_for_event(super::stream_value_from_handle(stream_handle), event) as i64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_raw_listeners(stream_handle: i64, event: f64) -> i64 {
    js_node_stream_method_listeners(stream_handle, event)
}

fn listeners_array_for_event(stream: f64, event: f64) -> *mut crate::array::ArrayHeader {
    let mut arr = crate::array::js_array_alloc(0);
    if let Some(key) = listener_key_for_event(event) {
        for listener in super::listener_snapshot(stream, key) {
            arr = crate::array::js_array_push_f64(arr, listener);
        }
    }
    arr
}
