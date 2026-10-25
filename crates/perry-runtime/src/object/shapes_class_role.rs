//! Receiver classification comes from the receiver's shape and published holder.
//! Numeric class-id membership is only a descriptor registration concern.
use super::*;

/// The receiver carries ordinary record semantics, without a class surface.
/// A published CLASS holder blocks a negative class answer. This also covers
/// native code publishing a prototype surface after the receiver was born.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` receiver.
#[inline]
pub(crate) unsafe fn object_is_plain_record(obj: *const crate::object::ObjectHeader) -> bool {
    let class_id = (*obj).class_id;
    if class_id == 0 {
        return true;
    }
    if class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE {
        return false;
    }
    let Some(shape) = object_shape_record(obj) else {
        return false;
    };
    shape_is_plain_record(class_id, shape.proto_id(), shape.object_kind())
}

/// The same role proof for a caller that already borrowed the live shape.
#[inline]
pub(crate) fn shape_is_plain_record(class_id: u32, proto: u64, kind: ShapeObjectKind) -> bool {
    if class_id == 0 {
        return true;
    }
    // Reserved native/synthetic identities have intrinsic semantics. Their
    // generic paths own those semantics, even if their layout is ordinary.
    if class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE {
        return false;
    }
    if (PROTO_ID_CLASS..PROTO_ID_UNIQUE).contains(&proto) {
        return false;
    }
    kind.is_ordinary_layout() && identity_prototype_word(PROTO_ID_CLASS | u64::from(class_id)) == 0
}

/// A receiver can reach a class method holder through its current identity.
/// Ordinary native module holders also have class ids, but their shapes do
/// not name class prototypes. Their calls use ordinary native dispatch.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` receiver.
#[inline]
pub(crate) unsafe fn receiver_has_class_identity(obj: *const crate::object::ObjectHeader) -> bool {
    let class_id = (*obj).class_id;
    if class_id == 0 {
        return false;
    }
    if identity_prototype_word(PROTO_ID_CLASS | u64::from(class_id)) != 0 {
        return true;
    }
    if let Some(shape) = object_shape_record(obj) {
        return (PROTO_ID_CLASS..PROTO_ID_UNIQUE).contains(&shape.proto_id());
    }
    // Preserve first-birth resolution for an unseeded compiler identity.
    // A reserved native/synthetic header without a holder has no class edge.
    class_id < crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
}

/// A structural transition retains its receiver's semantic class from the
/// predecessor shape. Only the first native/unseeded birth needs the index.
pub(super) unsafe fn receiver_vtable_class(obj: *const crate::object::ObjectHeader) -> u32 {
    let class_id = (*obj).class_id;
    if class_id == 0
        || class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
    {
        return 0;
    }
    if let Some(shape) = object_shape_record(obj) {
        let proto = shape.proto_id();
        if !(PROTO_ID_CLASS..PROTO_ID_UNIQUE).contains(&proto)
            && identity_prototype_word(PROTO_ID_CLASS | u64::from(class_id)) == 0
        {
            return 0;
        }
        return crate::object::class_generic_origin(class_id).unwrap_or(class_id);
    }
    vtable_class(class_id)
}

/// A preinstalled shape owns a newborn's role. Check its implied prototype
/// against the header's class identity or already-published holder, without
/// reconstructing anonymous membership from an id-indexed lookup.
pub(super) unsafe fn newborn_proto_matches(
    obj: *const crate::object::ObjectHeader,
    proto: u64,
) -> bool {
    if !(*obj).meta.is_null()
        || (*super::super::gc_header_for(obj))._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
    {
        return object_proto_id(obj) == proto;
    }
    if proto == PROTO_ID_DEFAULT {
        return (*obj).class_id == 0
            || identity_prototype_word(PROTO_ID_CLASS | u64::from((*obj).class_id)) == 0;
    }
    if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&proto) {
        return proto == class_identity_proto_id((*obj).class_id);
    }
    object_proto_id(obj) == proto
}
