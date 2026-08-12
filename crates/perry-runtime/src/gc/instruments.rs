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

use std::sync::atomic::{AtomicU64, Ordering};

static COPYING_MINORS: AtomicU64 = AtomicU64::new(0);
static MOVED_OBJECTS: AtomicU64 = AtomicU64::new(0);

/// Called once per COMPLETED copying minor, with what it relocated.
///
/// `copied + promoted`, not `copied` alone: #7657 made the explicit-`gc()` path
/// precise, which lets `gc/tenuring.rs` seed the adaptive threshold from these
/// cycles, and on two ratchet probes survivors are now promoted on first copy
/// rather than copied into survivor space. A `copied_objects > 0` liveness
/// assertion would have been pinned permanently false on exactly those probes.
#[inline]
pub(crate) fn note_copying_minor_moved(copied_objects: usize, promoted_objects: usize) {
    COPYING_MINORS.fetch_add(1, Ordering::Relaxed);
    MOVED_OBJECTS.fetch_add(
        (copied_objects + promoted_objects) as u64,
        Ordering::Relaxed,
    );
}

/// How many COPYING minors have completed in this process.
pub fn copying_minor_cycles() -> u64 {
    COPYING_MINORS.load(Ordering::Relaxed)
}

/// `copied_objects + promoted_objects` summed over every copying minor.
pub fn moved_objects_total() -> u64 {
    MOVED_OBJECTS.load(Ordering::Relaxed)
}

static LOOP_POLLS: AtomicU64 = AtomicU64::new(0);

/// Every `js_gc_loop_safepoint` that got past the compile-time/runtime opt-in.
///
/// This is the counter that answers "was the COMPILE-TIME half live", and it
/// exists because the obvious external check does not work: `nm`/`objdump
/// -d BIN | grep -c js_gc_loop_safepoint` reports **0** on a binary whose polls
/// then fire 20069 times, so an operator following that advice concludes the
/// polls are absent when they are not. Measured, not assumed.
#[inline]
pub(crate) fn note_loop_poll_reached() {
    LOOP_POLLS.fetch_add(1, Ordering::Relaxed);
}

/// How many loop back-edge polls this run reached.
pub fn loop_polls_reached() -> u64 {
    LOOP_POLLS.load(Ordering::Relaxed)
}

static INCREMENTAL_CYCLE_STARTS: AtomicU64 = AtomicU64::new(0);
static INCREMENTAL_STEPS: AtomicU64 = AtomicU64::new(0);
static INCREMENTAL_COMPLETIONS: AtomicU64 = AtomicU64::new(0);

/// A budgeted (incremental) cycle was STARTED.
#[inline]
pub(crate) fn note_incremental_cycle_start() {
    INCREMENTAL_CYCLE_STARTS.fetch_add(1, Ordering::Relaxed);
}

/// One budgeted step advanced an active incremental cycle. This counts the
/// mutator-assist slices an allocating program pays *between* collections, so a
/// run whose `[gc]` output is empty can still show what the incremental
/// collector charged it.
#[inline]
pub(crate) fn note_incremental_step() {
    INCREMENTAL_STEPS.fetch_add(1, Ordering::Relaxed);
}

/// A budgeted cycle reached its finisher.
#[inline]
pub(crate) fn note_incremental_completion() {
    INCREMENTAL_COMPLETIONS.fetch_add(1, Ordering::Relaxed);
}

/// Budgeted incremental cycles started in this process.
pub fn incremental_cycle_starts() -> u64 {
    INCREMENTAL_CYCLE_STARTS.load(Ordering::Relaxed)
}

/// Budgeted incremental steps executed in this process.
pub fn incremental_steps() -> u64 {
    INCREMENTAL_STEPS.load(Ordering::Relaxed)
}

/// Budgeted incremental cycles completed in this process.
pub fn incremental_completions() -> u64 {
    INCREMENTAL_COMPLETIONS.load(Ordering::Relaxed)
}

static MARK_BARRIER_ARM_EVENTS: AtomicU64 = AtomicU64::new(0);
static MARK_BARRIER_ARMED_US: AtomicU64 = AtomicU64::new(0);
/// Microseconds-since-`process_epoch` at which the currently-open armed window
/// started, `0` when no window is open. Lock-free on purpose: this runs inside
/// the collector, where taking a lock is a hazard nobody should have to reason
/// about for a counter.
static MARK_BARRIER_ARMED_SINCE_US: AtomicU64 = AtomicU64::new(0);

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
    MARK_BARRIER_ARM_EVENTS.fetch_add(1, Ordering::Relaxed);
    // `+1` so an arm at epoch microsecond 0 stays distinguishable from "closed".
    let now = process_epoch_us().saturating_add(1);
    let _ =
        MARK_BARRIER_ARMED_SINCE_US.compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
}

