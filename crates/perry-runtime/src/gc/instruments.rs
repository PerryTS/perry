//! Instrument-liveness counters (#7604): did the run's GC stress actually
//! exercise anything?
//!
//! They are not properties of any one stress mode — they count what the
//! COLLECTOR did (copying minors completed, objects relocated, loop back-edge
//! polls reached) regardless of what forced it. That is why they live here
//! rather than inside `schedule.rs`: a counter that lives in the knob it
//! measures dies with the knob.
//!
//! Process-global rather than thread-local: the report is about the run.

use std::cell::Cell;
#[cfg(any(test, perry_gc_instruments))]
use std::sync::atomic::{AtomicU64, Ordering};

/// A liveness counter. Only the instrumented runtime build (the
/// `perry_gc_instruments` cfg, set by `scripts/build_instr_runtime.sh` and by
/// debug-profile builds) and lib-test builds of any profile (the unit tests
/// assert on these counters, so a release-profile test run must not read 0)
/// store and count. In the default release
/// archives it is zero-sized, `add` compiles to nothing and `get` is 0, so the
/// polls and GC events that note an event pay no locked read-modify-write for a
/// number nothing reads (a `lock incq` on the "nothing due" budgeted-step
/// poll was 1% of the instructions and 7% of the cycles of a regex loop).
pub(crate) struct Counter {
    #[cfg(any(test, perry_gc_instruments))]
    cell: AtomicU64,
}

