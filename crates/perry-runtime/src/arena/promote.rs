//! Whole-block in-place promotion of the young generation (#7742).
//!
//! # Why
//!
//! The copying minor relocates every survivor object by object. Each move
//! costs a fresh old-gen allocation, a `memcpy`, a `layout_transfer`, per-object
//! old-page accounting, the `GcMoveHookKind` hooks, and a forwarding stub — and
//! then every referring slot has to be rewritten to the new address. That price
//! is worth paying when most of the nursery is garbage: the survivors are few
//! and compaction buys back the whole block.
//!
//! It is worth nothing at all when the nursery is *fully live*. `retain.ts`
//! (3M records, none ever dropped) measured **1.000** young-survival on every
//! copying minor, so every one of its 2.1M promotions paid ~240 ns of
//! bookkeeping to move an object that had no reason to move.
//!
//! Whole-block promotion is the standard answer (V8 calls it page promotion):
//! when a block is (near-)entirely live, relabel the block as old-gen instead
//! of evacuating it object by object. Nothing moves, so there is no copy, no
//! allocation, no layout transfer, no move hook, no forwarding pointer, and —
//! because addresses are unchanged — **no slot anywhere in the heap or in any
//! address-keyed runtime side table needs rewriting**.
//!
//! # Shape of the operation
//!
//! Promotion is decided *per cycle*, not per block, because a block's liveness
//! is not knowable before the trace and block identity does not survive a reset
//! (Eden blocks are recycled at offset 0). What IS knowable is the previous
//! cycle's measured young-survival ratio, and that ratio is re-measured on
//! every cycle including the promoting ones — see `gc::promote_in_place`. So a
//! misprediction costs at most one nursery's worth of retained garbage before
//! the policy corrects itself.
//!
//! The two halves:
//!
//! * [`retag_young_for_in_place_promotion`] runs *before* the trace. It flips
//!   every in-use Eden and survivor block's page range — BOTH semispaces, see
//!   the comment on the loop — to generation
//!   `Old`, space [`HeapSpace::PromotedYoung`]. From that instant the barrier
//!   predicates (`barrier_parent_needs_remembering`,
//!   `remembered_child_needs_tracking`) treat those objects as old — which is
//!   what makes the trace record the right remembered-set edges — while
//!   `classify_arena` can still tell they were young at cycle start and so must
//!   be traced once. The blocks stay in their own arenas for the duration, so
//!   no old-gen allocation can land in them mid-cycle.
//! * [`finish_in_place_promotion`] runs where the from-space reset would have.
//!   It walks each block linearly (cheap: one `size` hop per object, no
//!   hashing), stamps `GC_FLAG_TENURED` on every header so the `Old ⟹ TENURED`
//!   invariant the generated write barrier's fast path relies on (#7511) still
//!   holds, registers the **marked** (live) objects into the old-gen page index
//!   in per-page bulk runs, moves the block into `OLD_ARENA` leaving a
//!   tombstone behind, and retags the range to plain [`HeapSpace::Old`].
//!
//! # What is deliberately NOT done
//!
//! Unmarked objects on a promoted block are dead. They are neither swept nor
//! registered — they stay as old-gen garbage until the next full mark-sweep,
//! which finds them through the ordinary linear old-arena walk. That retention
//! is the entire footprint cost of the technique, it is bounded by
//! `(1 − liveness) × young bytes` per cycle, and the policy layer both caps its
//! running total and reports it in the GC trace.

use super::page_meta::{
    generation_page_base, register_promoted_page_headers, register_promoted_page_run,
    retag_block_space, GENERATION_PAGE_SIZE,
};
use super::*;

