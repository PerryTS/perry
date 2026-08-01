//! Conservative-scan fallback accounting and the safepoint-deferral counters
//! that make "the fallback never fires" a *measured* statement (#7148).
//!
//! # Why this module exists
//!
//! `ManualGcScanGuard::force_full_scan()` pins this thread's conservative
//! native-stack scan on for the duration of one collection. That is sound —
//! it retains anything reachable from a register or an unspilled native local
//! — but it is *catastrophically* expensive, and the cost is not a slowdown:
//! a conservative scan makes the copying minor ineligible
//! (`CopiedMinorFallbackReason::ConservativeStack`), so a cycle that takes it
//! runs **no copying minor at all**. `benchmarks/gc_ratchet/README.md`
//! measures the end-to-end effect of scanning on the eight ratchet probes:
//! `heap_used_bytes` +364% to +5371%, and `minor_cycles` → **0 on all eight**.
//!
//! So every `force_full_scan()` that fires is a collection that reclaims by
//! sweeping instead of by evacuating, on a heap that has retained everything
//! the native stack happened to look like a pointer to.
//!
//! # The rule
//!
//! Before #7148 there were six `force_full_scan()` sites and no way to tell,
//! for any given program, whether any of them ever ran. "It never fires in
//! practice" was an assumption. Per CLAUDE.md's four-ways-a-gate-cannot-fail
//! rule — *a gate must assert its subject was live* — an unobservable fallback
//! is indistinguishable from a fallback that fires on every cycle.
//!
//! Every site therefore records itself here, and the two *deferral* paths that
//! replace a scan with a precise safepoint collection record themselves too.
//! A test or a benchmark can now assert both halves of the claim:
//!
//! * the conservative fallback did **not** fire (`scan_fallback_count`), and
//! * the precise safepoint collection that replaced it **did**
//!   (`safepoint_drain_count`) — not merely that nothing crashed.
//!
//! # Relationship to the explicit-safepoint contract
//!
//! The statepoint experiment (`exp/stackmap-viability`,
//! `docs/statepoint-gc-experiment.md`) states the invariant this instrument
//! measures compliance with: *a collection that skips the conservative stack
//! scan consumes only precise roots, so it may only begin at a declared
//! safepoint (a loop back-edge poll or the outermost microtask-pump
//! boundary); anywhere else it must scan conservatively.* That experiment
//! enforces the invariant with a thread-local declared-safepoint flag plus a
//! check at the root-scan subphase, in two levels (heal / strict panic).
//!
//! This module is the *reachability census* for the same contract on `main`:
//! it counts the sites where the contract's "anywhere else" arm is taken.
//! Driving those counts to zero is what makes the conservative scanner
//! deletable; the contract's enforcement check is what makes zero *provable*
//! rather than observed. The two compose — census first, enforcement second.

use std::cell::Cell;

/// A `force_full_scan()` callsite. Ordering matters only for the counter array.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConservativeScanSite {
    /// `gc_check_trigger` old-gen reclaim, at an allocation point, after the
    /// safepoint deferral ran out of slack. Automatic.
    OldReclaimSlackValve,
    /// `gc_check_trigger` nursery-churn direct minor, at an allocation point,
    /// after the safepoint deferral ran out of slack. Automatic.
    NurseryChurnSlackValve,
    /// `gc_try_emergency_reclaim` — a heap allocation already failed and the
    /// caller is about to panic. Automatic; cannot defer (see `mod.rs`).
    EmergencyReclaim,
    /// `manual_gc_collect_now` — explicit `gc()`. Explicit.
    ManualCollect,
    /// `js_gc_module_minor` — explicit `perry/gc` `minor()`. Explicit.
    ManualMinor,
    // ★ There is deliberately no `HostPressure` variant. `js_gc_memory_pressure`
    // used to force the scan unconditionally; after #7148 it either collects
    // with precise roots (empty shadow stack) or defers to a safepoint (a
    // generated frame is live), so it has no conservative arm left to count.
    // The site is not "unreached", it is **deleted** — which is the strongest
    // form of what #7148 asks for, and why an unconstructed variant kept here
    // "for completeness" would be the wrong kind of tidy: an enum arm nothing
    // can produce is a claim no test can check.
}

impl ConservativeScanSite {
    pub(crate) const COUNT: usize = 5;

    const fn index(self) -> usize {
        match self {
            Self::OldReclaimSlackValve => 0,
            Self::NurseryChurnSlackValve => 1,
            Self::EmergencyReclaim => 2,
            Self::ManualCollect => 3,
            Self::ManualMinor => 4,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OldReclaimSlackValve => "old_reclaim_slack_valve",
            Self::NurseryChurnSlackValve => "nursery_churn_slack_valve",
            Self::EmergencyReclaim => "emergency_reclaim",
            Self::ManualCollect => "manual_collect",
            Self::ManualMinor => "manual_minor",
        }
    }

    /// Whether this site is reached without any user code calling `gc()`.
    /// The automatic sites are the ones #7148 is about: they are the
    /// collections a program pays for without asking for them.
    pub(crate) const fn is_automatic(self) -> bool {
        match self {
            Self::OldReclaimSlackValve | Self::NurseryChurnSlackValve | Self::EmergencyReclaim => {
                true
            }
            Self::ManualCollect | Self::ManualMinor => false,
        }
    }

