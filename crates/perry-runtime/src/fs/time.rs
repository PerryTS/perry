use super::validate;

#[no_mangle]
pub extern "C" fn js_fs_to_unix_timestamp(time_value: f64) -> f64 {
    let js_value = crate::value::JSValue::from_bits(time_value.to_bits());

    if js_value.is_any_string() {
        let number = crate::builtins::js_number_coerce(time_value);
        if !number.is_nan() {
            return number;
        }
    }

    if validate::is_numeric(js_value) {
        let number = if js_value.is_int32() {
            js_value.as_int32() as f64
        } else {
            js_value.as_number()
        };
        if number.is_finite() {
            if number < 0.0 {
                return crate::date::js_date_now() / 1000.0;
            }
            return number;
        }
    }

    if crate::date::is_date_value(time_value) {
        return crate::date::date_cell_timestamp(time_value) / 1000.0;
    }

    let message = format!(
        "The \"time\" argument must be an instance of Date or an Time in seconds. Received {}",
        validate::describe_received(time_value)
    );
    validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}
