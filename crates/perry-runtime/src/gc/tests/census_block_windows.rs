//! #10182: census membership finds a pointer's block through a direct-mapped
//! index of 1 MiB windows instead of a binary search over the block fences.
//!
//! A real census over several old blocks, an oversized block and nursery blocks
//! is queried at every block edge, inside every block, between blocks and far
//! outside the heap, and each answer is compared with the binary search it
//! replaces. The sabotaged twin ignores the base inside a window and the
//! comparison notices. A fabricated set whose bases share a window keeps the
//! binary search.

use super::super::*;
use super::support::*;
use crate::gc::trace::block_window_sabotage;

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .expect("block-window test thread must not panic");
}

unsafe fn plant() {
    for i in 0..60_000usize {
        crate::arena::arena_alloc_gc_old([0usize, 24, 120][i % 3], 8, GC_TYPE_STRING);
    }
    crate::arena::arena_alloc_gc_old(3 * crate::arena::BLOCK_SIZE, 8, GC_TYPE_STRING);
    for _ in 0..20_000usize {
        young_leaf();
    }
}

fn mismatches(valid: &ValidPointerSet) -> (usize, usize) {
    let mut queries = 0usize;
    let mut wrong = 0usize;
    let mut probe = |addr: usize| {
        queries += 1;
        let (direct, search) = valid.census_block_base_both_ways(addr);
        if direct != search {
            wrong += 1;
        }
    };
    let window = 1usize << 20;
    probe(0);
    probe(usize::MAX);
    for block in &valid.arena_blocks {
        for addr in [
            block.base.saturating_sub(window),
            block.base.saturating_sub(1),
            block.base,
            block.base + 8,
            block.base + block.extent / 2,
            block.base + block.extent.saturating_sub(1),
            block.base + block.extent,
            (block.base >> 20) << 20,
            ((block.base >> 20) << 20).saturating_sub(1),
            ((block.base >> 20) + 1) << 20,
            block.base + window,
            block.base + 4 * window,
        ] {
            probe(addr);
        }
        let mut addr = block.base.saturating_sub(4096);
        while addr < block.base + block.extent + 4096 {
            probe(addr);
            addr += 4093;
        }
    }
    (queries, wrong)
}

#[test]
fn the_block_window_index_answers_exactly_like_the_fence_search() {
    run_isolated(|| {
        unsafe { plant() };
        let valid = ValidPointerSetBuilder::new().finish();
        assert!(
            !valid.block_windows.is_empty(),
            "premise: the census built a block window index"
        );
        assert!(
            valid.arena_blocks.len() >= 4,
            "premise: several census blocks"
        );
        assert!(
            valid.arena_blocks.iter().any(|b| b.sorted),
            "premise: an oversized block"
        );
        let (queries, wrong) = mismatches(&valid);
        assert!(queries > 1000, "premise: {queries} queries");
        assert_eq!(wrong, 0, "{wrong} of {queries} lookups disagree");
    });
}

#[test]
fn sabotaged_block_window_lookup_is_caught_by_the_comparison() {
    run_isolated(|| {
        unsafe { plant() };
        let valid = ValidPointerSetBuilder::new().finish();
        let (_, wrong) = {
            let _sabotage = block_window_sabotage::Guard::arm();
            mismatches(&valid)
        };
        assert!(
            wrong > 0,
            "ignoring the in-window base must disagree somewhere"
        );
    });
}

#[test]
fn bases_sharing_a_window_keep_the_fence_search() {
    let mut valid = ValidPointerSet::new();
    let base = 0x7000_0000_0000usize;
    valid.begin_arena_block(0, base, 4096);
    valid.push_arena(base + GC_HEADER_SIZE);
    valid.begin_arena_block(1, base + 8192, 4096);
    valid.push_arena(base + 8192 + GC_HEADER_SIZE);
    valid.build_block_windows();
    assert!(valid.block_windows.is_empty());
    assert_eq!(
        valid.census_block_base_both_ways(base + 8192 + 16),
        (Some(base + 8192), Some(base + 8192))
    );
}
