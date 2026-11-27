//! The budgeted collection cycle: starting it, stepping it under a work or
//! time budget, draining it, and the `js_gc_step_*` entries that report its
//! progress.

use super::*;

pub const JS_GC_STEP_STATUS_IDLE: u32 = 0;
pub const JS_GC_STEP_STATUS_ACTIVE: u32 = 1;
pub const JS_GC_STEP_STATUS_COMPLETED: u32 = 2;
pub const JS_GC_STEP_STATUS_SKIPPED: u32 = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct JsGcStepResult {
    pub status: u32,
    pub phase: u32,
    pub collection_kind: u32,
    pub trigger_kind: u32,
    pub active: u32,
    pub completed: u32,
    pub arena_debt_bytes: u64,
    pub malloc_debt_objects: u64,
    pub old_reclaim_debt_bytes: u64,
}
#[derive(Clone, Copy)]
enum BudgetedGcRebaseline {
    ArenaBytes { pre_in_use: usize },
    MallocCount { pre_count: usize, pre_in_use: usize },
    OldReclaim,
}
struct BudgetedGcCycle {
    state: GcCycleState,
    trigger_kind: GcTriggerKind,
    collection_kind: GcCollectionKind,
    rebaseline: BudgetedGcRebaseline,
    /// Next automatic slice. Caller-requested budgets do not advance it.
    automatic_work_units: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::gc) enum BudgetedGcTrigger {
    OldReclaim,
    ArenaBytes,
    /// The young-generation scavenge cap ([`young_scavenge_cap_due`]).
    ///
    /// Split out of `ArenaBytes` by #7909. Every *collection* treats the two
    /// identically — the split exists solely so the budgeted stepper can tell
    /// them apart at the moment it decides whether to START a cycle, because
    /// the quantity this one tests (`copying_from_space_in_use_bytes`) is one
    /// a budgeted low-pause NON-MOVING cycle cannot lower. See
    /// [`nursery_cap_active`] for why: a non-moving minor sweeps in place and
    /// leaves from-space occupied.
    YoungScavengeCap,
    MallocCount,
}
crate::perry_thread_local! {
    static GC_BUDGETED_CYCLE: RefCell<Option<BudgetedGcCycle>> = const { RefCell::new(None) };
    pub(super) static GC_BUDGETED_CYCLE_ACTIVE: TriggerInput<bool> = const { TriggerInput::new(false) };
    pub(super) static GC_BUDGETED_STEP_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn gc_budgeted_cycle_active() -> bool {
    GC_BUDGETED_CYCLE_ACTIVE.with(TriggerInput::get)
}

fn gc_budgeted_start_blocked() -> bool {
    GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || registered_root_scanners_block_budgeted_gc()
}

fn gc_budgeted_resume_blocked() -> bool {
    GC_FLAGS.with(|f| f.get()) & GC_FLAG_SUPPRESSED != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || registered_root_scanners_block_budgeted_gc()
}

struct BudgetedGcStepGuard;

impl BudgetedGcStepGuard {
    fn enter() -> Option<Self> {
        super::allocation_pacing::checkpoint();
        GC_BUDGETED_STEP_ACTIVE.with(|active| {
            if active.get() {
                None
            } else {
                active.set(true);
                Some(Self)
            }
        })
    }
}

impl Drop for BudgetedGcStepGuard {
    fn drop(&mut self) {
        GC_BUDGETED_STEP_ACTIVE.with(|active| active.set(false));
    }
}

fn gc_start_budgeted_full_cycle(
    trigger_kind: GcTriggerKind,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    // #9231: budgeted cycles construct GcCycleState directly instead of
    // entering through gc_collect_{minor,full}_with_trigger, so the lazy
    // stack-map index must be built here while allocation is still legal.
    // Otherwise the first root-scan step reaches #9182's fail-closed guard
    // with an owed index and aborts.
    crate::arena::sync_inline_arena_state();
    super::roots::ensure_stack_maps_built();
    super::diagnostic_hooks::before_collection();
    let mut state = GcCycleState::new_full(GcTriggerSnapshot::capture(trigger_kind));
    state.set_progress_kind(progress_kind);
    BudgetedGcCycle {
        collection_kind: state.collection_kind(),
        state,
        trigger_kind,
        rebaseline,
        automatic_work_units: GC_MUTATOR_ASSIST_WORK_UNITS,
    }
}

fn gc_start_budgeted_minor_fallback_cycle(
    trigger_kind: GcTriggerKind,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    gc_start_budgeted_minor_fallback_cycle_with_snapshot(
        GcTriggerSnapshot::capture(trigger_kind),
        rebaseline,
        progress_kind,
    )
}

fn gc_start_budgeted_minor_fallback_cycle_with_snapshot(
    trigger: GcTriggerSnapshot,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    crate::snap_funnel!(Gc);
    // Same direct-constructor path as gc_start_budgeted_full_cycle above.
    crate::arena::sync_inline_arena_state();
    super::roots::ensure_stack_maps_built();
    super::diagnostic_hooks::before_collection();
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let mut trace = crate::gc::diagnostic_hooks::start_trace(GcCollectionKind::Minor, trigger);
    if let Some(trace) = trace.as_mut() {
        trace.progress_kind = progress_kind;
    }
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    clear_mark_seeds();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let low_pause_non_moving = progress_kind.is_budgeted();
    // #7611: the `PERRY_GEN_GC_EVACUATE` conjunct that used to be here was
    // deleted as an untested configuration. The flag's controllers are
    // `low_pause_non_moving` (behavioural test
    // `budgeted_low_pause_minor_does_not_evacuate`) and the imprecise-point
    // veto (`an_imprecise_point_moves_no_object`), both in `evacuation_veto`.
    let move_veto = evacuation_veto(low_pause_non_moving);
    let evacuation_policy_allowed = move_veto.is_none();
    let force_evacuation = evacuation_policy_allowed && gc_force_evacuate_enabled();
    let evacuation_policy_disabled_reason = move_veto.unwrap_or(EVACUATION_POLICY_DISABLED_REASON);
    let old_page_selection = if evacuation_policy_allowed && old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    let old_page_source_blocks =
        crate::arena::old_arena_source_blocks_for_pages(&old_page_selection.pages);
    let state = GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        progress_kind,
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        evacuation_policy_disabled_reason,
        old_page_selection,
        old_page_source_blocks,
    );
    BudgetedGcCycle {
        collection_kind: state.collection_kind(),
        state,
        trigger_kind: trigger.kind,
        rebaseline,
        automatic_work_units: GC_MUTATOR_ASSIST_WORK_UNITS,
    }
}