/// One block captured by [`retag_young_for_in_place_promotion`], to be finished
/// by [`finish_in_place_promotion`].
#[derive(Clone, Copy, Debug)]
struct PromotedBlock {
    /// Which arena the block still lives in, so the finish walk can take it out.
    source: PromotionSource,
    /// Index into that arena's `blocks`.
    index: usize,
    base: usize,
    size: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromotionSource {
    Eden,
    Survivor(usize),
}

/// The set of young blocks this cycle promotes in place. Created before the
/// trace, consumed after it.
#[derive(Default)]
pub(crate) struct InPlacePromotion {
    blocks: Vec<PromotedBlock>,
    /// Reserved bytes the young generation is about to lose to old-gen.
    ///
    /// This is NOT bookkeeping trivia — it is the whole of the pacing contract.
    /// The arena-bytes trigger fires on `arena_total_bytes()`, and the runway
    /// between two collections is therefore "Eden's free capacity + the trigger
    /// step". A copying minor recycles Eden's blocks, so that capacity survives
    /// the collection and the runway stays wide. Promotion HANDS THOSE BLOCKS
    /// AWAY, so without giving the capacity back the runway collapses to the
    /// bare step. Measured on `retain.ts`: Eden's high-water fell 57 MB → 18 MB,
    /// collections went 6 → 12, and the extra pressure bought a second full
    /// collection at 690 ms — a 0.81 s → 1.52 s regression from a change that
    /// made every individual promotion cheaper.
    ///
    /// The capacity is given back as trigger HEADROOM
    /// (`gc::note_promoted_young_capacity`), not as eagerly mapped blocks.
    /// Re-reserving the blocks up front also fixes the pacing, but it maps
    /// memory the program may never reach: measured on `retain_wide.ts`, the
    /// eager form ratcheted Eden to 89 MB and left peak RSS at 470 MB against
    /// the baseline's 447 MB. Headroom costs nothing until the mutator actually
    /// allocates into it.
    reserved_bytes: usize,
}

impl InPlacePromotion {
    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub(crate) fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Young capacity this promotion hands to old-gen — see
    /// [`InPlacePromotion::reserved_bytes`].
    pub(crate) fn reserved_bytes(&self) -> usize {
        self.reserved_bytes
    }
}

/// Where the finish walk gets each object's liveness from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PromotionLiveness {
    /// `GC_FLAG_MARKED | GC_FLAG_PINNED` on the still-marked headers — the
    /// cycle traced, so it knows.
    Marked,
    /// The cycle did not trace (#7888), so it does not know and does not
    /// pretend to: every parseable object on the block is registered.
    ///
    /// Registering an object that is in fact dead is safe, and is strictly
    /// closer to what the index is FOR than omitting it would be. The index
    /// answers "which objects live on this page" for the remembered-set dirty
    /// scan; a dead promoted object is still an object on that page, still
    /// intact memory, and — because promotion moves nothing — still holds the
    /// slot values it held when it died, whose targets were promoted alongside
    /// it. The next full mark-sweep frees it and unregisters it in the same
    /// pass. What it costs is accounting precision: the page's live-bytes
    /// figure counts it until then.
    AssumeAllLive,
}

/// Per-block liveness, produced by the finish walk. This is the measurement the
/// promotion policy is calibrated against — `PERRY_GC_DIAG=1` prints it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InPlacePromotionStats {
    pub(crate) blocks: usize,
    pub(crate) objects: usize,
    pub(crate) live_objects: usize,
    pub(crate) bytes: usize,
    pub(crate) live_bytes: usize,
    /// Blocks whose live fraction was below 50% — the shape that would have
    /// been better served by evacuation. Zero on every benchmark measured.
    pub(crate) sparse_blocks: usize,
    /// Blocks with ZERO live objects, and their bytes. These are the blocks a
    /// promotion keeps for nothing: no live object needs their addresses held
    /// still, so the ordinary from-space reset would have recycled them.
    pub(crate) dead_blocks: usize,
    pub(crate) dead_block_bytes: usize,
}

/// Retag every in-use young block as old-gen `PromotedYoung`.
///
/// Returns the blocks captured. An empty result means there was nothing to
/// promote and the caller must fall back to the ordinary copying path (in
/// particular it must NOT skip the from-space reset).
pub(crate) fn retag_young_for_in_place_promotion() -> InPlacePromotion {
    sync_inline_arena_state();
    let mut promotion = InPlacePromotion::default();

    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        collect_blocks(&arena.blocks, PromotionSource::Eden, &mut promotion);
    });
    // BOTH semispaces, not just the active one. The inactive space is to-space
    // and is empty at cycle start, so this normally captures nothing — but
    // taking every in-use young block unconditionally is what makes "after the
    // retag, no address in the heap classifies as `Nursery`" true BY
    // CONSTRUCTION rather than by an argument about semispace bookkeeping. Two
    // provable no-op eliminations downstream rest on exactly that claim.
    for idx in [0usize, 1usize] {
        with_survivor_arena(idx, |arena| {
            collect_blocks(
                &arena.blocks,
                PromotionSource::Survivor(idx),
                &mut promotion,
            );
        });
    }

    for block in &promotion.blocks {
        retag_block_space(
            block.base,
            block.size,
            HeapGeneration::Old,
            HeapSpace::PromotedYoung,
        );
    }
    promotion
}

