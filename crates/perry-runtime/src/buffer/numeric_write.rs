//! `Buffer.prototype.write{U}Int*`, `writeFloat*`, `writeDouble*` and
//! `writeBig{U}Int64*`, run in the order Node's `lib/internal/buffer.js` runs
//! them: `+value`, the value range check, then the offset check. The 8-bit
//! writers check the offset type before the range. The BigInt writers do not
//! coerce once: like Node they convert the value for each comparison with
//! their bounds and again for each word they store, and reject a non-BigInt
//! only there, after the offset check.

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
    if let Kind::BigInt | Kind::BigUint = kind {
        check_bigint_range(&original, kind == Kind::BigInt);
        let offset = checked_offset();
        // `value & 0xffffffffn` converts the value and its low word is stored
        // before `value >> 32n` converts it again.
        let (low_at, high_at) = if big_endian { (4.0, 0.0) } else { (0.0, 4.0) };
        let low = bigint_bits(original.get_nanbox_f64());
        store(
            receiver.get_nanbox_f64(),
            offset + low_at,
            4,
            big_endian,
            low,
        );
        let high = bigint_bits(original.get_nanbox_f64()) >> 32;
        store(
            receiver.get_nanbox_f64(),
            offset + high_at,
            4,
            big_endian,
            high,
        );
        return Some(offset + 8.0);
    }
    let value = crate::builtins::js_number_coerce(original.get_nanbox_f64());
    let (offset, bits) = match kind {
        Kind::Float => (checked_offset(), (value as f32).to_bits() as u64),
        Kind::Double => (checked_offset(), value.to_bits()),
        _ => {
            if n == 1 {
                validate_offset_type(offset.get_nanbox_f64());
            }
            check_int_range(value, n, kind == Kind::Int);
            (checked_offset(), int_bits(value, n))
        }
    };
    store(receiver.get_nanbox_f64(), offset, n, big_endian, bits);
    Some(offset + n as f64)
}

/// Stores the low `n` bytes of `bits` at `offset`, already checked to fit.
fn store(receiver: f64, offset: f64, n: usize, big_endian: bool, bits: u64) {
    let le = bits.to_le_bytes();
    let buf = crate::value::js_nanbox_get_pointer(receiver) as *mut BufferHeader;
    numeric::buffer_write_at(buf, offset as i32, n, true, |s| {
        for (i, byte) in s.iter_mut().enumerate() {
            *byte = le[if big_endian { n - 1 - i } else { i }];
        }
    });
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

/// `ToPrimitive(value, number)`, which each of Node's relational and bitwise
/// BigInt operations runs on the value.
fn to_primitive_number(value: f64) -> f64 {
    use crate::value::OrdinaryToPrimitiveOutcome::*;
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    match unsafe { crate::value::to_primitive_number(value.get_nanbox_f64()) } {
        Primitive(primitive) => primitive,
        DefaultString => crate::value::js_nanbox_string(crate::value::js_jsvalue_to_string(
            value.get_nanbox_f64(),
        ) as i64),
        TypeError => {
            crate::byte_cell_support::throw_type_error(b"Cannot convert object to primitive value")
        }
    }
}

/// The two's-complement low 64 bits of ToNumeric(`value`), which must be a
/// BigInt.
fn bigint_bits(value: f64) -> u64 {
    let numeric = to_primitive_number(value);
    if !JSValue::from_bits(numeric.to_bits()).is_bigint() {
        crate::byte_cell_support::throw_bigint_number_mix();
    }
    numeric::bigint_value_to_i64(numeric) as u64
}

/// How a primitive compares with `bound` under Node's relational operators
/// against a BigInt: a BigInt or Number by value, a string through
/// StringToBigInt (`None` when it does not parse), anything else as its
/// ToNumber.
fn compare_with_bound(primitive: f64, bound: f64) -> Option<std::cmp::Ordering> {
    let jv = JSValue::from_bits(primitive.to_bits());
    let bigint = if jv.is_bigint() {
        jv.as_bigint_ptr()
    } else if jv.is_any_string() {
        let text = crate::validators::received::read_js_string_pub(primitive);
        crate::bigint::string_to_bigint(&text)? as *const _
    } else if unsafe { crate::symbol::js_is_symbol(primitive) } != 0 {
        crate::byte_cell_support::throw_type_error(b"Cannot convert a Symbol value to a number")
    } else {
        return crate::builtins::js_number_coerce(primitive).partial_cmp(&bound);
    };
    Some(crate::bigint::bigint_cmp_f64(bigint, bound).cmp(&0))
}

/// `checkInt` against the 64-bit BigInt bounds: `value > max`, then
/// `value < min`, each converting the value on its own.
fn check_bigint_range(original: &crate::gc::RuntimeHandle<'_>, signed: bool) {
    use std::cmp::Ordering::Less;
    let (min, end) = if signed {
        (-(2f64.powi(63)), 2f64.powi(63))
    } else {
        (0.0, 2f64.powi(64))
    };
    let value = || to_primitive_number(original.get_nanbox_f64());
    let above = compare_with_bound(value(), end).is_some_and(|order| order != Less);
    if !above && compare_with_bound(value(), min) != Some(Less) {
        return;
    }
    let range = if signed {
        ">= -(2n ** 63n) and < 2n ** 63n"
    } else {
        ">= 0n and < 2n ** 64n"
    };
    throw_out_of_range("value", range, &received_value(original.get_nanbox_f64()));
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
        // Compared before `bigint_decimal` allocates and can move the BigInt.
        let ptr = jv.as_bigint_ptr();
        let large = crate::bigint::bigint_cmp_f64(ptr, -4_294_967_296.0) < 0
            || crate::bigint::bigint_cmp_f64(ptr, 4_294_967_296.0) > 0;
        let digits = bigint_decimal(value);
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
