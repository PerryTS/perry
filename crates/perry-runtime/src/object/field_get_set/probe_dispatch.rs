//! Cold Set/Symbol property dispatch reached from the by-name slow tail.

use super::*;

#[cold]
#[optimize(size)]
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
        let sym_f64 = crate::value::js_nanbox_pointer(obj as i64);
        if key_bytes == b"description" {
            return Some(JSValue::from_bits(
                crate::symbol::js_symbol_description(sym_f64).to_bits(),
            ));
        }
    }
    Some(JSValue::undefined())
}

#[cold]
#[optimize(size)]
#[inline(never)]
pub(super) unsafe fn set_property_if_registered(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if !crate::set::is_registered_set(obj as usize) {
        return None;
    }
    Some(super::map_set_receiver::map_set_instance_property(
        obj, key, false,
    ))
}
