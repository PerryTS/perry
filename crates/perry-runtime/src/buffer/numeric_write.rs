//! `Buffer.prototype.write{U}Int*`, `writeFloat*`, `writeDouble*` and
//! `writeBig{U}Int64*`, run in the order Node's `lib/internal/buffer.js` runs
//! them: `+value`, the value range check, then the offset check. The 8-bit
//! writers check the offset type before the range. The BigInt writers take
//! ToNumeric and reject a Number only after the offset check, where Node's
//! `value & 0xffffffffn` mixes types.

use super::*;
use crate::validators::received::bigint_decimal;
use crate::validators::{
    add_numerical_separator, format_range_received, throw_invalid_arg_type, throw_out_of_range,
};
use crate::value::JSValue;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Uint,
    Int,
    Float,
    Double,
    BigUint,
    BigInt,
}

/// `(kind, byte count, big endian)`; a `None` byte count is the
/// `writeIntLE(value, offset, byteLength)` family.
fn method_shape(name: &str) -> Option<(Kind, Option<usize>, bool)> {
    use Kind::*;
    Some(match name {
        "writeUInt8" | "writeUint8" => (Uint, Some(1), false),
        "writeInt8" => (Int, Some(1), false),
        "writeUInt16BE" | "writeUint16BE" => (Uint, Some(2), true),
        "writeUInt16LE" | "writeUint16LE" => (Uint, Some(2), false),
        "writeInt16BE" => (Int, Some(2), true),
        "writeInt16LE" => (Int, Some(2), false),
        "writeUInt32BE" | "writeUint32BE" => (Uint, Some(4), true),
        "writeUInt32LE" | "writeUint32LE" => (Uint, Some(4), false),
        "writeInt32BE" => (Int, Some(4), true),
        "writeInt32LE" => (Int, Some(4), false),
        "writeFloatBE" => (Float, Some(4), true),
        "writeFloatLE" => (Float, Some(4), false),
        "writeDoubleBE" => (Double, Some(8), true),
        "writeDoubleLE" => (Double, Some(8), false),
        "writeBigInt64BE" => (BigInt, Some(8), true),
        "writeBigInt64LE" => (BigInt, Some(8), false),
        "writeBigUInt64BE" | "writeBigUint64BE" => (BigUint, Some(8), true),
        "writeBigUInt64LE" | "writeBigUint64LE" => (BigUint, Some(8), false),
        "writeUIntBE" | "writeUintBE" => (Uint, None, true),
        "writeUIntLE" | "writeUintLE" => (Uint, None, false),
        "writeIntBE" => (Int, None, true),
        "writeIntLE" => (Int, None, false),
        _ => return None,
    })
}

/// Runs the numeric `write*` method `name` on `receiver` and returns the
/// offset past the written bytes; `None` when `name` is not one.
pub(crate) fn dispatch_numeric_write(receiver: f64, name: &str, args: &[f64]) -> Option<f64> {
    let (kind, fixed, big_endian) = method_shape(name)?;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg = |i: usize| args.get(i).copied().unwrap_or(undefined);
    let n = fixed.unwrap_or_else(|| variable_byte_length(arg(2)));
    // The fixed-width writers default `offset = 0`; the variable ones do not.
    let offset = match arg(1) {
        o if fixed.is_some() && o.to_bits() == crate::value::TAG_UNDEFINED => 0.0,
        o => o,
    };
    // `+value` can run a `valueOf` that collects.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let offset = scope.root_nanbox_f64(offset);
    let value = match kind {
        Kind::BigInt | Kind::BigUint => unsafe { crate::value::js_to_numeric(arg(0)) },
        _ => crate::builtins::js_number_coerce(arg(0)),
    };
    let value = scope.root_nanbox_f64(value);
    let value = || value.get_nanbox_f64();
    let checked_offset = || check_offset(receiver.get_nanbox_f64(), offset.get_nanbox_f64(), n);
    let (offset, bits) = match kind {
        Kind::Uint | Kind::Int => {
            if n == 1 {
                validate_offset_type(offset.get_nanbox_f64());
            }
            check_int_range(value(), n, kind == Kind::Int);
            (checked_offset(), int_bits(value(), n))
        }
        Kind::Float => (checked_offset(), (value() as f32).to_bits() as u64),
        Kind::Double => (checked_offset(), value().to_bits()),
        Kind::BigInt | Kind::BigUint => {
            check_bigint_range(value(), kind == Kind::BigInt);
            let offset = checked_offset();
            if !JSValue::from_bits(value().to_bits()).is_bigint() {
                crate::byte_cell_support::throw_bigint_number_mix();
            }
            (offset, numeric::bigint_value_to_i64(value()) as u64)
        }
    };
    let le = bits.to_le_bytes();
    let buf = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *mut BufferHeader;
    numeric::buffer_write_at(buf, offset as i32, n, true, |s| {
        for (i, byte) in s.iter_mut().enumerate() {
            *byte = le[if big_endian { n - 1 - i } else { i }];
        }
    });
    Some(offset + n as f64)
}

