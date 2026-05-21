//! concat / reverse / fill.
use super::*;
use std::ptr;

/// Append all elements from source array to destination array
/// Returns the (possibly reallocated) destination array pointer
#[no_mangle]
pub extern "C" fn js_array_concat(
    dest: *mut ArrayHeader,
    src: *const ArrayHeader,
) -> *mut ArrayHeader {
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return dest;
    }
    // Detect non-array sources: Sets register themselves in
    // SET_REGISTRY; convert to array first so spread-into-array
    // `[...new Set(...)]` reads the right elements instead of the
    // SetHeader's raw memory.
    if crate::set::is_registered_set(src as usize) {
        let arr = crate::set::js_set_to_array(src as *const crate::set::SetHeader);
        return js_array_concat(dest, arr);
    }
    // Same treatment for Maps — `[...map]` materializes [key, value]
    // pair Arrays. Without this branch, the loop below reads the
    // MapHeader's `size` field as `length` and pulls keys/values out of
    // the wrong offsets, producing garbage f64s (issue #540). The
    // companion `Array.from(map)` path goes through `js_array_clone`
    // which already has the matching Map arm.
    if crate::map::is_registered_map(src as usize) {
        let arr = crate::map::js_map_entries(src as *const crate::map::MapHeader);
        return js_array_concat(dest, arr);
    }
    // Issue #578: typed-array source — materialize through the per-kind
    // accessor so `[...new Uint8Array([1,2,3])]` and `arr.concat(typedArr)`
    // see the byte values, not the byte buffer reinterpreted as f64.
    if crate::typedarray::lookup_typed_array_kind(src as usize).is_some() {
        let arr = crate::typedarray::typed_array_to_array(
            src as *const crate::typedarray::TypedArrayHeader,
        );
        return js_array_concat(dest, arr);
    }
    // Uint8Array (legacy Buffer-backed) source — materialize byte values.
    if crate::buffer::is_registered_buffer(src as usize) {
        let arr = crate::buffer::buffer_to_array(src as *const crate::buffer::BufferHeader);
        return js_array_concat(dest, arr);
    }
    unsafe {
        let src_len = (*src).length;
        if src_len == 0 {
            return dest;
        }

        let src_elements = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Bulk-copy fast path: pre-grow once to fit dest_len+src_len,
        // then memcpy the source elements into the dest tail and update
        // length once. Replaces N individual `js_array_push_f64` calls
        // (each doing a forwarding-chain follow + capacity check). The
        // alias case (dest == src) is rare but possible — fall back to
        // the per-element loop for that, since growing dest invalidates
        // the src_elements pointer.
        let dest_resolved = clean_arr_ptr_mut(dest);
        if !dest_resolved.is_null() && dest_resolved as *const _ != src {
            let dest_len = (*dest_resolved).length;
            let new_len = dest_len + src_len;
            let result = if new_len > (*dest_resolved).capacity {
                js_array_grow(dest_resolved, new_len)
            } else {
                dest_resolved
            };
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            ptr::copy_nonoverlapping(
                src_elements,
                dst_elements.add(dest_len as usize),
                src_len as usize,
            );
            (*result).length = new_len;
            rebuild_array_layout_exact(result);
            return result;
        }

        // Fallback: per-element push (handles aliasing + null dest).
        let mut result = dest;
        for i in 0..src_len as usize {
            let element = *src_elements.add(i);
            result = js_array_push_f64(result, element);
        }
        result
    }
}

/// JS-semantic `Array.prototype.concat`: returns a NEW array with the
/// elements of both `arr` and `other`. Neither input is mutated. This is
/// what users get when they call `a.concat(b)`. `js_array_concat` above
/// mutates its first argument and is reserved for the internal
/// push-spread desugaring path.
#[no_mangle]
pub extern "C" fn js_array_concat_new(
    arr: *const ArrayHeader,
    other: *const ArrayHeader,
) -> *mut ArrayHeader {
    let a = clean_arr_ptr(arr);
    let b = clean_arr_ptr(other);
    unsafe {
        let a_len = if a.is_null() { 0 } else { (*a).length };
        let b_len = if b.is_null() { 0 } else { (*b).length };
        let total = a_len + b_len;

        let mut result = js_array_alloc(total);
        if !a.is_null() && a_len > 0 {
            let src = (a as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            for i in 0..a_len as usize {
                result = js_array_push_f64(result, *src.add(i));
            }
        }
        if !b.is_null() && b_len > 0 {
            let src = (b as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            for i in 0..b_len as usize {
                result = js_array_push_f64(result, *src.add(i));
            }
        }
        result
    }
}

/// `Array.prototype.reverse` — reverses in place and returns the same pointer.
#[no_mangle]
pub extern "C" fn js_array_reverse(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    unsafe {
        let len = (*arr).length as usize;
        if len <= 1 {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let mut i = 0usize;
        let mut j = len - 1;
        while i < j {
            let tmp = *elements.add(i);
            *elements.add(i) = *elements.add(j);
            *elements.add(j) = tmp;
            i += 1;
            j -= 1;
        }
        rebuild_array_layout(arr);
        arr
    }
}

/// `Array.prototype.fill(value)` — fills every element (0..length) with
/// `value`. Returns the same array pointer.
#[no_mangle]
pub extern "C" fn js_array_fill(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    unsafe {
        let len = (*arr).length as usize;
        if len == 0 {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len {
            *elements.add(i) = value;
        }
        rebuild_array_layout(arr);
        arr
    }
}

/// `Array.prototype.fill(value, start, end)` — fills the index range
/// `[start, end)` with `value`. Per ECMA-262: negative indices count from
/// the end (`len + idx`), then are clamped to `[0, len]`. `end > len`
/// clamps to `len`, `start > end` yields no-op. Returns the same array.
#[no_mangle]
pub extern "C" fn js_array_fill_range(
    arr: *mut ArrayHeader,
    value: f64,
    start: f64,
    end: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    unsafe {
        let len = (*arr).length as i64;
        if len == 0 {
            return arr;
        }
        let clamp = |idx: f64, default_to_len: bool| -> i64 {
            if idx.is_nan() {
                return 0;
            }
            let mut i = idx as i64;
            if idx.is_infinite() {
                if idx > 0.0 {
                    return len;
                }
                if default_to_len {
                    return len;
                }
                return 0;
            }
            if i < 0 {
                i += len;
                if i < 0 {
                    i = 0;
                }
            }
            if i > len {
                i = len;
            }
            i
        };
        let s = clamp(start, false);
        let e = clamp(end, true);
        if s >= e {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in s..e {
            *elements.add(i as usize) = value;
        }
        rebuild_array_layout(arr);
        arr
    }
}
