//! Arena sweep-cleanup state machine, split from `oldgen.rs` for the
//! 2000-line file cap (#9644 grew the defrag path).

use super::*;

pub(super) struct ArenaSweepCleanupState {
    subphase: ArenaSweepCleanupSubphase,
    general: crate::arena::ArenaResetEmptyBlocksState,
    survivor: Option<crate::arena::SurvivorArenaReclaimDeadBlocksState>,
    old: Option<crate::arena::OldArenaReclaimDeadBlocksState>,
    stats: crate::arena::ArenaResetStats,
}

impl ArenaSweepCleanupState {
    pub(super) fn new(
        block_has_live: &[bool],
        block_snapshots: &[crate::arena::ArenaBlockSnapshot],
        reclaim_dead_old_blocks: bool,
        targeted_old_blocks: Option<&crate::fast_hash::PtrHashSet<usize>>,
    ) -> Self {
        let survivor = reclaim_dead_old_blocks.then(|| {
            crate::arena::SurvivorArenaReclaimDeadBlocksState::new(block_has_live, block_snapshots)
        });
        let old = if reclaim_dead_old_blocks {
            Some(crate::arena::OldArenaReclaimDeadBlocksState::new_full(
                block_has_live,
                block_snapshots,
            ))
        } else {
            targeted_old_blocks.map(|selected| {
                crate::arena::OldArenaReclaimDeadBlocksState::new_selected(
                    block_has_live,
                    block_snapshots,
                    selected,
                )
            })
        };
        Self {
            subphase: ArenaSweepCleanupSubphase::General,
            general: crate::arena::ArenaResetEmptyBlocksState::new(block_has_live, block_snapshots),
            survivor,
            old,
            stats: crate::arena::ArenaResetStats::default(),
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        match self.subphase {
            ArenaSweepCleanupSubphase::General => {
                if self.general.step(budget) {
                    self.stats = add_reset_stats(self.stats, self.general.stats());
                    self.subphase = ArenaSweepCleanupSubphase::Survivor;
                }
                false
            }
            ArenaSweepCleanupSubphase::Survivor => {
                if let Some(survivor) = self.survivor.as_mut() {
                    if !survivor.step(budget) {
                        return false;
                    }
                    self.stats = add_reset_stats(self.stats, survivor.stats());
                }
                self.subphase = ArenaSweepCleanupSubphase::Old;
                false
            }
            ArenaSweepCleanupSubphase::Old => {
                if let Some(old) = self.old.as_mut() {
                    if !old.step(budget) {
                        return false;
                    }
                    self.stats = add_reset_stats(self.stats, old.stats());
                }
                self.subphase = ArenaSweepCleanupSubphase::Done;
                true
            }
            ArenaSweepCleanupSubphase::Done => true,
        }
    }

    pub(super) fn stats(&self) -> crate::arena::ArenaResetStats {
        self.stats
    }
}

impl IncrementalSweepState {
    /// #10182: a synchronous full that intends to promote its young generation
    /// in place must leave the young blocks in a state the promotion walk can
    /// read. `finish_in_place_promotion`'s only liveness sources are the mark
    /// bits (which this sweep clears as it goes) and "is this header's
    /// `obj_type` arena-walkable" — so the sweep invalidates the headers of the
    /// dead young objects it reclaims, exactly the way it already invalidates
    /// dead OLD ones (`invalidate_dead_old_arena_header`). After that the
    /// promotion's `PromotionLiveness::AssumeAllLive` walk registers precisely
    /// the survivors, and the page-run description re-parses to the same count
    /// because producer and expander apply the same `gc_type_is_arena_walkable`
    /// filter.
    ///
    /// Here rather than in `oldgen.rs` for the 2000-line file cap.
    pub(in crate::gc) fn invalidating_dead_young_headers(mut self, on: bool) -> Self {
        self.arena.invalidate_dead_young_headers = on;
        self
    }
}

impl ArenaSweepObjectsState {
    /// #10182: the per-object half of [`IncrementalSweepState::invalidating_dead_young_headers`].
    ///
    /// Only young (from-space: Eden + active survivor) blocks, only when the
    /// cycle planned a promotion. No `unregister_old_object_pages`: these
    /// objects were young, so they were never in the old-gen page index. `size`
    /// is deliberately preserved — every arena walker hops by it, the promotion
    /// walk and the page-run expansion included.
    #[inline]
    pub(super) unsafe fn invalidate_dead_young_header_for_promotion(
        &self,
        header: *mut GcHeader,
        block_idx: usize,
    ) {
        if !self.invalidate_dead_young_headers
            || !crate::arena::block_in_copying_from_space(
                block_idx,
                self.resettable_general_n,
                &self.active_survivor_blocks,
            )
        {
            return;
        }
        (*header).obj_type = 0;
        (*header).gc_flags = 0;
        (*header)._reserved = 0;
    }
}
