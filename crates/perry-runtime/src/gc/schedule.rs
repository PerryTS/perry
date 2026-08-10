//! Seeded GC-schedule fuzzing (#7154 tooling) — `PERRY_GC_SCHEDULE_SEED`.
//!
//! # Why the collection schedule is a knob at all
//!
//! A #7154-class bug is a value that is live but not rooted across a collection
//! point. Whether it is *caught* depends entirely on whether a collection lands
//! inside that window, so the observed failure rate is a property of the
//! *schedule*, not of the bug. Normal pacing puts collections tens of megabytes
//! apart: Socket Firewall's `sfw-registry --help` fails about 1 run in 60 there.
//! Confirming a fix by repetition at that rate needs ~1000 runs; with zero
//! failures in `N` runs the 95% upper bound on the true rate is only ~`3/N`, so
//! 120 clean runs bound a 1.7% bug at 2.5% — no evidence at all.
//!
//! `PERRY_GC_SCHEDULE_SEED=<u64>` makes the decision *"should this safepoint
//! collect?"* a deterministic pseudo-random function of the seed and a
//! monotonically increasing per-thread safepoint counter, at a density
//! `PERRY_GC_SCHEDULE_RATE` tunes (default 5%). The whole range is one knob:
//! rate 1 collects at *every* handled safepoint — maximum pressure, one fixed
//! schedule, slow, and distorting enough that some workloads never reach the
//! interesting code (on Socket Firewall's registry a rate-1 run dies in
//! `node-machine-id` first, so the useful rates there are the low ones). Two
//! properties follow from the seed, and they are the whole point:
//!
//! 1. **Amplification.** Varying *when* collections fire explores the actual bug
//!    space. Re-running one fixed schedule explores almost nothing — which is
//!    why 60 identical runs find the same bug once.
//! 2. **A failing seed is a reproducer.** The schedule is a pure function of
//!    `(seed, counter)`. Same seed + same program + same inputs ⇒ same
//!    collection schedule, run to run. A fuzzer that finds a bug and loses the
//!    reproducer is worthless, so the seed is also printed on startup and again
//!    on any crash, abort or panic.
//!
//! # What the knobs actually gate
//!
//! Per CLAUDE.md's GC-knob policy, precisely:
//!
//! `PERRY_GC_SCHEDULE_SEED=<u64>` (unset ⇒ mode OFF, and OFF is inert):
//!
//! 1. `js_gc_loop_safepoint` stops requiring `GC_SAFEPOINT_PENDING` before it
//!    descends into `gc_safepoint_moving_minor`. The schedule cannot select a
//!    safepoint that the gate returned from.
//! 2. In `gc_safepoint_moving_minor`, the per-thread safepoint counter is
//!    advanced once per *handled* safepoint (i.e. after the entry guards, at the
//!    point `GC_SAFEPOINT_PENDING` is cleared), and when
//!    `gc_budgeted_due_trigger()` reports nothing due, a minor is run anyway iff
//!    the schedule selected this counter value.
//! 3. `gc_force_evacuate_enabled()` becomes true, so a scheduled minor **moves**
//!    survivors instead of sweeping in place. Without this the mode would be a
//!    knob whose name promises relocation stress and whose effect is sweep
//!    pressure — the failure `PERRY_GC_FORCE_EVACUATE` already cost this project
//!    once (#6942 / #6946).
//!
//! It does **not**:
//!
//! - bypass `gc_safepoint_moving_minor`'s entry guards. A safepoint reached
//!   mid-allocation (`GC_FLAG_IN_ALLOC`), suppressed (`GC_FLAG_SUPPRESSED`),
//!   inside an unsafe FFI zone, under a non-zero `GC_ROOT_LOCK_DEPTH`, or during
//!   a budgeted cycle still returns without collecting **and without ticking the
//!   counter**, so the schedule stays aligned with safepoints that could have
//!   collected;
//! - leave survivors in place: a resolved seed implies forced evacuation
//!   UNCONDITIONALLY. `PERRY_GEN_GC_EVACUATE`, whose `=0` used to veto that,
//!   was deleted by #7611 precisely because an ambient veto silently turned
//!   the #7154 instrument into a no-op — the vacuous-green shape the knob
//!   kill-policy exists to catch;
//! - emit loop back-edge polls. Those are a **compile-time** property
//!   (`PERRY_GC_MOVING_LOOP_POLLS`, default ON since #7721; a binary compiled
//!   with `=0` has none). Without them the mode only sees event-loop-boundary
//!   safepoints and a compute-only loop never collects — check the exit
//!   summary's `loop_polls=` before trusting a clean sweep;
//! - suppress or replace pressure-driven collections. The rate is *additional*
//!   density on top of what the budgeted collector already does, never less.
//!
//! `PERRY_GC_SCHEDULE_RATE=<float in [0,1]>` (default `0.05`) gates **only** the
//! threshold the schedule hash is compared against — the expected fraction of
//! handled safepoints at which a collection is forced. It is inert unless
//! `PERRY_GC_SCHEDULE_SEED` is set. `0` means never (a deliberately inert-but-on
//! configuration, useful as a control: the banner and reporter still install, so
//! an A/B against `rate>0` isolates the schedule from the reporting). `1` means
//! every handled safepoint — the maximum-pressure endpoint, where the seed stops
//! mattering because every ordinal is selected whatever it hashes to.
//!
//! # Determinism: the guarantee, and its limit
//!
//! The decision function is `schedule_hit(seed, counter, threshold)`: two rounds
//! of SplitMix64 over `(seed, counter)`. It reads **no** wall-clock time, **no**
//! address, **no** allocation state, and **no** thread identity. So for a
//! single-threaded program the schedule is reproducible run to run given the
//! same seed, binary and inputs.
//!
//! **The counter is thread-local, and that is the honest scope of the
//! guarantee.** Each thread in a `perry/thread` program has its own arena and
//! its own collector, and its own counter starting at zero; each thread's
//! schedule is therefore deterministic *given that thread's own sequence of
//! handled safepoints*. What is not guaranteed is that a multi-threaded program
//! executes the same sequence of safepoints per thread on every run — that
//! depends on OS scheduling, which this mode does not control and does not
//! pretend to. A global atomic counter would be strictly worse: it would make
//! even each individual thread's schedule depend on interleaving. So:
//! **deterministic for single-threaded programs; per-thread deterministic, but
//! not run-to-run reproducible, for multi-threaded ones.** Programs that read
//! the clock, the network or the filesystem are of course only as reproducible
//! as those inputs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Default expected fraction of handled safepoints that collect. Chosen as a
/// middle: ~20x normal density on a poll-heavy workload, but two orders of
/// magnitude cheaper than the rate-1 endpoint, and low enough that the program's
/// own timing is not so distorted that it fails somewhere uninteresting first.
pub(crate) const DEFAULT_SCHEDULE_RATE: f64 = 0.05;

