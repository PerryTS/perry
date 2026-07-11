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

fn run_with_fresh_arenas(test: impl FnOnce() + Send + 'static) {
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
        assert!(stats.reusable_bytes > 0 || stats.deallocated_bytes > 0);
        assert!(
            after_reclaim.total_reserved_bytes <= after_alloc.total_reserved_bytes,
            "dead survivor blocks should become reusable or be returned"
        );
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
            stats.deallocated_blocks >= 1,
            "test setup should deallocate at least one nursery block"
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
fn generation_metadata_arena_reset_stats_reports_deallocated_blocks_as_returned_not_reusable() {
    run_with_fresh_arenas(|| {
        let (idx, base, size, _before_offset, stats) = reset_single_reclaimable_nursery_block(1);
        assert_eq!(stats.reset_blocks, 1);
        assert_eq!(stats.reusable_bytes, 0);
        assert_eq!(stats.deallocated_blocks, 1);
        assert_eq!(stats.deallocated_bytes, size);
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
            stats.deallocated_blocks >= 1,
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
