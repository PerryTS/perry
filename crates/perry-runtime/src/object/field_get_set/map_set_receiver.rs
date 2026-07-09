//! Map/Set instance receivers for dynamic property reads: `.size`, user
//! expando keys (exotic side table), and inherited `Map.prototype` /
//! `Set.prototype` member values. Extracted from `get_field_by_name_tail.rs`
//! (its GC-type Map/Set arm) to stay under the file-size gate.

use super::*;

/// Resolve a named property read on a Map/Set *instance* (`MapHeader` /
/// `SetHeader` — NOT an `ObjectHeader`, so the generic object walk must never
/// see it).
///
/// Order: built-in `size` → own expando keys (`cache.custom = x`, stored in
/// the exotic side table under `ExoticKind::Map`/`Set` — own props win over
/// inherited members) → builtin-prototype data fields (`m.set`,
/// `m.constructor`, … — what makes `m.set.call(m, k, v)` reflective dispatch
/// and `(new Map()).constructor === Map` work) → undefined.
pub(crate) unsafe fn map_set_instance_property(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    is_map: bool,
) -> JSValue {
    if !key.is_null() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
        if key_bytes == b"size" {
            return if is_map {
                JSValue::number(crate::map::js_map_size(obj as *const crate::map::MapHeader) as f64)
            } else {
                JSValue::number(crate::set::js_set_size(obj as *const crate::set::SetHeader) as f64)
            };
        }
        if let Ok(name) = std::str::from_utf8(key_bytes) {
            let kind = if is_map {
                crate::object::exotic_expando::ExoticKind::Map
            } else {
                crate::object::exotic_expando::ExoticKind::Set
            };
            let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
            if let Some(v) = crate::object::exotic_expando::exotic_get_own_property(
                obj as usize,
                kind,
                name,
                receiver,
            ) {
                return JSValue::from_bits(v.to_bits());
            }
        }
        let proto = crate::object::builtin_prototype_value(if is_map { "Map" } else { "Set" });
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *const ObjectHeader;
        if !proto_ptr.is_null() {
            if let Some(v) = own_data_field_by_name(proto_ptr, key) {
                return v;
            }
        }
    }
    JSValue::undefined()
}