/// Collections this mode has forced that would not otherwise have run. The
/// live-subject counter for every schedule-based verdict: a clean run with `0`
/// here exercised nothing (CLAUDE.md, "four ways a gate cannot fail" #4).
static SCHEDULE_FORCED: AtomicU64 = AtomicU64::new(0);

/// Handled safepoints seen by the schedule, summed across threads. Diagnostic
/// only — the per-thread counter that actually drives the schedule is the
/// thread-local below.
///
/// **Both counters are process-global, so a test asserting an exact delta on
/// them must hold `COPYING_NURSERY_TEST_LOCK`** (via `CopyingNurseryTestGuard`)
/// for the whole before/act/after window. Only a safepoint reached with the
/// mode ON ticks them, and the mode is a thread-local override in tests, so
/// today every ticking test already holds that lock and the deltas cannot
/// race. That is an invariant, not an accident: a new test that drives a
/// safepoint under `ScheduleGuard` without the nursery guard would make every
/// other test's `before + 1` flaky.
static SCHEDULE_SAFEPOINTS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The monotonically increasing safepoint ordinal this thread's schedule is
    /// a function of. Thread-local on purpose — see the determinism note above.
    static SAFEPOINT_COUNTER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

// ---------------------------------------------------------------------------
// Pure knob parsing + the decision function.
//
// Kept pure so both directions of both knobs are testable without mutating the
// process environment: the live readers cache in a `OnceLock`, so a test that
// set an env var would be at the mercy of which test ran first.
// ---------------------------------------------------------------------------

/// `PERRY_GC_SCHEDULE_SEED` — `None` (mode off) unless the value parses as a
/// `u64`. A typo must not silently enable a mode that changes when the collector
/// runs, so garbage reads as OFF rather than as seed 0.
pub(crate) fn parse_seed(raw: Option<&str>) -> Option<u64> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
}

/// `PERRY_GC_SCHEDULE_RATE` — expected fraction of handled safepoints that
/// collect. Unset, empty or unparseable ⇒ [`DEFAULT_SCHEDULE_RATE`]; out-of-range
/// values are clamped into `[0, 1]` rather than rejected, so a `2` reads as
/// "everything" instead of silently reverting to the default.
pub(crate) fn parse_rate(raw: Option<&str>) -> f64 {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => match value.parse::<f64>() {
            // NaN is the one parseable value with no sensible clamp, so it joins
            // the unparseable set. `inf` clamps to 1 like any other too-large
            // number — silently reverting it to 5% would leave the operator
            // believing they had turned the mode all the way up.
            Ok(rate) if !rate.is_nan() => rate.clamp(0.0, 1.0),
            _ => DEFAULT_SCHEDULE_RATE,
        },
        None => DEFAULT_SCHEDULE_RATE,
    }
}

