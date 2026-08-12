use super::*;
use crate::gc::{
    GcHeader, GC_FLAG_MARKED, GC_FLAG_TENURED, GC_HEADER_SIZE, GC_TYPE_ARRAY, GC_TYPE_BUFFER,
    GC_TYPE_STRING, GC_TYPE_TYPED_ARRAY, LARGE_OBJECT_THRESHOLD_BYTES,
};

fn general_block_index_for(addr: usize) -> Option<usize> {
    sync_inline_arena_state();
    ARENA.with(|a| {
        let arena = unsafe { &*a.get() };
        arena.blocks.iter().enumerate().find_map(|(idx, block)| {
            if block.data.is_null() {
                return None;
            }
            let base = block.data as usize;
            let end = base + block.size;
            (addr >= base && addr < end).then_some(idx)
        })
    })
}

fn general_block_offset(idx: usize) -> usize {
    sync_inline_arena_state();
    ARENA.with(|a| unsafe { (&*a.get()).blocks[idx].offset })
}

pub(super) fn run_with_fresh_arenas(test: impl FnOnce() + Send + 'static) {
    std::thread::spawn(test)
        .join()
        .expect("arena test panicked");
}

fn reset_old_nursery_block(dead_cycles_before: u32) -> (usize, usize, usize, ArenaResetStats) {
    let mut blocks = Vec::new();
    for _ in 0..7 {
        let ptr = arena_alloc(BLOCK_SIZE, 8) as usize;
        let idx = general_block_index_for(ptr).expect("allocation should be in nursery");
        blocks.push(idx);
    }
    blocks.sort_unstable();
    blocks.dedup();
    assert!(
        blocks.len() >= 7,
        "test setup should force seven distinct nursery blocks"
    );

    let current = ARENA.with(|a| unsafe { (&*a.get()).current });
    let keep_low = current.saturating_sub(4);
    let candidate = blocks
        .into_iter()
        .find(|&idx| idx < keep_low)
        .expect("test setup should leave a nursery block outside the keep window");

    let (base, size) = ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        let block = &mut arena.blocks[candidate];
        assert!(!block.data.is_null());
        assert!(block.offset > 0);
        block.dead_cycles = dead_cycles_before;
        (block.data as usize, block.size)
    });

    let mut block_has_live = vec![false; arena_block_count()];
    block_has_live[current] = true;
    let stats = arena_reset_empty_blocks(&block_has_live);
    (candidate, base, size, stats)
}

fn reset_single_reclaimable_nursery_block(
    dead_cycles_before: u32,
) -> (usize, usize, usize, usize, ArenaResetStats) {
    let mut blocks = Vec::new();
    for _ in 0..6 {
        let ptr = arena_alloc(BLOCK_SIZE, 8) as usize;
        let idx = general_block_index_for(ptr).expect("allocation should be in nursery");
        blocks.push(idx);
    }
    blocks.sort_unstable();
    blocks.dedup();
    assert_eq!(
        blocks.len(),
        6,
        "test setup should force six distinct nursery blocks"
    );

    let current = ARENA.with(|a| unsafe { (&*a.get()).current });
    let keep_low = current.saturating_sub(4);
    let candidate = blocks
        .into_iter()
        .find(|&idx| idx < keep_low)
        .expect("test setup should leave exactly one block outside the keep window");

    let (base, size, before_offset) = ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        let block = &mut arena.blocks[candidate];
        assert!(!block.data.is_null());
        assert!(block.offset > 0);
        block.dead_cycles = dead_cycles_before;
        (block.data as usize, block.size, block.offset)
    });

    let mut block_has_live = vec![false; arena_block_count()];
    block_has_live[current] = true;
    let stats = arena_reset_empty_blocks(&block_has_live);
    (candidate, base, size, before_offset, stats)
}

#[test]
fn survivor_reclaim_resets_dead_blocks() {
    run_with_fresh_arenas(|| {
        let baseline = arena_telemetry_snapshot();
        let _dead = arena_alloc_gc_survivor(2 * 1024 * 1024, 8, GC_TYPE_STRING);
        let after_alloc = arena_telemetry_snapshot();
        let survivor_in_use = after_alloc
            .survivor0
            .in_use_bytes
            .saturating_add(after_alloc.survivor1.in_use_bytes);
        assert!(
            survivor_in_use > baseline.survivor0.in_use_bytes + baseline.survivor1.in_use_bytes,
            "test allocation should occupy a survivor semispace"
        );

        let block_has_live = vec![false; arena_block_count()];
        let stats = survivor_arena_reclaim_dead_blocks(&block_has_live);
        let after_reclaim = arena_telemetry_snapshot();
        let survivor_after = after_reclaim
            .survivor0
            .in_use_bytes
            .saturating_add(after_reclaim.survivor1.in_use_bytes);

        assert_eq!(survivor_after, 0);
        assert!(stats.reset_blocks > 0);
        assert!(stats.reusable_bytes > 0 || stats.removed_bytes > 0);
        assert!(
            after_reclaim.total_reserved_bytes <= after_alloc.total_reserved_bytes,
            "dead survivor blocks should become reusable or be returned"
        );
    });
}

#[test]
fn budgeted_survivor_reclaim_accumulates_release_stats_across_slices() {
    run_with_fresh_arenas(|| {
        for _ in 0..3 {
            let ptr = arena_alloc_gc_survivor(BLOCK_SIZE, 8, GC_TYPE_STRING);
            assert!(!ptr.is_null());
        }

        let snapshots = arena_block_snapshots();
        let block_has_live = vec![false; snapshots.len()];
        let mut reclaim = SurvivorArenaReclaimDeadBlocksState::new(&block_has_live, &snapshots);
        let mut slices = 0;
        while !reclaim.step(1) {
            slices += 1;
            assert!(slices < 32, "one-unit survivor reclaim must converge");
        }

        let stats = reclaim.stats();
        assert!(slices > 3, "the test must span multiple reclamation slices");
        assert_eq!(stats.reset_blocks, 3);
        assert_eq!(stats.removed_blocks, 2);
        assert!(stats.removed_bytes >= 2 * BLOCK_SIZE);
        assert_eq!(stats.pooled_blocks, 2);
        assert_eq!(stats.pooled_bytes, stats.removed_bytes);
        assert_eq!(stats.deallocated_blocks, 0);
        assert_eq!(stats.deallocated_bytes, 0);
    });
}

fn page_range_for(base: usize, size: usize) -> std::ops::RangeInclusive<usize> {
    generation_page_for_addr(base)..=generation_page_for_addr(base + size - 1)
}

fn old_page_meta(page: usize) -> OldPageMeta {
    old_page_meta_for_tests(page).expect("old page metadata should be registered")
}

fn old_header_and_size(user_ptr: usize) -> (usize, usize) {
    let header_addr = user_ptr - GC_HEADER_SIZE;
    let total_size = unsafe { (*(header_addr as *const GcHeader)).size as usize };
    (header_addr, total_size)
}

fn assert_seen_headers(label: &str, seen: &[usize], expected: &[usize]) {
    for &header in expected {
        assert!(
            seen.contains(&header),
            "{label} did not visit expected header {header:#x}"
        );
    }
}

fn synthetic_old_block_range() -> (usize, usize) {
    (0x4000_0000_0000usize, GENERATION_PAGE_SIZE * 3)
}

#[test]
fn old_page_metadata_registers_old_block_pages() {
    run_with_fresh_arenas(|| {
        // Old-arena blocks are lazily materialized: force the first
        // block with a raw (non-GC-header) old alloc, which registers
        // the block's pages without registering any object bytes.
        let _ = arena_alloc_old(8, 8);
        OLD_ARENA.with(|a| unsafe {
            let arena = &*a.get();
            let block = &arena.blocks[arena.current];
            for page in page_range_for(block.data as usize, block.size) {
                let meta = old_page_meta(page);
                assert_eq!(meta.page_base, generation_page_base(page));
                assert_eq!(
                    meta.page_end,
                    generation_page_base(page) + GENERATION_PAGE_SIZE
                );
                assert_eq!(meta.allocated_bytes, 0);
                assert_eq!(meta.live_bytes, 0);
                assert_eq!(meta.dead_bytes, 0);
                assert_eq!(meta.object_count, 0);
                assert_eq!(meta.live_object_count, 0);
                assert_eq!(meta.dead_object_count, 0);
                assert_eq!(meta.pinned_bytes, 0);
                assert_eq!(meta.pinned_object_count, 0);
                assert_eq!(meta.dirty_slots, 0);
                assert!(!meta.dirty);
                assert!(!meta.evacuation_eligible);
            }
        });
    });
}

