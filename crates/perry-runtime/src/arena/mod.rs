//! Fast bump allocator for short-lived objects
//!
//! Uses thread-local bump allocation for fast object creation.
//! Objects allocated here are not individually freed - the entire arena
//! can be reset at once (e.g., at end of program or during GC).

pub(crate) use std::alloc::{alloc, Layout};
pub(crate) use std::cell::{Cell, RefCell, UnsafeCell};
pub(crate) use std::collections::{hash_map::Entry, HashMap};
pub(crate) use std::hash::{BuildHasherDefault, Hasher};

mod allocators;
mod block;
mod inline;
mod page_meta;
mod reset;
mod stats;
mod walk;

#[cfg(test)]
mod tests;

// Cross-sibling shared types/thread-locals (used by sibling modules via
// `use super::*;`). These are not part of the crate-public surface
// individually; the public re-exports below are explicit and named.
pub(crate) use allocators::{
    inactive_survivor_index, with_survivor_arena, with_survivor_arena_mut,
};
pub(crate) use block::{
    old_gen_in_use_bytes_sub, Arena, ArenaBlock, ACTIVE_SURVIVOR, ARENA, ARENA_TOTAL_BYTES,
    BLOCK_SIZE, FRESH_GENERAL_BLOCK_MIN_USED_BYTES, INLINE_STATE, LONGLIVED_ARENA, OLD_ARENA,
    OLD_GEN_IN_USE_BYTES, SURVIVOR_ARENA_0, SURVIVOR_ARENA_1,
};
pub(crate) use page_meta::{
    address_span_overlaps_pages, register_block_space, register_old_object_pages,
    unregister_block_generation, unregister_old_block_pages, OLD_GEN_RECLAIM_RETURNED_BYTES,
    OLD_GEN_RECLAIM_REUSABLE_BYTES,
};

// --- Public API (explicit named re-exports) ---

// inline.rs
pub use inline::{
    arena_start_fresh_general_block, js_inline_arena_slow_alloc, js_inline_arena_state,
    sync_inline_arena_state, InlineArenaState,
};

// allocators.rs (formerly arena.rs `alloc.rs` group)
pub use allocators::{
    arena_alloc, arena_alloc_gc, arena_alloc_gc_longlived, arena_alloc_gc_old,
    arena_alloc_longlived, arena_alloc_old, js_arena_alloc,
};
pub(crate) use allocators::{arena_alloc_gc_old_excluding_pages, arena_alloc_gc_survivor};

// walk.rs
#[cfg(feature = "diagnostics")]
pub(crate) use walk::ArenaRegionTelemetry;
pub use walk::{
    arena_block_count, arena_in_use_bytes, arena_total_bytes, arena_walk_objects,
    arena_walk_objects_addr_sorted, arena_walk_objects_filtered,
    arena_walk_objects_with_block_index, general_block_count, longlived_end,
    old_arena_walk_objects, ArenaResetStats,
};
pub(crate) use walk::{
    arena_block_snapshots, arena_telemetry_snapshot, general_block_in_recent_window,
    general_block_sizes, ArenaBlockSnapshot, ArenaObjectCursor, ArenaObjectCursorBuilder,
    ArenaTelemetrySnapshot, ArenaWalkOrder,
};

// reset.rs
pub(crate) use reset::{
    active_survivor_block_index_range, copying_active_survivor_in_use_bytes,
    copying_from_space_in_use_bytes, copying_prepare_to_space, copying_reset_from_spaces_and_flip,
    old_arena_reclaim_dead_blocks, old_arena_reclaim_selected_dead_blocks,
    survivor_arena_reclaim_dead_blocks, ArenaResetEmptyBlocksState, OldArenaReclaimDeadBlocksState,
    SurvivorArenaReclaimDeadBlocksState,
};
pub use reset::{arena_reset_all_blocks_to_zero, arena_reset_empty_blocks};

// stats.rs
pub(crate) use stats::{active_survivor_space, inactive_survivor_space};
pub use stats::{
    js_arena_stats, longlived_in_use_bytes, old_gen_in_use_bytes, pointer_in_nursery,
    pointer_in_old_gen,
};
#[cfg(test)]
pub(crate) use stats::{old_gen_in_use_bytes_recomputed, old_gen_in_use_bytes_resync};

// page_meta.rs (public + pub(crate) classification/page-meta API)
pub(crate) use page_meta::{
    classify_heap_generation, classify_heap_space, generation_page_for_addr,
    old_arena_page_index_remove_object, old_arena_source_blocks_for_pages,
    old_arena_walk_objects_on_pages, old_object_page_overlaps, old_page_account_dirty_slot,
    old_page_account_promoted_object, old_page_account_swept_object, old_page_clear_dirty,
    old_page_mark_dirty, old_page_meta_snapshot, old_page_summary, old_pages_begin_gc_cycle,
    old_pages_reset_sweep_accounting, unregister_old_object_pages, HeapGeneration, HeapSpace,
    OldArenaPageObjectCursor, OldArenaSourceBlockSelection, OldPageMeta, OldPageSummary,
};

#[cfg(test)]
pub(crate) use page_meta::{
    generation_page_base, old_arena_page_index_clear_for_tests, old_page_meta_for_tests,
    GENERATION_CLASS_SHIFT, GENERATION_PAGE_SIZE,
};
