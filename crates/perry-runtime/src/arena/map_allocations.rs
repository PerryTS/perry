//! Enumerate allocation-authored Map starts for copying reclamation and exit.
//! The bitmap already exists for interior-pointer rejection; no owner registry
//! or per-Map registration is needed. Ordinary sweeps use the type finalizer.
use super::*;
use crate::gc::{GcHeader, GC_HEADER_SIZE};

/// `from_space_only` selects Eden and the active survivor, never old objects.
/// Callbacks must not allocate or collect; the arena blocks are borrowed.
pub(crate) fn walk_map_allocations(from_space_only: bool, mut visit: impl FnMut(*mut GcHeader)) {
    if crate::map::map_stores_never_allocated() {
        return;
    }
    sync_inline_arena_state();
    let mut walk = |blocks: &[ArenaBlock]| {
        for block in blocks {
            walk_block_maps(block, &mut visit);
        }
    };
    ARENA.with(|arena| unsafe {
        walk(&(*arena.get()).blocks);
    });
    if !from_space_only || active_survivor_space() == HeapSpace::Survivor0 {
        SURVIVOR_ARENA_0.with(|arena| unsafe {
            walk(&(*arena.get()).blocks);
        });
    }
    if !from_space_only || active_survivor_space() == HeapSpace::Survivor1 {
        SURVIVOR_ARENA_1.with(|arena| unsafe {
            walk(&(*arena.get()).blocks);
        });
    }
    if !from_space_only {
        LONGLIVED_ARENA.with(|arena| unsafe {
            walk(&(*arena.get()).blocks);
        });
        OLD_ARENA.with(|arena| unsafe {
            walk(&(*arena.get()).blocks);
        });
    }
}

/// No TLS access: also used while an arena is being destroyed.
pub(super) fn walk_block_maps(block: &ArenaBlock, visit: &mut impl FnMut(*mut GcHeader)) {
    let words = block.offset.div_ceil(64 << OBJECT_START_SHIFT);
    for (word_idx, &word) in block.object_starts.iter().take(words).enumerate() {
        let mut bits = word;
        while bits != 0 {
            let slot = word_idx * 64 + bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let offset = slot << OBJECT_START_SHIFT;
            if offset + GC_HEADER_SIZE + std::mem::size_of::<crate::map::MapHeader>() > block.offset
            {
                continue;
            }
            unsafe {
                let header = block.data.add(offset).cast::<GcHeader>();
                // Free-list reuse can leave a bit for an older tenant.
                if (*header).obj_type == crate::gc::GC_TYPE_MAP
                    && (*header).size as usize
                        == GC_HEADER_SIZE + std::mem::size_of::<crate::map::MapHeader>()
                {
                    visit(header);
                }
            }
        }
    }
}
