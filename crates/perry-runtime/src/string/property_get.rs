//! Primitive string own properties followed by one canonical chain operation.
use super::{canonical_string_index, js_string_index_get, short_string_index_get};
use crate::value::JSValue;

#[no_mangle]
pub extern "C" fn js_string_index_get_boxed(value: f64, key: f64) -> f64 {
    let receiver = JSValue::from_bits(value.to_bits());
    if !receiver.is_any_string() {
        return crate::value::js_dyn_index_get(value, key);
    }
    let key_value = JSValue::from_bits(key.to_bits());
    if key_value.is_any_string() || unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        return string_property_get_canonical(value, key);
    }
    // Numeric own indices need no observable key conversion or key allocation.
    if key_value.is_number() {
        let own = if receiver.is_short_string() {
            short_string_index_get(receiver, key)
        } else {
            Some(js_string_index_get(receiver.as_string_ptr(), key))
        };
        if let Some(own) = own.filter(|v| v.to_bits() != crate::value::TAG_UNDEFINED) {
            return own;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let key = scope.root_nanbox_f64(key);
    let converted = unsafe { crate::object::js_to_property_key(key.get_nanbox_f64()) };
    string_property_get_canonical(receiver.get_nanbox_f64(), converted)
}

pub(crate) fn string_property_get_canonical(value: f64, key: f64) -> f64 {
    let receiver = JSValue::from_bits(value.to_bits());
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let name = unsafe {
        crate::string::js_string_key_bytes(JSValue::from_bits(key.to_bits()), &mut scratch)
    };
    let index = if let Some(name) = name {
        if name == b"length" {
            return if receiver.is_short_string() {
                receiver.short_string_utf16_len() as f64
            } else {
                unsafe { (*receiver.as_string_ptr()).utf16_len as f64 }
            };
        }
        if !receiver.is_short_string() {
            let string = receiver.as_string_ptr() as *const crate::object::ObjectHeader;
            if let Some(value) = unsafe {
                crate::object::field_get_set::asymmetric_key_string_property(string, name)
            } {
                return f64::from_bits(value.bits());
            }
        }
        std::str::from_utf8(name)
            .ok()
            .and_then(canonical_string_index)
    } else {
        None
    };
    if let Some(index) = index {
        if let Some(character) = receiver
            .is_short_string()
            .then(|| short_string_index_get(receiver, index as f64))
            .flatten()
        {
            return character;
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(value);
        let key = scope.root_nanbox_f64(key);
        let string = crate::string::js_string_materialize_to_heap(receiver.get_nanbox_f64());
        let own = js_string_index_get(string, index as f64);
        if own.to_bits() != crate::value::TAG_UNDEFINED {
            return own;
        }
        return crate::object::field_get_set::primitive_canonical_get(
            receiver.get_nanbox_f64(),
            key.get_nanbox_f64(),
            "String",
        );
    }
    crate::object::field_get_set::primitive_canonical_get(value, key, "String")
}
