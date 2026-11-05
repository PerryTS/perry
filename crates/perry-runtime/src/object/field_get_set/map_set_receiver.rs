//! Map/Set instance receivers for dynamic property reads: `.size`, user
//! expando keys (exotic side table), and inherited `Map.prototype` /
//! `Set.prototype` member values. Extracted from `get_field_by_name_tail.rs`
//! (its GC-type Map/Set arm) to stay under the file-size gate.

use super::*;

/// Resolve a named property read on a Map/Set *instance* (`MapHeader` /
/// `SetHeader` — NOT an `ObjectHeader`, so the generic object walk must never
/// see it).
///
/// Order: the ordinary [[Get]]: own expando keys (`cache.custom = x`, stored
/// in the exotic side table under `ExoticKind::Map`/`Set` — own props win over
/// inherited members) → the receiver's [[Prototype]] chain (`m.set`,
/// `m.constructor`, a subclass's `get size`, … — what makes
/// `m.set.call(m, k, v)` reflective dispatch and `(new Map()).constructor ===
/// Map` work) → undefined.
///
/// `size` reads the storage directly only when that IS what the chain would
/// answer: no own `size`, the receiver's [[Prototype]] is the intrinsic
/// `%Map.prototype%` / `%Set.prototype%`, and that prototype is unwritten (so
/// its `size` is still the builtin getter). Any other receiver (a subclass
/// instance, a relinked prototype, a redefined `size`) takes the chain, which
/// finds the user's accessor or the real builtin getter.
pub(crate) unsafe fn map_set_instance_property(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    is_map: bool,
) -> JSValue {
    if !key.is_null() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
        let is_size = key_bytes == b"size";
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
        // #11915: the receiver's REAL inherited chain — its recorded
        // `[[Prototype]]`, else `%Map.prototype%` / `%Set.prototype%` — and
        // every hop above it (`Object.prototype.hasOwnProperty`, a relinked
        // `Map.prototype`'s new parent), accessors included, with the
        // instance as the receiver.
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
        let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
        let recorded = crate::object::prototype_chain::object_static_prototype(obj as usize);
        let builtin =
            crate::object::builtin_prototype_value(if is_map { "Map" } else { "Set" }).to_bits();
        if is_size && recorded.is_none_or(|bits| bits == builtin) {
            let family = if is_map {
                crate::object::builtin_proto::BuiltinProto::Map
            } else {
                crate::object::builtin_proto::BuiltinProto::Set
            };
            if crate::object::builtin_proto::prototype_unwritten(family) {
                let cell = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize;
                return if is_map {
                    JSValue::number(
                        crate::map::js_map_size(cell as *const crate::map::MapHeader) as f64,
                    )
                } else {
                    JSValue::number(
                        crate::set::js_set_size(cell as *const crate::set::SetHeader) as f64,
                    )
                };
            }
        }
        let proto = recorded.unwrap_or(builtin);
        let obj = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize;
        let key =
            crate::value::js_nanbox_get_pointer(key.get_nanbox_f64()) as *const crate::StringHeader;
        if let Some(value) =
            crate::object::prototype_chain::resolve_inherited_field_from_prototype(obj, proto, key)
        {
            return value;
        }
    }
    JSValue::undefined()
}
