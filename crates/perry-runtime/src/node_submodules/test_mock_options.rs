use super::*;

pub(super) fn mock_option_times(options: f64) -> Option<u64> {
    let Some(value) = object_property(options, b"times") else {
        return None;
    };
    if is_undefined_value(value) {
        return None;
    }
    if crate::fs::validate::is_numeric(JSValue::from_bits(value.to_bits())) {
        let number = crate::validators::number_value(value);
        if number == f64::INFINITY {
            return None;
        }
    }
    Some(crate::validators::validate_integer(
        value,
        "options.times",
        1.0,
        crate::validators::MAX_SAFE_INTEGER,
    ) as u64)
}

pub(super) fn parse_mock_fn_options(options: f64) -> Option<u64> {
    if is_undefined_value(options) {
        return None;
    }
    crate::validators::validate_object(options, "options");
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    mock_option_times(options.get_nanbox_f64())
}
