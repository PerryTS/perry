//! Allocation debt is independent of occupancy: minors, promotion credits,
//! hole reuse and explicit external releases must not erase it. Only a full
//! reclaim pays it. The process total contains numeric debt, never heap roots.
//! Publication is batched by MiB; foreign heaps observe it at safepoints or
//! their next allocation batch. No atomic is added to generated bump code.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

const MIB: usize = 1024 * 1024;
const PUBLISH_BYTES: usize = MIB;
const PROCESS_BYTES: usize = 64 * MIB;
static PROCESS_DEBT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Default)]
struct Debt {
    allocated: usize,
    large: usize,
    /// Large births still owed an opportunity after their producing task. A
    /// mid-task full can find them live and must not erase this signal.
    burst: usize,
    burst_backoff: u32,
    boundary_full: bool,
    burst_at_full_start: usize,
    mature_at_full_start: usize,
    published: usize,
    live_after_full: usize,
    backoff: u32,
    seen_arena: usize,
    seen_large: usize,
}

#[derive(Default)]
struct Account(Cell<Debt>);

impl Drop for Account {
    fn drop(&mut self) {
        PROCESS_DEBT.fetch_sub(self.0.get().published, Ordering::Relaxed);
    }
}

crate::perry_thread_local! {
    static ACCOUNT: Account = Account::default();
}

/// Called only for successful mutator allocations (including swept holes).
/// Collector relocation and promotion are not new allocation pressure.
#[inline]
pub(crate) fn note_allocation(bytes: usize, large: bool) {
    if bytes == 0 || super::policy::allocation_pacing_collector_active() {
        return;
    }
    ACCOUNT.with(|account| {
        let mut debt = account.0.get();
        let account_published_before = debt.published;
        debt.allocated = debt.allocated.saturating_add(bytes);
        if large {
            debt.large = debt.large.saturating_add(bytes);
            debt.burst = debt.burst.saturating_add(bytes);
        }
        if debt.allocated.saturating_sub(debt.published) >= PUBLISH_BYTES {
            PROCESS_DEBT.fetch_add(debt.allocated - debt.published, Ordering::Relaxed);
            debt.published = debt.allocated;
        }
        account.0.set(debt);
        if debt.published != account_published_before {
            super::trigger_watermark::retire_trigger_watermark();
        }
    });
}

pub(super) fn due(old_pressure: usize, burst_boundary: bool) -> bool {
    sample_arena();
    ACCOUNT.with(|account| {
        let debt = account.0.get();
        let band = (128 * MIB).max(debt.live_after_full.saturating_mul(2));
        let large_band = (8 * MIB).max(debt.live_after_full / 2);
        let scale = 1usize << debt.backoff;
        let burst_scale = 1usize << debt.burst_backoff;
        let owed = debt.published >= band.saturating_mul(scale)
            || (burst_boundary
                && debt.allocated >= large_band.saturating_mul(burst_scale)
                // Nursery churn alone is paid cheaply by minors. Shared
                // pressure selects heaps that also grew substantial backing.
                && debt.large >= 4 * MIB
                && PROCESS_DEBT.load(Ordering::Relaxed) >= PROCESS_BYTES * burst_scale)
            || (burst_boundary && debt.burst >= large_band.saturating_mul(burst_scale));
        owed && (old_pressure >= 8 * MIB
            || old_pressure.saturating_add(super::malloc::malloc_resident_bytes()) >= 8 * MIB)
    })
}

/// Only a trace begun after the task unwinds can pay that task's burst.
/// Births during a budgeted full remain marked live and need a later trace.
pub(super) fn begin_full() {
    sample_arena();
    ACCOUNT.with(|account| {
        let mut debt = account.0.get();
        debt.boundary_full = BURST_BOUNDARY.with(Cell::get);
        debt.burst_at_full_start = if debt.boundary_full { debt.burst } else { 0 };
        debt.mature_at_full_start = mature_resident_bytes();
        account.0.set(debt);
    });
}

