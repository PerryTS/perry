//! Cold Set/Symbol property dispatch reached from the by-name slow tail.

use super::*;

#[cold]
#[inline(never)]
pub(super) unsafe fn symbol_property_if_registered(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if !crate::symbol::is_registered_symbol(obj as usize) {
        return None;
    }
    if !key.is_null() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
        let sym_f64 =
            f64::from_bits(0x7FFD_0000_0000_0000u64 | (obj as u64 & 0x0000_FFFF_FFFF_FFFF));
        if key_bytes == b"description" {
            return Some(JSValue::from_bits(
                crate::symbol::js_symbol_description(sym_f64).to_bits(),
            ));
        }
    }
    Some(JSValue::undefined())
}

#[cold]
#[inline(never)]
pub(super) unsafe fn set_property_if_registered(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if !crate::set::is_registered_set(obj as usize) {
        return None;
    }
    if !key.is_null() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
        if key_bytes == b"size" {
            let set = obj as *const crate::set::SetHeader;
            return Some(JSValue::number(crate::set::js_set_size(set) as f64));
        }
        if let Some(name) = set_method_value_name(key_bytes) {
            // Return the SAME brand-checking thunk installed on Set.prototype
            // so `const m = s.forEach; m.call(badThis)` throws a TypeError (and
            // `m === Set.prototype.forEach`). Fall back to the legacy
            // instance-bound closure if the prototype thunk isn't available.
            if let Ok(method_name) = std::str::from_utf8(name) {
                if let Some(value) =
                    super::super::collection_proto_thunks::collection_proto_method_value(
                        "Set",
                        method_name,
                    )
                {
                    return Some(JSValue::from_bits(value.to_bits()));
                }
            }
            let this_f64 = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
            let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
            return Some(JSValue::from_bits(result.to_bits()));
        }
        // User expando keys (`s.tag = x`) live in the exotic side table
        // (`ExoticKind::Set`); see the Map/Set arm in the caller.
        if let Ok(name) = std::str::from_utf8(key_bytes) {
            let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
            if let Some(value) = crate::object::exotic_expando::exotic_get_own_property(
                obj as usize,
                crate::object::exotic_expando::ExoticKind::Set,
                name,
                receiver,
            ) {
                return Some(JSValue::from_bits(value.to_bits()));
            }
        }
    }
    Some(JSValue::undefined())
}
