//! `DataView` numeric accessor methods (#2878).
//!
//! Node's `DataView` exposes byte-level numeric getters/setters
//! (`getInt8`/`getUint16`/`getFloat64`/… and the `set*` counterparts) with an
//! explicit little-endian flag (big-endian is the default). Perry models a
//! `DataView` as a `BufferHeader` aliasing (or slicing) its backing
//! `ArrayBuffer` — see `js_data_view_new` in `from.rs`.
//!
//! These helpers differ from the `Buffer.prototype.read*`/`write*` family
//! (`numeric.rs`) in one important way: DataView setters perform the abstract
//! `ToIntN`/`ToUintN` *wrap* on the value (`setInt8(0, -1)` then
//! `getUint8(0) === 255`, `setUint16(0, 70000)` wraps to `4464`) and only
//! throw `RangeError` for an out-of-bounds byte offset. The Buffer write
//! family instead range-checks the value and throws `ERR_OUT_OF_RANGE`, so
//! DataView cannot reuse it.

use super::*;

/// Numeric element kind for a DataView accessor. Encodes signedness, width and
/// float-ness; endianness is a separate flag passed alongside.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DataViewKind {
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

impl DataViewKind {
    #[inline]
    fn width(self) -> usize {
        match self {
            DataViewKind::Int8 | DataViewKind::Uint8 => 1,
            DataViewKind::Int16 | DataViewKind::Uint16 => 2,
            DataViewKind::Int32 | DataViewKind::Uint32 | DataViewKind::Float32 => 4,
            DataViewKind::Float64 | DataViewKind::BigInt64 | DataViewKind::BigUint64 => 8,
        }
    }

    /// Is this a BigInt-valued accessor (`getBigInt64`/`setBigUint64`/…)? Those
    /// read/write a NaN-boxed BigInt rather than a Number.
    #[inline]
    fn is_bigint(self) -> bool {
        matches!(self, DataViewKind::BigInt64 | DataViewKind::BigUint64)
    }

    /// Map a `get*`/`set*` method name (without the `get`/`set` prefix) to a
    /// kind. Returns `None` for an unrecognized element name.
    pub fn from_method_suffix(suffix: &str) -> Option<DataViewKind> {
        Some(match suffix {
            "Int8" => DataViewKind::Int8,
            "Uint8" => DataViewKind::Uint8,
            "Int16" => DataViewKind::Int16,
            "Uint16" => DataViewKind::Uint16,
            "Int32" => DataViewKind::Int32,
            "Uint32" => DataViewKind::Uint32,
            "Float32" => DataViewKind::Float32,
            "Float64" => DataViewKind::Float64,
            "BigInt64" => DataViewKind::BigInt64,
            "BigUint64" => DataViewKind::BigUint64,
            _ => return None,
        })
    }
}

fn throw_dataview_oob() -> ! {
    super::numeric::throw_dataview_offset_out_of_bounds()
}

#[inline]
/// `ToIndex(byteOffset)` for `GetViewValue`/`SetViewValue`: ToNumber →
/// ToIntegerOrInfinity → range-check `[0, 2^53-1]`. A Symbol or object byteOffset
/// is coerced through the full ToNumber (`js_number_coerce` throws a TypeError on
/// a Symbol and runs an object's `valueOf`/`toString`); a BigInt throws a
/// TypeError (ToIndex uses ToNumber, which — unlike `Number()` — rejects BigInt);
/// a negative or `> 2^53-1` integer (including ±Infinity) throws a RangeError.
/// Previously this used the bare `JSValue::to_number()`, which silently produced
/// `NaN`/`0` for those cases — so a Symbol didn't throw, `valueOf` never ran, and
/// negative/Infinity offsets only surfaced (if at all) as a later bounds error.
fn to_byte_offset(value: f64) -> i64 {
    if crate::value::JSValue::from_bits(value.to_bits()).is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    let n = crate::builtins::js_number_coerce(value);
    let i = if n.is_nan() { 0.0 } else { n.trunc() };
    if i < 0.0 || i > 9_007_199_254_740_991.0 {
        throw_dataview_oob();
    }
    i as i64
}

/// `ToNumber(value)` for `SetViewValue` on a non-BigInt accessor. Uses the full
/// ToNumber (`js_number_coerce`): a Symbol throws a TypeError, an object runs its
/// `valueOf`/`toString`, strings parse. The bare `JSValue::to_number()` silently
/// produced `NaN` for those, so `setFloat32(0, Symbol())` didn't throw and a
/// throwing `valueOf` never ran. Runs after `ToIndex(byteOffset)` (SetViewValue
/// step order). A BigInt accessor takes the `to_bigint_raw_or_throw` path instead.
#[inline]
fn to_number(value: f64) -> f64 {
    crate::builtins::js_number_coerce(value)
}

