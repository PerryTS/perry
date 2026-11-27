//! `Buffer.prototype.write{U}Int*`, `writeFloat*`, `writeDouble*` and
//! `writeBig{U}Int64*`, run in the order Node's `lib/internal/buffer.js` runs
//! them: `+value`, the value range check, then the offset check. The 8-bit
//! writers check the offset type before the range. The BigInt writers do not
//! coerce: they compare the value with their bounds and reject a non-BigInt
//! only after the offset check, where Node's `value & 0xffffffffn` mixes
//! types.

use super::numeric_offset::{check_offset, validate_offset_type, variable_byte_length};
use super::*;
use crate::validators::received::bigint_decimal;
use crate::validators::{add_numerical_separator, format_range_received, throw_out_of_range};
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
    // `+value` and ToPrimitive can run a `valueOf` that collects.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let offset = scope.root_nanbox_f64(offset);
    let original = scope.root_nanbox_f64(arg(0));
    let checked_offset = || check_offset(receiver.get_nanbox_f64(), offset.get_nanbox_f64(), n);
    let (offset, bits) = if let Kind::BigInt | Kind::BigUint = kind {
        let primitive = scope.root_nanbox_f64(to_primitive_number(original.get_nanbox_f64()));
        check_bigint_range(
            original.get_nanbox_f64(),
            primitive.get_nanbox_f64(),
            kind == Kind::BigInt,
        );
        let offset = checked_offset();
        // Node's `value & 0xffffffffn` runs ToNumeric again; the primitive
        // stands in for it, so a `valueOf` runs once here, not three times.
        let value = primitive.get_nanbox_f64();
        if !JSValue::from_bits(value.to_bits()).is_bigint() {
            crate::byte_cell_support::throw_bigint_number_mix();
        }
        (offset, numeric::bigint_value_to_i64(value) as u64)
    } else {
        let value = crate::builtins::js_number_coerce(original.get_nanbox_f64());
        match kind {
            Kind::Float => (checked_offset(), (value as f32).to_bits() as u64),
            Kind::Double => (checked_offset(), value.to_bits()),
            _ => {
                if n == 1 {
                    validate_offset_type(offset.get_nanbox_f64());
                }
                check_int_range(value, n, kind == Kind::Int);
                (checked_offset(), int_bits(value, n))
            }
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

/// `ToPrimitive(value, number)`, which Node's `value > max` comparisons run.
fn to_primitive_number(value: f64) -> f64 {
    use crate::value::OrdinaryToPrimitiveOutcome::*;
    match unsafe { crate::value::to_primitive_number(value) } {
        Primitive(primitive) => primitive,
        DefaultString => {
            crate::value::js_nanbox_string(crate::value::js_jsvalue_to_string(value) as i64)
        }
        TypeError => {
            crate::byte_cell_support::throw_type_error(b"Cannot convert object to primitive value")
        }
    }
}

/// `checkInt` against the 64-bit BigInt bounds. `primitive` compares as
/// Node's relational operators compare it with a BigInt: a Number by value, a
/// string through StringToBigInt (no match when it does not parse).
fn check_bigint_range(original: f64, primitive: f64, signed: bool) {
    let (min, end) = if signed {
        (-(2f64.powi(63)), 2f64.powi(63))
    } else {
        (0.0, 2f64.powi(64))
    };
    let jv = JSValue::from_bits(primitive.to_bits());
    let bigint = if jv.is_bigint() {
        Some(jv.as_bigint_ptr())
    } else if jv.is_any_string() {
        let text = crate::validators::received::read_js_string_pub(primitive);
        crate::bigint::string_to_bigint(&text).map(|ptr| ptr as *const _)
    } else if unsafe { crate::symbol::js_is_symbol(primitive) } != 0 {
        crate::byte_cell_support::throw_type_error(b"Cannot convert a Symbol value to a number")
    } else {
        None
    };
    let out_of_range = match bigint {
        Some(ptr) => {
            let cmp = |bound: f64| crate::bigint::bigint_cmp_f64(ptr, bound);
            cmp(min) < 0 || cmp(end) >= 0
        }
        None => jv.is_number() && (jv.as_number() < min || jv.as_number() >= end),
    };
    if !out_of_range {
        return;
    }
    let range = if signed {
        ">= -(2n ** 63n) and < 2n ** 63n"
    } else {
        ">= 0n and < 2n ** 64n"
    };
    throw_out_of_range("value", range, &received_value(original));
}

/// Node's `ERR_OUT_OF_RANGE` rendering of a received value that is not
/// coerced first: a Number or BigInt grouped past 2 ** 32, anything else
/// inspected.
fn received_value(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_number() {
        return format_range_received(jv.as_number());
    }
    if jv.is_bigint() {
        let digits = bigint_decimal(value);
        let ptr = jv.as_bigint_ptr();
        let large = crate::bigint::bigint_cmp_f64(ptr, -4_294_967_296.0) < 0
            || crate::bigint::bigint_cmp_f64(ptr, 4_294_967_296.0) > 0;
        let digits = if large {
            add_numerical_separator(&digits)
        } else {
            digits
        };
        return format!("{digits}n");
    }
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    crate::validators::received::read_js_string_pub(crate::builtins::js_util_inspect(
        value, undefined,
    ))
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
