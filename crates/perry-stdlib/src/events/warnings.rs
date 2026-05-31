use super::{closure_ptr_from_value, undefined_value, EventEmitterHandle, Handle};
use perry_runtime::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, js_object_get_field_by_name_f64,
    js_object_set_field_by_name, js_string_from_bytes, ObjectHeader,
};

fn js_string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_nanbox_string(ptr as i64)
}

unsafe fn set_field(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key_ptr, value);
}

unsafe fn make_warning(handle: Handle, event: &str, count: usize, max: f64) -> f64 {
    let max = if max.fract() == 0.0 && max.is_finite() {
        format!("{}", max as i64)
    } else {
        format!("{}", max)
    };
    let message = format!(
        "Possible EventEmitter memory leak detected. {count} {event} listeners added to [EventEmitter]. MaxListeners is {max}. Use emitter.setMaxListeners() to increase limit"
    );
    let message_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let warning = perry_runtime::error::js_error_new_with_message(message_ptr);
    let warning_obj = warning as *mut ObjectHeader;
    set_field(
        warning_obj,
        "name",
        js_string_value("MaxListenersExceededWarning"),
    );
    set_field(warning_obj, "emitter", js_nanbox_pointer(handle));
    set_field(warning_obj, "type", js_string_value(event));
    set_field(warning_obj, "count", count as f64);
    js_nanbox_pointer(warning as i64)
}

unsafe fn emit_warning(warning: f64) {
    let process = perry_runtime::object::js_create_native_module_namespace(
        b"process".as_ptr(),
        "process".len(),
    );
    let process_obj = js_nanbox_get_pointer(process) as *mut ObjectHeader;
    if !process_obj.is_null() {
        let key = b"emitWarning";
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let emit_warning = js_object_get_field_by_name_f64(process_obj, key_ptr);
        if closure_ptr_from_value(emit_warning).is_some() {
            let args = [warning];
            let previous_this = perry_runtime::object::js_implicit_this_set(process);
            perry_runtime::closure::js_native_call_value(emit_warning, args.as_ptr(), args.len());
            perry_runtime::object::js_implicit_this_set(previous_this);
            return;
        }
    }
    perry_runtime::process::js_process_emit_warning(warning, undefined_value(), undefined_value());
}

pub(super) fn maybe_emit_max_listeners_warning(
    emitter: &mut EventEmitterHandle,
    handle: Handle,
    event: &str,
) {
    let max = emitter.max_listeners;
    if max == 0.0 || !max.is_finite() {
        return;
    }
    let count = emitter.events.get(event).map(|v| v.len()).unwrap_or(0);
    if count <= max as usize || emitter.warned_events.contains(event) {
        return;
    }
    emitter.warned_events.insert(event.to_string());
    unsafe { emit_warning(make_warning(handle, event, count, max)) }
}