impl Counter {
    pub(crate) const fn new() -> Self {
        Self {
            #[cfg(any(test, perry_gc_instruments))]
            cell: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    pub(crate) fn add(&self, n: u64) {
        #[cfg(any(test, perry_gc_instruments))]
        self.cell.fetch_add(n, Ordering::Relaxed);
        #[cfg(not(any(test, perry_gc_instruments)))]
        let _ = n;
    }

    #[inline(always)]
    pub(crate) fn get(&self) -> u64 {
        #[cfg(any(test, perry_gc_instruments))]
        return self.cell.load(Ordering::Relaxed);
        #[cfg(not(any(test, perry_gc_instruments)))]
        0
    }

    /// Raise the stored value to `value` if it is larger.
    #[inline(always)]
    pub(crate) fn raise_to(&self, value: u64) {
        #[cfg(any(test, perry_gc_instruments))]
        self.cell.fetch_max(value, Ordering::Relaxed);
        #[cfg(not(any(test, perry_gc_instruments)))]
        let _ = value;
    }

    /// Store `value` when the counter is 0.
    #[inline(always)]
    pub(crate) fn set_if_zero(&self, value: u64) {
        #[cfg(any(test, perry_gc_instruments))]
        let _ = self
            .cell
            .compare_exchange(0, value, Ordering::Relaxed, Ordering::Relaxed);
        #[cfg(not(any(test, perry_gc_instruments)))]
        let _ = value;
    }

    /// Return the stored value and reset it to 0.
    #[inline(always)]
    pub(crate) fn take(&self) -> u64 {
        #[cfg(any(test, perry_gc_instruments))]
        return self.cell.swap(0, Ordering::Relaxed);
        #[cfg(not(any(test, perry_gc_instruments)))]
        0
    }

    #[cfg(test)]
    pub(crate) fn reset(&self) {
        #[cfg(any(test, perry_gc_instruments))]
        self.cell.store(0, Ordering::Relaxed);
    }
}

// The default release archives carry no counter state at all: the cfg is the
// single switch, and a counter that stores in a build that does not instrument
// would put the locked read-modify-write back on the polls. A lib-test build
// always counts (see `Counter`), so the shipping shape is checked here, at
// compile time, in every build that is not a test or instrumented one.
#[cfg(not(any(test, perry_gc_instruments)))]
const _: () = assert!(std::mem::size_of::<Counter>() == 0);

static COPYING_MINORS: Counter = Counter::new();
static MOVED_OBJECTS: Counter = Counter::new();

/// Called once per COMPLETED copying minor, with what it relocated.
///
/// `copied + promoted`, not `copied` alone: #7657 made the explicit-`gc()` path
/// precise, which lets `gc/tenuring.rs` seed the adaptive threshold from these
/// cycles, and on two ratchet probes survivors are now promoted on first copy
/// rather than copied into survivor space. A `copied_objects > 0` liveness
/// assertion would have been pinned permanently false on exactly those probes.
#[inline]
pub(crate) fn note_copying_minor_moved(copied_objects: usize, promoted_objects: usize) {
    COPYING_MINORS.add(1);
    MOVED_OBJECTS.add((copied_objects + promoted_objects) as u64);
}

/// How many COPYING minors have completed in this process.
pub fn copying_minor_cycles() -> u64 {
    COPYING_MINORS.get()
}

/// `copied_objects + promoted_objects` summed over every copying minor.
pub fn moved_objects_total() -> u64 {
    MOVED_OBJECTS.get()
}

static LOOP_POLLS: Counter = Counter::new();

/// Every `js_gc_loop_safepoint` that got past the compile-time/runtime opt-in.
///
/// This is the counter that answers "was the COMPILE-TIME half live", and it
/// exists because the obvious external check does not work: `nm`/`objdump
/// -d BIN | grep -c js_gc_loop_safepoint` reports **0** on a binary whose polls
/// then fire 20069 times, so an operator following that advice concludes the
/// polls are absent when they are not. Measured, not assumed.
#[inline]
pub(crate) fn note_loop_poll_reached() {
    LOOP_POLLS.add(1);
}

/// How many loop back-edge polls this run reached.
pub fn loop_polls_reached() -> u64 {
    LOOP_POLLS.get()
}

static INCREMENTAL_CYCLE_STARTS: Counter = Counter::new();
static INCREMENTAL_STEPS: Counter = Counter::new();
static INCREMENTAL_COMPLETIONS: Counter = Counter::new();

crate::perry_thread_local! {
    static THREAD_INCREMENTAL_COMPLETIONS: Cell<u64> = const { Cell::new(0) };
}

/// A budgeted (incremental) cycle was STARTED.
#[inline]
pub(crate) fn note_incremental_cycle_start() {
    INCREMENTAL_CYCLE_STARTS.add(1);
}

/// One budgeted step advanced an active incremental cycle. This counts the
/// mutator-assist slices an allocating program pays *between* collections, so a
/// run whose `[gc]` output is empty can still show what the incremental
/// collector charged it.
#[inline]
pub(crate) fn note_incremental_step() {
    INCREMENTAL_STEPS.add(1);
}

/// A budgeted cycle reached its finisher.
#[inline]
pub(crate) fn note_incremental_completion() {
    INCREMENTAL_COMPLETIONS.add(1);
    THREAD_INCREMENTAL_COMPLETIONS.with(|count| count.set(count.get().saturating_add(1)));
}

/// Budgeted cycles completed on the collection thread.  Failure diagnostics
/// compare this with the value at the preceding verified minor, rather than
/// using the process-wide count (which would mix independent agent heaps).
pub(crate) fn incremental_completions_on_current_thread() -> u64 {
    THREAD_INCREMENTAL_COMPLETIONS.with(Cell::get)
}

/// Budgeted incremental cycles started in this process.
pub fn incremental_cycle_starts() -> u64 {
    INCREMENTAL_CYCLE_STARTS.get()
}

/// Budgeted incremental steps executed in this process.
pub fn incremental_steps() -> u64 {
    INCREMENTAL_STEPS.get()
}

/// Budgeted incremental cycles completed in this process.
pub fn incremental_completions() -> u64 {
    INCREMENTAL_COMPLETIONS.get()
}

static MARK_BARRIER_ARM_EVENTS: Counter = Counter::new();
static MARK_BARRIER_ARMED_US: Counter = Counter::new();
/// Microseconds-since-`process_epoch` at which the currently-open armed window
/// started, `0` when no window is open. Lock-free on purpose: this runs inside
/// the collector, where taking a lock is a hazard nobody should have to reason
/// about for a counter.
static MARK_BARRIER_ARMED_SINCE_US: Counter = Counter::new();

fn process_epoch_us() -> u64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros() as u64
}

/// The incremental (SATB) mark barrier became armed on some thread.
///
/// The barrier is not a per-cycle cost — it is a *per-microsecond* one. While
/// it is armed, every heap-pointer store, every shadow-slot root store and
/// every allocation (allocate-black) in the program pays for the cycle, so what
/// a cost investigation needs is the wall time it stays armed, not how many
/// cycles armed it. On `gc-handoff/apps/asyncpipe.ts` that is 37 ms of a 127 ms
/// program spent shading for a cycle that never completes and never collects
/// anything (#7909) — a number that was previously not observable at all.
pub(crate) fn note_mark_barrier_armed() {
    MARK_BARRIER_ARM_EVENTS.add(1);
    // `+1` so an arm at epoch microsecond 0 stays distinguishable from "closed".
    let now = process_epoch_us().saturating_add(1);
    MARK_BARRIER_ARMED_SINCE_US.set_if_zero(now);
}

/// The incremental mark barrier was disarmed.
pub(crate) fn note_mark_barrier_disarmed() {
    let started = MARK_BARRIER_ARMED_SINCE_US.take();
    if started != 0 {
        let now = process_epoch_us().saturating_add(1);
        MARK_BARRIER_ARMED_US.add(now.saturating_sub(started));
    }
}

/// How many times the incremental mark barrier was armed.
pub fn mark_barrier_arm_events() -> u64 {
    MARK_BARRIER_ARM_EVENTS.get()
}

/// Microseconds the incremental mark barrier has been armed, including a window
/// still open at the time of the call — a starved cycle's window never closes,
/// and that is exactly the case this number exists to report.
pub fn mark_barrier_armed_us() -> u64 {
    let closed = MARK_BARRIER_ARMED_US.get();
    let started = MARK_BARRIER_ARMED_SINCE_US.get();
    let open = if started == 0 {
        0
    } else {
        process_epoch_us().saturating_add(1).saturating_sub(started)
    };
    closed + open
}

static MOVING_SAFEPOINT_BLOCKED_BY_BUDGETED: Counter = Counter::new();

/// A precise safepoint was rejected because a budgeted cycle was active.
#[inline]
pub(crate) fn note_moving_safepoint_blocked_by_budgeted() {
    MOVING_SAFEPOINT_BLOCKED_BY_BUDGETED.add(1);
}

/// How many precise safepoints an active budgeted cycle rejected.
pub fn moving_safepoints_blocked_by_budgeted() -> u64 {
    MOVING_SAFEPOINT_BLOCKED_BY_BUDGETED.get()
}

static MOVING_SAFEPOINT_BLOCKED_BY_ALLOC: Counter = Counter::new();
static MOVING_SAFEPOINT_BLOCKED_BY_UNSAFE_ZONE: Counter = Counter::new();
static MOVING_SAFEPOINT_BLOCKED_BY_ROOT_LOCK: Counter = Counter::new();

/// The other three entry guards of `gc_safepoint_moving_minor`.
///
/// Before this existed only the budgeted arm was counted, so "the precise
/// collector did not run here" had exactly one observable explanation and
/// three invisible ones — and a hypothesis about a *different* suppressor
/// (an active `setjmp`/`try` region, #7910) could be neither confirmed nor
/// refuted from a run. Each arm is counted separately because they mean
/// different things: `in_alloc` and `root_lock` are transient and retried,
/// `unsafe_zone` is held across native work, and `budgeted` can be permanent.
#[inline]
pub(crate) fn note_moving_safepoint_blocked(in_alloc: bool, unsafe_zone: bool, root_lock: bool) {
    if in_alloc {
        MOVING_SAFEPOINT_BLOCKED_BY_ALLOC.add(1);
    }
    if unsafe_zone {
        MOVING_SAFEPOINT_BLOCKED_BY_UNSAFE_ZONE.add(1);
    }
    if root_lock {
        MOVING_SAFEPOINT_BLOCKED_BY_ROOT_LOCK.add(1);
    }
}

/// `(in_alloc_or_suppressed, unsafe_ffi_zone, root_lock_depth)` rejection counts.
pub fn moving_safepoints_blocked_by_other_guards() -> (u64, u64, u64) {
    (
        MOVING_SAFEPOINT_BLOCKED_BY_ALLOC.get(),
        MOVING_SAFEPOINT_BLOCKED_BY_UNSAFE_ZONE.get(),
        MOVING_SAFEPOINT_BLOCKED_BY_ROOT_LOCK.get(),
    )
}

/// Why a budgeted step did no work. Every arm is a distinct reason a cycle can
/// stall, and a stalled-but-active cycle keeps the mark barrier armed.
#[derive(Clone, Copy)]
pub(crate) enum BudgetedStepSkip {
    Reentrant,
    NoTrigger,
    StartBlocked,
    ResumeBlocked,
    /// #7909: a cycle was NOT started because the only due trigger was the
    /// young-generation scavenge cap, which a budgeted (low-pause non-moving)
    /// cycle cannot discharge. The pressure is deferred to the precise
    /// safepoint instead, where the copying minor can evacuate it.
    ///
    /// This is the one arm that is a *refusal*, not a stall: every other arm
    /// leaves the trigger owned by a cycle that will eventually run, while this
    /// one hands ownership to a different collector. It is counted because the
    /// alternative — starting the cycle — is invisible in the `[gc]` trace, so
    /// without a counter neither state can be told from "nothing was due".
    NurseryCapUndischargeable,
}

static SKIP_REENTRANT: Counter = Counter::new();
static SKIP_NO_TRIGGER: Counter = Counter::new();
static SKIP_START_BLOCKED: Counter = Counter::new();
static SKIP_RESUME_BLOCKED: Counter = Counter::new();
static SKIP_NURSERY_CAP_UNDISCHARGEABLE: Counter = Counter::new();

#[inline]
pub(crate) fn note_budgeted_step_skip(reason: BudgetedStepSkip) {
    match reason {
        BudgetedStepSkip::Reentrant => &SKIP_REENTRANT,
        BudgetedStepSkip::NoTrigger => &SKIP_NO_TRIGGER,
        BudgetedStepSkip::StartBlocked => &SKIP_START_BLOCKED,
        BudgetedStepSkip::ResumeBlocked => &SKIP_RESUME_BLOCKED,
        BudgetedStepSkip::NurseryCapUndischargeable => &SKIP_NURSERY_CAP_UNDISCHARGEABLE,
    }
    .add(1);
}

/// `(reentrant, no_trigger, start_blocked, resume_blocked)`.
pub fn budgeted_step_skips() -> (u64, u64, u64, u64) {
    (
        SKIP_REENTRANT.get(),
        SKIP_NO_TRIGGER.get(),
        SKIP_START_BLOCKED.get(),
        SKIP_RESUME_BLOCKED.get(),
    )
}

/// #7909: budgeted cycles declined because only the undischargeable young-gen
/// scavenge cap was due. Each one is a stalled cycle that did NOT happen.
pub fn budgeted_step_nursery_cap_deferrals() -> u64 {
    SKIP_NURSERY_CAP_UNDISCHARGEABLE.get()
}

// ---------------------------------------------------------------------------
// #7903 — step-boundedness telemetry
//
// `js_gc_step_us` can only check its clock BETWEEN work units, so the honest
// question about a "time-budgeted" collector is not "what budget was requested"
// but "how long did the longest single step actually take, and what was it
// doing". These counters answer that directly, and they are the liveness proof
// for the slicing work: a run where `weak_steps_sliced` is zero has not
// exercised the sliced path at all, however green it looks.
// ---------------------------------------------------------------------------

/// Longest single `cycle.state.step(...)` observed, microseconds.
static STEP_MAX_US: Counter = Counter::new();
/// Longest single ATOMIC final-remark (root re-scan + transitive drain).
///
/// Deliberately a separate number from `STEP_MAX_US`: final remark is an
/// intentionally atomic phase whose cost is bounded by the newly-reachable
/// graph, not by the step budget. Folding it into the general maximum would let
/// a heap-sized pause hide behind "the collector's worst step".
static REMARK_MAX_US: Counter = Counter::new();
static REMARK_COUNT: Counter = Counter::new();
/// FinalizationRegistry records scanned during weak processing.
static WEAK_RECORDS: Counter = Counter::new();
/// Most records charged to a single weak-processing step.
static WEAK_MAX_RECORDS_PER_STEP: Counter = Counter::new();
/// Steps that ended PARTWAY THROUGH one registry's record array — the sliced
/// path actually running. Zero means the slicing never happened.
static WEAK_STEPS_SLICED: Counter = Counter::new();
/// A registry's record cursor was invalidated by mutator restructuring.
static WEAK_REGISTRY_RESTARTS: Counter = Counter::new();
/// A registry hit the restart cap and was finished in one atomic pass.
static WEAK_REGISTRY_ATOMIC_FINISHES: Counter = Counter::new();

#[inline]
fn bump_max(slot: &Counter, value: u64) {
    slot.raise_to(value);
}

/// Record one budgeted step's wall duration.
#[inline]
pub(crate) fn note_budgeted_step_duration(us: u64) {
    bump_max(&STEP_MAX_US, us);
    STEP_TOTAL_US.add(us);
}

/// Record one atomic final-remark's wall duration.
#[inline]
pub(crate) fn note_final_remark_duration(us: u64) {
    bump_max(&REMARK_MAX_US, us);
    REMARK_COUNT.add(1);
    REMARK_TOTAL_US.add(us);
}

// ---------------------------------------------------------------------------
// Cumulative mutator-visible GC time — the concurrent-GC decision number.
//
// The maxima above answer "how bad is the worst pause"; these answer "what
// FRACTION of the run went to collection at all", which is the
// measurement-first gate for taking marking off the mutator thread: if the
// share on real workloads is single-digit percent, a concurrent collector
// has nothing worth hiding. Relaxed atomics, always on — one fetch_add per
// GC event is noise next to the event itself.
//
// `full_collect_total_us` measures the SYNCHRONOUS `js_gc_collect` entry
// wall-clock; a forced full that internally drives budgeted steps also
// charges those steps, so the sum of the four buckets can double-count that
// path. Report the buckets separately and let the reader add what applies —
// on the workloads this exists for (cc/pi), forced fulls are rare to absent.
// ---------------------------------------------------------------------------

/// Total microseconds spent in budgeted incremental steps.
static STEP_TOTAL_US: Counter = Counter::new();
/// Total microseconds spent in atomic final remarks.
static REMARK_TOTAL_US: Counter = Counter::new();
/// Total microseconds spent in copying minor scavenges.
static MINOR_TOTAL_US: Counter = Counter::new();
/// Total microseconds spent inside synchronous `js_gc_collect` calls.
static FULL_TOTAL_US: Counter = Counter::new();

/// Record one copying minor's pause duration.
#[inline]
pub(crate) fn note_copying_minor_pause_us(us: u64) {
    MINOR_TOTAL_US.add(us);
}

/// Record one synchronous full collection's wall duration.
#[inline]
pub(crate) fn note_full_collect_us(us: u64) {
    FULL_TOTAL_US.add(us);
}

/// (budgeted-step, final-remark, copying-minor, synchronous-full) cumulative
/// microseconds since process start.
pub fn gc_time_totals_us() -> (u64, u64, u64, u64) {
    (
        STEP_TOTAL_US.get(),
        REMARK_TOTAL_US.get(),
        MINOR_TOTAL_US.get(),
        FULL_TOTAL_US.get(),
    )
}

/// Microseconds since the first call in this process (the same epoch the
/// mark-barrier timer uses) — the wall-clock denominator for the share line.
pub(crate) fn wall_us_since_epoch() -> u64 {
    process_epoch_us()
}

/// Record the FinalizationRegistry records one weak-processing step charged,
/// and whether that step stopped mid-registry.
#[inline]
pub(crate) fn note_weak_step_records(records: u64, sliced: bool) {
    if records > 0 {
        WEAK_RECORDS.add(records);
        bump_max(&WEAK_MAX_RECORDS_PER_STEP, records);
    }
    if sliced {
        WEAK_STEPS_SLICED.add(1);
    }
}

#[inline]
pub(crate) fn note_weak_registry_restart() {
    WEAK_REGISTRY_RESTARTS.add(1);
}

#[inline]
pub(crate) fn note_weak_registry_atomic_finish() {
    WEAK_REGISTRY_ATOMIC_FINISHES.add(1);
}

/// Longest single budgeted step, microseconds.
pub fn step_max_us() -> u64 {
    STEP_MAX_US.get()
}

/// Longest single atomic final remark, microseconds.
pub fn final_remark_max_us() -> u64 {
    REMARK_MAX_US.get()
}

/// Atomic final remarks performed.
pub fn final_remark_count() -> u64 {
    REMARK_COUNT.get()
}

/// FinalizationRegistry records scanned during weak processing.
pub fn weak_records_scanned() -> u64 {
    WEAK_RECORDS.get()
}

/// Most records charged to a single weak-processing step.
pub fn weak_max_records_per_step() -> u64 {
    WEAK_MAX_RECORDS_PER_STEP.get()
}

/// Steps that ended partway through one registry's record array.
pub fn weak_steps_sliced() -> u64 {
    WEAK_STEPS_SLICED.get()
}

/// Record cursors invalidated by mutator restructuring.
pub fn weak_registry_restarts() -> u64 {
    WEAK_REGISTRY_RESTARTS.get()
}

/// Registries that hit the restart cap and were finished atomically.
pub fn weak_registry_atomic_finishes() -> u64 {
    WEAK_REGISTRY_ATOMIC_FINISHES.get()
}

/// Reset the #7903 step-boundedness counters. Test-only: the process-wide
/// statics would otherwise leak between in-process test cases.
#[cfg(test)]
pub(crate) fn reset_step_bound_counters() {
    STEP_MAX_US.reset();
    REMARK_MAX_US.reset();
    REMARK_COUNT.reset();
    WEAK_RECORDS.reset();
    WEAK_MAX_RECORDS_PER_STEP.reset();
    WEAK_STEPS_SLICED.reset();
    WEAK_REGISTRY_RESTARTS.reset();
    WEAK_REGISTRY_ATOMIC_FINISHES.reset();
}

/// Times one atomic final remark and records it on drop.
///
/// An RAII guard rather than a pair of calls so that an early `return` out of
/// the phase cannot silently drop the sample — an unmeasured atomic phase is
/// exactly the state #7903 exists to end.
pub(crate) struct FinalRemarkTimer(std::time::Instant);

impl FinalRemarkTimer {
    #[inline]
    pub(crate) fn start() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Drop for FinalRemarkTimer {
    fn drop(&mut self) {
        note_final_remark_duration(self.0.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
    }
}

/// Run-time knobs served by the `gc-instruments` cargo feature.
///
/// A binary built without the feature cannot run these instruments, and a
/// run that sets one of them is always asking a question the instrument was
/// supposed to answer (a stress schedule, a fault on stale from-space
/// access, a census dump). Answering it with silence would let every gate
/// built on the knob pass having exercised nothing, so the process aborts at
/// startup instead — see CLAUDE.md "Four ways a gate can be unable to fail".
// Read only by the feature-off startup check below.
#[cfg_attr(perry_gc_instruments, allow(dead_code))]
pub(crate) const INSTRUMENT_KNOBS: &[&str] = &[
    "PERRY_GC_CENSUS",
    "PERRY_GC_PROTECT_FROMSPACE",
    "PERRY_GC_PROTECT_OLD_SWEEP",
    "PERRY_GC_BUDGETED_OLD_RECLAIM",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_FROMSPACE_SCAN",
    "PERRY_GC_FROMSPACE_SCAN_ABORT",
    "PERRY_ALLOC_SITE_SAMPLE",
    "PERRY_GC_VERIFY_MARK",
    "PERRY_GC_VERIFY_CLASSIFIER",
    "PERRY_STACK_SYMBOLS",
    "PERRY_FIELD_REPR_VERIFY",
];

/// The first instrument knob set (non-empty) in the environment, if any.
#[cfg_attr(perry_gc_instruments, allow(dead_code))]
pub(crate) fn requested_instrument_knob() -> Option<&'static str> {
    INSTRUMENT_KNOBS
        .iter()
        .copied()
        .find(|knob| std::env::var_os(knob).is_some_and(|v| !v.is_empty()))
}

/// Startup check for binaries built without the instruments (see above).
#[cfg(not(perry_gc_instruments))]
pub(crate) fn refuse_instrument_knobs_without_instruments() {
    if let Some(knob) = requested_instrument_knob() {
        instruments_unavailable(knob);
    }
}

#[cfg(not(perry_gc_instruments))]
#[cold]
#[inline(never)]
fn instruments_unavailable(knob: &str) -> ! {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr(),
        "perry: {knob} is set, but this binary was built without the GC \
         instruments (cargo feature `gc-instruments`). Recompile with \
         PERRY_GC_INSTRUMENTS=1 (or with {knob} set while compiling) so the \
         instrument is linked in."
    );
    std::process::abort()
}

#[cfg(test)]
mod counter_tests {
    use super::Counter;

    /// The instrumented build (and every debug-profile test build) must really
    /// count: tests that read `copying_minor_cycles`, `loop_polls_reached`,
    /// `budgeted_step_skips`, ... depend on it.
    #[cfg(perry_gc_instruments)]
    #[test]
    fn counter_counts_in_the_instrumented_build() {
        let c = Counter::new();
        c.add(2);
        c.add(3);
        assert_eq!(c.get(), 5);
        c.raise_to(4);
        assert_eq!(c.get(), 5);
        c.raise_to(11);
        assert_eq!(c.get(), 11);
        assert_eq!(c.take(), 11);
        assert_eq!(c.get(), 0);
        c.set_if_zero(7);
        c.set_if_zero(8);
        assert_eq!(c.get(), 7);
        c.reset();
        assert_eq!(c.get(), 0);
    }
}
