//! Idle-time old-gen reclaim: the collector's answer to a mutator that has
//! stopped allocating.
//!
//! Every automatic collection in this runtime is scheduled by ALLOCATION —
//! arena bytes, malloc count, old-gen growth, the nursery cap. A program that
//! bursts (a paste into a TUI, a request storm, a build step) and then goes
//! quiet therefore freezes its heap at whatever the last mid-burst cycle left:
//! the promoted-then-dead residue of the burst sits in old-gen until the NEXT
//! burst grows the arena far enough to trip a pacing predicate, which an idle
//! program never does. Measured on the compiled claude-code TUI: 425–680 MB
//! resident through minutes of idle, flat to the byte, against node's
//! ~150–200 MB for the same bundle. V8 closes this gap with its memory
//! reducer — a major GC started from an idle notification, repeated while it
//! pays, then parked until the mutator is active again. This module is that
//! reducer.
//!
//! # Where it runs
//!
//! `event_pump::js_wait_for_event` calls [`park_hook`] at the one point the
//! generated event loop is about to PARK: no notify pending, no microtask
//! queued, no timer due right now. That is the runtime's definition of idle.
//! It is deliberately not "no allocation for N seconds": a TUI never
//! satisfies that (the compiled claude-code TUI copies ~10 MB of surviving
//! render tree in a young collection every ~6 s at rest), but it parks
//! thousands of times a minute.
//!
//! # What it runs
//!
//! A FULL cycle through the budgeted incremental stepper — the same
//! `GcCycleState` the arena/old-reclaim pacing uses — started with
//! `GcTriggerKind::IdleReclaim` and the block-pool drain armed, so the
//! sweep's released blocks leave the process instead of sitting in the reuse
//! pool, and driven here in time slices of [`IDLE_RECLAIM_SLICE_US`]. Before
//! every slice the hook asks the pump whether a wake has arrived
//! (`event_pump::main_thread_wake_pending`) and returns to the loop the
//! moment one has, so input landing mid-cycle waits for at most one slice;
//! the cycle then finishes wherever budgeted cycles always finish — mutator
//! assists, the pump safepoint, or the next park. Nothing here is a new
//! collector. It is the existing one, given the idle time it never had.
//!
//! The same slicing loop also drives a budgeted cycle the PACER started and
//! then could not finish because the mutator went quiet (the
//! `cycle_starts > completions` shape `[gc-incremental]` reports). An open
//! cycle keeps the mark barrier armed and its reclaim unpublished; finishing it
//! at idle is pure win and needs no new machinery.
//!
//! # When it runs
//!
//! Three gates, all O(1), evaluated at every park:
//!
//! 1. **Activity or arena debt.** Normally at least `2^backoff` collections
//!    the reducer did not start itself have completed since its last full. A
//!    collection is the signal that the mutator allocated enough to matter.
//!    The exception is a bounded [`super::arena_right_size`] episode: arena
//!    blocks need two full observations before their mappings can be returned,
//!    and an idle heap cannot create the second through mutator activity.
//! 2. **Quiet.** At least [`IDLE_RECLAIM_QUIET_MS`] since the last such
//!    collection was observed — a burst still in progress collects every few
//!    hundred milliseconds and must not be interleaved with a whole-heap mark.
//! 3. **Rate.** At least [`IDLE_RECLAIM_MIN_INTERVAL_MS`] since the reducer's
//!    own last full started.
//!
//! **Productivity backoff.** A full whose SWEEP freed less than
//! [`IDLE_RECLAIM_PRODUCTIVE_PCT`] percent of the old-gen occupancy it started
//! with, or less than [`IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES`], doubles the
//! activity requirement
//! up to `2^`[`IDLE_RECLAIM_MAX_BACKOFF_SHIFT`] collections; a productive full
//! resets it to one. This is what keeps a RETAINING idle heap (the TUI's
//! six-second young-collection sawtooth) from paying a whole-heap mark per
//! minor: after a few unproductive attempts the reducer runs once per ~32
//! collections, and any burst — which produces many collections in a row —
//! re-arms it promptly.
//!
//! The price is the cycle's own freed-bytes count and NOT the change in
//! `arena::old_gen_in_use_bytes`, which is the sum of the live old blocks'
//! bump offsets: a non-moving sweep hands dead objects to the old-gen free
//! list and the block keeps its offset, so only a whole-block release moves
//! that number. Pricing on it scored every full on a real workload
//! unproductive — measured on the compiled claude-code TUI, two reducer fulls
//! freed 2.37 MB and 0.51 MB while occupancy stood still at 93.6 MB with
//! 50 MB already on the old-gen free list, and the reducer backed off to one
//! attempt per four collections inside a minute of idle. Returning that free
//! list to the OS needs compaction, which a budgeted (non-moving) cycle
//! cannot do; the `reusable=` field on the `[gc-idle-reclaim] done` line
//! reports how much is waiting for it.
//!
//! **Work cap.** While a cycle is open the hook never spends more than
//! [`IDLE_RECLAIM_MAX_WORK_MS_PER_SECOND`] of any wall-clock second stepping
//! it; past that it lets the loop park for the rest of its budget. A healthy
//! cycle finishes a little later; a cycle that could never finish (which would
//! be a stepper bug, not a reducer one) costs at most half a core instead of
//! keeping the process from ever sleeping.
//!
//! # Kill switch
//!
//! `PERRY_GC_IDLE_RECLAIM=0` (default ON; `env_default_on_enabled`
//! vocabulary). Its OFF state is asserted by
//! `gc::tests::idle_reclaim::kill_switch_off_leaves_the_heap_alone`, per
//! CLAUDE.md's knob kill-policy.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Time-slice granted to the stepper between two wake checks. A keystroke
/// that lands while a slice is running waits for at most this long (plus the
/// stepper's own unsliced phases, which are the same on every budgeted path).
pub const IDLE_RECLAIM_SLICE_US: u64 = 4_000;