/// Read `width` bytes starting at `offset` from a DataView's backing storage.
/// Throws `RangeError` (`ERR_OUT_OF_BOUNDS`) when the range escapes the view.
unsafe fn read_bytes<const N: usize>(buf: *const BufferHeader, offset: i64) -> [u8; N] {
    if buf.is_null() || offset < 0 {
        throw_dataview_oob();
    }
    let len = (*buf).length as i64;
    if offset + (N as i64) > len {
        throw_dataview_oob();
    }
    let base = buffer_data(buf).add(offset as usize);
    let mut out = [0u8; N];
    ptr::copy_nonoverlapping(base, out.as_mut_ptr(), N);
    out
}

/// Write `bytes` at `offset` into a DataView's backing storage, propagating to
/// any aliased views. Throws `RangeError` when the range escapes the view.
unsafe fn write_bytes(buf: *mut BufferHeader, offset: i64, bytes: &[u8]) {
    if buf.is_null() || offset < 0 {
        throw_dataview_oob();
    }
    let len = (*buf).length as i64;
    if offset + (bytes.len() as i64) > len {
        throw_dataview_oob();
    }
    let base = buffer_data_mut(buf).add(offset as usize);
    ptr::copy_nonoverlapping(bytes.as_ptr(), base, bytes.len());
    super::view::propagate_written_range_from_receiver(
        buf as usize,
        offset as u32,
        base,
        bytes.len() as u32,
    );
}

/// `DataView.prototype.get<Kind>(byteOffset, littleEndian?)`.
/// `buf_f64` is the NaN-boxed DataView (BufferHeader) pointer.
pub fn js_data_view_get(buf_f64: f64, offset_value: f64, kind: DataViewKind, little: bool) -> f64 {
    let buf = unbox_buffer_ptr(buf_f64.to_bits()) as *const BufferHeader;
    let offset = to_byte_offset(offset_value);
    unsafe {
        match kind {
            DataViewKind::Int8 => (read_bytes::<1>(buf, offset)[0] as i8) as f64,
            DataViewKind::Uint8 => read_bytes::<1>(buf, offset)[0] as f64,
            DataViewKind::Int16 => {
                let b = read_bytes::<2>(buf, offset);
                if little {
                    i16::from_le_bytes(b) as f64
                } else {
                    i16::from_be_bytes(b) as f64
                }
            }
            DataViewKind::Uint16 => {
                let b = read_bytes::<2>(buf, offset);
                if little {
                    u16::from_le_bytes(b) as f64
                } else {
                    u16::from_be_bytes(b) as f64
                }
            }
            DataViewKind::Int32 => {
                let b = read_bytes::<4>(buf, offset);
                if little {
                    i32::from_le_bytes(b) as f64
                } else {
                    i32::from_be_bytes(b) as f64
                }
            }
            DataViewKind::Uint32 => {
                let b = read_bytes::<4>(buf, offset);
                if little {
                    u32::from_le_bytes(b) as f64
                } else {
                    u32::from_be_bytes(b) as f64
                }
            }
            DataViewKind::Float32 => {
                let b = read_bytes::<4>(buf, offset);
                if little {
                    f32::from_le_bytes(b) as f64
                } else {
                    f32::from_be_bytes(b) as f64
                }
            }
            DataViewKind::Float64 => {
                let b = read_bytes::<8>(buf, offset);
                if little {
                    f64::from_le_bytes(b)
                } else {
                    f64::from_be_bytes(b)
                }
            }
            DataViewKind::BigInt64 => {
                let b = read_bytes::<8>(buf, offset);
                let v = if little {
                    i64::from_le_bytes(b)
                } else {
                    i64::from_be_bytes(b)
                };
                crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(v) as i64)
            }
            DataViewKind::BigUint64 => {
                let b = read_bytes::<8>(buf, offset);
                let v = if little {
                    u64::from_le_bytes(b)
                } else {
                    u64::from_be_bytes(b)
                };
                crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_u64(v) as i64)
            }
        }
    }
}

