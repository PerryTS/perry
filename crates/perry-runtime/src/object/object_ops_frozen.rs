//! Object freeze / seal / extensibility helpers, extracted from
//! `object_ops.rs` to keep that file under the 2k-line limit. These map the
//! `Object.freeze` / `Object.seal` / `Object.preventExtensions` family (and
//! their `is*` predicates) onto the per-object GC-header reserved-flag bits.

use super::*;

/// Drop `writable`/`configurable` on every own **symbol-keyed** property of
/// `obj`. The string-keyed table is handled by `mark_all_keys`; symbol props
/// live in a separate side table, so `Object.freeze`/`Object.seal` must walk
/// it too (else a frozen object's symbol props stay writable and strict writes
/// to them wrongly succeed).
unsafe fn mark_all_symbol_keys(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    drop_configurable: bool,
) {
    let owner = obj as usize;
    for (sym_ptr, _) in crate::symbol::clone_symbol_entries_for_obj_ptr(owner) {
        let mut attrs = crate::symbol::get_symbol_property_attrs(owner, sym_ptr)
            .unwrap_or_else(|| PropertyAttrs::new(true, true, true));
        if drop_writable {
            attrs.bits &= !PropertyAttrs::WRITABLE;
        }
        if drop_configurable {
            attrs.bits &= !PropertyAttrs::CONFIGURABLE;
        }
        crate::symbol::set_symbol_property_attrs(owner, sym_ptr, attrs);
    }
}

#[no_mangle]
pub extern "C" fn js_object_freeze(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop writable + configurable for every existing key.
            mark_all_keys(
                obj, /*drop_writable=*/ true, false, /*drop_configurable=*/ true,
            );
            mark_all_symbol_keys(
                obj, /*drop_writable=*/ true, /*drop_configurable=*/ true,
            );
        }
    }
    obj_value
}

/// Object.seal(obj) — sets the sealed flag and drops `configurable` on every
/// existing key. Writable is preserved (sealed ≠ frozen). Returns the object.
#[no_mangle]
pub extern "C" fn js_object_seal(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop configurable for every existing key (but leave writable intact).
            mark_all_keys(
                obj, /*drop_writable=*/ false, false, /*drop_configurable=*/ true,
            );
            mark_all_symbol_keys(
                obj, /*drop_writable=*/ false, /*drop_configurable=*/ true,
            );
        }
    }
    obj_value
}

/// Object.preventExtensions(obj) — sets the no-extend flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_prevent_extensions(obj_value: f64) -> f64 {
    // A Proxy is a small registered id, not a heap object — `extract_obj_ptr`
    // yields the fake pointer and `gc_header_for` would deref unmapped memory.
    // Route through the `[[PreventExtensions]]` trap; per spec throw a TypeError
    // if it reports failure, then return the proxy. (Proxy crash cluster.)
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        let ok = crate::proxy::js_reflect_prevent_extensions(obj_value);
        if crate::value::js_is_truthy(ok) == 0 {
            throw_object_type_error(b"'preventExtensions' on proxy: trap returned falsish");
        }
        return obj_value;
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// `TestIntegrityLevel(O, level)` (ECMA-262 7.3.16) for an ordinary heap
/// object: the object must be non-extensible, and every own property must be
/// non-configurable — plus, for the `frozen` level, every own *data* property
/// must be non-writable. Returns `true` if the object satisfies the level.
///
/// A key with no side-table descriptor entry uses the default
/// `{writable: true, enumerable: true, configurable: true}`, which is
/// configurable (and writable), so any such key fails both levels — matching
/// the behaviour of `Object.freeze`/`Object.seal`, which populate the table.
unsafe fn object_integrity_level(obj: *mut ObjectHeader, frozen: bool) -> bool {
    // Must be non-extensible first.
    let gc = gc_header_for(obj);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND == 0 {
        return false;
    }
    // Arrays: a non-empty array's dense elements are configurable/writable
    // unless frozen/sealed dropped those bits (tracked by the array flags).
    if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
        let arr = obj as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        if len > 0 {
            // Has index properties: frozen iff the FROZEN flag is set; sealed
            // iff SEALED (frozen implies sealed).
            let flag = if frozen {
                crate::gc::OBJ_FLAG_FROZEN
            } else {
                crate::gc::OBJ_FLAG_SEALED
            };
            return (*gc)._reserved & flag != 0;
        }
        // Empty array + non-extensible ⇒ integrity holds.
        return true;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return true; // no own keys + non-extensible ⇒ frozen/sealed
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return true;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return false;
    }
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let stored_key = key_val.as_string_ptr();
        if stored_key.is_null() {
            continue;
        }
        let name_ptr = (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*stored_key).byte_len as usize;
        let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // No side-table entry ⇒ default {w,e,c}=true ⇒ configurable ⇒ fails.
        let Some(attrs) = get_property_attrs(obj as usize, name) else {
            return false;
        };
        if attrs.configurable() {
            return false;
        }
        if frozen {
            // Data properties must be non-writable; accessor properties have no
            // writability constraint.
            let is_accessor = get_accessor_descriptor(obj as usize, name).is_some();
            if !is_accessor && attrs.writable() {
                return false;
            }
        }
    }
    // Symbol-keyed own properties must satisfy the same constraints.
    let owner = obj as usize;
    for (sym_ptr, _) in crate::symbol::clone_symbol_entries_for_obj_ptr(owner) {
        let Some(attrs) = crate::symbol::get_symbol_property_attrs(owner, sym_ptr) else {
            return false; // default {w,e,c}=true ⇒ configurable ⇒ fails
        };
        if attrs.configurable() {
            return false;
        }
        if frozen {
            let is_accessor =
                crate::symbol::symbol_accessor_descriptor_bits(owner, sym_ptr).is_some();
            if !is_accessor && attrs.writable() {
                return false;
            }
        }
    }
    true
}

/// Object.isFrozen(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_frozen(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        // Proxy: a frozen proxy is one whose target is non-extensible with all
        // own props non-configurable/non-writable. Fall back to extensibility.
        let ext = crate::proxy::js_reflect_is_extensible(obj_value);
        return if crate::value::js_is_truthy(ext) == 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously frozen
        }
        let gc = gc_header_for(obj);
        // Fast path: the FROZEN flag is authoritative when set.
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        if object_integrity_level(obj, /*frozen=*/ true) {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isSealed(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_sealed(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        let ext = crate::proxy::js_reflect_is_extensible(obj_value);
        return if crate::value::js_is_truthy(ext) == 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously sealed
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_SEALED != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        if object_integrity_level(obj, /*frozen=*/ false) {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isExtensible(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_extensible(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    // Proxy receiver: route through the `[[IsExtensible]]` trap rather than
    // dereferencing the fake pointer. (Proxy crash cluster.)
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        let r = crate::proxy::js_reflect_is_extensible(obj_value);
        return if crate::value::js_is_truthy(r) != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_FALSE); // non-objects are not extensible
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            f64::from_bits(TAG_FALSE)
        } else {
            f64::from_bits(TAG_TRUE)
        }
    }
}
