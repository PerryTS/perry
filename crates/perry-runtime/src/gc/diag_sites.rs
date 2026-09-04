//! `PERRY_GC_DIAG=1`: WHY a collection was decided, WHICH arm ran a full
//! mark-sweep, and WHAT the budgeted collector charged the mutator per cycle
//! and per charge site.
//!
//! The per-cycle diag lines (`[gc-copy-minor]`, `[gc-step]`, `[gc]`) report
//! what a collection did; none of them says why it was scheduled. On the
//! compiled claude-code TUI a 400-character streamed reply cost 42 copying
//! minors — most of them over a nearly empty Eden — plus ten back-to-back
//! synchronous full mark-sweeps from the allocation-point old-reclaim arm,
//! and the only way to tell which predicate fired was to re-derive every input
//! by hand. These lines print the inputs at the decision:
//!
//! * `[gc-trigger] site=… kind=…` — every predicate input the trigger policy
//!   reads (`arena_total` vs the armed base trigger, from-space occupancy vs
//!   the nursery cap, old-gen reclaimable pressure vs its baseline and band,
//!   the malloc-count pair, the pending/retaining flags), emitted at each
//!   site that decides to collect.
//! * `[gc-full] site=… trigger=…` — one line per full mark-sweep, naming the
//!   arm that started it (`alloc_point_old_reclaim`, `safepoint_old_reclaim`,
//!   `budgeted`, `manual`, …) with a running per-site count.
//! * `[gc-budgeted] start|done …` — one pair per budgeted (incremental) cycle:
//!   trigger, full/minor, how many steps drove it, the wall time of those
//!   steps split by cycle phase, and the root-scan share of the total.
//! * `[gc-charge] …` — the mutator-assist and synchronous-full work charged
//!   to each calling site (return-address chain, resolved to the JS display
//!   name where the frame is compiled user code), so "which JS operation is
//!   paying for the collector" is a counter rather than a profile guess.
//!
//! Everything here is gated on [`gc_diag_enabled`] and costs one cached-bool
//! read when off.

use super::*;
use std::collections::HashMap;
use std::time::Instant;

/// Print the predicate inputs behind a collection decision.
pub(super) fn trigger_decision(site: &'static str, kind: &'static str) {
    if !gc_diag_enabled() {
        return;
    }
    let arena_total = crate::arena::arena_total_bytes();
    let next_base = policy::next_arena_trigger_base();
    let armed = policy::GC_TRIGGER_ARMED.with(Cell::get);
    let from_space = crate::arena::copying_from_space_in_use_bytes();
    let nursery_cap = tenuring::scavenge_nursery_cap_effective_bytes();
    let old_reclaimable = policy::old_gen_reclaimable_pressure_bytes();
    let external = policy::external_side_live_bytes();
    let old_baseline = policy::GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(Cell::get);
    let old_band = policy::gc_old_reclaim_growth_band_bytes(old_baseline);
    let old_threshold = gc_old_gen_reclaim_threshold_dyn_bytes();
    let old_pending = policy::GC_OLD_RECLAIM_PENDING.with(Cell::get);
    let retaining = policy::GC_MAJOR_PACING_RETAINING.with(Cell::get);
    let malloc = malloc_object_count();
    let next_malloc = policy::GC_NEXT_MALLOC_TRIGGER.with(Cell::get);
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    let old_free = old_free_bytes();
    eprintln!(
        "[gc-trigger] site={site} kind={kind} arena_total={arena_total} next_base={next_base} armed={armed} \
         from_space={from_space} nursery_cap={nursery_cap} old_in_use={old_in_use} old_free={old_free} \
         old_reclaimable={old_reclaimable} external_side={external} old_baseline={old_baseline} \
         old_band={old_band} old_threshold={old_threshold} old_pending={old_pending} retaining={retaining} \
         malloc={malloc} next_malloc={next_malloc}"
    );
}

crate::perry_thread_local! {
    /// Label the arm that is about to run a synchronous full leaves for the
    /// chokepoint (`gc_collect_full_mark_sweep_with_trigger`) to consume.
    static FULL_SITE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Name the arm behind the next synchronous full mark-sweep.
pub(super) fn set_full_site(site: &'static str) {
    FULL_SITE.with(|s| s.set(Some(site)));
}

/// Consume the pending arm label; `sync` when none was set (manual `gc()`,
/// emergency, escalation).
pub(super) fn take_full_site() -> &'static str {
    FULL_SITE.with(|s| s.take()).unwrap_or("sync")
}

