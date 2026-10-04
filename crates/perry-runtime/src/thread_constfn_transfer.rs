//! Reconstruct final ConstFn facts in the receiver's own shape table.
//! The wire owns scalar facts and permanent image-info addresses, as closure
//! serialization already does. It carries neither heap edges nor ShapeIds.
use crate::object::{field_rep, shapes, static_shapes, ObjectHeader};

#[derive(Debug, Clone)]
pub struct ConstFnTransferFacts {
    rep: u64,
    infos: Vec<shapes::ConstFnSlotInfo>,
}

pub(super) unsafe fn snapshot(
    obj: *const ObjectHeader,
    names: Option<&[Vec<u8>]>,
    field_count: usize,
) -> Option<ConstFnTransferFacts> {
    let d = shapes::object_shape_descriptor(obj)?;
    if d.object_kind != shapes::ShapeObjectKind::Ordinary
        || d.semantic_generation != 0
        || d.hole_count != 0
        || d.summary != 0
        || d.proto_id != shapes::class_proto_id((*obj).class_id)
        || !(*obj).meta.is_null()
        || d.logical_key_count as usize != names?.len()
        || d.live_inline_slot_count as usize != field_count
        || d.special_constfn_mask == 0
        || field_rep::has_deprecated(d.rep)
        || d.deprecation_targets() != (0, 0)
    {
        return None;
    }
    Some(ConstFnTransferFacts {
        rep: d.rep,
        infos: d.constfn_infos().to_vec(),
    })
}

pub(super) unsafe fn restore(
    obj: *mut ObjectHeader,
    class_id: u32,
    live: usize,
    names: &[Vec<u8>],
    facts: &ConstFnTransferFacts,
) -> *mut ObjectHeader {
    // Wire key names came from the source's ordinary key slots. No authority
    // comes from those bytes: the shared finalizer compares them and every
    // closure/F64 lane with the receiving object's current keys and values.
    let packed: Vec<u8> = names
        .iter()
        .flat_map(|n| n.iter().copied().chain([0]))
        .collect();
    let entries: Vec<static_shapes::ConstFnStaticEntry> = facts
        .infos
        .iter()
        .map(|i| static_shapes::ConstFnStaticEntry {
            slot: i.slot as u32,
            info: i.info as usize as *const crate::closure::JsFunctionInfo,
        })
        .collect();
    static_shapes::finalize_constfn_static(
        obj as usize as u64,
        0,
        packed.as_ptr(),
        packed.len() as u32,
        names.len() as u32,
        live as u32,
        class_id,
        facts.rep,
        entries.as_ptr(),
        entries.len() as u32,
        true,
    ) as usize as *mut ObjectHeader
}