/// Node's `boundsError(byteLength, 6, 'byteLength')` for `writeIntLE` and
/// its siblings.
fn variable_byte_length(byte_length: f64) -> usize {
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

fn validate_offset_type(offset: f64) -> f64 {
    let jv = JSValue::from_bits(offset.to_bits());
    if !jv.is_number() {
        throw_invalid_arg_type("offset", "of type number", offset);
    }
    jv.as_number()
}

/// Node's `checkBounds`: `offset` is a number and `n` bytes fit at it.
fn check_offset(receiver: f64, offset: f64, n: usize) -> f64 {
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

/// Node's `checkInt` range for an `n`-byte integer. `NaN` passes and is
/// written as 0.
fn check_int_range(value: f64, n: usize, signed: bool) {
    let bits = 8 * n as i32;
    let (min, max) = if signed {
        (-(2f64.powi(bits - 1)), 2f64.powi(bits - 1) - 1.0)
    } else {
        (0.0, 2f64.powi(bits) - 1.0)
    };
    if !(value < min || value > max) {
        return;
    }
    let range = match (n > 4, signed) {
        (true, true) => format!(">= -(2 ** {}) and < 2 ** {}", bits - 1, bits - 1),
        (true, false) => format!(">= 0 and < 2 ** {bits}"),
        (false, _) => format!(">= {min} and <= {max}"),
    };
    throw_out_of_range("value", &range, &format_range_received(value));
}

/// The bytes Node stores for an in-range integer `value`: the low word is
/// `ToUint32(value)` and a 5- or 6-byte write takes its high word as
/// `Math.floor(value * 2 ** -32)`, so a negative fraction differs from
/// truncation.
fn int_bits(value: f64, n: usize) -> u64 {
    if value.is_nan() {
        return 0;
    }
    let low = value.trunc() as i64 as u32 as u64;
    if n <= 4 {
        low
    } else {
        ((value / 4_294_967_296.0).floor() as i64 as u64) << 32 | low
    }
}

/// `checkInt` against the 64-bit BigInt bounds for a ToNumeric `value`; a
/// Number compares by value and fails later as a BigInt mix.
// shortcut: a string compares here as its ToNumber, where Node compares it
// as a StringToBigInt and prints it quoted; upgrade if `"1e30"` matters.
fn check_bigint_range(value: f64, signed: bool) {
    let (min, end) = if signed {
        (-(2f64.powi(63)), 2f64.powi(63))
    } else {
        (0.0, 2f64.powi(64))
    };
    let jv = JSValue::from_bits(value.to_bits());
    let received = if jv.is_bigint() {
        let ptr = jv.as_bigint_ptr();
        let cmp = |bound: f64| crate::bigint::bigint_cmp_f64(ptr, bound);
        if cmp(min) >= 0 && cmp(end) < 0 {
            return;
        }
        let digits = bigint_decimal(value);
        let large = cmp(-4_294_967_296.0) < 0 || cmp(4_294_967_296.0) > 0;
        let digits = if large {
            add_numerical_separator(&digits)
        } else {
            digits
        };
        format!("{digits}n")
    } else if jv.is_number() && (jv.as_number() < min || jv.as_number() >= end) {
        format_range_received(jv.as_number())
    } else {
        return;
    };
    let range = if signed {
        ">= -(2n ** 63n) and < 2n ** 63n"
    } else {
        ">= 0n and < 2n ** 64n"
    };
    throw_out_of_range("value", range, &received);
}

#[cfg(test)]
mod tests {
    use super::method_shape;

    /// The dispatcher exposes a name as a Buffer method only from its own
    /// list, so a numeric writer missing from either list is unreachable or
    /// falls through to `undefined`.
    #[test]
    fn method_shapes_match_the_buffer_method_names() {
        let kinds = [
            "UInt", "Uint", "Int", "Float", "Double", "BigInt", "BigUInt", "BigUint",
        ];
        let widths = ["", "8", "16", "32", "64"];
        let endians = ["", "BE", "LE"];
        let mut writers = 0;
        for kind in kinds {
            for width in widths {
                for endian in endians {
                    let name = format!("write{kind}{width}{endian}");
                    let listed = crate::object::is_buffer_method_name(&name);
                    assert_eq!(method_shape(&name).is_some(), listed, "{name}");
                    writers += usize::from(listed);
                }
            }
        }
        assert_eq!(writers, 31);
    }
}
