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
