use perry_runtime::{js_nanbox_pointer, js_string_from_bytes, ObjectHeader};

use super::event_names::{js_string_value, set_object_prop};

pub(super) fn emit_max_listeners_warning(
    emitter_value: f64,
    event_value: f64,
    event_display: &str,
    count: usize,
    max: f64,
) {
    let max_display = super::format_max_listeners_received(max);
    let message = format!(
        "Possible EventEmitter memory leak detected. {count} {event_display} listeners added to [EventEmitter]. MaxListeners is {max_display}. Use emitter.setMaxListeners() to increase limit"
    );
    let message_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let warning = perry_runtime::error::js_error_new_with_message(message_ptr);
    let warning_value = js_nanbox_pointer(warning as i64);
    set_object_prop(
        warning as *mut ObjectHeader,
        "name",
        js_string_value("MaxListenersExceededWarning"),
    );
    set_object_prop(warning as *mut ObjectHeader, "emitter", emitter_value);
    set_object_prop(warning as *mut ObjectHeader, "type", event_value);
    set_object_prop(warning as *mut ObjectHeader, "count", count as f64);
    perry_runtime::process::emit_warning_via_process_emit_warning(warning_value);
}
