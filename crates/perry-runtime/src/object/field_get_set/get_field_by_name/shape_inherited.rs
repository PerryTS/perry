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
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let record = super::super::super::shapes::object_shape_record(obj)?;
    let kind = record.object_kind();
    // Class constructor objects have their own read semantics and layout.
    if !kind.is_ordinary_layout() {
        return None;
    }
    let pid = record.proto_id();
    // Declared-class reads already have their shape-authorized class route.
    // Only a branded intrinsic needs this pre-fallback CLASS route: the old
    // class-id band test excluded it. Do not scan a declaration's own keys
    // again or replace its admitted read with a generic prototype walk.
    if (super::super::super::shapes::PROTO_ID_CLASS
        ..super::super::super::shapes::PROTO_ID_UNIQUE)
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