/// SplitMix64. A fixed, portable, arithmetic-only bit mixer: identical output on
/// every target and every build, which `DefaultHasher` (deliberately unspecified,
/// and randomly seeded per process for `HashMap`) would not be.
const fn splitmix64(x: u64) -> u64 {
    let z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Sentinel threshold meaning "every handled safepoint". `u64::MAX` is also a
/// legitimate hash value, so `hit` cannot be expressed as a plain `<` at rate 1
/// without losing one safepoint in 2^64 — irrelevant in practice, but a rate-1
/// arm that is not *exactly* 100% density is the sort of off-by-epsilon that
/// costs an investigation round when someone checks the endpoint.
const THRESHOLD_ALWAYS: u64 = u64::MAX;

/// Map a rate in `[0, 1]` onto the threshold the schedule hash is compared
/// against. `0` ⇒ never (threshold `0`, and the comparison is strict `<`),
/// `1` ⇒ [`THRESHOLD_ALWAYS`].
pub(crate) fn rate_threshold(rate: f64) -> u64 {
    if rate.is_nan() || rate <= 0.0 {
        return 0;
    }
    if rate >= 1.0 {
        return THRESHOLD_ALWAYS;
    }
    // `2^64 * rate` saturated into u64. The product is computed in f64, so the
    // realised rate is accurate to ~2^-53 — far tighter than any workload can
    // resolve.
    let scaled = rate * (2.0_f64).powi(64);
    if scaled >= (u64::MAX as f64) {
        u64::MAX - 1
    } else {
        scaled as u64
    }
}

/// The decision: does the safepoint with ordinal `counter` collect under `seed`?
///
/// Pure. No clock, no address, no thread identity — see the determinism note at
/// the top of this file. The seed is mixed *through* SplitMix64 before being
/// combined with the counter so that adjacent seeds (`1`, `2`, `3`, … — exactly
/// what a sweep produces) give unrelated schedules rather than schedules that
/// agree on most safepoints.
pub(crate) fn schedule_hit(seed: u64, counter: u64, threshold: u64) -> bool {
    if threshold == 0 {
        return false;
    }
    if threshold == THRESHOLD_ALWAYS {
        return true;
    }
    splitmix64(splitmix64(seed) ^ counter) < threshold
}

// ---------------------------------------------------------------------------
// Live readers.
// ---------------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    /// Test-only override. Thread-local, so one test turning the mode on cannot
    /// change collector behaviour for any other test.
    static SCHEDULE_OVERRIDE: std::cell::Cell<Option<(u64, u64)>> =
        const { std::cell::Cell::new(None) };
}

/// Resolved `(seed, threshold)`, or `None` when the mode is off. Cached: the
/// environment is read exactly once per process.
fn resolved() -> Option<(u64, u64)> {
    #[cfg(test)]
    if let Some(over) = SCHEDULE_OVERRIDE.with(std::cell::Cell::get) {
        return Some(over);
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<Option<(u64, u64)>> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let seed = parse_seed(std::env::var("PERRY_GC_SCHEDULE_SEED").ok().as_deref())?;
        let rate = parse_rate(std::env::var("PERRY_GC_SCHEDULE_RATE").ok().as_deref());
        let resolved = (seed, rate_threshold(rate));
        // Announce on resolution, i.e. at the first safepoint of the run. The
        // seed must never be something the operator has to remember: it is in
        // the output from the start AND on every failure path below.
        publish_seed(seed);
        announce(seed, rate);
        install_failure_reporter();
        Some(resolved)
    })
}

/// Is seeded GC-schedule fuzzing on? One cached-`Option` load, so the default
/// path pays a predictable-branch check and nothing else.
pub(crate) fn gc_schedule_enabled() -> bool {
    resolved().is_some()
}

/// Advance this thread's safepoint ordinal and report whether the schedule
/// selects it. Called **once per handled safepoint** from
/// `gc_safepoint_moving_minor`, after its entry guards.
///
/// Returns `false` immediately when the mode is off, so the default path pays a
/// single cached load and nothing else.
pub(crate) fn schedule_tick() -> bool {
    let Some((seed, threshold)) = resolved() else {
        return false;
    };
    let counter = SAFEPOINT_COUNTER.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    SCHEDULE_SAFEPOINTS.fetch_add(1, Ordering::Relaxed);
    schedule_hit(seed, counter, threshold)
}

/// Record that the schedule forced a collection pressure would not have run.
#[inline]
pub(crate) fn note_schedule_forced_collection() {
    SCHEDULE_FORCED.fetch_add(1, Ordering::Relaxed);
}

/// How many collections the seeded schedule forced. A run that reports `0` here
/// exercised nothing — most often because the binary was compiled without
/// `PERRY_GC_MOVING_LOOP_POLLS=1` and the workload never reached the event loop,
/// or because `PERRY_GC_SCHEDULE_RATE=0`.
pub fn gc_schedule_forced_collections() -> u64 {
    SCHEDULE_FORCED.load(Ordering::Relaxed)
}

/// How many handled GC safepoints the schedule has seen, summed across threads.
/// The denominator for `gc_schedule_forced_collections()`: it distinguishes
/// "the schedule declined" from "there were no safepoints to decline".
pub fn gc_schedule_safepoints() -> u64 {
    SCHEDULE_SAFEPOINTS.load(Ordering::Relaxed)
}

/// The verdict a rate-1 run gets at exit: what the instrument actually did
/// (#7604).
///
/// `Some(Ok(summary))` when the maximum-density schedule moved something,
/// `Some(Err(complaint))` when it exercised nothing and every "clean at rate 1"
/// claim from the run is vacuous. `None` when the mode is off **or the rate is
/// below the every-safepoint endpoint**: at a sampling rate, a short run that
/// forces nothing is a legitimate outcome (`PERRY_GC_SCHEDULE_RATE=0` is the
/// documented on-but-selects-nothing control arm, and a sparse sweep seed that
/// fires late is not a broken instrument), so only the arm that PROMISED
/// maximum pressure is held to having produced it. Sub-endpoint runs still get
/// their liveness counters printed by `report_exit_summary`.
pub fn schedule_liveness_report() -> Option<Result<String, String>> {
    let (_, threshold) = resolved()?;
    if threshold < rate_threshold(1.0) {
        return None;
    }
    Some(schedule_verdict(
        gc_schedule_forced_collections(),
        super::instruments::copying_minor_cycles(),
        super::instruments::moved_objects_total(),
        super::instruments::loop_polls_reached(),
        super::policy::gc_moving_loop_polls_enabled(),
    ))
}

