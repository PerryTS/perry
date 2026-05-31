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
}

impl DataViewKind {
    #[inline]
    fn width(self) -> usize {
        match self {
            DataViewKind::Int8 | DataViewKind::Uint8 => 1,
            DataViewKind::Int16 | DataViewKind::Uint16 => 2,
            DataViewKind::Int32 | DataViewKind::Uint32 | DataViewKind::Float32 => 4,
            DataViewKind::Float64 => 8,
        }
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
            _ => return None,
        })
    }
}

fn throw_dataview_oob() -> ! {
    super::numeric::throw_dataview_offset_out_of_bounds()
}

#[inline]
fn to_byte_offset(value: f64) -> i64 {
    let n = crate::value::JSValue::from_bits(value.to_bits()).to_number();
    if n.is_nan() {
        0
    } else if !n.is_finite() {
        // ±Infinity → out of any finite buffer range; surfaced as OOB later.
        if n > 0.0 {
            i64::MAX
        } else {
            i64::MIN
        }
    } else {
        n.trunc() as i64
    }
}

#[inline]
fn to_number(value: f64) -> f64 {
    crate::value::JSValue::from_bits(value.to_bits()).to_number()
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
    let n = to_number(value);
    unsafe {
        match kind {
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