/// Minimum time since the last collection the reducer did not start itself
/// before it will start one of its own.
pub const IDLE_RECLAIM_QUIET_MS: u64 = 3_000;

/// Minimum spacing between two reducer-started fulls.
pub const IDLE_RECLAIM_MIN_INTERVAL_MS: u64 = 10_000;

/// A reducer full counts as productive when its sweep freed at least this many
/// bytes AND at least [`IDLE_RECLAIM_PRODUCTIVE_PCT`] percent of the old-gen
/// occupancy it started with.
pub const IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES: usize = 4 * 1024 * 1024;

/// See [`IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES`].
pub const IDLE_RECLAIM_PRODUCTIVE_PCT: usize = 5;

/// Cap on the unproductive-streak backoff: the activity requirement never
/// exceeds `2^this` collections.
pub const IDLE_RECLAIM_MAX_BACKOFF_SHIFT: u32 = 5;

/// Most collector work the park hook will do in any one wall-clock second
/// while a budgeted cycle is open; past this the loop parks instead.
pub const IDLE_RECLAIM_MAX_WORK_MS_PER_SECOND: u64 = 500;

/// What the park site should do after the hook returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParkVerdict {
    /// Do not park: collector work ran (the timer budget is stale) or a wake is
    /// already pending. Go back around the loop.
    Resume,
    /// Park, for at most this many milliseconds of the caller's budget.
    Park(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartReason {
    /// The existing memory-reducer signal: the mutator completed enough
    /// collections and then went quiet.
    Activity,
    /// Sustained arena slack still needs full observations before empty blocks
    /// can be returned, even though the mutator has done nothing new.
    ArenaRightSize,
}

impl StartReason {
    fn as_str(self) -> &'static str {
        match self {
            StartReason::Activity => "activity",
            StartReason::ArenaRightSize => "arena_right_size",
        }
    }
}

