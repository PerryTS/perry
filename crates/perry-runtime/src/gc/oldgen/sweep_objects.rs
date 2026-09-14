//! The arena object walk of a sweep, split from `oldgen.rs` for the 2000-line
//! file cap when #10182 added block-granular reclamation to it.

use super::*;

pub(super) struct ArenaSweepObjectsState {
    cursor: crate::arena::ArenaObjectCursor,
    /// Dead old headers awaiting one batched page-index removal (see `sweep_batch`).
    pending_old_unregister: super::sweep_batch::PendingOldUnregister,
    /// Page accounting of consecutive single-page old objects, applied once per
    /// page (see `arena::page_meta::sweep_tally`). `usize::MAX` when empty.
    old_page_tally_page: usize,
    old_page_tally: crate::arena::OldPageSweepTally,
    block_snapshots: Vec<crate::arena::ArenaBlockSnapshot>,
    block_has_live: Vec<bool>,
    resettable_general_n: usize,
    old_block_start: usize,
    overflow_active: bool,
    do_age_bump: bool,
    reclaim_dead_old_blocks: bool,
    /// This sweep follows a MINOR trace, whose mark bits say nothing about the
    /// old generation: old-gen parents are black leaves whose slots are only
    /// visited through dirty remembered-set pages, so an object reachable only
    /// from a non-dirty old parent is never marked. "Unmarked" therefore does
    /// NOT imply "dead" for anything in the old generation.
    ///
    /// Two consequences, both handled below:
    ///
    /// * Forwarding stubs must ALL be retained: array growth installs
    ///   PERMANENT stubs (#6228 — stale pre-growth pointers keep resolving for
    ///   reads, references are never rewritten). An old parent (e.g. a
    ///   long-lived Map's entries buffer) whose page is no longer dirty never
    ///   marks the stub its slot points at, so reclaiming it is a
    ///   use-after-free.
    /// * Ordinary old-gen objects must not be reclaimed either (#6892). The
    ///   minor never frees their memory, but `reclaim_dead_object` still runs
    ///   `finalize_dead_arena_payload` on them, which wipes a LIVE object's GC
    ///   slot-layout mask and payload side tables and frees its external
    ///   payload buffers.
    ///
    /// Full traces DO visit every live parent, so mark-based reclaim stays
    /// sound there (and bounds the accumulation).
    minor_sweep: bool,
    /// Old-gen blocks selected for page defrag this cycle. Every indexed
    /// occupant was evacuated out during this same cycle, so what is left
    /// really is reclaimable even in a minor — and the block-level reclaim
    /// needs `block_has_live` to stay false for them.
    targeted_old_blocks: Option<crate::fast_hash::PtrHashSet<usize>>,
    pub(super) freed_bytes: u64,
    pub(super) retained_forwarded_stub_objects: usize,
    pub(super) retained_forwarded_stub_bytes: usize,
    /// #7598 Eden census: see `SweepTraceStats`.
    pub(super) eden_live_bytes: u64,
    pub(super) eden_dead_bytes: u64,
    pub(super) arena_live_bytes: u64,
    /// #7901: see `SweepTraceStats::arena_live_from_space_bytes`.
    pub(super) arena_live_from_space_bytes: u64,
    active_survivor_blocks: std::ops::Range<usize>,
    /// #10182: blocks this sweep reclaims without entering, and what they held.
    block_skip_blocks: u64,
    block_skip_objects: u64,
    block_skip_bytes: u64,
    /// #10182: per block, the census parsed every header and none of them was
    /// invalidated, and the block has not changed since. Empty unless the
    /// block skip ran against an armed census.
    /// Cleared for a block as soon as this sweep invalidates a header in it.
    census_hole_free: Vec<bool>,
    /// #10241: a promoted-cohort full armed `promoted_cohort`'s survival
    /// probe, and this is the synchronous full sweep that answers it.
    survival_probe: bool,
}

