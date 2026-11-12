//! splice + slice.
use super::*;
use std::ptr;

#[cfg(test)]
thread_local! {
    static SPLICE_COLLECT_AFTER_ROOTING_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn test_collect_after_splice_roots_once() {
    SPLICE_COLLECT_AFTER_ROOTING_ONCE.with(|armed| armed.set(true));
}

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
        if (*arr).length > (*arr).capacity {
            // A sparse Array keeps a short store under its length: the reads
            // and the tail move below work on `length` raw slots.
            let dense = densify_sparse_receiver(arr, items, items_count);
            if (*dense).length > (*dense).capacity {
                // A frozen, sealed or non-extensible Array's store does not
                // grow.
                crate::collection_iter::throw_type_error(
                    "Cannot splice a sparse array that is not extensible",
                );
            }
            return js_array_splice(dense, start, delete_count, items, items_count, out_arr);
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

        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        // `items` points at caller-owned raw storage. The storage address is
        // stable, but an evacuating collection cannot rewrite pointer values
        // inside it, so root every value before species creation can allocate
        // or invoke user code.
        let item_handles = if items.is_null() {
            Vec::new()
        } else {
            scope.root_nanbox_f64_slice(std::slice::from_raw_parts(items, items_count as usize))
        };
        #[cfg(test)]
        SPLICE_COLLECT_AFTER_ROOTING_ONCE.with(|armed| {
            if armed.replace(false) {
                crate::gc::gc_collect_minor();
            }
        });

        // Create array of deleted elements via ArraySpeciesCreate (ECMA-262
        // §23.1.3.31 step 11): reads `O.constructor` / `@@species` and throws
        // on a poisoned getter or non-constructor species before the receiver
        // is mutated.
        let recv_value = arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
            f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
        });
        // Only the default species arm yields a fresh plain array to fill by
        // slot writes; a species constructor's result is defined into.
        let (deleted_box, deleted_is_plain) =
            crate::array::species::array_species_create(recv_value, actual_delete as usize);
        let deleted_handle = scope.root_nanbox_f64(deleted_box);
        let removed_value_handle =
            scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED));

        // Copy deleted elements to return array. ECMA-262 §23.1.3.31 step
        // 12.b: each removed index goes through HasProperty/Get — a hole
        // backed by an inherited `Array.prototype[k]` element lands as an OWN
        // property of the deleted array (test262 splice/S15.4.4.12_A4_T3);
        // a genuinely absent index stays a hole.
        let spec_read = |i: usize| -> f64 {
            let v = arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
                let elements_ptr =
                    crate::array::array_elements_ptr(arr as *const ArrayHeader) as *const f64;
                *elements_ptr.add(start_idx as usize + i)
            });
            if v.to_bits() == crate::value::TAG_HOLE {
                let idx = start_idx + i as u32;
                if arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
                    crate::array::array_spec_has_index(arr, idx)
                }) {
                    return arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
                        crate::array::array_spec_get(arr, idx)
                    });
                }
            }
            v
        };
        if deleted_is_plain {
            let deleted = crate::value::js_nanbox_get_pointer(deleted_handle.get_nanbox_f64())
                as *mut ArrayHeader;
            (*deleted).length = actual_delete;
            // Hole reads also consult recorded custom prototypes, which are
            // not covered by array_iteration_is_exotic's canonical-proto flags.
            let src_exotic = arr_handle
                .with_mut_ptr::<ArrayHeader, _>(|arr| crate::array::array_iteration_is_exotic(arr))
                || crate::object::prototype_chain::array_static_proto_recorded();
            for i in 0..actual_delete as usize {
                let value = spec_read(i);
                if src_exotic {
                    // Publish before the next getter can collect or throw,
                    // leaving the species result reachable with a partial copy.
                    let deleted =
                        crate::value::js_nanbox_get_pointer(deleted_handle.get_nanbox_f64())
                            as *mut ArrayHeader;
                    note_array_slot(deleted, i, value.to_bits());
                } else {
                    // GC_STORE_AUDIT(BARRIERED): no source callbacks; layout/barrier rebuild follows the copy.
                    let deleted =
                        crate::value::js_nanbox_get_pointer(deleted_handle.get_nanbox_f64())
                            as *mut ArrayHeader;
                    let deleted_elements =
                        crate::array::array_elements_ptr(deleted as *const ArrayHeader) as *mut f64;
                    ptr::write(deleted_elements.add(i), value);
                }
            }
            let deleted = crate::value::js_nanbox_get_pointer(deleted_handle.get_nanbox_f64())
                as *mut ArrayHeader;
            rebuild_array_layout(deleted);
        } else {
            // Step 12: CreateDataPropertyOrThrow per present index, then
            // step 13: Set(A, "length", actualDeleteCount, true).
            for i in 0..actual_delete as usize {
                let value = spec_read(i);
                if value.to_bits() == crate::value::TAG_HOLE {
                    continue; // absent → no property created in the result
                }
                removed_value_handle.set_nanbox_f64(value);
                crate::array::species::species_result_define(
                    deleted_handle.get_nanbox_f64(),
                    i,
                    removed_value_handle.get_nanbox_f64(),
                );
            }
            crate::array::species::species_result_set_length(
                deleted_handle.get_nanbox_f64(),
                actual_delete as usize,
            );
        }

        // A frozen array has non-writable elements and `length`: the moves,
        // the inserted items and the final `Set(O, "length", …, true)` all
        // fail with `Throw = true` (ECMA-262 §23.1.3.31 steps 18-24), even for
        // `splice(0, 0)`. Throw before the receiver is touched.
        if arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| super::array_is_frozen(arr)) {
            crate::collection_iter::throw_type_error("Cannot mutate a frozen array");
        }

        // Calculate new length
        let new_len = len as u32 - actual_delete + items_count;

        // Grow array if needed
        let should_grow =
            arr_handle.with_mut_ptr::<ArrayHeader, _>(|current| new_len > (*current).capacity);
        if should_grow {
            let grown = arr_handle
                .with_mut_ptr::<ArrayHeader, _>(|current| js_array_grow(current, new_len));
            arr_handle.set_raw_mut_ptr(grown);
        }
        arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
            let flags = array_object_flags_resolved(arr);
            let elements_ptr =
                crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut f64;

            // EK §3.1 `splice`: the prefix before the move; the exact rule is
            // installed once the new length is published below.
            let ek_prefix = (*arr).ek_prefix;

            // Shift elements after the splice point
            let tail_start = start_idx + actual_delete;
            let tail_len = len as u32 - tail_start;

            if items_count != actual_delete && tail_len > 0 {
                // Need to shift the tail
                let src = elements_ptr.add(tail_start as usize);
                let dst = elements_ptr.add((start_idx + items_count) as usize);
                // GC_STORE_AUDIT(BARRIERED): the dense-move finisher translates
                // survivor dirty pages below.
                ptr::copy(src, dst, tail_len as usize);
            }

            // Insert new items
            if items_count > 0 && !item_handles.is_empty() {
                for (i, item_handle) in item_handles.iter().enumerate() {
                    let item = item_handle.get_nanbox_f64();
                    // A uniquely-owned string spliced in now aliases the array slot —
                    // demote it to shared so a later `s += x` doesn't mutate it in
                    // place. No-op for SSO / non-string. (This insert path doesn't
                    // funnel through `note_array_slot`.)
                    crate::string::js_string_addref_if_heap_string(item);
                    let item = canonicalize_array_numeric_store_value_from_flags(flags, item);
                    // GC_STORE_AUDIT(BARRIERED): inserted items are covered by the
                    // dense-move finisher below.
                    ptr::write(elements_ptr.add(start_idx as usize + i), item);
                }
            }

            // [0, s) is untouched whatever happens next: a throw below leaves
            // the moved contents under the old length.
            crate::array::ek_lower_prefix(arr, start_idx);
            // ECMA-262 §23.1.3.31 step 24: Set(O, "length", …, true) — throws on a
            // non-writable `length` (test262 splice/S15.4.4.12_A6.1_T2/T3).
            super::push_pop::guard_writable_length(arr);
            if new_len < len as u32 {
                // The tail moved down: its old top slots are vacated.
                super::array_truncate_length(arr, new_len);
            } else {
                (*arr).length = new_len;
            }
            // The rule assumes [s, s+k) holds the k items; a call without an
            // item buffer writes none, and only [0, s) is then known.
            let proved = if items_count == 0 || !item_handles.is_empty() {
                crate::array::ek_prefix_after_splice(
                    ek_prefix,
                    start_idx,
                    actual_delete,
                    items_count,
                )
            } else {
                ek_prefix.min(start_idx)
            };
            crate::array::ek_set_prefix(arr, ek_prefix, proved);
            crate::array::ek_debug_verify(arr);
            let moved_count = if items_count != actual_delete {
                tail_len as usize
            } else {
                0
            };
            finish_array_dense_move_layout(
                arr,
                elements_ptr.add(tail_start as usize).cast(),
                elements_ptr.add((start_idx + items_count) as usize).cast(),
                moved_count,
                elements_ptr.add(start_idx as usize).cast(),
                items_count as usize,
            );

            // Return modified array via out param
            *out_arr = arr;
        });

        crate::value::js_nanbox_get_pointer(deleted_handle.get_nanbox_f64()) as *mut ArrayHeader
    }
}