#[derive(Default)]
struct IdleReclaimState {
    /// Collections not started by the reducer, as of the last observation.
    last_seen_external: u64,
    /// Reducer clock (ms) at the last time `last_seen_external` changed.
    last_external_change_ms: u64,
    /// `last_seen_external` when the reducer last started (or finished) a full.
    external_at_last_attempt: u64,
    /// Reducer clock (ms) when the reducer last started a full.
    last_attempt_ms: u64,
    /// Fulls the reducer started on this thread.
    attempts: u64,
    /// Fulls the reducer completed on this thread. Thread-local on purpose:
    /// `gc_total_collection_count` is per thread (each `perry/thread` worker
    /// owns an arena and a collector), so the subtraction in
    /// `external_collections` must be too — a process-wide count would let one
    /// thread's idle fulls hide another thread's activity.
    completions: u64,
    /// Unproductive-streak backoff; the activity requirement is `1 << shift`.
    backoff_shift: u32,
    /// Old-gen occupancy when the in-flight (or last) reducer full started.
    old_in_use_at_start: usize,
    /// Whether the first observation has been taken (so a fresh thread does
    /// not read its zeroed clock as "quiet since the epoch").
    observed: bool,
    /// Start of the current one-second work-accounting window.
    work_window_start_ms: u64,
    /// Collector work charged inside that window.
    work_in_window_ms: u64,
}

crate::perry_thread_local! {
    static STATE: RefCell<IdleReclaimState> = RefCell::new(IdleReclaimState::default());
    #[cfg(test)]
    static TEST_NOW_MS: Cell<Option<u64>> = const { Cell::new(None) };
    #[cfg(test)]
    static TEST_ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
    #[cfg(test)]
    static TEST_MAX_SLICES: Cell<Option<u32>> = const { Cell::new(None) };
    #[cfg(test)]
    static TEST_SLICE_US: Cell<Option<u64>> = const { Cell::new(None) };
    #[cfg(test)]
    static TEST_WORK_CHARGE_MS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Rotate the work window if a second has passed; report whether the cap is
/// already spent for this one.
fn work_capped(now: u64) -> bool {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if now.saturating_sub(st.work_window_start_ms) >= 1_000 {
            st.work_window_start_ms = now;
            st.work_in_window_ms = 0;
        }
        st.work_in_window_ms >= IDLE_RECLAIM_MAX_WORK_MS_PER_SECOND
    })
}

/// Charge one slice's wall time to the current window.
fn charge_work(elapsed_ms: u64) {
    #[cfg(test)]
    let elapsed_ms = TEST_WORK_CHARGE_MS.with(Cell::get).unwrap_or(elapsed_ms);
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.work_in_window_ms = st.work_in_window_ms.saturating_add(elapsed_ms);
    });
}

fn slice_us() -> u64 {
    #[cfg(test)]
    if let Some(us) = TEST_SLICE_US.with(Cell::get) {
        return us;
    }
    IDLE_RECLAIM_SLICE_US
}

// Process-wide counters, in the `instruments` style: each is a live-subject
// witness for a gate that would otherwise pass vacuously ("no regression"
// means nothing if the reducer never ran).
static ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static COMPLETIONS: AtomicU64 = AtomicU64::new(0);
static PRODUCTIVE: AtomicU64 = AtomicU64::new(0);
static FREED_BYTES: AtomicU64 = AtomicU64::new(0);
static OLD_RECLAIMED_BYTES: AtomicU64 = AtomicU64::new(0);
static SLICES: AtomicU64 = AtomicU64::new(0);
static YIELDS: AtomicU64 = AtomicU64::new(0);
static START_BLOCKED: AtomicU64 = AtomicU64::new(0);
static WORK_CAPPED: AtomicU64 = AtomicU64::new(0);
static POST_PURGES: AtomicU64 = AtomicU64::new(0);

