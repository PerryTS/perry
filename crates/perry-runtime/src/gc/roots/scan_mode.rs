//! Conservative native-stack scan mode: how a collection decides whether to
//! walk this thread's native (Rust) stack looking for anything that might be a
//! heap pointer.
//!
//! Split out of `roots.rs` in #7148 — that file crossed the 2,000-line gate,
//! and this is the topically self-contained piece: the mode enum, its env/
//! override precedence, the `ManualGcScanGuard` that pins `Full` for one
//! collection, and the decision the mark phase consults. Nothing else in
//! `roots.rs` reads its internals.
//!
//! ★ The decision this module returns is the single most expensive bit in the
//! collector. `Scan` makes the copying minor ineligible
//! (`CopiedMinorFallbackReason::ConservativeStack`), so a cycle that takes it
//! runs no copying minor at all — measured at `heap_used_bytes` +364%..+5371%
//! and `minor_cycles` → 0 on all eight `gc_ratchet` probes. See
//! `super::super::scan_fallback` for the census of who asks for it.

use super::super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConservativeStackScanMode {
    Auto,
    Disabled,
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub(crate) enum ConservativeStackScanDecision {
    Scan,
    #[default]
    SkipDisabled,
}

impl ConservativeStackScanDecision {
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::SkipDisabled => "skip_disabled",
        }
    }
}

pub(crate) fn conservative_stack_scan_mode_from_value(
    value: Option<&str>,
) -> ConservativeStackScanMode {
    let Some(value) = value else {
        return ConservativeStackScanMode::Auto;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => ConservativeStackScanMode::Auto,
        "0" | "off" | "false" => ConservativeStackScanMode::Disabled,
        "1" | "on" | "true" | "full" | "debug" => ConservativeStackScanMode::Full,
        _ => ConservativeStackScanMode::Auto,
    }
}

thread_local! {
    /// Per-thread override for the conservative-scan mode, taking precedence over
    /// the `#[cfg(test)]` default but not an explicit env var. The GC unit tests
    /// set this to `Auto` (skip) inside their controlled-root scopes so a forced
    /// collection still reclaims the objects they hold only as native-stack
    /// locals; see `ScopedRootScannerRegistryGuard`.
    pub(crate) static CONSERVATIVE_STACK_SCAN_OVERRIDE: std::cell::Cell<Option<ConservativeStackScanMode>> =
        const { std::cell::Cell::new(None) };
}

/// Set (or clear) this thread's conservative-scan mode override, returning the
/// previous value.
#[allow(dead_code)] // test-only scaffolding: GC unit tests (gc/tests) pin the scan mode via this override
pub(crate) fn set_conservative_stack_scan_override(
    mode: Option<ConservativeStackScanMode>,
) -> Option<ConservativeStackScanMode> {
    CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| c.replace(mode))
}

/// Scoped guard forcing the conservative native-stack scan for an explicit
/// `gc()` collection (#4977). In the default `Auto` mode a full collection
/// skips the native scan, but at a `gc()` callsite live module-init/top-level
/// locals may be held only on the native stack — neither the precise
/// shadow-stack roots nor the module-var scanners cover them — so the
/// collector reclaimed live object graphs and later field reads returned
/// dangling-pointer garbage. An already-pinned per-thread override wins (the
/// GC unit tests pin `Auto` so a forced collection still reclaims objects they
/// hold only as native-stack locals), and an explicit
/// `PERRY_CONSERVATIVE_STACK_SCAN` env value beats any override either way,
/// so the bisection escape hatch keeps working.
///
/// ★ #7148: engaging this guard is *expensive*, not merely imprecise — the
/// conservative scan makes the copying minor ineligible
/// (`CopiedMinorFallbackReason::ConservativeStack`), so the cycle it covers
/// runs no copying minor at all (`benchmarks/gc_ratchet/README.md`:
/// `heap_used_bytes` +364%..+5371%, `minor_cycles` → 0 on all eight probes).
/// Every callsite therefore declares which of the six sites it is and is
/// counted in `super::scan_fallback`, so "this never fires in practice" is a
/// measurement instead of an assumption.
pub(crate) struct ManualGcScanGuard {
    engaged: bool,
}

impl ManualGcScanGuard {
    pub(crate) fn force_full_scan(site: super::ConservativeScanSite) -> Self {
        super::record_scan_fallback(site);
        let engaged = CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| {
            if c.get().is_some() {
                return false;
            }
            c.set(Some(ConservativeStackScanMode::Full));
            true
        });
        Self { engaged }
    }
}

