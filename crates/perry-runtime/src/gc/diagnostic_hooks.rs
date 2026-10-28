//! Optional diagnostics entry points. Slots are filled only by the runtime
//! diagnostics installer; collector walks select their specialization per cycle.
use super::*;
use crate::feature_hooks::Hook;

pub(super) static MINOR_COLLECTION: Hook<
    fn(GcTriggerSnapshot, FullEscalation, CopyingFastPath) -> GcCollectOutcome,
> = Hook::empty();

type CycleStep = fn(&mut GcCycleState, GcWorkBudget) -> GcCycleStepResult;
static CYCLE_STEP: Hook<CycleStep> = Hook::empty();
pub(super) fn cycle_step() -> CycleStep {
    CYCLE_STEP
        .get()
        .unwrap_or(GcCycleState::step_with_diagnostics::<false>)
}

pub(super) struct CollectionCharge(Option<super::diag_sites::ChargeProbe>);
impl CollectionCharge {
    pub(super) fn begin() -> Self {
        Self(begin_charge())
    }
    pub(super) fn end(self, units: usize, kind: super::diag_sites::ChargeKind) {
        if let Some(probe) = self.0 {
            end_charge(probe, units, kind);
        }
    }
}

fn begin_charge_impl() -> Option<super::diag_sites::ChargeProbe> {
    Some(super::diag_sites::ChargeProbe::begin())
}
fn emit_trace_impl(trace: GcCycleTrace) {
    trace.emit(GcStepSnapshot::current());
}

static SITE_TRIGGER_DECISION: Hook<fn(&'static str, &'static str)> = Hook::empty();
pub(super) fn site_trigger_decision(site: &'static str, kind: &'static str) {
    match SITE_TRIGGER_DECISION.get() {
        Some(f) => f(site, kind),
        None => (),
    }
}

static SITE_SET_FULL_SITE: Hook<fn(&'static str)> = Hook::empty();
pub(super) fn site_set_full_site(site: &'static str) {
    match SITE_SET_FULL_SITE.get() {
        Some(f) => f(site),
        None => (),
    }
}

static SITE_TAKE_FULL_SITE: Hook<fn() -> &'static str> = Hook::empty();
pub(super) fn site_take_full_site() -> &'static str {
    match SITE_TAKE_FULL_SITE.get() {
        Some(f) => f(),
        None => "sync",
    }
}