/// Reducer fulls started in this process.
pub fn idle_reclaim_attempts() -> u64 {
    ATTEMPTS.load(Ordering::Relaxed)
}

/// Reducer fulls completed in this process (wherever they finished).
pub fn idle_reclaim_completions() -> u64 {
    COMPLETIONS.load(Ordering::Relaxed)
}

/// Reducer fulls that met the productivity bar.
pub fn idle_reclaim_productive() -> u64 {
    PRODUCTIVE.load(Ordering::Relaxed)
}

/// Sweep-freed bytes across all reducer fulls.
pub fn idle_reclaim_freed_bytes() -> u64 {
    FREED_BYTES.load(Ordering::Relaxed)
}

/// Old-gen occupancy the reducer's fulls removed, summed.
pub fn idle_reclaim_old_reclaimed_bytes() -> u64 {
    OLD_RECLAIMED_BYTES.load(Ordering::Relaxed)
}

/// Time slices the park hook spent stepping (any budgeted cycle).
pub fn idle_reclaim_slices() -> u64 {
    SLICES.load(Ordering::Relaxed)
}

/// Times the park hook stopped stepping because a wake was pending.
pub fn idle_reclaim_yields() -> u64 {
    YIELDS.load(Ordering::Relaxed)
}

/// Times an owed reducer full could not start (a start guard held).
pub fn idle_reclaim_start_blocked() -> u64 {
    START_BLOCKED.load(Ordering::Relaxed)
}

/// Times the park hook let the loop park because the per-second work cap was
/// spent with a cycle still open.
pub fn idle_reclaim_work_capped() -> u64 {
    WORK_CAPPED.load(Ordering::Relaxed)
}

/// Allocator purges the park hook ran after a budgeted cycle completed in it.
pub fn idle_reclaim_post_purges() -> u64 {
    POST_PURGES.load(Ordering::Relaxed)
}

/// Current unproductive-streak backoff shift on this thread.
pub fn idle_reclaim_backoff_shift() -> u32 {
    STATE.with(|s| s.borrow().backoff_shift)
}

/// `PERRY_GC_IDLE_RECLAIM` — default ON; `0`/`off`/`false`/`no` disable.
pub fn idle_reclaim_enabled_from_value(raw: Option<&str>) -> bool {
    super::env_default_on_from_value(raw)
}

fn idle_reclaim_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_ENABLED.with(Cell::get) {
        return forced;
    }
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| super::env_default_on_enabled("PERRY_GC_IDLE_RECLAIM"))
}

fn now_ms() -> u64 {
    #[cfg(test)]
    if let Some(now) = TEST_NOW_MS.with(Cell::get) {
        return now;
    }
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = *EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

/// Collections completed on this thread that the reducer did not start.
fn external_collections() -> u64 {
    let own = STATE.with(|s| s.borrow().completions);
    gc_total_collection_count().saturating_sub(own)
}

fn old_gen_occupancy() -> usize {
    crate::arena::old_gen_in_use_bytes().saturating_add(policy::external_side_live_bytes())
}

/// Observe the external-collection counter and decide whether a reducer full
/// is owed right now. Pure bookkeeping — no collector state is touched.
fn start_reason(now: u64) -> Option<StartReason> {
    // Read the counters BEFORE taking the state borrow: `external_collections`
    // borrows the same cell.
    let external = external_collections();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if !st.observed || external != st.last_seen_external {
            st.observed = true;
            st.last_seen_external = external;
            st.last_external_change_ms = now;
        }
        if now.saturating_sub(st.last_external_change_ms) < IDLE_RECLAIM_QUIET_MS {
            return None;
        }
        if st.attempts > 0 && now.saturating_sub(st.last_attempt_ms) < IDLE_RECLAIM_MIN_INTERVAL_MS
        {
            return None;
        }
        // Arena capacity release itself needs multiple full observations. Once
        // sustained low utilization has created that bounded debt, requiring
        // another mutator collection here recreates #9709's deadlock: the
        // mutator is idle precisely because there is no more activity.
        if super::arena_right_size::owed() {
            return Some(StartReason::ArenaRightSize);
        }
        let since_attempt = external.saturating_sub(st.external_at_last_attempt);
        if since_attempt < (1u64 << st.backoff_shift) {
            return None;
        }
        Some(StartReason::Activity)
    })
}

