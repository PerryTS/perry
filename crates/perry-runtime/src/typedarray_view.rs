//! TypedArray constructor bounds/alignment checks and common 16-byte views.
//! Each view names its flattened owner and byte offset; no cached data address
//! or view metadata table participates in access or collection.

use crate::typedarray::{
    clean_ta_ptr, elem_size_for_kind, js_typed_array_new, name_for_kind, throw_range_error,
    TypedArrayHeader,
};

/// `ToIndex(value)` for a typed-array view's `byteOffset` / `length`
/// arguments (#4103): `undefined` / `NaN` → 0, otherwise `ToIntegerOrInfinity`
/// truncated toward zero, with a `RangeError` for a negative or
/// out-of-`[[0, 2^53-1]]` result.
pub(crate) fn typed_array_view_to_index(value: f64) -> i64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return 0;
    }
    // Real `ToNumber`: runs `valueOf`/`Symbol.toPrimitive` on objects
    // (observable, may throw) and throws TypeError on a Symbol or BigInt
    // argument — `jv.to_number()` did none of that, so an object byteOffset
    // silently became 0 and Symbol offsets/lengths never threw.
    let n = crate::typedarray::jsvalue_to_f64(value);
    if n.is_nan() {
        return 0;
    }
    // `ToIntegerOrInfinity` truncates BEFORE the range check, so a value in
    // (-1, 0) is 0, not a RangeError.
    let integer = n.trunc();
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        throw_range_error(b"Invalid typed array length");
    }
    integer as i64
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
    let handles = crate::gc::RuntimeHandleScope::new();
    let source = handles.root_nanbox_f64(source);
    let length_value = handles.root_nanbox_f64(length_value);
    let bpe = elem_size_for_kind(kind) as i64;

    // ES ordering (InitializeTypedArrayFromArrayBuffer): ToIndex(byteOffset)
    // and ToIndex(length) run BEFORE the IsDetachedBuffer check — either can
    // execute user code (`valueOf`) that detaches the buffer — so all buffer
    // reads and bounds checks happen after, against post-coercion state.
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
    let length_jv = crate::value::JSValue::from_bits(length_value.get_nanbox_f64().to_bits());
    let requested = if length_jv.is_undefined() {
        None
    } else {
        Some(typed_array_view_to_index(length_value.get_nanbox_f64()))
    };
    let addr = crate::value::JSValue::from_bits(source.get_nanbox_f64().to_bits())
        .as_pointer::<crate::buffer::BufferHeader>() as usize;
    if crate::buffer::is_detached_buffer(addr) {
        crate::typedarray::throw_type_error(b"Cannot perform Construct on a detached ArrayBuffer");
    }
    let src = addr as *const crate::buffer::BufferHeader;
    let total_len = crate::buffer::js_buffer_length(src) as i64;
    if offset > total_len {
        throw_range_error(
            format!("Start offset {offset} is outside the bounds of the buffer").as_bytes(),
        );
    }

    let elem_count = match requested {
        None => {
            let remaining = total_len - offset;
            // ES2024: the whole-multiple requirement is for a FIXED-length
            // buffer only. A length-tracking view over a resizable one floors
            // (its length is recomputed on every resize anyway) (#10873).
            if bpe > 1 && remaining % bpe != 0 && !crate::buffer::is_resizable_buffer(addr) {
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
        }
        Some(requested) => {
            if offset + requested * bpe > total_len {
                throw_range_error(format!("Invalid typed array length: {requested}").as_bytes());
            }
            requested
        }
    };

    crate::buffer::store::new_view(
        crate::typedarray::type_for_kind(kind),
        addr,
        offset as u32,
        elem_count.max(0) as u32,
        requested.is_none(),
    )
    .cast()
}

#[derive(Copy, Clone)]
pub(crate) struct ViewMeta {
    pub backing: usize,
    pub byte_offset: u32,
}

#[inline]
pub(crate) fn is_view_length_tracking(addr: usize) -> bool {
    crate::buffer::store::is_view(addr)
        && unsafe {
            (*crate::buffer::store::header(addr))._reserved
                & crate::codegen_abi::BYTES_LENGTH_TRACKING
                != 0
        }
}

#[inline]
pub(crate) fn view_meta_of(addr: usize) -> Option<ViewMeta> {
    if !crate::buffer::store::is_view(addr) {
        return None;
    }
    Some(unsafe {
        ViewMeta {
            backing: crate::buffer::store::owner(addr),
            byte_offset: (*(addr as *const crate::buffer::BufferHeader)).capacity,
        }
    })
}

pub fn js_typed_array_byte_offset(ta: *const TypedArrayHeader) -> u32 {
    crate::buffer::buffer_byte_offset(clean_ta_ptr(ta) as usize)
}

pub fn js_typed_array_backing_buffer(
    ta: *const TypedArrayHeader,
) -> *mut crate::buffer::BufferHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return std::ptr::null_mut();
    }
    crate::buffer::ensure_buffer_ab_alias(ta as usize) as *mut crate::buffer::BufferHeader
}
