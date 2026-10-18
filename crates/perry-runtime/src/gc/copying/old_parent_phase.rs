//! Root-first ordering of the copying minor, and what it decides.
//!
//! A child module, so `use super::*` reaches the parent's private items.
//!
//! The minor drains everything the precise roots reach (native stack and
//! statepoints, globals, handles, pinned and registered native roots) to
//! completion BEFORE it seeds from the remembered set. A young object first
//! met after that point is reachable only through old parents, and a minor
//! cannot tell a live old parent from a dead one: it never traces old-gen.
//! effect's 32 MiB schedule is the case that made this matter: one dead,
//! tenured 2,000-element array held 435k young objects (21.7 MB) through its
//! dirty slots, the minor promoted all of them, and the next full swept them
//! (reports/2026-10-18-gctrigger.md).
//!
//! So an object first discovered in the remembered-set phase is not promoted
//! by the adaptive threshold. It is copied into survivor space, its age still
//! advances, and it is promoted only by the obligations that hold for every
//! young object: the maximum representable age
//! ([`GC_COPY_PROMOTION_SURVIVALS`]), an already-tenured header, and the
//! never-move kinds. If the old parent is live, the object is promoted once
//! it reaches that age; if the parent is dead, the next full frees the parent,
//! its remembered entry goes with it, and the object dies young.
//!
//! The one per-cycle input is the survivor budget, frozen when the collector
//! is created: survivor space may hold at most one effective nursery of
//! copies. A kept-young object that would take survivor space past it is
//! promoted as before and counted, so an old-parent cohort larger than the
//! nursery cannot grow survivor space without bound.
//!
//! The decision is the traversal order of this one collection. What a minor
//! kept young is reported once to `promoted_cohort`, which schedules a precise
//! full at the same safepoint when the copy work that cohort is still owed
//! outprices the full (stage 2): a dead parent is then freed before its
//! cohort is copied again or promoted.

use super::*;

/// Survivor space a single minor may fill: one effective nursery, never less
/// than the base cap. Read once per cycle (see `CopyingNurseryCollector::new`).
pub(in crate::gc) fn old_parent_survivor_budget_bytes() -> usize {
    #[cfg(test)]
    if let Some(forced) = SURVIVOR_BUDGET_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    crate::gc::tenuring::scavenge_nursery_cap_effective_bytes()
        .max(crate::gc::policy::gc_scavenge_nursery_cap_bytes())
}

#[cfg(test)]
thread_local! {
    /// Scoped budget pin, so the overflow fallback is testable without
    /// filling a whole nursery. Thread-local like the tenuring pin.
    static SURVIVOR_BUDGET_TEST_OVERRIDE: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Pin the survivor budget on this thread until the guard drops.
#[cfg(test)]
pub(in crate::gc) fn set_survivor_budget_for_test(bytes: usize) -> SurvivorBudgetTestGuard {
    SurvivorBudgetTestGuard(SURVIVOR_BUDGET_TEST_OVERRIDE.with(|cell| cell.replace(Some(bytes))))
}

#[cfg(test)]
pub(in crate::gc) struct SurvivorBudgetTestGuard(Option<usize>);

#[cfg(test)]
impl Drop for SurvivorBudgetTestGuard {
    fn drop(&mut self) {
        SURVIVOR_BUDGET_TEST_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

impl CopyingNurseryCollector {
    /// Called once, after the root phase's drain has reached its fixed point.
    /// Every young object moved from here on was first reached through an old
    /// parent.
    pub(in crate::gc) fn begin_old_parent_phase(&mut self) {
        self.old_parent_phase = true;
    }

    /// `move_young`'s threshold arm for an object first reached in the
    /// remembered-set phase: `true` promotes it (survivor budget exhausted),
    /// `false` keeps it young. Counts both outcomes.
    #[inline]
    pub(in crate::gc) fn old_parent_threshold_promotes(&mut self, total: usize) -> bool {
        if self.stats.copied_bytes.saturating_add(total) > self.survivor_budget_bytes {
            self.stats.old_parent_overflow_promoted_objects += 1;
            self.stats.old_parent_overflow_promoted_bytes += total;
            return true;
        }
        self.stats.old_parent_kept_young_objects += 1;
        self.stats.old_parent_kept_young_bytes += total;
        false
    }

    /// Every young object moved in the remembered-set phase, whatever its
    /// destination. One that stays young is owed one copy per survival left
    /// before the maximum age, if its old parents are live.
    #[inline]
    pub(in crate::gc) fn note_old_parent_only(&mut self, total: usize, next_age: u8, promoted: bool) {
        self.stats.old_parent_only_objects += 1;
        self.stats.old_parent_only_bytes += total;
        if !promoted {
            self.stats.old_parent_young_bytes += total;
            let copies_left = GC_COPY_PROMOTION_SURVIVALS.saturating_sub(next_age) as usize;
            self.stats.old_parent_owed_copy_bytes += total * copies_left;
        }
    }
}
