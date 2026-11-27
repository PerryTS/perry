//! Node's offset and `byteLength` checks for the numeric `read*`/`write*`
//! methods (`validateNumber`, `checkBounds` and `boundsError` in
//! `lib/internal/buffer.js`).

use super::*;
use crate::validators::{format_range_received, throw_invalid_arg_type, throw_out_of_range};
use crate::value::JSValue;

/// Node's `boundsError(byteLength, 6, 'byteLength')` for the variable-width
/// `readIntLE`/`writeIntLE` family.
pub(super) fn variable_byte_length(byte_length: f64) -> usize {
    let jv = JSValue::from_bits(byte_length.to_bits());
    if !jv.is_number() {
        throw_invalid_arg_type("byteLength", "of type number", byte_length);
    }
    let n = jv.as_number();
    if n.floor() != n {
        throw_out_of_range("byteLength", "an integer", &format_range_received(n));
    }
    if !(1.0..=6.0).contains(&n) {
        throw_out_of_range("byteLength", ">= 1 and <= 6", &format_range_received(n));
    }
    n as usize
}

pub(super) fn validate_offset_type(offset: f64) -> f64 {
    let jv = JSValue::from_bits(offset.to_bits());
    if !jv.is_number() {
        throw_invalid_arg_type("offset", "of type number", offset);
    }
    jv.as_number()
}

/// Node's `checkBounds`: `offset` is a number and `n` bytes fit at it.
pub(super) fn check_offset(receiver: f64, offset: f64, n: usize) -> f64 {
    let offset = validate_offset_type(offset);
    let buf = crate::value::js_nanbox_get_pointer(receiver) as *const BufferHeader;
    let last = js_buffer_length(buf) as f64 - n as f64;
    if offset >= 0.0 && offset <= last && offset.floor() == offset {
        return offset;
    }
    if offset.floor() != offset {
        throw_out_of_range("offset", "an integer", &format_range_received(offset));
    }
    if last < 0.0 {
        numeric::throw_buffer_out_of_bounds();
    }
    throw_out_of_range(
        "offset",
        &format!(">= 0 and <= {last}"),
        &format_range_received(offset),
    )
}