#[cfg(test)]
pub(in crate::gc) fn test_start_budgeted_minor_fallback_state_with_trace(
    trigger_kind: GcTriggerKind,
    progress_kind: GcProgressKind,
) -> GcCycleState {
    let trigger = GcTriggerSnapshot {
        kind: trigger_kind,
        steps_before: Some(GcStepSnapshot::current()),
    };
    let cycle = gc_start_budgeted_minor_fallback_cycle_with_snapshot(
        trigger,
        BudgetedGcRebaseline::ArenaBytes {
            pre_in_use: crate::arena::arena_in_use_bytes(),
        },
        progress_kind,
    );
    cycle.state
}

fn gc_start_budgeted_cycle_for_pressure(progress_kind: GcProgressKind) -> Option<BudgetedGcCycle> {
    let trigger = gc_budgeted_due_trigger()?;
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    Some(match trigger {
        BudgetedGcTrigger::OldReclaim => {
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
            gc_start_budgeted_full_cycle(
                GcTriggerKind::OldGenBytes,
                BudgetedGcRebaseline::OldReclaim,
                progress_kind,
            )
        }
        // #7909: identical treatment — a cycle that DOES start for nursery
        // pressure (a non-budgeted one, which can evacuate) is the same
        // arena-bytes collection it always was.
        BudgetedGcTrigger::ArenaBytes | BudgetedGcTrigger::YoungScavengeCap => {
            let rebaseline = BudgetedGcRebaseline::ArenaBytes {
                pre_in_use: crate::arena::arena_in_use_bytes(),
            };
            // Major-GC pacing: escalate to a full when arena live-bytes grew
            // past K× the last full's live set — the non-moving minor can't free
            // array-growth forwarding stubs (see `arena_growth_full_escalation_due`).
            if gen_gc_enabled() && !arena_growth_full_escalation_due() {
                gc_start_budgeted_minor_fallback_cycle(
                    GcTriggerKind::ArenaBytes,
                    rebaseline,
                    progress_kind,
                )
            } else {
                gc_start_budgeted_full_cycle(GcTriggerKind::ArenaBytes, rebaseline, progress_kind)
            }
        }
        BudgetedGcTrigger::MallocCount => {
            let rebaseline = BudgetedGcRebaseline::MallocCount {
                pre_count: malloc_object_count(),
                pre_in_use: crate::arena::arena_in_use_bytes(),
            };
            // Major-GC pacing (malloc-count trigger twin of the ArenaBytes branch).
            if gen_gc_enabled() && !arena_growth_full_escalation_due() {
                gc_start_budgeted_minor_fallback_cycle(
                    GcTriggerKind::MallocCount,
                    rebaseline,
                    progress_kind,
                )
            } else {
                gc_start_budgeted_full_cycle(GcTriggerKind::MallocCount, rebaseline, progress_kind)
            }
        }
    })
}

