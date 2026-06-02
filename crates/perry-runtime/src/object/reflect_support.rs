//! Reflect-specific support predicates (#2756/#2758/#2760/#2762).
//!
//! These helpers expose just enough of an object's recorded metadata
//! (extensibility flag, own-key presence, per-property writable/configurable
//! attributes) for `crate::proxy`'s `Reflect.*` entry points to compute the
//! correct boolean results that Node returns — without the Reflect code
//! reaching into object internals directly. Split out of `object_ops.rs` to
//! keep that file under the 2000-line lint cap.

use super::object_ops::{extract_obj_ptr, gc_header_for};

/// Is `value` a heap object that codegen would treat as a target? Returns
/// `false` for primitives, null/undefined, class refs, and other non-pointer
/// tags. Used by `Reflect.preventExtensions` / `Reflect.isExtensible` to throw
/// a `TypeError` on non-object targets (whereas the `Object.*` helpers tolerate
/// them).
pub(crate) fn js_value_is_heap_object(value: f64) -> bool {
    unsafe { !extract_obj_ptr(value).is_null() }
}

/// Does the heap object behind `value` currently carry the `OBJ_FLAG_NO_EXTEND`
/// flag? Returns `false` for non-objects.
pub(crate) fn obj_value_no_extend(value: f64) -> bool {
    unsafe {
        let obj = extract_obj_ptr(value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return false;
        }
        let gc = gc_header_for(obj);
        (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0
    }
}

/// Does the heap object behind `value` have an own (string-keyed) property
/// named `key`? Used to distinguish "define a new property on a non-extensible
/// object" (fails) from "redefine an existing one" (may succeed). Symbol keys
/// are resolved through the symbol side-table.
pub(crate) fn obj_value_has_own_key(value: f64, key: f64) -> bool {
    unsafe {
        if crate::symbol::js_is_symbol(key) != 0 {
            let v = crate::symbol::js_object_get_symbol_property(value, key);
            return v.to_bits() != crate::value::TAG_UNDEFINED;
        }
        let obj = extract_obj_ptr(value);
        if obj.is_null() {
            return false;
        }
        let obj_addr = obj as usize;
        if crate::closure::is_closure_ptr(obj_addr) {
            let Some(key_name) = key_to_rust_string(key) else {
                return false;
            };
            return crate::closure::closure_has_own_dynamic_prop(obj_addr, &key_name)
                || matches!(key_name.as_str(), "length" | "name" | "prototype");
        }
        let key_str = crate::builtins::js_string_coerce(key);
        if key_str.is_null() {
            return false;
        }
        let keys = (*obj).keys_array;
        if keys.is_null() || (keys as usize) < 0x10000 {
            return false;
        }
        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let stored = crate::array::js_array_get(keys, i as u32);
            if crate::string::js_string_key_matches(stored, key_str) {
                return true;
            }
        }
        false
    }
}

/// Look up the writable/configurable attributes Perry has recorded for
/// `(value, key)`. Returns `None` when no descriptor has been installed (the JS
/// default of all-true applies). The booleans are `(writable, configurable)`.
pub(crate) fn obj_value_attrs(value: f64, key: f64) -> Option<(bool, bool)> {
    unsafe {
        let obj = extract_obj_ptr(value);
        if obj.is_null() {
            return None;
        }
        let k = key_to_rust_string(key)?;
        super::get_property_attrs(obj as usize, &k).map(|a| (a.writable(), a.configurable()))
    }
}

unsafe fn key_to_rust_string(value: f64) -> Option<String> {
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
        .ok()
        .map(|s| s.to_string())
}
