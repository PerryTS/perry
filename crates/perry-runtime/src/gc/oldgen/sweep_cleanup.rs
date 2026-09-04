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
