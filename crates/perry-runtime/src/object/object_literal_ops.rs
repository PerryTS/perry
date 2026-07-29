//! Object-literal helper entry points.
//!
//! Kept separate from `object_ops.rs` so object-literal semantics fixes do not
//! push that already-large module over the repository file-size gate.

use super::*;

pub(super) unsafe fn object_literal_key_to_string(key_value: f64) -> *mut crate::StringHeader {
    let key_jsv = crate::value::JSValue::from_bits(key_value.to_bits());
    if !(key_jsv.is_pointer() && crate::symbol::js_is_symbol(key_value) == 0) {
        return crate::value::js_jsvalue_to_string(key_value);
    }
    let obj_ptr = (key_value.to_bits() & crate::value::POINTER_MASK) as usize;
    if !(obj_ptr >= 0x10000 && is_valid_obj_ptr(obj_ptr as *const u8)) {
        return crate::value::js_jsvalue_to_string(key_value);
    }
    // #6935: the null-proto pre-check below is itself GC-capable — the two
    // `js_string_from_bytes` calls allocate, and a `toString` / `valueOf`
    // stored as an accessor makes `js_object_get_field_by_name` run user JS.
    // Any of those can evacuate the key object, so the receiver pointer (and
    // the two freshly allocated name strings) must be re-derived through
    // handles after each step instead of reusing the raw `obj_ptr` captured on
    // entry. The final `js_jsvalue_to_string` — the real coercion — likewise
    // has to see the *current* address of the key.
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_handle = scope.root_nanbox_f64(key_value);
    let to_string_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"toString".as_ptr(),
        b"toString".len() as u32,
    ));
    let value_of_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"valueOf".as_ptr(),
        b"valueOf".len() as u32,
    ));
    let live_obj = |handle: &crate::gc::RuntimeHandle<'_>| -> *const ObjectHeader {
        (handle.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader
    };
    let to_string = js_object_get_field_by_name(
        live_obj(&key_handle),
        to_string_key.get_raw_const_ptr::<crate::StringHeader>(),
    );
    let value_of = js_object_get_field_by_name(
        live_obj(&key_handle),
        value_of_key.get_raw_const_ptr::<crate::StringHeader>(),
    );
    let gc = gc_header_for(live_obj(&key_handle));
    if ((*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO) != 0
        && to_string.is_undefined()
        && value_of.is_undefined()
    {
        throw_object_type_error(b"Cannot convert object to primitive value");
    }
    crate::value::js_jsvalue_to_string(key_handle.get_nanbox_f64())
}

#[no_mangle]
pub unsafe extern "C" fn js_object_literal_to_property_key(key_value: f64) -> f64 {
    if crate::symbol::js_is_symbol(key_value) != 0 {
        return key_value;
    }
    let key_str = object_literal_key_to_string(key_value);
    if key_str.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::value::js_nanbox_string(key_str as i64)
}

#[no_mangle]
pub unsafe extern "C" fn js_object_literal_set_computed(
    obj_value: f64,
    key_value: f64,
    value: f64,
) -> f64 {
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return value;
    }
    if crate::symbol::js_is_symbol(key_value) != 0 {
        return crate::symbol::js_object_set_symbol_property(obj_value, key_value, value);
    }
    if super::property_key_coercion_is_inert(key_value) {
        let key_str = object_literal_key_to_string(key_value);
        if key_str.is_null() {
            return value;
        }
        mark_object_dynamic_shape_unknown(obj);
        js_object_set_field_by_name(obj, key_str, value);
        return value;
    }
    // #6935: `object_literal_key_to_string` runs the user key coercion, so it
    // can allocate → GC → evacuate. The receiver `obj` and the `value` about to
    // be stored in it were both raw across that call; a stale receiver dropped
    // the write on a forwarding stub and a stale `value` planted a dangling
    // pointer inside a live object.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let value_handle = scope.root_nanbox_f64(value);
    let key_str = object_literal_key_to_string(key_value);
    if key_str.is_null() {
        return value_handle.get_nanbox_f64();
    }
    let key_handle = scope.root_string_ptr(key_str);
    let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let value = value_handle.get_nanbox_f64();
    mark_object_dynamic_shape_unknown(obj);
    js_object_set_field_by_name(
        obj,
        key_handle.get_raw_const_ptr::<crate::StringHeader>(),
        value,
    );
    value
}