/// `DataView.prototype.set<Kind>(byteOffset, value, littleEndian?)`.
/// Performs the abstract `ToIntN`/`ToUintN` wrap on the value (no value-range
/// throw, matching Node) and returns `undefined`.
pub fn js_data_view_set(
    buf_f64: f64,
    offset_value: f64,
    value: f64,
    kind: DataViewKind,
    little: bool,
) -> f64 {
    let buf = unbox_buffer_ptr(buf_f64.to_bits()) as *mut BufferHeader;
    let offset = to_byte_offset(offset_value);
    if kind.is_bigint() {
        // SetViewValue for a BigInt accessor: `ToBigInt(value)` (a Number throws
        // `TypeError`) runs before the bounds check, then the raw 8 bytes are
        // stored with the requested endianness (both kinds share the bit layout).
        let raw = to_bigint_raw_or_throw(value);
        let b = if little {
            raw.to_le_bytes()
        } else {
            raw.to_be_bytes()
        };
        unsafe { write_bytes(buf, offset, &b) };
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let n = to_number(value);
    unsafe {
        match kind {
            DataViewKind::BigInt64 | DataViewKind::BigUint64 => unreachable!(),
            DataViewKind::Int8 | DataViewKind::Uint8 => {
                // ToUint8 / ToInt8 wrap to the same byte; store identically.
                let byte = wrap_to_u64(n, 8) as u8;
                write_bytes(buf, offset, &[byte]);
            }
            DataViewKind::Int16 | DataViewKind::Uint16 => {
                let v = wrap_to_u64(n, 16) as u16;
                let b = if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                };
                write_bytes(buf, offset, &b);
            }
            DataViewKind::Int32 | DataViewKind::Uint32 => {
                let v = wrap_to_u64(n, 32) as u32;
                let b = if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                };
                write_bytes(buf, offset, &b);
            }
            DataViewKind::Float32 => {
                let v = n as f32;
                let b = if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                };
                write_bytes(buf, offset, &b);
            }
            DataViewKind::Float64 => {
                let b = if little {
                    n.to_le_bytes()
                } else {
                    n.to_be_bytes()
                };
                write_bytes(buf, offset, &b);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// ToIntN/ToUintN: truncate toward zero then reduce modulo 2^bits. NaN and the
/// infinities map to 0 (per the abstract `ToNumber` → `ToIntegerOrInfinity`
/// step used by DataView setters).
#[inline]
fn wrap_to_u64(n: f64, bits: u32) -> u64 {
    if !n.is_finite() {
        return 0;
    }
    let truncated = n.trunc();
    // `as i128` then modulo keeps the low `bits` bits regardless of sign.
    let modulus = 1i128 << bits;
    let reduced = (truncated as i128).rem_euclid(modulus);
    reduced as u64
}

/// `ToBigInt(value)` for a `setBigInt64`/`setBigUint64` write, returning the
/// BigInt's low 64 bits (the only ones an 8-byte slot holds; both signed and
/// unsigned share the bit layout). Per the ECMAScript `ToBigInt` operation, a
/// Number (incl. a NaN-boxed int32), `undefined`, `null`, and Symbols are NOT
/// convertible and throw a `TypeError`; BigInt passes through, Boolean and
/// String coerce.
fn to_bigint_raw_or_throw(value: f64) -> u64 {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());
    let bi: *const crate::bigint::BigIntHeader = if jsval.is_bigint() {
        jsval.as_bigint_ptr() as *const crate::bigint::BigIntHeader
    } else if jsval.is_bool() {
        crate::bigint::js_bigint_from_i64(if jsval.as_bool() { 1 } else { 0 })
    } else if jsval.is_any_string() {
        // StringToBigInt (a malformed numeric string throws SyntaxError).
        crate::bigint::js_bigint_from_f64(value)
    } else {
        throw_bigint_conversion_type_error(value);
    };
    let bi = crate::bigint::clean_bigint_ptr(bi);
    if bi.is_null() {
        return 0;
    }
    unsafe { (*bi).limbs[0] }
}

/// Throw `TypeError: Cannot convert <x> to a BigInt`, matching Node's
/// `ToBigInt` rejection text for a DataView BigInt setter.
#[cold]
fn throw_bigint_conversion_type_error(value: f64) -> ! {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());
    let label = if jsval.is_undefined() {
        "undefined".to_string()
    } else if jsval.is_null() {
        "null".to_string()
    } else if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        "a Symbol value".to_string()
    } else if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else {
        format!("{value}")
    };
    let msg = format!("Cannot convert {label} to a BigInt");
    let err = crate::error::js_typeerror_new(crate::string::js_string_from_bytes(
        msg.as_ptr(),
        msg.len() as u32,
    ));
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
