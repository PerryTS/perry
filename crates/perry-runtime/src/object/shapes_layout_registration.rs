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
    kind: ShapeObjectKind,
    proto_id: u64,
    summary: u8,
    rep: u64,
    integrity: u16,
    generation: u64,
) {
    let table = &crate::state::state().shapes;
    let mut record = ShapeRecord::new(keys as u64, key_count, slot_count, generation, kind, 0)
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
            generation,
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

/// A semantic generation no other shape carries: a carried prototype
/// layout's (`layout_desc::PROTOTYPE_IMAGE_VARIANT`), so that layout is
/// disjoint from every unmarked object's, as a prototype mark's transition
/// makes it.
pub(crate) fn fresh_semantic_generation() -> u64 {
    let generation = SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        shape_id_exhausted_abort();
    }
    generation
}
