//! JavaScript number semantics shared by enum folding and property keys.

/// ECMAScript Number::toString, including -0, NaN, and infinities.
pub fn js_number_to_string(value: f64) -> String {
    ryu_js::Buffer::new().format(value).to_string()
}

/// ECMAScript ToInt32: truncate, then reduce modulo 2^32.
pub(crate) fn js_to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}
