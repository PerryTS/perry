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
/// Smallest amount of large-birth garbage worth a full. Native buffers are
/// born in the old generation and only a full trace can prove one dead, so a
/// request loop that streams 1-6 MiB bodies owes a full for every band of
/// them. A full costs a trace of the whole live set (about 3 ms per MiB on a
/// WASI host), so the band is the knob that trades resident garbage for
/// collector time; it grows with the live set so a large heap pays
/// proportionally fewer fulls.
const LARGE_BAND_FLOOR: usize = 32 * MIB;
static PROCESS_DEBT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Default)]
struct Debt {
    allocated: usize,
    large: usize,
    /// Large births still owed an opportunity after their producing task. A
    /// mid-task full can find them live and must not erase this signal.
    burst: usize,
    burst_backoff: u32,
    /// The part of `burst` the last full left unpaid. Those births were live
    /// when it traced, so they sit inside `live_after_full` while still owing
    /// the reclaim; growth past the last full is measured without them.
    burst_unpaid_at_full: usize,
    boundary_full: bool,
    burst_at_full_start: usize,
    mature_at_full_start: usize,
    published: usize,
    /// Old, external and registered malloc bytes after the last full.
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
    due_arm(old_pressure, burst_boundary).is_some()
}

/// Which arm of the byte-pacing policy asks for a full, if any.
///
/// `pacing` pays total allocation once the mature heap has also grown. The two
/// burst arms pay a large-allocation burst at a task boundary, and they ask
/// for the same evidence of mature growth: a nursery-resident burst dies in
/// the next minor, while a full costs a trace of the whole live set. Without
/// the growth test a warm request loop whose buffers all die young paid a
/// full (about 50 ms at 25 MB live) every few requests that freed almost
/// nothing, because the volume of large births alone kept crossing the band
/// even at the maximum backoff.
fn due_arm(old_pressure: usize, burst_boundary: bool) -> Option<&'static str> {
    sample_arena();
    ACCOUNT.with(|account| {
        let debt = account.0.get();
        let large_band = LARGE_BAND_FLOOR.max(debt.live_after_full);
        let scale = 1usize << debt.backoff;
        let burst_scale = 1usize << debt.burst_backoff;
        let mature = old_pressure.saturating_add(super::malloc::malloc_resident_bytes());
        if mature < 8 * MIB {
            return None;
        }
        // OWNER 2026-10-29 (lane GCT): the next full is due when the mature
        // heap has grown by the live bytes measured at the last full times the
        // growth factor, and while the young generation is measured retaining
        // (live data, not garbage) that factor is
        // `MAJOR_PACING_LIVE_GROWTH_RETAINING_MULTIPLIER`. Only that
        // live-derived mature-growth term takes the multiplier; the published
        // (total allocation, nursery garbage included) term stays at twice the
        // live bytes. The absolute floors are unchanged. The multiplier is
        // read only once the floor is crossed (the band is never below it), so
        // a program that has not allocated that much pays nothing for it.
        if debt.published >= (128 * MIB).saturating_mul(scale) {
            let growth = super::policy::major_pacing_live_growth_multiplier();
            let band = (128 * MIB).max(debt.live_after_full.saturating_mul(2));
            let pacing_large_band =
                LARGE_BAND_FLOOR.max(debt.live_after_full.saturating_mul(growth));
            if debt.published >= band.saturating_mul(scale)
                // Total allocation includes nursery garbage. A full pays mature
                // growth; a stable old heap must leave this debt to minors.
                && mature
                    >= debt
                        .live_after_full
                        .saturating_add(pacing_large_band.saturating_mul(scale))
            {
                return Some("pacing");
            }
        }
        if !burst_boundary {
            return None;
        }
        let burst_grown = mature
            >= debt
                .live_after_full
                .saturating_sub(debt.burst_unpaid_at_full)
                .saturating_add(large_band.saturating_mul(burst_scale));
        if !burst_grown {
            return None;
        }
        if debt.allocated >= large_band.saturating_mul(burst_scale)
            // Nursery churn alone is paid cheaply by minors. Shared
            // pressure selects heaps that also grew substantial backing.
            && debt.large >= 4 * MIB
            && PROCESS_DEBT.load(Ordering::Relaxed) >= PROCESS_BYTES * burst_scale
        {
            return Some("burst-allocated");
        }
        if debt.burst >= large_band.saturating_mul(burst_scale) {
            return Some("burst-large");
        }
        None
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
        if super::diagnostic_hooks::enabled() {
            eprintln!("[gc-byte-pacing] thread={:?} allocated={} large={} process_debt={} live={} freed={} mature_live={} mature_reclaimed={} backoff={}",
                std::thread::current().id(), previous.allocated, previous.large,
                PROCESS_DEBT.load(Ordering::Relaxed), live, freed, mature_live, mature_reclaimed, previous.backoff);
        }
        PROCESS_DEBT.fetch_sub(previous.published, Ordering::Relaxed);
        // Cheap nursery garbage must not make a full look productive while
        // the mature live set remains unchanged. Allocate-black births can
        // mask reclaimed old bytes; conservatively credit the net reduction.
        let productive = mature_reclaimed >= 4 * MIB && mature_reclaimed >= mature_live / 4;
        let boundary = previous.boundary_full;
        let next_backoff = |shift: u32| {
            if productive { 0 }
            else if mature_live <= previous.live_after_full.saturating_add(4 * MIB) {
                (shift + 1).min(3)
            } else { shift }
        };
        account.0.set(Debt {
            live_after_full: mature_live,
            backoff: next_backoff(previous.backoff),
            burst: previous.burst.saturating_sub(previous.burst_at_full_start),
            burst_unpaid_at_full: previous.burst.saturating_sub(previous.burst_at_full_start),
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
    // Bytes a mutator already released are not reclaimable by a full; only
    // old storage and live external bytes can be owed to this boundary.
    let pressure = || {
        super::policy::old_gen_reclaimable_pressure_bytes()
            .saturating_add(super::policy::external_side_live_bytes())
    };
    let Some(arm) = due_arm(pressure(), true) else {
        return false;
    };
    // An unfinished non-moving minor blocks every precise safepoint. A
    // worker about to park might never supply its remaining slices. Finish
    // it through its guarded stepper only when mature reclaim is owed.
    // Keep forced budgeted-old schedules sliced for sweep-window testing.
    let mut arm = arm;
    if super::policy::gc_budgeted_cycle_active() && !super::schedule::budgeted_old_reclaim_forced()
    {
        super::policy::gc_drain_active_budgeted_cycle();
        match due_arm(pressure(), true) {
            Some(again) => arm = again,
            None => return false,
        }
    }
    super::diagnostic_hooks::note_reclaim_reason(arm);
    super::diagnostic_hooks::log_burst_boundary(arm);
    super::policy::GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    // A native producer may return to a compiled loop rather than the pump
    // that explicitly invokes a precise poll. Arm the same existing handoff
    // as alloc-point deferral, including the nursery arm's slack base.
    if super::policy::gc_moving_loop_polls_enabled()
        && !super::policy::GC_SAFEPOINT_PENDING.with(Cell::get)
    {
        super::policy::GC_SAFEPOINT_DEFER_ARENA_BASE
            .with(|base| base.set(crate::arena::arena_total_bytes()));
        super::policy::set_safepoint_pending(true);
    }
    true
}

thread_local! {
    static BURST_BOUNDARY: Cell<bool> = const { Cell::new(false) };
    static COLLECTOR_STEP: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn collector_step_active() -> bool {
    COLLECTOR_STEP.with(Cell::get)
}

/// The enforced half of the generated GC-effects `throws` premise: a
/// collection never unwinds into the frame that triggered it
/// (`scripts/gc_call_effects/seeds.txt`, kinds `collector` and `poll` are cut
/// in the `throws` solve). Called on the THROW path only, by `js_throw` and by
/// every raise entry (the Itanium and SEH raises, the aarch64 fast transport),
/// so the no-throw path pays nothing. A JS throw raised while a collector step
/// is in progress is a runtime defect, never a catchable exception: it aborts
/// here instead of unwinding through a frame the codegen compiled as a plain
/// `call`.
#[inline]
pub(crate) fn forbid_throw_in_collection() {
    if COLLECTOR_STEP.with(Cell::get) {
        throw_in_collection_abort();
    }
}

#[cold]
#[inline(never)]
fn throw_in_collection_abort() -> ! {
    eprintln!(
        "perry: FATAL: a JavaScript exception was raised during a garbage collection; \
         the collector never throws, and an unwind here would skip frames the \
         compiler emitted as plain calls"
    );
    std::process::abort();
}

#[cfg(test)]
thread_local! {
    /// Test-only sabotage hook: runs inside the collector step, right after the
    /// step flag is set. Lets a test raise a throw from "inside a collection".
    static TEST_STEP_HOOK: Cell<Option<fn()>> = const { Cell::new(None) };
}

pub(super) struct CollectorStepGuard(bool);

impl CollectorStepGuard {
    pub(super) fn enter() -> Self {
        sample_arena();
        let guard = Self(COLLECTOR_STEP.with(|active| active.replace(true)));
        #[cfg(test)]
        if let Some(hook) = TEST_STEP_HOOK.with(Cell::get) {
            hook();
        }
        guard
    }
}

impl Drop for CollectorStepGuard {
    fn drop(&mut self) {
        // Sample while the guard is active: collector bumps are not debt.
        sample_arena();
        COLLECTOR_STEP.with(|active| active.set(self.0));
    }
}

#[cfg(test)]
pub(super) fn test_debt() -> (usize, usize) {
    sample_arena();
    ACCOUNT.with(|account| (account.0.get().allocated, account.0.get().large))
}

pub(super) fn checkpoint() {
    sample_arena();
}

fn sample_arena() {
    // Runtime helpers and generated code share the Eden offset. Every debt
    // sample must first materialize that offset, including policy questions
    // between checkpoints and the sample before a collector guard drops.
    crate::arena::sync_inline_arena_state();
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

#[cfg(test)]
mod tests {
    use super::*;

    const ABORT_CHILD_ENV: &str = "PERRY_TEST_THROW_IN_COLLECTION_CHILD";

    /// A JS throw raised from inside a collection must abort the process with
    /// the fatal message, never unwind (the premise of the generated
    /// `throws` column). The child process installs the sabotage hook, runs a
    /// real collection and throws from inside it; the parent checks it died by
    /// abort and said why.
    #[test]
    fn js_throw_inside_a_collection_aborts() {
        if std::env::var_os(ABORT_CHILD_ENV).is_some() {
            TEST_STEP_HOOK.with(|h| h.set(Some(|| crate::exception::js_throw(1.0))));
            let _ = super::super::gc_collect_minor();
            // Reaching here means the throw returned or unwound: fail loudly.
            std::process::exit(77);
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "gc::allocation_pacing::tests::js_throw_inside_a_collection_aborts",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ABORT_CHILD_ENV, "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("exception was raised during a garbage collection"),
            "no fatal message; status {:?}, stderr: {stderr}",
            out.status
        );
        assert_ne!(out.status.code(), Some(77), "the throw returned");
        assert!(!out.status.success());
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(out.status.signal(), Some(6), "expected SIGABRT");
        }
    }

    /// Outside a collection the check is inert.
    #[test]
    fn forbid_throw_is_inert_outside_a_collection() {
        forbid_throw_in_collection();
        let _step = CollectorStepGuard::enter();
        drop(_step);
        forbid_throw_in_collection();
    }

    #[test]
    fn shared_runtime_bumps_do_not_charge_collector_bytes_to_mutator() {
        std::thread::spawn(|| {
            let _triggers =
                super::super::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            crate::arena::js_inline_arena_state();
            let before = test_debt();
            for _ in 0..16 {
                crate::arena::arena_alloc_gc(32, 8, super::super::GC_TYPE_STRING);
            }
            let mutator = test_debt();
            assert_eq!(mutator.0 - before.0, 16 * 40);
            assert_eq!(mutator.1, before.1);
            {
                let _step = CollectorStepGuard::enter();
                for _ in 0..64 {
                    crate::arena::arena_alloc_gc(32, 8, super::super::GC_TYPE_STRING);
                }
            }
            assert_eq!(
                test_debt(),
                mutator,
                "collector bumps are not allocation pressure"
            );
            crate::arena::arena_alloc_gc(32, 8, super::super::GC_TYPE_STRING);
            assert_eq!(test_debt().0, mutator.0 + 40);
        })
        .join()
        .unwrap();
    }
}

/// Dedicated leaf allocation already has the ordinary collecting-allocation
/// effect. Finish an older non-moving cycle before publishing its new owner;
/// a suppressed/unsafe construction leaves the guarded stepper deferred.
pub(crate) fn finish_cycle_before_large_birth() {
    if super::policy::gc_budgeted_cycle_active() && !super::schedule::budgeted_old_reclaim_forced()
    {
        super::policy::gc_drain_active_budgeted_cycle();
    }
}

/// Numeric pressure arms the existing poll, without tracing a partially
/// initialized producer result or consulting the old-large major band.
pub(crate) fn arm_young_after_large_birth() {
    if super::policy::young_scavenge_cap_due()
        && !super::policy::GC_SAFEPOINT_PENDING.with(Cell::get)
    {
        super::policy::GC_SAFEPOINT_DEFER_ARENA_BASE
            .with(|base| base.set(crate::arena::arena_total_bytes()));
        super::policy::set_safepoint_pending(true);
    }
}

/// The legacy JSON output route is justified by a copying nursery's cost.
/// Large non-moving owners count against the young budget, but cannot stand
/// in for the moving tree this staged workaround was written for.
pub(crate) fn moving_young_generation_holds_a_nursery() -> bool {
    super::policy::young_generation_holds_a_nursery()
        && crate::arena::moving_from_space_in_use_bytes() >= super::gc_scavenge_nursery_cap_bytes()
}

/// Installed reporting arm; the collector calls the coarse optional slot.
pub(super) fn report_burst_boundary(arm: &'static str) {
    if super::telemetry::gc_log_enabled() {
        let debt = ACCOUNT.with(|account| account.0.get());
        eprintln!(
            "[gc-log] owed arm={arm} pressure={} allocated={} large={} burst={} live_after_full={} unpaid={} backoff={} burst_backoff={}",
            super::policy::old_gen_reclaimable_pressure_bytes().saturating_add(super::policy::external_side_live_bytes()), debt.allocated, debt.large, debt.burst, debt.live_after_full,
            debt.burst_unpaid_at_full, debt.backoff, debt.burst_backoff
        );
    }
}