#[test]
fn old_page_metadata_tracks_old_object_allocation() {
    run_with_fresh_arenas(|| {
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let (header_addr, total_size) = old_header_and_size(old_ptr);
        let overlaps = old_object_page_overlaps(header_addr, total_size);

        let mut total_overlap = 0usize;
        for (page, bytes) in overlaps {
            total_overlap += bytes;
            let meta = old_page_meta(page);
            assert_eq!(meta.allocated_bytes, bytes);
            assert_eq!(meta.live_bytes, 0);
            assert_eq!(meta.dead_bytes, 0);
            assert_eq!(meta.object_count, 1);
            assert_eq!(meta.live_object_count, 0);
            assert_eq!(meta.dead_object_count, 0);
            assert_eq!(meta.pinned_bytes, 0);
            assert_eq!(meta.pinned_object_count, 0);
            assert!(!meta.dirty);
            assert!(!meta.evacuation_eligible);
        }
        assert_eq!(total_overlap, total_size);
    });
}

#[test]
fn old_page_metadata_snapshot_is_sorted_by_page() {
    run_with_fresh_arenas(|| {
        let _first = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let _second = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;

        let snapshot = old_page_meta_snapshot();
        assert!(!snapshot.is_empty());
        assert!(
            snapshot
                .windows(2)
                .all(|pair| pair[0].page_base <= pair[1].page_base),
            "old page metadata snapshot should be deterministic"
        );
    });
}

#[test]
fn old_page_metadata_reregisters_after_block_metadata_removal() {
    run_with_fresh_arenas(|| {
        let (base, size) = synthetic_old_block_range();
        register_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
        let pages: Vec<usize> = page_range_for(base, size).collect();
        assert!(pages
            .iter()
            .all(|&page| old_page_meta_for_tests(page).is_some()));

        unregister_block_generation(base, size);
        assert!(
            pages
                .iter()
                .all(|&page| old_page_meta_for_tests(page).is_none()),
            "old page metadata should be removed with the old block"
        );

        register_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
        for &page in &pages {
            let meta = old_page_meta(page);
            assert_eq!(meta.allocated_bytes, 0);
            assert_eq!(meta.live_bytes, 0);
            assert_eq!(meta.dead_bytes, 0);
            assert_eq!(meta.object_count, 0);
            assert_eq!(meta.live_object_count, 0);
            assert_eq!(meta.dead_object_count, 0);
        }
        unregister_block_generation(base, size);
    });
}

#[test]
fn old_page_metadata_distributes_multi_page_object_bytes_and_indexes_pages() {
    run_with_fresh_arenas(|| {
        let old_ptr = arena_alloc_gc_old(GENERATION_PAGE_SIZE * 2 + 77, 8, GC_TYPE_STRING) as usize;
        let (header_addr, total_size) = old_header_and_size(old_ptr);
        let overlaps = old_object_page_overlaps(header_addr, total_size);
        assert!(
            overlaps.len() > 1,
            "test allocation should span multiple old pages"
        );

        let mut pages = crate::fast_hash::new_ptr_hash_set();
        let mut total_overlap = 0usize;
        for &(page, bytes) in &overlaps {
            pages.insert(page);
            total_overlap += bytes;
            let meta = old_page_meta(page);
            assert_eq!(meta.allocated_bytes, bytes);
            assert_eq!(meta.live_bytes, 0);
            assert_eq!(meta.dead_bytes, 0);
            assert_eq!(meta.object_count, 1);
            assert_eq!(meta.live_object_count, 0);
            assert_eq!(meta.dead_object_count, 0);
            assert_eq!(meta.pinned_bytes, 0);
            assert_eq!(meta.pinned_object_count, 0);
            assert!(!meta.evacuation_eligible);
        }
        assert_eq!(total_overlap, total_size);

        let mut visited = Vec::new();
        let count = old_arena_walk_objects_on_pages(&pages, |header| {
            visited.push(header as usize);
        });
        assert_eq!(count, 1);
        assert_eq!(visited, vec![header_addr]);
    });
}

#[test]
fn old_page_metadata_removes_object_and_block_metadata() {
    run_with_fresh_arenas(|| {
        let old_ptr = arena_alloc_gc_old(96, 8, GC_TYPE_STRING) as usize;
        let (header_addr, total_size) = old_header_and_size(old_ptr);
        let overlaps = old_object_page_overlaps(header_addr, total_size);
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        for &(page, _) in &overlaps {
            pages.insert(page);
        }
        unregister_old_object_pages(header_addr, total_size);
        for &(page, _) in &overlaps {
            let meta = old_page_meta(page);
            assert_eq!(meta.allocated_bytes, 0);
            assert_eq!(meta.live_bytes, 0);
            assert_eq!(meta.dead_bytes, 0);
            assert_eq!(meta.object_count, 0);
            assert_eq!(meta.live_object_count, 0);
            assert_eq!(meta.dead_object_count, 0);
            assert!(!meta.evacuation_eligible);
        }
        assert_eq!(old_arena_walk_objects_on_pages(&pages, |_| {}), 0);

        let (base, size) = synthetic_old_block_range();
        register_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
        let block_pages: Vec<usize> = page_range_for(base, size).collect();
        assert!(block_pages
            .iter()
            .all(|&page| old_page_meta_for_tests(page).is_some()));
        unregister_block_generation(base, size);
        assert!(block_pages
            .iter()
            .all(|&page| old_page_meta_for_tests(page).is_none()));
    });
}

#[test]
fn generation_metadata_classifies_arena_regions() {
    run_with_fresh_arenas(|| {
        let nursery = arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
        let longlived = arena_alloc_gc_longlived(32, 8, GC_TYPE_STRING) as usize;
        let old = arena_alloc_gc_old(32, 8, GC_TYPE_STRING) as usize;

        assert_eq!(classify_heap_generation(nursery), HeapGeneration::Nursery);
        assert_eq!(
            classify_heap_generation(longlived),
            HeapGeneration::Longlived
        );
        assert_eq!(classify_heap_generation(old), HeapGeneration::Old);
        assert!(pointer_in_nursery(nursery));
        assert!(!pointer_in_nursery(longlived));
        assert!(!pointer_in_old_gen(longlived));
        assert!(pointer_in_old_gen(old));
    });
}

#[test]
fn generation_metadata_bucket_keeps_exact_range_boundaries() {
    run_with_fresh_arenas(|| {
        let bucket_base = 0x0055_0000_0000usize & !((1usize << GENERATION_CLASS_SHIFT) - 1);
        let nursery_base = bucket_base + 0x1000;
        let old_base = bucket_base + 0x4000;
        let range_size = 0x1000;

        register_block_space(
            nursery_base,
            range_size,
            HeapGeneration::Nursery,
            HeapSpace::NurseryEden,
        );
        register_block_space(old_base, range_size, HeapGeneration::Old, HeapSpace::Old);

        assert_eq!(
            classify_heap_generation(nursery_base + 0x80),
            HeapGeneration::Nursery
        );
        assert_eq!(
            classify_heap_generation(old_base + 0x80),
            HeapGeneration::Old
        );
        assert_eq!(
            classify_heap_generation(bucket_base + 0x3000),
            HeapGeneration::Unknown,
            "same metadata bucket must not classify holes between exact ranges"
        );

        unregister_block_generation(nursery_base, range_size);
        assert_eq!(
            classify_heap_generation(nursery_base + 0x80),
            HeapGeneration::Unknown
        );
        assert_eq!(
            classify_heap_generation(old_base + 0x80),
            HeapGeneration::Old,
            "removing one range must not remove another range in the same bucket"
        );

        unregister_block_generation(old_base, range_size);
        assert_eq!(
            classify_heap_generation(old_base + 0x80),
            HeapGeneration::Unknown
        );
    });
}

#[test]
fn large_object_arena_alloc_gc_is_old_tenured_and_indexed() {
    run_with_fresh_arenas(|| {
        let payload = crate::gc::LARGE_OBJECT_THRESHOLD_BYTES;
        let ptr = arena_alloc_gc(payload, 8, GC_TYPE_STRING) as usize;
        let header_addr = ptr - GC_HEADER_SIZE;
        let total = unsafe { (*(header_addr as *const GcHeader)).size as usize };

        assert!(
            crate::gc::is_large_object_total_size(total),
            "test allocation should exceed the large-object threshold"
        );
        assert_eq!(classify_heap_generation(ptr), HeapGeneration::Old);
        assert!(pointer_in_old_gen(ptr));
        assert!(!pointer_in_nursery(ptr));
        unsafe {
            let header = header_addr as *const GcHeader;
            assert_ne!((*header).gc_flags & GC_FLAG_TENURED, 0);
        }

        let overlaps = old_object_page_overlaps(header_addr, total);
        assert!(!overlaps.is_empty());
        for &(page, _) in &overlaps {
            let meta = old_page_meta(page);
            assert_eq!(meta.object_count, 1);
        }
    });
}

