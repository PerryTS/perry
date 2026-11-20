//! GC progress contract: pause budgets per progress kind (split from
//! policy.rs for the per-file size gate). Work-unit budgets and soft pause
//! targets for the budgeted stepper, plus the debt-pacing gain constant used
//! by `gc_mutator_assist_scaled_work_units` (policy.rs).

use super::*;

/// Initial work budget for ordinary automatic GC steps. The cycle grows its
/// slice on each successful automatic poll so the barrier window is bounded.
pub const GC_NORMAL_INCREMENTAL_WORK_UNITS: usize = 2_048;
/// Soft telemetry target for ordinary automatic GC steps.
pub const GC_NORMAL_INCREMENTAL_SOFT_PAUSE_US: u64 = 2_000;
/// BASE work budget for allocation-side mutator assist steps. The actual
/// per-assist budget is debt-scaled (`gc_mutator_assist_scaled_work_units`):
/// this constant alone is only enough when the collector is keeping up.
pub const GC_MUTATOR_ASSIST_WORK_UNITS: usize = 256;
/// Soft telemetry target for allocation-side mutator assist steps.
pub const GC_MUTATOR_ASSIST_SOFT_PAUSE_US: u64 = 500;
/// Debt-proportional assist pacing: one extra work unit per this many bytes
/// of arena debt (allocation past the armed trigger). This is the gain of a
/// proportional controller whose equilibrium debt scales as
/// sqrt(cycle_work × gain⁻¹): measured on a 10M-allocation churn loop, a
/// 1024-bytes-per-unit gain left cycles spanning ~300 MB of allocation
/// (pct_freed 156-190% in the re-arm DIAG) and RSS at 3.5× the synchronous
/// collector's. At 64 bytes per unit the same loop completes cycles within
/// ~its trigger step and RSS lands near parity. When the collector is
/// keeping up (debt ≈ 0) the budget stays at the base, so low-latency
/// workloads never see the scaled assists.
pub const GC_ASSIST_DEBT_BYTES_PER_WORK_UNIT: u64 = 32;

/// Runtime-visible classification for GC progress.
///
/// Only `NormalIncremental` and `MutatorAssist` satisfy the low-pause
/// invariant defined by this contract: work-unit slices. Automatic slices
/// grow toward completion; caller-requested slices keep their hard limit.
/// Explicit synchronous work and emergency full collections are allowed
/// to be unbounded only because they are separately requested or separately
/// reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcProgressKind {
    NormalIncremental,
    MutatorAssist,
    ExplicitSynchronous,
    ExplicitFull,
    EmergencyFull,
    LegacySynchronous,
}

impl GcProgressKind {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NormalIncremental => "normal_incremental",
            Self::MutatorAssist => "mutator_assist",
            Self::ExplicitSynchronous => "explicit_synchronous",
            Self::ExplicitFull => "explicit_full",
            Self::EmergencyFull => "emergency_full",
            Self::LegacySynchronous => "legacy_synchronous",
        }
    }

    #[inline]
    pub const fn is_budgeted(self) -> bool {
        matches!(self, Self::NormalIncremental | Self::MutatorAssist)
    }

    #[inline]
    pub const fn report_class(self) -> &'static str {
        match self {
            Self::NormalIncremental | Self::MutatorAssist => "ordinary_budgeted",
            Self::ExplicitSynchronous | Self::ExplicitFull => "explicit",
            Self::EmergencyFull => "emergency",
            Self::LegacySynchronous => "legacy",
        }
    }
}

/// Initial automatic work budget plus a soft pause target for telemetry.
/// Explicit step APIs apply their caller-supplied work limit exactly.
///
/// `None` means the path is intentionally unbounded and must be labeled by its
/// `GcProgressKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcPauseBudget {
    pub work_units: Option<usize>,
    pub pause_us: Option<u64>,
}

impl GcPauseBudget {
    #[inline]
    pub const fn bounded(work_units: usize, pause_us: u64) -> Self {
        Self {
            work_units: Some(work_units),
            pause_us: Some(pause_us),
        }
    }