/// The verdict as a pure function of the counters, so the decision is testable
/// without mutating process-global state that every other test in this crate
/// shares.
///
/// `polls_requested` is the RUNTIME half of `PERRY_GC_MOVING_LOOP_POLLS`. When
/// it is on and `loop_polls` is still zero, the operator asked for in-loop
/// coverage and got none — the exact "arms but never fires" shape #7604
/// reported, and the one a `forced_collections > 0` from event-loop-boundary
/// collections would otherwise paper over.
pub(crate) fn schedule_verdict(
    forced: u64,
    cycles: u64,
    moved: u64,
    loop_polls: u64,
    polls_requested: bool,
) -> Result<String, String> {
    let summary = format!(
        "[gc-schedule] forced_collections={forced} copying_minors={cycles} \
         moved_objects={moved} loop_polls={loop_polls}"
    );
    let cause = if forced == 0 {
        Some("no safepoint ever forced a collection")
    } else if cycles == 0 {
        Some(
            "every forced collection was escalated to a non-moving full \
             mark-sweep, so nothing was relocated",
        )
    } else if polls_requested && loop_polls == 0 {
        Some(
            "PERRY_GC_MOVING_LOOP_POLLS=1 was set but NOT ONE back-edge poll \
             was reached, so every collection came from an event-loop \
             boundary and no loop body was covered",
        )
    } else {
        None
    };
    match cause {
        None => Ok(summary),
        Some(cause) => Err(format!(
            "{summary}\n\
             [gc-schedule] THIS RUN EXERCISED NOTHING WORTH TRUSTING. \
             PERRY_GC_SCHEDULE_RATE=1 was set and {cause}. Any \"clean at \
             rate 1\" conclusion from this run is vacuous.\n\
             [gc-schedule] The usual causes: the binary was COMPILED without \
             PERRY_GC_MOVING_LOOP_POLLS=1 (it is a compile-time opt-in as well \
             as a runtime one), or its hot loops are ones codegen emits no poll \
             for -- provably alloc-free bodies by design \
             (`loop_purity::loop_may_allocate`), and the specialized `for` / \
             `for-of` / `for-in` lowerings by omission (see \
             `emit_gc_loop_safepoint`'s COVERAGE note). `loop_polls` above is \
             the direct answer; do NOT try to count the call sites with \
             `nm`/`objdump`, which report 0 on a binary whose polls demonstrably \
             fire 20069 times."
        )),
    }
}

/// RAII test override. `threshold` is taken directly so tests can pin the
/// always/never arms without going through float parsing.
#[cfg(test)]
pub(crate) struct ScheduleGuard {
    prev: Option<(u64, u64)>,
    armed: bool,
}

#[cfg(test)]
impl ScheduleGuard {
    pub(crate) fn set(seed: u64, threshold: u64) -> Self {
        // #7781: a schedule that cannot reach the poll decides at six
        // event-loop boundaries instead of thousands of back-edges. Arm on
        // set, release on drop.
        //
        // The bookkeeping is deliberately ASYMMETRIC: only `set` arms, and only
        // its own `Drop` releases. `off()` must NOT disarm-then-let-Drop-rearm:
        // `disarm_poll` saturates at zero, so a disarm that lands on 0 is lost
        // while the paired re-arm is not — a permanent +1 leak that pins the
        // poll armed for the rest of the process. Over-arming for a guard's
        // lifetime costs a wasted call; a leaked arm is forever.
        let prev = SCHEDULE_OVERRIDE.with(|cell| cell.replace(Some((seed, threshold))));
        let armed = prev.is_none();
        if armed {
            super::poll_arm::arm_poll();
        }
        Self { prev, armed }
    }
    pub(crate) fn off() -> Self {
        let prev = SCHEDULE_OVERRIDE.with(|cell| cell.replace(None));
        Self { prev, armed: false }
    }
}

#[cfg(test)]
impl Drop for ScheduleGuard {
    fn drop(&mut self) {
        SCHEDULE_OVERRIDE.with(|cell| cell.set(self.prev));
        if self.armed {
            super::poll_arm::disarm_poll();
        }
    }
}

#[cfg(test)]
pub(crate) fn reset_thread_counter_for_test() {
    SAFEPOINT_COUNTER.with(|cell| cell.set(0));
}

// --------------------------------------------------- allocation pacing (#7728)
//
// `PERRY_GC_SCHEDULE_ALLOC_KB`: the poll arm only offers a safepoint to the
// seed once a stride of NEW nursery material has accumulated; `=0` restores the
// literal every-poll candidate set. Unpaced, the every-poll arm costs ~511 µs
// per loop iteration to relocate a mean of 5.9 objects — 24 minutes for a 19 s
// program (#7728) — which is an instrument nobody switches on. Pacing by allocation keeps the schedule deterministic:
// a deterministic program allocates deterministically, so `(seed, counter)`
// replay is unaffected; the stride only bounds which polls become candidates.

