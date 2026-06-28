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
        // Runtime plain-object receiver behind a statically-Array variable
        // (`var x = []; … x = {0:0,1:1}; x.splice(1,1)` — test262
        // splice/S15.4.4.12_A4_T1 #7): reading it as an ArrayHeader corrupts
        // (and `clean_arr_ptr` may NULL it out, silently no-op'ing); run the
        // generic spec engine on the object instead. Probe the RAW pointer
        // BEFORE the array-plausibility clean.
        if let Some(recv) = crate::array::non_array_object_receiver(arr) {
            let mut args: Vec<f64> = vec![
                start as f64,
                if delete_count == i32::MAX {
                    f64::INFINITY
                } else {
                    delete_count as f64
                },
            ];
            if !items.is_null() {
                for i in 0..items_count as usize {
                    args.push(*items.add(i));
                }
            }
            let removed = crate::array::object_splice(recv, args.as_ptr(), args.len());
            let removed_ptr = crate::value::js_nanbox_get_pointer(removed) as *mut ArrayHeader;
            if !out_arr.is_null() {
                *out_arr = arr;
            }
            return removed_ptr;
        }
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

        // Create array of deleted elements via ArraySpeciesCreate (ECMA-262
        // §23.1.3.31 step 11): reads `O.constructor` / `@@species` and throws
        // on a poisoned getter or non-constructor species before the receiver
        // is mutated.
        let recv_value = f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
        let deleted_box =
            crate::array::species::array_species_create(recv_value, actual_delete as usize);
        let deleted_is_plain = crate::array::species::species_result_is_plain_array(deleted_box);
        let deleted = crate::value::js_nanbox_get_pointer(deleted_box) as *mut ArrayHeader;

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Copy deleted elements to return array. ECMA-262 §23.1.3.31 step
        // 12.b: each removed index goes through HasProperty/Get — a hole
        // backed by an inherited `Array.prototype[k]` element lands as an OWN
        // property of the deleted array (test262 splice/S15.4.4.12_A4_T3);
        // a genuinely absent index stays a hole.
        let spec_read = |i: usize| -> f64 {
            let v = *elements_ptr.add(start_idx as usize + i);
            if v.to_bits() == crate::value::TAG_HOLE {
                let idx = start_idx + i as u32;
                if crate::array::array_spec_has_index(arr, idx) {
                    return crate::array::array_spec_get(arr, idx);
                }
            }
            v
        };
        if deleted_is_plain {
            (*deleted).length = actual_delete;
            let deleted_elements =
                (deleted as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for i in 0..actual_delete as usize {
                // GC_STORE_AUDIT(BARRIERED): deleted-array init is followed by layout/barrier rebuild.
                ptr::write(deleted_elements.add(i), spec_read(i));
            }
            rebuild_array_layout(deleted);
        } else {
            for i in 0..actual_delete as usize {
                crate::array::species::species_result_set(deleted_box, i, spec_read(i));
            }
        }

        // Calculate new length
        let new_len = len as u32 - actual_delete + items_count;

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
            // GC_STORE_AUDIT(BARRIERED): splice tail memmove is followed by layout/barrier rebuild.
            ptr::copy(src, dst, tail_len as usize);
        }

        // Insert new items
        if items_count > 0 && !items.is_null() {
            for i in 0..items_count as usize {
                let item = *items.add(i);
                // A uniquely-owned string spliced in now aliases the array slot —
                // demote it to shared so a later `s += x` doesn't mutate it in
                // place. No-op for SSO / non-string. (This insert path doesn't
                // funnel through `note_array_slot`.)
                crate::string::js_string_addref_if_heap_string(item);
                // GC_STORE_AUDIT(BARRIERED): splice inserted item writes are followed by layout/barrier rebuild.
                ptr::write(elements_ptr.add(start_idx as usize + i), item);
            }
        }

        // ECMA-262 §23.1.3.31 step 24: Set(O, "length", …, true) — throws on a
        // non-writable `length` (test262 splice/S15.4.4.12_A6.1_T2/T3).
        super::push_pop::guard_writable_length(arr);
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

#[no_mangle]
pub extern "C" fn js_array_splice_delete_count(value: f64) -> i32 {
    array_slice_value_to_index(value)
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    // #3148: TypedArray slice — return a same-kind TypedArray.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_slice(
            arr as *const crate::typedarray::TypedArrayHeader,
            start,
            end,
        ) as *mut ArrayHeader;
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

        // ECMA-262 §23.1.3.25 step 8: ArraySpeciesCreate(O, count) — reads
        // `O.constructor` / `@@species` and throws on a poisoned getter or
        // non-constructor species before any element is copied.
        let recv_value = f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
        let result_box =
            crate::array::species::array_species_create(recv_value, slice_len as usize);
        let is_plain = crate::array::species::species_result_is_plain_array(result_box);
        let result = crate::value::js_nanbox_get_pointer(result_box) as *mut ArrayHeader;

        // Copy elements — use spec-generic Get when the source is exotic
        // (has Array.prototype or Object.prototype indexed properties) so
        // inherited indices appear in the result just as [[Get]] would return
        // them (ECMA-262 §23.1.3.25 step 8b "If HasProperty(O, from)…").
        let src_elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let src_exotic = crate::array::array_iteration_is_exotic(arr);
        if is_plain {
            (*result).length = slice_len;
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for i in 0..slice_len as usize {
                let src_idx = start_idx as usize + i;
                let v = if src_exotic {
                    if crate::array::array_spec_has_index(arr, src_idx as u32) {
                        crate::array::array_spec_get(arr, src_idx as u32)
                    } else {
                        f64::from_bits(crate::value::TAG_HOLE)
                    }
                } else {
                    // GC_STORE_AUDIT(BARRIERED): slice result init is followed by layout/barrier rebuild.
                    ptr::read(src_elements.add(src_idx))
                };
                ptr::write(dst_elements.add(i), v);
            }
            rebuild_array_layout(result);
        } else {
            // Custom species container: CreateDataPropertyOrThrow per element.
            for i in 0..slice_len as usize {
                let src_idx = start_idx as usize + i;
                let v = if src_exotic {
                    if !crate::array::array_spec_has_index(arr, src_idx as u32) {
                        continue; // absent slot → no property created in result
                    }
                    crate::array::array_spec_get(arr, src_idx as u32)
                } else {
                    let v = ptr::read(src_elements.add(src_idx));
                    if v.to_bits() == crate::value::TAG_HOLE {
                        continue; // hole → no property created in result
                    }
                    v
                };
                crate::array::species::species_result_set(result_box, i, v);
            }
        }

        result
    }
}
