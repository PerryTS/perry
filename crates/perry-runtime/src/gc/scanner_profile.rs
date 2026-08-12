//! Per-scanner attribution for the copied-minor root scan (#7915).
//!
//! A copying minor runs the whole registered mutable-root scanner list THREE
//! times — preflight, mark, rewrite — and the aggregate telemetry
//! (`root_sources.runtime_mutable_scanners`) sums all of them into one number.
//! That is enough to see that the cost is per-root rather than per-copied
//! object, and not enough to say WHICH registry holds the roots.
//!
//! This module records, per registered scanner and per pass, the wall time it
//! spent and the slots/pointer-roots/rewrites it accounted for. It is gated on
//! the existing `PERRY_GC_DIAG` knob (no new knob — see CLAUDE.md's GC knob
//! kill-policy) and is inert otherwise: the enable check is a cached bool and
//! every recording site is behind it.

use std::cell::RefCell;
use std::time::Instant;

#[derive(Clone, Copy, Default)]
pub(super) struct ScannerProfileRow {
    pub(super) nanos: u64,
    pub(super) calls: u32,
    pub(super) slots: u64,
    pub(super) pointer_roots: u64,
    pub(super) rewrites: u64,
}

crate::perry_thread_local! {
    static SCANNER_PROFILE: RefCell<Vec<(&'static str, ScannerProfileRow)>> =
        const { RefCell::new(Vec::new()) };
    static PROFILE_ENABLED: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[inline]
pub(super) fn scanner_profile_enabled() -> bool {
    PROFILE_ENABLED.with(|cell| {
        let cached = cell.get();
        if cached != 0 {
            return cached == 2;
        }
        let on = std::env::var_os("PERRY_GC_DIAG").is_some();
        cell.set(if on { 2 } else { 1 });
        on
    })
}

/// Time `body`, attributing its wall time and its slot deltas to `name`.
///
/// `deltas` is `(slots, pointer_roots, rewrites)` measured by the caller around
/// the same call — the trace stats live behind a raw pointer the caller already
/// owns, so reading them here would need a second borrow.
pub(super) fn record_scanner<R>(body: impl FnOnce() -> R) -> (R, u64) {
    if !scanner_profile_enabled() {
        return (body(), 0);
    }
    let start = Instant::now();
    let out = body();
    let nanos = start.elapsed().as_nanos() as u64;
    (out, nanos)
}

/// Read the three counters this module attributes, before a scanner runs.
pub(super) fn snapshot_stats(
    stats: Option<*mut super::telemetry::RootSourceSlotTraceStats>,
) -> (u64, u64, u64) {
    match stats {
        Some(stats) if scanner_profile_enabled() => unsafe {
            (
                (*stats).slots_scanned as u64,
                (*stats).pointer_roots as u64,
                (*stats).rewritten_slots as u64,
            )
        },
        _ => (0, 0, 0),
    }
}

pub(super) fn note_stats_delta(
    name: &'static str,
    nanos: u64,
    before: (u64, u64, u64),
    stats: Option<*mut super::telemetry::RootSourceSlotTraceStats>,
) {
    if !scanner_profile_enabled() {
        return;
    }
    let after = snapshot_stats(stats);
    note_scanner(
        name,
        nanos,
        after.0.saturating_sub(before.0),
        after.1.saturating_sub(before.1),
        after.2.saturating_sub(before.2),
    );
}

pub(super) fn note_scanner(
    name: &'static str,
    nanos: u64,
    slots: u64,
    pointer_roots: u64,
    rewrites: u64,
) {
    if !scanner_profile_enabled() {
        return;
    }
    SCANNER_PROFILE.with(|rows| {
        let mut rows = rows.borrow_mut();
        if let Some((_, row)) = rows.iter_mut().find(|(row_name, _)| *row_name == name) {
            row.nanos = row.nanos.saturating_add(nanos);
            row.calls = row.calls.saturating_add(1);
            row.slots = row.slots.saturating_add(slots);
            row.pointer_roots = row.pointer_roots.saturating_add(pointer_roots);
            row.rewrites = row.rewrites.saturating_add(rewrites);
            return;
        }
        rows.push((
            name,
            ScannerProfileRow {
                nanos,
                calls: 1,
                slots,
                pointer_roots,
                rewrites,
            },
        ));
    });
}

/// Print the per-scanner breakdown accumulated since the last report, then
/// clear it. Called once per copied minor from the `[gc-copy-minor]` diag site.
pub(super) fn report_and_reset(cycle_label: &str) {
    if !scanner_profile_enabled() {
        return;
    }
    let mut rows = SCANNER_PROFILE.with(|rows| std::mem::take(&mut *rows.borrow_mut()));
    if rows.is_empty() {
        return;
    }
    rows.sort_by(|a, b| b.1.nanos.cmp(&a.1.nanos));
    let total_ns: u64 = rows.iter().map(|(_, row)| row.nanos).sum();
    let total_slots: u64 = rows.iter().map(|(_, row)| row.slots).sum();
    eprintln!(
        "[gc-scanner-profile] {cycle_label} scanners={} total_us={} total_slots={}",
        rows.len(),
        total_ns / 1000,
        total_slots
    );
    for (name, row) in rows.iter().take(14) {
        if row.nanos == 0 && row.slots == 0 {
            continue;
        }
        eprintln!(
            "[gc-scanner-profile]   {name} us={} calls={} slots={} ptr_roots={} rewrites={}",
            row.nanos / 1000,
            row.calls,
            row.slots,
            row.pointer_roots,
            row.rewrites
        );
    }
}
