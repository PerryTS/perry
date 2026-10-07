//! Oversized old object/byte backing must never be selected for relocation.
//! Real arena allocation supplies the ranges; explicit sweep accounting makes
//! each range fragmented without relying on collector scheduling.
use super::super::types::*;
use super::*;

#[test]
fn old_defrag_and_idle_compaction_never_select_large_or_byte_extents() {
    std::thread::spawn(|| unsafe {
        let _triggers =
            super::super::tests::support::GcTriggerThresholdTestGuard::suppress_automatic_triggers(
            );
        let mut live = Vec::new();
        let mut dead = Vec::new();
        live.push(crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize);
        dead.push(crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize);
        let mut extents = Vec::new();
        for obj_type in [GC_TYPE_STRING, GC_TYPE_BUFFER_ARRAY_BUFFER] {
            let big = crate::arena::arena_alloc_gc(3 * crate::arena::BLOCK_SIZE + 64, 8, obj_type)
                as usize;
            assert!(crate::arena::pointer_in_old_gen(big));
            extents.push(big);
            live.push(big);
            live.push(crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize);
            dead.push(crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize);
        }
        crate::arena::old_pages_reset_sweep_accounting();
        for (objects, is_live) in [(&live, true), (&dead, false)] {
            for &user in objects {
                let header = user - GC_HEADER_SIZE;
                let total = (*(header as *const GcHeader)).size as usize;
                crate::arena::old_page_account_swept_object(header, total, is_live, false);
            }
        }
        let snapshot = crate::arena::old_page_meta_snapshot();
        let ranges = crate::arena::old_arena_block_ranges();
        let large_ranges: Vec<_> = extents
            .iter()
            .map(|&user| {
                let idx = crate::arena::old_arena_block_range_index(&ranges, user).unwrap();
                let range = ranges[idx];
                assert!(range.3 > crate::arena::BLOCK_SIZE);
                assert!(
                    snapshot
                        .iter()
                        .any(|m| (range.0..range.1).contains(&m.page_base)
                            && m.live_bytes > 0
                            && m.dead_bytes > 0
                            && m.pinned_bytes == 0),
                    "LIVE SUBJECT: fragmented unpinned oversized extent"
                );
                range
            })
            .collect();
        for selection in [
            select_old_page_defrag_pages_from_snapshot(&snapshot, true),
            select_whole_blocks(&snapshot, OldPageDefragSelection::default()),
        ] {
            assert!(
                selection.selected_pages > 0,
                "LIVE SUBJECT: ordinary old block selected"
            );
            for range in &large_ranges {
                assert!(
                    selection.pages.iter().all(|&page| {
                        !(range.0..range.1).contains(&crate::arena::generation_page_base(page))
                    }),
                    "large/byte extent selected: {range:?}"
                );
            }
        }
        // Restore the old selector's missing extent guard. Both selection
        // paths must now select a forbidden extent, proving this witness has
        // teeth independently of the relocation type check downstream.
        let _sabotage = extent_selection_sabotage::Guard::arm();
        for selection in [
            select_old_page_defrag_pages_from_snapshot(&snapshot, true),
            select_whole_blocks(&snapshot, OldPageDefragSelection::default()),
        ] {
            assert!(
                large_ranges
                    .iter()
                    .any(|range| selection.pages.iter().any(|&page| {
                        (range.0..range.1).contains(&crate::arena::generation_page_base(page))
                    })),
                "SABOTAGE: missing extent guard must select large backing"
            );
        }
    })
    .join()
    .expect("extent selection test panicked");
}