/// What one budgeted step decided, without the debt figures.
///
/// The step machinery returns this; only callers that hand a
/// [`JsGcStepResult`] to someone attach the debt, through
/// [`GcStepReport::with_debt`]. The runtime's own polls (regex quanta, the
/// microtask pump, the event loop) discard the result, and a
/// `GcDebtSnapshot` costs a second evaluation of the nursery cap, the arena
/// trigger and the old-reclaim band on every poll that finds nothing due.
///
/// Attaching the debt after the step returns reads the same values it used to
/// read inside the step: the snapshot was always the last thing a step
/// computed, and the only work between that point and the caller is dropping
/// `BudgetedGcStepGuard`, which clears a flag the snapshot does not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GcStepReport {
    pub(crate) status: u32,
    phase: u32,
    collection_kind: u32,
    trigger_kind: u32,
    active: bool,
    completed: bool,
}

impl GcStepReport {
    /// The FFI result, with the debt measured now.
    pub(crate) fn with_debt(self) -> JsGcStepResult {
        let debt = GcDebtSnapshot::current();
        JsGcStepResult {
            status: self.status,
            phase: self.phase,
            collection_kind: self.collection_kind,
            trigger_kind: self.trigger_kind,
            active: u32::from(self.active),
            completed: u32::from(self.completed),
            arena_debt_bytes: debt.arena_debt_bytes,
            malloc_debt_objects: debt.malloc_debt_objects,
            old_reclaim_debt_bytes: debt.old_reclaim_debt_bytes,
        }
    }
}

fn gc_step_result(
    status: u32,
    phase: u32,
    collection_kind: u32,
    trigger_kind: u32,
    active: bool,
    completed: bool,
) -> GcStepReport {
    GcStepReport {
        status,
        phase,
        collection_kind,
        trigger_kind,
        active,
        completed,
    }
}

fn gc_idle_step_result() -> GcStepReport {
    gc_step_result(JS_GC_STEP_STATUS_IDLE, 0, 0, 0, false, false)
}

fn gc_cycle_step_result(status: u32, cycle: &BudgetedGcCycle, completed: bool) -> GcStepReport {
    gc_step_result(
        status,
        cycle.state.phase().ffi_code(),
        cycle.collection_kind.ffi_code(),
        cycle.trigger_kind.ffi_code(),
        !completed,
        completed,
    )
}