fn note_started(now: u64, reason: StartReason) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.attempts += 1;
        st.last_attempt_ms = now;
        st.external_at_last_attempt = st.last_seen_external;
        st.old_in_use_at_start = old_gen_occupancy();
        ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        if reason == StartReason::ArenaRightSize {
            super::arena_right_size::note_started();
        }
        if gc_diag_enabled() {
            let (_, right_size_fulls_remaining, _, usage) = super::arena_right_size::snapshot();
            eprintln!(
                "[gc-idle-reclaim] start attempt={} reason={} external_collections={} \
                 backoff_shift={} old_in_use={} arena_live={} arena_capacity={} \
                 right_size_fulls_remaining={}",
                st.attempts,
                reason.as_str(),
                st.last_seen_external,
                st.backoff_shift,
                st.old_in_use_at_start,
                usage.live_bytes,
                usage.capacity_bytes,
                right_size_fulls_remaining,
            );
        }
    });
}

/// Called by the budgeted-cycle finisher when a cycle whose trigger is
/// `GcTriggerKind::IdleReclaim` completes — wherever it completed. Prices the
/// full against the occupancy it started with and moves the backoff.
pub(super) fn note_cycle_completed(freed_bytes: u64) {
    COMPLETIONS.fetch_add(1, Ordering::Relaxed);
    FREED_BYTES.fetch_add(freed_bytes, Ordering::Relaxed);
    // Count our completion first, then read the external count with the cell
    // released: `external_collections` borrows it too.
    STATE.with(|s| s.borrow_mut().completions += 1);
    let external = external_collections();
    // The sweep that just ran is what makes old-page `dead_bytes` current,
    // which is what the compaction's selection reads.
    super::idle_compact::note_reclaim_full_completed();
    let after = old_gen_occupancy();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let before = st.old_in_use_at_start;
        let reclaimed = before.saturating_sub(after);
        OLD_RECLAIMED_BYTES.fetch_add(reclaimed as u64, Ordering::Relaxed);
        let bar = IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES.max(before / 100 * IDLE_RECLAIM_PRODUCTIVE_PCT);
        // Price the full by what its sweep freed. `old_gen_occupancy` sums the
        // live blocks' bump offsets, which a non-moving sweep cannot lower —
        // it returns objects to the old-gen free list and the block keeps its
        // offset — so the occupancy delta reads ~0 for a cycle that freed
        // megabytes, and every real full scored unproductive (module docs).
        // The delta stays in the counters and the trace as the whole-block
        // release signal it actually is.
        let productive = freed_bytes >= bar as u64;
        if productive {
            PRODUCTIVE.fetch_add(1, Ordering::Relaxed);
            st.backoff_shift = 0;
        } else {
            st.backoff_shift = (st.backoff_shift + 1).min(IDLE_RECLAIM_MAX_BACKOFF_SHIFT);
        }
        // Re-baseline activity at completion, not start: collections that
        // landed while an interrupted cycle was finishing elsewhere were
        // marked by it, so they are not new work for the next attempt.
        st.external_at_last_attempt = external;
        st.last_seen_external = external;
        if gc_diag_enabled() {
            eprintln!(
                "[gc-idle-reclaim] done old_in_use={before}->{after} reclaimed_old={reclaimed} freed={freed_bytes} bar={bar} reusable={} productive={productive} backoff_shift={}",
                old_free_bytes(),
                st.backoff_shift
            );
        }
    });
}