/// The birth-generation threshold is TYPE-DEPENDENT, and this pins the split
/// rather than the constants.
///
/// The test above allocates `LARGE_OBJECT_THRESHOLD_BYTES` of `GC_TYPE_STRING`
/// and asserts it is born Old + `GC_FLAG_TENURED`. The *same size* of
/// `GC_TYPE_ARRAY` must be born in the NURSERY, because being born tenured
/// costs a pointer-bearing object far more than its own bytes: a minor never
/// sweeps old-gen, so the container and — through the remembered set —
/// everything it names stay live until a full mark-sweep. `shapes.ts`'s
/// 2000-element array is 16 400 bytes, sixteen over the old flat line, and that
/// alone made its two minors re-mark 94 000 then 118 006 slots.
///
/// Sabotage check: reverting `arena_alloc_gc` to the flat
/// `is_large_object_total_size` fails the first assertion here — it is the
/// whole of the change.
#[test]
fn pointer_bearing_objects_get_a_wider_born_tenured_threshold_than_pointer_free_ones() {
    run_with_fresh_arenas(|| {
        // Just over the pointer-FREE line, well under the pointer-bearing one.
        let payload = LARGE_OBJECT_THRESHOLD_BYTES;

        let array = arena_alloc_gc(payload, 8, GC_TYPE_ARRAY) as usize;
        assert!(
            pointer_in_nursery(array),
            "a pointer-bearing object between the two thresholds must be born \
             young, or every object it reaches is immortal until a full GC"
        );
        assert!(!pointer_in_old_gen(array));
        unsafe {
            let header = (array - GC_HEADER_SIZE) as *const GcHeader;
            assert_eq!(
                (*header).gc_flags & GC_FLAG_TENURED,
                0,
                "born-young means born untenured"
            );
        }

        // Same size, pointer-free: unchanged, still born tenured in old-gen.
        let string = arena_alloc_gc(payload, 8, GC_TYPE_STRING) as usize;
        assert!(pointer_in_old_gen(string));
        assert!(!pointer_in_nursery(string));

        // Above the wider line, a pointer-bearing object is born tenured again.
        let huge = arena_alloc_gc(
            crate::gc::LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES + 64,
            8,
            GC_TYPE_ARRAY,
        ) as usize;
        assert!(pointer_in_old_gen(huge));
        unsafe {
            let header = (huge - GC_HEADER_SIZE) as *const GcHeader;
            assert_ne!((*header).gc_flags & GC_FLAG_TENURED, 0);
        }
    });
}

/// Everything the widened threshold admits to the nursery must be MOVABLE.
///
/// Two independent ceilings, and a violation of either is silent: an object
/// larger than `MAX_YOUNG_MOVE_BYTES` is refused by `move_young` and left
/// behind in from-space, and one larger than a nursery block cannot be
/// bump-allocated there at all. Neither would fail a correctness test until a
/// collection landed on such an object, so the invariant is asserted directly
/// on the constants.
#[test]
fn pointer_bearing_large_object_threshold_is_movable() {
    assert!(
        crate::gc::LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES < crate::gc::MAX_YOUNG_MOVE_BYTES,
        "the allocator must not admit to the nursery an object move_young refuses to relocate"
    );
    assert!(
        crate::gc::LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES <= block::BLOCK_SIZE,
        "a nursery-resident object must fit in a nursery block"
    );
    assert!(
        crate::gc::LARGE_OBJECT_THRESHOLD_BYTES
            <= crate::gc::LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES,
        "the pointer-bearing threshold is a widening, never a narrowing"
    );
}

/// The type table, not a hardcoded type list, is what selects the threshold.
#[test]
fn large_object_threshold_follows_the_type_table_pointer_free_flag() {
    use crate::gc::{
        large_object_threshold_for_type, LARGE_OBJECT_THRESHOLD_BYTES as SMALL,
        LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES as WIDE,
    };
    // pointer_free: false
    assert_eq!(large_object_threshold_for_type(GC_TYPE_ARRAY), WIDE);
    assert_eq!(
        large_object_threshold_for_type(crate::gc::GC_TYPE_OBJECT),
        WIDE
    );
    assert_eq!(
        large_object_threshold_for_type(crate::gc::GC_TYPE_CLOSURE),
        WIDE
    );
    // pointer_free: true
    assert_eq!(large_object_threshold_for_type(GC_TYPE_STRING), SMALL);
    assert_eq!(large_object_threshold_for_type(GC_TYPE_BUFFER), SMALL);
    // An unknown type takes the conservative value: the widening is justified
    // by the type table saying the payload is traced, and it cannot say that.
    assert_eq!(large_object_threshold_for_type(u8::MAX), SMALL);
}

#[test]
fn large_buffer_and_typed_array_old_objects_are_seen_by_arena_walkers() {
    run_with_fresh_arenas(|| {
        let buf = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
        let ta = crate::typedarray::typed_array_alloc(
            crate::typedarray::KIND_UINT8,
            LARGE_OBJECT_THRESHOLD_BYTES as u32,
        ) as usize;
        let buf_header = buf - GC_HEADER_SIZE;
        let ta_header = ta - GC_HEADER_SIZE;
        let expected = [buf_header, ta_header];

        unsafe {
            assert_eq!((*(buf_header as *const GcHeader)).obj_type, GC_TYPE_BUFFER);
            assert_eq!(
                (*(ta_header as *const GcHeader)).obj_type,
                GC_TYPE_TYPED_ARRAY
            );
        }
        assert!(pointer_in_old_gen(buf));
        assert!(pointer_in_old_gen(ta));

        let mut normal = Vec::new();
        arena_walk_objects(|header| {
            let header = header as usize;
            if expected.contains(&header) {
                normal.push(header);
            }
        });
        assert_seen_headers("arena_walk_objects", &normal, &expected);

        let mut old_only = Vec::new();
        old_arena_walk_objects(|header| {
            let header = header as usize;
            if expected.contains(&header) {
                old_only.push(header);
            }
        });
        assert_seen_headers("old_arena_walk_objects", &old_only, &expected);

        let mut addr_sorted = Vec::new();
        arena_walk_objects_addr_sorted(|header| {
            let header = header as usize;
            if expected.contains(&header) {
                addr_sorted.push(header);
            }
        });
        assert_seen_headers("arena_walk_objects_addr_sorted", &addr_sorted, &expected);

        let mut indexed = Vec::new();
        let mut selected_blocks = Vec::new();
        arena_walk_objects_with_block_index(|header, block_idx| {
            let header = header as usize;
            if expected.contains(&header) {
                indexed.push(header);
                if !selected_blocks.contains(&block_idx) {
                    selected_blocks.push(block_idx);
                }
            }
        });
        assert_seen_headers("arena_walk_objects_with_block_index", &indexed, &expected);
        assert!(
            !selected_blocks.is_empty(),
            "indexed walk should identify target old blocks"
        );

        let mut filtered = Vec::new();
        arena_walk_objects_filtered(
            |block_idx| selected_blocks.contains(&block_idx),
            |header, _block_idx| {
                let header = header as usize;
                if expected.contains(&header) {
                    filtered.push(header);
                }
            },
        );
        assert_seen_headers("arena_walk_objects_filtered", &filtered, &expected);
    });
}

#[test]
fn generation_metadata_survives_nursery_block_reset() {
    run_with_fresh_arenas(|| {
        let (idx, base, size, stats) = reset_old_nursery_block(0);
        assert!(
            stats.reset_blocks >= 1,
            "test setup should reset at least one nursery block"
        );
        ARENA.with(|a| unsafe {
            let arena = &*a.get();
            assert!(!arena.blocks[idx].data.is_null());
            assert_eq!(arena.blocks[idx].offset, 0);
        });
        assert_eq!(classify_heap_generation(base), HeapGeneration::Nursery);
        assert_eq!(
            classify_heap_generation(base + size - 1),
            HeapGeneration::Nursery
        );
    });
}

#[test]
fn generation_metadata_arena_reset_stats_reports_reusable_bytes_for_retained_reset_blocks() {
    run_with_fresh_arenas(|| {
        let (idx, _base, _size, before_offset, stats) = reset_single_reclaimable_nursery_block(0);
        assert_eq!(stats.reset_blocks, 1);
        assert_eq!(stats.reusable_bytes, before_offset);
        assert_eq!(stats.deallocated_blocks, 0);
        assert_eq!(stats.deallocated_bytes, 0);
        assert_eq!(stats.pooled_blocks, 0);
        assert_eq!(stats.pooled_bytes, 0);
        ARENA.with(|a| unsafe {
            let arena = &*a.get();
            assert!(!arena.blocks[idx].data.is_null());
            assert_eq!(arena.blocks[idx].offset, 0);
        });
    });
}

