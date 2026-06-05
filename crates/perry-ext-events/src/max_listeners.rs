use super::*;

fn format_max_listeners_received(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn max_listeners_is_number(value: f64) -> bool {
    let bits = value.to_bits();
    let high = bits & TAG_MASK;
    if high == INT32_TAG {
        return true;
    }
    !matches!(
        bits,
        TAG_UNDEFINED_F64_BITS | TAG_NULL_F64_BITS | 0x7FFC_0000_0000_0003 | TAG_TRUE_F64_BITS
    ) && !matches!(high, BIGINT_TAG | POINTER_TAG | STRING_TAG | INT32_TAG)
}

fn max_listeners_number(value: f64) -> f64 {
    let bits = value.to_bits();
    if (bits & TAG_MASK) == INT32_TAG {
        (bits as u32 as i32) as f64
    } else {
        value
    }
}

fn throw_max_listeners_invalid_type(value: f64) -> ! {
    let message = format!(
        "The \"setMaxListeners\" argument must be of type number. Received {}",
        describe_received(value)
    );
    throw_with_code(&message, "ERR_INVALID_ARG_TYPE", ErrorKind::TypeError)
}

fn throw_max_listeners_out_of_range(n: f64) -> ! {
    let message = format!(
        "The value of \"setMaxListeners\" is out of range. It must be >= 0. Received {}",
        format_max_listeners_received(n)
    );
    throw_with_code(&message, "ERR_OUT_OF_RANGE", ErrorKind::RangeError)
}

pub(super) fn validate_max_listeners(value: f64) -> f64 {
    if !max_listeners_is_number(value) {
        throw_max_listeners_invalid_type(value);
    }
    let n = max_listeners_number(value);
    if n.is_nan() || n < 0.0 {
        throw_max_listeners_out_of_range(n);
    }
    n
}

unsafe fn string_value(text: &str) -> f64 {
    let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    f64::from_bits(nanbox_string_bits(ptr))
}

unsafe fn set_object_string_prop(obj: *mut ObjectHeader, name: &str, value: &str) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, string_value(value));
}

unsafe fn set_object_number_prop(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

unsafe fn set_object_value_prop(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

unsafe fn get_object_field_by_str(obj: *const ObjectHeader, name: &str) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_get_field_by_name_f64(obj, key)
}

unsafe fn current_process_value() -> Option<f64> {
    let global = JsValue::from_bits(js_get_global_this().to_bits());
    if !global.is_pointer() {
        return None;
    }
    let process = get_object_field_by_str(global.as_pointer::<ObjectHeader>(), "process");
    if JsValue::from_bits(process.to_bits()).is_pointer() {
        Some(process)
    } else {
        None
    }
}

unsafe fn call_process_emit_warning(warning: f64) {
    let Some(process) = current_process_value() else {
        return;
    };
    let process_value = JsValue::from_bits(process.to_bits());
    let process_obj = process_value.as_pointer::<ObjectHeader>();
    let emit_warning = get_object_field_by_str(process_obj, "emitWarning");
    let previous_this = js_implicit_this_set(process);
    let args = [warning];
    js_native_call_value(emit_warning, args.as_ptr(), args.len());
    js_implicit_this_set(previous_this);
}

pub(super) unsafe fn emit_max_listeners_warning(
    handle: Handle,
    event_name: &str,
    count: usize,
    max: f64,
) {
    let message = format!(
        "Possible EventEmitter memory leak detected. {count} {event_name} listeners added to [EventEmitter]. MaxListeners is {}. Use emitter.setMaxListeners() to increase limit",
        format_max_listeners_received(max)
    );
    let message_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let warning = js_error_new_with_message(message_ptr);
    set_object_string_prop(warning, "name", "MaxListenersExceededWarning");
    set_object_string_prop(warning, "type", event_name);
    set_object_number_prop(warning, "count", count as f64);
    set_object_value_prop(warning, "emitter", nanbox_pointer_bits(handle));
    call_process_emit_warning(nanbox_pointer_bits(warning as i64));
}