/// The incremental mark barrier was disarmed.
pub(crate) fn note_mark_barrier_disarmed() {
    let started = MARK_BARRIER_ARMED_SINCE_US.swap(0, Ordering::Relaxed);
    if started != 0 {
        let now = process_epoch_us().saturating_add(1);
        MARK_BARRIER_ARMED_US.fetch_add(now.saturating_sub(started), Ordering::Relaxed);
    }
}

/// How many times the incremental mark barrier was armed.
pub fn mark_barrier_arm_events() -> u64 {
    MARK_BARRIER_ARM_EVENTS.load(Ordering::Relaxed)
}

/// Microseconds the incremental mark barrier has been armed, including a window
/// still open at the time of the call — a starved cycle's window never closes,
/// and that is exactly the case this number exists to report.
pub fn mark_barrier_armed_us() -> u64 {
    let closed = MARK_BARRIER_ARMED_US.load(Ordering::Relaxed);
    let started = MARK_BARRIER_ARMED_SINCE_US.load(Ordering::Relaxed);
    let open = if started == 0 {
        0
    } else {
        process_epoch_us().saturating_add(1).saturating_sub(started)
    };
    closed + open
}

static MOVING_SAFEPOINT_BLOCKED_BY_BUDGETED: AtomicU64 = AtomicU64::new(0);

/// A precise safepoint was rejected because a budgeted cycle was active.
#[inline]
pub(crate) fn note_moving_safepoint_blocked_by_budgeted() {
    MOVING_SAFEPOINT_BLOCKED_BY_BUDGETED.fetch_add(1, Ordering::Relaxed);
}

/// How many precise safepoints an active budgeted cycle rejected.
pub fn moving_safepoints_blocked_by_budgeted() -> u64 {
    MOVING_SAFEPOINT_BLOCKED_BY_BUDGETED.load(Ordering::Relaxed)
}

static MOVING_SAFEPOINT_BLOCKED_BY_ALLOC: AtomicU64 = AtomicU64::new(0);
static MOVING_SAFEPOINT_BLOCKED_BY_UNSAFE_ZONE: AtomicU64 = AtomicU64::new(0);
static MOVING_SAFEPOINT_BLOCKED_BY_ROOT_LOCK: AtomicU64 = AtomicU64::new(0);

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
        MOVING_SAFEPOINT_BLOCKED_BY_ALLOC.fetch_add(1, Ordering::Relaxed);
    }
    if unsafe_zone {
        MOVING_SAFEPOINT_BLOCKED_BY_UNSAFE_ZONE.fetch_add(1, Ordering::Relaxed);
    }
    if root_lock {
        MOVING_SAFEPOINT_BLOCKED_BY_ROOT_LOCK.fetch_add(1, Ordering::Relaxed);
    }
}

/// `(in_alloc_or_suppressed, unsafe_ffi_zone, root_lock_depth)` rejection counts.
pub fn moving_safepoints_blocked_by_other_guards() -> (u64, u64, u64) {
    (
        MOVING_SAFEPOINT_BLOCKED_BY_ALLOC.load(Ordering::Relaxed),
        MOVING_SAFEPOINT_BLOCKED_BY_UNSAFE_ZONE.load(Ordering::Relaxed),
        MOVING_SAFEPOINT_BLOCKED_BY_ROOT_LOCK.load(Ordering::Relaxed),
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
}

static SKIP_REENTRANT: AtomicU64 = AtomicU64::new(0);
static SKIP_NO_TRIGGER: AtomicU64 = AtomicU64::new(0);
static SKIP_START_BLOCKED: AtomicU64 = AtomicU64::new(0);
static SKIP_RESUME_BLOCKED: AtomicU64 = AtomicU64::new(0);

#[inline]
pub(crate) fn note_budgeted_step_skip(reason: BudgetedStepSkip) {
    match reason {
        BudgetedStepSkip::Reentrant => &SKIP_REENTRANT,
        BudgetedStepSkip::NoTrigger => &SKIP_NO_TRIGGER,
        BudgetedStepSkip::StartBlocked => &SKIP_START_BLOCKED,
        BudgetedStepSkip::ResumeBlocked => &SKIP_RESUME_BLOCKED,
    }
    .fetch_add(1, Ordering::Relaxed);
}

/// `(reentrant, no_trigger, start_blocked, resume_blocked)`.
pub fn budgeted_step_skips() -> (u64, u64, u64, u64) {
    (
        SKIP_REENTRANT.load(Ordering::Relaxed),
        SKIP_NO_TRIGGER.load(Ordering::Relaxed),
        SKIP_START_BLOCKED.load(Ordering::Relaxed),
        SKIP_RESUME_BLOCKED.load(Ordering::Relaxed),
    )
}