#[test]
fn generation_metadata_removed_on_nursery_block_deallocation() {
    run_with_fresh_arenas(|| {
        let (idx, base, _size, stats) = reset_old_nursery_block(1);
        assert!(
            stats.removed_blocks >= 1,
            "test setup should remove at least one nursery block"
        );
        ARENA.with(|a| unsafe {
            let arena = &*a.get();
            assert!(arena.blocks[idx].data.is_null());
            assert_eq!(arena.blocks[idx].size, 0);
        });
        assert_eq!(classify_heap_generation(base), HeapGeneration::Unknown);
        assert!(!pointer_in_nursery(base));
    });
}

#[test]
fn generation_metadata_arena_reset_stats_distinguishes_pooled_from_deallocated_blocks() {
    run_with_fresh_arenas(|| {
        let (idx, base, size, _before_offset, stats) = reset_single_reclaimable_nursery_block(1);
        assert_eq!(stats.reset_blocks, 1);
        assert_eq!(stats.reusable_bytes, 0);
        assert_eq!(stats.removed_blocks, 1);
        assert_eq!(stats.removed_bytes, size);
        assert_eq!(stats.pooled_blocks, 1);
        assert_eq!(stats.pooled_bytes, size);
        assert_eq!(stats.deallocated_blocks, 0);
        assert_eq!(stats.deallocated_bytes, 0);
        ARENA.with(|a| unsafe {
            let arena = &*a.get();
            assert!(arena.blocks[idx].data.is_null());
            assert_eq!(arena.blocks[idx].size, 0);
        });
        assert_eq!(classify_heap_generation(base), HeapGeneration::Unknown);
    });
}

#[test]
fn generation_metadata_registered_on_tombstone_reuse() {
    run_with_fresh_arenas(|| {
        let (idx, _base, _size, stats) = reset_old_nursery_block(1);
        assert!(
            stats.removed_blocks >= 1,
            "test setup should create a nursery tombstone"
        );

        let oversized = arena_alloc(BLOCK_SIZE + 64, 8) as usize;
        ARENA.with(|a| unsafe {
            let arena = &*a.get();
            assert!(!arena.blocks[idx].data.is_null());
            assert!(
                arena.blocks[idx].size > BLOCK_SIZE,
                "oversized allocation should replace the tombstone with a fresh block"
            );
        });
        assert_eq!(general_block_index_for(oversized), Some(idx));
        assert_eq!(classify_heap_generation(oversized), HeapGeneration::Nursery);
    });
}

/// Issue #179: a longlived-arena allocation must not land inside any
/// general-arena block. This is the architectural guarantee behind
/// the "segregated quarantine" design — GP blocks can be reset on
/// GC without touching cached object pointers, which stay parked in
/// longlived blocks.
#[test]
fn longlived_pointer_is_disjoint_from_general_blocks() {
    // Force a general-arena allocation first so block 0 exists.
    let gen_ptr = arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
    let ll_ptr = arena_alloc_gc_longlived(32, 8, GC_TYPE_STRING) as usize;

    // Collect general-arena block ranges.
    let mut general_ranges: Vec<(usize, usize)> = Vec::new();
    ARENA.with(|a| {
        let arena = unsafe { &*a.get() };
        for block in &arena.blocks {
            general_ranges.push((block.data as usize, block.size));
        }
    });

    let in_general = general_ranges
        .iter()
        .any(|&(base, size)| ll_ptr >= base && ll_ptr < base + size);
    assert!(
        !in_general,
        "longlived pointer {ll_ptr:#x} landed inside a general-arena block; \
         segregation is broken"
    );

    // Sanity: general allocation IS in a general block.
    let gen_in_general = general_ranges
        .iter()
        .any(|&(base, size)| gen_ptr >= base && gen_ptr < base + size);
    assert!(
        gen_in_general,
        "general alloc {gen_ptr:#x} not in any general block"
    );
}

#[test]
fn test_arena_reset_reuses_dead_general_block_without_touching_live_block() {
    let mut dead_blocks = Vec::new();

    for _ in 0..6 {
        let ptr = arena_alloc(BLOCK_SIZE, 8) as usize;
        let block_idx =
            general_block_index_for(ptr).expect("dead allocation should land in general arena");
        dead_blocks.push(block_idx);
    }

    dead_blocks.sort_unstable();
    dead_blocks.dedup();
    assert!(
        dead_blocks.len() >= 6,
        "test setup should force six distinct full general blocks"
    );

    let live_ptr = arena_alloc_gc(24, 8, GC_TYPE_STRING);
    let live_addr = live_ptr as usize;
    let live_header_addr = live_addr - GC_HEADER_SIZE;
    let live_block =
        general_block_index_for(live_addr).expect("live allocation should be in general arena");
    let current = ARENA.with(|a| unsafe { (&*a.get()).current });
    let keep_low = current.saturating_sub(4);
    let reset_candidate = dead_blocks
        .iter()
        .copied()
        .find(|&idx| idx < keep_low)
        .expect("test setup should leave at least one dead block outside the keep window");

    let before_offset = general_block_offset(reset_candidate);
    assert!(
        before_offset > 0,
        "reset candidate should contain dead allocations before reset"
    );

    unsafe {
        let header = (live_header_addr as *mut u8) as *mut GcHeader;
        (*header).gc_flags |= GC_FLAG_MARKED;
        *(live_ptr as *mut u64) = 0xCAFE_BABE_DEAD_BEEF;
        *(live_ptr.add(8) as *mut u64) = 0x1234_5678_9ABC_DEF0;
    }
    let live_header_size = unsafe { (*(live_header_addr as *const GcHeader)).size };

    ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        arena.blocks[reset_candidate].dead_cycles = 0;
        arena.blocks[live_block].dead_cycles = 0;
    });

    let mut block_has_live = vec![false; arena_block_count()];
    block_has_live[live_block] = true;
    arena_reset_empty_blocks(&block_has_live);

    assert_eq!(
        general_block_offset(reset_candidate),
        0,
        "dead general block should be reset for reuse"
    );
    assert!(
        general_block_offset(live_block) > 0,
        "live general block should keep its nonzero offset"
    );

    let blocks_after_reset = general_block_count();
    let _reused = arena_alloc_gc(24, 8, GC_TYPE_STRING);
    assert_eq!(
        general_block_count(),
        blocks_after_reset,
        "allocation after reset should reuse existing arena capacity"
    );

    unsafe {
        assert_eq!(*(live_ptr as *const u64), 0xCAFE_BABE_DEAD_BEEF);
        assert_eq!(*(live_ptr.add(8) as *const u64), 0x1234_5678_9ABC_DEF0);
        let header = (live_header_addr as *mut u8) as *mut GcHeader;
        assert_eq!((*header).obj_type, GC_TYPE_STRING);
        assert_eq!(
            (*header).size,
            live_header_size,
            "live header size should not change during reset"
        );
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
}

/// Walker + block-index contract: longlived objects get global
/// block indices at or above `general_block_count()`, so the
/// `arena_reset_empty_blocks` range check correctly skips them.
#[test]
fn longlived_walk_yields_indices_outside_general_range() {
    // Ensure each arena has at least one block with one allocation.
    let _g = arena_alloc_gc(16, 8, GC_TYPE_ARRAY) as usize;
    let ll = arena_alloc_gc_longlived(24, 8, GC_TYPE_STRING) as usize;

    let general_n = general_block_count();
    let mut seen_ll_idx: Option<usize> = None;
    arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let user_ptr = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) } as usize;
        if user_ptr == ll {
            seen_ll_idx = Some(block_idx);
        }
    });
    let idx = seen_ll_idx.expect("longlived allocation not visited by walker");
    assert!(
        idx >= general_n,
        "longlived block_idx {idx} must be ≥ general_block_count {general_n}"
    );
}