fn gc_budgeted_status_result() -> GcStepReport {
    if !gc_budgeted_cycle_active() {
        return gc_idle_step_result();
    }

    let result = GC_BUDGETED_CYCLE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|cycle| gc_cycle_step_result(JS_GC_STEP_STATUS_ACTIVE, cycle, false))
    });
    match result {
        Some(result) => result,
        None => {
            GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
            gc_idle_step_result()
        }
    }
}

fn gc_budgeted_skipped_result() -> GcStepReport {
    if !gc_budgeted_cycle_active() {
        return gc_step_result(JS_GC_STEP_STATUS_SKIPPED, 0, 0, 0, false, false);
    }

    GC_BUDGETED_CYCLE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|cycle| gc_cycle_step_result(JS_GC_STEP_STATUS_SKIPPED, cycle, false))
            .unwrap_or_else(|| gc_step_result(JS_GC_STEP_STATUS_SKIPPED, 0, 0, 0, false, false))
    })
}

fn gc_finish_budgeted_cycle(mut cycle: BudgetedGcCycle) -> GcStepReport {
    let outcome = cycle
        .state
        .take_outcome()
        .expect("completed budgeted GC cycle must produce an outcome");
    let freed_for_diag = outcome.freed_bytes;
    match cycle.rebaseline {
        BudgetedGcRebaseline::ArenaBytes { pre_in_use } => {
            gc_finish_arena_trigger_collection(pre_in_use, outcome);
        }
        BudgetedGcRebaseline::MallocCount {
            pre_count,
            pre_in_use,
        } => {
            gc_finish_malloc_trigger_collection(pre_count, pre_in_use, outcome);
        }
        BudgetedGcRebaseline::OldReclaim => {
            let freed = outcome.emit_after_current();
            if matches!(cycle.trigger_kind, GcTriggerKind::IdleReclaim) {
                super::idle_reclaim::note_cycle_completed(freed);
            }
        }
    }
    GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
    super::diagnostic_hooks::site_budgeted_completed(freed_for_diag);
    gc_step_result(
        JS_GC_STEP_STATUS_COMPLETED,
        GcCyclePhase::Complete.ffi_code(),
        cycle.collection_kind.ffi_code(),
        cycle.trigger_kind.ffi_code(),
        false,
        true,
    )
}

enum BudgetedStepOutcome {
    Result(GcStepReport),
    Completed(BudgetedGcCycle),
}

/// Finish any parked budgeted cycle through its own machinery before a
/// synchronous (direct/manual/emergency) collection constructs a fresh
/// `GcCycleState`. Two cycles must never be alive at once: they share
/// `GC_FLAG_MARKED`, the mark-seed queue, the incremental-barrier TLS, and
/// the allocate-black birth-flag lifecycle — a synchronous full mark-sweep
/// landing mid-budgeted-cycle erases the parked cycle's marks, so its
/// eventual sweep frees whatever the interloper didn't re-mark (measured as
/// the manual-`gc()` SIGSEGV escalation of the #6224 stress). One unbounded
/// step drives the parked cycle to completion via the normal finisher.
pub(in crate::gc) fn gc_drain_active_budgeted_cycle() {
    if !gc_budgeted_cycle_active() {
        return;
    }
    // One `step()` call advances only the CURRENT phase (even with an
    // unbounded budget) — completing the whole cycle takes one call per
    // remaining phase, exactly like `run_to_completion`'s loop. Bound the
    // loop defensively: 8 phases, and a blocked stepper (suppression /
    // unsafe zone / reentrancy guard) returns without progress — bail then
    // rather than spin.
    for _ in 0..64 {
        let result = gc_budgeted_step_work_units_inner(usize::MAX);
        if !gc_budgeted_cycle_active() {
            return;
        }
        if result.status == JS_GC_STEP_STATUS_SKIPPED {
            break;
        }
    }
    if crate::gc::diagnostic_hooks::enabled() {
        eprintln!("[gc-drain] WARNING: parked budgeted cycle could not be drained before synchronous collection");
    }
}

