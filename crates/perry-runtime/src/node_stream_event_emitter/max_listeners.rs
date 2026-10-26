use super::*;

pub(super) fn set_stream_max_listeners(stream: f64, value: f64) -> f64 {
    let value = validate_max_listeners(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    state_set(
        stream.get_nanbox_f64(),
        crate::runtime_state_key!(MAX_LISTENERS_KEY),
        value,
    );
    stream.get_nanbox_f64()
}

pub(super) fn format_max_listeners_received(n: f64) -> String {
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

pub(super) fn throw_max_listeners_invalid_type(value: f64) -> ! {
    let message = format!(
        "The \"setMaxListeners\" argument must be of type number. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(super) fn throw_max_listeners_out_of_range(n: f64) -> ! {
    let message = format!(
        "The value of \"setMaxListeners\" is out of range. It must be >= 0. Received {}",
        format_max_listeners_received(n)
    );
    crate::fs::validate::throw_range_error_with_code(&message)
}

pub(crate) fn validate_max_listeners(value: f64) -> f64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if !crate::fs::validate::is_numeric(js_value) {
        throw_max_listeners_invalid_type(value);
    }
    let n = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else {
        js_value.as_number()
    };
    if n.is_nan() || n < 0.0 {
        throw_max_listeners_out_of_range(n);
    }
    n
}

pub(super) fn stream_max_listeners(stream: f64) -> f64 {
    // node: `_maxListeners === undefined ? defaultMaxListeners : _maxListeners`.
    let max = state_get(stream, crate::runtime_state_key!(MAX_LISTENERS_KEY));
    if is_undefined(max) {
        default_max_listeners()
    } else {
        max
    }
}
