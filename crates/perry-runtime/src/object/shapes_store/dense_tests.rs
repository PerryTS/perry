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
    assert!(slab
        .record_ptr(ShapeId(EXOTIC_SHAPE_ID_BASE + 17))
        .is_none());
    assert_eq!(slab.region.committed_bytes(), 0);
    let id = ShapeId(EXOTIC_SHAPE_ID_BASE + 17);
    let word = LayoutWord::new(LayoutRow(1), LAYOUT_BAND_EXOTIC, LAYOUT_KIND_PLAIN).unwrap();
    let published = slab.publish_static(id, word, record()).unwrap();
    assert_eq!(published, PublishedShape { identity: id, word });
    assert_eq!(slab.region.committed_bytes(), slab.region.page);
    assert_eq!(slab.handle_by_identity(id), Some(word));
    assert_eq!(unsafe { (*slab.record_for_handle(word)).shape_id }, id);
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
    let first = slab
        .publish_dynamic(ShapeId(SHAPE_ID_BASE + 5), record())
        .unwrap();
    let second = slab
        .publish_dynamic(ShapeId(SHAPE_ID_BASE + 6), record())
        .unwrap();
    let start = first.word.row().0;
    assert!(start >= NATIVE_LAYOUT_ROWS && start % super::DYNAMIC_CHUNK_ROWS == 0);
    assert_eq!(second.word.row().0, start + 1);
    slab.remove(first.identity).unwrap();
    assert!(slab.region.committed_bytes() != 0);
    assert_eq!(
        unsafe { (*slab.record_for_handle(second.word)).shape_id },
        second.identity
    );
    let third = slab
        .publish_dynamic(ShapeId(SHAPE_ID_BASE + 7), record())
        .unwrap();
    assert_eq!(third.word.row().0, start + 2);
    let boundary = (slab.region.page / 128) as u32;
    // A 16 KiB page can extend beyond the default native-layout prefix.
    // Declare both fixture rows as static before testing their page custody.
    slab.static_rows = slab.static_rows.max(boundary + 1);
    for row in [boundary - 1, boundary] {
        let identity = ShapeId(SHAPE_ID_BASE + 100 + row);
        let word = LayoutWord::new(LayoutRow(row), 0, 0).unwrap();
        slab.publish_static(identity, word, record()).unwrap();
    }
    let low = ShapeId(SHAPE_ID_BASE + 100 + boundary - 1);
    slab.remove(low).unwrap();
    assert!(slab.record_ptr(low).is_none());
    assert!(slab
        .record_ptr(ShapeId(SHAPE_ID_BASE + 100 + boundary))
        .is_some());
}

#[test]
fn publication_checks_capacity_tags_and_identity_ownership() {
    let mut slab = ShapeSlab::new();
    let id = ShapeId(SHAPE_ID_BASE + 31);
    let word = LayoutWord::new(LayoutRow(0), 0, 0).unwrap();
    slab.publish_static(id, word, record()).unwrap();
    assert_eq!(slab.publish_static(id, word, record()).unwrap().word, word);
    assert_eq!(
        slab.publish_static(id, LayoutWord(word.0 + 1), record()),
        Err(PublishError::IdentityConflict)
    );
    assert_eq!(
        slab.publish_static(ShapeId(id.0 + 1), word, record()),
        Err(PublishError::RowConflict)
    );
    assert_eq!(
        slab.publish_static(ShapeId(id.0 + 1), LayoutWord(u32::MAX), record()),
        Err(PublishError::InvalidWord)
    );
    assert_eq!(
        super::ChunkAllocator::new().claim(super::DYNAMIC_BASE_ROWS as usize),
        Err(PublishError::Capacity)
    );
}

#[test]
fn record_abi_offsets_are_target_independent() {
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 128);
    assert_eq!(std::mem::align_of::<ShapeRecord>(), 8);
    assert_eq!(
        std::mem::offset_of!(ShapeRecord, family_desc_index_plus_one),
        76
    );
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
    let chunks: std::collections::HashSet<u32> = a
        .iter()
        .chain(&b)
        .map(|r| r / super::DYNAMIC_CHUNK_ROWS)
        .collect();
    assert_eq!(
        chunks.len(),
        2 * 100usize.div_ceil(super::DYNAMIC_CHUNK_ROWS as usize)
    );
}

#[test]
fn images_with_different_static_counts_never_share_a_chunk_row() {
    // F2 #4(b): the dynamic base is one process-wide value, independent of S.
    let mut a = ShapeSlab::new();
    a.static_rows = 130;
    a.next_row = 130;
    let mut b = ShapeSlab::new();
    b.static_rows = 500;
    b.next_row = 500;
    let mut rows = std::collections::HashSet::new();
    for i in 0..70u32 {
        for (slab, tag) in [(&mut a, 0), (&mut b, 1)] {
            let id = ShapeId(SHAPE_ID_BASE + 5000 + i * 2 + tag);
            let row = slab.publish_dynamic(id, record()).unwrap().word.row().0;
            assert!(
                row >= super::DYNAMIC_BASE_ROWS,
                "dynamic row {row} below the fixed base"
            );
            assert!(rows.insert(row), "row {row} handed to two images");
        }
    }
}

#[test]
fn dead_agents_chunks_are_never_handed_out_again() {
    use super::{ChunkAllocator, DYNAMIC_BASE_ROWS as BASE, DYNAMIC_CHUNK_ROWS as C};
    let alloc = ChunkAllocator::new();
    let cap = (BASE + 2 * C) as usize;
    let (x, y) = (alloc.claim(cap).unwrap(), alloc.claim(cap).unwrap());
    assert_eq!((x, y), (BASE, BASE + C));
    assert_eq!(
        alloc.claim(cap),
        Err(PublishError::Capacity),
        "exhausted with both live"
    );
    alloc.release(&[x], &[]);
    assert_eq!(
        alloc.claim(cap),
        Err(PublishError::Capacity),
        "a freed chunk is never reused: process-shared memos may still name its words"
    );
    assert_eq!(alloc.free_len(), 1, "the freed chunk stays on the list");
}

#[test]
fn a_dropped_slab_returns_its_chunks_to_the_process() {
    let before = super::CHUNKS.free_len();
    let mut slab = ShapeSlab::new();
    for i in 0..(super::DYNAMIC_CHUNK_ROWS + 1) {
        slab.publish_dynamic(ShapeId(SHAPE_ID_BASE + 9000 + i), record())
            .unwrap();
    }
    assert_eq!(slab.chunks.len(), 2);
    drop(slab);
    assert_eq!(super::CHUNKS.free_len(), before + 2);
}

#[test]
fn region_bookkeeping_is_independent_of_process_history() {
    // Burn a long history of chunks (live elsewhere), then a late agent's
    // bookkeeping covers only its own chunk.
    let mut burned = ShapeSlab::new();
    for _ in 0..4000 {
        super::CHUNKS.claim(burned.region.capacity()).unwrap();
    }
    burned.chunks.clear();
    let mut late = ShapeSlab::new();
    late.publish_dynamic(ShapeId(SHAPE_ID_BASE + 12000), record())
        .unwrap();
    assert_eq!(late.region.leaf_count(), 1);
    assert_eq!(late.region.committed_bytes(), late.region.page);
}
