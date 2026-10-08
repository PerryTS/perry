//! User-assigned own properties on `Error` objects (`err.code = ...`), kept beside the error's diagnostics.

use super::*;

unsafe fn error_user_prop_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

/// Record a user-assigned own property on an `Error` object. `error_ptr` is
/// the `ErrorHeader` pointer (the NaN-box pointer payload). Called from the
/// `GC_TYPE_ERROR` branch of `js_object_set_field_by_name`.
pub fn set_error_user_prop(error_ptr: usize, key: &str, value: f64) {
    if error_ptr == 0 {
        return;
    }
    // #6759 phase 1: the property bag now hangs off the error's own metadata
    // record instead of a table keyed by its address, so it moves with the
    // error, dies with it, and cannot be inherited by a later tenant of a
    // recycled address. Insertion order comes free from the bag object's
    // `keys_array`.
    unsafe {
        let Some(bag) = crate::object::cell_expando_ensure(error_ptr) else {
            return;
        };
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::js_object_set_field_by_name(bag, key_ptr, value);
    }
}

/// Look up a user-assigned own property on an `Error` object, materialising it
/// back into a NaN-boxed `f64`. Returns `None` if no such property was set.
/// Called from the `GC_TYPE_ERROR` branch of `js_object_get_field_by_name`.
pub fn error_user_prop(error_ptr: usize, key: &str) -> Option<f64> {
    if error_ptr == 0 {
        return None;
    }
    unsafe {
        let bag = crate::object::cell_expando_get(error_ptr)?;
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        // Distinguish "absent" from "present and undefined": a bare get would
        // return `undefined` for both, and the caller uses `None` to mean the
        // error has no such own property at all.
        let key_boxed = f64::from_bits(crate::js_nanbox_string(key_ptr as i64).to_bits());
        if !crate::object::obj_value_has_own_key(
            crate::value::js_nanbox_pointer(bag as i64),
            key_boxed,
        ) {
            return None;
        }
        Some(f64::from_bits(
            crate::object::js_object_get_field_by_name(bag, key_ptr).bits(),
        ))
    }
}

/// Return user-assigned own properties on an Error object as materialized JS
/// values so util.inspect/console formatting can show them.
pub fn error_user_props(error_ptr: usize) -> Vec<(String, f64)> {
    if error_ptr == 0 {
        return Vec::new();
    }
    unsafe {
        let Some(bag) = crate::object::cell_expando_get(error_ptr) else {
            return Vec::new();
        };
        // The bag is an ordinary object, so its `keys_array` already holds the
        // keys in ECMA-262 insertion order — no sort, and no ordering of our
        // own to keep in step with node's.
        let keys_view = crate::object::object_keys(bag);
        let keys = keys_view.arr();
        if keys.is_null() {
            return Vec::new();
        }
        let len = keys_view.count() as usize;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let key_val = crate::array::js_array_get_f64(keys, i as u32);
            // A tombstoned key slot (#9029) reads back as undefined through
            // the hole canonicalization (#323); stringifying it would mint a
            // phantom "undefined" prop. Undefined is never a legal key.
            if key_val.to_bits() == crate::value::TAG_UNDEFINED {
                continue;
            }
            let name_ptr = crate::value::js_jsvalue_to_string(key_val);
            if name_ptr.is_null() {
                continue;
            }
            let name = error_user_prop_string(f64::from_bits(
                crate::js_nanbox_string(name_ptr as i64).to_bits(),
            ));
            let value =
                f64::from_bits(crate::object::js_object_get_field_by_name(bag, name_ptr).bits());
            out.push((name, value));
        }
        out
    }
}
