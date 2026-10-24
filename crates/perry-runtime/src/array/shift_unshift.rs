/// Shift an element from the beginning of an array.
/// Returns the removed element, or `undefined` if the array is empty (per
/// ECMAScript §23.1.3.27). See the matching note on `js_array_pop_f64` —
/// returning bare `f64::NAN` here was a perry bug that broke the
/// `entry !== undefined` check in connection-pool drivers like
/// `@perryts/mysql`. Issue #536.
#[no_mangle]
pub extern "C" fn js_array_shift_f64(arr: *mut ArrayHeader) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    // Borrowed array-like receiver — see `js_array_pop_f64`.
    if let Some(recv) = crate::array::plain_object_value(arr) {
        return crate::array::generic_object_shift(recv);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    unsafe {
        let length = (*arr).length;
        if length == 0 {
            if array_is_frozen(arr) {
                throw_frozen_array_mutation();
            }
            guard_writable_length(arr);
            return TAG_UNDEFINED_F64;
        }

        // A raw memmove is only equivalent to Shift when every observable
        // indexed operation is an ordinary dense-array access. Indexed
        // descriptors and prototype properties require the specified live
        // HasProperty/Get/Set/Delete order; their accessors can also freeze the
        // receiver or make `length` non-writable before the final length Set.
        if crate::array::array_iteration_is_exotic(arr)
            || (crate::object::prototype_chain::array_static_proto_recorded()
                && array_custom_prototype(arr).is_some())
            || array_object_flags_resolved(arr)
                & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED)
                != 0
        {
            return shift_array_spec_path(arr);
        }
        super::storage::shift_dense(arr)
    }
}

/// ECMA-262 Array.prototype.shift for a real array whose indexed operations
/// are observable. The loop keeps the original length while consulting live
/// presence and values, and roots both the receiver and carried values across
/// accessors which may allocate or move either one.
unsafe fn shift_array_spec_path(arr: *mut ArrayHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let first_handle = scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED));
    let from_value_handle = scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED));
    let len = (*arr).length;

    let (first, _) = arr_handle.across_mut::<ArrayHeader, _>(|| {
        arr_handle.with_mut_ptr(|current| crate::array::array_spec_get(current, 0))
    });
    first_handle.set_nanbox_f64(first);

    for from in 1..len {
        let from_present = arr_handle.with_mut_ptr::<ArrayHeader, _>(|current| {
            crate::array::array_spec_has_index(current, from)
        });
        let to = from - 1;
        if from_present {
            let (value, _) = arr_handle.across_mut::<ArrayHeader, _>(|| {
                arr_handle.with_mut_ptr(|current| crate::array::array_spec_get(current, from))
            });
            from_value_handle.set_nanbox_f64(value);
            shift_array_spec_set(&arr_handle, to, &from_value_handle);
        } else {
            shift_array_spec_delete(&arr_handle, to);
        }
    }

    shift_array_spec_delete(&arr_handle, len - 1);

    // Set(O, "length", len - 1, true) occurs after every indexed operation.
    // Re-read the receiver state because any getter/setter above may have
    // frozen it or replaced `length` with a non-writable descriptor.
    arr_handle.with_mut_ptr::<ArrayHeader, _>(|current| {
        let current = clean_arr_ptr_mut(current);
        if array_is_frozen(current) {
            throw_frozen_array_mutation();
        }
        guard_writable_length(current);
        crate::array::array_truncate_length(current, len - 1);
        rebuild_array_layout(current);
    });
    first_handle.get_nanbox_f64()
}

fn shift_array_spec_set(
    arr_handle: &crate::gc::RuntimeHandle<'_>,
    index: u32,
    value_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let (next, post_gc) = arr_handle.across_mut::<ArrayHeader, _>(|| {
        let value = value_handle.get_nanbox_f64();
        // The mutators specify `Set(O, …, true)` regardless of the calling
        // code's strictness (#9394).
        arr_handle.with_mut_ptr(|current| crate::array::array_spec_set(current, index, value, true))
    });
    let next = clean_arr_ptr_mut(next);
    let current = if next.is_null() {
        clean_arr_ptr_mut(post_gc)
    } else {
        next
    };
    arr_handle.set_raw_mut_ptr(current);
}

fn shift_array_spec_delete(arr_handle: &crate::gc::RuntimeHandle<'_>, index: u32) {
    let (deleted, _) = arr_handle.across_mut::<ArrayHeader, _>(|| {
        arr_handle.with_mut_ptr(|current| crate::array::js_array_delete(current, index))
    });
    if deleted == 0 {
        throw_cannot_delete_array_index(index);
    }
}

/// Unshift an element to the beginning of an array, growing if needed
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_unshift_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    // #5552: a uniquely-owned (refcount==1) string unshifted to the front aliases
    // the new slot — demote it to shared so a later `s += x` on the source local
    // allocates fresh instead of mutating the stored element. No-op for SSO /
    // non-string (mirrors `js_array_push_f64`, #5548).
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if crate::array::array_iteration_is_exotic(arr) {
        return unshift_array_spec_path(arr, &[value]);
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    if array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;

        let arr = if length >= capacity {
            js_array_grow(arr, length + 1)
        } else {
            arr
        };
        let flags = array_object_flags_resolved(arr);
        let value =
            canonicalize_array_numeric_store_value_from_flags(flags, value_handle.get_nanbox_f64());

        let elements_ptr = crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut f64;

        // Shift all elements up
        // GC_STORE_AUDIT(BARRIERED): the dense-move finisher translates
        // survivor dirty pages and barriers the inserted slot below.
        ptr::copy(elements_ptr, elements_ptr.add(1), length as usize);
        // Write new element at beginning
        ptr::write(elements_ptr, value);
        (*arr).length = length + 1;
        // EK §3.1 `unshift`: old [0, P) lands at [1, P + 1) and slot 0 holds
        // the value, so the prefix grows by one (descriptor clamp inside).
        let prefix = (*arr).ek_prefix;
        crate::array::ek_set_prefix(arr, prefix, prefix + 1);
        finish_array_dense_move_layout(
            arr,
            elements_ptr.cast(),
            elements_ptr.add(1).cast(),
            length as usize,
            elements_ptr.cast(),
            1,
        );
        arr
    }
}

