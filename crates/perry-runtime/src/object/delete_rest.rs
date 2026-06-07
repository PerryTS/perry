//! `delete obj.x` and object-rest (`{...rest}`) semantics:
//! `js_object_delete_field`, `js_object_delete_dynamic`, `js_object_rest`.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation.

use super::*;

/// Delete a field from an object by its string key name
/// Returns 1 if the field was deleted (or didn't exist), 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> i32 {
    if obj.is_null() || key.is_null() {
        return 1;
    }
    // A Proxy is a small registered id in [0xF0000, 0x100000), not a heap
    // ObjectHeader. Dereferencing it below (GC header / keys_array reads) would
    // segfault. Route `delete proxy.k` / `delete proxy[k]` through the proxy
    // `deleteProperty` trap. (#2846-family Proxy crash cluster.)
    {
        let addr = obj as u64;
        if (0xF0000..0x100000).contains(&addr) {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                let r = crate::proxy::js_proxy_delete(boxed, key_f64);
                return if crate::value::js_is_truthy(r) != 0 {
                    1
                } else {
                    0
                };
            }
        }
    }
    if (obj as usize) < 0x10000 {
        unsafe {
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                let class_id = obj as usize as u32;
                if super::class_registry::class_name_for_id(class_id).is_some() {
                    super::class_registry::class_delete_own_dynamic_prop(class_id, name);
                    super::class_registry::class_mark_key_deleted(class_id, name);
                }
            }
        }
        return 1;
    }
    unsafe {
        if let Some(addr) =
            crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
        {
            return crate::typedarray_props::typed_array_delete_own_property(
                addr as *mut crate::typedarray::TypedArrayHeader,
                key,
            );
        }
        if let Some(result) = super::arguments_object_before_delete(obj, key) {
            return result;
        }
        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                    if let Some(attrs) = get_property_attrs(obj as usize, name) {
                        if !attrs.configurable() {
                            return 0;
                        }
                    }
                    if let Some(index) = super::canonical_array_index(name) {
                        return crate::array::js_array_delete(
                            obj as *mut crate::array::ArrayHeader,
                            index,
                        );
                    }
                }
                crate::array::array_named_property_delete(
                    obj as *const crate::array::ArrayHeader,
                    key,
                );
                return 1;
            }
        }
        // #3655: `delete fn.name` / `delete fn.userProp`. Functions/closures
        // aren't `ObjectHeader`s — reading `keys_array` off one is out of
        // bounds. The built-in `name`/`length` slots are `configurable:true`,
        // so a delete records the key in the closure deleted-key side table
        // (consulted by hasOwnProperty/getOwnProperty*/value reads);
        // user-attached props are dropped from the dynamic-prop table outright.
        if crate::closure::is_closure_ptr(obj as usize) {
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                // A non-configurable slot — e.g. a constructor's `prototype`,
                // which #3655 registers as `{configurable:false}` — can't be
                // deleted: leave it intact and report failure (strict mode
                // throws on the `false` return; sloppy mode no-ops).
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.configurable() {
                        return 0;
                    }
                }
                crate::closure::closure_delete_own_dynamic_prop(obj as usize, name);
                crate::closure::closure_mark_key_deleted(obj as usize, name);
            }
            return 1;
        }
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
            // #1781: SSO-aware match — pre-fix `delete obj.id` on an
            // object whose `id` lived as an inline SSO key reported
            // success vacuously without actually deleting anything.
            if crate::string::js_string_key_matches(key_val, key) {
                found_idx = Some(i);
                break;
            }
        }

        let i = match found_idx {
            Some(i) => i,
            None => return 1, // Not found — delete succeeds vacuously
        };
        let key_name = {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()
        };
        if let Some(name) = key_name {
            if let Some(attrs) = get_property_attrs(obj as usize, name) {
                if !attrs.configurable() {
                    return 0;
                }
            }
        }

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
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
            *dst_elements.add(j) = *src_elements.add(j);
        }
        for j in i..new_count {
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
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
                let slot = fields_ptr.add(j);
                crate::gc::runtime_store_jsvalue_slot(obj as usize, slot as usize, j, next.bits());
            } else {
                overflow_set(obj as usize, j, next.bits());
            }
        }
        // Clear the now-tail slot so reads past keys_array.length see undefined.
        if new_count < alloc_limit {
            let fields_ptr =
                (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
            let slot = fields_ptr.add(new_count);
            crate::gc::runtime_store_jsvalue_slot(
                obj as usize,
                slot as usize,
                new_count,
                crate::value::TAG_UNDEFINED,
            );
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
/// Returns 1 if successful, 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_dynamic(obj: *mut ObjectHeader, key: f64) -> i32 {
    // Proxy receiver (small registered id) — route through the proxy
    // `deleteProperty` trap before any key coercion that would deref the fake
    // pointer. Handles symbol keys too (the string path also funnels into
    // `js_object_delete_field`, which has its own guard).
    {
        let addr = obj as u64;
        if (0xF0000..0x100000).contains(&addr) {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let r = crate::proxy::js_proxy_delete(boxed, key);
                return if crate::value::js_is_truthy(r) != 0 {
                    1
                } else {
                    0
                };
            }
        }
    }
    let key_val = JSValue::from_bits(key.to_bits());

    // If the key is a string, use js_object_delete_field. #1781: accept
    // inline SSO short keys — `delete obj["abc"]` for a <=5-char key arrives
    // as a SHORT_STRING_TAG value that is_string() rejects, so the delete
    // silently no-op'd (fell through to "succeeds vacuously"). Materialize
    // the key to a heap header so js_object_delete_field can match it.
    if key_val.is_any_string() {
        let key_str =
            crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
        return js_object_delete_field(obj, key_str);
    }

    let property_key = unsafe { js_to_property_key(key) };
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        // Symbol-keyed delete (`delete obj[Symbol.iterator]`). Previously this
        // fell through to the vacuous `return 1`, so the delete *reported*
        // success while leaving the property in place — `verifyProperty`'s
        // `isConfigurable` (delete-then-hasOwn) then saw the property survive
        // and flagged a configurable symbol property as non-configurable
        // (Test262 `Map.prototype/Symbol.iterator.js`). Route to the symbol
        // property table delete, which honors the configurable attribute.
        let obj_f64 = crate::value::js_nanbox_pointer(obj as i64);
        return unsafe { crate::symbol::js_object_delete_symbol_property(obj_f64, property_key) };
    }
    let key_str = crate::value::js_jsvalue_to_string(property_key);
    if !key_str.is_null() {
        return js_object_delete_field(obj, key_str as *const crate::StringHeader);
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

        // Collect indices of keys to include (not in exclude list and not undefined/deleted).
        // #1781: SSO-aware — the pre-fix `is_string()` on the source
        // key dropped ≤5-byte SSO keys from `rest`; the exclude-loop's
        // `is_string()` similarly missed inline-SSO exclude entries,
        // so a `{a, ...rest}` pattern silently kept `a` in `rest` when
        // both the source key and the exclude key were SSO.
        let mut include_indices: Vec<usize> = Vec::new();
        let mut src_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            let key_bytes = match crate::string::js_string_key_bytes(key_val, &mut src_buf) {
                Some(b) => b.to_vec(),
                None => continue,
            };

            // Check if field was deleted
            let field_val = js_object_get_field(src, i as u32);
            if field_val.is_undefined() {
                continue;
            }

            // Check if this key is in the exclude list
            let mut excluded = false;
            for j in 0..exclude_count {
                let ex_val = crate::array::js_array_get(exclude_keys, j as u32);
                if crate::string::js_string_key_matches_bytes(ex_val, &key_bytes) {
                    excluded = true;
                    break;
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
        set_object_keys_array(rest_obj, rest_keys);

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

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: `delete obj["id"]` for a key <= 5 bytes — the dynamic key
    /// arrives as an inline SSO value that `is_string()` (STRING_TAG-only)
    /// rejected, so the delete silently no-op'd (fell through to "succeeds
    /// vacuously") and the property stayed put.
    #[test]
    fn delete_dynamic_removes_property_via_sso_key() {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 0);
            let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);

            let obj_box = crate::value::js_nanbox_pointer(obj as i64);
            let sso = crate::value::JSValue::try_short_string(b"id").unwrap();
            assert!(sso.is_short_string());
            // present before delete
            assert_ne!(
                crate::value::js_is_truthy(crate::object::js_object_has_property(
                    obj_box,
                    f64::from_bits(sso.bits())
                )),
                0
            );

            let ok = js_object_delete_dynamic(obj, f64::from_bits(sso.bits()));
            assert_eq!(ok, 1, "delete should report success");

            // gone after delete
            assert_eq!(
                crate::value::js_is_truthy(crate::object::js_object_has_property(
                    obj_box,
                    f64::from_bits(sso.bits())
                )),
                0,
                "SSO key should be removed after delete"
            );
        }
    }
}