const SCHEDULE_DEFAULT_STRIDE_BYTES: usize = 4 * 1024;

/// Largest accepted stride: 1 GiB of new nursery material between candidates.
///
/// The cap exists because an UNBOUNDED stride is a silent off switch. A value
/// big enough to saturate — or merely bigger than the program ever allocates —
/// leaves `schedule_poll_collection_due` false after the first poll forever, so
/// the seed selects nothing on the poll path and the run reports a clean sweep
/// having tested nothing. That is the failure this project keeps paying for
/// (#6942 / #7024), and a debug instrument must not have a spelling that
/// disables it while still looking on.
const SCHEDULE_MAX_STRIDE_BYTES: usize = 1024 * 1024 * 1024;

/// Pure knob parse for `PERRY_GC_SCHEDULE_ALLOC_KB`, in KB.
///
/// `Some(0)` is a deliberate, meaningful value — "every poll is a candidate" —
/// so it must not be filtered out the way a nonsense value is. Anything above
/// [`SCHEDULE_MAX_STRIDE_BYTES`] CLAMPS to it rather than saturating, for the
/// reason given on that constant.
pub(crate) fn parse_schedule_alloc_kb(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .map(|kb| kb.saturating_mul(1024).min(SCHEDULE_MAX_STRIDE_BYTES))
        .unwrap_or(SCHEDULE_DEFAULT_STRIDE_BYTES)
}

