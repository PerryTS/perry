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
    /// Some header in the block was already MARKED or PINNED when the census
    /// read it (a subset of `obligation`, kept apart for
    /// `young_generation_unmarked`).
    pub(crate) premarked: bool,
    /// The census parsed every header of the block itself, walkable or not
    /// (`ValidPointerSetBuilder::census_whole_block`), so `non_walkable` is a
    /// complete answer. The per-object census never sees a non-walkable header.
    pub(crate) whole_walk: bool,
    /// Some header in the block does not parse as an arena object — an
    /// invalidated dead header (`obj_type == 0`) among them.
    pub(crate) non_walkable: bool,
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
            premarked: false,
            whole_walk: false,
            non_walkable: false,
        };
    }

    /// The block just begun is being parsed header by header in one pass.
    #[inline]
    pub(crate) fn note_whole_block_walk(&mut self) {
        if self.armed {
            self.current.whole_walk = true;
        }
    }

    /// The current block holds a header that does not parse as an object.
    #[inline]
    pub(crate) fn note_non_walkable(&mut self) {
        if self.armed {
            self.current.non_walkable = true;
        }
    }

    /// Record one censused header of the current block. Branch-light: this
    /// runs once per arena object in every synchronous full's census.
    ///
    /// # Safety
    /// `header` must be a walkable arena header inside the current block.
    #[inline(always)]
    pub(crate) unsafe fn note_header(&mut self, header: *const GcHeader) {
        let obj_type = (*header).obj_type;
        let size = (*header).size as u64;
        let (flag_obligation, premarked) = census_header_flag_facts(header);
        let type_obligation = self.obligation_by_type[obj_type as usize];
        let block = &mut self.current;
        block.objects += 1;
        block.bytes += size;
        block.obligation |= flag_obligation | type_obligation;
        block.premarked |= premarked;
    }

    /// Set the current block's facts from a record another walk made of it
    /// (`adopt_census`), applying this census's per-type obligations to the
    /// recorded object types.
    pub(crate) fn adopt_block_facts(
        &mut self,
        objects: u64,
        bytes: u64,
        types: &[u64; 4],
        flag_obligation: bool,
        premarked: bool,
        non_walkable: bool,
    ) {
        if !self.armed {
            return;
        }
        let type_obligation = (0..256usize)
            .any(|t| types[t >> 6] & (1u64 << (t & 63)) != 0 && self.obligation_by_type[t]);
        let block = &mut self.current;
        block.objects = objects;
        block.bytes = bytes;
        block.obligation = flag_obligation || type_obligation;
        block.premarked = premarked;
        block.non_walkable = non_walkable;
    }

    /// The facts of the block currently being censused (tests only).
    #[cfg(test)]
    pub(crate) fn current_facts_for_tests(&self) -> CensusBlock {
        self.current
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

    /// After the mark of a synchronous full: does the young generation (Eden
    /// and both survivor spaces) hold **no** marked or pinned object?
    ///
    /// Every in-use young block must be censused, unchanged since the census
    /// (no allocate-black birth, no block created after it), free of headers
    /// that were already marked or pinned when censused, and unreached by the
    /// trace. An unreached block holds no object the trace marked (see this
    /// module's doc: every census-built mark passes a membership query that
    /// records its block), so every young object is then garbage. `false` when
    /// the census is disarmed or anything is uncertain.
    pub(crate) fn young_generation_unmarked(&self) -> bool {
        if !self.armed {
            return false;
        }
        let snapshots = crate::arena::arena_block_snapshots();
        let young = crate::arena::young_block_count().min(snapshots.len());
        snapshots[..young]
            .iter()
            .enumerate()
            .all(|(block_idx, snapshot)| {
                if snapshot.data == 0 || snapshot.offset == 0 {
                    return true;
                }
                self.block(block_idx).is_some_and(|block| {
                    !block.premarked
                        && !self.reached(block_idx)
                        && block.data == snapshot.data
                        && block.end == snapshot.data.saturating_add(snapshot.offset)
                })
            })
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
    /// `verify::full_remembered_rebuild_provably_empty` answers true whatever
    /// the heap holds.
    pub(crate) const FORCE_REBUILD_SKIP: u8 = 4;
    /// The sweep never applies its per-page accounting tally before a page-index
    /// flush or a step end (it is applied only at page changes).
    pub(crate) const FORGET_PAGE_TALLY_ORDER: u8 = 8;
    /// A dead old header is invalidated without first expanding the described
    /// promoted run of its page.
    pub(crate) const FORGET_RUN_EXPANSION: u8 = 16;
    /// The sweep treats every whole-walked block as holding no invalidated
    /// header, whatever the census saw.
    pub(crate) const FORGET_HOLES: u8 = 32;
    /// The whole-block sweep's fast path keeps objects without recording that
    /// their block holds a live object.
    pub(crate) const FORGET_WHOLE_BLOCK_LIVE: u8 = 64;

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

/// The per-object census facts that depend on a header's flags rather than its
/// type: `(flag obligation, pre-marked)`. The flag obligation covers a pinned,
/// forwarded or already-marked header, one without `GC_FLAG_ARENA`, and an array
/// whose raw-f64 layout bits would fire a typed-feedback invalidation.
///
/// # Safety
/// `header` is a readable arena header.
#[inline(always)]
pub(crate) unsafe fn census_header_flag_facts(header: *const GcHeader) -> (bool, bool) {
    let flags = (*header).gc_flags;
    let exceptional_flags = (flags ^ GC_FLAG_ARENA)
        & (GC_FLAG_ARENA | GC_FLAG_MARKED | GC_FLAG_PINNED | GC_FLAG_FORWARDED)
        != 0;
    let raw_f64_array = (*header).obj_type == GC_TYPE_ARRAY
        && (*header)._reserved & (GC_ARRAY_RAW_F64_LAYOUT | GC_ARRAY_RAW_F64_HOLES) != 0;
    (
        exceptional_flags | raw_f64_array,
        flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0,
    )
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
    static HOLE_REBUILD_BLOCKS_SKIPPED: Cell<u64> = const { Cell::new(0) };
}

/// Record live old blocks one sweep's hole-list rebuild did not parse because
/// they provably hold no invalidated header (live-subject counter).
pub(crate) fn note_hole_rebuild_blocks_skipped(blocks: u64) {
    HOLE_REBUILD_BLOCKS_SKIPPED.with(|c| c.set(c.get().saturating_add(blocks)));
}

/// Live old blocks this thread's hole-list rebuilds skipped, since thread start.
#[cfg(test)]
pub(crate) fn hole_rebuild_blocks_skipped() -> u64 {
    HOLE_REBUILD_BLOCKS_SKIPPED.with(Cell::get)
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
