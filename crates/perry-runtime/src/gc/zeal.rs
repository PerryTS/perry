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
//! SpiderMonkey's `gcZeal`, it forces an **evacuating** minor at every GC
//! safepoint, so an unrooted value moves on its FIRST exposure, deterministically.
//!
//! # What the knob actually gates
//!
//! `PERRY_GC_ZEAL=1`:
//!
//! 1. Every loop back-edge poll (`js_gc_loop_safepoint`) runs a minor, instead
//!    of only draining an already-deferred one (`GC_SAFEPOINT_PENDING`).
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
//! `PERRY_GC_MOVING_LOOP_POLLS=1` (default off since #7161). Zeal cannot
//! conjure a poll that codegen never emitted. A binary compiled without polls
//! still gets (2) and (3) — event-loop-boundary zeal — but a compute-only loop
//! that never yields will not collect at all. **For the #7154 hunt, compile AND
//! run with `PERRY_GC_MOVING_LOOP_POLLS=1`.** `zeal_forced_collections()`
//! reports how many collections zeal actually forced, so "clean under zeal" can
//! be checked against zeal having done anything.
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

/// RAII test override for zeal.
#[cfg(test)]
pub(crate) struct ZealGuard(Option<bool>);

#[cfg(test)]
impl ZealGuard {
    pub(crate) fn set(enabled: bool) -> Self {
        Self(ZEAL_OVERRIDE.with(|cell| cell.replace(Some(enabled))))
    }
}

#[cfg(test)]
impl Drop for ZealGuard {
    fn drop(&mut self) {
        ZEAL_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

#[inline]
pub(crate) fn note_zeal_forced_collection() {
    ZEAL_FORCED.fetch_add(1, Ordering::Relaxed);
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
    ))
}

/// The verdict as a pure function of the three counters, so the decision is
/// testable without mutating process-global state that every other test in this
/// crate shares.
pub(crate) fn zeal_verdict(forced: u64, cycles: u64, moved: u64) -> Result<String, String> {
    let summary = format!(
        "[gc-zeal] forced_collections={forced} copying_minors={cycles} moved_objects={moved}"
    );
    if forced == 0 || cycles == 0 {
        return Err(format!(
            "{summary}\n\
             [gc-zeal] THIS RUN EXERCISED NOTHING. PERRY_GC_ZEAL=1 was set and \
             {}. Any \"clean under zeal\" conclusion from this run is vacuous.\n\
             [gc-zeal] The two usual causes: the binary was compiled WITHOUT \
             PERRY_GC_MOVING_LOOP_POLLS=1 (so there are no back-edge polls to \
             force), or its hot loops are ones codegen emits no poll for -- \
             provably alloc-free bodies by design, and the specialized `for` / \
             `for-of` / `for-in` lowerings by omission (see \
             `emit_gc_loop_safepoint`'s COVERAGE note). Check with: \
             objdump -d BIN | grep -c js_gc_loop_safepoint",
            if forced == 0 {
                "no safepoint ever forced a collection"
            } else {
                "every forced collection was escalated to a non-moving full \
                 mark-sweep, so nothing was relocated"
            }
        ));
    }
    Ok(summary)
}

#[cfg(test)]
mod verdict_tests {
    use super::*;

    /// The verdict must be able to say NO. Every counter combination that means
    /// "the instrument did not fire" is asserted individually, because they have
    /// different causes and the message has to name the right one.
    #[test]
    fn a_zeal_run_that_exercised_nothing_is_an_error() {
        let no_safepoint = zeal_verdict(0, 0, 0).expect_err("forced=0 must be an error");
        assert!(no_safepoint.contains("no safepoint ever forced a collection"));

        // The #7604 shape proper: zeal DID force collections, and every one was
        // escalated to a full mark-sweep, which moves nothing. `forced > 0`
        // alone would have called this run live.
        let all_escalated = zeal_verdict(4096, 0, 0).expect_err("cycles=0 must be an error");
        assert!(all_escalated.contains("escalated to a non-moving full"));
        assert!(all_escalated.contains("copying_minors=0"));
    }

    /// ...and YES, with the numbers, when it did fire.
    #[test]
    fn a_zeal_run_that_moved_objects_is_reported_ok() {
        let ok = zeal_verdict(741_630, 741_630, 8_899_560).expect("a moving run must pass");
        assert!(ok.contains("forced_collections=741630"));
        assert!(ok.contains("copying_minors=741630"));
        assert!(ok.contains("moved_objects=8899560"));
    }

    /// A copying minor that relocated nothing THIS cycle is still a live
    /// instrument — `moved=0` with `cycles>0` happens whenever the nursery had
    /// no survivors, and failing on it would make the verdict flaky rather than
    /// informative. Pinned so a future "tighten it to moved>0" edit has to
    /// argue with a test.
    #[test]
    fn a_copying_minor_with_no_survivors_is_not_a_failure() {
        assert!(zeal_verdict(1, 1, 0).is_ok());
    }
}