/// `arena_reset_empty_blocks` must never reset a longlived block,
/// even if its block-has-live slot is `false`. This is the load-
/// bearing correctness guarantee: cache-held pointers into the
/// longlived arena must survive GC cycles where the cache itself
/// is the only thing referencing them.
#[test]
fn reset_never_clears_longlived_blocks() {
    let ll = arena_alloc_gc_longlived(40, 8, GC_TYPE_STRING) as usize;
    let ll_header_in_block = {
        // The header sits GC_HEADER_SIZE before the user pointer;
        // use the user pointer for range comparison below.
        ll - GC_HEADER_SIZE
    };

    let n_blocks = arena_block_count();
    // Build a block_has_live where EVERY block is marked dead.
    let all_dead = vec![false; n_blocks];
    arena_reset_empty_blocks(&all_dead);

    // The longlived allocation must still be readable (its block
    // wasn't reset, so the bytes are still there).
    let mut found = false;
    LONGLIVED_ARENA.with(|a| {
        let arena = unsafe { &*a.get() };
        for block in &arena.blocks {
            let base = block.data as usize;
            if ll_header_in_block >= base && ll_header_in_block < base + block.size {
                // Block still has nonzero offset (not reset).
                assert!(
                    block.offset > 0,
                    "longlived block reset to offset=0 despite reset_empty_blocks guard"
                );
                found = true;
            }
        }
    });
    assert!(found, "longlived alloc not located in any longlived block");
}

/// Gen-GC Phase B: an old-gen allocation must not land inside
/// any general-arena (= nursery) block. Mirror of
/// `longlived_pointer_is_disjoint_from_general_blocks`.
#[test]
fn old_gen_pointer_is_disjoint_from_nursery_blocks() {
    let _gen_ptr = arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
    let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let old_header = old_ptr - GC_HEADER_SIZE;
    ARENA.with(|a| {
        let arena = unsafe { &*a.get() };
        for block in &arena.blocks {
            let base = block.data as usize;
            let end = base + block.size;
            assert!(
                old_header < base || old_header >= end,
                "old-gen alloc landed inside a nursery block (got {:x}, block [{:x}, {:x}))",
                old_header,
                base,
                end,
            );
        }
    });
}

/// Gen-GC Phase B: an old-gen allocation must not land inside
/// any longlived block either — three regions are pairwise
/// disjoint.
#[test]
fn old_gen_pointer_is_disjoint_from_longlived_blocks() {
    let _ll = arena_alloc_gc_longlived(40, 8, GC_TYPE_STRING) as usize;
    let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let old_header = old_ptr - GC_HEADER_SIZE;
    LONGLIVED_ARENA.with(|a| {
        let arena = unsafe { &*a.get() };
        for block in &arena.blocks {
            let base = block.data as usize;
            let end = base + block.size;
            assert!(
                old_header < base || old_header >= end,
                "old-gen alloc landed inside a longlived block",
            );
        }
    });
}

/// Gen-GC Phase B: walker must yield indices for old-gen
/// blocks at `>= longlived_end()`. Confirms the global block-
/// index plan: nursery first, then longlived, then old-gen.
#[test]
fn old_gen_walk_yields_indices_after_longlived() {
    let _gen = arena_alloc_gc(24, 8, GC_TYPE_STRING) as usize;
    let _ll = arena_alloc_gc_longlived(24, 8, GC_TYPE_STRING) as usize;
    let old_ptr = arena_alloc_gc_old(24, 8, GC_TYPE_STRING) as usize;
    let old_header = old_ptr - GC_HEADER_SIZE;
    let boundary = longlived_end();
    let mut found_at_idx: Option<usize> = None;
    arena_walk_objects_with_block_index(|hdr, block_idx| {
        if hdr as usize == old_header {
            found_at_idx = Some(block_idx);
        }
    });
    let idx = found_at_idx.expect("old-gen alloc not yielded by walker");
    assert!(
        idx >= boundary,
        "old-gen block index {} should be >= longlived_end() {}",
        idx,
        boundary,
    );
}

/// Gen-GC Phase B: arena_reset_empty_blocks must NEVER touch
/// an old-gen block, even when every general/longlived/old
/// block is marked dead. Promotion implies indefinite lifetime.
#[test]
fn reset_never_clears_old_gen_blocks() {
    let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let old_header = old_ptr - GC_HEADER_SIZE;
    let n_blocks = arena_block_count();
    let all_dead = vec![false; n_blocks];
    arena_reset_empty_blocks(&all_dead);
    let mut still_alive = false;
    OLD_ARENA.with(|a| {
        let arena = unsafe { &*a.get() };
        for block in &arena.blocks {
            let base = block.data as usize;
            if old_header >= base && old_header < base + block.size {
                assert!(
                    block.offset > 0,
                    "old-gen block reset to offset=0 despite reset guard",
                );
                still_alive = true;
            }
        }
    });
    assert!(
        still_alive,
        "old-gen alloc not located in any old-gen block"
    );
}

#[test]
fn old_arena_block_reuse_does_not_repoint_eden_inline_state() {
    // #1824 regression. `Arena::alloc`'s block-reuse forward-scan calls
    // `resync_inline_to_current`, which mirrors the codegen inline
    // bump-allocator's `INLINE_STATE`. `INLINE_STATE` must track ONLY the
    // general nursery-Eden arena. A non-Eden (old-gen / survivor) allocation
    // that forward-scans to reuse an earlier block must NOT repoint
    // `INLINE_STATE`: doing so pointed it at a foreign block, and the next
    // Eden `arena_alloc` then wrote that block's offset into the live Eden
    // block — rewinding the bump pointer so a fresh string allocation
    // overwrote a still-live suspended async-step closure (read back later as
    // a garbage function pointer → SIGSEGV during the await continuation).
    run_with_fresh_arenas(|| {
        // Initialize INLINE_STATE from the Eden arena and capture it.
        let _ = js_inline_arena_state();
        let _ = arena_alloc(64, 8); // make sure inline.data is live
        let (eden_data, eden_size) = INLINE_STATE.with(|s| {
            let st = unsafe { &*s.get() };
            (st.data, st.size)
        });
        assert!(
            !eden_data.is_null(),
            "Eden INLINE_STATE should be initialized"
        );

        // Drive the OLD_ARENA into a forward-scan-reuse state: a reusable
        // earlier block (offset 0) plus a full current block. The old
        // arena starts with a lazy tombstone, so materialize block 0
        // with a real allocation first.
        OLD_ARENA.with(|a| unsafe {
            let arena = &mut *a.get();
            let _ = arena.alloc(64, 8); // materialize block 0
            assert_eq!(arena.current, 0, "first old alloc should fill slot 0");
            arena.install_fresh_block(BLOCK_SIZE); // >=2 blocks, current = newest
            let cur = arena.current;
            assert!(cur > 0, "fresh block should advance current past block 0");
            arena.blocks[cur].offset = arena.blocks[cur].size; // current is full
            arena.blocks[0].offset = 0; // block 0 reusable
        });
        // The hand-mutated offsets above bypassed the tracked alloc
        // paths — resync the delta-maintained old-gen in-use cache so
        // the debug cross-check in `old_gen_in_use_bytes()` (reached
        // via the next alloc's gc_check_trigger) sees a consistent pair.
        old_gen_in_use_bytes_resync();
        OLD_ARENA.with(|a| unsafe {
            let arena = &mut *a.get();
            // Current full → forward-scan reuses block 0 → current = 0 →
            // resync_inline_to_current(OLD_ARENA).
            let _ = arena.alloc(64, 8);
            assert_eq!(arena.current, 0, "forward-scan should have reused block 0");
        });

        // The OLD_ARENA block reuse must have left Eden's INLINE_STATE intact.
        let (after_data, after_size) = INLINE_STATE.with(|s| {
            let st = unsafe { &*s.get() };
            (st.data, st.size)
        });
        assert_eq!(
            after_data, eden_data,
            "old-gen block reuse must not repoint Eden INLINE_STATE.data (#1824)"
        );
        assert_eq!(
            after_size, eden_size,
            "Eden INLINE_STATE.size must be intact"
        );
    });
}

/// The survivor/longlived/old arenas start with a lazy tombstone block:
/// a fresh JS-touching thread pays only Eden's eager 1 MB, and each
/// lazy region materializes its first real block on first allocation.
#[test]
fn lazy_regions_defer_initial_block_allocation() {
    run_with_fresh_arenas(|| {
        let before = arena_telemetry_snapshot();
        assert!(
            before.arena.reserved_bytes >= BLOCK_SIZE,
            "Eden must stay eagerly materialized for the inline allocator"
        );
        assert_eq!(before.survivor0.reserved_bytes, 0);
        assert_eq!(before.survivor1.reserved_bytes, 0);
        assert_eq!(before.longlived.reserved_bytes, 0);
        assert_eq!(before.old.reserved_bytes, 0);
        assert_eq!(old_gen_in_use_bytes(), 0);

        // First allocation in each lazy region materializes its block.
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING);
        assert!(!old_ptr.is_null());
        let ll_ptr = arena_alloc_gc_longlived(40, 8, GC_TYPE_STRING);
        assert!(!ll_ptr.is_null());
        let survivor_ptr = arena_alloc_gc_survivor(40, 8, GC_TYPE_STRING);
        assert!(!survivor_ptr.is_null());

        let after = arena_telemetry_snapshot();
        assert!(after.old.reserved_bytes >= BLOCK_SIZE);
        assert!(after.longlived.reserved_bytes >= BLOCK_SIZE);
        assert!(
            after.survivor0.reserved_bytes >= BLOCK_SIZE
                || after.survivor1.reserved_bytes >= BLOCK_SIZE,
            "the inactive survivor semispace should have materialized"
        );
        assert!(after.old.in_use_bytes > 0);
        assert_eq!(
            old_gen_in_use_bytes(),
            old_gen_in_use_bytes_recomputed(),
            "cached old-gen in-use bytes must match the recompute after lazy materialization"
        );
    });
}

