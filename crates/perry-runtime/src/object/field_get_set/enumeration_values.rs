/// Returns an array of the object's field values
#[no_mangle]
pub extern "C" fn js_object_values(obj: *const ObjectHeader) -> *mut ArrayHeader {
    // An elements-backed Array-subclass instance: its present elements come
    // first (ascending), then the shape's own enumerable properties.
    if unsafe { crate::array::subclass_elements::backed(obj as usize) }.is_some() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_h = scope.root_raw_const_ptr(obj);
        let (shape_part, obj) =
            obj_h.across_const::<ObjectHeader, _>(|| js_object_values_shape(obj));
        if let Some((_, elements)) =
            unsafe { crate::array::subclass_elements::backed(obj as usize) }
        {
            return unsafe {
                crate::array::subclass_elements::prepend_index_values(elements, shape_part)
            };
        }
        return shape_part;
    }
    js_object_values_shape(obj)
}

/// [`js_object_values`] over the shape alone.
fn js_object_values_shape(obj: *const ObjectHeader) -> *mut ArrayHeader {
    // #8149: a registered BUFFER receiver — node `Buffer`, `Uint8Array`,
    // `ArrayBuffer`, `SharedArrayBuffer` or `DataView`. Asked FIRST, above the
    // `is_valid_obj_ptr` guard: a `BufferHeader` is not an `ObjectHeader`, and
    // the generic walk below reads its payload bytes as `keys_array` — `[]`
    // when they are zero, SIGBUS in `js_array_length` when they are not.
    // See `registered_buffer_own_keys`.
    if let Some(result) = registered_buffer_enum(strip_nanbox_addr(obj), MapSetEnum::Values) {
        return result;
    }
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    // Map/Set receiver → no own enumerable properties; see the matching
    // guard in `js_object_keys` for the rationale.
    if crate::map::is_registered_map(stripped as usize)
        || crate::set::is_registered_set(stripped as usize)
    {
        return map_set_exotic_enum(stripped, MapSetEnum::Values);
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_values(
                addr as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    if crate::typedarray::lookup_typed_array_kind(stripped as usize).is_some() {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_values(
                stripped as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    // Arrays: emit each present (non-hole) element value, then enumerable named
    // properties. `js_object_values` has no `ArrayHeader` layout, so the generic
    // object path below would read an array's body as object fields and crash;
    // handle arrays explicitly (mirrors the `js_object_keys` array branch).
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = crate::array::clean_arr_ptr(stripped as *const crate::array::ArrayHeader);
                let length = (*arr).length;
                if length > 100_000 {
                    return crate::array::js_array_alloc(0);
                }
                let elements =
                    crate::array::array_elements_ptr(arr as *const crate::array::ArrayHeader)
                        as *const u64;
                let result = crate::array::js_array_alloc(length);
                for i in 0..length {
                    if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                        continue;
                    }
                    let v = crate::array::js_array_get(arr, i);
                    crate::array::js_array_push_f64(result, f64::from_bits(v.bits()));
                }
                for name in crate::array::array_named_property_enumeration_names(arr, true, length)
                {
                    if let Some(v) = crate::array::array_named_property_get_by_name(arr, &name) {
                        crate::array::js_array_push_f64(result, v);
                    }
                }
                return result;
            }
        }
    }
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893: defensive sibling of `js_object_entries` —
        // see that function's comment for the rationale.
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        if let Some(result) = super::super::string_wrapper::enumerate(
            obj,
            super::super::string_wrapper::Enumeration::Values,
        ) {
            return result;
        }
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let Some(result) = native_module_enum(obj, MapSetEnum::Values) {
                return result;
            }
        }
        // Iterate up to keys_len (logical property count), not
        // field_count — same fix as Object.entries above. Without
        // this, objects with overflow fields silently returned only
        // their first 8 values.
        let keys_view = crate::object::object_keys(obj);
        let keys = keys_view.arr();
        let count = if !keys.is_null() {
            keys_view.count() as usize
        } else {
            crate::object::object_live_slot_count(obj) as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        // #2438: walk slots in OrdinaryOwnPropertyKeys order so values line up
        // with the spec key order (and with `Object.keys`/`Object.entries`).
        let order = ecma_own_key_order(keys_view);
        let pos = |j: usize| -> u32 {
            match &order {
                Some(ord) => ord[j],
                None => j as u32,
            }
        };
        // Snapshot the own key list before reading values, then read each
        // through the name-keyed `[[Get]]` so own accessors fire and getter side
        // effects don't perturb the key set (mirrors `js_object_entries`).
        //
        // Two correctness requirements drive this shape:
        //   * GC safety — a getter fired by `js_object_get_field_by_name` can
        //     delete a future key and allocate/GC before we visit it. A key kept
        //     only as a NaN-boxed pointer inside this Rust-heap `Vec` is not a
        //     stack-visible GC root, so it could dangle. We snapshot the owned
        //     key *bytes* and rematerialize the string at read time instead.
        //   * EnumerableOwnProperties — enumerability is determined per key at
        //     read time, not cached up front: an earlier getter can create a
        //     descriptor or flip a future key's enumerability, so we defer the
        //     `descriptor_marks_non_enumerable` check to the read phase.
        // The shape answers for every key at once: an own key is hidden
        // only when the receiver is a class instance or its shape has a
        // private entry (#11791), and no key of this snapshot becomes one.
        let hide_private = own_keys_may_hide(obj);
        let mut snapshot_keys: Vec<Vec<u8>> = Vec::with_capacity(count);
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for j in 0..count {
            let i = pos(j);
            if keys.is_null() || i >= keys_view.count() {
                continue;
            }
            let key_val = keys_view.get(i);
            if hide_private && own_slot_hidden(obj, i, key_val) {
                continue;
            }
            if let Some(bytes) = crate::string::js_string_key_bytes(key_val, &mut key_buf) {
                snapshot_keys.push(bytes.to_vec());
            }
        }
        for key_bytes in snapshot_keys {
            let key_str =
                crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
            if key_str.is_null() {
                continue;
            }
            // Re-check own + enumerable at read time (a prior getter may have
            // removed/hidden the key, or created a descriptor) — see
            // `js_object_entries`.
            if !super::super::own_key_present(obj as *mut ObjectHeader, key_str) {
                continue;
            }
            if descriptor_marks_non_enumerable(obj, JSValue::string_ptr(key_str)) {
                continue;
            }
            let value = js_object_get_field_by_name(obj as *const ObjectHeader, key_str);
            crate::array::js_array_push_f64(result, f64::from_bits(value.bits()));
        }

        result
    }
}

/// Get the entries of an object as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_object_entries(obj: *const ObjectHeader) -> *mut ArrayHeader {
    // An elements-backed Array-subclass instance: its present elements come
    // first (ascending), then the shape's own enumerable properties.
    if unsafe { crate::array::subclass_elements::backed(obj as usize) }.is_some() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_h = scope.root_raw_const_ptr(obj);
        let (shape_part, obj) =
            obj_h.across_const::<ObjectHeader, _>(|| js_object_entries_shape(obj));
        if let Some((_, elements)) =
            unsafe { crate::array::subclass_elements::backed(obj as usize) }
        {
            return unsafe {
                crate::array::subclass_elements::prepend_index_entries(elements, shape_part)
            };
        }
        return shape_part;
    }
    js_object_entries_shape(obj)
}

use super::entries_shape::js_object_entries_shape;
