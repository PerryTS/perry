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
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        let item_handles = if items.is_null() || items_count == 0 {
            Vec::new()
        } else {
            scope.root_nanbox_f64_slice(std::slice::from_raw_parts(items, items_count as usize))
        };
        // Runtime plain-object receiver behind a statically-Array variable
        // (`var x = []; … x = {0:0,1:1}; x.splice(1,1)` — test262
        // splice/S15.4.4.12_A4_T1 #7): reading it as an ArrayHeader corrupts
        // (and `clean_arr_ptr` may NULL it out, silently no-op'ing); run the
        // generic spec engine on the object instead. Probe the RAW pointer
        // BEFORE the array-plausibility clean.
        if let Some(recv) =
            crate::array::non_array_object_receiver(arr_handle.get_raw_mut_ptr::<ArrayHeader>())
        {
            let mut args: Vec<f64> = vec![
                start as f64,
                if delete_count == i32::MAX {
                    f64::INFINITY
                } else {
                    delete_count as f64
                },
            ];
            for item in &item_handles {
                args.push(item.get_nanbox_f64());
            }
            let removed = crate::array::object_splice(recv, args.as_ptr(), args.len());
            let removed_ptr = crate::value::js_nanbox_get_pointer(removed) as *mut ArrayHeader;
            if !out_arr.is_null() {
                *out_arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            }
            return removed_ptr;
        }
        let arr = clean_arr_ptr_mut(arr_handle.get_raw_mut_ptr::<ArrayHeader>());
        if arr.is_null() {
            if !out_arr.is_null() {
                *out_arr = js_array_alloc(0);
            }
            return js_array_alloc(0);
        }
        arr_handle.set_raw_mut_ptr(arr);
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
        let recv_value = f64::from_bits(
            crate::value::JSValue::pointer(arr_handle.get_raw_const_ptr::<u8>()).bits(),
        );
        let (deleted_box, deleted_is_default) =
            crate::array::species::array_species_create_with_capacity(
                recv_value,
                actual_delete as usize,
                actual_delete,
            );
        let deleted_box_handle = scope.root_nanbox_f64(deleted_box);
        let deleted_handle = scope
            .root_raw_mut_ptr(crate::value::js_nanbox_get_pointer(deleted_box) as *mut ArrayHeader);

        // Copy deleted elements to return array. ECMA-262 §23.1.3.31 step
        // 12.b: each removed index goes through HasProperty/Get — a hole
        // backed by an inherited `Array.prototype[k]` element lands as an OWN
        // property of the deleted array (test262 splice/S15.4.4.12_A4_T3);
        // a genuinely absent index stays a hole.
        let spec_read = |i: usize| -> f64 {
            let idx = start_idx + i as u32;
            if crate::array::array_spec_has_index(
                arr_handle.get_raw_const_ptr::<ArrayHeader>(),
                idx,
            ) {
                // HasProperty can invoke user code and collect. Re-read the
                // receiver through its handle before the subsequent Get.
                return crate::array::array_spec_get(
                    arr_handle.get_raw_const_ptr::<ArrayHeader>(),
                    idx,
                );
            }
            f64::from_bits(crate::value::TAG_HOLE)
        };
        if deleted_is_default {
            let deleted = deleted_handle.get_raw_mut_ptr::<ArrayHeader>();
            (*deleted).length = actual_delete;
            if crate::array::array_iteration_is_exotic(arr_handle.get_raw_const_ptr()) {
                for i in 0..actual_delete as usize {
                    let value = spec_read(i);
                    note_array_slot(
                        deleted_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        i,
                        value.to_bits(),
                    );
                }
            } else {
                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                let elements_ptr =
                    (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
                let deleted = deleted_handle.get_raw_mut_ptr::<ArrayHeader>();
                let deleted_elements =
                    (deleted as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
                for i in 0..actual_delete as usize {
                    // GC_STORE_AUDIT(BARRIERED): the dense copy cannot run user code and is followed by a layout rebuild.
                    ptr::write(
                        deleted_elements.add(i),
                        ptr::read(elements_ptr.add(start_idx as usize + i)),
                    );
                }
                rebuild_array_layout(deleted);
            }
        } else {
            for i in 0..actual_delete as usize {
                let value = spec_read(i);
                if value.to_bits() == crate::value::TAG_HOLE {
                    continue;
                }
                crate::array::species::species_result_create_data_property(
                    deleted_box_handle.get_nanbox_f64(),
                    i,
                    value,
                );
            }
            crate::array::from_concat::set_result_length(
                deleted_box_handle.get_nanbox_f64(),
                actual_delete as usize,
            );
        }

        // Calculate new length
        let new_len = len as u32 - actual_delete + items_count;

        // Grow array if needed
        let current = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        let arr = if new_len > (*current).capacity {
            js_array_grow(current, new_len)
        } else {
            current
        };
        arr_handle.set_raw_mut_ptr(arr);
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
        if !item_handles.is_empty() {
            for (i, item_handle) in item_handles.iter().enumerate() {
                let item = item_handle.get_nanbox_f64();
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

        deleted_handle.get_raw_mut_ptr::<ArrayHeader>()
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

/// Splice entry point for JS-valued start/deleteCount arguments. Root the
/// receiver and every argument before either ToIntegerOrInfinity coercion:
/// `valueOf` can run user code and evacuate the array or a later inserted
/// value before [`js_array_splice`] receives raw pointers.
#[no_mangle]
pub extern "C" fn js_array_splice_values(
    arr: *mut ArrayHeader,
    start_value: f64,
    delete_count_value: f64,
    provided_prefix: u32,
    items: *const f64,
    items_count: u32,
    out_arr: *mut *mut ArrayHeader,
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = scope.root_raw_mut_ptr(arr);
    let start_value = scope.root_nanbox_f64(start_value);
    let delete_count_value = scope.root_nanbox_f64(delete_count_value);
    let item_handles = if items.is_null() || items_count == 0 {
        Vec::new()
    } else {
        unsafe {
            scope.root_nanbox_f64_slice(std::slice::from_raw_parts(items, items_count as usize))
        }
    };

    let start = if provided_prefix >= 1 {
        array_slice_start_index(start_value.get_nanbox_f64())
    } else {
        0
    };
    let delete_count = match provided_prefix {
        0 => 0,
        1 => i32::MAX,
        _ => js_array_splice_delete_count(delete_count_value.get_nanbox_f64()),
    };
    let refreshed_items = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&item_handles);
    js_array_splice(
        arr.get_raw_mut_ptr::<ArrayHeader>(),
        start,
        delete_count,
        refreshed_items.as_ptr(),
        refreshed_items.len() as u32,
        out_arr,
    )
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = scope.root_raw_const_ptr(arr);
    let start_value = scope.root_nanbox_f64(start_value);
    let end_value = scope.root_nanbox_f64(end_value);
    let start = array_slice_start_index(start_value.get_nanbox_f64());
    let end = array_slice_end_index(end_value.get_nanbox_f64());
    js_array_slice(arr.get_raw_const_ptr::<ArrayHeader>(), start, end)
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
        let scope = crate::gc::RuntimeHandleScope::new();
        let source = super::sort::RootedArrayElems::new(&scope, arr as *mut ArrayHeader);
        let len = (*source.arr()).length as i32;

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
        let recv_value =
            f64::from_bits(crate::value::JSValue::pointer(source.arr() as *const u8).bits());
        let (result_box, is_default) = crate::array::species::array_species_create_with_capacity(
            recv_value,
            slice_len as usize,
            slice_len,
        );
        let result_box_handle = scope.root_nanbox_f64(result_box);

        // Copy elements — use spec-generic Get when the source is exotic
        // (has Array.prototype or Object.prototype indexed properties) so
        // inherited indices appear in the result just as [[Get]] would return
        // them (ECMA-262 §23.1.3.25 step 8b "If HasProperty(O, from)…").
        let src_exotic = crate::array::array_iteration_is_exotic(source.arr());
        if is_default {
            let result = super::sort::RootedArrayElems::new(
                &scope,
                crate::value::js_nanbox_get_pointer(result_box_handle.get_nanbox_f64())
                    as *mut ArrayHeader,
            );
            (*result.arr()).length = slice_len;
            if src_exotic {
                for i in 0..slice_len as usize {
                    let src_idx = start_idx as usize + i;
                    let v = if crate::array::array_spec_has_index(source.arr(), src_idx as u32) {
                        crate::array::array_spec_get(source.arr(), src_idx as u32)
                    } else {
                        f64::from_bits(crate::value::TAG_HOLE)
                    };
                    // The exotic read above can run user code and can throw
                    // or collect. Record each result write immediately:
                    // deferring a bulk rebuild until the loop returned left
                    // an existing plain array returned by @@species with a
                    // permanently stale side mask when a later getter threw.
                    // A collection from a later getter could also observe the
                    // same incomplete description while this loop was still
                    // running (#9983).
                    result.set(i, v);
                }
            } else {
                let src_elements = (source.arr() as *const u8)
                    .add(std::mem::size_of::<ArrayHeader>())
                    as *const f64;
                let result_elements =
                    (result.arr() as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
                for i in 0..slice_len as usize {
                    let src_idx = start_idx as usize + i;
                    // GC_STORE_AUDIT(BARRIERED): slice result init is followed by layout/barrier rebuild.
                    ptr::write(result_elements.add(i), ptr::read(src_elements.add(src_idx)));
                }
                rebuild_array_layout(result.arr());
            }
        } else {
            // Custom species container: CreateDataPropertyOrThrow per element.
            // Defining a result property can invoke a Proxy trap that changes
            // a later source index or its prototype, so HasProperty/Get must
            // be repeated on every iteration even when the source began dense.
            for i in 0..slice_len as usize {
                let src_idx = start_idx as usize + i;
                if !crate::array::array_spec_has_index(source.arr(), src_idx as u32) {
                    continue; // absent slot → no property created in result
                }
                let v = crate::array::array_spec_get(source.arr(), src_idx as u32);
                crate::array::species::species_result_create_data_property(
                    result_box_handle.get_nanbox_f64(),
                    i,
                    v,
                );
            }
        }

        crate::value::js_nanbox_get_pointer(result_box_handle.get_nanbox_f64()) as *mut ArrayHeader
    }
}
