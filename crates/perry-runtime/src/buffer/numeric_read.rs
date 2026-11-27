//! `Buffer.prototype.read{U}Int*`, `readFloat*`, `readDouble*` and
//! `readBig{U}Int64*` with Node's offset checks (`lib/internal/buffer.js`):
//! the offset is never coerced, it must be an integer, and the bytes must
//! fit.

use super::numeric_offset::{check_offset, variable_byte_length};
use super::*;

type FixedRead = extern "C" fn(f64, i32) -> f64;
type VariableRead = extern "C" fn(f64, i32, i32) -> f64;

enum Reader {
    Fixed(usize, FixedRead),
    /// The `readIntLE(offset, byteLength)` family.
    Variable(VariableRead),
}

fn reader(name: &str) -> Option<Reader> {
    use Reader::*;
    Some(match name {
        "readUInt8" | "readUint8" => Fixed(1, js_buffer_read_uint8),
        "readInt8" => Fixed(1, js_buffer_read_int8),
        "readUInt16BE" | "readUint16BE" => Fixed(2, js_buffer_read_uint16_be),
        "readUInt16LE" | "readUint16LE" => Fixed(2, js_buffer_read_uint16_le),
        "readInt16BE" => Fixed(2, js_buffer_read_int16_be),
        "readInt16LE" => Fixed(2, js_buffer_read_int16_le),
        "readUInt32BE" | "readUint32BE" => Fixed(4, js_buffer_read_uint32_be),
        "readUInt32LE" | "readUint32LE" => Fixed(4, js_buffer_read_uint32_le),
        "readInt32BE" => Fixed(4, js_buffer_read_int32_be),
        "readInt32LE" => Fixed(4, js_buffer_read_int32_le),
        "readFloatBE" => Fixed(4, js_buffer_read_float_be),
        "readFloatLE" => Fixed(4, js_buffer_read_float_le),
        "readDoubleBE" => Fixed(8, js_buffer_read_double_be),
        "readDoubleLE" => Fixed(8, js_buffer_read_double_le),
        "readBigInt64BE" => Fixed(8, js_buffer_read_bigint64_be),
        "readBigInt64LE" => Fixed(8, js_buffer_read_bigint64_le),
        "readBigUInt64BE" | "readBigUint64BE" => Fixed(8, js_buffer_read_biguint64_be),
        "readBigUInt64LE" | "readBigUint64LE" => Fixed(8, js_buffer_read_biguint64_le),
        "readUIntBE" | "readUintBE" => Variable(js_buffer_read_uint_be),
        "readUIntLE" | "readUintLE" => Variable(js_buffer_read_uint_le),
        "readIntBE" => Variable(js_buffer_read_int_be),
        "readIntLE" => Variable(js_buffer_read_int_le),
        _ => return None,
    })
}

/// Runs the numeric `read*` method `name` on `receiver`; `None` when `name`
/// is not one.
pub(crate) fn dispatch_numeric_read(receiver: f64, name: &str, args: &[f64]) -> Option<f64> {
    let reader = reader(name)?;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let offset = args.first().copied().unwrap_or(undefined);
    Some(match reader {
        // The fixed-width readers default `offset = 0`.
        Reader::Fixed(n, read) => {
            let offset = if offset.to_bits() == crate::value::TAG_UNDEFINED {
                0.0
            } else {
                offset
            };
            read(receiver, check_offset(receiver, offset, n) as i32)
        }
        Reader::Variable(read) => {
            if offset.to_bits() == crate::value::TAG_UNDEFINED {
                crate::validators::throw_invalid_arg_type("offset", "of type number", offset);
            }
            let n = variable_byte_length(args.get(1).copied().unwrap_or(undefined));
            read(receiver, check_offset(receiver, offset, n) as i32, n as i32)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::reader;

    /// The dispatcher exposes a name as a Buffer method only from its own
    /// list, so a numeric reader missing from either list is unreachable or
    /// falls through to `undefined`.
    #[test]
    fn readers_match_the_buffer_method_names() {
        let kinds = [
            "UInt", "Uint", "Int", "Float", "Double", "BigInt", "BigUInt", "BigUint",
        ];
        let widths = ["", "8", "16", "32", "64"];
        let endians = ["", "BE", "LE"];
        let mut readers = 0;
        for kind in kinds {
            for width in widths {
                for endian in endians {
                    let name = format!("read{kind}{width}{endian}");
                    let listed = crate::object::is_buffer_method_name(&name);
                    assert_eq!(reader(&name).is_some(), listed, "{name}");
                    readers += usize::from(listed);
                }
            }
        }
        assert_eq!(readers, 31);
    }
}