/// Give a sparse Array (`length > capacity`) a store that covers its length,
/// so that the raw-slot `splice` body may run on it: the present far elements
/// move from the named layout into their slots (`js_array_grow` adopts them),
/// every other new slot is a hole. The receiver keeps its identity through
/// the growth's forwarding. A length the allocator cannot back throws
/// `RangeError: Invalid array length` and leaves the receiver as it was.
///
/// The growth allocates, so the caller's items are rooted across it and
/// written back to the caller's buffer.
#[cold]
#[inline(never)]
unsafe fn densify_sparse_receiver(
    arr: *mut ArrayHeader,
    items: *const f64,
    items_count: u32,
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let item_handles = if items.is_null() || items_count == 0 {
        Vec::new()
    } else {
        scope.root_nanbox_f64_slice(std::slice::from_raw_parts(items, items_count as usize))
    };
    let dense = js_array_grow(arr, (*arr).length);
    for (i, handle) in item_handles.iter().enumerate() {
        // The buffer is caller-owned raw storage a moving collection cannot
        // rewrite: hand the (possibly moved) values back.
        ptr::write((items as *mut f64).add(i), handle.get_nanbox_f64());
    }
    dense
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
    if crate::typedarray_hook_slots::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray_hook_slots::js_typed_array_slice(
            arr as *const crate::typedarray_hook_slots::TypedArrayHeader,
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
        if !crate::array::species::default_species_is_certain(recv_value) {
            // An own `constructor`, a recorded prototype or a `constructor`
            // accessor: the resolution can run user code, so the receiver is
            // rooted there and the result may be any object.
            return slice_with_species(arr, start_idx, slice_len);
        }
        // The default species: ArrayCreate(count), filled by slot writes.
        let array = crate::array::species::default_species_array(slice_len as usize, 0);

        // Copy elements — use spec-generic Get when the source is exotic
        // (has Array.prototype or Object.prototype indexed properties) so
        // inherited indices appear in the result just as [[Get]] would return
        // them (ECMA-262 §23.1.3.25 step 8b "If HasProperty(O, from)…").
        let src_elements =
            crate::array::array_elements_ptr(arr as *const ArrayHeader) as *const f64;
        let src_exotic = crate::array::array_iteration_is_exotic(arr);
        (*array).length = slice_len;
        let dst_elements =
            crate::array::array_elements_ptr(array as *const ArrayHeader) as *mut f64;
        for i in 0..slice_len as usize {
            let src_idx = start_idx as usize + i;
            let v = if src_exotic {
                if crate::array::array_spec_has_index(arr, src_idx as u32) {
                    crate::array::array_spec_get(arr, src_idx as u32)
                } else {
                    f64::from_bits(crate::value::TAG_HOLE)
                }
            } else {
                ptr::read(src_elements.add(src_idx))
            };
            if src_exotic {
                // Publish before the next getter can collect or throw,
                // leaving the species result reachable with a partial copy.
                note_array_slot(array, i, v.to_bits());
            } else {
                // GC_STORE_AUDIT(BARRIERED): raw source reads cannot call out; rebuild follows the copy.
                ptr::write(dst_elements.add(i), v);
            }
        }
        rebuild_array_layout(array);

        array
    }
}