/// Bytes of new nursery material required between poll-arm candidates.
pub(crate) fn schedule_poll_stride_bytes() -> usize {
    #[cfg(test)]
    if let Some(stride) = SCHEDULE_STRIDE_OVERRIDE.with(std::cell::Cell::get) {
        return stride;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        parse_schedule_alloc_kb(std::env::var("PERRY_GC_SCHEDULE_ALLOC_KB").ok().as_deref())
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only stride override, thread-local for the same reason
    /// `SCHEDULE_OVERRIDE` is.
    static SCHEDULE_STRIDE_OVERRIDE: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// RAII test override for the pacing stride.
#[cfg(test)]
pub(crate) struct ScheduleStrideGuard(Option<usize>);

#[cfg(test)]
impl ScheduleStrideGuard {
    pub(crate) fn set(stride_bytes: usize) -> Self {
        Self(SCHEDULE_STRIDE_OVERRIDE.with(|cell| cell.replace(Some(stride_bytes))))
    }
}

#[cfg(test)]
impl Drop for ScheduleStrideGuard {
    fn drop(&mut self) {
        SCHEDULE_STRIDE_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

thread_local! {
    /// From-space high-water mark at or above which the next poll-arm candidate
    /// is due. Per-thread because the arena it measures is.
    ///
    /// Starts at 0 so the FIRST poll is always a candidate: a program that
    /// allocates less than one stride in total must still exercise the
    /// instrument rather than silently becoming a run in which the schedule
    /// selected nothing.
    static SCHEDULE_NEXT_CANDIDATE_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Is this poll a pacing candidate, given current from-space bytes?
#[inline]
pub(crate) fn schedule_poll_collection_due(from_space_bytes: usize) -> bool {
    from_space_bytes >= SCHEDULE_NEXT_CANDIDATE_BYTES.with(std::cell::Cell::get)
}

/// Rearm the pacing high-water mark after a candidate poll ran the safepoint.
///
/// Takes the from-space level measured *after* the safepoint, so the next
/// candidate needs a full stride of genuinely new allocation on top of whatever
/// survived — a high-water mark rather than a delta, for the same reason as
/// #7728's original.
#[inline]
pub(crate) fn note_schedule_poll_collection(from_space_bytes_after: usize) {
    let next = from_space_bytes_after.saturating_add(schedule_poll_stride_bytes());
    SCHEDULE_NEXT_CANDIDATE_BYTES.with(|cell| cell.set(next));
}

/// Polls the pacing skipped before the seed ever saw them.
static SCHEDULE_POLLS_PACED: AtomicU64 = AtomicU64::new(0);

#[inline]
pub(crate) fn note_schedule_poll_paced() {
    SCHEDULE_POLLS_PACED.fetch_add(1, Ordering::Relaxed);
}

/// How many back-edge polls the pacing skipped. Reported in the exit summary so
/// a run states its own pacing rather than leaving the operator to infer it
/// from a safepoint count that looks lower than it "should" be.
pub fn schedule_polls_paced() -> u64 {
    SCHEDULE_POLLS_PACED.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn reset_schedule_pacing_for_test() {
    SCHEDULE_NEXT_CANDIDATE_BYTES.with(|cell| cell.set(0));
}

// ---------------------------------------------------------------------------
// Reporting the seed. Requirement: if the process crashes or aborts under this
// mode, the seed must appear in the output.
//
// Three layers, because the ways a perry process dies are not one thing:
//   1. a startup banner, so the seed is in the log even if the failure mode is
//      a hang or a `_exit` that runs no handler at all;
//   2. a panic hook, chained to whatever hook was installed before it, for Rust
//      panics and `panic = "abort"`;
//   3. a signal handler for the fatal set, chained to whatever handler was
//      installed before it — notably the from-space quarantine's SIGSEGV
//      reporter, which is the instrument this mode is expected to be paired
//      with.
// ---------------------------------------------------------------------------

static REPORTER_INSTALLED: AtomicBool = AtomicBool::new(false);

fn announce(seed: u64, rate: f64) {
    eprintln!(
        "[gc-schedule] seeded GC-schedule fuzzing ACTIVE: seed={seed} rate={rate}\n\
         [gc-schedule] reproduce with: PERRY_GC_SCHEDULE_SEED={seed} PERRY_GC_SCHEDULE_RATE={rate}"
    );
}

/// The one-line summary printed on every failure path. Built from the two
/// counters plus the resolved knobs, so a report also says whether the mode was
/// *doing* anything when the process died.
fn install_failure_reporter() {
    if REPORTER_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    // Capture the installing thread as the runtime main thread. The exit
    // summary is once-only and gated on `is_main_thread_or_unrecorded`, whose
    // unrecorded arm passes EVERY thread while no main thread is registered —
    // so a worker tearing down first could win the swap with non-final
    // counts. The schedule activates on the thread that owns its lifecycle,
    // which makes this the right owner for its summary.
    crate::native_handle::runtime_main_thread_id();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report_to_stderr("panic");
        previous(info);
    }));
    install_signal_reporter();
    install_exit_summary();
}

/// `atexit` as a backstop for exit paths that return through libc. Perry's own
/// exits deliberately do not (`process/env_misc.rs::terminate_without_atexit`
/// calls `_exit` to dodge cleanup handlers that have SIGILL'd), which is why the
/// primary hook is [`report_exit_summary`] on the teardown funnel and this is
/// only the belt to its braces. [`SUMMARY_EMITTED`] keeps them from
/// double-printing.
#[cfg(unix)]
fn install_exit_summary() {
    extern "C" fn summary() {
        report_exit_summary();
    }
    // SAFETY: `atexit` with a plain `extern "C" fn` that touches only atomics
    // and stderr.
    unsafe {
        libc::atexit(summary);
    }
}

#[cfg(not(unix))]
fn install_exit_summary() {}

static SUMMARY_EMITTED: AtomicBool = AtomicBool::new(false);

/// A run that exits 0 must still say how much schedule it actually executed.
///
/// Without this a sweep cannot tell "every seed passed" from "the binary was
/// compiled without `PERRY_GC_MOVING_LOOP_POLLS=1`, so there were no safepoints
/// to select and no seed could possibly have failed" — CLAUDE.md's fourth way a
/// gate cannot fail, and the one that would make this whole mode worthless.
/// `scripts/gc_schedule_fuzz.sh` reads the `safepoints=` field for exactly that
/// check.
///
/// Called from `js_gc_release_current_thread_collection_side_allocations`, which
/// every process-exit path funnels through (the generated exit epilogue,
/// `js_process_exit`, and the fatal-path teardown), and once only. Inert when
/// the mode is off.
pub(crate) fn report_exit_summary() {
    let Some((seed, _)) = resolved() else {
        return;
    };
    // Emit only from the main thread. Every thread routes through the
    // collection-side-allocation release on teardown, and the counters are
    // process-global atomics; a worker tearing down first would win the
    // once-only `swap` and print counts that are not yet final — and
    // `gc_schedule_fuzz.sh` reads `safepoints=0` as the vacuous case. The
    // main thread tears down at process exit, so it sees the true totals.
    // Falls back to emitting when the main thread was never recorded, so the
    // summary is never silently dropped.
    if !crate::native_handle::is_main_thread_or_unrecorded() {
        return;
    }
    if SUMMARY_EMITTED.swap(true, Ordering::SeqCst) {
        return;
    }
    eprintln!(
        "[gc-schedule] done: seed={seed} safepoints={} scheduled_collections={} \
         polls_paced={} copying_minors={} moved_objects={} loop_polls={}",
        gc_schedule_safepoints(),
        gc_schedule_forced_collections(),
        schedule_polls_paced(),
        super::instruments::copying_minor_cycles(),
        super::instruments::moved_objects_total(),
        super::instruments::loop_polls_reached(),
    );
}

/// Async-signal-safety is irrelevant on the panic path, so this half can format
/// freely.
fn report_to_stderr(cause: &str) {
    let Some((seed, _)) = resolved() else {
        return;
    };
    eprintln!(
        "\n[gc-schedule] FAILURE ({cause}) under seed={seed}\n\
         [gc-schedule]   safepoints={} scheduled_collections={}\n\
         [gc-schedule]   REPRODUCER: re-run this exact command with \
         PERRY_GC_SCHEDULE_SEED={seed} set.",
        gc_schedule_safepoints(),
        gc_schedule_forced_collections(),
    );
}

#[cfg(not(unix))]
fn install_signal_reporter() {}

/// Re-layer the schedule reporter on top of a handler installed after it.
///
/// The from-space quarantine installs its own SIGSEGV/SIGBUS reporter lazily, on
/// the first page-set retirement — i.e. always *after* this mode resolved, which
/// happens at the first safepoint. Without this hook the quarantine's install
/// would silently drop the seed line from precisely the pairing an investigator
/// reaches for (`PERRY_GC_SCHEDULE_SEED=… PERRY_GC_PROTECT_FROMSPACE=1`). Called
/// from `arena::quarantine::install_fault_reporter`; a no-op when this mode is
/// off, so a quarantine-only run keeps exactly today's signal disposition.
///
/// Unix-only, and so is its single caller: on Windows the quarantine's
/// `ProtectPages` mode has already degraded to poison-only because `mprotect` /
/// `sigaction` are not exposed there, so there is no reporter to install and
/// nothing to re-layer.
#[cfg(unix)]
pub(crate) fn reinstall_signal_reporter() {
    if !REPORTER_INSTALLED.load(Ordering::SeqCst) {
        return;
    }
    install_signal_reporter_inner();
}

#[cfg(unix)]
fn install_signal_reporter() {
    install_signal_reporter_inner();
}

/// Fatal signals worth reporting a seed for. `SIGABRT` covers `panic = "abort"`
/// and the runtime's own `abort()` paths; `SIGSEGV`/`SIGBUS` are the stale-deref
/// shapes this mode exists to provoke; `SIGILL`/`SIGTRAP` catch a corrupted code
/// pointer landing somewhere undecodable.
#[cfg(unix)]
const FATAL_SIGNALS: [libc::c_int; 5] = [
    libc::SIGSEGV,
    libc::SIGBUS,
    libc::SIGABRT,
    libc::SIGILL,
    libc::SIGTRAP,
];

/// Previously installed `sa_sigaction` per entry of [`FATAL_SIGNALS`], so the
/// handler can chain rather than clobber. `SIG_DFL` (0) and `SIG_IGN` (1) mean
/// "nothing to chain to".
#[cfg(unix)]
static PREVIOUS_HANDLERS: [AtomicU64; 5] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

#[cfg(unix)]
fn install_signal_reporter_inner() {
    for (slot, signum) in FATAL_SIGNALS.iter().copied().enumerate() {
        // SAFETY: standard `sigaction` install with an `SA_SIGINFO` handler; the
        // `old` out-parameter is a zeroed, correctly typed local.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            let mut old: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = schedule_fault_handler as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(signum, &action, &mut old) != 0 {
                continue;
            }
            // Only a handler installed with `SA_SIGINFO` has a valid
            // `sa_sigaction` (3-argument) member; a 1-argument `sa_handler`
            // installer leaves `sa_sigaction` meaningless to read, and the
            // chain site transmutes the stored value to a 3-argument fn. Store
            // 0 (treated as "nothing to chain to") unless the previous handler
            // was itself `SA_SIGINFO`, so we never call a 1-arg handler through
            // the wrong signature. SIG_DFL/SIG_IGN already read as 0/1.
            let previous = if old.sa_flags & libc::SA_SIGINFO != 0 {
                old.sa_sigaction as u64
            } else {
                0
            };
            // Never chain to ourselves: `reinstall_signal_reporter_after` can be
            // reached twice, and a self-chain is an infinite recursion inside a
            // signal handler.
            if previous != schedule_fault_handler as *const () as u64 {
                PREVIOUS_HANDLERS[slot].store(previous, Ordering::SeqCst);
            }
        }
    }
}

/// Minimal `write(2)` formatter. Deliberately avoids `format!` / `eprintln!` so
/// the handler does not allocate — the same discipline as the quarantine
/// reporter it chains to.
#[cfg(unix)]
struct SignalWriter {
    buf: [u8; 512],
    len: usize,
}

#[cfg(unix)]
impl SignalWriter {
    fn new() -> Self {
        Self {
            buf: [0; 512],
            len: 0,
        }
    }
    fn str(&mut self, s: &str) {
        for &byte in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = byte;
                self.len += 1;
            }
        }
    }
    fn dec(&mut self, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut n = 0;
        if value == 0 {
            digits[0] = b'0';
            n = 1;
        }
        while value != 0 {
            digits[n] = b'0' + (value % 10) as u8;
            n += 1;
            value /= 10;
        }
        for i in (0..n).rev() {
            if self.len < self.buf.len() {
                self.buf[self.len] = digits[i];
                self.len += 1;
            }
        }
    }
    fn flush(&self) {
        // SAFETY: writing `self.len` initialized bytes to stderr.
        unsafe {
            libc::write(2, self.buf.as_ptr() as *const libc::c_void, self.len);
        }
    }
}