crate::perry_thread_local! {
    static FULL_SITE_COUNTS: RefCell<Vec<(&'static str, u32)>> = const { RefCell::new(Vec::new()) };
    static BUDGETED: RefCell<Option<BudgetedCycleDiag>> = const { RefCell::new(None) };
    static CHARGES: RefCell<HashMap<[usize; CHARGE_DEPTH], Charge>> = RefCell::new(HashMap::new());
}

/// Test-only: how many synchronous fulls `full_started` counted at `site`.
#[cfg(test)]
pub(super) fn test_full_site_count(site: &str) -> u32 {
    FULL_SITE_COUNTS.with(|c| {
        c.borrow()
            .iter()
            .find(|(s, _)| *s == site)
            .map_or(0, |(_, n)| *n)
    })
}

/// Test-only: `(calls, units, us, fulls, minors)` of every charge row.
#[cfg(test)]
pub(super) fn test_charge_rows() -> Vec<(u64, u64, u64, u64, u64)> {
    CHARGES.with(|c| {
        c.borrow()
            .values()
            .map(|r| (r.calls, r.units, r.us, r.fulls, r.minors))
            .collect()
    })
}

/// Test-only: `(steps, step_us, units, root_scan_us)` of the last completed
/// budgeted cycle's accounting.
#[cfg(test)]
pub(super) fn test_last_budgeted() -> Option<(u64, u64, u64, u64)> {
    LAST_BUDGETED.with(Cell::get)
}

#[cfg(test)]
crate::perry_thread_local! {
    static LAST_BUDGETED: Cell<Option<(u64, u64, u64, u64)>> = const { Cell::new(None) };
}

/// One full mark-sweep is starting from `site`.
pub(super) fn full_started(site: &'static str, trigger: GcTriggerKind) {
    if !gc_diag_enabled() {
        return;
    }
    let count = FULL_SITE_COUNTS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(entry) = c.iter_mut().find(|(s, _)| *s == site) {
            entry.1 += 1;
            entry.1
        } else {
            c.push((site, 1));
            1
        }
    });
    eprintln!(
        "[gc-full] site={site} trigger={trigger:?} count_at_site={count} old_reclaimable={} old_baseline={}",
        policy::old_gen_reclaimable_pressure_bytes(),
        policy::GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(Cell::get)
    );
}

/// `GcCyclePhase::ffi_code()` runs 1..=8; index by it directly.
const PHASE_SLOTS: usize = 9;

struct BudgetedCycleDiag {
    trigger: GcTriggerKind,
    collection: &'static str,
    progress: &'static str,
    started: Instant,
    steps: u64,
    step_us: u64,
    units: u64,
    phase_us: [u64; PHASE_SLOTS],
    phase_steps: [u64; PHASE_SLOTS],
}

/// A budgeted cycle was just installed as the active cycle.
pub(super) fn budgeted_started(
    trigger: GcTriggerKind,
    collection: GcCollectionKind,
    progress: GcProgressKind,
) {
    if !gc_diag_enabled() {
        return;
    }
    let collection = match collection {
        GcCollectionKind::Full => "full",
        GcCollectionKind::Minor => "minor",
    };
    eprintln!(
        "[gc-budgeted] start trigger={trigger:?} kind={collection} progress={} old_reclaimable={} old_baseline={} arena_total={}",
        progress.as_str(),
        policy::old_gen_reclaimable_pressure_bytes(),
        policy::GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(Cell::get),
        crate::arena::arena_total_bytes()
    );
    BUDGETED.with(|b| {
        *b.borrow_mut() = Some(BudgetedCycleDiag {
            trigger,
            collection,
            progress: progress.as_str(),
            started: Instant::now(),
            steps: 0,
            step_us: 0,
            units: 0,
            phase_us: [0; PHASE_SLOTS],
            phase_steps: [0; PHASE_SLOTS],
        });
    });
}

/// One budgeted step ran: `phase_before` is the phase it started in.
pub(super) fn budgeted_step_done(phase_code: u32, elapsed_us: u64, units: usize) {
    if !gc_diag_enabled() {
        return;
    }
    BUDGETED.with(|b| {
        if let Some(d) = b.borrow_mut().as_mut() {
            d.steps += 1;
            d.step_us += elapsed_us;
            d.units = d.units.saturating_add(units as u64);
            let slot = (phase_code as usize).min(PHASE_SLOTS - 1);
            d.phase_us[slot] += elapsed_us;
            d.phase_steps[slot] += 1;
        }
    });
}

