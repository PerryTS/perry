//! Complete named array Get after the caller classifies the receiver.
use super::*;

pub(super) unsafe fn get(obj: *const ObjectHeader, key: *const crate::StringHeader) -> JSValue {
    let Some(obj) = super::array_retargeted_proto::live_array_owner(obj) else {
        return JSValue::undefined();
    };
    if !key.is_null() {
        // Prototype resolution can allocate; names remain owned bytes throughout
        // the operation, rather than a borrowed heap payload.
        let key_copy = crate::object::field_get_set::HeapKeyBytes::copy_of_key(key);
        let key_bytes = key_copy.as_bytes();
        let arr = obj as *const crate::array::ArrayHeader;
        if key_bytes == b"length" {
            return JSValue::number(crate::array::js_array_length(arr) as f64);
        }
        // #9192; see `array_retargeted_proto::array_proto_slot`.
        if key_bytes == b"__proto__" {
            return super::array_retargeted_proto::array_proto_slot(obj);
        }
        // date-fns / drizzle / lodash duck-typing path:
        // `arr.constructor === Array`, `new arr.constructor(...)`,
        // etc. expect a non-undefined function-typed value that
        // refers back to the intrinsic Array constructor. Read the
        // prototype's actual property, independently of its global
        // binding and including user prototype mutations.
        if key_bytes == b"constructor" {
            // An own `constructor` expando (`arr.constructor = Foo`)
            // shadows the intrinsic — observable via ArraySpeciesCreate
            // (map/filter/slice/splice/concat) and reflection. Only fall
            // back to the inherited property when there is no own write.
            if let Some(v) = own_data_field_by_name(obj, key) {
                return v;
            }
            if let Some(v) = crate::array::array_named_property_get(arr, key) {
                return JSValue::from_bits(v.to_bits());
            }
            // #9192; see `array_retargeted_proto::array_constructor_slot`.
            if let Some(v) = super::array_retargeted_proto::array_constructor_slot(obj) {
                return v;
            }
            return super::accessors::array_prototype_property_value("constructor", obj as usize)
                .unwrap_or_else(JSValue::undefined);
        }
        if let Ok(name) = std::str::from_utf8(key_bytes) {
            if let Some(index) = super::super::canonical_array_index(name) {
                {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.get != 0 {
                            let receiver = crate::value::js_nanbox_pointer(obj as i64);
                            return invoke_accessor_getter(acc.get, receiver);
                        }
                        return JSValue::undefined();
                    }
                }
                if super::super::has_own_helpers::array_own_key_present(arr, key) {
                    return JSValue::from_bits(
                        crate::array::js_array_get_f64(arr, index).to_bits(),
                    );
                }
                if let Some(v) = array_prototype_property_value(name, obj as usize) {
                    return v;
                }
                return JSValue::undefined();
            }
            // Named (non-index) accessor installed via
            // `Object.defineProperty(arr, "prop", {get,set})`.
            {
                if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                    if acc.get != 0 {
                        let receiver = crate::value::js_nanbox_pointer(obj as i64);
                        return invoke_accessor_getter(acc.get, receiver);
                    }
                    return JSValue::undefined();
                }
            }
            if let Some(v) = own_data_field_by_name(obj, key) {
                return v;
            }
            if let Some(v) = crate::array::array_named_property_get(arr, key) {
                return JSValue::from_bits(v.to_bits());
            }
            if let Some(v) = array_prototype_property_value(name, obj as usize) {
                return v;
            }
        }
    }
    JSValue::undefined()
}
