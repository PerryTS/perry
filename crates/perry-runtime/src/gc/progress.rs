//! GC progress contract: pause budgets per progress kind (split from
//! policy.rs for the per-file size gate). Work-unit budgets and soft pause
//! targets for the budgeted stepper, plus the debt-pacing gain constant used
//! by `gc_mutator_assist_scaled_work_units` (policy.rs).

/// Hard work budget for ordinary automatic GC steps once the collector is
/// split into resumable phases.
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
pub const GC_ASSIST_DEBT_BYTES_PER_WORK_UNIT: u64 = 64;

/// Runtime-visible classification for GC progress.
///
/// Only `NormalIncremental` and `MutatorAssist` satisfy the low-pause
/// invariant today defined by this contract: bounded by work units, not heap
/// size. Explicit synchronous work and emergency full collections are allowed
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

/// Hard work-unit limit plus a soft pause target for telemetry.
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
