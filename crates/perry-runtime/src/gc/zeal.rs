//! GC zeal mode (#7154 tooling) — `PERRY_GC_ZEAL`.
//!
//! # Why
//!
//! A #7154-class bug is a value that is live but not rooted across a collection
//! point. Whether it is *caught* depends entirely on whether a collection
//! happens to land inside that window. In a normal run the window is a few
//! instructions wide and collections are tens of megabytes apart, so the bug is
//! observed only when an unrelated allocation burst lines up with it — which is
//! why the #7154 hunt needed a `zod` workload and ten rounds.
//!
//! Zeal removes the coincidence. Modelled on V8's `--stress-scavenge` and
//! SpiderMonkey's `gcZeal`, it forces an **evacuating** minor at GC safepoints,
//! so an unrooted value moves on its first exposure, deterministically. Since
//! #7728 that is allocation-PACED rather than literally every safepoint — see
//! the pacing section below for why, and for the escape hatch that restores it.
//!
//! # What the knob actually gates
//!
//! `PERRY_GC_ZEAL=1`:
//!
//! 1. A loop back-edge poll (`js_gc_loop_safepoint`) runs a minor, instead of
//!    only draining an already-deferred one (`GC_SAFEPOINT_PENDING`) — at the
//!    first poll past each `PERRY_GC_ZEAL_ALLOC_KB` of new nursery material
//!    (#7728); at EVERY poll when that is `0`.
//! 2. Every outermost microtask-pump safepoint runs a minor, instead of only
//!    when `gc_budgeted_due_trigger()` reports nursery/old pressure.
//! 3. `gc_force_evacuate_enabled()` becomes true, so the minor **moves** every
//!    marked non-pinned nursery object rather than leaving survivors in place.
//!    Without this a zealous minor could run and move nothing, which would be a
//!    gate that cannot fail.
//!
//! It does **not** change which collections are *sound* — every forced
//! collection runs at a point the collector already treats as a precise-root
//! safepoint. It only changes how often.
//!
//! ## Point 1 requires a compile-time opt-in too
//!
//! Loop back-edge polls are only *emitted* when the compiler ran with
//! `PERRY_GC_MOVING_LOOP_POLLS` on — **default ON since #7721**, off from #7161
//! until then. Zeal cannot conjure a poll that codegen never emitted. A binary
//! compiled with `PERRY_GC_MOVING_LOOP_POLLS=0` still gets (2) and (3) —
//! event-loop-boundary zeal — but a compute-only loop that never yields will not
//! collect at all. That configuration is exactly what made zeal look free before
//! #7721: it was collecting nothing. Check `loop_polls=` in the exit verdict.
//!
//! Codegen also emits no poll for a provably alloc-free loop body (by design —
//! `loop_purity::loop_may_allocate`) nor for the specialized `for` / `for-of` /
//! `for-in` lowerings (by omission — `emit_gc_loop_safepoint`'s COVERAGE note),
//! so "I passed the flag" is not the same claim as "my loop has a poll".
//!
//! ## You do not have to remember to check any of this (#7604)
//!
//! A zeal run prints its own verdict at the process-exit boundary and **exits
//! 70** when it exercised nothing — see `zeal_verdict`. Before that, the
//! liveness counter this module exposes was unreadable from a compiled program
//! and the documented check could not be performed.
//!
//! # Pacing: why "every poll" is not a usable default (#7254, #7728)
//!
//! Point 1 above says "every loop back-edge poll", and until #7728 it meant
//! that literally. That was affordable only for as long as back-edge polls were
//! a compile-time opt-in nobody took: with `PERRY_GC_MOVING_LOOP_POLLS` default
//! OFF (#7161), a compute-only program reached **no** loop safepoint, so zeal
//! forced nothing, cost nothing — and proved nothing. #7721 made the poll
//! default ON, which is a large throughput win for the collector and turned
//! zeal from free-and-vacuous into correct-but-unusable in the same commit.
//!
//! Measured on the pinned quiet host, a tree-walking-interpreter workload
//! (`iso_miss.ts`, 4.5 s without zeal there):
//!
//! | rounds | polls | forced collections | wall |
//! |---|--:|--:|--:|
//! | 1 | 70,963 | 70,968 | 36.3 s |
//! | 2 | 141,926 | 141,931 | 72.7 s |
//! | 40 (extrapolated, linear) | ~2.84 M | ~2.84 M | ~24 min |
//!
//! Perfectly linear — this was never a livelock, it was ~511 µs of fixed
//! per-collection cost (root scan over the shadow stack plus ~55 side-table
//! scanners) paid once per loop iteration, to relocate a mean of **5.9
//! objects**. Practically all of that work is the collection's fixed overhead,
//! not the relocation zeal exists to stress. An instrument whose smallest
//! honest run takes 24 minutes is an instrument nobody switches on, and #7254
//! had already logged "a striking concentration of multi-minute-plus runs"
//! under this pairing without triaging it.
//!
//! So zeal is **allocation-paced**: it forces a collection at the first poll at
//! which `PERRY_GC_ZEAL_ALLOC_KB` of new nursery material has accumulated since
//! the last one (default 4 KB — roughly a nursery block's worth of objects, and
//! ~1/4000th of the 16 MB cap the ordinary scavenge trigger uses). This is V8's
//! `--gc-interval` model and SpiderMonkey's `gcZeal(mode, frequency)`, both of
//! which pace for the same reason.
//!
//! Measured on the same workload after the change: **98.8 s and the correct
//! answer**, with 193,087 forced collections out of 2,838,560 polls, all of them
//! copying minors, relocating 3,115,719 objects. Unpaced the same run is
//! ~1,426 s. See `ZEAL_DEFAULT_STRIDE_BYTES` for the stride sweep that picked
//! the default.
//!
//! **`PERRY_GC_ZEAL_ALLOC_KB=0` restores the literal every-poll semantics**, and
//! that is the right setting for a small fixture (`gc_instrument_smoke.sh` pins
//! it) or for a window executed only once. What pacing gives up is precisely
//! that: a bug window crossed a single time may now sit between two forced
//! collections. A window that *recurs* — every shape in the #7154 family, which
//! is why the reproducers are loops — is still caught, just after N KB of
//! allocation rather than on the first iteration.
//!
//! The pacing is a **monotone** high-water mark, not a "bytes since" delta:
//! each forced collection rearms to `from_space_after + stride`, so a collection
//! that reclaims nothing (an escalation to a non-moving full mark-sweep) still
//! demands another `stride` bytes of real allocation before the next one. Total
//! forced collections are therefore bounded by `bytes_allocated / stride`
//! whatever the collector does with them — the property that makes the fix a
//! bound rather than a hope, and the one #7592's livelock lacked.
//!
//! # Why there is no allocation-point level
//!
//! An obvious `PERRY_GC_ZEAL=2` would collect at every allocation. It was
//! deliberately not implemented: the allocation-point arm in `gc_check_trigger`
//! takes `ManualGcScanGuard::force_full_scan`, and a forced conservative stack
//! scan makes the copying minor ineligible
//! (`CopiedMinorFallbackReason::ConservativeStack`). A level 2 would therefore
//! run many *non-moving* minors and move nothing — a knob whose name promises
//! relocation stress and whose effect is sweep pressure. That is precisely the
//! failure `PERRY_GC_FORCE_EVACUATE` already cost this project once (#6942 /
//! #6946), so the level does not exist rather than existing untrustworthy.