    #[inline]
    pub const fn unbounded() -> Self {
        Self {
            work_units: None,
            pause_us: None,
        }
    }

    #[inline]
    pub const fn is_bounded(self) -> bool {
        self.work_units.is_some()
    }
}

/// GC progress policy exposed to runtime and trace consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcProgressContract {
    pub normal_step_budget: GcPauseBudget,
    pub assist_budget: GcPauseBudget,
    pub explicit_synchronous_policy: GcPauseBudget,
    pub explicit_full_policy: GcPauseBudget,
    pub emergency_policy: GcPauseBudget,
}

impl GcProgressContract {
    #[inline]
    pub const fn budget_for(self, kind: GcProgressKind) -> GcPauseBudget {
        match kind {
            GcProgressKind::NormalIncremental => self.normal_step_budget,
            GcProgressKind::MutatorAssist => self.assist_budget,
            GcProgressKind::ExplicitSynchronous => self.explicit_synchronous_policy,
            GcProgressKind::ExplicitFull => self.explicit_full_policy,
            GcProgressKind::EmergencyFull => self.emergency_policy,
            GcProgressKind::LegacySynchronous => GcPauseBudget::unbounded(),
        }
    }
}

impl Default for GcProgressContract {
    fn default() -> Self {
        Self {
            normal_step_budget: GcPauseBudget::bounded(
                GC_NORMAL_INCREMENTAL_WORK_UNITS,
                GC_NORMAL_INCREMENTAL_SOFT_PAUSE_US,
            ),
            assist_budget: GcPauseBudget::bounded(
                GC_MUTATOR_ASSIST_WORK_UNITS,
                GC_MUTATOR_ASSIST_SOFT_PAUSE_US,
            ),
            explicit_synchronous_policy: GcPauseBudget::unbounded(),
            explicit_full_policy: GcPauseBudget::unbounded(),
            emergency_policy: GcPauseBudget::unbounded(),
        }
    }
}

/// Return Perry's process-wide GC progress contract.
pub fn gc_progress_contract() -> GcProgressContract {
    GcProgressContract::default()
}

/// Number of most-recent pause samples retained per thread (#6187).
pub const GC_RECENT_PAUSE_WINDOW: usize = 32;

pub struct GcStats {
    // Cumulative collector work, observed by diagnostics without hot-path hooks.
    pub(super) forwarded_stub_membership_recoveries: u64,
    pub(super) block_persist_force_marks: u64,
    pub(super) adopted_blocks: u64,
    pub(super) hole_rebuild_blocks_skipped: u64,
    pub(super) block_skip_reclaimed_blocks: u64,
    pub(super) block_skip_reclaimed_objects: u64,
    pub(super) block_skip_reclaimed_bytes: u64,

    pub collection_count: u64,
    pub total_freed_bytes: u64,
    pub last_pause_us: u64,
    /// 2026-07-09 audit (#6187): always-on pause observability. A frame
    /// scheduler or an ops dashboard needs more than the last sample; the
    /// max-since-start plus a small ring of recent pauses costs a few
    /// stores per collection and a few hundred bytes of TLS. The rich
    /// per-phase traces stay behind PERRY_GC_TRACE.
    pub max_pause_us: u64,
    pub recent_pauses_us: [u64; GC_RECENT_PAUSE_WINDOW],
    pub recent_cursor: u8,
    pub recent_len: u8,
}

impl GcStats {
    /// Single funnel for per-collection accounting: last/max pause and the
    /// recent-pause ring advance together with the counters, so no future
    /// collection path can update one without the others.
    ///
    pub(super) fn record_collection(&mut self, freed_bytes: u64, elapsed_us: u64) {
        super::json_defer::collection_completed();
        self.collection_count += 1;
        self.total_freed_bytes = self.total_freed_bytes.saturating_add(freed_bytes);
        self.last_pause_us = elapsed_us;
        if elapsed_us > self.max_pause_us {
            self.max_pause_us = elapsed_us;
        }
        self.recent_pauses_us[self.recent_cursor as usize] = elapsed_us;
        self.recent_cursor = ((self.recent_cursor as usize + 1) % GC_RECENT_PAUSE_WINDOW) as u8;
        if (self.recent_len as usize) < GC_RECENT_PAUSE_WINDOW {
            self.recent_len += 1;
        }
    }
}