impl ArenaSweepObjectsState {
    pub(super) fn new(
        do_age_bump: bool,
        reclaim_dead_old_blocks: bool,
        minor_sweep: bool,
        targeted_old_blocks: Option<crate::fast_hash::PtrHashSet<usize>>,
    ) -> Self {
        let n_blocks = crate::arena::arena_block_count();
        let block_snapshots = crate::arena::arena_block_snapshots();
        crate::arena::old_pages_reset_sweep_accounting();
        Self {
            cursor: crate::arena::ArenaObjectCursor::new(crate::arena::ArenaWalkOrder::BlockIndex),
            pending_old_unregister: Default::default(),
            old_page_tally_page: usize::MAX,
            old_page_tally: Default::default(),
            block_snapshots,
            block_has_live: vec![false; n_blocks],
            resettable_general_n: crate::arena::general_block_count(),
            old_block_start: crate::arena::longlived_end(),
            // Wave 2: also arms the closure dynamic-props dead-payload arm
            // (one gate check per sweep-state build, not per object).
            overflow_active: !crate::object::overflow_fields_is_empty()
                || crate::closure::closure_dynamic_side_tables_nonempty(),
            do_age_bump,
            reclaim_dead_old_blocks,
            minor_sweep,
            targeted_old_blocks,
            freed_bytes: 0,
            retained_forwarded_stub_objects: 0,
            retained_forwarded_stub_bytes: 0,
            eden_live_bytes: 0,
            eden_dead_bytes: 0,
            arena_live_bytes: 0,
            arena_live_from_space_bytes: 0,
            active_survivor_blocks: crate::arena::active_survivor_block_index_range(),
            block_skip_blocks: 0,
            block_skip_objects: 0,
            block_skip_bytes: 0,
            census_hole_free: Vec::new(),
            survival_probe: false,
        }
    }

    /// #10182: reclaim every dead, obligation-free block without visiting its
    /// objects. See `gc::trace::block_skip` for the soundness argument; this
    /// is the half that decides, per block of THIS sweep's snapshot:
    ///
    /// * the sweep follows a full trace that reclaims dead old blocks (a
    ///   minor's marks say nothing about old-gen, and a targeted defrag sweep
    ///   keeps its own per-object accounting);
    /// * the census walked the same block (`data`) up to the same bump offset,
    ///   so nothing was born into it after the census;
    /// * no census hit landed in it and no header in it owed per-object work;
    /// * the cleanup that follows resets or releases the block when it has no
    ///   live object: general blocks outside the recent window, survivor
    ///   blocks, and old blocks. The recent general window keeps its dead
    ///   blocks across a sweep and the longlived arena has no dead-block
    ///   cleanup, so both keep the per-object path.
    ///
    /// A skipped block contributes nothing to `block_has_live`, which is the
    /// only liveness the cleanup reads.
    pub(super) fn apply_block_skip(&mut self, census: &super::super::trace::BlockCensus) {
        // Reached only from a synchronous full sweep, which is the one whose
        // marks are final and whose whole-block walk the probe reads.
        self.survival_probe =
            !self.minor_sweep && super::super::promoted_cohort::survival::survival_probe_armed();
        if self.minor_sweep
            || !self.reclaim_dead_old_blocks
            || self.targeted_old_blocks.is_some()
            || !census.is_armed()
        {
            return;
        }
        let survivors = crate::arena::survivor_block_index_range();
        #[cfg(not(test))]
        let forget_holes = false;
        #[cfg(test)]
        let forget_holes = super::super::trace::block_skip::sabotage::get()
            & super::super::trace::block_skip::sabotage::FORGET_HOLES
            != 0;
        self.census_hole_free = self
            .block_snapshots
            .iter()
            .enumerate()
            .map(|(block_idx, snapshot)| {
                census.block(block_idx).is_some_and(|block| {
                    block.whole_walk
                        && (!block.non_walkable || forget_holes)
                        && block.data == snapshot.data
                        && block.end == snapshot.data.saturating_add(snapshot.offset)
                })
            })
            .collect();
        let mut skip = vec![false; self.block_snapshots.len()];
        let mut any = false;
        for (block_idx, snapshot) in self.block_snapshots.iter().enumerate() {
            if snapshot.data == 0 || snapshot.offset == 0 {
                continue;
            }
            let cleanup_reclaims_dead_block = if block_idx < self.resettable_general_n {
                !crate::arena::general_block_in_recent_window(block_idx)
            } else {
                survivors.contains(&block_idx) || block_idx >= self.old_block_start
            };
            if !cleanup_reclaims_dead_block {
                continue;
            }
            let Some(block) = census.block(block_idx) else {
                continue;
            };
            #[cfg(not(test))]
            let (obligation, reached) = (block.obligation, census.reached(block_idx));
            #[cfg(test)]
            let (obligation, reached) = {
                use super::super::trace::block_skip::sabotage;
                let bits = sabotage::get();
                (
                    block.obligation && bits & sabotage::FORGET_OBLIGATIONS == 0,
                    census.reached(block_idx) && bits & sabotage::FORGET_REACHED == 0,
                )
            };
            if obligation
                || reached
                || block.data != snapshot.data
                || block.end != snapshot.data.saturating_add(snapshot.offset)
            {
                continue;
            }
            skip[block_idx] = true;
            any = true;
            if self.survival_probe {
                super::super::promoted_cohort::survival::note_probe_block_skipped(
                    snapshot.data,
                    snapshot.offset,
                );
            }
            self.freed_bytes = self.freed_bytes.saturating_add(block.bytes);
            if block_idx < self.resettable_general_n {
                self.eden_dead_bytes = self.eden_dead_bytes.saturating_add(block.bytes);
            }
            self.block_skip_blocks += 1;
            self.block_skip_objects = self.block_skip_objects.saturating_add(block.objects);
            self.block_skip_bytes = self.block_skip_bytes.saturating_add(block.bytes);
        }
        #[cfg(test)]
        super::super::trace::block_skip::sabotage::record_skipped_block_bases(
            skip.iter()
                .enumerate()
                .filter(|&(_, &skipped)| skipped)
                .map(|(block_idx, _)| self.block_snapshots[block_idx].data)
                .collect(),
        );
        if any {
            #[cfg(test)]
            if super::super::trace::block_skip::sabotage::get() == 0 {
                verify_skipped_blocks_hold_no_live_object(&skip);
            }
            self.cursor.set_skip_blocks(skip);
            super::super::trace::block_skip::note_block_skip_reclaimed(
                self.block_skip_blocks,
                self.block_skip_objects,
                self.block_skip_bytes,
            );
        }
    }