/// Step the active budgeted cycle in wake-checked slices until it completes,
/// a wake arrives, the caller's timer deadline passes, the per-second work cap
/// is spent, or the stepper reports it cannot proceed.
fn drive_active_cycle(deadline_ms: u64) -> ParkVerdict {
    let mut did_work = false;
    #[cfg(test)]
    let mut slices_left = TEST_MAX_SLICES.with(Cell::get);
    while policy::gc_budgeted_cycle_active() {
        if crate::event_pump::main_thread_wake_pending() {
            YIELDS.fetch_add(1, Ordering::Relaxed);
            return ParkVerdict::Resume;
        }
        let before = now_ms();
        if before >= deadline_ms {
            return ParkVerdict::Resume;
        }
        if work_capped(before) {
            WORK_CAPPED.fetch_add(1, Ordering::Relaxed);
            return ParkVerdict::Park(deadline_ms - before);
        }
        #[cfg(test)]
        if let Some(left) = slices_left.as_mut() {
            if *left == 0 {
                return if did_work {
                    ParkVerdict::Resume
                } else {
                    ParkVerdict::Park(deadline_ms - before)
                };
            }
            *left -= 1;
        }
        let result = policy::gc_idle_reclaim_step(slice_us());
        SLICES.fetch_add(1, Ordering::Relaxed);
        did_work = true;
        charge_work(now_ms().saturating_sub(before));
        if result.status == JS_GC_STEP_STATUS_COMPLETED {
            // The cycle's reclaim tail already purged the allocator, but the
            // cycle's OWN bookkeeping (mark stacks, side-table snapshots) was
            // still live then and has only now been dropped. Nobody is waiting,
            // so ask once more: measured on the idle-reclaim fixture this is
            // the difference between RSS falling by ~4 MB and by the ~22 MB
            // the sweep actually deallocated.
            let _ = super::cycle_malloc_trim::run_malloc_trim(GcProgressKind::NormalIncremental);
            POST_PURGES.fetch_add(1, Ordering::Relaxed);
            return ParkVerdict::Resume;
        }
        if result.status != JS_GC_STEP_STATUS_ACTIVE {
            // The stepper is blocked (suppressed / unsafe zone / re-entrant):
            // nothing more to do here now.
            return ParkVerdict::Resume;
        }
    }
    if did_work {
        ParkVerdict::Resume
    } else {
        ParkVerdict::Park(deadline_ms.saturating_sub(now_ms()))
    }
}

/// The park-site hook. `budget_ms` is how long the caller was about to park;
/// the verdict says whether to park at all, and for how much of that budget.
pub(crate) fn park_hook(budget_ms: u64) -> ParkVerdict {
    if !idle_reclaim_enabled() {
        return ParkVerdict::Park(budget_ms);
    }
    let now = now_ms();
    let deadline = now.saturating_add(budget_ms);
    if policy::gc_budgeted_cycle_active() {
        // A cycle is already open — the pacer's or ours. Idle time is the
        // cheapest time to finish it.
        return drive_active_cycle(deadline);
    }
    // A full of ours has completed, so page `dead_bytes` are current. If the
    // old-gen free list has outgrown what a non-moving sweep can hand back,
    // this is where the moving collection that CAN hand it back runs
    // (`gc/idle_compact.rs`); it is synchronous, so the timer budget computed
    // by the caller is stale afterwards.
    if super::idle_compact::maybe_compact(now) {
        return ParkVerdict::Resume;
    }
    let Some(reason) = start_reason(now) else {
        return ParkVerdict::Park(budget_ms);
    };
    if !policy::gc_idle_reclaim_try_start() {
        START_BLOCKED.fetch_add(1, Ordering::Relaxed);
        return ParkVerdict::Park(budget_ms);
    }
    note_started(now, reason);
    drive_active_cycle(deadline)
}

