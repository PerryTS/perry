//! Array-specific branches for `Object.*` operations.
//!
//! Split out of `object_ops.rs` to keep that file under the repository
//! line-count guard while preserving the public FFI entry points there.

use super::*;

unsafe fn is_array_object(obj: *const ObjectHeader) -> bool {
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
}

pub(crate) unsafe fn array_property_is_enumerable(
    obj: *mut ObjectHeader,
    key_str: *const crate::StringHeader,
    key_name: &str,
) -> Option<f64> {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if !is_array_object(obj) {
        return None;
    }
    if key_name == "length" {
        return Some(f64::from_bits(TAG_FALSE));
    }
    let arr = obj as *const crate::array::ArrayHeader;
    if !super::has_own_helpers::array_own_key_present(arr, key_str) {
        return Some(f64::from_bits(TAG_FALSE));
    }
    let enumerable = if super::canonical_array_index(key_name).is_some() {
        true
    } else {
        super::get_property_attrs(obj as usize, key_name)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(true)
    };
    Some(f64::from_bits(if enumerable {
        TAG_TRUE
    } else {
        TAG_FALSE
    }))
}

pub(crate) unsafe fn define_array_property(
    obj: *mut ObjectHeader,
    obj_value: f64,
    key_str: *const crate::StringHeader,
    key_name: Option<&str>,
    descriptor_value: f64,
) -> Option<f64> {
    if !is_array_object(obj) {
        return None;
    }
    let Some(key_name) = key_name else {
        return Some(obj_value);
    };
    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return Some(obj_value);
    }
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let has_value = own_key_present(desc_ptr, value_key);
    let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
    let value = if has_value {
        f64::from_bits(value_field.bits())
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };

    if key_name == "length" {
        if has_value {
            crate::array::js_array_set_length(obj as *mut crate::array::ArrayHeader, value);
        }
        return Some(obj_value);
    }
    if let Some(index) = super::canonical_array_index(key_name) {
        if has_value {
            crate::array::js_array_set_f64_extend(
                obj as *mut crate::array::ArrayHeader,
                index,
                value,
            );
        }
        return Some(obj_value);
    }

    crate::array::array_named_property_set(obj as *mut crate::array::ArrayHeader, key_str, value);

    let read_bool = |name: &[u8]| -> Option<bool> {
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        if !own_key_present(desc_ptr, k) {
            return None;
        }
        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
        Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
    };
    let writable = read_bool(b"writable").unwrap_or(false);
    let enumerable = read_bool(b"enumerable").unwrap_or(false);
    let configurable = read_bool(b"configurable").unwrap_or(false);
    set_property_attrs(
        obj as usize,
        key_name.to_string(),
        PropertyAttrs::new(writable, enumerable, configurable),
    );
    Some(obj_value)
}

fn builtin_constructor_prototype_value(name: &[u8]) -> Option<f64> {
    let ctor = js_get_global_this_builtin_value(name.as_ptr(), name.len());
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
    proto_value.is_pointer().then_some(proto)
}

pub(crate) fn array_get_prototype_of_addr(raw_addr: usize) -> Option<f64> {
    if let Some(array_proto) = builtin_constructor_prototype_value(b"Array") {
        let proto_addr = crate::value::js_nanbox_get_pointer(array_proto) as usize;
        if proto_addr != raw_addr {
            return Some(array_proto);
        }
    }
    builtin_constructor_prototype_value(b"Object")
}
