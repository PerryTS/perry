//! Startup adoption of declared layouts into the existing shape table.
use super::*;

pub(crate) fn allocate_declared_layout_id(proto_id: u64) -> u32 {
    alloc_shape_id(proto_id).unwrap_or_else(|_| shape_id_exhausted_abort())
}

pub(crate) unsafe fn install_declared_layout(
    id: u32,
    word: u32,
    family_desc_index_plus_one: u32,
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
    record.family_desc_index_plus_one = family_desc_index_plus_one;
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
    let _ = table;
    super::publish_static_record(
        crate::codegen_abi::PublishedShape {
            identity: crate::codegen_abi::ShapeId(id),
            word: crate::codegen_abi::LayoutWord(word),
        },
        record,
        facts,
    )
    .expect("declared layout publication");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// F2 #8: repeated agreeing static requests keep one entry per word in
    /// by_facts and families.
    #[test]
    fn repeated_agreeing_static_requests_do_not_duplicate_index_entries() {
        crate::test_support::bootstrap_test_thread();
        let id = allocate_declared_layout_id(PROTO_ID_DEFAULT);
        let word = crate::codegen_abi::LayoutWord::new(crate::codegen_abi::LayoutRow(3001), 0, 0)
            .unwrap()
            .0;
        for _ in 0..3 {
            unsafe {
                install_declared_layout(
                    id,
                    word,
                    0,
                    std::ptr::null_mut(),
                    0,
                    0,
                    ShapeObjectKind::Ordinary,
                    PROTO_ID_DEFAULT,
                    0,
                    0,
                    0,
                    0,
                );
            }
        }
        let table = &crate::state::state().shapes;
        let inner = table.inner.borrow();
        let in_facts: usize = inner
            .by_facts
            .values()
            .map(|ids| ids.as_slice().iter().filter(|&&w| w == word).count())
            .sum();
        let in_families: usize = inner
            .families
            .values()
            .map(|ids| ids.as_slice().iter().filter(|&&w| w == word).count())
            .sum();
        assert_eq!((in_facts, in_families), (1, 1));
    }
}
