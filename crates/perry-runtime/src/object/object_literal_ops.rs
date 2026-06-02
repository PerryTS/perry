//! Object-literal helper entry points.
//!
//! Kept separate from `object_ops.rs` so object-literal semantics fixes do not
//! push that already-large module over the repository file-size gate.

use super::*;

pub(super) unsafe fn object_literal_key_to_string(key_value: f64) -> *mut crate::StringHeader {
    let key_jsv = crate::value::JSValue::from_bits(key_value.to_bits());
    if key_jsv.is_pointer() && crate::symbol::js_is_symbol(key_value) == 0 {
        let obj_ptr = (key_value.to_bits() & crate::value::POINTER_MASK) as usize;
        if obj_ptr >= 0x10000 && is_valid_obj_ptr(obj_ptr as *const u8) {
            let to_string_key =
                crate::string::js_string_from_bytes(b"toString".as_ptr(), b"toString".len() as u32);
            let value_of_key =
                crate::string::js_string_from_bytes(b"valueOf".as_ptr(), b"valueOf".len() as u32);
            let obj = obj_ptr as *const ObjectHeader;
            let gc = gc_header_for(obj);
            let to_string = js_object_get_field_by_name(obj, to_string_key);
            let value_of = js_object_get_field_by_name(obj, value_of_key);
            if ((*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO) != 0
                && to_string.is_undefined()
                && value_of.is_undefined()
            {
                throw_object_type_error(b"Cannot convert object to primitive value");
            }
        }
    }
    crate::value::js_jsvalue_to_string(key_value)
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
    let key_str = object_literal_key_to_string(key_value);
    if key_str.is_null() {
        return value;
    }
    mark_object_dynamic_shape_unknown(obj);
    js_object_set_field_by_name(obj, key_str, value);
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
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        let key_value = js_to_property_key(key_value);
        if crate::symbol::js_is_symbol(key_value) != 0 {
            return crate::symbol::js_object_define_symbol_accessor(
                obj_value, key_value, getter, setter,
            );
        }
        let key_str = object_literal_key_to_string(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        mark_object_dynamic_shape_unknown(obj);
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        super::object_ops::ensure_key_in_keys_array(obj, key_str);
        let Some(k) = key_rust else {
            return obj_value;
        };
        let recv_box = crate::value::js_nanbox_pointer(obj as i64);
        let existing = get_accessor_descriptor(obj as usize, &k).unwrap_or_default();
        let undef = crate::value::TAG_UNDEFINED;
        let get_bits = if getter.to_bits() == undef {
            existing.get
        } else {
            crate::closure::clone_closure_rebind_this(getter.to_bits(), recv_box)
        };
        let set_bits = if setter.to_bits() == undef {
            existing.set
        } else {
            crate::closure::clone_closure_rebind_this(setter.to_bits(), recv_box)
        };
        set_accessor_descriptor(
            obj as usize,
            k.clone(),
            AccessorDescriptor {
                get: get_bits,
                set: set_bits,
            },
        );
        set_property_attrs(obj as usize, k, PropertyAttrs::new(true, true, true));
        obj_value
    }
}
