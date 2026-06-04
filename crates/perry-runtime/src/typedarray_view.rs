//! `new TA(buffer, byteOffset, length?)` view-constructor validation (#4103).
//!
//! Split out of `typedarray.rs` to keep that file under the 2000-line gate.
//! Perry does not yet model real offset-honoring views (the `byteOffset`
//! getter still reports 0 and the view copies rather than aliases the backing
//! store — the broader gap #4103 calls out); this module adds the spec
//! `RangeError` bounds/alignment validation and copies the correct bytes at
//! the requested offset so the multi-arg form matches Node.

use std::ptr;

use crate::typedarray::{
    data_ptr_mut, elem_size_for_kind, js_typed_array_new, name_for_kind, throw_range_error,
    typed_array_alloc, TypedArrayHeader,
};

/// `ToIndex(value)` for a typed-array view's `byteOffset` / `length`
/// arguments (#4103): `undefined` / `NaN` → 0, otherwise `ToIntegerOrInfinity`
/// truncated toward zero, with a `RangeError` for a negative or
/// out-of-`[[0, 2^53-1]]` result.
fn typed_array_view_to_index(value: f64) -> i64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return 0;
    }
    let n = jv.to_number();
    if n.is_nan() {
        return 0;
    }
    if n < 0.0 || n > 9_007_199_254_740_991.0 {
        throw_range_error(b"Invalid typed array length");
    }
    n.trunc() as i64
}

/// `new TA(buffer, byteOffset, length?)` for the non-`Uint8Array` typed-array
/// kinds — create a view over an `ArrayBuffer` with the spec's offset/length
/// validation (#4103). Perry still models the result as an owning
/// `TypedArrayHeader` (the `byteOffset` getter reports 0), but the bounds
/// checks match Node and throw `RangeError` on violation:
///   - `byteOffset % BYTES_PER_ELEMENT == 0` (alignment),
///   - `byteOffset <= buffer.byteLength`,
///   - when `length` is omitted, the remaining bytes are a whole multiple of
///     `BYTES_PER_ELEMENT`,
///   - `byteOffset + length * BYTES_PER_ELEMENT <= buffer.byteLength`.
/// `offset_value` / `length_value` are the raw NaN-boxed arguments
/// (`undefined` when absent) so `ToIndex` runs here. A non-`ArrayBuffer`
/// source routes to the normal `js_typed_array_new` constructor.
#[no_mangle]
pub extern "C" fn js_typed_array_view(
    kind: i32,
    source: f64,
    offset_value: f64,
    length_value: f64,
) -> *mut TypedArrayHeader {
    let kind = kind as u8;
    let bits = source.to_bits();
    if (bits >> 48) != 0x7FFD {
        return js_typed_array_new(kind as i32, source);
    }
    let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if !crate::buffer::is_registered_buffer(addr) || !crate::buffer::is_any_array_buffer(addr) {
        return js_typed_array_new(kind as i32, source);
    }
    let bpe = elem_size_for_kind(kind) as i64;
    let src = addr as *const crate::buffer::BufferHeader;
    let total_len = unsafe { (*src).length as i64 };

    let offset = typed_array_view_to_index(offset_value);
    if bpe > 1 && offset % bpe != 0 {
        throw_range_error(
            format!(
                "start offset of {} should be a multiple of {}",
                name_for_kind(kind),
                bpe
            )
            .as_bytes(),
        );
    }
    if offset > total_len {
        throw_range_error(
            format!("Start offset {offset} is outside the bounds of the buffer").as_bytes(),
        );
    }

    let length_jv = crate::value::JSValue::from_bits(length_value.to_bits());
    let elem_count = if length_jv.is_undefined() {
        let remaining = total_len - offset;
        if bpe > 1 && remaining % bpe != 0 {
            throw_range_error(
                format!(
                    "byte length of {} should be a multiple of {}",
                    name_for_kind(kind),
                    bpe
                )
                .as_bytes(),
            );
        }
        remaining / bpe
    } else {
        let requested = typed_array_view_to_index(length_value);
        if offset + requested * bpe > total_len {
            throw_range_error(format!("Invalid typed array length: {requested}").as_bytes());
        }
        requested
    };

    // The native byte layout matches between BufferHeader and TypedArrayHeader,
    // so copy the backing bytes at `offset` directly to preserve element values.
    let count = elem_count.max(0) as u32;
    let ta = typed_array_alloc(kind, count);
    if crate::buffer::is_shared_array_buffer(addr) {
        crate::typedarray::mark_typed_array_shared_backing(ta);
    }
    if count > 0 {
        unsafe {
            let src_data = crate::buffer::buffer_data(src).add(offset as usize);
            let dst = data_ptr_mut(ta);
            ptr::copy_nonoverlapping(src_data, dst, (count as i64 * bpe) as usize);
        }
    }
    ta
}