/// Put every block [`retag_young_for_in_place_promotion`] captured back in the
/// young generation, in the space it came from (#7937).
///
/// This is the whole of the physical rollback for a speculative first-cycle
/// promotion, and it is complete because the retag is the whole of the physical
/// commitment: nothing moved, no block changed arenas, no bump pointer moved,
/// and `finish_in_place_promotion` — the only step that hands a block to
/// `OLD_ARENA` — has not run. The caller owns the two remaining obligations
/// (clear the marks the trace set, and do not consume the remembered set); see
/// `gc::copying`'s rollback for why the list is exactly that long.
pub(crate) fn undo_in_place_promotion_retag(promotion: &InPlacePromotion) {
    for block in &promotion.blocks {
        let space = match block.source {
            PromotionSource::Eden => HeapSpace::NurseryEden,
            PromotionSource::Survivor(0) => HeapSpace::Survivor0,
            PromotionSource::Survivor(_) => HeapSpace::Survivor1,
        };
        retag_block_space(block.base, block.size, HeapGeneration::Nursery, space);
    }
}

/// Bytes still in use in blocks that classify as young generation. The premise
/// `gc::copying::debug_assert_no_remembering_possible` re-derives: after
/// [`retag_young_for_in_place_promotion`] this must be zero.
#[cfg(debug_assertions)]
pub(crate) fn young_in_use_bytes_after_retag() -> usize {
    let mut total = 0usize;
    let mut count = |blocks: &[ArenaBlock]| {
        for block in blocks {
            if block.data.is_null() || block.offset == 0 {
                continue;
            }
            if matches!(
                super::page_meta::classify_heap_space(block.data as usize),
                HeapSpace::NurseryEden | HeapSpace::Survivor0 | HeapSpace::Survivor1
            ) {
                total = total.saturating_add(block.offset);
            }
        }
    };
    ARENA.with(|arena| count(&unsafe { &*arena.get() }.blocks));
    for idx in [0usize, 1usize] {
        with_survivor_arena(idx, |arena| count(&arena.blocks));
    }
    total
}

fn collect_blocks(blocks: &[ArenaBlock], source: PromotionSource, out: &mut InPlacePromotion) {
    for (index, block) in blocks.iter().enumerate() {
        if block.data.is_null() || block.offset == 0 {
            continue;
        }
        out.blocks.push(PromotedBlock {
            source,
            index,
            base: block.data as usize,
            size: block.size,
        });
        out.reserved_bytes = out.reserved_bytes.saturating_add(block.size);
    }
}

/// Finish the promotion: stamp headers, index the live objects, hand the blocks
/// to `OLD_ARENA`, and leave both young regions empty.
///
/// MUST run before `CopyingNurseryCollector::clear_marks` — liveness here is
/// `GC_FLAG_MARKED | GC_FLAG_PINNED` on the still-marked headers.
pub(crate) fn finish_in_place_promotion(
    promotion: InPlacePromotion,
    liveness: PromotionLiveness,
) -> InPlacePromotionStats {
    let mut stats = InPlacePromotionStats {
        blocks: promotion.blocks.len(),
        ..InPlacePromotionStats::default()
    };
    if promotion.blocks.is_empty() {
        return stats;
    }

    // #7624: the page-objects index must be complete before this adds to it,
    // for the same reason every other reader flushes — a pending registration
    // for one of these pages would otherwise be folded in afterwards and land
    // behind the run this walk appends.
    super::page_meta::flush_deferred_old_page_registrations();

    let mut moved_blocks: Vec<ArenaBlock> = Vec::with_capacity(promotion.blocks.len());
    for block in &promotion.blocks {
        let taken = take_block(*block);
        let Some(taken) = taken else { continue };
        let (objects, live_objects, live_bytes) = stamp_and_index_block(&taken, liveness);
        stats.objects += objects;
        stats.live_objects += live_objects;
        stats.bytes += taken.offset;
        stats.live_bytes += live_bytes;
        if taken.offset > 0 && live_bytes * 2 < taken.offset {
            stats.sparse_blocks += 1;
        }
        if taken.offset > 0 && live_objects == 0 {
            stats.dead_blocks += 1;
            stats.dead_block_bytes += taken.offset;
        }
        moved_blocks.push(taken);
    }

    // Hand the blocks to old-gen. The page range is retagged from
    // `PromotedYoung` to plain `Old` here rather than in the walk above so that
    // nothing observes a block that is simultaneously listed in `OLD_ARENA` and
    // labelled young.
    OLD_ARENA.with(|old| {
        let old = unsafe { &mut *old.get() };
        for block in moved_blocks {
            let base = block.data as usize;
            let size = block.size;
            let offset = block.offset;
            super::block::old_gen_in_use_bytes_add(offset);
            install_block_into(old, block);
            retag_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
        }
    });

    // Both young regions are empty now. Eden needs a usable current block for
    // the inline bump allocator; the survivor flip keeps the semispace
    // alternation the copying path maintains.
    reset_young_after_promotion();
    stats
}

