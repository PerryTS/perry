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

/// ToUint32 (ECMA-262 §7.1.6) of an already-`ToNumber`-coerced value.
fn to_uint32(number: f64) -> u32 {
    if !number.is_finite() || number == 0.0 {
        return 0;
    }
    number.trunc().rem_euclid(4_294_967_296.0) as u32
}

/// `ArraySetLength(A, Desc)` (ECMA-262 §10.4.2.4): the array exotic
/// `[[DefineOwnProperty]]` for the `"length"` property. The `length` property
/// is a non-configurable, non-enumerable data property; its writability is
/// tracked in the property-attrs side table (absent ⇒ writable). Returns `true`
/// if the definition succeeds, `false` if it must be rejected (the caller turns
/// that into a thrown `TypeError` for `Object.defineProperty` or a `false`
/// return for `Reflect.defineProperty`). A non-integer / out-of-range length
/// throws a `RangeError`, which propagates through both callers.
pub(crate) unsafe fn array_set_length_from_descriptor(
    obj: *mut ObjectHeader,
    descriptor_value: f64,
) -> bool {
    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return true;
    }
    let arr = obj as *mut crate::array::ArrayHeader;

    let read_present = |name: &[u8]| -> bool {
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        own_key_present(desc_ptr, k)
    };
    let read_bool = |name: &[u8]| -> bool {
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
        crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0
    };

    let has_get = read_present(b"get");
    let has_set = read_present(b"set");
    let has_accessor = has_get || has_set;
    let has_value = read_present(b"value");
    let has_writable = read_present(b"writable");
    let new_writable = has_writable && read_bool(b"writable");
    let has_enumerable = read_present(b"enumerable");
    let new_enumerable = has_enumerable && read_bool(b"enumerable");
    let has_configurable = read_present(b"configurable");
    let new_configurable = has_configurable && read_bool(b"configurable");

    // Steps 3-5 (only when a value is supplied): ToUint32 then ToNumber, in that
    // order — each runs `ToNumber` on the descriptor's `value`, so a `valueOf`
    // observer is invoked exactly twice and may mutate the array between calls.
    // Read the current `length` descriptor AFTER both coercions so such a
    // mutation (e.g. flipping `writable` to false) is honored.
    let new_len: Option<u32> = if has_value {
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
        let value = f64::from_bits(value_field.bits());
        let uint = to_uint32(crate::builtins::js_number_coerce(value));
        let number = crate::builtins::js_number_coerce(value);
        // SameValueZero(newLen, numberLen): a fractional / out-of-range length
        // is a RangeError.
        if (uint as f64) != number {
            crate::array::array_length_range_error();
        }
        Some(uint)
    } else {
        None
    };

    let old_len = (*arr).length;
    // `length` is non-configurable, non-enumerable; writable defaults to true
    // until explicitly set otherwise via the side table.
    let cur_writable = super::get_property_attrs(obj as usize, "length")
        .map(|a| a.writable())
        .unwrap_or(true);

    // ValidateAndApplyPropertyDescriptor against the current (non-configurable
    // data) `length` descriptor.
    if has_configurable && new_configurable {
        return false; // can't make a non-configurable property configurable
    }
    if has_enumerable && new_enumerable {
        return false; // can't make a non-enumerable property enumerable
    }
    if has_accessor {
        return false; // can't turn a non-configurable data prop into an accessor
    }
    if !cur_writable {
        if has_writable && new_writable {
            return false; // can't re-enable writability on a non-configurable prop
        }
        if let Some(n) = new_len {
            if n != old_len {
                return false; // can't change the value of a non-writable length
            }
        }
    }

    // Apply. Shrinking deletes the now-out-of-range indices (handled by
    // `js_array_set_length`, which holes the truncated slots); growing pads with
    // holes. `js_array_set_length` doesn't consult the writable side table, and
    // we've already validated the write above.
    if let Some(n) = new_len {
        crate::array::js_array_set_length(arr, n as f64);
    }
    if has_writable {
        // Persist the new writability (enumerable/configurable stay false).
        super::set_property_attrs(
            obj as usize,
            "length".to_string(),
            PropertyAttrs::new(new_writable, false, false),
        );
    }
    true
}

/// `Reflect.defineProperty` hook for the array `length` property. Returns
/// `Some(ok)` only when `obj_value` is an array and `key_value` is `"length"`,
/// so non-length array defines keep flowing through the ordinary path.
pub(crate) unsafe fn array_length_reflect_define(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> Option<bool> {
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() || !is_array_object(obj) {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(key_value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    let key_name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)).ok()?;
    if key_name != "length" {
        return None;
    }
    Some(array_set_length_from_descriptor(obj, descriptor_value))
}

pub(crate) unsafe fn define_array_property(
    obj: *mut ObjectHeader,
    obj_value: f64,
    key_str: *const crate::StringHeader,
    key_name: Option<&str>,
    descriptor_value: f64,
) -> Option<bool> {
    if !is_array_object(obj) {
        return None;
    }
    let Some(key_name) = key_name else {
        return Some(true);
    };

    if key_name == "length" {
        return Some(array_set_length_from_descriptor(obj, descriptor_value));
    }

    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return Some(true);
    }
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let has_value = own_key_present(desc_ptr, value_key);
    let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
    let value = if has_value {
        f64::from_bits(value_field.bits())
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };

    if let Some(index) = super::canonical_array_index(key_name) {
        if has_value {
            crate::array::js_array_set_f64_extend(
                obj as *mut crate::array::ArrayHeader,
                index,
                value,
            );
        }
        return Some(true);
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
    let _ = obj_value;
    Some(true)
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
