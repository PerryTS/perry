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
    let start = first.word.row().0;
    assert!(start >= NATIVE_LAYOUT_ROWS && start % super::DYNAMIC_CHUNK_ROWS == 0);
    assert_eq!(second.word.row().0, start + 1);
    slab.remove(first.identity).unwrap();
    assert!(slab.region.committed_bytes() != 0);
    assert_eq!(unsafe { (*slab.record_for_handle(second.word)).shape_id }, second.identity.0);
    let third = slab.publish_dynamic(ShapeId(SHAPE_ID_BASE + 7), record()).unwrap();
    assert_eq!(third.word.row().0, start + 2);
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
    assert_eq!(
        super::claim_dynamic_chunk(slab.static_rows, slab.static_rows as usize),
        Err(PublishError::Capacity)
    );
}

#[test]
fn record_abi_offsets_are_target_independent() {
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 128);
    assert_eq!(std::mem::align_of::<ShapeRecord>(), 8);
    assert_eq!(std::mem::offset_of!(ShapeRecord, family_desc_index_plus_one), 76);
    assert_eq!(std::mem::offset_of!(ShapeRecord, shape_id), 120);
    assert_eq!(std::mem::offset_of!(ShapeRecord, layout_word), 124);
}

#[test]
fn agents_never_receive_the_same_dynamic_row() {
    // Two agents publish dynamic rows concurrently; their rows are disjoint,
    // chunk-aligned and committed only in the owner's Region. Fails if the
    // chunk counter were per-agent (both would start at the same row).
    let rows = |n: u32| {
        std::thread::spawn(move || {
            let mut slab = ShapeSlab::new();
            let mut out = Vec::new();
            for i in 0..n {
                let id = ShapeId(SHAPE_ID_BASE + 1000 + i);
                out.push(slab.publish_dynamic(id, record()).unwrap().word.row().0);
            }
            // A row of the other agent's chunk is not committed here.
            let foreign = out[0] + super::DYNAMIC_CHUNK_ROWS * 1000;
            assert!(!slab.region.row_committed(foreign as usize));
            out
        })
    };
    let (a, b) = (rows(100), rows(100));
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    for r in &a {
        assert!(!b.contains(r), "row {r} handed to two agents");
    }
    let chunks: std::collections::HashSet<u32> =
        a.iter().chain(&b).map(|r| r / super::DYNAMIC_CHUNK_ROWS).collect();
    assert_eq!(chunks.len(), 2 * 100usize.div_ceil(super::DYNAMIC_CHUNK_ROWS as usize));
}
