//! `delete obj.x` and object-rest (`{...rest}`) semantics:
//! `js_object_delete_field`, `js_object_delete_dynamic`, `js_object_rest`.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation.

use super::*;

/// Delete a field from an object by its string key name
/// Returns 1 if the field was deleted (or didn't exist), 0 otherwise
/// Note: In strict mode, this would return 0 for non-configurable properties,
/// but we don't track configurability, so we always return 1.
#[no_mangle]
pub extern "C" fn js_object_delete_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> i32 {
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            // No keys array means no fields to delete, but delete "succeeds" vacuously
            return 1;
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let mut found_idx: Option<usize> = None;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        let i = match found_idx {
            Some(i) => i,
            None => return 1, // Not found — delete succeeds vacuously
        };

        // Proper delete: shift remaining keys + values down by one, then
        // shorten keys_array. Pre-fix this just set the value to
        // undefined and left the key in place, so `Object.keys`,
        // `Object.entries`, `for-in` etc. all still saw the deleted
        // property. Bun and Node remove the property entirely; we
        // match that.
        let field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(field_count as usize, 8);
        let new_count = key_count - 1;

        // CRITICAL: clone the keys_array before mutating it. The same
        // keys_array is shared across all objects that built the same
        // shape via `transition_cache_lookup`-hit fast paths. Without
        // cloning, mutating its length / contents to remove the deleted
        // key would corrupt every other object that picks up this
        // shape — they'd silently lose entries they never deleted.
        let keys_cloned = crate::array::js_array_alloc(new_count.max(1) as u32 + 4);
        let src_elements =
            (keys as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
        let dst_elements =
            (keys_cloned as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
        // Copy keys [0..i) ++ [i+1..N) into [0..new_count).
        for j in 0..i {
            *dst_elements.add(j) = *src_elements.add(j);
        }
        for j in i..new_count {
            *dst_elements.add(j) = *src_elements.add(j + 1);
        }
        (*keys_cloned).length = new_count as u32;
        super::rebuild_array_layout_from_slots(keys_cloned);
        set_object_keys_array(obj, keys_cloned);

        // 1) Shift values down: for slot j in i..new_count, copy slot j+1
        //    into slot j. Inline reads/writes for j < alloc_limit;
        //    overflow_get/set otherwise.
        for j in i..new_count {
            let next = js_object_get_field(obj, (j + 1) as u32);
            // Inline write if target slot < alloc_limit, else overflow.
            if j < alloc_limit {
                let fields_ptr =
                    (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                ptr::write(fields_ptr.add(j), next);
                super::note_object_field_slot(obj, j, next.bits());
            } else {
                overflow_set(obj as usize, j, next.bits());
            }
        }
        // Clear the now-tail slot so reads past keys_array.length see undefined.
        if new_count < alloc_limit {
            let fields_ptr =
                (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
            ptr::write(fields_ptr.add(new_count), JSValue::undefined());
            super::note_object_field_slot(obj, new_count, crate::value::TAG_UNDEFINED);
        } else {
            overflow_set(obj as usize, new_count, crate::value::TAG_UNDEFINED);
        }

        // 2) (Keys already shifted into the cloned keys_array above —
        //    we built the new keys directly with the deleted entry
        //    omitted, so no in-place shift is needed.)

        // 3) Adjust field_count: keep within bounds. If the original
        //    field_count counted this slot, drop by one.
        if (i as u32) < field_count {
            (*obj).field_count = field_count - 1;
        }

        // 4) Invalidate the keys-index sidecar for this object — the
        //    slot map is now stale (entries past `i` have shifted).
        //    The next lookup at threshold will rebuild from current
        //    keys_array.
        KEYS_INDEX.with(|m| {
            m.borrow_mut().remove(&(obj as usize));
        });

        1
    }
}

/// Delete a field from an object using a dynamic key (could be string or number index)
/// For arrays, this sets the element to undefined
/// Returns 1 if successful, 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_dynamic(obj: *mut ObjectHeader, key: f64) -> i32 {
    let key_val = JSValue::from_bits(key.to_bits());

    // If the key is a string, use js_object_delete_field
    if key_val.is_string() {
        let key_str = key_val.as_string_ptr();
        return js_object_delete_field(obj, key_str);
    }

    // If the key is a number, treat as array index
    if key_val.is_number() {
        let index = key_val.as_number() as usize;
        // Try to treat it as an array and set the element to undefined
        // This is a simplified implementation - real JS delete on arrays
        // creates a hole (sparse array), but we just set to undefined
        let arr = obj as *mut crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr) as usize;
        if index < len {
            crate::array::js_array_set(arr, index as u32, JSValue::undefined());
            return 1;
        }
    }

    // For other types, delete succeeds vacuously
    1
}

/// Create a rest object from destructuring: copies all properties from src except excluded keys.
/// exclude_keys is an array of NaN-boxed string pointers (the explicitly destructured keys).
/// Returns a pointer to a new object with the remaining key-value pairs.
#[no_mangle]
pub extern "C" fn js_object_rest(
    src: *const ObjectHeader,
    exclude_keys: *const ArrayHeader,
) -> *mut ObjectHeader {
    if src.is_null() {
        return js_object_alloc(0, 0);
    }
    unsafe {
        let keys = (*src).keys_array;
        if keys.is_null() {
            return js_object_alloc(0, 0);
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        let exclude_count = if exclude_keys.is_null() {
            0
        } else {
            crate::array::js_array_length(exclude_keys) as usize
        };

        // Collect indices of keys to include (not in exclude list and not undefined/deleted)
        let mut include_indices: Vec<usize> = Vec::new();
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() {
                continue;
            }
            let key_str = key_val.as_string_ptr();

            // Check if field was deleted
            let field_val = js_object_get_field(src, i as u32);
            if field_val.is_undefined() {
                continue;
            }

            // Check if this key is in the exclude list
            let mut excluded = false;
            for j in 0..exclude_count {
                let ex_val = crate::array::js_array_get(exclude_keys, j as u32);
                if ex_val.is_string() {
                    let ex_str = ex_val.as_string_ptr();
                    if crate::string::js_string_equals(key_str, ex_str) != 0 {
                        excluded = true;
                        break;
                    }
                }
            }
            if !excluded {
                include_indices.push(i);
            }
        }

        // Allocate new object with the right number of fields
        let rest_count = include_indices.len() as u32;
        let rest_obj = js_object_alloc(0, rest_count);

        // Create keys array for the rest object
        let rest_keys = crate::array::js_array_alloc_with_length(rest_count);
        (*rest_obj).keys_array = rest_keys;

        // Copy included key-value pairs
        for (new_idx, &src_idx) in include_indices.iter().enumerate() {
            let key_val = crate::array::js_array_get(keys, src_idx as u32);
            crate::array::js_array_set(rest_keys, new_idx as u32, key_val);

            let field_val = js_object_get_field(src, src_idx as u32);
            js_object_set_field(rest_obj, new_idx as u32, field_val);
        }

        rest_obj
    }
}
