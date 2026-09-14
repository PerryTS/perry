//! The arena object walk of a sweep, split from `oldgen.rs` for the 2000-line
//! file cap when #10182 added block-granular reclamation to it.

use super::*;

pub(super) struct ArenaSweepObjectsState {
    cursor: crate::arena::ArenaObjectCursor,
    /// Dead old headers awaiting one batched page-index removal (see `sweep_batch`).
    pending_old_unregister: super::sweep_batch::PendingOldUnregister,
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
        if self.minor_sweep
            || !self.reclaim_dead_old_blocks
            || self.targeted_old_blocks.is_some()
            || !census.is_armed()
        {
            return;
        }
        let survivors = crate::arena::survivor_block_index_range();
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
    pub(super) fn push_live_block_holes(&mut self) {
        if self.reclaim_dead_old_blocks {
            super::old_free_rebuild_from_live_old_blocks(
                &self.block_has_live,
                self.old_block_start,
            );
            if crate::gc::gc_diag_enabled() {
                eprintln!("[gc-old-free] reusable_bytes={}", super::old_free_bytes());
            }
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        let mut done = false;
        while remaining > 0 {
            let Some((header_ptr, block_idx)) = self.cursor.next() else {
                done = true;
                break;
            };
            remaining -= 1;
            self.process_object(header_ptr as *mut GcHeader, block_idx);
        }
        // Never leave a dead header in the page index across a step boundary.
        self.pending_old_unregister.flush();
        done
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
            crate::arena::old_page_account_swept_object(
                header as usize,
                (*header).size as usize,
                true,
                pinned,
            );
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
            crate::arena::old_page_account_swept_object(header as usize, total_size, false, false);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
        layout_clear_for_ptr(user_ptr as usize);
        if self.overflow_active {
            gc_type_clear_dead_payload_side_tables((*header).obj_type, user_ptr as usize);
        }
        if self.reclaim_dead_old_blocks && dead_old {
            self.pending_old_unregister.defer(header, total_size);
        } else {
            (*header).gc_flags = flags & !(GC_FLAG_FORWARDED | GC_FLAG_MARKED);
        }
    }

    unsafe fn reclaim_dead_object(&mut self, header: *mut GcHeader, block_idx: usize) {
        let total_size = (*header).size as usize;
        let dead_old = block_idx >= self.old_block_start;
        if dead_old {
            crate::arena::old_page_account_swept_object(header as usize, total_size, false, false);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
        if block_idx < self.resettable_general_n {
            self.eden_dead_bytes = self.eden_dead_bytes.saturating_add(total_size as u64);
        }
        finalize_dead_arena_payload(header, user_ptr, self.overflow_active);
        if self.reclaim_dead_old_blocks && dead_old {
            self.pending_old_unregister.defer(header, total_size);
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