/// The resolved seed, cached into a plain atomic so the signal handler never
/// touches the `OnceLock`/env path. Written by `resolved()` on the first call.
#[cfg(unix)]
static REPORTED_SEED: AtomicU64 = AtomicU64::new(u64::MAX);

#[cfg(unix)]
extern "C" fn schedule_fault_handler(
    signum: libc::c_int,
    info: *mut libc::siginfo_t,
    ctx: *mut libc::c_void,
) {
    let mut out = SignalWriter::new();
    out.str("\n[gc-schedule] FAILURE (signal ");
    out.dec(signum as u64);
    out.str(") under seed=");
    out.dec(REPORTED_SEED.load(Ordering::Relaxed));
    out.str("\n[gc-schedule]   safepoints=");
    out.dec(SCHEDULE_SAFEPOINTS.load(Ordering::Relaxed));
    out.str(" scheduled_collections=");
    out.dec(SCHEDULE_FORCED.load(Ordering::Relaxed));
    out.str("\n[gc-schedule]   REPRODUCER: re-run with PERRY_GC_SCHEDULE_SEED=");
    out.dec(REPORTED_SEED.load(Ordering::Relaxed));
    out.str("\n");
    out.flush();

    let slot = FATAL_SIGNALS.iter().position(|&s| s == signum);
    let previous = slot.map_or(0, |slot| PREVIOUS_HANDLERS[slot].load(Ordering::Relaxed));
    // Restore the default disposition for THIS signal *before* anything else.
    // Returning from a synchronous fault handler (SIGSEGV/SIGBUS/SIGILL) re-runs
    // the faulting instruction; if the chained handler below also returns
    // without resolving the fault, a disposition still pointing here would
    // re-enter this handler forever. With SIG_DFL restored first, the re-fault
    // dies at the real site — core file, debugger and crash reporter all see
    // it — no matter what the chained handler does.
    // SAFETY: standard handler teardown.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signum, &action, std::ptr::null_mut());
    }
    // 0 = SIG_DFL, 1 = SIG_IGN: nothing to chain to — fall through to the
    // now-restored default and re-fault.
    if previous > 1 {
        // SAFETY: `install_signal_reporter_inner` only stores a `previous`
        // value here when the predecessor was installed with `SA_SIGINFO`
        // (today: the from-space quarantine's reporter), so the three-argument
        // form is its true signature.
        unsafe {
            let chained: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                std::mem::transmute(previous as usize as *const ());
            chained(signum, info, ctx);
        }
    }
}