// ---------------------------------------------------------------------------
// #7022: the allocation-point GC trigger must not run under a live `&mut Arena`
// borrow.
//
// `gc_check_trigger()` collects, and a collection ALLOCATES INTO THE ARENAS:
// promotion and C4b evacuation call `arena_alloc_gc_old`, an evacuating minor
// (#7019, default-on) fills a survivor semispace, and either can reach
// `Arena::install_fresh_block` → `self.blocks.push(..)` on the *same* arena the
// allocating frame is holding. A `Vec` growth there frees the buffer the outer
// frame then indexes, and `&mut` carries `noalias`, so the outer frame is also
// entitled to have cached `blocks.ptr`/`len` across the call.
//
// The two tests below pin the split that removes the hazard: the trigger lives
// in `arena_cell_alloc` (raw pointer, borrows re-derived per statement) and NOT
// in `Arena::alloc` (`&mut self`).
// ---------------------------------------------------------------------------

#[test]
fn allocation_point_gc_trigger_runs_with_no_live_arena_borrow() {
    reset_gc_trigger_arena_probe();
    // Larger than any existing block, so the current-block fast path must miss
    // and the slow path (the one that collects) is guaranteed to run.
    let ptr = OLD_ARENA.with(|a| unsafe { arena_cell_alloc(a.get(), BLOCK_SIZE + 1, 8) });
    assert!(!ptr.is_null(), "forced slow-path old-gen allocation failed");
    assert!(
        gc_trigger_arena_calls() > 0,
        "the forced slow path must reach the allocation-point GC trigger; \
         without it this test asserts nothing"
    );
    assert_eq!(
        gc_trigger_arena_borrow_depth(),
        0,
        "gc_check_trigger() ran while an `&mut Arena` borrow was live — the \
         collector allocates into this same arena and may reallocate its \
         `blocks` Vec underneath the borrow (#7022)"
    );
}

#[test]
fn raw_arena_alloc_method_never_reaches_the_gc_trigger() {
    reset_gc_trigger_arena_probe();
    let ptr = OLD_ARENA.with(|a| unsafe { (*a.get()).alloc(BLOCK_SIZE + 1, 8) });
    assert!(!ptr.is_null(), "forced slow-path old-gen allocation failed");
    assert_eq!(
        gc_trigger_arena_calls(),
        0,
        "`Arena::alloc(&mut self, ..)` must stay collection-free: it is called \
         with a live borrow, so a GC trigger inside it re-enters the arena \
         (#7022). The trigger belongs in `arena_cell_alloc`."
    );
}

#[test]
fn emergency_block_reclaim_runs_with_no_live_arena_borrow() {
    // The out-of-memory path is the second place a trigger can fire from an
    // arena allocation: `reserve_arena_block` runs `gc_try_emergency_reclaim()`
    // when the OS refuses memory, and that collection allocates into the arenas
    // exactly like `gc_check_trigger` does. It gets the same rule. Driven here
    // by a one-shot injected allocation failure rather than real heap
    // exhaustion, so the invariant is asserted rather than assumed.
    reset_gc_trigger_arena_probe();
    force_next_block_alloc_failure();
    // Bigger than any existing block, so a fresh block — and therefore
    // `reserve_arena_block` — is unavoidable.
    let ptr = OLD_ARENA.with(|a| unsafe { arena_cell_alloc(a.get(), BLOCK_SIZE + 1, 8) });
    assert!(
        !ptr.is_null(),
        "the emergency retry must still hand back a usable block"
    );
    assert!(
        gc_trigger_arena_calls() >= 2,
        "the run must have reached BOTH the allocation-point trigger and the \
         emergency reclaim ({} trigger(s) seen); without the second one this \
         test asserts nothing",
        gc_trigger_arena_calls()
    );
    assert_eq!(
        gc_trigger_arena_borrow_depth(),
        0,
        "gc_try_emergency_reclaim() ran while an `&mut Arena` borrow was live — \
         the emergency collection allocates into this same arena and may \
         reallocate its `blocks` Vec underneath the borrow (#7022)"
    );
}

/// #7438: a released block offered to the recycled-block pool is handed back
/// by the next same-size block reservation instead of a fresh allocator
/// mapping — the mechanism that bounds ever-dirtied pages at the concurrent
/// high-water instead of cumulative promotion volume.
#[test]
fn recycled_block_pool_reuses_released_blocks() {
    let layout = std::alloc::Layout::from_size_align(BLOCK_SIZE, 16).unwrap();
    let raw = unsafe { std::alloc::alloc(layout) };
    assert!(!raw.is_null());
    let before = block_pool_bytes_for_test();
    assert!(
        block_pool_put(raw, BLOCK_SIZE),
        "pool must accept a block under its cap"
    );
    assert_eq!(block_pool_bytes_for_test(), before + BLOCK_SIZE);

    // The reservation funnel must serve the pooled block back (LIFO) rather
    // than minting a fresh mapping.
    let block = crate::arena::block::reserve_arena_block(BLOCK_SIZE / 2);
    assert_eq!(
        block.data as usize, raw as usize,
        "same-size reservation must reuse the pooled block"
    );
    assert_eq!(block.size, BLOCK_SIZE);
    assert_eq!(block.offset, 0);
    assert_eq!(block_pool_bytes_for_test(), before);
    // Hand it back to the allocator so the test doesn't leak the mapping.
    unsafe { std::alloc::dealloc(block.data, layout) };
}

/// A thread exiting with a non-empty pool must run the pool's own `Drop`
/// rather than stranding its blocks. Before the `BlockPool` newtype the
/// thread-local held a bare `Vec<(*mut u8, usize)>`, so the TLS destructor
/// freed the Vec's buffer and leaked every block it pointed at — up to
/// `BLOCK_POOL_CAP_BYTES` per exiting thread, which `perry/thread`'s
/// `spawn`/`parallelMap` create routinely.
///
/// The dealloc itself is `cfg!(test)`-skipped (#4665, exactly as in
/// `Arena::drop`), so this asserts the destructor RUNS and that pools are
/// per-thread; it cannot observe the free. Its value is that a regression to
/// a bare `Vec` — or a drain called from another TLS destructor, whose
/// ordering is unspecified — still has to keep this path alive.
#[test]
fn block_pool_is_per_thread_and_drops_with_its_thread() {
    let before = block_pool_bytes_for_test();
    let handle = std::thread::spawn(|| {
        // Fresh thread => fresh pool.
        assert_eq!(block_pool_bytes_for_test(), 0);
        let layout = std::alloc::Layout::from_size_align(BLOCK_SIZE, 16).unwrap();
        let raw = unsafe { std::alloc::alloc(layout) };
        assert!(!raw.is_null());
        assert!(
            block_pool_put(raw, BLOCK_SIZE),
            "pool should accept the block"
        );
        assert_eq!(block_pool_bytes_for_test(), BLOCK_SIZE);
        // Thread exits here with a non-empty pool: BlockPool::drop must run.
    });
    handle
        .join()
        .expect("spawned thread must exit cleanly, not double-free");
    // The other thread's pool never touched ours.
    assert_eq!(block_pool_bytes_for_test(), before);
}

/// #7875: per-thread LIFO ownership must not multiply the allowance by the
/// number of simultaneously-live `perry/thread` agents. Four threads race to
/// reserve 8 MiB against the same process counter under a 2 MiB cap; the
/// census reaches the cap, never four copies of it, and returns to zero after
/// the simulated owners release their shares. The production wrapper passes
/// `BLOCK_POOL_PROCESS_BYTES` to this exact reservation primitive.
#[test]
fn block_pool_cap_is_process_wide_across_live_threads() {
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ready = std::sync::Arc::new(std::sync::Barrier::new(5));
    let release = std::sync::Arc::new(std::sync::Barrier::new(5));
    let mut threads = Vec::new();

    for _ in 0..4 {
        let counter = counter.clone();
        let ready = ready.clone();
        let release = release.clone();
        threads.push(std::thread::spawn(move || {
            let mut reserved = 0;
            for _ in 0..2 {
                if super::block::block_pool_counter_try_reserve(
                    &counter,
                    BLOCK_SIZE,
                    2 * BLOCK_SIZE,
                ) {
                    reserved += BLOCK_SIZE;
                }
            }
            ready.wait();
            release.wait();
            counter.fetch_sub(reserved, std::sync::atomic::Ordering::Relaxed);
        }));
    }

    ready.wait();
    assert_eq!(
        counter.load(std::sync::atomic::Ordering::Relaxed),
        2 * BLOCK_SIZE,
        "all live threads together must share one process-wide cap"
    );
    release.wait();
    for thread in threads {
        thread.join().expect("pool worker must exit cleanly");
    }
    assert_eq!(
        counter.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "each owner must release its share of the process census"
    );
}