/// The active budgeted cycle completed and was rebaselined.
pub(super) fn budgeted_completed(freed_bytes: u64) {
    if !gc_diag_enabled() {
        return;
    }
    let Some(d) = BUDGETED.with(|b| b.borrow_mut().take()) else {
        return;
    };
    #[cfg(test)]
    LAST_BUDGETED.with(|c| c.set(Some((d.steps, d.step_us, d.units, d.phase_us[2]))));
    const NAMES: [&str; PHASE_SLOTS] = [
        "?",
        "build_valid_ptrs",
        "root_scan",
        "mark",
        "block_persist",
        "atomic_finalize",
        "sweep",
        "reclaim",
        "complete",
    ];
    let mut phases = String::new();
    for (name, (us, steps)) in NAMES
        .iter()
        .zip(d.phase_us.iter().zip(d.phase_steps.iter()))
        .skip(1)
    {
        if *steps == 0 {
            continue;
        }
        phases.push_str(&format!(" {name}={us}us/{steps}steps"));
    }
    let root_share = (d.phase_us[2] * 1000).checked_div(d.step_us).unwrap_or(0);
    eprintln!(
        "[gc-budgeted] done trigger={:?} kind={} progress={} steps={} step_us={} units={} wall_us={} freed={} root_scan_permille={} phases:{}",
        d.trigger,
        d.collection,
        d.progress,
        d.steps,
        d.step_us,
        d.units,
        d.started.elapsed().as_micros(),
        freed_bytes,
        root_share,
        phases
    );
    report_charges("budgeted-done");
}

/// Return-address chain depth kept per charge site. Frame 0 is the caller of
/// `gc_check_trigger` (the allocator or `js_json_parse`); the next ones walk
/// out to the compiled JS function that issued the allocation.
const CHARGE_DEPTH: usize = 6;

#[derive(Default, Clone, Copy)]
struct Charge {
    calls: u64,
    units: u64,
    us: u64,
    minors: u64,
    fulls: u64,
}

/// Wraps one `gc_check_trigger` arm: captures the caller chain on `begin`
/// (only under the diag), and on `end` charges the elapsed time and the work
/// units it drove to that chain.
pub(super) struct ChargeProbe {
    pcs: [usize; crate::error::MAX_CAPTURED_FRAMES],
    n: usize,
    started: Option<Instant>,
}

impl ChargeProbe {
    #[inline]
    pub(super) fn begin() -> Self {
        let mut probe = Self {
            pcs: [0; crate::error::MAX_CAPTURED_FRAMES],
            n: 0,
            started: None,
        };
        if gc_diag_enabled() {
            probe.n = crate::error::capture_ips(&mut probe.pcs);
            probe.started = Some(Instant::now());
        }
        probe
    }

    /// `kind`: what the arm did — `assist` (budgeted step), `sync_full`,
    /// `direct_minor`.
    pub(super) fn end(self, units: usize, kind: ChargeKind) {
        let Some(started) = self.started else {
            return;
        };
        let us = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        let mut key = [0usize; CHARGE_DEPTH];
        // Skip frame 0: it is the return into `gc_check_trigger` itself.
        let chain = &self.pcs[1.min(self.n)..self.n];
        for (slot, pc) in key.iter_mut().zip(chain) {
            *slot = *pc;
        }
        CHARGES.with(|c| {
            let mut c = c.borrow_mut();
            let e = c.entry(key).or_default();
            e.calls += 1;
            e.units = e.units.saturating_add(units as u64);
            e.us += us;
            match kind {
                ChargeKind::Assist => {}
                ChargeKind::SyncFull => e.fulls += 1,
                ChargeKind::DirectMinor => e.minors += 1,
            }
        });
    }
}

#[derive(Clone, Copy)]
pub(super) enum ChargeKind {
    Assist,
    SyncFull,
    DirectMinor,
}

/// Print the heaviest charge sites since the last report, then reset.
pub(super) fn report_charges(label: &str) {
    if !gc_diag_enabled() {
        return;
    }
    let rows: Vec<([usize; CHARGE_DEPTH], Charge)> =
        CHARGES.with(|c| c.borrow_mut().drain().collect());
    if rows.is_empty() {
        return;
    }
    let total_us: u64 = rows.iter().map(|(_, r)| r.us).sum();
    let total_calls: u64 = rows.iter().map(|(_, r)| r.calls).sum();
    let mut rows = rows;
    rows.sort_by_key(|(_, r)| std::cmp::Reverse(r.us));
    eprintln!(
        "[gc-charge] {label}: sites={} calls={total_calls} total_us={total_us}",
        rows.len()
    );
    for (key, r) in rows.iter().take(12) {
        let n = key.iter().position(|&p| p == 0).unwrap_or(CHARGE_DEPTH);
        eprintln!(
            "[gc-charge]   us={} calls={} units={} fulls={} minors={} site={}",
            r.us,
            r.calls,
            r.units,
            r.fulls,
            r.minors,
            crate::error::describe_chain(&key[..n], 5)
        );
    }
}