thread_local! {
    pub(super) static GC_STATS: RefCell<GcStats> = const { RefCell::new(GcStats {
        forwarded_stub_membership_recoveries: 0,
        block_persist_force_marks: 0,
        adopted_blocks: 0,
        hole_rebuild_blocks_skipped: 0,
        block_skip_reclaimed_blocks: 0,
        block_skip_reclaimed_objects: 0,
        block_skip_reclaimed_bytes: 0,

        collection_count: 0,
        total_freed_bytes: 0,
        last_pause_us: 0,
        max_pause_us: 0,
        recent_pauses_us: [0; GC_RECENT_PAUSE_WINDOW],
        recent_cursor: 0,
        recent_len: 0,
    }) };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct GcDebtSnapshot {
    pub(super) arena_debt_bytes: u64,
    pub(super) malloc_debt_objects: u64,
    pub(super) old_reclaim_debt_bytes: u64,
}

impl GcDebtSnapshot {
    #[inline]
    pub(super) fn current() -> Self {
        let total = crate::arena::arena_total_bytes();
        // #6950: read the SAME trigger the arming path compares against.
        // `gc_budgeted_due_trigger` uses `effective_next_arena_trigger()`, which
        // substitutes the device/`PERRY_GC_HEAP_LIMIT`-derived ceiling while the
        // raw cell still holds its 128 MB desktop-default const initializer
        // (`GC_TRIGGER_ARMED == false`). Reading the raw cell here made the two
        // disagree: a cycle armed at a 2 MB effective trigger measured its own
        // debt against 128 MB and therefore reported ZERO debt, so
        // `gc_mutator_assist_scaled_work_units` never scaled past its 256-unit
        // floor and the budgeted cycle crawled without ever completing —
        // 300k escaping allocations / 330 MB RSS with ZERO collections. That is
        // exactly the unbounded-growth failure the debt-proportional pacing was
        // introduced to prevent.
        let next_arena_trigger = effective_next_arena_trigger();
        let malloc_count = malloc_object_count();
        let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
        let old_in_use = crate::arena::old_gen_in_use_bytes();
        let old_baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());

        Self {
            arena_debt_bytes: total.saturating_sub(next_arena_trigger) as u64,
            malloc_debt_objects: malloc_count.saturating_sub(next_malloc_trigger) as u64,
            old_reclaim_debt_bytes: gc_old_reclaim_debt_bytes(old_in_use, old_baseline),
        }
    }

    #[inline]
    pub(super) fn max_components(self, other: Self) -> Self {
        Self {
            arena_debt_bytes: self.arena_debt_bytes.max(other.arena_debt_bytes),
            malloc_debt_objects: self.malloc_debt_objects.max(other.malloc_debt_objects),
            old_reclaim_debt_bytes: self
                .old_reclaim_debt_bytes
                .max(other.old_reclaim_debt_bytes),
        }
    }
}

pub(super) fn gc_last_pause_us() -> u64 {
    GC_STATS.with(|stats| stats.borrow().last_pause_us)
}

/// Total collections so far on this thread. `js_gc_module_idle_hint` compares
/// this before/after a trigger check to report whether a collection ran.
pub(super) fn gc_total_collection_count() -> u64 {
    GC_STATS.with(|stats| stats.borrow().collection_count)
}

pub(crate) struct GcCollectOutcome {
    pub(super) freed_bytes: u64,
    pub(super) malloc_swept: bool,
    pub(super) trace: Option<GcCycleTrace>,
}

pub(super) struct CopiedMinorFastPathOutcome {
    pub(super) freed_bytes: u64,
    pub(super) malloc_swept: bool,
}

impl GcCollectOutcome {
    #[inline]
    pub(super) fn emit_after_current(self) -> u64 {
        let Self {
            freed_bytes, trace, ..
        } = self;
        if let Some(trace) = trace {
            super::diagnostic_hooks::emit_trace(trace);
        }
        freed_bytes
    }
}
