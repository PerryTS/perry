use super::*;

/// Follow an ordinary receiver's published shape link with the original
/// receiver, before the legacy intrinsic fallback. Both raw and boxed
/// pointers reach generic Get (split method reads pass the boxed form).
pub(super) unsafe fn get(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let bits = obj as u64;
    let obj = match bits >> 48 {
        0 => obj,
        0x7FFD => (bits & crate::value::POINTER_MASK) as *const ObjectHeader,
        _ => return None,
    };
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if crate::gc_type_is_not_object!(header.obj_type) {
        return None;
    }
    let record = super::super::super::shapes::object_shape_record(obj)?;
    let kind = record.object_kind();
    // Class constructor objects have their own read semantics and layout.
    if !kind.is_ordinary_layout() {
        return None;
    }
    // A native-module namespace shares the ORDINARY LAYOUT but not its read
    // semantics: its surface is the module table keyed off the object's module
    // name, not the shape's key list (which holds only `__module__`). Admitting
    // one here lets the recorded-`[[Prototype]]` walk answer the read, which
    // pre-empts the namespace vtable's `vt_get_own_field` and therefore
    // `get_native_module_constant`. That only bit the one namespace carrying a
    // prototype — `cluster.default`, wired to `EventEmitter.prototype` by
    // `cluster::cluster_default_value` — so
    // `process.getBuiltinModule('cluster').isPrimary` read `undefined` while
    // every other namespace (no recorded link, already refused below) resolved.
    // #9202.
    if kind == super::super::super::shapes::ShapeObjectKind::NativeNamespace {
        return None;
    }
    let pid = record.proto_id();
    // Declared-class reads already have their shape-authorized class route.
    // Only a branded intrinsic needs this pre-fallback CLASS route: the old
    // class-id band test excluded it. Do not scan a declaration's own keys
    // again or replace its admitted read with a generic prototype walk.
    if (super::super::super::shapes::PROTO_ID_CLASS..super::super::super::shapes::PROTO_ID_UNIQUE)
        .contains(&pid)
        && record.weak_collection_brand().is_none()
    {
        return None;
    }
    let shape = super::super::super::shapes::object_shape_stamp(obj);
    let recorded = super::super::super::shapes::object_prototype_word(obj);
    let bits = if recorded == 0 {
        super::super::super::shapes::shape_prototype_word(shape)
    } else {
        recorded
    };
    if bits >> 48 != 0x7FFD
        && bits != crate::value::TAG_NULL
        && pid != super::super::super::shapes::PROTO_ID_NULL
    {
        return None; // An unmaterialized class/default holder keeps Get.
    }
    // Admit the live link before any own-key work. The native data probe
    // declined accessors too, so a usable link alone cannot prove absence.
    if super::super::super::own_key_present(obj as *mut ObjectHeader, key) {
        return None;
    }
    Some(
        super::super::super::prototype_chain::resolve_inherited_field_from_prototype(
            obj as usize,
            bits,
            key,
        )
        .unwrap_or_else(JSValue::undefined),
    )
}
