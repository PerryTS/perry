//! The allocation-point invariant (RFC deferred collection, step S5; D1/D2 in
//! `docs/src/internals/rfc-deferred-collection.md`).
//!
//! > **D2.** An allocation may take a new block, arm the poll word, run
//! > heap-only budgeted work (mark propagation, weak processing, sweep,
//! > reclaim), or run a conservative non-moving collection in exactly two
//! > cases: the valve fires, or the OldReclaim arm becomes due. It never starts
//! > a phase that reads frame roots *precisely*, and it never starts a moving
//! > phase.
//!
//! "Allocation point" is not a guess about the caller. It is exactly the
//! dynamic extent of `gc_check_trigger`'s evaluation, which every allocation
//! slow path funnels into (the arena block-full path, every `gc_malloc`, the
//! explicit JSON mid-parse checks, and the root-lock flush of a deferred
//! `CheckTrigger`). [`AllocationPointGuard`] marks that extent; everything the
//! collector does inside it is held to D2:
//!
//! * a synchronous collection started there must have the conservative scan
//!   forced ([`assert_d2_synchronous_collection`]) — which also makes the
//!   copying minor ineligible, so it cannot move;
//! * a budgeted cycle whose next work is a frame-root phase (`RootScan`,
//!   `FinalRootRemark`) is PARKED instead of stepped: the step returns, and the
//!   poll word is armed so the next declared poll serves the phase with a
//!   precise root set (`policy::gc_safepoint_moving_minor`).
//!
//! The one exception is the parked-cycle valve ([`parked_valve_due`]): a
//! program that allocates [`super::GC_MOVING_DEFER_SLACK_BYTES`] past the
//! point a cycle parked, without reaching a single poll, has its root phase
//! served at the allocation point. It is counted, it is a hard CI failure on
//! the gap suite and the ratchet probes (decision 5), and its soundness rests
//! on today's codegen treating every allocating call as a statepoint — see the
//! note on [`note_parked_valve_fired`].
//!
//! Also here, because they share the "what did allocation do" question:
//! the valve ledger (`PERRY_GC_VALVE_LEDGER`, decision 5) and the bytes the
//! arena grew by inside `GC_UNSAFE_ZONES` (decision 10, diagnostic only).

use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    /// Depth of `gc_check_trigger` evaluations on this thread. Nesting is
    /// possible (a root-lock flush inside an evaluation), so a counter rather
    /// than a flag.
    static ALLOC_POINT_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// `Some(arena_total)` while a budgeted cycle is parked at a frame-root
    /// phase, recording the arena size when it parked (the valve measures its
    /// slack from here, the way the nursery deferral measures from
    /// `GC_SAFEPOINT_DEFER_ARENA_BASE`).
    static PARKED_AT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// RAII marker for the dynamic extent of one allocation-point trigger
/// evaluation.
pub(super) struct AllocationPointGuard(());

impl AllocationPointGuard {
    #[inline]
    pub(super) fn enter() -> Self {
        ALLOC_POINT_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self(())
    }
}

