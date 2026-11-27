//! Which collection is due: the ladder a budgeted start, a safepoint and the
//! trigger watermark read, the old-reclaim debt it prices, and the hot
//! thread-locals its fast path reads.

use super::*;

pub(in crate::gc) fn gc_old_reclaim_debt_bytes(old_in_use: usize, baseline: usize) -> u64 {
    // #10928: one trigger, shared with `old_reclaim_pressure_due` -- the
    // absolute first-crossing arm it used to mirror no longer exists, and a
    // debt computed against a trigger the predicate does not use is exactly
    // the #7024 two-predicates-collapse the shared helper exists to prevent.
    let trigger = baseline.saturating_add(gc_old_reclaim_growth_band_bytes(baseline));
    old_in_use.saturating_sub(trigger) as u64
}

/// The thread-locals [`gc_budgeted_due_trigger`] reads on its fast path, with
/// the hot-cache slot each one claimed.
///
/// Enumerating them in the module that reads them is what makes the coverage
/// test able to fail: reverting any declaration below to a raw
/// `thread_local!` removes `slot_index` and breaks this function's build at
/// the declaration's own name, rather than leaving a silently slow path — the
/// failure mode `tls_hot.rs` was written to abolish and that this path
/// nonetheless kept for three years.
#[cfg(test)]
pub(crate) fn trigger_path_hot_slot_indices() -> Vec<(&'static str, u32)> {
    // Touch each one first: a slot is claimed on first read, not at
    // declaration, so an unread declaration reports the unassigned sentinel.
    //
    // `GC_DEFERRED_REQUEST` is deliberately absent: `defer_gc_request` reads it
    // only when `GC_ROOT_LOCK_DEPTH` is non-zero, so on the fast path it is
    // never touched and never claims a slot. Listing it made this function's
    // own test fail with `index 4294967295` on the first run — which is the
    // evidence that the test can fail, and the reason the list is "what the
    // fast path reads" rather than "what the module declares".
    let _ = gc_budgeted_due_trigger();
    let _ = super::trigger_watermark::nothing_due();
    vec![
        (
            "GC_OLD_RECLAIM_PENDING",
            GC_OLD_RECLAIM_PENDING.slot_index(),
        ),
        (
            "GC_LAST_OLD_RECLAIM_IN_USE_BYTES",
            GC_LAST_OLD_RECLAIM_IN_USE_BYTES.slot_index(),
        ),
        (
            "GC_NEXT_MALLOC_TRIGGER",
            GC_NEXT_MALLOC_TRIGGER.slot_index(),
        ),
        ("GC_NEXT_TRIGGER_BYTES", GC_NEXT_TRIGGER_BYTES.slot_index()),
        ("GC_TRIGGER_ARMED", GC_TRIGGER_ARMED.slot_index()),
        (
            "GC_EXTERNAL_SIDE_LIVE_BYTES",
            GC_EXTERNAL_SIDE_LIVE_BYTES.slot_index(),
        ),
        (
            "GC_MAJOR_PACING_RETAINING",
            GC_MAJOR_PACING_RETAINING.slot_index(),
        ),
        ("GC_FLAGS", GC_FLAGS.slot_index()),
        (
            "GC_TRIGGER_WATERMARK",
            super::trigger_watermark::trigger_watermark_slot_index(),
        ),
        (
            "GC_BUDGETED_CYCLE_ACTIVE",
            GC_BUDGETED_CYCLE_ACTIVE.slot_index(),
        ),
        (
            "GC_BUDGETED_STEP_ACTIVE",
            GC_BUDGETED_STEP_ACTIVE.slot_index(),
        ),
        (
            "OLD_GEN_IN_USE_BYTES",
            crate::arena::old_gen_in_use_bytes_slot_index(),
        ),
        ("OLD_FREE_BYTES", super::old_free_bytes_slot_index()),
        ("MALLOC_STATE", super::malloc_state_slot_index()),
    ]
}

/// #10169: record that a document-sized JSON leaf was just born old in the
/// arena because the young generation is at least that large and has not been
/// measured as dying (`string::json_leaf_prefers_arena`). Read once by the
/// next trigger decision.
pub(crate) fn note_young_leaf_born_old() {
    GC_YOUNG_LEAF_BORN_OLD.with(|flag| flag.set(true));
}

pub(in crate::gc) fn gc_budgeted_due_trigger() -> Option<BudgetedGcTrigger> {
    gc_budgeted_due_trigger_eval().0
}

/// Reuse of a repeatable [`gc_budgeted_due_trigger_eval`] answer across the
/// questions one `gc_check_trigger` call asks. An unrepeatable answer is
/// returned once and the next question evaluates again.
pub(in crate::gc) struct DueTriggerMemo(Option<Option<BudgetedGcTrigger>>);