#[no_mangle]
pub unsafe extern "C" fn js_object_literal_set_prototype(obj_value: f64, proto_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let proto_bits = proto_value.to_bits();
    let proto_jsv = crate::value::JSValue::from_bits(proto_bits);
    if proto_jsv.is_null() {
        super::prototype_chain::object_set_static_prototype(obj as usize, TAG_NULL);
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if crate::symbol::js_is_symbol(proto_value) != 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if value_is_object_like(proto_value) {
        super::prototype_chain::object_set_static_prototype(obj as usize, proto_bits);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Object-literal accessor installer for `{ get k(){}, set k(v){} }` (#2442).
///
/// Installs or merges the accessor descriptor for `(obj, key)`. Literal
/// accessors are enumerable/configurable, and separate getter/setter entries
/// for the same key combine into one descriptor.
#[no_mangle]
pub extern "C" fn js_object_define_accessor(
    obj_value: f64,
    key_value: f64,
    getter: f64,
    setter: f64,
) -> f64 {
    unsafe {
        if extract_obj_ptr(obj_value).is_null() {
            return obj_value;
        }
        // #6935: `js_to_property_key` runs the user key coercion,
        // `object_literal_key_to_string` may allocate, `ensure_key_in_keys_array`
        // grows the keys array and `clone_closure_rebind_this` allocates the
        // bound copies — every one of them can GC and evacuate. The receiver
        // AND the two accessor closures being installed were raw locals across
        // all of that, so a stale getter/setter would be recorded in the
        // descriptor table under the object's (possibly also stale) address.
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_value_handle = scope.root_heap_word_u64(obj_value.to_bits());
        let getter_handle = scope.root_nanbox_f64(getter);
        let setter_handle = scope.root_nanbox_f64(setter);
        let key_value = js_to_property_key(key_value);
        let obj_value = f64::from_bits(obj_value_handle.get_heap_word_u64());
        if crate::symbol::js_is_symbol(key_value) != 0 {
            return crate::symbol::js_object_define_symbol_accessor(
                obj_value,
                key_value,
                getter_handle.get_nanbox_f64(),
                setter_handle.get_nanbox_f64(),
            );
        }
        let key_str = object_literal_key_to_string(key_value);
        if key_str.is_null() {
            return f64::from_bits(obj_value_handle.get_heap_word_u64());
        }
        let key_handle = scope.root_string_ptr(key_str);
        let obj = extract_obj_ptr(f64::from_bits(obj_value_handle.get_heap_word_u64()));
        mark_object_dynamic_shape_unknown(obj);
        let key_rust: Option<String> = {
            let key_str = key_handle.get_raw_const_ptr::<crate::StringHeader>();
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        super::object_ops::ensure_key_in_keys_array(
            obj,
            key_handle.get_raw_const_ptr::<crate::StringHeader>() as *mut crate::StringHeader,
        );
        let obj_value = f64::from_bits(obj_value_handle.get_heap_word_u64());
        let Some(k) = key_rust else {
            return obj_value;
        };
        let obj = extract_obj_ptr(obj_value);
        let existing = get_accessor_descriptor(obj as usize, &k).unwrap_or_default();
        // The previously installed accessor pair is a pair of NaN-boxed closure
        // pointers too — they are carried across the `clone_closure_rebind_this`
        // allocations below, so root them as well.
        let existing_get = scope.root_nanbox_u64(existing.get);
        let existing_set = scope.root_nanbox_u64(existing.set);
        let undef = crate::value::TAG_UNDEFINED;
        let recv_box = crate::value::js_nanbox_pointer(obj as i64);
        let get_bits = if getter_handle.get_nanbox_u64() == undef {
            existing_get.get_nanbox_u64()
        } else {
            crate::closure::clone_closure_rebind_this(getter_handle.get_nanbox_u64(), recv_box)
        };
        let get_bits_handle = scope.root_nanbox_u64(get_bits);
        let recv_box = crate::value::js_nanbox_pointer(extract_obj_ptr(f64::from_bits(
            obj_value_handle.get_heap_word_u64(),
        )) as i64);
        let set_bits = if setter_handle.get_nanbox_u64() == undef {
            existing_set.get_nanbox_u64()
        } else {
            crate::closure::clone_closure_rebind_this(setter_handle.get_nanbox_u64(), recv_box)
        };
        let obj = extract_obj_ptr(f64::from_bits(obj_value_handle.get_heap_word_u64()));
        set_accessor_descriptor(
            obj as usize,
            k.clone(),
            AccessorDescriptor {
                get: get_bits_handle.get_nanbox_u64(),
                set: set_bits,
            },
        );
        set_property_attrs(obj as usize, k, PropertyAttrs::new(true, true, true));
        f64::from_bits(obj_value_handle.get_heap_word_u64())
    }
}