impl Drop for ManualGcScanGuard {
    fn drop(&mut self) {
        if self.engaged {
            CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| c.set(None));
        }
    }
}

pub(crate) fn conservative_stack_scan_mode() -> ConservativeStackScanMode {
    // Explicit env var wins (so the dedicated mode tests and ops bisection keep
    // working), then a per-thread override, then the build-time default.
    let env_mode = std::env::var("PERRY_CONSERVATIVE_STACK_SCAN")
        .ok()
        .map(|value| conservative_stack_scan_mode_from_value(Some(&value)));
    let pinned = CONSERVATIVE_STACK_SCAN_OVERRIDE.with(|c| c.get());

    // ★ #7148: in the TEST build a pinned override beats an env request for
    // `Full`. The env var may make the scan *less* aggressive than the mode a
    // test declared, never more.
    //
    // Why: `PERRY_CONSERVATIVE_STACK_SCAN=full` failed **134 of 1574**
    // perry-runtime tests — a shipped escape hatch in a state nobody had
    // verified. The failures were not soundness failures. A collector test's
    // central assertion is *"this object should have been collected"*, and a
    // conservative scan retains whatever the native stack happens to look like
    // a pointer to, so an ambient `=full` broke exactly the assertions the
    // suite exists to make. Since #7147 the test build has ONE declared scan
    // mode (`Auto`) and the isolation guards are its authority; letting an
    // ambient env var silently re-impose `Full` on a guarded scope reintroduces
    // the test-build-is-a-different-GC-configuration problem #7147 removed.
    //
    // Kept rather than deleted (CLAUDE.md's kill-policy default) because
    // `=full` is load-bearing *outside* the test build: it is the ratchet's
    // validated sensitivity arm — the run that produced the 60 regression rows
    // proving `gc-ratchet` can actually fail (`benchmarks/gc_ratchet/README.md`).
    // Deleting the mode would delete a gate's only end-to-end proof. This makes
    // it verified instead: the suite is green under `=full`.
    #[cfg(test)]
    if matches!(env_mode, Some(ConservativeStackScanMode::Full)) {
        if let Some(pinned) = pinned {
            return pinned;
        }
    }

    if let Some(mode) = env_mode {
        return mode;
    }
    if let Some(mode) = pinned {
        return mode;
    }
    // ONE default for every build, tests included.
    //
    // The test build used to default to `Full` on the grounds that unit tests
    // root GC-managed pointers as raw locals on the *native* (Rust) stack
    // rather than the shadow stack, so the conservative scan was what kept them
    // alive. That is a description of how some tests were written, not a
    // requirement of testing, and it had a cost that outweighed the
    // convenience: it made the test build a DIFFERENT GC configuration from
    // production. A missing precise root — the #7055 shape — was rescued by the
    // native scan under test and was a live wrong answer in production, so the
    // suite filtered out precisely the failure class it exists to catch. It
    // also made the copying minor ineligible under test
    // (`CopiedMinorFallbackReason::ConservativeStack`), so relocation was
    // largely unexercised.
    //
    // The correct mechanism already existed and the tests already used it:
    // `RuntimeHandleScope` / `gc_temp_root_*`, plus the isolation guards, which
    // pin `Auto` themselves — 351 of the 531 gc tests were already running this
    // way. Flipping the default cost exactly one test conversion
    // (`test_minor_preserves_old_to_young_edge_across_minors`, which read a
    // relocated child through a stale pre-collection local).
    //
    // A test that needs the scan *provably* off — not merely resolved off by
    // today's `Auto` policy — should pin `ConservativeScanDisabledGuard`.
    ConservativeStackScanMode::Auto
}

#[inline]
pub(crate) fn conservative_stack_scan_decision_for(
    mode: ConservativeStackScanMode,
    _shadow_frame_active: bool,
) -> ConservativeStackScanDecision {
    match mode {
        ConservativeStackScanMode::Disabled => ConservativeStackScanDecision::SkipDisabled,
        ConservativeStackScanMode::Full => ConservativeStackScanDecision::Scan,
        ConservativeStackScanMode::Auto => ConservativeStackScanDecision::SkipDisabled,
    }
}

pub(crate) fn conservative_stack_scan_decision() -> ConservativeStackScanDecision {
    conservative_stack_scan_decision_for(
        conservative_stack_scan_mode(),
        shadow_stack_has_active_frame(),
    )
}