impl DueTriggerMemo {
    pub(in crate::gc) const fn new() -> Self {
        Self(None)
    }

    pub(in crate::gc) fn get(
        &mut self,
        eval: impl FnOnce() -> (Option<BudgetedGcTrigger>, bool),
    ) -> Option<BudgetedGcTrigger> {
        if let Some(due) = self.0 {
            return due;
        }
        let (due, repeatable) = eval();
        if repeatable {
            self.0 = Some(due);
        }
        due
    }
}

/// [`gc_budgeted_due_trigger`], plus whether evaluating it again with no state
/// changed in between is guaranteed to give the same answer.
///
/// Only the #10169 flag branch below breaks that: it consumes the flag and
/// returns early, so a second evaluation takes the ordinary path and may
/// answer differently (for example `OldReclaim`). The #8122 census seed does
/// not break it. The seed runs inside the young-cap arm before that arm's
/// comparison, so the first evaluation already compared against the seeded
/// cap; no earlier arm reads the census and the malloc arm after it does not
/// either, and a second evaluation cannot seed again.
pub(in crate::gc) fn gc_budgeted_due_trigger_eval() -> (Option<BudgetedGcTrigger>, bool) {
    let (due, repeatable, _) = gc_budgeted_due_trigger_probe();
    (due, repeatable)
}

/// [`gc_budgeted_due_trigger_eval`], plus — when nothing is due — the
/// watermark under which that answer keeps holding (#10698). Every other
/// answer carries [`TriggerWatermark::RETIRED`].
pub(super) fn gc_budgeted_due_trigger_probe() -> (Option<BudgetedGcTrigger>, bool, TriggerWatermark)
{
    const RETIRED: TriggerWatermark = TriggerWatermark::RETIRED;
    // #10169: a leaf born old under young pressure gives the nursery minor
    // ONE-TIME priority over old-reclaim, and only while the young generation
    // is still unmeasured. A young generation that a minor has already
    // measured as retained wholesale is either promoted (so the next leaf
    // finds it small) or, when it dies at every loop edge, best left to the
    // old-reclaim full that sweeps it in Eden together with the leaf. The one
    // case that must not fall through is an unmeasured young generation that
    // stays live: old-reclaim would re-mark it in place at every full, so it
    // is promoted by a minor first. The flag is consumed here whatever the
    // decision, so it can never starve old-reclaim.
    if GC_YOUNG_LEAF_BORN_OLD.with(TriggerInput::get) {
        GC_YOUNG_LEAF_BORN_OLD.with(|flag| flag.set(false));
        if !super::young_generation_measured_retained() && young_scavenge_cap_due() {
            return (Some(BudgetedGcTrigger::YoungScavengeCap), false, RETIRED);
        }
    }
    let old_pending = GC_OLD_RECLAIM_PENDING.with(TriggerInput::get);
    // #6010: external Map/Set side-buffer bytes escalate to OldReclaim too.
    let old_reclaimable = old_gen_reclaimable_pressure_bytes();
    let old_in_use = old_reclaimable.saturating_add(external_side_old_reclaim_pressure_bytes());
    let old_baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    let old_reclaim_why = if old_pending {
        Some("pending")
    } else if super::allocation_pacing::due(old_in_use, false) {
        Some("pacing")
    } else if old_reclaim_pressure_due(old_in_use, old_baseline) {
        Some("growth_band")
    } else {
        None
    };
    if let Some(why) = old_reclaim_why {
        if why != "pending" {
            super::diagnostic_hooks::note_reclaim_reason(why);
        }
        return (Some(BudgetedGcTrigger::OldReclaim), true, RETIRED);
    }

    // Two separately-scoped arena arms (see `young_scavenge_cap_due` for why
    // they must not share a basis): the adaptive base trigger against the
    // whole arena, and the scavenge nursery cap against the young generation
    // only.
    // Copying capacity never exceeds the gross arena bytes, so the leaf
    // exclusion (a block walk) is owed only once the gross figure is at the base.
    let base = next_arena_trigger_base();
    if crate::arena::arena_total_bytes() >= base && crate::arena::copying_capacity_bytes() >= base {
        return (Some(BudgetedGcTrigger::ArenaBytes), true, RETIRED);
    }
    // Old-gen is untouched since the read above: the arms in between only
    // read, and the census seed walks the young generation.
    let young = young_scavenge_cap_probe(|| old_reclaimable);
    if young.due {
        return (Some(BudgetedGcTrigger::YoungScavengeCap), true, RETIRED);
    }

    let malloc_count = malloc_object_count();
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
    if malloc_count >= next_malloc_trigger {
        return (Some(BudgetedGcTrigger::MallocCount), true, RETIRED);
    }

    (
        None,
        true,
        TriggerWatermark::after_nothing_due(next_malloc_trigger, young.reading),
    )
}