fn gc_budgeted_step_work_units_inner(work_units: usize) -> GcStepReport {
    gc_budgeted_step_work_units_inner_with_progress(
        work_units,
        GcProgressKind::NormalIncremental,
        false,
    )
}

/// Start a FULL budgeted cycle on behalf of the idle-time reclaim
/// (`gc/idle_reclaim.rs`), bypassing the pressure predicates: the reducer's
/// whole point is to collect when nothing is due. Same start guards as every
/// budgeted cycle; the block-pool drain is armed so the sweep's released
/// blocks leave the process, and the sticky old-reclaim debt is consumed
/// because a full satisfies it. Returns whether a cycle was opened.
pub(in crate::gc) fn gc_idle_reclaim_try_start() -> bool {
    if gc_budgeted_cycle_active() || gc_budgeted_start_blocked() {
        return false;
    }
    let Some(_guard) = BudgetedGcStepGuard::enter() else {
        return false;
    };
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    crate::arena::request_block_pool_drain();
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    let cycle = gc_start_budgeted_full_cycle(
        GcTriggerKind::IdleReclaim,
        BudgetedGcRebaseline::OldReclaim,
        GcProgressKind::NormalIncremental,
    );
    GC_BUDGETED_CYCLE.with(|slot| {
        *slot.borrow_mut() = Some(cycle);
    });
    GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(true));
    super::instruments::note_incremental_cycle_start();
    true
}

/// One time slice of the active budgeted cycle for the idle-time reclaim:
/// normal-increment work units, repeated until `budget_us` has elapsed or the
/// cycle stops reporting `ACTIVE`. Never starts a cycle of its own — when none
/// is active the stepper's pressure check runs exactly as it would at any host
/// safepoint.
pub(in crate::gc) fn gc_idle_reclaim_step(budget_us: u64) -> GcStepReport {
    crate::snap_funnel!(Gc);
    let start = Instant::now();
    let mut result = gc_budgeted_step_work_units_inner_with_progress(
        GC_NORMAL_INCREMENTAL_WORK_UNITS,
        GcProgressKind::NormalIncremental,
        true,
    );
    while result.status == JS_GC_STEP_STATUS_ACTIVE
        && start.elapsed().as_micros() < u128::from(budget_us)
    {
        result = gc_budgeted_step_work_units_inner_with_progress(
            GC_NORMAL_INCREMENTAL_WORK_UNITS,
            GcProgressKind::NormalIncremental,
            true,
        );
    }
    result
}

/// #7909: arm the precise-root safepoint for nursery pressure the budgeted
/// stepper just declined, mirroring `gc_check_trigger`'s deferral arm exactly
/// (including the arena baseline the slack valve measures from — leaving that
/// stale would make `moving_defer_within_slack` read an already-exceeded
/// baseline and disable deferral for the rest of the process, the #7024 shape).
pub(super) fn defer_nursery_cap_to_precise_safepoint() {
    if GC_SAFEPOINT_PENDING.with(Cell::get) {
        return;
    }
    GC_SAFEPOINT_DEFER_ARENA_BASE.with(|base| base.set(crate::arena::arena_total_bytes()));
    set_safepoint_pending(true);
}

fn gc_budgeted_step_work_units_inner_with_progress(
    work_units: usize,
    start_progress_kind: GcProgressKind,
    automatic: bool,
) -> GcStepReport {
    match gc_budgeted_poll(work_units, start_progress_kind, automatic) {
        BudgetedPoll::Stepped(report) => report,
        BudgetedPoll::NothingDue => gc_idle_step_result(),
    }
}

/// What one budgeted step did.
enum BudgetedPoll {
    /// The step ran, was skipped by a guard, or acted on a trigger.
    Stepped(GcStepReport),
    /// No cycle was active and the trigger ladder answered "nothing due" —
    /// from the published watermark or from a repeatable evaluation — and
    /// the step wrote no collector state. Asking the ladder again before the
    /// mutator runs gives the same answer.
    NothingDue,
}