#[test]
fn allocation_failure_recovery_drains_mismatched_pooled_blocks() {
    let layout = std::alloc::Layout::from_size_align(BLOCK_SIZE, 16).unwrap();
    let raw = unsafe { std::alloc::alloc(layout) };
    assert!(!raw.is_null());
    assert!(block_pool_put(raw, BLOCK_SIZE));

    force_next_block_alloc_failure();
    let block = crate::arena::block::reserve_arena_block(BLOCK_SIZE + 1);
    assert_eq!(
        block_pool_bytes_for_test(),
        0,
        "emergency full collection must drain blocks unusable for the failed size"
    );

    let returned_layout = std::alloc::Layout::from_size_align(block.size, 16).unwrap();
    unsafe { std::alloc::dealloc(block.data, returned_layout) };
}

// ---------------------------------------------------------------------------
// #7624: deferred old-object page registration.
//
// `arena_alloc_gc_old` records its page registration in a thread-local buffer
// instead of folding it into `OLD_GEN_PAGE_OBJECTS`/`OLD_GEN_PAGE_META` on the
// spot. The deferral is only invisible if EVERY reader and EVERY remover of
// those two tables flushes first, so that is what these pin — one test per
// obligation, each written so that deleting the corresponding
// `flush_deferred_old_page_registrations()` call turns it red.
// ---------------------------------------------------------------------------

/// The rule this whole family enforces, made checkable rather than remembered.
///
/// The per-obligation tests below each pin ONE flush site, which is the right
/// shape for the sites that exist today — but they are blind to a site that
/// does not exist yet. A future edit that adds a function touching either table
/// gets no test, and the deferral silently starts being visible to it. This
/// closes that: both tables are thread-locals private to `page_meta.rs`, so the
/// toucher set is enumerable from the source, and every toucher must either
/// flush or appear below with a reason.
///
/// A name in `EXEMPT` that no longer touches either table also fails, so a
/// removed function cannot leave a stale exemption behind (the shape
/// `gc_root_dominance_allowlist.json` uses).
#[test]
fn deferred_registration_flush_sites() {
    // Every exemption is a claim about why the deferral cannot be observed.
    const EXEMPT: &[(&str, &str)] = &[
        (
            "register_old_block_pages",
            "creates zeroed per-page META entries when a BLOCK is registered; \
             reads no counter the deferral owes",
        ),
        (
            "update_old_page_meta_for_object",
            "the flush's own target — it is what applies the batch",
        ),
        (
            "register_old_object_pages",
            "the eager path itself; the flush calls its logic, and \
             arena_alloc_gc_old_excluding_pages still calls it directly",
        ),
        (
            "old_page_account_swept_object",
            "per-object sweep writer. It calls refresh_policy_bits, which reads \
             allocated_bytes, but the flush refreshes every page it touches and \
             every READER flushes first, so no reader can observe a stale bit. \
             Kept flush-free so the sweep path pays nothing",
        ),
        (
            "old_page_account_promoted_object",
            "as old_page_account_swept_object — per-object, same argument",
        ),
        (
            "old_page_account_dirty_slots",
            "touches only dirty_slots/epoch, which no registration contributes to. \
             The batched form of the above: the dirty scan walks ascending \
             contiguous slots, so ~512 of them share one page and one probe",
        ),
        (
            "old_page_mark_dirty",
            "per-store barrier path; asks only whether a META entry exists, and \
             entries are created per page at BLOCK registration, not per object",
        ),
        (
            "old_page_clear_dirty",
            "as old_page_mark_dirty — the dirty bit only",
        ),
        (
            "next",
            "OldArenaPageObjectCursor::next. `new` flushes and the budgeted \
             stepping window marks without allocating into old-gen, so the \
             buffer cannot re-fill mid-walk; `next` debug-asserts exactly that \
             rather than paying a thread-local read per object",
        ),
        (
            "old_arena_page_index_clear_for_tests",
            "DISCARDS the buffer instead: a caller asking for an empty index \
             must not get a repopulated one",
        ),
        ("defer_old_object_page_registration", "the producer"),
        (
            "register_promoted_page_run",
            "#7742: called once per PAGE from `finish_in_place_promotion`'s \
             single linear walk of a promoted block, which flushes once before \
             the whole walk. Flushing per call would be the same flush repeated \
             256 times per 1 MiB block — and cannot be needed, because nothing \
             between the walk's start and its end allocates into old-gen",
        ),
        (
            "register_promoted_page_headers",
            "the TRACED promotion's eager arm, split out of \
             register_promoted_page_run. Same argument: one call per PAGE from \
             `finish_in_place_promotion`'s single linear walk, which flushes \
             once before the whole walk, and nothing between the walk's start \
             and its end allocates into old-gen",
        ),
        (
            "expand_promoted_run",
            "expands a DESCRIBED promoted page into the object list. Every \
             caller has already flushed: the four readers/removers do so as \
             their #7624 obligation, `materialize_all_promoted_page_runs` runs \
             immediately after `old_pages_begin_gc_cycle`, and \
             `register_promoted_page_run` is inside the promotion walk covered \
             by the entry above",
        ),
        (
            "flush_deferred_old_page_registrations",
            "the flush entry point",
        ),
        (
            "flush_deferred_old_page_registrations_batch",
            "the flush body",
        ),
        (
            "deferred_old_page_registrations_len",
            "test-only observer of the buffer, not of either table",
        ),
    ];

    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/arena/page_meta.rs"),
    )
    .expect("page_meta.rs must be readable");

    // Split into function bodies by tracking `fn <name>` headers at any indent.
    let mut current: Option<String> = None;
    let mut bodies: Vec<(String, String)> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("pub(crate) fn ")
            .or_else(|| trimmed.strip_prefix("pub fn "))
            .or_else(|| trimmed.strip_prefix("fn "))
        {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            current = Some(name.clone());
            bodies.push((name, String::new()));
        }
        if current.is_some() {
            if let Some(last) = bodies.last_mut() {
                last.1.push_str(line);
                last.1.push('\n');
            }
        }
    }

    let touches = |body: &str| {
        body.contains("OLD_GEN_PAGE_OBJECTS.with") || body.contains("OLD_GEN_PAGE_META.with")
    };
    let exempt_names: Vec<&str> = EXEMPT.iter().map(|(n, _)| *n).collect();

    let mut offenders = Vec::new();
    let mut touching = std::collections::BTreeSet::new();
    for (name, body) in &bodies {
        if !touches(body) {
            continue;
        }
        touching.insert(name.clone());
        if body.contains("flush_deferred_old_page_registrations()") {
            continue;
        }
        if exempt_names.contains(&name.as_str()) {
            continue;
        }
        offenders.push(name.clone());
    }

    assert!(
        offenders.is_empty(),
        "these functions in arena/page_meta.rs read or mutate OLD_GEN_PAGE_OBJECTS / \
         OLD_GEN_PAGE_META without first calling flush_deferred_old_page_registrations(), \
         and are not listed as exempt: {offenders:?}.\n\
         A deferred registration is invisible to a reader that does not flush, and a \
         REMOVER that does not flush is worse — the removal no-ops and the later flush \
         resurrects the dead entry. Add the flush, or add the function to EXEMPT with \
         the argument for why the deferral cannot be observed there (#7624)."
    );

    // Stale exemptions fail too, so this list cannot rot into suppression.
    let stale: Vec<&str> = exempt_names
        .iter()
        .copied()
        .filter(|n| {
            !touching.contains(*n)
                && !matches!(
                    *n,
                    "defer_old_object_page_registration"
                        | "flush_deferred_old_page_registrations"
                        | "flush_deferred_old_page_registrations_batch"
                        | "deferred_old_page_registrations_len"
                        | "old_arena_page_index_clear_for_tests"
                )
        })
        .collect();
    assert!(
        stale.is_empty(),
        "EXEMPT names nothing that touches either table any more: {stale:?}. \
         Delete the entry (#7624)."
    );

    // And the gate must be looking at something.
    assert!(
        touching.len() >= 10,
        "only found {} functions touching the page tables — the parser above has \
         probably stopped matching, which would make this gate vacuous",
        touching.len()
    );
}

