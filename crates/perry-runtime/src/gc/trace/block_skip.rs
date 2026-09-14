//! Block-granular reclamation for the synchronous full sweep (#10182).
//!
//! A full sweep used to visit every object in the arena: each dead one paid
//! for page accounting, `finalize_dead_arena_payload` and a page-index
//! removal, and only then did the block cleanup notice that the block had no
//! live object and reset it wholesale. After a parse/scan loop nearly every
//! block holds only dead objects, so a full's cost scaled with the garbage.
//!
//! This module lets the sweep reclaim such a block **without entering it**.
//! Two facts decide it, both gathered without any extra heap walk:
//!
//! 1. **Nothing in the block was reached by the trace.** The exact census
//!    (`ValidPointerSetBuilder`) seals its address-ordered runs at block
//!    boundaries and records the block each run belongs to. Every mark a
//!    census-built cycle sets is preceded by a successful membership query
//!    against that census (`ValidPointerSet::contains` or `enclosing_object`),
//!    and a successful query marks the run's block as *reached*. A reached
//!    block is a superset of a block holding a marked object, so an unreached
//!    block holds none. The two mark paths that do not consult the census are
//!    excluded structurally rather than recorded: allocate-black births change
//!    the block's bump offset (a block whose offset moved since the census is
//!    never skipped), and block persistence only force-marks the recent general
//!    window, which is never skipped either.
//!
//! 2. **No object in the block owes per-object work.** The census reads every
//!    header anyway; it records an *obligation* when an object is pinned,
//!    forwarded, already marked, lacks `GC_FLAG_ARENA`, carries a finalize
//!    hook (Map/Set side allocations, promises, native owners and views,
//!    typed-array view metadata, Temporal cells, errors, RegExps, lazy tapes),
//!    is an array whose raw-f64 layout bits would fire a typed-feedback
//!    invalidation, or is an object while the legacy overflow table or the
//!    wasm module-wrapper registry holds entries. What remains of
//!    `finalize_dead_arena_payload` for the other objects is address-keyed
//!    side-table removal that the full trace's dead-owner fan-out has already
//!    performed at sweep entry (`ELEMENT_SHAPES`, per-object layouts, closure
//!    dynamic props — `dead_owner::DEAD_KEY_PRUNES`), plus `_reserved` header
//!    bits that every allocator rewrites to zero, so the block reset retires
//!    them.
//!
//! The page-index and old-page bookkeeping the per-object path does for a dead
//! old object is superseded by the block cleanup, which drops every page of a
//! non-live old block (`unregister_old_block_pages`). Freed-byte and Eden-dead
//! accounting come from the census's per-block byte sums, which cover exactly
//! the objects the sweep cursor would have yielded.
//!
//! Only synchronous full cycles with a census-built pointer set use this: a
//! budgeted cycle runs the mutator between phases (births into swept holes do
//! not move an offset) and resolves membership through the page classifier,
//! so it has no census to record against.

use super::*;
use std::cell::Cell;

/// What the census saw in one arena block (global block index).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CensusBlock {
    pub(crate) data: usize,
    /// `data + offset` when the census walked the block.
    pub(crate) end: usize,
    pub(crate) objects: u64,
    pub(crate) bytes: u64,
    pub(crate) censused: bool,
    /// Some object in the block needs the per-object sweep path.
    pub(crate) obligation: bool,
}

/// Per-block census facts and trace reachability for one cycle's
/// `ValidPointerSet`.
pub(crate) struct BlockCensus {
    blocks: Vec<CensusBlock>,
    reached: Vec<Cell<bool>>,
    /// The block the census is inside, folded into `blocks` at the next block
    /// change (`usize::MAX` when none).
    current_idx: usize,
    current: CensusBlock,
    /// `obligation_by_type[obj_type]`: objects of this type always need the
    /// per-object sweep path while this census is current.
    obligation_by_type: [bool; 256],
    armed: bool,
}

impl BlockCensus {
    /// An empty census that records nothing (fabricated test sets, classifier
    /// mode).
    pub(crate) fn disarmed() -> Self {
        Self {
            blocks: Vec::new(),
            reached: Vec::new(),
            current_idx: usize::MAX,
            current: CensusBlock::default(),
            obligation_by_type: [true; 256],
            armed: false,
        }
    }

