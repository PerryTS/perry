use super::*;
use crate::codegen_abi::{LAYOUT_BAND_EXOTIC, LAYOUT_KIND_PLAIN, NATIVE_LAYOUT_ROWS};

fn record() -> ShapeRecord {
    ShapeRecord::new(0, 0, 0, 0, ShapeObjectKind::Ordinary, 0)
}

#[test]
fn reservation_and_identity_absence_never_commit_records() {
    let mut slab = ShapeSlab::new();
    assert_eq!(slab.region.committed_bytes(), 0);
    assert!(slab.record_ptr(ShapeId(SHAPE_ID_BASE + 123)).is_none());
    assert!(slab.record_ptr(ShapeId(EXOTIC_SHAPE_ID_BASE + 17)).is_none());
    assert_eq!(slab.region.committed_bytes(), 0);
    let id = ShapeId(EXOTIC_SHAPE_ID_BASE + 17);
    let word = LayoutWord::new(LayoutRow(1), LAYOUT_BAND_EXOTIC, LAYOUT_KIND_PLAIN).unwrap();
    let published = slab.publish_static(id, word, record()).unwrap();
    assert_eq!(published, PublishedShape { identity: id, word });
    assert_eq!(slab.region.committed_bytes(), slab.region.page);
    assert_eq!(slab.handle_by_identity(id), Some(word));
    assert_eq!(unsafe { (*slab.record_for_handle(word)).shape_id }, id.0);
    slab.remove(id).unwrap();
    assert!(slab.record_ptr(id).is_none());
    assert_eq!(slab.region.committed_bytes(), 0);
    // Recommit below the issued-row bound; Windows must not trust that bound.
    slab.publish_static(id, word, record()).unwrap();
    assert_eq!(slab.region.committed_bytes(), slab.region.page);
}

#[test]
fn page_boundary_keeps_partial_page_and_dynamic_rows_never_reuse() {
    let mut slab = ShapeSlab::new();
    let first = slab.publish_dynamic(ShapeId(SHAPE_ID_BASE + 5), record()).unwrap();
    let second = slab.publish_dynamic(ShapeId(SHAPE_ID_BASE + 6), record()).unwrap();
    assert_eq!(first.word.row().0, NATIVE_LAYOUT_ROWS);
    assert_eq!(second.word.row().0, NATIVE_LAYOUT_ROWS + 1);
    slab.remove(first.identity).unwrap();
    assert!(slab.region.committed_bytes() != 0);
    assert_eq!(unsafe { (*slab.record_for_handle(second.word)).shape_id }, second.identity.0);
    let third = slab.publish_dynamic(ShapeId(SHAPE_ID_BASE + 7), record()).unwrap();
    assert_eq!(third.word.row().0, NATIVE_LAYOUT_ROWS + 2);
    let boundary = (slab.region.page / 128) as u32;
    for row in [boundary - 1, boundary] {
        let identity = ShapeId(SHAPE_ID_BASE + 100 + row);
        let word = LayoutWord::new(LayoutRow(row), 0, 0).unwrap();
        slab.publish_static(identity, word, record()).unwrap();
    }
    let low = ShapeId(SHAPE_ID_BASE + 100 + boundary - 1);
    slab.remove(low).unwrap();
    assert!(slab.record_ptr(low).is_none());
    assert!(slab.record_ptr(ShapeId(SHAPE_ID_BASE + 100 + boundary)).is_some());
}

#[test]
fn publication_checks_capacity_tags_and_identity_ownership() {
    let mut slab = ShapeSlab::new();
    let id = ShapeId(SHAPE_ID_BASE + 31);
    let word = LayoutWord::new(LayoutRow(0), 0, 0).unwrap();
    slab.publish_static(id, word, record()).unwrap();
    assert_eq!(slab.publish_static(id, word, record()).unwrap().word, word);
    assert_eq!(slab.publish_static(id, LayoutWord(word.0 + 1), record()),
        Err(PublishError::IdentityConflict));
    assert_eq!(slab.publish_static(ShapeId(id.0 + 1), word, record()),
        Err(PublishError::RowConflict));
    assert_eq!(slab.publish_static(ShapeId(id.0 + 1), LayoutWord(u32::MAX), record()),
        Err(PublishError::InvalidWord));
    slab.next_row = slab.region.capacity() as u32;
    assert_eq!(slab.publish_dynamic(ShapeId(id.0 + 1), record()), Err(PublishError::Capacity));
}

#[test]
fn record_abi_offsets_are_target_independent() {
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 128);
    assert_eq!(std::mem::align_of::<ShapeRecord>(), 8);
    assert_eq!(std::mem::offset_of!(ShapeRecord, family_desc_index_plus_one), 76);
    assert_eq!(std::mem::offset_of!(ShapeRecord, shape_id), 120);
    assert_eq!(std::mem::offset_of!(ShapeRecord, layout_word), 124);
}
