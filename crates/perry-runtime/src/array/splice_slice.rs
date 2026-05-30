//! splice + slice.
use super::*;
use std::ptr;

/// Splice an array - removes elements and optionally inserts new ones
/// start: starting index (can be negative for from-end)
/// delete_count: number of elements to delete
/// items: pointer to elements to insert (can be null if no items)
/// items_count: number of elements to insert
/// Returns a new array containing the deleted elements
/// ALSO modifies arr in place, returns the modified array pointer (may have reallocated)
/// The return is packed: lower 48 bits = deleted array ptr, we return via out param
#[no_mangle]
pub extern "C" fn js_array_splice(
    arr: *mut ArrayHeader,
    start: i32,
    delete_count: i32,
    items: *const f64,
    items_count: u32,
    out_arr: *mut *mut ArrayHeader,
) -> *mut ArrayHeader {
    unsafe {
        let arr = clean_arr_ptr_mut(arr);
        if arr.is_null() {
            if !out_arr.is_null() {
                *out_arr = js_array_alloc(0);
            }
            return js_array_alloc(0);
        }
        let len = (*arr).length as i32;

        // Normalize start index
        let start_idx = if start < 0 {
            (len + start).max(0) as u32
        } else {
            (start as u32).min(len as u32)
        };

        // Normalize delete count
        let actual_delete = if delete_count < 0 {
            0
        } else {
            (delete_count as u32).min(len as u32 - start_idx)
        };

        // Create array of deleted elements
        let deleted = js_array_alloc(actual_delete);
        (*deleted).length = actual_delete;

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let deleted_elements =
            (deleted as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Copy deleted elements to return array
        for i in 0..actual_delete as usize {
            ptr::write(
                deleted_elements.add(i),
                *elements_ptr.add(start_idx as usize + i),
            );
        }
        rebuild_array_layout(deleted);

        // Calculate new length
        let new_len = (len as u32 - actual_delete + items_count) as u32;

        // Grow array if needed
        let arr = if new_len > (*arr).capacity {
            js_array_grow(arr, new_len)
        } else {
            arr
        };
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Shift elements after the splice point
        let tail_start = start_idx + actual_delete;
        let tail_len = len as u32 - tail_start;

        if items_count != actual_delete && tail_len > 0 {
            // Need to shift the tail
            let src = elements_ptr.add(tail_start as usize);
            let dst = elements_ptr.add((start_idx + items_count) as usize);
            ptr::copy(src, dst, tail_len as usize);
        }

        // Insert new items
        if items_count > 0 && !items.is_null() {
            for i in 0..items_count as usize {
                ptr::write(elements_ptr.add(start_idx as usize + i), *items.add(i));
            }
        }

        (*arr).length = new_len;
        rebuild_array_layout(arr);

        // Return modified array via out param
        *out_arr = arr;

        deleted
    }
}

fn array_slice_value_to_index(value: f64) -> i32 {
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() {
        0
    } else if number >= i32::MAX as f64 {
        i32::MAX
    } else if number <= i32::MIN as f64 {
        i32::MIN
    } else {
        number.trunc() as i32
    }
}

fn array_slice_start_index(value: f64) -> i32 {
    array_slice_value_to_index(value)
}

fn array_slice_end_index(value: f64) -> i32 {
    if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        i32::MAX
    } else {
        array_slice_value_to_index(value)
    }
}

#[no_mangle]
pub extern "C" fn js_array_slice_values(
    arr: *const ArrayHeader,
    start_value: f64,
    end_value: f64,
) -> *mut ArrayHeader {
    let start = array_slice_start_index(start_value);
    let end = array_slice_end_index(end_value);
    js_array_slice(arr, start, end)
}

/// Slice an array, returning a new array with elements from start to end (exclusive).
/// Handles negative indices (from end of array).
/// If end is i32::MAX, slices to end of array.
#[no_mangle]
pub extern "C" fn js_array_slice(
    arr: *const ArrayHeader,
    start: i32,
    end: i32,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as i32;

        // Normalize start index
        let start_idx = if start < 0 {
            (len + start).max(0) as u32
        } else {
            (start as u32).min(len as u32)
        };

        // Normalize end index
        let end_idx = if end == i32::MAX {
            len as u32
        } else if end < 0 {
            (len + end).max(0) as u32
        } else {
            (end as u32).min(len as u32)
        };

        // Calculate slice length
        let slice_len = end_idx.saturating_sub(start_idx);

        // Allocate new array
        let result = js_array_alloc(slice_len);
        (*result).length = slice_len;

        // Copy elements
        let src_elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst_elements = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for i in 0..slice_len as usize {
            ptr::write(
                dst_elements.add(i),
                ptr::read(src_elements.add(start_idx as usize + i)),
            );
        }
        rebuild_array_layout(result);

        result
    }
}