/// Publish the seed where the signal handler can read it without allocating.
#[cfg(unix)]
fn publish_seed(seed: u64) {
    REPORTED_SEED.store(seed, Ordering::SeqCst);
}

#[cfg(not(unix))]
fn publish_seed(_seed: u64) {}

#[cfg(test)]
mod verdict_tests {
    use super::*;

    /// The verdict must be able to say NO. Every counter combination that means
    /// "the instrument did not fire" is asserted individually, because they have
    /// different causes and the message has to name the right one.
    #[test]
    fn a_rate_one_run_that_exercised_nothing_is_an_error() {
        let no_safepoint =
            schedule_verdict(0, 0, 0, 0, false).expect_err("forced=0 must be an error");
        assert!(no_safepoint.contains("no safepoint ever forced a collection"));

        // The schedule DID force collections and every one was escalated to a
        // full mark-sweep, which moves nothing. `forced > 0` alone would have
        // called this run live.
        let all_escalated =
            schedule_verdict(4096, 0, 0, 4096, true).expect_err("cycles=0 must be an error");
        assert!(all_escalated.contains("escalated to a non-moving full"));
        assert!(all_escalated.contains("copying_minors=0"));
    }

    /// ★ #7604's own shape, and the one a two-counter verdict would have passed.
    ///
    /// Measured on the compute-only probe: `PERRY_GC_MOVING_LOOP_POLLS=1` set at
    /// both compile and run time, zero back-edge polls reached (codegen emits
    /// none for a provably alloc-free body), and the every-safepoint arm still
    /// forced 5 collections at event-loop boundaries which moved 4 objects.
    /// Every counter except `loop_polls` says "live"; no loop body was covered
    /// at all.
    #[test]
    fn polls_requested_but_never_reached_is_an_error() {
        let armed_never_fired = schedule_verdict(5, 5, 4, 0, true)
            .expect_err("polls requested and none reached must be an error");
        assert!(armed_never_fired.contains("NOT ONE back-edge poll"));
        assert!(armed_never_fired.contains("loop_polls=0"));

        // ...and the SAME counters without the request are fine: an
        // event-loop-boundary-only run is a legitimate, weaker mode, and
        // failing it would make the verdict wrong rather than strict.
        assert!(schedule_verdict(5, 5, 4, 0, false).is_ok());
    }

    /// ...and YES, with the numbers, when it did fire.
    #[test]
    fn a_rate_one_run_that_moved_objects_is_reported_ok() {
        let ok = schedule_verdict(741_630, 741_630, 8_899_560, 741_630, true)
            .expect("a moving run must pass");
        assert!(ok.contains("forced_collections=741630"));
        assert!(ok.contains("copying_minors=741630"));
        assert!(ok.contains("moved_objects=8899560"));
        assert!(ok.contains("loop_polls=741630"));
    }

    /// A copying minor that relocated nothing THIS cycle is still a live
    /// instrument — `moved=0` with `cycles>0` happens whenever the nursery had
    /// no survivors, and failing on it would make the verdict flaky rather than
    /// informative. Pinned so a future "tighten it to moved>0" edit has to
    /// argue with a test.
    #[test]
    fn a_copying_minor_with_no_survivors_is_not_a_failure() {
        assert!(schedule_verdict(1, 1, 0, 1, true).is_ok());
    }

    /// The verdict is an endpoint-only contract: below rate 1 a run that forces
    /// nothing is a legitimate sampling outcome (rate 0 is the documented
    /// control arm), so `schedule_liveness_report` must return `None` rather
    /// than an `Err` that would turn every sparse sweep seed into a false
    /// failure.
    #[test]
    fn sub_endpoint_rates_get_no_verdict() {
        let _g = ScheduleGuard::set(7, rate_threshold(0.05));
        assert!(schedule_liveness_report().is_none());
        let _g = ScheduleGuard::set(7, rate_threshold(1.0));
        assert!(schedule_liveness_report().is_some());
    }
}
