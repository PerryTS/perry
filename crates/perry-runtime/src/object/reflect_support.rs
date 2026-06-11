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
        // Typed arrays use a side table (small ones carry no `GcHeader`, so
        // the header read below would be allocator-metadata garbage).
        if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
            return crate::typedarray_props::typed_array_owner_no_extend(obj as usize);
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
        // TypedArray FIRST: own keys are the valid integer indices plus the
        // expando side table. Must precede the GC-header read below — small
        // typed arrays are plain-`alloc`ed without a `GcHeader`, so reading
        // `addr - 8` is allocator-metadata garbage.
        if crate::typedarray::lookup_typed_array_kind(obj_addr).is_some() {
            let key_str = crate::builtins::js_string_coerce(key);
            if key_str.is_null() {
                return false;
            }
            return crate::typedarray_props::typed_array_has_own_property(
                obj as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
        }
        if obj_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc = gc_header_for(obj);
            if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
                || (*gc).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                let arr = crate::array::clean_arr_ptr(obj as *const crate::array::ArrayHeader);
                if arr.is_null() {
                    return false;
                }
                let key_str = crate::builtins::js_string_coerce(key);
                if key_str.is_null() {
                    return false;
                }
                return super::has_own_helpers::array_own_key_present(arr, key_str);
            }
        }
        if crate::closure::is_closure_ptr(obj_addr) {
            let Some(key_name) = key_to_rust_string(key) else {
                return false;
            };
            return super::has_own_helpers::closure_own_key_present(obj_addr, &key_name);
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

#[inline]
fn reflect_bool(b: bool) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(b).bits())
}

/// Ordinary (non-proxy) `Reflect.defineProperty` `[[DefineOwnProperty]]`,
/// reporting success as a NaN-boxed boolean. Shared by `crate::proxy`'s
/// `Reflect.defineProperty` entry point (both the no-trap and direct paths).
pub(crate) fn reflect_define_property(obj: f64, key: f64, descriptor: f64) -> f64 {
    // TypedArrays are Integer-Indexed exotic objects: a canonical numeric index
    // key returns true/false here rather than going through the ordinary object
    // machinery (which would mishandle in-bounds element writes and treats the
    // view as non-extensible).
    match unsafe { super::typed_array_define_own_property(obj, key, descriptor) } {
        super::TypedArrayDefineOutcome::Defined => return reflect_bool(true),
        super::TypedArrayDefineOutcome::Rejected => return reflect_bool(false),
        super::TypedArrayDefineOutcome::NotTypedArray => {}
    }
    // The array exotic `[[DefineOwnProperty]]` for `length` (ArraySetLength)
    // reports success/failure as a boolean here rather than throwing — bypass
    // the generic non-configurable pre-check below, which would mishandle the
    // (non-configurable but writable) `length` property.
    if let Some(ok) = unsafe { super::array_length_reflect_define(obj, key, descriptor) } {
        return reflect_bool(ok);
    }
    let has_own = obj_value_has_own_key(obj, key);
    // Redefining a non-configurable existing property fails.
    if has_own {
        if let Some((_writable, configurable)) = obj_value_attrs(obj, key) {
            if !configurable {
                return reflect_bool(false);
            }
        }
    } else if obj_value_no_extend(obj) {
        // Defining a brand-new property on a non-extensible object fails.
        return reflect_bool(false);
    }
    super::js_object_define_property(obj, key, descriptor);
    reflect_bool(true)
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