pub(super) fn finish_full(live: usize, freed: usize) {
    sample_arena();
    let live = live.saturating_add(super::malloc::malloc_resident_bytes());
    let mature_live = mature_resident_bytes();
    ACCOUNT.with(|account| {
        let previous = account.0.get();
        let mature_reclaimed = previous.mature_at_full_start.saturating_sub(mature_live);
        if super::gc_diag_enabled() {
            eprintln!("[gc-byte-pacing] thread={:?} allocated={} large={} process_debt={} live={} freed={} mature_live={} mature_reclaimed={} backoff={}",
                std::thread::current().id(), previous.allocated, previous.large,
                PROCESS_DEBT.load(Ordering::Relaxed), live, freed, mature_live, mature_reclaimed, previous.backoff);
        }
        PROCESS_DEBT.fetch_sub(previous.published, Ordering::Relaxed);
        // Cheap nursery garbage must not make a full look productive while
        // the mature live set remains unchanged. Allocate-black births can
        // mask reclaimed old bytes; conservatively credit the net reduction.
        let productive = mature_reclaimed >= 4 * MIB && mature_reclaimed >= live / 4;
        let boundary = previous.boundary_full;
        let next_backoff = |previous: u32| {
            if productive { 0 }
            else if live <= account.0.get().live_after_full.saturating_add(4 * MIB) {
                (previous + 1).min(3)
            } else { previous }
        };
        account.0.set(Debt {
            live_after_full: live,
            backoff: next_backoff(previous.backoff),
            burst: previous.burst.saturating_sub(previous.burst_at_full_start),
            burst_backoff: if boundary { next_backoff(previous.burst_backoff) }
                else { previous.burst_backoff },
            seen_arena: previous.seen_arena,
            seen_large: previous.seen_large,
            ..Debt::default()
        });
        super::trigger_watermark::retire_trigger_watermark();
    });
}

fn mature_resident_bytes() -> usize {
    super::policy::old_gen_reclaimable_pressure_bytes()
        .saturating_add(super::policy::external_side_live_bytes())
        .saturating_add(super::malloc::malloc_resident_bytes())
}

/// A burst boundary still uses the normal guarded precise collection path.
pub(crate) fn burst_boundary() -> bool {
    let pressure = || {
        super::policy::old_gen_reclaimable_pressure_bytes()
            .saturating_add(super::policy::external_side_old_reclaim_pressure_bytes())
    };
    if !due(pressure(), true) {
        return false;
    }
    // An unfinished non-moving minor blocks every precise safepoint. A
    // worker about to park might never supply its remaining slices. Finish
    // it through its guarded stepper only when mature reclaim is owed.
    // Keep forced budgeted-old schedules sliced for sweep-window testing.
    if super::policy::gc_budgeted_cycle_active() && !super::schedule::budgeted_old_reclaim_forced()
    {
        super::policy::gc_drain_active_budgeted_cycle();
        if !due(pressure(), true) {
            return false;
        }
    }
    super::policy::GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    true
}

thread_local! {
    static BURST_BOUNDARY: Cell<bool> = const { Cell::new(false) };
    static COLLECTOR_STEP: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn collector_step_active() -> bool {
    COLLECTOR_STEP.with(Cell::get)
}

pub(super) struct CollectorStepGuard(bool);

impl CollectorStepGuard {
    pub(super) fn enter() -> Self {
        crate::arena::sync_inline_arena_state();
        sample_arena();
        Self(COLLECTOR_STEP.with(|active| active.replace(true)))
    }
}

impl Drop for CollectorStepGuard {
    fn drop(&mut self) {
        sample_arena();
        COLLECTOR_STEP.with(|active| active.set(self.0));
    }
}

#[cfg(test)]
pub(super) fn test_debt() -> (usize, usize) {
    crate::arena::sync_inline_arena_state();
    sample_arena();
    ACCOUNT.with(|account| (account.0.get().allocated, account.0.get().large))
}

pub(super) fn checkpoint() {
    crate::arena::sync_inline_arena_state();
    sample_arena();
}

fn sample_arena() {
    let (bytes, large) = crate::arena::allocation_totals();
    let (new_bytes, new_large) = ACCOUNT.with(|account| {
        let mut debt = account.0.get();
        let delta = (
            bytes.saturating_sub(debt.seen_arena),
            large.saturating_sub(debt.seen_large),
        );
        debt.seen_arena = bytes;
        debt.seen_large = large;
        account.0.set(debt);
        delta
    });
    // Charge the large share separately without calling it nursery churn.
    note_allocation(new_large, true);
    note_allocation(new_bytes.saturating_sub(new_large), false);
}

/// Marks the outermost pump checkpoint. A full begun here can pay the burst
/// present at its start; allocation bytes themselves reset on every full.
pub(crate) struct BurstBoundaryGuard(bool);

impl BurstBoundaryGuard {
    pub(crate) fn enter() -> Self {
        checkpoint();
        Self(BURST_BOUNDARY.with(|boundary| boundary.replace(true)))
    }
}

impl Drop for BurstBoundaryGuard {
    fn drop(&mut self) {
        BURST_BOUNDARY.with(|boundary| boundary.set(self.0));
    }
}