static SITE_FULL_STARTED: Hook<fn(&'static str, GcTriggerKind)> = Hook::empty();
pub(super) fn site_full_started(site: &'static str, trigger: GcTriggerKind) {
    match SITE_FULL_STARTED.get() {
        Some(f) => f(site, trigger),
        None => (),
    }
}

static SITE_BUDGETED_STARTED: Hook<fn(GcTriggerKind, GcCollectionKind, GcProgressKind)> =
    Hook::empty();
pub(super) fn site_budgeted_started(
    trigger: GcTriggerKind,
    collection: GcCollectionKind,
    progress: GcProgressKind,
) {
    match SITE_BUDGETED_STARTED.get() {
        Some(f) => f(trigger, collection, progress),
        None => (),
    }
}

static SITE_BUDGETED_STEP_DONE: Hook<fn(u32, u64, usize)> = Hook::empty();
pub(super) fn site_budgeted_step_done(phase_code: u32, elapsed_us: u64, units: usize) {
    match SITE_BUDGETED_STEP_DONE.get() {
        Some(f) => f(phase_code, elapsed_us, units),
        None => (),
    }
}

static SITE_BUDGETED_COMPLETED: Hook<fn(u64)> = Hook::empty();
pub(super) fn site_budgeted_completed(freed_bytes: u64) {
    match SITE_BUDGETED_COMPLETED.get() {
        Some(f) => f(freed_bytes),
        None => (),
    }
}

static SITE_REPORT_CHARGES: Hook<fn(&str)> = Hook::empty();
pub(super) fn site_report_charges(label: &str) {
    match SITE_REPORT_CHARGES.get() {
        Some(f) => f(label),
        None => (),
    }
}

static SITE_PRIMITIVE_DISPATCH: Hook<fn(&[u8], &str, u64)> = Hook::empty();
pub(crate) fn site_primitive_dispatch(builtin: &[u8], method: &str, recv_chars: u64) {
    match SITE_PRIMITIVE_DISPATCH.get() {
        Some(f) => f(builtin, method, recv_chars),
        None => (),
    }
}

static SITE_REPORT_PRIMITIVE_DISPATCH: Hook<fn(&str)> = Hook::empty();
pub(super) fn site_report_primitive_dispatch(label: &str) {
    match SITE_REPORT_PRIMITIVE_DISPATCH.get() {
        Some(f) => f(label),
        None => (),
    }
}

static ENABLED: Hook<fn() -> bool> = Hook::empty();
pub(crate) fn enabled() -> bool {
    match ENABLED.get() {
        Some(f) => f(),
        None => false,
    }
}

static START_TRACE: Hook<fn(GcCollectionKind, GcTriggerSnapshot) -> Option<GcCycleTrace>> =
    Hook::empty();
pub(super) fn start_trace(
    kind: GcCollectionKind,
    trigger: GcTriggerSnapshot,
) -> Option<GcCycleTrace> {
    match START_TRACE.get() {
        Some(f) => f(kind, trigger),
        None => None,
    }
}

static BEFORE_COLLECTION: Hook<fn()> = Hook::empty();
pub(super) fn before_collection() {
    match BEFORE_COLLECTION.get() {
        Some(f) => f(),
        None => (),
    }
}

static NOTE_RECLAIM_REASON: Hook<fn(&'static str)> = Hook::empty();
pub(super) fn note_reclaim_reason(why: &'static str) {
    match NOTE_RECLAIM_REASON.get() {
        Some(f) => f(why),
        None => (),
    }
}

static COLLECTION_LOG: Hook<fn(bool, GcTriggerKind, u64, u64)> = Hook::empty();
pub(super) fn collection_log(
    full: bool,
    trigger: GcTriggerKind,
    freed_bytes: u64,
    elapsed_us: u64,
) {
    match COLLECTION_LOG.get() {
        Some(f) => f(full, trigger, freed_bytes, elapsed_us),
        None => (),
    }
}

static EMIT_TRACE: Hook<fn(GcCycleTrace)> = Hook::empty();
pub(super) fn emit_trace(trace: GcCycleTrace) {
    match EMIT_TRACE.get() {
        Some(f) => f(trace),
        None => (),
    }
}

static BEGIN_CHARGE: Hook<fn() -> Option<super::diag_sites::ChargeProbe>> = Hook::empty();
pub(super) fn begin_charge() -> Option<super::diag_sites::ChargeProbe> {
    match BEGIN_CHARGE.get() {
        Some(f) => f(),
        None => None,
    }
}

static END_CHARGE: Hook<fn(super::diag_sites::ChargeProbe, usize, super::diag_sites::ChargeKind)> =
    Hook::empty();
pub(super) fn end_charge(
    probe: super::diag_sites::ChargeProbe,
    units: usize,
    kind: super::diag_sites::ChargeKind,
) {
    match END_CHARGE.get() {
        Some(f) => f(probe, units, kind),
        None => (),
    }
}

static LOG_BURST_BOUNDARY: Hook<fn(&'static str)> = Hook::empty();
pub(super) fn log_burst_boundary(arm: &'static str) {
    if let Some(log) = LOG_BURST_BOUNDARY.get() {
        log(arm);
    }
}

pub(crate) fn install() {
    LOG_BURST_BOUNDARY.set(super::allocation_pacing::report_burst_boundary);
    MINOR_COLLECTION.set(super::gc_collect_minor_with_diagnostics::<true>);
    CYCLE_STEP.set(GcCycleState::start_diagnostic_step);
    SITE_TRIGGER_DECISION.set(super::diag_sites::trigger_decision);
    SITE_SET_FULL_SITE.set(super::diag_sites::set_full_site);
    SITE_TAKE_FULL_SITE.set(super::diag_sites::take_full_site);
    SITE_FULL_STARTED.set(super::diag_sites::full_started);
    SITE_BUDGETED_STARTED.set(super::diag_sites::budgeted_started);
    SITE_BUDGETED_STEP_DONE.set(super::diag_sites::budgeted_step_done);
    SITE_BUDGETED_COMPLETED.set(super::diag_sites::budgeted_completed);
    SITE_REPORT_CHARGES.set(super::diag_sites::report_charges);
    SITE_PRIMITIVE_DISPATCH.set(super::diag_sites::primitive_dispatch);
    SITE_REPORT_PRIMITIVE_DISPATCH.set(super::diag_sites::report_primitive_dispatch);
    ENABLED.set(super::telemetry::gc_diag_enabled);
    START_TRACE.set(GcCycleTrace::new);
    BEFORE_COLLECTION.set(super::verify::verify_array_hole_tails_at_collection);
    NOTE_RECLAIM_REASON.set(super::telemetry::note_old_reclaim_why);
    COLLECTION_LOG.set(super::telemetry::log_collection);
    EMIT_TRACE.set(emit_trace_impl);
    BEGIN_CHARGE.set(begin_charge_impl);
    END_CHARGE.set(super::diag_sites::ChargeProbe::end);
}