#[inline(always)]
fn gc_budgeted_poll(
    work_units: usize,
    start_progress_kind: GcProgressKind,
    automatic: bool,
) -> BudgetedPoll {
    if work_units == 0 {
        return BudgetedPoll::Stepped(gc_budgeted_status_result());
    }

    // "Nothing due" is the allocation point's question too, and is answered
    // the same way (#10698): from the watermark the last evaluation published,
    // while none of its inputs has changed (an active cycle retires it). Asked
    // before the guard, whose entry checkpoint samples the arena: a runtime
    // poll (each regex capture copy, the microtask pump) paid that sample and
    // the whole ladder, ~800 instructions, to learn nothing was due. A
    // reentrant step still reports the skip below.
    if !GC_BUDGETED_STEP_ACTIVE.with(Cell::get) && super::trigger_watermark::nothing_due() {
        #[cfg(test)]
        super::trigger_watermark::verify_nothing_due();
        super::instruments::note_budgeted_step_skip(
            super::instruments::BudgetedStepSkip::NoTrigger,
        );
        return BudgetedPoll::NothingDue;
    }

    let Some(_guard) = BudgetedGcStepGuard::enter() else {
        super::instruments::note_budgeted_step_skip(
            super::instruments::BudgetedStepSkip::Reentrant,
        );
        return BudgetedPoll::Stepped(gc_budgeted_skipped_result());
    };

    // The common host poll: no cycle, nothing due. Everything past this point
    // (starting and stepping a cycle) lives out of line, so that poll does not
    // pay the multi-kilobyte frame the cycle machinery needs. `_guard` stays
    // held across the call, exactly as when the code was inline.
    let due = if gc_budgeted_cycle_active() {
        None
    } else {
        let (due, repeatable, watermark) = gc_budgeted_due_trigger_probe();
        // A "nothing due" answers the next poll (above) and the next
        // allocation point, published under the conditions
        // `gc_check_trigger` publishes under: no cycle active, no root lock.
        if due.is_none() && !gc_root_lock_held() {
            super::trigger_watermark::publish_trigger_watermark(watermark);
        }
        let Some(due) = due else {
            super::instruments::note_budgeted_step_skip(
                super::instruments::BudgetedStepSkip::NoTrigger,
            );
            return if repeatable {
                BudgetedPoll::NothingDue
            } else {
                BudgetedPoll::Stepped(gc_idle_step_result())
            };
        };
        Some(due)
    };
    BudgetedPoll::Stepped(gc_budgeted_start_or_step(
        due,
        work_units,
        start_progress_kind,
        automatic,
    ))
}

