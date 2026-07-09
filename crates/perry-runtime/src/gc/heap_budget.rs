//! Device-derived heap budget (2026-07-09 GC audit, theme T1).
//!
//! Split from `policy.rs` (repo lint caps files at 2000 lines). See the
//! banner comment below for the full design rationale.

use std::sync::OnceLock;

use super::policy::{
    GC_COPY_PROMOTION_HANDOFF_MIN_BYTES, GC_MOVING_DEFER_HARD_CAP_BYTES,
    GC_OLD_GEN_RECLAIM_GROWTH_BYTES, GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES,
    GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES, GC_TRIGGER_ABSOLUTE_CEILING,
};

// ─────────────────────────────────────────────────────────────────────────
// Device-derived heap budget (2026-07-09 GC audit, theme T1 "device-blind
// policy").
//
// Every sizing constant in this collector was tuned on 16-64 GB desktop
// machines. On a watch-class device (~30-60 MB jetsam budget) or a small
// container, the 128 MB first trigger alone exceeds the OS-imposed process
// budget — the process was jetsam/OOM-killed before the collector ever ran
// once. The budget below derives an upper bound for this process's memory
// from, in priority order:
//
//   1. `PERRY_GC_HEAP_LIMIT` — explicit deployer override, in MB.
//   2. `os_proc_available_memory()` — Apple embedded (iOS/tvOS/watchOS/
//      visionOS): bytes left before jetsam, sampled at first GC use.
//   3. cgroup `memory.max` / `memory.limit_in_bytes` — containers
//      (via the existing `js_process_constrained_memory` parser).
//   4. Half of physical RAM (`js_os_totalmem`) — an allowance, not a
//      claim on the whole machine.
//
// Budgets of ≥1 GB clamp nothing (every scaled fraction exceeds its
// desktop default), and are represented as `None` so all accessors stay on
// their historical constant path — desktop/server behavior is unchanged.
// ─────────────────────────────────────────────────────────────────────────

pub(crate) fn gc_heap_budget_bytes() -> Option<usize> {
    static CACHED: OnceLock<Option<usize>> = OnceLock::new();
    *CACHED.get_or_init(|| {
        if let Ok(v) = std::env::var("PERRY_GC_HEAP_LIMIT") {
            if let Ok(mb) = v.trim().parse::<u64>() {
                if mb > 0 {
                    return Some((mb as usize).saturating_mul(1024 * 1024));
                }
            }
        }
        let mut budget: Option<usize> = None;
        let mut consider = |candidate: f64| {
            if candidate.is_finite() && candidate >= 1024.0 * 1024.0 {
                let c = candidate as usize;
                budget = Some(budget.map_or(c, |b| b.min(c)));
            }
        };
        #[cfg(any(
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos"
        ))]
        {
            extern "C" {
                // libSystem, iOS 13+/watchOS 6+: bytes this process may
                // still allocate before hitting its jetsam limit.
                fn os_proc_available_memory() -> usize;
            }
            let avail = unsafe { os_proc_available_memory() };
            if avail > 0 {
                consider(avail as f64);
            }
        }
        consider(crate::process::js_process_constrained_memory());
        let total = crate::os::js_os_totalmem();
        if total.is_finite() && total > 0.0 {
            consider(total / 2.0);
        }
        match budget {
            Some(b) if b < 1024 * 1024 * 1024 => Some(b),
            _ => None,
        }
    })
}

/// `default.min(budget/den × num).max(floor)`; the historical default on
/// unbudgeted (desktop/server) machines.
fn budget_scaled(default: usize, num: usize, den: usize, floor: usize) -> usize {
    budget_scaled_with(gc_heap_budget_bytes(), default, num, den, floor)
}

pub(super) fn budget_scaled_with(
    budget: Option<usize>,
    default: usize,
    num: usize,
    den: usize,
    floor: usize,
) -> usize {
    match budget {
        Some(budget) => default.min((budget / den).saturating_mul(num)).max(floor),
        None => default,
    }
}

macro_rules! budget_scaled_accessor {
    ($(#[$doc:meta])* $name:ident, $default:expr, $num:expr, $den:expr, $floor:expr) => {
        $(#[$doc])*
        pub(crate) fn $name() -> usize {
            static CACHED: OnceLock<usize> = OnceLock::new();
            *CACHED.get_or_init(|| budget_scaled($default, $num, $den, $floor))
        }
    };
}

budget_scaled_accessor!(
    /// First-GC / adaptive-trigger ceiling: a quarter of the device budget,
    /// capped at the historical 128 MB.
    gc_trigger_absolute_ceiling_bytes,
    GC_TRIGGER_ABSOLUTE_CEILING,
    1,
    4,
    2 * 1024 * 1024
);
budget_scaled_accessor!(
    /// Post-collection headroom floor (historically 16 MB) — scales down
    /// with the trigger so a small-budget device doesn't get 16 MB of
    /// headroom on an 8 MB trigger.
    gc_trigger_headroom_floor_bytes,
    16 * 1024 * 1024,
    1,
    32,
    1024 * 1024
);
budget_scaled_accessor!(
    gc_old_gen_reclaim_threshold_dyn_bytes,
    GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    1,
    8,
    4 * 1024 * 1024
);
budget_scaled_accessor!(
    gc_old_gen_reclaim_growth_dyn_bytes,
    GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
    1,
    12,
    2 * 1024 * 1024
);
budget_scaled_accessor!(
    gc_copy_promotion_handoff_min_dyn_bytes,
    GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
    1,
    16,
    2 * 1024 * 1024
);
budget_scaled_accessor!(
    gc_moving_defer_hard_cap_dyn_bytes,
    GC_MOVING_DEFER_HARD_CAP_BYTES,
    1,
    4,
    2 * 1024 * 1024
);
budget_scaled_accessor!(
    gc_tiny_parse_in_use_trigger_dyn_bytes,
    GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES,
    1,
    8,
    2 * 1024 * 1024
);
budget_scaled_accessor!(
    gc_tiny_parse_full_gc_in_use_trigger_dyn_bytes,
    GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES,
    1,
    16,
    1024 * 1024
);

/// RSS evacuation-pressure thresholds (historically 192/256 MB — above the
/// entire process budget of every small device, so the pressure arms never
/// fired exactly where they matter most).
pub(crate) fn gc_rss_pressure_dyn_bytes() -> u64 {
    static CACHED: OnceLock<u64> = OnceLock::new();
    *CACHED.get_or_init(|| {
        budget_scaled(
            super::oldgen::RSS_PRESSURE_BYTES as usize,
            1,
            2,
            16 * 1024 * 1024,
        ) as u64
    })
}

pub(crate) fn gc_rss_hard_pressure_dyn_bytes() -> u64 {
    static CACHED: OnceLock<u64> = OnceLock::new();
    *CACHED.get_or_init(|| {
        budget_scaled(
            super::oldgen::RSS_HARD_PRESSURE_BYTES as usize,
            2,
            3,
            24 * 1024 * 1024,
        ) as u64
    })
}