/// Unshift an element as raw JSValue bits (u64), for object/pointer values
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_unshift_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_unshift_f64(arr, bits_as_f64)
}

/// `arr.unshift(...items)` (#2814) — insert zero or more elements at the front
/// in source order, growing the array if needed. Returns the (possibly
/// reallocated) array header so the caller can read the new length / write the
/// new pointer back. With `count == 0` the array is returned unchanged.
#[no_mangle]
pub extern "C" fn js_array_unshift_variadic(
    arr: *mut ArrayHeader,
    items: *const f64,
    count: u32,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    // `unshift` always performs `Set(O, "length", …)` (even zero-arg), so both
    // a frozen array and a non-writable `length` throw before the no-op early
    // return. Frozen check must come first because freeze doesn't record "length"
    // attrs, so `guard_writable_length` alone wouldn't catch it.
    if count != 0 && crate::array::array_iteration_is_exotic(arr) {
        let values = unsafe {
            if items.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(items, count as usize)
            }
        };
        return unshift_array_spec_path(arr, values);
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    if count == 0 {
        return arr;
    }
    if array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    // The caller-owned alloca itself is stable, but a copying collection can
    // move any pointer values stored in it without rewriting those raw words.
    // Give every item a mutable runtime root before growth can allocate.
    let item_handles = unsafe {
        if items.is_null() {
            Vec::new()
        } else {
            scope.root_nanbox_f64_slice(std::slice::from_raw_parts(items, count as usize))
        }
    };
    let n = item_handles.len();
    let (length, capacity) = arr_handle.with_mut_ptr::<ArrayHeader, _>(|current| unsafe {
        ((*current).length, (*current).capacity)
    });
    if length + n as u32 > capacity {
        let grown = arr_handle
            .with_mut_ptr::<ArrayHeader, _>(|current| js_array_grow(current, length + n as u32));
        arr_handle.set_raw_mut_ptr(grown);
    }
    arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| unsafe {
        let flags = array_object_flags_resolved(arr);
        let elements_ptr = crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut f64;
        // Shift existing elements up by `n`.
        // GC_STORE_AUDIT(BARRIERED): the dense-move finisher translates
        // survivor dirty pages and barriers the inserted slots below.
        ptr::copy(elements_ptr, elements_ptr.add(n), length as usize);
        // Write items in source order at the front. #5552: demote each
        // uniquely-owned string before it aliases its slot (no-op for SSO /
        // non-string).
        for (i, value) in item_handles.iter().enumerate() {
            let v = value.get_nanbox_f64();
            crate::string::js_string_addref_if_heap_string(v);
            let v = canonicalize_array_numeric_store_value_from_flags(flags, v);
            // GC_STORE_AUDIT(BARRIERED): inserted slots are covered by the
            // dense-move finisher below.
            ptr::write(elements_ptr.add(i), v);
        }
        (*arr).length = length + n as u32;
        // EK §3.1 `unshift`: old [0, P) lands at [n, P + n) and [0, n) holds
        // the items, so the prefix grows by n (descriptor clamp inside).
        let prefix = (*arr).ek_prefix;
        crate::array::ek_set_prefix(arr, prefix, prefix + n as u32);
        finish_array_dense_move_layout(
            arr,
            elements_ptr.cast(),
            elements_ptr.add(n).cast(),
            length as usize,
            elements_ptr.cast(),
            n,
        );
        arr
    })
}

fn unshift_array_spec_path(arr: *mut ArrayHeader, items: &[f64]) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let item_handles: Vec<_> = items
        .iter()
        .map(|value| scope.root_nanbox_f64(*value))
        .collect();
    let length = unsafe { (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length };
    let count = u32::try_from(item_handles.len()).unwrap_or_else(|_| {
        crate::array::array_length_range_error();
    });
    let new_length = length.checked_add(count).unwrap_or_else(|| {
        crate::array::array_length_range_error();
    });

    let mut k = length;
    while k > 0 {
        let from = k - 1;
        let to = from + count;
        if crate::array::array_spec_has_index(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), from) {
            let iteration_scope = crate::gc::RuntimeHandleScope::new();
            let value =
                crate::array::array_spec_get(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), from);
            let value_handle = iteration_scope.root_nanbox_f64(value);
            shift_array_spec_set(&arr_handle, to, &value_handle);
        } else {
            shift_array_spec_delete(&arr_handle, to);
        }
        k -= 1;
    }
    for (index, value) in item_handles.iter().enumerate() {
        shift_array_spec_set(&arr_handle, index as u32, value);
    }

    unsafe {
        let current = clean_arr_ptr_mut(arr_handle.get_raw_mut_ptr::<ArrayHeader>());
        arr_handle.set_raw_mut_ptr(current);
        // Getters/setters in the indexed moves may have changed this state.
        if array_is_frozen(current) {
            throw_frozen_array_mutation();
        }
        guard_writable_length(current);
        (*current).length = new_length;
        rebuild_array_layout(current);
        current
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_UNSHIFT_VARIADIC: extern "C" fn(*mut ArrayHeader, *const f64, u32) -> *mut ArrayHeader =
    js_array_unshift_variadic;
