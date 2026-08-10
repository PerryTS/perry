//! Seeded GC-schedule fuzzing (#7154 tooling) — `PERRY_GC_SCHEDULE_SEED`.
//!
//! # Why a third setting between "normal" and "zeal"
//!
//! A #7154-class bug is a value that is live but not rooted across a collection
//! point. Whether it is *caught* depends entirely on whether a collection lands
//! inside that window, so the observed failure rate is a property of the
//! *schedule*, not of the bug. Two settings existed:
//!
//! - **Normal.** Collections are tens of megabytes apart. Socket Firewall's
//!   `sfw-registry --help` fails about 1 run in 60 here. Confirming a fix by
//!   repetition at that rate needs ~1000 runs; with zero failures in `N` runs
//!   the 95% upper bound on the true rate is only ~`3/N`, so 120 clean runs
//!   bound a 1.7% bug at 2.5% — no evidence at all.
//! - **`PERRY_GC_ZEAL=1`.** Collect at *every* safepoint. Maximum pressure, but
//!   all-or-nothing: it is one fixed schedule, it is slow, and it changes the
//!   program's timing enough that some workloads never reach the interesting
//!   code (on Socket Firewall's registry it dies in `node-machine-id` first, so
//!   zeal cannot be used there at all).
//!
//! This mode is the middle. `PERRY_GC_SCHEDULE_SEED=<u64>` makes the decision
//! *"should this safepoint collect?"* a deterministic pseudo-random function of
//! the seed and a monotonically increasing per-thread safepoint counter, at a
//! tunable density (`PERRY_GC_SCHEDULE_RATE`, default 5%). Two properties
//! follow, and they are the whole point:
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
//!    descends into `gc_safepoint_moving_minor` — exactly the bypass zeal
//!    performs, and for the same reason: the schedule cannot select a safepoint
//!    that the gate returned from.
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
//! - override an explicit `PERRY_GEN_GC_EVACUATE=0` — that wins, and with it set
//!   this mode moves nothing and surfaces nothing, exactly as with zeal;
//! - emit loop back-edge polls. Those need the **compile-time**
//!   `PERRY_GC_MOVING_LOOP_POLLS=1` (default off since #7161). Without them the
//!   mode only sees event-loop-boundary safepoints and a compute-only loop never
//!   collects. Compile *and* run with the poll opt-in;
//! - suppress or replace pressure-driven collections. The rate is *additional*
//!   density on top of what the budgeted collector already does, never less.
//!
//! `PERRY_GC_SCHEDULE_RATE=<float in [0,1]>` (default `0.05`) gates **only** the
//! threshold the schedule hash is compared against — the expected fraction of
//! handled safepoints at which a collection is forced. It is inert unless
//! `PERRY_GC_SCHEDULE_SEED` is set. `0` means never (a deliberately inert-but-on
//! configuration, useful as a control: the banner and reporter still install, so
//! an A/B against `rate>0` isolates the schedule from the reporting). `1` means
//! every handled safepoint, which is zeal's density — reachable, but if that is
//! what you want, `PERRY_GC_ZEAL=1` says so more plainly.
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
/// magnitude cheaper than zeal, and low enough that the program's own timing is
/// not so distorted that it fails somewhere uninteresting first.
pub(crate) const DEFAULT_SCHEDULE_RATE: f64 = 0.05;

/// Collections this mode has forced that would not otherwise have run. The
/// live-subject counter for every schedule-based verdict: a clean run with `0`
/// here exercised nothing (CLAUDE.md, "four ways a gate cannot fail" #4).
static SCHEDULE_FORCED: AtomicU64 = AtomicU64::new(0);

/// Handled safepoints seen by the schedule, summed across threads. Diagnostic
/// only — the per-thread counter that actually drives the schedule is the
/// thread-local below.
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
/// without losing one safepoint in 2^64 — irrelevant in practice, but a mode
/// whose rate-1 arm is not *exactly* zeal density is the sort of off-by-epsilon
/// that costs an investigation round when someone diffs the two.
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

/// Is seeded GC-schedule fuzzing on? One cached-`Option` load — the same cost
/// class as the zeal check beside it.
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

/// RAII test override. `threshold` is taken directly so tests can pin the
/// always/never arms without going through float parsing.
#[cfg(test)]
pub(crate) struct ScheduleGuard(Option<(u64, u64)>);

#[cfg(test)]
impl ScheduleGuard {
    pub(crate) fn set(seed: u64, threshold: u64) -> Self {
        // #7781: mirror `ZealGuard` — a schedule that cannot reach the poll
        // decides at six event-loop boundaries instead of thousands of
        // back-edges. Arm on set, release on drop, exactly the zeal pair.
        let prev = SCHEDULE_OVERRIDE.with(|cell| cell.replace(Some((seed, threshold))));
        if prev.is_none() {
            super::arm_poll();
        }
        Self(prev)
    }
    pub(crate) fn off() -> Self {
        let prev = SCHEDULE_OVERRIDE.with(|cell| cell.replace(None));
        if prev.is_some() {
            super::disarm_poll();
        }
        Self(prev)
    }
}

#[cfg(test)]
impl Drop for ScheduleGuard {
    fn drop(&mut self) {
        let restored = self.0;
        let current = SCHEDULE_OVERRIDE.with(|cell| cell.replace(restored));
        // Re-balance the arm to match the transition this drop performs.
        match (current.is_some(), restored.is_some()) {
            (true, false) => super::disarm_poll(),
            (false, true) => super::arm_poll(),
            _ => {}
        }
    }
}

#[cfg(test)]
pub(crate) fn reset_thread_counter_for_test() {
    SAFEPOINT_COUNTER.with(|cell| cell.set(0));
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
    if SUMMARY_EMITTED.swap(true, Ordering::SeqCst) {
        return;
    }
    eprintln!(
        "[gc-schedule] done: seed={seed} safepoints={} scheduled_collections={}",
        gc_schedule_safepoints(),
        gc_schedule_forced_collections(),
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
            let previous = old.sa_sigaction as u64;
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
    // 0 = SIG_DFL, 1 = SIG_IGN: nothing to chain to. Restore the default
    // disposition and return, so the instruction re-faults and the process dies
    // exactly where it should — core file, debugger and crash reporter all see
    // the real site.
    if previous > 1 {
        // SAFETY: the only handler this can chain to is one installed with
        // `SA_SIGINFO` by this process (today: the from-space quarantine's
        // reporter), so the three-argument form is its true signature.
        unsafe {
            let chained: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                std::mem::transmute(previous as usize as *const ());
            chained(signum, info, ctx);
        }
        return;
    }
    // SAFETY: standard handler teardown.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signum, &action, std::ptr::null_mut());
    }
}

/// Publish the seed where the signal handler can read it without allocating.
#[cfg(unix)]
fn publish_seed(seed: u64) {
    REPORTED_SEED.store(seed, Ordering::SeqCst);
}

#[cfg(not(unix))]
fn publish_seed(_seed: u64) {}
