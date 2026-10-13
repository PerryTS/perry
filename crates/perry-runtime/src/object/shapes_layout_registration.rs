//! Startup adoption of declared layouts into the existing shape table.
use super::*;

pub(crate) fn allocate_declared_layout_id(proto_id: u64) -> u32 {
    alloc_shape_id(proto_id).unwrap_or_else(|_| shape_id_exhausted_abort())
}

pub(crate) unsafe fn install_declared_layout(
    id: u32,
    keys: *mut ArrayHeader,
    key_count: u32,
    slot_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    integrity: u16,
) {
    let table = &crate::state::state().shapes;
    let kind = ShapeObjectKind::Ordinary;
    let mut record = ShapeRecord::new(keys as u64, key_count, slot_count, 0, kind, 0)
        .with_proto_id(proto_id)
        .with_summary(summary)
        .with_special_facts(rep, &[], &[]);
    record.set_integrity_flags(integrity);
    record.set(RECORD_FLAG_EXTERNAL_CARRIER, true);
    let facts = shapes_store::integrity_facts_key(
        shapes_store::facts_key_proto_with_special(
            keys as u64,
            key_count,
            slot_count,
            0,
            kind,
            0,
            proto_id,
            summary,
            rep,
            &[],
            &[],
        ),
        integrity,
    );
    let mut inner = table.inner.borrow_mut();
    table.slab_mut().insert(id, record);
    inner.facts_append_fresh(facts, id);
    inner.family_append_fresh(keys as u64, id);
}