    /// A census that records blocks. The type obligations are fixed here:
    /// nothing a synchronous cycle runs between the census and the sweep can
    /// populate the legacy overflow table or the module-wrapper registry.
    pub(crate) fn armed() -> Self {
        let object_side_tables_live = !crate::object::overflow_fields_is_empty()
            || crate::object::module_wrapper_registry_ever_used();
        let mut obligation_by_type = [true; 256];
        for (obj_type, slot) in obligation_by_type.iter_mut().enumerate() {
            *slot = type_needs_per_object_sweep(obj_type as u8, object_side_tables_live);
        }
        Self {
            blocks: Vec::new(),
            reached: Vec::new(),
            current_idx: usize::MAX,
            current: CensusBlock::default(),
            obligation_by_type,
            armed: true,
        }
    }

    #[inline]
    pub(crate) fn is_armed(&self) -> bool {
        self.armed
    }

    /// Start censusing block `block_idx` (`data`/`offset` as the cursor
    /// snapshotted it). Headers noted afterwards belong to it until the next
    /// `begin_block`.
    #[inline]
    pub(crate) fn begin_block(&mut self, block_idx: usize, data: usize, offset: usize) {
        if !self.armed {
            return;
        }
        self.flush_block();
        self.current_idx = block_idx;
        self.current = CensusBlock {
            data,
            end: data.saturating_add(offset),
            objects: 0,
            bytes: 0,
            censused: true,
            obligation: false,
        };
    }

    /// Record one censused header of the current block. Branch-light: this
    /// runs once per arena object in every synchronous full's census.
    ///
    /// # Safety
    /// `header` must be a walkable arena header inside the current block.
    #[inline(always)]
    pub(crate) unsafe fn note_header(&mut self, header: *const GcHeader) {
        let flags = (*header).gc_flags;
        let obj_type = (*header).obj_type;
        let size = (*header).size as u64;
        let exceptional_flags = (flags ^ GC_FLAG_ARENA)
            & (GC_FLAG_ARENA | GC_FLAG_MARKED | GC_FLAG_PINNED | GC_FLAG_FORWARDED)
            != 0;
        let raw_f64_array = obj_type == GC_TYPE_ARRAY
            && (*header)._reserved & (GC_ARRAY_RAW_F64_LAYOUT | GC_ARRAY_RAW_F64_HOLES) != 0;
        let type_obligation = self.obligation_by_type[obj_type as usize];
        let block = &mut self.current;
        block.objects += 1;
        block.bytes += size;
        block.obligation |= exceptional_flags | type_obligation | raw_f64_array;
    }

    /// Fold the current block into the per-index table. Called at every block
    /// change and once when the census finishes.
    pub(crate) fn flush_block(&mut self) {
        if !self.armed || self.current_idx == usize::MAX {
            return;
        }
        let block_idx = self.current_idx;
        if block_idx >= self.blocks.len() {
            self.blocks.resize(block_idx + 1, CensusBlock::default());
            self.reached.resize_with(block_idx + 1, || Cell::new(false));
        }
        self.blocks[block_idx] = self.current;
        self.current_idx = usize::MAX;
    }

    /// The trace reached (and so may have marked) an object in `block_idx`.
    #[inline(always)]
    pub(crate) fn note_reached(&self, block_idx: u32) {
        if let Some(cell) = self.reached.get(block_idx as usize) {
            cell.set(true);
        }
    }

    /// Blocks a `require_marked` whole-heap walk may skip right now: censused,
    /// not reached by the trace, free of obligations (which include every
    /// pinned or pre-marked header), and not grown since the census. Such a
    /// block holds no marked or pinned object, so the walk would visit each
    /// of its objects only to reject it. `None` when nothing qualifies or the
    /// census is disarmed.
    pub(crate) fn unmarked_blocks(&self) -> Option<Vec<bool>> {
        if !self.armed {
            return None;
        }
        let snapshots = crate::arena::arena_block_snapshots();
        let mut skip = vec![false; snapshots.len()];
        let mut any = false;
        for (block_idx, snapshot) in snapshots.iter().enumerate() {
            let Some(block) = self.block(block_idx) else {
                continue;
            };
            if block.obligation
                || self.reached(block_idx)
                || block.data != snapshot.data
                || block.end != snapshot.data.saturating_add(snapshot.offset)
            {
                continue;
            }
            skip[block_idx] = true;
            any = true;
        }
        any.then_some(skip)
    }