use std::sync::atomic::{AtomicU64, Ordering};

/// Collections zeal has forced that would not otherwise have run. The live-
/// subject counter for every zeal-based verdict.
static ZEAL_FORCED: AtomicU64 = AtomicU64::new(0);

/// Pure knob parse, so the mapping is testable without mutating the process
/// environment (the live reader caches in a `OnceLock`).
pub(crate) fn parse_zeal(raw: Option<&str>) -> bool {
    matches!(raw, Some("1") | Some("on") | Some("true"))
}

#[cfg(test)]
thread_local! {
    /// Test-only override. Thread-local, so one test turning zeal on cannot
    /// change collector behaviour for any other test.
    static ZEAL_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// `PERRY_GC_ZEAL=1`/`on`/`true` — force an evacuating minor at every safepoint.
pub(crate) fn gc_zeal_enabled() -> bool {
    #[cfg(test)]
    if let Some(zeal) = ZEAL_OVERRIDE.with(std::cell::Cell::get) {
        return zeal;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| parse_zeal(std::env::var("PERRY_GC_ZEAL").ok().as_deref()))
}

/// RAII test override for zeal: the previous override, and whether this guard
/// took an arm on the back-edge poll's global word that it owes back.
#[cfg(test)]
pub(crate) struct ZealGuard(Option<bool>, bool);

#[cfg(test)]
impl ZealGuard {
    pub(crate) fn set(enabled: bool) -> Self {
        // Mirror production: zeal keeps the back-edge poll's global arming word
        // non-zero (`gc/poll_arm.rs::resolve_poll_seed`), because a poll that
        // reads zero never calls in and so can force nothing. A test vehicle
        // that skipped this would let `js_gc_loop_safepoint` no-op under a
        // `ZealGuard` and report the collection zeal never got to run.
        if enabled {
            super::arm_poll();
        }
        Self(
            ZEAL_OVERRIDE.with(|cell| cell.replace(Some(enabled))),
            enabled,
        )
    }
}

#[cfg(test)]
impl Drop for ZealGuard {
    fn drop(&mut self) {
        ZEAL_OVERRIDE.with(|cell| cell.set(self.0));
        if self.1 {
            super::disarm_poll();
        }
    }
}

#[inline]
pub(crate) fn note_zeal_forced_collection() {
    ZEAL_FORCED.fetch_add(1, Ordering::Relaxed);
}

// ------------------------------------------------------------- pacing (#7728)

/// Default stride: 4 KB of new nursery material between zeal-forced collections.
///
/// Measured, not picked. The whole sweep below is ONE binary and ONE env var on
/// the pinned quiet host — the interpreter workload at a quarter scale, whose
/// `loop_polls` is **283,852 in every row**, so the only thing the knob changes
/// is the decision to collect, not the number of safepoints:
///
/// | `ALLOC_KB` | forced collections | moved objects | wall |
/// |---|--:|--:|--:|
/// | 0 (pre-#7728) | 283,857 | 1,629,647 | 142.6 s |
/// | 1 | 70,929 | 815,460 | 36.1 s |
/// | **4 (default)** | **19,314** | **325,830** | **10.2 s** |
/// | 16 | 5,070 | 129,959 | 3.0 s |
/// | 64 | 1,291 | 52,357 | 1.1 s |
///
/// Row 0 is the pre-fix behaviour reproduced exactly — 283,857 collections for
/// 283,852 polls, i.e. 1:1 — and it is what made the full-scale workload take
/// ~24 minutes.
///
/// 4 KB rather than the faster 16/64 is deliberate: this is a *correctness*
/// instrument, so the default errs toward sensitivity. It still collects once
/// per ~15 loop iterations, which catches a recurring window almost
/// immediately, while being 14x cheaper than unpaced. An operator who wants
/// speed raises it; one who wants a once-executed window sets `0`.
///
/// Every row keeps `copying_minors == forced_collections` and `moved > 0`, so
/// no stride silently degrades the instrument into non-moving sweeps.
const ZEAL_DEFAULT_STRIDE_BYTES: usize = 4 * 1024;

/// Pure knob parse for `PERRY_GC_ZEAL_ALLOC_KB`, in KB. `Some(0)` is a
/// deliberate, meaningful value — "collect at every poll", the pre-#7728
/// semantics — so it must not be filtered out the way a nonsense value is.
pub(crate) fn parse_zeal_alloc_kb(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .map(|kb| kb.saturating_mul(1024))
        .unwrap_or(ZEAL_DEFAULT_STRIDE_BYTES)
}

/// Bytes of new nursery material required between zeal-forced collections.
pub(crate) fn zeal_poll_stride_bytes() -> usize {
    #[cfg(test)]
    if let Some(stride) = ZEAL_STRIDE_OVERRIDE.with(std::cell::Cell::get) {
        return stride;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        parse_zeal_alloc_kb(std::env::var("PERRY_GC_ZEAL_ALLOC_KB").ok().as_deref())
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only stride override, thread-local for the same reason `ZEAL_OVERRIDE` is.
    static ZEAL_STRIDE_OVERRIDE: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// RAII test override for the pacing stride.
#[cfg(test)]
pub(crate) struct ZealStrideGuard(Option<usize>);

#[cfg(test)]
impl ZealStrideGuard {
    pub(crate) fn set(stride_bytes: usize) -> Self {
        Self(ZEAL_STRIDE_OVERRIDE.with(|cell| cell.replace(Some(stride_bytes))))
    }
}

#[cfg(test)]
impl Drop for ZealStrideGuard {
    fn drop(&mut self) {
        ZEAL_STRIDE_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

thread_local! {
    /// From-space high-water mark at or above which the next zeal-forced
    /// collection is due. Per-thread because the arena it measures is.
    ///
    /// Starts at 0 so the FIRST poll always collects: a program that allocates
    /// less than one stride in total must still exercise the instrument rather
    /// than silently becoming a run in which zeal did nothing.
    static ZEAL_NEXT_FORCE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Is a zeal-forced collection due at this poll, given current from-space bytes?
#[inline]
pub(crate) fn zeal_poll_collection_due(from_space_bytes: usize) -> bool {
    from_space_bytes >= ZEAL_NEXT_FORCE_BYTES.with(std::cell::Cell::get)
}

/// Rearm the pacing high-water mark after a zeal poll ran the safepoint.
///
/// Takes the from-space level measured *after* the collection, so the next
/// forced collection needs a full stride of genuinely new allocation on top of
/// whatever survived. See the module docs for why this is a high-water mark
/// rather than a delta.
#[inline]
pub(crate) fn note_zeal_poll_collection(from_space_bytes_after: usize) {
    let next = from_space_bytes_after.saturating_add(zeal_poll_stride_bytes());
    ZEAL_NEXT_FORCE_BYTES.with(|cell| cell.set(next));
}

/// Polls at which zeal declined to collect because the stride was not yet met.
static ZEAL_POLLS_PACED: AtomicU64 = AtomicU64::new(0);

#[inline]
pub(crate) fn note_zeal_poll_paced() {
    ZEAL_POLLS_PACED.fetch_add(1, Ordering::Relaxed);
}

/// How many back-edge polls the pacing skipped. Reported in the zeal verdict so
/// a run states its own pacing rather than leaving the operator to infer it
/// from a collection count that looks lower than it "should" be.
pub fn zeal_polls_paced() -> u64 {
    ZEAL_POLLS_PACED.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn reset_zeal_pacing_for_test() {
    ZEAL_NEXT_FORCE_BYTES.with(|cell| cell.set(0));
}

/// How many collections zeal has forced. A zeal run that reports `0` here
/// exercised nothing (most often: the binary was compiled without
/// `PERRY_GC_MOVING_LOOP_POLLS=1` and the workload never reached the event
/// loop).
pub fn zeal_forced_collections() -> u64 {
    ZEAL_FORCED.load(Ordering::Relaxed)
}

// --------------------------------------------------- instrument liveness (#7604)
//
// ★ `zeal_forced_collections()` above was, until #7604, UNREADABLE from a
// compiled program. CLAUDE.md's instrument table said "Check
// `crate::gc::zeal_forced_collections()` is nonzero" and there was no JS API,
// no diagnostic line and no exit report through which to do so. The only
// alternative — `PERRY_GC_DIAG=1` and grep — wrote **212 MB of stderr in ten
// minutes** on a 400k-iteration ratchet probe, so it is not a usable check
// either. A liveness counter nobody can read is the same thing as no liveness
// counter.
//
// Two more counters are needed alongside it, because "zeal forced a collection"
// and "a collection MOVED something" are different claims and only the second
// one is what zeal exists to produce. A forced minor can still be escalated to
// a full mark-sweep by the throughput-pacing predicates, and a full sweep moves
// nothing — which is #7604's "zero copying minors" in one sentence.
//
// Process-global rather than thread-local: the report is about the run.

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
///
/// **Exhaustive exactly under zeal**, which is the one place it is read
/// (`zeal_verdict`). A back-edge whose `PERRY_GC_POLL_ARMED` load reads zero
/// never calls into the runtime at all — that is the point of `gc/poll_arm.rs`
/// — so outside zeal this counts polls that had something to consider, not
/// back-edges executed. Zeal keeps the word armed for the life of the process
/// (`resolve_poll_seed`), so under zeal the two are the same number and the
/// "not one back-edge poll was reached" diagnosis stays sound.
pub fn loop_polls_reached() -> u64 {
    LOOP_POLLS.load(Ordering::Relaxed)
}

/// The verdict a zeal run gets at exit: what the instrument actually did.
///
/// `Ok(summary)` when zeal moved something, `Err(summary)` when the run
/// exercised nothing and every "clean under zeal" claim from it is vacuous.
/// Returns `None` when zeal is off, so the report costs a bool read.
pub fn zeal_liveness_report() -> Option<Result<String, String>> {
    if !gc_zeal_enabled() {
        return None;
    }
    Some(zeal_verdict(
        zeal_forced_collections(),
        copying_minor_cycles(),
        moved_objects_total(),
        loop_polls_reached(),
        super::policy::gc_moving_loop_polls_enabled(),
        zeal_polls_paced(),
        zeal_poll_stride_bytes(),
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
/// zeal would otherwise paper over.
pub(crate) fn zeal_verdict(
    forced: u64,
    cycles: u64,
    moved: u64,
    loop_polls: u64,
    polls_requested: bool,
    paced_polls: u64,
    stride_bytes: usize,
) -> Result<String, String> {
    let summary = format!(
        "[gc-zeal] forced_collections={forced} copying_minors={cycles} \
         moved_objects={moved} loop_polls={loop_polls} \
         paced_polls={paced_polls} stride_bytes={stride_bytes}"
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
             [gc-zeal] THIS RUN EXERCISED NOTHING WORTH TRUSTING. \
             PERRY_GC_ZEAL=1 was set and {cause}. Any \"clean under zeal\" \
             conclusion from this run is vacuous.\n\
             [gc-zeal] The usual causes: the binary was COMPILED without \
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

#[cfg(test)]
mod verdict_tests {
    use super::*;

    /// The verdict must be able to say NO. Every counter combination that means
    /// "the instrument did not fire" is asserted individually, because they have
    /// different causes and the message has to name the right one.
    #[test]
    fn a_zeal_run_that_exercised_nothing_is_an_error() {
        let no_safepoint =
            zeal_verdict(0, 0, 0, 0, false, 0, 4096).expect_err("forced=0 must be an error");
        assert!(no_safepoint.contains("no safepoint ever forced a collection"));

        // Zeal DID force collections and every one was escalated to a full
        // mark-sweep, which moves nothing. `forced > 0` alone would have called
        // this run live.
        let all_escalated =
            zeal_verdict(4096, 0, 0, 4096, true, 0, 4096).expect_err("cycles=0 must be an error");
        assert!(all_escalated.contains("escalated to a non-moving full"));
        assert!(all_escalated.contains("copying_minors=0"));
    }

    /// ★ #7604's own shape, and the one a two-counter verdict would have passed.
    ///
    /// Measured on the compute-only probe: `PERRY_GC_MOVING_LOOP_POLLS=1` set at
    /// both compile and run time, zero back-edge polls reached (codegen emits
    /// none for a provably alloc-free body), and zeal still forced 5 collections
    /// at event-loop boundaries which moved 4 objects. Every counter except
    /// `loop_polls` says "live"; no loop body was covered at all.
    #[test]
    fn polls_requested_but_never_reached_is_an_error() {
        let armed_never_fired = zeal_verdict(5, 5, 4, 0, true, 0, 4096)
            .expect_err("polls requested and none reached must be an error");
        assert!(armed_never_fired.contains("NOT ONE back-edge poll"));
        assert!(armed_never_fired.contains("loop_polls=0"));

        // ...and the SAME counters without the request are fine: an
        // event-loop-boundary zeal run is a legitimate, weaker mode, and
        // failing it would make the verdict wrong rather than strict.
        assert!(zeal_verdict(5, 5, 4, 0, false, 0, 4096).is_ok());
    }

    /// ...and YES, with the numbers, when it did fire.
    #[test]
    fn a_zeal_run_that_moved_objects_is_reported_ok() {
        let ok = zeal_verdict(741_630, 741_630, 8_899_560, 741_630, true, 0, 4096)
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
        assert!(zeal_verdict(1, 1, 0, 1, true, 0, 4096).is_ok());
    }
}