/// `slice` for a receiver whose species needs the full resolution
/// (ECMA-262 §23.1.3.28 steps 8-15): `ArraySpeciesCreate(O, count)`, then per
/// present index `CreateDataPropertyOrThrow(A, n, Get(O, k))`, then
/// `Set(A, "length", count, true)`. The constructor and the getters it reads
/// can collect, so the receiver and the result are rooted and every element is
/// read through the receiver's live address.
#[cold]
#[inline(never)]
unsafe fn slice_with_species(
    arr: *const ArrayHeader,
    start_idx: u32,
    slice_len: u32,
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr as *mut ArrayHeader);
    let recv_value = arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
        f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
    });
    let (result_box, is_default) =
        crate::array::species::array_species_create(recv_value, slice_len as usize);
    let result_handle = scope.root_nanbox_f64(result_box);
    let value_handle = scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED));
    // The species constructor may have shrunk the receiver: a slot at or past
    // its live length or capacity is absent unless a prototype supplies it.
    let read = |i: usize| -> Option<f64> {
        arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
            let index = start_idx + i as u32;
            if index < (*arr).length && index < (*arr).capacity {
                let elements =
                    crate::array::array_elements_ptr(arr as *const ArrayHeader) as *const f64;
                let v = ptr::read(elements.add(index as usize));
                if v.to_bits() != crate::value::TAG_HOLE {
                    return Some(v);
                }
            }
            if crate::array::array_spec_has_index(arr, index) {
                Some(crate::array::array_spec_get(arr, index))
            } else {
                None
            }
        })
    };
    for i in 0..slice_len as usize {
        let Some(v) = read(i) else {
            continue; // absent → no property created in the result
        };
        if is_default {
            let array = crate::value::js_nanbox_get_pointer(result_handle.get_nanbox_f64())
                as *mut ArrayHeader;
            // Publish before the next getter can collect or throw.
            note_array_slot(array, i, v.to_bits());
        } else {
            value_handle.set_nanbox_f64(v);
            crate::array::species::species_result_define(
                result_handle.get_nanbox_f64(),
                i,
                value_handle.get_nanbox_f64(),
            );
        }
    }
    if is_default {
        let array =
            crate::value::js_nanbox_get_pointer(result_handle.get_nanbox_f64()) as *mut ArrayHeader;
        rebuild_array_layout(array);
        array
    } else {
        crate::array::species::species_result_set_length(
            result_handle.get_nanbox_f64(),
            slice_len as usize,
        );
        crate::value::js_nanbox_get_pointer(result_handle.get_nanbox_f64()) as *mut ArrayHeader
    }
}