    #[cfg(test)]
    pub(crate) const ALL: [Self; Self::COUNT] = [
        Self::OldReclaimSlackValve,
        Self::NurseryChurnSlackValve,
        Self::EmergencyReclaim,
        Self::ManualCollect,
        Self::ManualMinor,
    ];
}

/// A precise-root collection that ran *instead of* a conservative one. These
/// are the "the precise path actually ran" counters — the live-subject
/// assertion for every gate in this family, without which "nothing scanned"
/// would also be satisfied by "nothing collected".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SafepointDrainKind {
    /// A deferred nursery-pressure trigger collected at a safepoint.
    NurseryMinor,
    /// A deferred old-gen reclaim collected at a safepoint (#7148).
    OldReclaim,
    /// A host memory-pressure request collected **synchronously, inline in
    /// `js_gc_memory_pressure`**, with precise roots because no generated frame
    /// was live (#7148). Unlike the other two kinds this is not a deferred
    /// drain through `js_gc_loop_safepoint` → `gc_safepoint_moving_minor`: the
    /// handler is already at a precise point, and deferring there would shed
    /// nothing, since a process idle enough to get an OS memory warning reaches
    /// no loop back-edge and pumps no microtasks. (When a frame *is* live the
    /// handler defers, and the drain is counted as `OldReclaim` or
    /// `NurseryMinor` by whichever trigger it armed.)
    HostPressure,
}

impl SafepointDrainKind {
    pub(crate) const COUNT: usize = 3;

    const fn index(self) -> usize {
        match self {
            Self::NurseryMinor => 0,
            Self::OldReclaim => 1,
            Self::HostPressure => 2,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NurseryMinor => "nursery_minor",
            Self::OldReclaim => "old_reclaim",
            Self::HostPressure => "host_pressure",
        }
    }
}

thread_local! {
    static SCAN_FALLBACKS: Cell<[u64; ConservativeScanSite::COUNT]> =
        const { Cell::new([0; ConservativeScanSite::COUNT]) };
    static SAFEPOINT_DRAINS: Cell<[u64; SafepointDrainKind::COUNT]> =
        const { Cell::new([0; SafepointDrainKind::COUNT]) };
}

/// Record that `site` is about to force the conservative native-stack scan.
///
/// Called from `ManualGcScanGuard::force_full_scan`, i.e. exactly once per
/// engagement, *including* engagements that turn out to be no-ops because an
/// override was already pinned — the census is of the callsite's intent, not
/// of the guard's internal bookkeeping. Under `PERRY_GC_DIAG` each occurrence
/// prints a line so an ops/benchmark run shows which sites a program reaches
/// and how often.
pub(crate) fn record_scan_fallback(site: ConservativeScanSite) {
    let count = SCAN_FALLBACKS.with(|c| {
        let mut counts = c.get();
        counts[site.index()] = counts[site.index()].saturating_add(1);
        c.set(counts);
        counts[site.index()]
    });
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        eprintln!(
            "[gc-scan-fallback] site={} automatic={} count={}",
            site.as_str(),
            site.is_automatic(),
            count
        );
    }
}

/// Record that a deferred collection drained at a precise-root safepoint —
/// the collection that a `force_full_scan()` site would otherwise have run
/// conservatively at an allocation point.
pub(crate) fn record_safepoint_drain(kind: SafepointDrainKind) {
    let count = SAFEPOINT_DRAINS.with(|c| {
        let mut counts = c.get();
        counts[kind.index()] = counts[kind.index()].saturating_add(1);
        c.set(counts);
        counts[kind.index()]
    });
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        eprintln!(
            "[gc-safepoint-drain] kind={} count={}",
            kind.as_str(),
            count
        );
    }
}

#[cfg(test)]
pub(crate) fn scan_fallback_count(site: ConservativeScanSite) -> u64 {
    SCAN_FALLBACKS.with(|c| c.get()[site.index()])
}

/// Total conservative-scan fallbacks across the four **automatic** sites. This
/// is the number #7148 wants driven to zero: explicit `gc()` is a user request
/// and is not part of the claim.
#[cfg(test)]
pub(crate) fn automatic_scan_fallback_total() -> u64 {
    SCAN_FALLBACKS.with(|c| {
        let counts = c.get();
        ConservativeScanSite::ALL
            .iter()
            .filter(|site| site.is_automatic())
            .map(|site| counts[site.index()])
            .sum()
    })
}

#[cfg(test)]
pub(crate) fn safepoint_drain_count(kind: SafepointDrainKind) -> u64 {
    SAFEPOINT_DRAINS.with(|c| c.get()[kind.index()])
}

/// Reset both counter families. Test-only: the counters are process-lifetime
/// on a real run.
#[cfg(test)]
pub(crate) fn reset_scan_fallback_counters() {
    SCAN_FALLBACKS.with(|c| c.set([0; ConservativeScanSite::COUNT]));
    SAFEPOINT_DRAINS.with(|c| c.set([0; SafepointDrainKind::COUNT]));
}