/// Detach the block from its owning arena, leaving the `data = null, size = 0`
/// tombstone every arena path already tolerates (C4b-δ leaves the same shape),
/// so block indices stay stable across the cycle.
fn take_block(block: PromotedBlock) -> Option<ArenaBlock> {
    let detach = |arena: &mut Arena| -> Option<ArenaBlock> {
        // The index recorded at retag time is right in every case that can
        // occur — no arena path reorders `blocks`, it only fills tombstones or
        // pushes. Confirm it anyway and fall back to a search, because the
        // alternative to finding this block is leaving an `Old`-registered
        // block sitting in a young arena, where the next reset would zero live
        // data. There is no safe "give up" branch here.
        let found = match arena.blocks.get(block.index) {
            Some(slot) if slot.data as usize == block.base => Some(block.index),
            _ => arena
                .blocks
                .iter()
                .position(|slot| slot.data as usize == block.base),
        };
        let index = found.unwrap_or_else(|| {
            panic!(
                "in-place promotion cannot find the block it retagged \
                 (base={:#x} size={}); it is registered as old-gen but still \
                 owned by a young arena",
                block.base, block.size
            )
        });
        Some(std::mem::replace(
            &mut arena.blocks[index],
            ArenaBlock {
                data: std::ptr::null_mut(),
                size: 0,
                offset: 0,
                dead_cycles: 0,
            },
        ))
    };
    match block.source {
        PromotionSource::Eden => ARENA.with(|arena| detach(unsafe { &mut *arena.get() })),
        PromotionSource::Survivor(idx) => with_survivor_arena_mut(idx, detach),
    }
}

fn install_block_into(arena: &mut Arena, block: ArenaBlock) {
    if let Some(slot) = arena.blocks.iter_mut().find(|slot| slot.data.is_null()) {
        *slot = block;
        return;
    }
    arena.blocks.push(block);
}

/// Hand one page's finished run to the index, either DESCRIBED (untraced
/// promotion — the parse can reconstruct it) or STORED (traced promotion — only
/// the about-to-be-cleared marks know which objects are live).
#[allow(clippy::too_many_arguments)]
fn flush_page_run(
    describe: bool,
    page: usize,
    first: usize,
    last: usize,
    count: usize,
    headers: &[usize],
    bytes: usize,
) {
    if describe {
        register_promoted_page_run(page, first, last, count, bytes);
    } else {
        register_promoted_page_headers(page, headers, bytes);
    }
}