    /// #7437: rebuild the old-gen hole free list once the object walk
    /// completes — block liveness is final at that point, and the block
    /// cleanup that follows only touches blocks with NO live object, which
    /// the rebuild's filter already skips.
    ///
    /// #10182: the rebuild also skips a live block that provably holds no
    /// hole, i.e. no header with `obj_type == 0`. Those headers are produced
    /// only by invalidating a dead old object, and consumed only by reuse.
    /// A block qualifies when this cycle's census parsed all of its headers
    /// and found none that does not parse as an object, the block has not
    /// grown since, and this sweep invalidated nothing in it. Nothing else in a
    /// synchronous full writes a header between the census and here. On a
    /// pacing full that keeps one promoted JSON tree, the rebuild otherwise
    /// re-parses the whole tree to find no hole.
    pub(super) fn push_live_block_holes(&mut self) {
        if self.reclaim_dead_old_blocks {
            let old_block_start = self.old_block_start;
            let block_has_live = &self.block_has_live;
            let hole_free = &self.census_hole_free;
            let mut skipped = 0u64;
            super::old_free_rebuild_from_old_blocks(|block_idx| {
                if block_idx < old_block_start
                    || !block_has_live.get(block_idx).copied().unwrap_or(false)
                {
                    return false;
                }
                if hole_free.get(block_idx).copied().unwrap_or(false) {
                    skipped += 1;
                    return false;
                }
                true
            });
            super::super::trace::block_skip::note_hole_rebuild_blocks_skipped(skipped);
            if crate::gc::gc_diag_enabled() {
                eprintln!(
                    "[gc-old-free] reusable_bytes={} rebuild_skipped_blocks={skipped}",
                    super::old_free_bytes()
                );
            }
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        let mut done = false;
        if budget == usize::MAX && self.cursor.at_block_boundary() {
            while let Some((block_idx, data, offset, size)) = self.cursor.next_whole_block() {
                let live_before = self.arena_live_bytes;
                // SAFETY: the block was snapshotted by this sweep's cursor.
                unsafe { self.sweep_whole_block(block_idx, data, offset, size) };
                if self.survival_probe {
                    super::super::promoted_cohort::survival::note_probe_block_swept(
                        data,
                        offset,
                        self.arena_live_bytes - live_before,
                    );
                }
            }
            remaining = 0;
            done = true;
        }
        while remaining > 0 {
            let Some((header_ptr, block_idx)) = self.cursor.next() else {
                done = true;
                break;
            };
            remaining -= 1;
            self.process_object(header_ptr as *mut GcHeader, block_idx);
        }
        // Never leave a dead header in the page index, or a page's accounting
        // unapplied, across a step boundary. The tally goes first: the
        // unregister flush zeroes the accounting of pages it empties.
        if self.page_tally_order_kept() {
            self.apply_old_page_tally();
            self.pending_old_unregister.flush();
        } else {
            self.pending_old_unregister.flush();
            self.apply_old_page_tally();
        }
        done
    }

    /// Sweep one whole block in a single pass (#10182). It visits exactly the
    /// headers `ArenaObjectCursor::next_budgeted` yields for the block and
    /// handles each exactly as `process_object` would. The common case — a
    /// marked, unpinned, unforwarded object in a block that does not age-bump —
    /// is `keep_live_object` with the per-block constants (old or general,
    /// from-space membership, age bumping) hoisted out of the loop; every other
    /// header goes through `process_object` unchanged.
    ///
    /// # Safety
    /// `data`/`offset`/`size` are an arena block as this sweep's cursor
    /// snapshotted it.
    unsafe fn sweep_whole_block(
        &mut self,
        block_idx: usize,
        data: usize,
        offset: usize,
        size: usize,
    ) {
        let is_old = block_idx >= self.old_block_start;
        let general = block_idx < self.resettable_general_n;
        let age_bump = self.do_age_bump && general;
        let from_space = crate::arena::block_in_copying_from_space(
            block_idx,
            self.resettable_general_n,
            &self.active_survivor_blocks,
        );
        #[cfg(test)]
        let record_live = super::super::trace::block_skip::sabotage::get()
            & super::super::trace::block_skip::sabotage::FORGET_WHOLE_BLOCK_LIVE
            == 0;
        #[cfg(not(test))]
        let record_live = true;
        let mut kept_live = false;
        let mut cursor = 0usize;
        while cursor < offset {
            let aligned = (cursor + 7) & !7;
            if aligned >= offset {
                break;
            }
            let header = (data + aligned) as *mut GcHeader;
            let total_size = (*header).size as usize;
            if total_size == 0 || total_size > size {
                break;
            }
            cursor = aligned + total_size;
            if !crate::gc::gc_type_is_arena_walkable((*header).obj_type) {
                continue;
            }
            let flags = (*header).gc_flags;
            if age_bump
                || flags & (GC_FLAG_MARKED | GC_FLAG_PINNED | GC_FLAG_FORWARDED) != GC_FLAG_MARKED
            {
                self.process_object(header, block_idx);
                continue;
            }
            if is_old {
                self.account_old_object(header, total_size, true, false);
            }
            kept_live = true;
            if general {
                self.eden_live_bytes = self.eden_live_bytes.saturating_add(total_size as u64);
            }
            self.arena_live_bytes = self.arena_live_bytes.saturating_add(total_size as u64);
            if from_space {
                self.arena_live_from_space_bytes = self
                    .arena_live_from_space_bytes
                    .saturating_add(total_size as u64);
            }
            (*header).gc_flags = flags & !GC_FLAG_MARKED;
        }
        if kept_live && record_live {
            if let Some(slot) = self.block_has_live.get_mut(block_idx) {
                *slot = true;
            }
        }
    }

    /// Account one swept old object on its page(s), batching single-page
    /// objects per page.
    #[inline(always)]
    fn account_old_object(
        &mut self,
        header: *mut GcHeader,
        total_size: usize,
        live: bool,
        pinned: bool,
    ) {
        match crate::arena::old_object_single_page(header as usize, total_size) {
            Some(page) => {
                if page != self.old_page_tally_page {
                    self.apply_old_page_tally();
                    self.old_page_tally_page = page;
                }
                self.old_page_tally.add(total_size, live, pinned);
            }
            None => crate::arena::old_page_account_swept_object(
                header as usize,
                total_size,
                live,
                pinned,
            ),
        }
    }

    fn apply_old_page_tally(&mut self) {
        if self.old_page_tally_page != usize::MAX {
            crate::arena::old_page_account_swept_tally(
                self.old_page_tally_page,
                &self.old_page_tally,
            );
        }
        self.old_page_tally_page = usize::MAX;
        self.old_page_tally = Default::default();
    }

    /// The tally is applied before every page-index flush (always, outside the
    /// sabotaged test).
    #[inline(always)]
    fn page_tally_order_kept(&self) -> bool {
        #[cfg(test)]
        {
            use super::super::trace::block_skip::sabotage;
            sabotage::get() & sabotage::FORGET_PAGE_TALLY_ORDER == 0
        }
        #[cfg(not(test))]
        true
    }

    /// Queue a dead old header's page-index removal, applying the page tally
    /// first when the queue is about to flush.
    unsafe fn defer_old_unregister(&mut self, header: *mut GcHeader, total_size: usize) {
        if self.pending_old_unregister.flushes_on_next_defer() && self.page_tally_order_kept() {
            self.apply_old_page_tally();
        }
        self.pending_old_unregister.defer(header, total_size);
    }

    pub(super) fn block_has_live(&self) -> &[bool] {
        &self.block_has_live
    }

    pub(super) fn block_snapshots(&self) -> &[crate::arena::ArenaBlockSnapshot] {
        &self.block_snapshots
    }

    pub(super) fn maybe_print_diag(&self) {
        if !crate::gc::gc_diag_enabled() {
            return;
        }
        let live_general = (0..self.resettable_general_n)
            .filter(|&i| self.block_has_live[i])
            .count();
        let live_ll = (self.resettable_general_n..self.block_has_live.len())
            .filter(|&i| self.block_has_live[i])
            .count();
        eprintln!(
            "[gc] blocks: general={} ({} live), non_general={} ({} live, survivors+longlived+old), freed_bytes={} retained_forwarded_stub_bytes={} retained_forwarded_stub_objects={} block_skip_reclaimed_blocks={} block_skip_reclaimed_objects={} block_skip_reclaimed_bytes={}",
            self.resettable_general_n,
            live_general,
            self.block_has_live.len() - self.resettable_general_n,
            live_ll,
            self.freed_bytes,
            self.retained_forwarded_stub_bytes,
            self.retained_forwarded_stub_objects,
            self.block_skip_blocks,
            self.block_skip_objects,
            self.block_skip_bytes,
        );
    }

    fn process_object(&mut self, header: *mut GcHeader, block_idx: usize) {
        unsafe {
            let age_bump_this = self.do_age_bump && block_idx < self.resettable_general_n;
            let flags = (*header).gc_flags;
            if flags == 0 {
                self.reclaim_dead_object(header, block_idx);
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                self.keep_live_object(header, block_idx, flags, age_bump_this, true, true);
                return;
            }
            if flags & GC_FLAG_FORWARDED != 0 {
                self.process_forwarded_object(header, block_idx, flags);
                return;
            }
            if flags & GC_FLAG_MARKED == 0 && self.unmarked_is_provably_dead(block_idx) {
                self.reclaim_dead_object(header, block_idx);
            } else {
                self.keep_live_object(header, block_idx, flags, age_bump_this, false, true);
            }
        }
    }

    /// Does `flags & MARKED == 0` actually prove this object is garbage?
    ///
    /// Only when the trace that produced the marks covered the object's
    /// generation. A minor trace never marks the old generation (see
    /// `minor_sweep`), so an unmarked old-gen object is merely *unvisited* —
    /// it stays live and must not be finalized. #6892: reclaiming one wiped
    /// the GC slot-layout mask of a live old-gen array, after which the next
    /// `layout_note_slot` rebuilt the mask from a single slot and the
    /// following minor stopped tracing the array's other pointer elements,
    /// sweeping objects that were still referenced.
    ///
    /// The old-page defrag targets are exempt: this cycle evacuated every
    /// indexed occupant, so the remainder is genuinely reclaimable.
    #[inline]
    fn unmarked_is_provably_dead(&self, block_idx: usize) -> bool {
        if !self.minor_sweep || block_idx < self.old_block_start {
            return true;
        }
        self.targeted_old_blocks
            .as_ref()
            .is_some_and(|selected| selected.contains(&block_idx))
    }
}

impl ArenaSweepObjectsState {
    unsafe fn keep_live_object(
        &mut self,
        header: *mut GcHeader,
        block_idx: usize,
        flags: u8,
        age_bump_this: bool,
        pinned: bool,
        count_in_live_census: bool,
    ) {
        if block_idx >= self.old_block_start {
            self.account_old_object(header, (*header).size as usize, true, pinned);
        }
        if block_idx < self.block_has_live.len() {
            self.block_has_live[block_idx] = true;
        }
        if block_idx < self.resettable_general_n {
            self.eden_live_bytes = self.eden_live_bytes.saturating_add((*header).size as u64);
        }
        if count_in_live_census {
            let size = (*header).size as u64;
            self.arena_live_bytes = self.arena_live_bytes.saturating_add(size);
            // #7901: the from-space share of the census, so a following copied
            // minor can remove exactly what it replaces.
            if crate::arena::block_in_copying_from_space(
                block_idx,
                self.resettable_general_n,
                &self.active_survivor_blocks,
            ) {
                self.arena_live_from_space_bytes =
                    self.arena_live_from_space_bytes.saturating_add(size);
            }
        }
        if age_bump_this && flags & GC_FLAG_TENURED == 0 {
            if flags & GC_FLAG_HAS_SURVIVED != 0 {
                (*header).gc_flags =
                    (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED & !GC_FLAG_MARKED;
            } else {
                (*header).gc_flags = (flags | GC_FLAG_HAS_SURVIVED) & !GC_FLAG_MARKED;
            }
        } else {
            (*header).gc_flags = flags & !GC_FLAG_MARKED;
        }
    }

    unsafe fn process_forwarded_object(
        &mut self,
        header: *mut GcHeader,
        block_idx: usize,
        flags: u8,
    ) {
        // See `minor_sweep`: a minor cannot prove a stub unreferenced (old-gen
        // parents are black leaves), so it must keep them all; a full trace
        // reclaims the genuinely unreferenced ones.
        let retain_stub = self.minor_sweep
            || flags & GC_FLAG_MARKED != 0
            || (block_idx < self.resettable_general_n
                && crate::arena::general_block_in_recent_window(block_idx));
        if retain_stub {
            // A full collection can leave an unmarked stub in the recent-block
            // safety window. It still pins the block, but it is proven dead and
            // therefore excluded from live-allocation accounting.
            let count_in_live_census = self.minor_sweep || flags & GC_FLAG_MARKED != 0;
            self.keep_live_object(header, block_idx, flags, false, false, count_in_live_census);
            if block_idx < self.resettable_general_n {
                self.retained_forwarded_stub_objects =
                    self.retained_forwarded_stub_objects.saturating_add(1);
                self.retained_forwarded_stub_bytes = self
                    .retained_forwarded_stub_bytes
                    .saturating_add((*header).size as usize);
            }
            return;
        }

        let total_size = (*header).size as usize;
        let dead_old = block_idx >= self.old_block_start;
        if dead_old {
            self.account_old_object(header, total_size, false, false);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
        layout_clear_for_ptr(user_ptr as usize);
        if self.overflow_active {
            gc_type_clear_dead_payload_side_tables((*header).obj_type, user_ptr as usize);
        }
        if self.reclaim_dead_old_blocks && dead_old {
            self.note_invalidated(block_idx);
            self.defer_old_unregister(header, total_size);
        } else {
            (*header).gc_flags = flags & !(GC_FLAG_FORWARDED | GC_FLAG_MARKED);
        }
    }

    unsafe fn reclaim_dead_object(&mut self, header: *mut GcHeader, block_idx: usize) {
        let total_size = (*header).size as usize;
        let dead_old = block_idx >= self.old_block_start;
        if dead_old {
            self.account_old_object(header, total_size, false, false);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
        if block_idx < self.resettable_general_n {
            self.eden_dead_bytes = self.eden_dead_bytes.saturating_add(total_size as u64);
        }
        finalize_dead_arena_payload(header, user_ptr, self.overflow_active);
        if self.reclaim_dead_old_blocks && dead_old {
            self.note_invalidated(block_idx);
            self.defer_old_unregister(header, total_size);
        }
    }

    #[inline]
    fn note_invalidated(&mut self, block_idx: usize) {
        if let Some(slot) = self.census_hole_free.get_mut(block_idx) {
            *slot = false;
        }
    }
}

/// Test builds re-check every skip against the headers themselves: a block the
/// census recorded as unreached must hold no marked or pinned object. This is
/// what turns a missed mark path into a failing unit test instead of a freed
/// live object.
#[cfg(test)]
fn verify_skipped_blocks_hold_no_live_object(skip: &[bool]) {
    crate::arena::arena_walk_objects_filtered(
        |block_idx| skip.get(block_idx).copied().unwrap_or(false),
        |header_ptr, block_idx| unsafe {
            let flags = (*(header_ptr as *const GcHeader)).gc_flags;
            assert!(
                flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0,
                "block-granular sweep skipped block {block_idx}, which holds a live header {header_ptr:p} (flags {flags:#x})"
            );
        },
    );
}