/// The part of [`gc_budgeted_step_work_units_inner_with_progress`] that starts
/// a cycle for `due` (when `Some`, i.e. no cycle was active and a trigger was
/// due) and steps the active cycle. The caller holds `BudgetedGcStepGuard`.
#[inline(never)]
fn gc_budgeted_start_or_step(
    due: Option<BudgetedGcTrigger>,
    work_units: usize,
    start_progress_kind: GcProgressKind,
    automatic: bool,
) -> GcStepReport {
    crate::snap_funnel!(Gc);
    if let Some(due) = due {
        if start_progress_kind.is_budgeted()
            && (due == BudgetedGcTrigger::YoungScavengeCap
                || (automatic
                    && gc_moving_loop_polls_enabled()
                    && matches!(
                        due,
                        BudgetedGcTrigger::ArenaBytes | BudgetedGcTrigger::MallocCount
                    )))
        {
            // Nursery allocation and reserved capacity are minor pressure, even
            // when old storage makes the whole-arena trigger cross first. A
            // non-moving budgeted cycle blocks the precise minor that can pay
            // this debt. Use the allocation path's existing deferral for all
            // nursery triggers. OldReclaim is selected earlier from old growth.
            super::instruments::note_budgeted_step_skip(
                super::instruments::BudgetedStepSkip::NurseryCapUndischargeable,
            );
            defer_nursery_cap_to_precise_safepoint();
            return gc_idle_step_result();
        }
        if gc_budgeted_start_blocked() {
            super::instruments::note_budgeted_step_skip(
                super::instruments::BudgetedStepSkip::StartBlocked,
            );
            return gc_budgeted_skipped_result();
        }
        super::diagnostic_hooks::site_trigger_decision("budgeted_start", "due");
        let cycle = gc_start_budgeted_cycle_for_pressure(start_progress_kind)
            .expect("budgeted GC pressure was observed before starting cycle");
        super::diagnostic_hooks::site_budgeted_started(
            cycle.trigger_kind,
            cycle.collection_kind,
            start_progress_kind,
        );
        GC_BUDGETED_CYCLE.with(|slot| {
            *slot.borrow_mut() = Some(cycle);
        });
        GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(true));
        super::instruments::note_incremental_cycle_start();
    }

    if gc_budgeted_resume_blocked() {
        super::instruments::note_budgeted_step_skip(
            super::instruments::BudgetedStepSkip::ResumeBlocked,
        );
        return gc_budgeted_skipped_result();
    }

    let outcome = GC_BUDGETED_CYCLE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(cycle) = slot.as_mut() else {
            GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
            return BudgetedStepOutcome::Result(gc_idle_step_result());
        };

        // #7903: the step's own wall duration, not the budget that was asked
        // for. `js_gc_step_us` can only consult its clock BETWEEN units, so the
        // only honest statement about pause is a measured maximum.
        let step_started = std::time::Instant::now();
        let phase_code = cycle.state.phase().ffi_code();
        let work_units = if automatic {
            work_units.max(cycle.automatic_work_units)
        } else {
            work_units
        };
        let mut step = cycle.state.step(GcWorkBudget::bounded(work_units));
        // Conditional edges can require several rounds even with an unlimited
        // work budget. An unlimited request finishes this phase through the
        // same stepper, including automatic slices whose budget has saturated
        // and the existing synchronous drain. Finite requests stay sliced.
        while work_units == usize::MAX
            && !step.completed
            && cycle.state.phase().ffi_code() == phase_code
        {
            step = cycle.state.step(GcWorkBudget::unbounded());
        }
        if automatic {
            // A fixed slice lets a large heap keep the mark barrier active for
            // thousands of polls. Grow the existing slice until it pays the
            // work: after at most usize::BITS successful automatic polls it is
            // unbounded, then each remaining phase finishes in one poll. The
            // allocation assist's debt-scaled request can advance it sooner.
            // Explicit step APIs retain exactly the budget their caller asked.
            cycle.automatic_work_units = work_units.saturating_mul(2);
        }
        let step_us = step_started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        super::instruments::note_budgeted_step_duration(step_us);
        super::diagnostic_hooks::site_budgeted_step_done(phase_code, step_us, work_units);
        super::instruments::note_incremental_step();
        if step.completed {
            super::instruments::note_incremental_completion();
            BudgetedStepOutcome::Completed(slot.take().expect("active budgeted GC cycle exists"))
        } else {
            BudgetedStepOutcome::Result(gc_cycle_step_result(
                JS_GC_STEP_STATUS_ACTIVE,
                cycle,
                false,
            ))
        }
    });

    match outcome {
        BudgetedStepOutcome::Result(result) => result,
        BudgetedStepOutcome::Completed(cycle) => gc_finish_budgeted_cycle(cycle),
    }
}

