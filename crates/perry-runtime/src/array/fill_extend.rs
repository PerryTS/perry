//! Bulk numeric dense fills for compiler-proven trivial loops
//! (`js_array_fill_f64_{const,iota}[_len]_extend`).
//!
//! Split out of `indexing.rs` (which owns the per-element get/set paths these
//! fall back to) to keep that file under the 2000-line cap. Every entry point
//! here is `#[no_mangle] extern "C"` and reached by emitted code only — the
//! declarations live in `perry-codegen/src/runtime_decls/arrays.rs` and the
//! loop-pattern lowering in `perry-codegen/src/stmt/loops.rs`.

use super::header::{array_numeric_layout, NumericArrayLayout};
use super::indexing_support::note_array_index_write;
use super::*;
use std::ptr;

/// Bulk numeric dense fill for compiler-proven trivial loops:
/// `for (let i = 0; i < end; i++) arr[i] = constant`.
///
/// This keeps JavaScript semantics for frozen/sealed/descriptor arrays by
/// falling back to the ordinary extending setter. The fast path is restricted
/// to dense writes from index 0 through `end - 1`, so the resulting live
/// payload is entirely numeric and can be marked raw-f64 / pointer-free once.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_const_extend(
    arr: *mut ArrayHeader,
    end: u32,
    value: f64,
) -> *mut ArrayHeader {
    if end == 0 {
        return clean_arr_ptr_mut(arr);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        let new_arr = js_array_alloc(end);
        return js_array_fill_f64_const_extend(new_arr, end, value);
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        for i in 0..end {
            out = js_array_set_f64_extend(out, i, value);
        }
        return out;
    }

    let Some(number) = value_bits_to_number(value.to_bits()) else {
        let mut out = arr;
        for i in 0..end {
            out = js_array_set_f64_extend(out, i, value);
        }
        return out;
    };

    unsafe {
        let old_length = (*arr).length;
        let raw_before = array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64);
        if old_length > end && !raw_before {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, value);
            }
            return fallback;
        }

        let mut out = if end > (*arr).capacity {
            js_array_grow(arr, end)
        } else {
            arr
        };
        out = clean_arr_ptr_mut(out);
        if out.is_null() || end > (*out).capacity {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, value);
            }
            return fallback;
        }
        let elements_ptr = (out as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..end as usize {
            // GC_STORE_AUDIT(POINTER_FREE): bulk numeric fill writes raw f64s only.
            ptr::write(elements_ptr.add(i), number);
        }
        if (*out).length < end {
            (*out).length = end;
        }
        set_array_numeric_layout(out, NumericArrayLayout::RawF64);
        out
    }
}

/// Bulk numeric dense fill for loops bounded by current array length:
/// `for (let i = 0; i < arr.length; i++) arr[i] = constant`.
///
/// If the receiver is exotic (frozen/sealed/descriptors/etc.), the fallback
/// re-reads `.length` on every iteration so it preserves the source loop's
/// observable behavior when setters mutate array length.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_const_len_extend(
    arr: *mut ArrayHeader,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    let end = js_array_length(arr);
    if end == 0 || arr.is_null() {
        return arr;
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        let mut i = 0;
        while i < js_array_length(out) {
            out = js_array_set_f64_extend(out, i, value);
            i += 1;
        }
        return out;
    }
    js_array_fill_f64_const_extend(arr, end, value)
}

/// Bulk numeric dense fill for compiler-proven trivial loops:
/// `for (let i = 0; i < end; i++) arr[i] = i`.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_iota_extend(
    arr: *mut ArrayHeader,
    end: u32,
) -> *mut ArrayHeader {
    if end == 0 {
        return clean_arr_ptr_mut(arr);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        let new_arr = js_array_alloc(end);
        return js_array_fill_f64_iota_extend(new_arr, end);
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        for i in 0..end {
            out = js_array_set_f64_extend(out, i, i as f64);
        }
        return out;
    }

    unsafe {
        let old_length = (*arr).length;
        let raw_before = array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64);
        if old_length > end && !raw_before {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, i as f64);
            }
            return fallback;
        }

        let mut out = if end > (*arr).capacity {
            js_array_grow(arr, end)
        } else {
            arr
        };
        out = clean_arr_ptr_mut(out);
        if out.is_null() || end > (*out).capacity {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, i as f64);
            }
            return fallback;
        }
        let elements_ptr = (out as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..end as usize {
            // GC_STORE_AUDIT(POINTER_FREE): bulk iota fill writes raw f64s only.
            ptr::write(elements_ptr.add(i), i as f64);
        }
        if (*out).length < end {
            (*out).length = end;
        }
        set_array_numeric_layout(out, NumericArrayLayout::RawF64);
        out
    }
}

/// Bulk numeric dense fill for loops bounded by current array length:
/// `for (let i = 0; i < arr.length; i++) arr[i] = i`.
///
/// If the receiver is exotic (frozen/sealed/descriptors/etc.), the fallback
/// re-reads `.length` on every iteration so it preserves the source loop's
/// observable behavior when setters mutate array length.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_iota_len_extend(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    let end = js_array_length(arr);
    if end == 0 || arr.is_null() {
        return arr;
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        let mut i = 0;
        while i < js_array_length(out) {
            out = js_array_set_f64_extend(out, i, i as f64);
            i += 1;
        }
        return out;
    }
    js_array_fill_f64_iota_extend(arr, end)
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_FILL_F64_CONST_EXTEND: extern "C" fn(
    *mut ArrayHeader,
    u32,
    f64,
) -> *mut ArrayHeader = js_array_fill_f64_const_extend;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_FILL_F64_IOTA_EXTEND: extern "C" fn(*mut ArrayHeader, u32) -> *mut ArrayHeader =
    js_array_fill_f64_iota_extend;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_FILL_F64_CONST_LEN_EXTEND: extern "C" fn(
    *mut ArrayHeader,
    f64,
) -> *mut ArrayHeader = js_array_fill_f64_const_len_extend;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_FILL_F64_IOTA_LEN_EXTEND: extern "C" fn(*mut ArrayHeader) -> *mut ArrayHeader =
    js_array_fill_f64_iota_len_extend;
