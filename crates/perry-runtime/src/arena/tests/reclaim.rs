use super::*;

#[test]
pub(super) fn object_start_bitmap_stamps_only_maps_and_clears_on_reset() {
    run_with_fresh_arenas(|| {
        let unstamped_array = arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as usize;
        let (_, array_base, array_bitmap) = classify_heap_space_in_range(unstamped_array)
            .expect("allocation must be in an arena range");
        assert!(
            !arena_header_is_object_start(
                unstamped_array - GC_HEADER_SIZE,
                array_base,
                array_bitmap,
            ),
            "ordinary arena allocations must not pay to stamp exact starts"
        );

        let user = arena_alloc_gc(16, 8, GC_TYPE_MAP) as usize;
        let header = user - GC_HEADER_SIZE;
        let (_, range_base, bitmap) =
            classify_heap_space_in_range(user).expect("allocation must be in an arena range");

        assert!(arena_header_is_object_start(header, range_base, bitmap));
        assert!(
            !arena_header_is_object_start(header + 8, range_base, bitmap),
            "an aligned payload word must not be recorded as an object start"
        );

        arena_reset_all_blocks_to_zero();
        let (_, reset_base, reset_bitmap) =
            classify_heap_space_in_range(user).expect("reset retains the arena block registration");
        assert!(
            !arena_header_is_object_start(header, reset_base, reset_bitmap),
            "reset must clear stale object boundaries before block reuse"
        );
    });
}

#[test]
pub(super) fn survivor_reclaim_resets_dead_blocks() {
    run_with_fresh_arenas(|| {
        let _heap_change = crate::gc::heap_generation::HeapChange::begin(
            crate::gc::heap_generation::HeapChangeKind::Sweep,
        );
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
pub(super) fn budgeted_survivor_reclaim_accumulates_release_stats_across_slices() {
    run_with_fresh_arenas(|| {
        let _heap_change = crate::gc::heap_generation::HeapChange::begin(
            crate::gc::heap_generation::HeapChangeKind::Sweep,
        );
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