/// Allocation-side mutator assist: a debt-scaled, automatically growing
/// slice of GC work performed from the allocator (`gc_check_trigger`) rather
/// than from a host safepoint. Assists drive **every** resumable phase of the
/// active budgeted cycle, exactly like a host safepoint — the only difference
/// is the smaller per-step budget carried in `work_units`. This is what closes
/// the incremental sweep-parking hole (#6180): a pure compute loop that never
/// reaches the event pump still finishes the cycle (and disables the mark
/// barrier / reclaims memory) purely from the allocations it keeps making, so
/// RSS stays bounded. `AtomicFinalizeSubphase::WeakProcessing` snapshots the
/// live-holder registry and consumes at most the supplied number of holders per
/// assist. Under persistent work the budget grows toward completion rather
/// than leaving the incremental barrier active indefinitely.
pub(super) fn gc_mutator_assist_step_work_units_inner_with_progress(
    work_units: usize,
    start_progress_kind: GcProgressKind,
) -> GcStepReport {
    gc_budgeted_step_work_units_inner_with_progress(work_units, start_progress_kind, true)
}

/// A host safepoint that reports what it did, debt included. For callers
/// that read the result: `js_gc_safepoint` and tests.
pub(crate) fn gc_runtime_safepoint() -> JsGcStepResult {
    gc_runtime_safepoint_report().with_debt()
}

/// The runtime's own safepoint poll (regex quanta, the microtask pump, the
/// event loop). Makes exactly the decisions [`gc_runtime_safepoint`] makes and
/// skips only the debt snapshot, which none of these callers reads.
#[inline]
pub(crate) fn gc_runtime_safepoint_poll() {
    let _ = gc_runtime_safepoint_report();
}

/// [`gc_runtime_safepoint_poll`] for the outermost microtask-drain boundary:
/// the same step, returning whether it was [`BudgetedPoll::NothingDue`] — no
/// cycle active, the trigger ladder answered "nothing due" repeatably, and
/// no collector state written — so the precise safepoint that follows can
/// act on that answer instead of deriving it again.
pub(crate) fn gc_boundary_safepoint_poll() -> bool {
    let budget = gc_progress_contract().budget_for(GcProgressKind::NormalIncremental);
    let Some(work_units) = budget.work_units else {
        let _ = gc_budgeted_status_result();
        return false;
    };
    matches!(
        gc_budgeted_poll(work_units, GcProgressKind::NormalIncremental, true),
        BudgetedPoll::NothingDue
    )
}

fn gc_runtime_safepoint_report() -> GcStepReport {
    let budget = gc_progress_contract().budget_for(GcProgressKind::NormalIncremental);
    let Some(work_units) = budget.work_units else {
        return gc_budgeted_status_result();
    };
    gc_budgeted_step_work_units_inner_with_progress(
        work_units,
        GcProgressKind::NormalIncremental,
        true,
    )
}

fn write_gc_step_result(out: *mut JsGcStepResult, result: JsGcStepResult) -> u32 {
    if !out.is_null() {
        unsafe {
            *out = result;
        }
    }
    result.status
}

#[no_mangle]
pub extern "C" fn js_gc_step_work_units(work_units: u64, out: *mut JsGcStepResult) -> u32 {
    let work_units = usize::try_from(work_units).unwrap_or(usize::MAX);
    let result = gc_budgeted_step_work_units_inner(work_units).with_debt();
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_step_us(budget_us: u64, out: *mut JsGcStepResult) -> u32 {
    crate::snap_funnel!(Gc);
    if budget_us == 0 {
        let result = gc_budgeted_status_result().with_debt();
        return write_gc_step_result(out, result);
    }

    let start = Instant::now();
    let mut result = gc_budgeted_step_work_units_inner(1);
    while result.status == JS_GC_STEP_STATUS_ACTIVE
        && start.elapsed().as_micros() < u128::from(budget_us)
    {
        result = gc_budgeted_step_work_units_inner(1);
    }
    write_gc_step_result(out, result.with_debt())
}

#[no_mangle]
pub extern "C" fn js_gc_step_status(out: *mut JsGcStepResult) -> u32 {
    let result = gc_budgeted_status_result().with_debt();
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_safepoint(out: *mut JsGcStepResult) -> u32 {
    let result = gc_runtime_safepoint();
    write_gc_step_result(out, result)
}