/// A synthetic old-gen block plus `count` distinct in-range header addresses.
/// Registration never dereferences a header, so fabricated addresses exercise
/// the bookkeeping exactly as real ones do — and keep the test independent of
/// how many objects an allocator happens to fit in a page.
fn synthetic_old_headers(count: usize) -> Vec<usize> {
    let (base, min_size) = synthetic_old_block_range();
    let size = (count * 64)
        .next_multiple_of(GENERATION_PAGE_SIZE)
        .max(min_size);
    register_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
    (0..count).map(|i| base + i * 64).collect()
}

fn page_object_count(page: usize) -> usize {
    old_page_meta_for_tests(page)
        .map(|meta| meta.object_count)
        .unwrap_or(0)
}

#[test]
fn old_gen_birth_defers_its_page_registration() {
    run_with_fresh_arenas(|| {
        assert_eq!(deferred_old_page_registrations_len(), 0);
        let _old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        assert!(
            deferred_old_page_registrations_len() > 0,
            "arena_alloc_gc_old must defer, not register eagerly — otherwise \
             the change is inert and every measurement of it is vacuous"
        );
    });
}

#[test]
fn cycle_start_flushes_deferred_registrations() {
    run_with_fresh_arenas(|| {
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let (header_addr, total_size) = old_header_and_size(old_ptr);
        assert!(deferred_old_page_registrations_len() > 0);

        // The single flush point all three cycle constructors route through.
        old_pages_begin_gc_cycle();

        assert_eq!(
            deferred_old_page_registrations_len(),
            0,
            "old_pages_begin_gc_cycle must leave the deferral buffer empty"
        );
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        for (page, _) in old_object_page_overlaps(header_addr, total_size) {
            pages.insert(page);
        }
        let mut visited = Vec::new();
        old_arena_walk_objects_on_pages(&pages, |header| visited.push(header as usize));
        assert_seen_headers("post-cycle-start walk", &visited, &[header_addr]);
    });
}

/// The other half of the cycle-constructor claim. `cycle_start_flushes_...`
/// proves `old_pages_begin_gc_cycle` flushes; this proves each of the three
/// constructors actually calls it, which is what makes "every collection begins
/// with a complete index" true rather than merely asserted in a comment.
#[test]
fn every_cycle_constructor_routes_through_the_flush_point() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gc");
    for (file, what) in [
        ("mod.rs", "non-moving / copying minor"),
        ("cycle.rs", "full mark-sweep (GcCycleState::new_full)"),
        ("policy.rs", "budgeted minor"),
    ] {
        let src = std::fs::read_to_string(root.join(file))
            .unwrap_or_else(|e| panic!("cannot read gc/{file}: {e}"));
        assert!(
            src.contains("old_pages_begin_gc_cycle()"),
            "the {what} constructor in gc/{file} no longer calls \
             old_pages_begin_gc_cycle(); deferred old-page registrations would \
             survive into the cycle unflushed (#7624)"
        );
    }
}

#[test]
fn deferral_buffer_flushes_at_its_size_cap() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(DEFERRED_OLD_PAGE_REGISTRATION_CAP);
        for &header in &headers {
            defer_old_object_page_registration(header, 64);
        }
        assert_eq!(
            deferred_old_page_registrations_len(),
            0,
            "the buffer must self-flush at DEFERRED_OLD_PAGE_REGISTRATION_CAP \
             so it cannot grow without bound between collections"
        );
        // And the cap flush is a real registration, not a discard.
        assert!(page_object_count(generation_page_for_addr(headers[0])) > 0);
    });
}

/// Each reader of the two tables, one obligation per assertion. Delete any one
/// `flush_deferred_old_page_registrations()` in `page_meta.rs` and exactly one
/// of these goes red.
#[test]
fn every_index_reader_flushes_before_reading() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(4);
        let page = generation_page_for_addr(headers[0]);
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(page);

        // 1. old_arena_walk_objects_on_pages
        defer_old_object_page_registration(headers[0], 64);
        let mut visited = Vec::new();
        old_arena_walk_objects_on_pages(&pages, |h| visited.push(h as usize));
        assert_seen_headers("old_arena_walk_objects_on_pages", &visited, &[headers[0]]);
        assert_eq!(deferred_old_page_registrations_len(), 0);

        // 2. OldArenaPageObjectCursor — same index, incremental reader.
        defer_old_object_page_registration(headers[1], 64);
        let mut cursor = OldArenaPageObjectCursor::new(&pages);
        assert_eq!(
            deferred_old_page_registrations_len(),
            0,
            "OldArenaPageObjectCursor::new must flush before it starts stepping"
        );
        let mut seen = Vec::new();
        while let Some(h) = cursor.next() {
            seen.push(h);
        }
        assert_seen_headers("OldArenaPageObjectCursor", &seen, &headers[..2]);

        // 3. old_page_summary (OLD_GEN_PAGE_META)
        let before = old_page_summary().object_count;
        defer_old_object_page_registration(headers[2], 64);
        assert_eq!(
            old_page_summary().object_count,
            before + 1,
            "old_page_summary must flush; a mid-cycle promotion burst would \
             otherwise be missing from allocated_bytes/object_count"
        );

        // 4. old_page_meta_snapshot — drives defrag page selection.
        defer_old_object_page_registration(headers[3], 64);
        let snapshot = old_page_meta_snapshot();
        assert_eq!(deferred_old_page_registrations_len(), 0);
        let page_base = generation_page_base(page);
        let meta = snapshot
            .iter()
            .find(|m| m.page_base == page_base)
            .expect("snapshot should carry the page");
        assert_eq!(meta.object_count, 4);
    });
}

/// The remover obligation, and the one that is easiest to get wrong: a removal
/// that runs while the object is still only DEFERRED is a no-op, and the later
/// flush then puts the dead object back. Registration ORDER, not just eventual
/// visibility, is what the flush-before-remove rule buys.
#[test]
fn removing_a_deferred_object_does_not_resurrect_it() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(3);
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(generation_page_for_addr(headers[0]));

        let visited_now = |pages: &crate::fast_hash::PtrHashSet<usize>| {
            let mut visited = Vec::new();
            old_arena_walk_objects_on_pages(pages, |h| visited.push(h as usize));
            visited
        };

        // 1. unregister_old_object_pages
        defer_old_object_page_registration(headers[0], 64);
        unregister_old_object_pages(headers[0], 64);
        assert!(
            !visited_now(&pages).contains(&headers[0]),
            "a deferred entry removed before its flush was resurrected by the \
             flush — unregister_old_object_pages must flush first (#7624)"
        );

        // 2. old_arena_page_index_remove_object
        defer_old_object_page_registration(headers[1], 64);
        old_arena_page_index_remove_object(headers[1], 64);
        assert!(
            !visited_now(&pages).contains(&headers[1]),
            "old_arena_page_index_remove_object must flush first (#7624)"
        );

        // 3. unregister_old_block_pages — the whole page goes away, and a
        //    later flush must not recreate it pointing into a recycled block.
        defer_old_object_page_registration(headers[2], 64);
        unregister_old_block_pages(&[generation_page_for_addr(headers[2])]);
        assert!(
            !visited_now(&pages).contains(&headers[2]),
            "unregister_old_block_pages must flush first (#7624)"
        );
    });
}

/// The batched flush skips the dedup scan over entries added within the same
/// batch. That is only sound if it still catches the case the dedup exists for:
/// hole reuse handing back an address registered BEFORE the batch.
#[test]
fn batched_flush_matches_eager_registration() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(64);
        let page = generation_page_for_addr(headers[0]);

        // A pre-existing (pre-batch) registration, as hole reuse would leave.
        register_old_object_pages(headers[0], 64);
        assert_eq!(page_object_count(page), 1);

        // Now defer the whole set INCLUDING the already-registered address.
        for &header in &headers {
            defer_old_object_page_registration(header, 64);
        }
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(page);
        let mut visited = Vec::new();
        old_arena_walk_objects_on_pages(&pages, |h| visited.push(h as usize));

        visited.sort_unstable();
        let mut expected = headers.clone();
        expected.sort_unstable();
        assert_eq!(
            visited, expected,
            "batched flush must produce exactly the eager index — no duplicate \
             for the re-registered address, no dropped entry"
        );
        assert_eq!(
            page_object_count(page),
            headers.len(),
            "page object_count must match the eager path's, counting the \
             re-registered address exactly once"
        );
    });
}