impl Drop for AllocationPointGuard {
    #[inline]
    fn drop(&mut self) {
        ALLOC_POINT_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Whether the current thread is inside an allocation-point trigger
/// evaluation.
#[inline]
pub(super) fn at_allocation_point() -> bool {
    ALLOC_POINT_DEPTH.with(|depth| depth.get() != 0)
}

/// Run `f` with the allocation-point marker lifted. For the parked-cycle
/// valve only: it is the one path that deliberately serves a root phase from
/// an allocation point, and it is counted.
pub(super) fn with_allocation_point_lifted<R>(f: impl FnOnce() -> R) -> R {
    let saved = ALLOC_POINT_DEPTH.with(|depth| depth.replace(0));
    let result = f();
    ALLOC_POINT_DEPTH.with(|depth| depth.set(saved));
    result
}

/// D2's enforcement for synchronous collections: a collection that begins at
/// an allocation point must be one of the conservative arms, i.e. its caller
/// must have requested the conservative scan (`ManualGcScanGuard::force_full_scan`,
/// which also makes the copying minor ineligible).
///
/// Called at the two synchronous chokepoints (`gc_collect_minor_with_trigger_inner`
/// and `gc_collect_full_mark_sweep_with_trigger`). Every allocation-point arm
/// that collects — OldReclaim, the nursery valve, the polls-off direct minor,
/// the emergency reclaim — takes that guard first, so this is structurally
/// unreachable. It checks the REQUEST, not the resulting scan decision: the
/// unit-test isolation guards and the `PERRY_CONSERVATIVE_STACK_SCAN=off`
/// bisection escape hatch both override the decision on purpose, and neither is
/// a new path into a precise collection. It panics in every build rather than
/// healing: a heal path nothing can reach is an untested mode (the kill
/// policy), and the check is one thread-local read per collection.
#[inline]
pub(super) fn assert_d2_synchronous_collection() {
    if !at_allocation_point() {
        return;
    }
    if super::roots::conservative_scan_requested() {
        return;
    }
    D2_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    panic!(
        "perry GC invariant D2 violated: a precise-root collection began at an \
         allocation point. Allocation may only arm the poll, run heap-only \
         work, or run a conservative non-moving collection (the valve, \
         OldReclaim, emergency reclaim). See docs/src/internals/rfc-deferred-collection.md."
    );
}

static D2_VIOLATIONS: AtomicU64 = AtomicU64::new(0);
static ROOT_PHASES_PARKED: AtomicU64 = AtomicU64::new(0);
static ROOT_PHASES_SERVED_AT_POLL: AtomicU64 = AtomicU64::new(0);
static PARKED_VALVE_FIRES: AtomicU64 = AtomicU64::new(0);
static OWED_REQUESTS_ROUTED: AtomicU64 = AtomicU64::new(0);
static OWED_REQUESTS_SERVED: AtomicU64 = AtomicU64::new(0);
static UNSAFE_ZONE_GROWTH_BYTES: AtomicU64 = AtomicU64::new(0);
static UNSAFE_ZONE_GROWTH_EVENTS: AtomicU64 = AtomicU64::new(0);
static MAX_POLL_WAIT_BYTES: AtomicU64 = AtomicU64::new(0);

/// A deferred collection was drained (at a poll) or given up on (by the
/// valve) `waited` arena bytes after it was armed. The maximum is the measured
/// answer to decision 3's question — how far does a program allocate between
/// arming a collection and reaching a poll — and it is what a straight-line
/// body that needs statement-boundary polls would show.
#[inline]
pub(super) fn note_poll_wait(waited: usize) {
    MAX_POLL_WAIT_BYTES.fetch_max(waited as u64, Ordering::Relaxed);
}

/// An allocation point found the active budgeted cycle about to read frame
/// roots. Arms the poll (via `arm`) the first time for this park and records
/// where it parked. Returns whether this call started a new park.
pub(super) fn park_root_phase(arena_total: usize, arm: impl FnOnce()) -> bool {
    PARKED_AT.with(|parked| {
        if parked.get().is_some() {
            return false;
        }
        parked.set(Some(arena_total));
        ROOT_PHASES_PARKED.fetch_add(1, Ordering::Relaxed);
        arm();
        true
    })
}

/// The cycle is no longer waiting at a frame-root phase (it was served, or it
/// ended). Idempotent.
#[inline]
pub(super) fn clear_park() {
    PARKED_AT.with(|parked| parked.set(None));
}

/// Whether a cycle is currently parked at a frame-root phase on this thread.
#[cfg(test)]
pub(super) fn root_phase_parked() -> bool {
    PARKED_AT.with(|parked| parked.get().is_some())
}

/// The parked cycle has waited `slack` arena bytes past its park point with no
/// poll to serve it.
pub(super) fn parked_valve_due(arena_total: usize, slack: usize) -> bool {
    #[cfg(test)]
    let slack = TEST_SLACK.with(Cell::get).unwrap_or(slack);
    PARKED_AT.with(|parked| {
        parked
            .get()
            .is_some_and(|base| arena_total >= base.saturating_add(slack))
    })
}

/// A declared poll served a parked root phase.
pub(super) fn note_root_phase_served_at_poll() {
    ROOT_PHASES_SERVED_AT_POLL.fetch_add(1, Ordering::Relaxed);
}

/// The parked-cycle valve fired: a budgeted cycle's frame-root phase ran at an
/// allocation point because no poll was reached within the slack.
///
/// ★ This is the one place S5 still reads frame roots precisely at an
/// allocation point, and it is sound for the same reason A-assist was sound
/// before S5: codegen still treats every allocating call as a statepoint, so
/// the frame that allocated is mapped. That stops being true at S6 (L2b makes
/// `AllocOnly` helpers leaves). Before S6 this arm must either be proven
/// unreachable — the gate in `scripts/gc_valve_ledger_check.py` holds it at
/// zero on the gap suite and the ratchet probes — or be replaced by an abort of
/// the parked cycle followed by the conservative valve. Budgeted cycles are
/// classifier-mode and cannot take the conservative scan themselves.
pub(super) fn note_parked_valve_fired() {
    PARKED_VALVE_FIRES.fetch_add(1, Ordering::Relaxed);
    if super::gc_diag_enabled() {
        eprintln!(
            "[gc-alloc-point] parked_valve fired count={}",
            PARKED_VALVE_FIRES.load(Ordering::Relaxed)
        );
    }
}

/// A root-lock exit had a collection (not just a trigger check) deferred to
/// it, and routed it to the poll instead of running it (the RFC's "D stops
/// collecting").
pub(super) fn note_owed_request_routed() {
    OWED_REQUESTS_ROUTED.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn note_owed_request_served() {
    OWED_REQUESTS_SERVED.fetch_add(1, Ordering::Relaxed);
}

/// Decision 10: an arena block was taken while `GC_UNSAFE_ZONES` was held, so
/// nothing — no poll, no valve — could collect. Diagnostic only; the arena's
/// block-acquire slow path is the only caller.
#[inline]
pub(crate) fn note_block_if_unsafe_zone(bytes: usize) {
    if !super::gc_blocked_by_unsafe_zone() {
        return;
    }
    UNSAFE_ZONE_GROWTH_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    UNSAFE_ZONE_GROWTH_EVENTS.fetch_add(1, Ordering::Relaxed);
}

/// Snapshot of this module's counters, for the exit summary and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocPointCounters {
    pub d2_violations: u64,
    pub root_phases_parked: u64,
    pub root_phases_served_at_poll: u64,
    pub parked_valve_fires: u64,
    pub owed_requests_routed: u64,
    pub owed_requests_served: u64,
    pub unsafe_zone_growth_bytes: u64,
    pub unsafe_zone_growth_events: u64,
    pub max_poll_wait_bytes: u64,
}

pub fn alloc_point_counters() -> AllocPointCounters {
    AllocPointCounters {
        d2_violations: D2_VIOLATIONS.load(Ordering::Relaxed),
        root_phases_parked: ROOT_PHASES_PARKED.load(Ordering::Relaxed),
        root_phases_served_at_poll: ROOT_PHASES_SERVED_AT_POLL.load(Ordering::Relaxed),
        parked_valve_fires: PARKED_VALVE_FIRES.load(Ordering::Relaxed),
        owed_requests_routed: OWED_REQUESTS_ROUTED.load(Ordering::Relaxed),
        owed_requests_served: OWED_REQUESTS_SERVED.load(Ordering::Relaxed),
        unsafe_zone_growth_bytes: UNSAFE_ZONE_GROWTH_BYTES.load(Ordering::Relaxed),
        unsafe_zone_growth_events: UNSAFE_ZONE_GROWTH_EVENTS.load(Ordering::Relaxed),
        max_poll_wait_bytes: MAX_POLL_WAIT_BYTES.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
pub(crate) fn reset_alloc_point_counters() {
    D2_VIOLATIONS.store(0, Ordering::Relaxed);
    ROOT_PHASES_PARKED.store(0, Ordering::Relaxed);
    ROOT_PHASES_SERVED_AT_POLL.store(0, Ordering::Relaxed);
    PARKED_VALVE_FIRES.store(0, Ordering::Relaxed);
    OWED_REQUESTS_ROUTED.store(0, Ordering::Relaxed);
    OWED_REQUESTS_SERVED.store(0, Ordering::Relaxed);
    UNSAFE_ZONE_GROWTH_BYTES.store(0, Ordering::Relaxed);
    UNSAFE_ZONE_GROWTH_EVENTS.store(0, Ordering::Relaxed);
    MAX_POLL_WAIT_BYTES.store(0, Ordering::Relaxed);
    clear_park();
}

/// The `[gc-alloc-point]` exit line (`PERRY_GC_DIAG=1`).
pub(super) fn alloc_point_exit_line() -> String {
    let c = alloc_point_counters();
    format!(
        "[gc-alloc-point] valve_fires={} parked_valve_fires={} old_reclaim_alloc_point={} \
         emergency_reclaims={} root_phases_parked={} root_phases_served_at_poll={} \
         owed_requests_routed={} owed_requests_served={} safepoint_drains={} \
         unsafe_zone_growth_bytes={} unsafe_zone_growth_events={} max_poll_wait_bytes={}",
        super::scan_fallback::scan_fallback_count_any_thread(
            super::ConservativeScanSite::NurseryChurnSlackValve
        ),
        c.parked_valve_fires,
        super::scan_fallback::scan_fallback_count_any_thread(
            super::ConservativeScanSite::OldReclaimAllocPoint
        ),
        super::scan_fallback::scan_fallback_count_any_thread(
            super::ConservativeScanSite::EmergencyReclaim
        ),
        c.root_phases_parked,
        c.root_phases_served_at_poll,
        c.owed_requests_routed,
        c.owed_requests_served,
        super::scan_fallback::safepoint_drain_total_any_thread(),
        c.unsafe_zone_growth_bytes,
        c.unsafe_zone_growth_events,
        c.max_poll_wait_bytes,
    )
}

/// Decision 5's ledger: with `PERRY_GC_VALVE_LEDGER=<path>` set, every process
/// appends one line at exit recording whether an allocation-point valve fired.
///
/// The line is written whether or not anything fired — that is the "counter
/// asserting the check actually ran": the gate (`scripts/gc_valve_ledger_check.py`)
/// requires one line per test it ran, so a harness that stopped passing the
/// variable, or a binary whose exit path skipped the funnel, reads as a
/// failure rather than as a clean run. Append-only, one `write` per line, so
/// concurrent processes do not interleave within a line.
pub(super) fn write_valve_ledger_line() {
    static WRITTEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Some(path) = std::env::var_os("PERRY_GC_VALVE_LEDGER").filter(|p| !p.is_empty()) else {
        return;
    };
    if !crate::native_handle::is_main_thread_or_unrecorded() {
        return;
    }
    if WRITTEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let c = alloc_point_counters();
    let valve = super::scan_fallback::scan_fallback_count_any_thread(
        super::ConservativeScanSite::NurseryChurnSlackValve,
    );
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "?".to_string());
    let line = format!(
        "v1 exe={exe} pid={} valve_fires={valve} parked_valve_fires={} d2_violations={} \
         old_reclaim_alloc_point={} root_phases_served_at_poll={} safepoint_drains={} \
         max_poll_wait_bytes={}\n",
        std::process::id(),
        c.parked_valve_fires,
        c.d2_violations,
        super::scan_fallback::scan_fallback_count_any_thread(
            super::ConservativeScanSite::OldReclaimAllocPoint
        ),
        c.root_phases_served_at_poll,
        super::scan_fallback::safepoint_drain_total_any_thread(),
        c.max_poll_wait_bytes,
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SLACK: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Make the parked-cycle valve due on the next allocation point, without
/// allocating the slack for real: park "at zero" with a zero slack.
#[cfg(test)]
pub(crate) fn test_make_parked_valve_due() {
    PARKED_AT.with(|parked| parked.set(Some(0)));
    TEST_SLACK.with(|slack| slack.set(Some(0)));
}

#[cfg(test)]
pub(crate) fn test_clear_parked_valve_override() {
    TEST_SLACK.with(|slack| slack.set(None));
}