/// `PERRY_GC_DIAG=1` exit line.
pub(super) fn emit_diag() {
    eprintln!(
        "[gc-idle-reclaim] enabled={} attempts={} completions={} productive={} freed_bytes={} \
         old_reclaimed_bytes={} slices={} yields={} start_blocked={} work_capped={} post_purges={} \
         backoff_shift={}",
        idle_reclaim_enabled(),
        idle_reclaim_attempts(),
        idle_reclaim_completions(),
        idle_reclaim_productive(),
        idle_reclaim_freed_bytes(),
        idle_reclaim_old_reclaimed_bytes(),
        idle_reclaim_slices(),
        idle_reclaim_yields(),
        idle_reclaim_start_blocked(),
        idle_reclaim_work_capped(),
        idle_reclaim_post_purges(),
        idle_reclaim_backoff_shift(),
    );
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    /// Pin the reducer clock for this thread (`None` restores the real clock).
    pub(crate) fn set_test_now_ms(now: Option<u64>) {
        TEST_NOW_MS.with(|c| c.set(now));
    }

    /// Pin the kill switch for this thread (`None` restores the env read).
    pub(crate) fn set_test_enabled(enabled: Option<bool>) {
        TEST_ENABLED.with(|c| c.set(enabled));
    }

    /// Bound the slices one `park_hook` call may run (`None` = unbounded).
    pub(crate) fn set_test_max_slices(max: Option<u32>) {
        TEST_MAX_SLICES.with(|c| c.set(max));
    }

    /// Override the slice budget (`Some(0)` = exactly one stepper call per
    /// slice, so a cycle cannot complete inside one slice).
    pub(crate) fn set_test_slice_us(us: Option<u64>) {
        TEST_SLICE_US.with(|c| c.set(us));
    }

    /// Charge every slice this much wall time instead of its measured time,
    /// so the per-second work cap can be exercised under a pinned clock.
    pub(crate) fn set_test_work_charge_ms(ms: Option<u64>) {
        TEST_WORK_CHARGE_MS.with(|c| c.set(ms));
    }

    /// Forget this thread's reducer history.
    pub(crate) fn reset_state() {
        STATE.with(|s| *s.borrow_mut() = IdleReclaimState::default());
    }

    /// Pin the old-gen occupancy the in-flight full started with, so a
    /// completion can be priced against a known `before` without reproducing
    /// the allocation pattern that would produce one.
    pub(crate) fn set_old_in_use_at_start(bytes: usize) {
        STATE.with(|s| s.borrow_mut().old_in_use_at_start = bytes);
    }

    pub(crate) fn thread_attempts() -> u64 {
        STATE.with(|s| s.borrow().attempts)
    }

    /// Restores every override on drop.
    pub(crate) struct IdleReclaimTestGuard;

    impl IdleReclaimTestGuard {
        pub(crate) fn new(now_ms: u64) -> Self {
            // A stale notify from an earlier test would read as a wake pending
            // at every slice and starve the cycle; production consumes it
            // before the hook runs.
            crate::event_pump::clear_main_thread_notified_for_test();
            reset_state();
            super::super::arena_right_size::test_support::reset_state();
            super::super::arena_right_size::test_support::set_test_usage(Some(
                super::super::arena_right_size::ArenaUsage {
                    live_bytes: super::super::arena_right_size::ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES,
                    capacity_bytes:
                        super::super::arena_right_size::ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES,
                },
            ));
            set_test_now_ms(Some(now_ms));
            set_test_enabled(Some(true));
            set_test_max_slices(None);
            set_test_slice_us(None);
            set_test_work_charge_ms(None);
            Self
        }
    }

    impl Drop for IdleReclaimTestGuard {
        fn drop(&mut self) {
            set_test_now_ms(None);
            set_test_enabled(None);
            set_test_max_slices(None);
            set_test_slice_us(None);
            set_test_work_charge_ms(None);
            reset_state();
            super::super::arena_right_size::test_support::set_test_usage(None);
            super::super::arena_right_size::test_support::reset_state();
        }
    }
}