    pub(crate) fn block(&self, block_idx: usize) -> Option<CensusBlock> {
        self.blocks.get(block_idx).copied().filter(|b| b.censused)
    }

    pub(crate) fn reached(&self, block_idx: usize) -> bool {
        self.reached.get(block_idx).is_some_and(Cell::get)
    }
}

/// Sabotage switches for the block-skip tests: each one breaks exactly one of
/// the two facts the skip rests on, so a test can show its own assertion fails
/// when that fact is not maintained. Test builds only.
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    pub(crate) const FORGET_REACHED: u8 = 1;
    pub(crate) const FORGET_OBLIGATIONS: u8 = 2;

    thread_local! {
        static SABOTAGE: Cell<u8> = const { Cell::new(0) };
    }

    thread_local! {
        static LAST_SKIPPED_BASES: std::cell::RefCell<Vec<usize>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(crate) fn get() -> u8 {
        SABOTAGE.with(Cell::get)
    }

    /// The `data` addresses of the blocks the most recent full sweep on this
    /// thread reclaimed without visiting.
    pub(crate) fn last_skipped_block_bases() -> Vec<usize> {
        LAST_SKIPPED_BASES.with(|bases| bases.borrow().clone())
    }

    pub(crate) fn record_skipped_block_bases(bases: Vec<usize>) {
        LAST_SKIPPED_BASES.with(|slot| *slot.borrow_mut() = bases);
    }

    /// Arms `bits` until the guard drops.
    pub(crate) struct Guard(u8);

    impl Guard {
        pub(crate) fn arm(bits: u8) -> Self {
            let previous = SABOTAGE.with(|s| s.replace(bits));
            Self(previous)
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            SABOTAGE.with(|s| s.set(self.0));
        }
    }
}

/// Does a dead object of `obj_type` need `reclaim_dead_object`'s per-object
/// work beyond what the full trace's dead-owner fan-out and the block reset
/// already do?
pub(crate) fn type_needs_per_object_sweep(obj_type: u8, object_side_tables_live: bool) -> bool {
    let Some(info) = gc_type_info(obj_type) else {
        return true;
    };
    if info.finalize_hook_kind != GcFinalizeHookKind::None {
        return true;
    }
    match info.move_hook_kind {
        // `clear_overflow_for_ptr` / `clear_module_wrapper_for_dead_ptr` are
        // no-ops while their tables are empty; neither is in the fan-out.
        GcMoveHookKind::ObjectOverflowFields => object_side_tables_live,
        // Pruned post-trace by `closure::prune_dead_closure_side_table_owners`.
        GcMoveHookKind::ClosureDynamicProps => false,
        GcMoveHookKind::ErrorSideTables
        | GcMoveHookKind::RegExpSideTables
        | GcMoveHookKind::LazyArrayTape => true,
        GcMoveHookKind::None
        | GcMoveHookKind::MapSideTables
        | GcMoveHookKind::SetSideTables
        | GcMoveHookKind::ExoticExpandoOwner => false,
    }
}

crate::perry_thread_local! {
    static BLOCK_SKIP_RECLAIMED_BLOCKS: Cell<u64> = const { Cell::new(0) };
    static BLOCK_SKIP_RECLAIMED_OBJECTS: Cell<u64> = const { Cell::new(0) };
    static BLOCK_SKIP_RECLAIMED_BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Record one sweep's block-skip reclamation (live-subject counters).
pub(crate) fn note_block_skip_reclaimed(blocks: u64, objects: u64, bytes: u64) {
    if blocks == 0 {
        return;
    }
    BLOCK_SKIP_RECLAIMED_BLOCKS.with(|c| c.set(c.get().saturating_add(blocks)));
    BLOCK_SKIP_RECLAIMED_OBJECTS.with(|c| c.set(c.get().saturating_add(objects)));
    BLOCK_SKIP_RECLAIMED_BYTES.with(|c| c.set(c.get().saturating_add(bytes)));
}

/// `(blocks, objects, bytes)` this thread's full sweeps reclaimed without
/// visiting, since thread start.
#[cfg(test)]
pub(crate) fn block_skip_reclaimed_totals() -> (u64, u64, u64) {
    (
        BLOCK_SKIP_RECLAIMED_BLOCKS.with(Cell::get),
        BLOCK_SKIP_RECLAIMED_OBJECTS.with(Cell::get),
        BLOCK_SKIP_RECLAIMED_BYTES.with(Cell::get),
    )
}