/// Linear walk of one promoted block: stamp `GC_FLAG_TENURED` on every header,
/// and register the live ones with the old-gen page index in per-page bulk
/// runs. Returns `(objects, live_objects, live_bytes)`.
///
/// The walk parses the block exactly the way `arena_walk_objects` and
/// `old_arena_walk_objects` do — hop by `GcHeader::size`, stop on an
/// implausible one. A block that does not parse to its own `offset` would leave
/// its tail unstamped and unindexed, which is a missed old→young edge; it is
/// also a block the existing sweep walkers could not parse either, so it is a
/// pre-existing whole-heap invariant rather than something this path can repair.
/// `debug_assert` catches it in CI instead of letting it be silent.
fn stamp_and_index_block(block: &ArenaBlock, liveness: PromotionLiveness) -> (usize, usize, usize) {
    use crate::gc::GcHeader;

    let mut objects = 0usize;
    let mut live_objects = 0usize;
    let mut live_bytes = 0usize;

    // One page's worth of live headers, flushed whenever the page changes.
    // The walk is in address order, so a page's headers are a contiguous
    // ascending run.
    //
    // On the UNTRACED path (`AssumeAllLive`) that run is DESCRIBED by its first
    // and last address instead of stored — every walkable object between them
    // is on the page, so the pair is exact and the list is re-derivable by the
    // same parse the sweep already uses. On the TRACED path the marks are the
    // only record of which objects are live and `clear_marks` destroys them, so
    // the list is stored as before. See `register_promoted_page_run`.
    let describe = matches!(liveness, PromotionLiveness::AssumeAllLive);
    let mut run_page: Option<usize> = None;
    let mut run_first = 0usize;
    let mut run_last = 0usize;
    let mut run_count = 0usize;
    let mut run_bytes = 0usize;
    let mut run_headers: Vec<usize> = Vec::new();

    let mut offset = 0usize;
    while offset < block.offset {
        let aligned = (offset + 7) & !7;
        if aligned >= block.offset {
            break;
        }
        let header_ptr = unsafe { block.data.add(aligned) };
        let header = header_ptr as *mut GcHeader;
        let total = unsafe { (*header).size } as usize;
        if total < crate::gc::GC_HEADER_SIZE || total > block.size - aligned {
            // Same guard the arena walkers use: an implausible size means we
            // have run off the end of the initialised region.
            break;
        }
        let obj_type = unsafe { (*header).obj_type };
        let flags = unsafe { (*header).gc_flags };
        // `Old ⟹ TENURED` (#7511): the generated barrier's fast path skips the
        // whole remembering call when the parent header has no TENURED bit, so
        // every object on a block that is about to be old-gen must carry it —
        // dead ones included, since the walk cannot prove a header will never
        // be looked at again.
        unsafe {
            crate::gc::stamp_header_promoted_in_place(header);
        }
        objects += 1;

        let live = match liveness {
            PromotionLiveness::Marked => {
                flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED) != 0
            }
            PromotionLiveness::AssumeAllLive => true,
        };
        if live && crate::gc::gc_type_is_arena_walkable(obj_type) {
            live_objects += 1;
            live_bytes += total;
            let header_addr = header_ptr as usize;
            let object_end = header_addr + total;
            let first_page = generation_page_for_addr(header_addr);
            let last_page = generation_page_for_addr(object_end - 1);
            for page in first_page..=last_page {
                let page_base = generation_page_base(page);
                let page_end = page_base + GENERATION_PAGE_SIZE;
                let overlap_start = header_addr.max(page_base);
                let overlap_end = object_end.min(page_end);
                if overlap_start >= overlap_end {
                    continue;
                }
                if run_page != Some(page) {
                    if let Some(previous) = run_page {
                        flush_page_run(
                            describe,
                            previous,
                            run_first,
                            run_last,
                            run_count,
                            &run_headers,
                            run_bytes,
                        );
                    }
                    run_page = Some(page);
                    run_first = header_addr;
                    run_count = 0;
                    run_bytes = 0;
                    run_headers.clear();
                }
                run_last = header_addr;
                run_count += 1;
                run_bytes += overlap_end - overlap_start;
                if !describe {
                    run_headers.push(header_addr);
                }
            }
        }
        offset = aligned + total;
    }
    if let Some(previous) = run_page {
        flush_page_run(
            describe,
            previous,
            run_first,
            run_last,
            run_count,
            &run_headers,
            run_bytes,
        );
    }
    debug_assert_eq!(
        offset, block.offset,
        "a promoted block did not parse to its own bump offset — its tail is \
         neither TENURED-stamped nor indexed, which is a missed old->young \
         edge (and a block the sweep walkers could not parse either)"
    );
    (objects, live_objects, live_bytes)
}

/// Put Eden back into a usable state and flip the (now both empty) survivor
/// semispaces, mirroring what `copying_reset_from_spaces_and_flip` leaves
/// behind.
///
/// Eden is left with ONE usable block; the capacity the promotion gave away is
/// handed back as trigger headroom instead of as mapped blocks (see
/// `InPlacePromotion::reserved_bytes`), so the mutator maps only what it
/// actually allocates.
fn reset_young_after_promotion() {
    crate::gc::ARENA_FREE_LIST.with(|fl| fl.borrow_mut().clear());
    crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));

    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        for block in arena.blocks.iter_mut() {
            block.offset = 0;
            block.dead_cycles = 0;
        }
        // `install_fresh_block` allocates through the NO-GC path
        // (`alloc_block_no_gc`) — the collecting `reserve_arena_block` would
        // start a second collection from inside this one.
        if arena.blocks.iter().all(|block| block.data.is_null()) {
            arena.install_fresh_block(BLOCK_SIZE);
        }
        arena.current = arena
            .blocks
            .iter()
            .position(|block| !block.data.is_null())
            .unwrap_or(0);
        INLINE_STATE.with(|s| {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
        });
    });

    for idx in [0usize, 1usize] {
        with_survivor_arena_mut(idx, |arena| {
            for block in arena.blocks.iter_mut() {
                block.offset = 0;
                block.dead_cycles = 0;
            }
            arena.current = 0;
        });
    }
    let active = ACTIVE_SURVIVOR.with(|active| active.get());
    ACTIVE_SURVIVOR.with(|active_cell| active_cell.set(1 - active));
}
