//! Idle-time old-gen compaction: the half of the idle memory story a
//! non-moving cycle cannot reach.
//!
//! [`super::idle_reclaim`] gave the collector idle time and a full budgeted
//! cycle to spend it on. That cycle sweeps: it finds the dead old-gen objects
//! and hands them to the old-gen free list. What it cannot do is give the
//! memory back, because the free list lives INSIDE partially-occupied 1 MiB
//! blocks and a non-moving sweep never empties one. Measured on the compiled
//! claude-code TUI, idle after a paste, with the reducer running:
//!
//! ```text
//! [gc] blocks: non_general=143 (130 live)   <- identical across both cycles
//! [gc-old-free] reusable_bytes=49986912 -> 50095240
//! [gc-idle-reclaim] done old_in_use=93608664->93606360 freed=2370304
//! ```
//!
//! 50 MB of a 93.6 MB old gen: swept, dead, reusable for future old-gen
//! allocation, and unreachable to the OS. Emptying those blocks needs
//! EVACUATION — moving the surviving occupants of a fragmented page out so the
//! whole block can be released — and that rides on the non-copying minor
//! (`gc/oldgen.rs`, selection in `gc/oldgen_defrag.rs`), never on a full
//! mark-sweep: `GcCycleState::new_full` takes no page selection at all.
//!
//! Two structural reasons the existing machinery never runs on this workload,
//! both of which this module has to step around rather than through:
//!
//! 1. **A budgeted cycle may not move.** `evacuation_policy_allowed =
//!    !low_pause_non_moving`, and `low_pause_non_moving` is exactly
//!    `progress_kind.is_budgeted()`. The reducer's cycle is budgeted by
//!    construction — it is stepped in 4 ms wake-checked slices — so evacuation
//!    is off in it by definition. Compaction therefore runs as its own
//!    SYNCHRONOUS collection, not as a phase of the reducer's.
//! 2. **The copying fast path consumes every ordinary minor.** Old-page defrag
//!    is only selected on the non-copying fallback, and on this workload the
//!    fast path is eligible every time (`[gc-copy-minor] eligible=true
//!    fallback=none` on all 520 minors of the session above). So the
//!    compacting collection asks for the fallback explicitly
//!    ([`super::CopyingFastPath::Skipped`]).
//!
//! # When it runs
//!
//! At the park site, after [`super::idle_reclaim`]'s full has completed — the
//! sweep is what makes a page's `dead_bytes` accounting current, and selection
//! reads exactly that. Gates, all O(1):
//!
//! 1. **Residue.** The old-gen free list holds at least
//!    [`IDLE_COMPACT_MIN_RESIDUE_BYTES`] and at least
//!    [`IDLE_COMPACT_MIN_RESIDUE_PCT`] percent of old-gen occupancy. Below
//!    that there is nothing worth a moving pause.
//! 2. **Freshness.** At least `2^backoff` reducer fulls have completed since
//!    the last compaction, so selection never runs on stale page metadata and
//!    an unproductive compaction is not retried against an unchanged heap.
//! 3. **Rate.** At least [`IDLE_COMPACT_MIN_INTERVAL_MS`] since the last one.
//!
//! **Productivity** is old-gen occupancy — `old_gen_in_use_bytes`, the sum of
//! the live blocks' bump offsets. For a SWEEP that meter is wrong and
//! [`super::idle_reclaim`] stopped using it; for compaction it is exactly
//! right, because releasing whole blocks is the only thing that moves it and
//! releasing whole blocks is the entire point. Under
//! [`IDLE_COMPACT_PRODUCTIVE_MIN_BYTES`] the requirement doubles, to
//! `2^`[`IDLE_COMPACT_MAX_BACKOFF_SHIFT`] fulls.
//!
//! # The pause
//!
//! This collection is not sliceable: a moving cycle cannot hand a half-rewritten
//! heap back to the mutator. The hook therefore checks for a pending wake
//! immediately before starting one and does not start if input is already
//! waiting — but a keystroke that lands DURING one waits for it. That pause is
//! recorded (`pause_us_max` on the diag line) rather than assumed, and the
//! residue gate is what keeps these rare: on the session above, one compaction
//! per 50 MB of accumulated free list, not one per idle period.
//!
//! # Kill switch
//!
//! `PERRY_GC_IDLE_COMPACT=0` (default ON; `env_default_on_enabled`
//! vocabulary). Note this is a DIFFERENT decision from `PERRY_GC_OLD_DEFRAG`,
//! which stays opt-in for ordinary allocation-triggered collections (#7917):
//! what changes here is that idle time — where nobody is waiting on the
//! result — pays for compaction the throughput path still declines.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Free-list residue below which compaction is not worth a moving pause.
pub const IDLE_COMPACT_MIN_RESIDUE_BYTES: usize = 8 * 1024 * 1024;

/// ...and it must also be at least this percent of old-gen occupancy, so a
/// large live heap with a proportionally small hole is left alone.
pub const IDLE_COMPACT_MIN_RESIDUE_PCT: usize = 25;

/// Minimum spacing between two idle compactions.
pub const IDLE_COMPACT_MIN_INTERVAL_MS: u64 = 30_000;

/// A compaction counts as productive when old-gen occupancy fell by this much.
pub const IDLE_COMPACT_PRODUCTIVE_MIN_BYTES: usize = 4 * 1024 * 1024;

/// Cap on the unproductive-streak backoff: the freshness requirement never
/// exceeds `2^this` reducer fulls.
pub const IDLE_COMPACT_MAX_BACKOFF_SHIFT: u32 = 5;

#[derive(Default)]
struct IdleCompactState {
    /// Reducer fulls completed since the last compaction.
    fulls_since_compact: u64,
    /// Reducer clock (ms) at the last compaction.
    last_compact_ms: u64,
    /// Compactions run on this thread.
    attempts: u64,
    /// Unproductive-streak backoff; the freshness requirement is `1 << shift`.
    backoff_shift: u32,
}

thread_local! {
    static STATE: RefCell<IdleCompactState> = RefCell::new(IdleCompactState::default());
    #[cfg(test)]
    static TEST_ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
    /// Forces the gate answer, so the RUN path (wake check, collection,
    /// pricing) can be exercised without conjuring an 8 MiB fragmented old gen
    /// in the test process.
    #[cfg(test)]
    static TEST_FORCE_OWED: Cell<Option<bool>> = const { Cell::new(None) };
}

static ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static PRODUCTIVE: AtomicU64 = AtomicU64::new(0);
static RELEASED_BYTES: AtomicU64 = AtomicU64::new(0);
static PAUSE_US_TOTAL: AtomicU64 = AtomicU64::new(0);
static PAUSE_US_MAX: AtomicU64 = AtomicU64::new(0);
static WAKE_DECLINED: AtomicU64 = AtomicU64::new(0);

/// Idle compactions started in this process.
pub fn idle_compact_attempts() -> u64 {
    ATTEMPTS.load(Ordering::Relaxed)
}

/// Idle compactions that met the productivity bar.
pub fn idle_compact_productive() -> u64 {
    PRODUCTIVE.load(Ordering::Relaxed)
}

/// Old-gen occupancy the compactions released, summed. This is block-granule
/// memory handed back to the arena, not free-list bytes.
pub fn idle_compact_released_bytes() -> u64 {
    RELEASED_BYTES.load(Ordering::Relaxed)
}

/// Total time the mutator spent inside idle compactions.
pub fn idle_compact_pause_us_total() -> u64 {
    PAUSE_US_TOTAL.load(Ordering::Relaxed)
}

/// The longest single compaction pause — the number the keystroke-latency
/// question is actually about.
pub fn idle_compact_pause_us_max() -> u64 {
    PAUSE_US_MAX.load(Ordering::Relaxed)
}

/// Times an owed compaction was declined because a wake was already pending.
pub fn idle_compact_wake_declined() -> u64 {
    WAKE_DECLINED.load(Ordering::Relaxed)
}

/// Current unproductive-streak backoff shift on this thread.
pub fn idle_compact_backoff_shift() -> u32 {
    STATE.with(|s| s.borrow().backoff_shift)
}

/// `PERRY_GC_IDLE_COMPACT` — default ON; `0`/`off`/`false`/`no` disable.
pub fn idle_compact_enabled_from_value(raw: Option<&str>) -> bool {
    super::env_default_on_from_value(raw)
}

fn idle_compact_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_ENABLED.with(Cell::get) {
        return forced;
    }
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| super::env_default_on_enabled("PERRY_GC_IDLE_COMPACT"))
}

/// The old-gen free-list residue: swept-dead bytes sitting inside live blocks.
fn residue_bytes() -> usize {
    old_free_bytes()
}

/// Called by [`super::idle_reclaim`] when one of its fulls completes: the
/// sweep that just ran is what makes page `dead_bytes` current.
pub(super) fn note_reclaim_full_completed() {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.fulls_since_compact = st.fulls_since_compact.saturating_add(1);
    });
}

/// Everything the gate decision reads, so the decision itself is a pure
/// function of numbers and can be tested at every boundary in both directions
/// — the live heap cannot be asked for an 8 MiB free list on demand.
#[derive(Clone, Copy, Debug)]
pub(super) struct CompactionGateInputs {
    /// Old-gen free-list bytes: swept-dead, reusable, inside live blocks.
    pub(super) residue: usize,
    /// Old-gen occupancy (the sum of the live blocks' bump offsets).
    pub(super) occupancy: usize,
    /// Reducer fulls completed since the last compaction.
    pub(super) fulls_since_compact: u64,
    /// Compactions this thread has run.
    pub(super) attempts: u64,
    /// Reducer clock (ms) at the last compaction.
    pub(super) last_compact_ms: u64,
    /// Unproductive-streak backoff shift.
    pub(super) backoff_shift: u32,
    /// Reducer clock (ms) now.
    pub(super) now: u64,
}

/// The three gates, in order: residue, freshness, rate. See the module docs.
pub(super) fn compaction_owed(i: CompactionGateInputs) -> bool {
    if i.residue < IDLE_COMPACT_MIN_RESIDUE_BYTES
        || i.residue < i.occupancy / 100 * IDLE_COMPACT_MIN_RESIDUE_PCT
    {
        return false;
    }
    if i.fulls_since_compact < (1u64 << i.backoff_shift) {
        return false;
    }
    if i.attempts > 0 && i.now.saturating_sub(i.last_compact_ms) < IDLE_COMPACT_MIN_INTERVAL_MS {
        return false;
    }
    true
}

/// Is a compaction owed right now? Reads the live counters and defers to
/// [`compaction_owed`]. No collector state is touched, and the caller has
/// already established that the loop is quiet.
fn should_compact(now: u64) -> bool {
    if !idle_compact_enabled() {
        return false;
    }
    #[cfg(test)]
    if let Some(forced) = TEST_FORCE_OWED.with(Cell::get) {
        return forced;
    }
    let inputs = STATE.with(|s| {
        let st = s.borrow();
        CompactionGateInputs {
            residue: residue_bytes(),
            occupancy: crate::arena::old_gen_in_use_bytes(),
            fulls_since_compact: st.fulls_since_compact,
            attempts: st.attempts,
            last_compact_ms: st.last_compact_ms,
            backoff_shift: st.backoff_shift,
            now,
        }
    });
    compaction_owed(inputs)
}

/// Run one compacting collection if one is owed. Returns whether it ran, so
/// the park hook can report the timer budget it just invalidated.
pub(super) fn maybe_compact(now: u64) -> bool {
    if !should_compact(now) {
        return false;
    }
    // Not sliceable: a moving cycle cannot hand a half-rewritten heap back.
    // If input is already waiting, this is not idle time any more.
    if crate::event_pump::main_thread_wake_pending() {
        WAKE_DECLINED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    if policy::gc_root_lock_held() || policy::gc_budgeted_cycle_active() {
        return false;
    }

    let before_occupancy = crate::arena::old_gen_in_use_bytes();
    let before_residue = residue_bytes();
    if gc_diag_enabled() {
        eprintln!(
            "[gc-idle-compact] start attempt={} old_in_use={before_occupancy} reusable={before_residue} backoff_shift={}",
            STATE.with(|s| s.borrow().attempts) + 1,
            idle_compact_backoff_shift(),
        );
    }

    let start = Instant::now();
    let freed = super::gc_collect_compacting_minor(GcTriggerSnapshot::capture(
        GcTriggerKind::IdleCompact,
    ))
    .emit_after_current();
    let pause_us = start.elapsed().as_micros() as u64;

    let after_occupancy = crate::arena::old_gen_in_use_bytes();
    let after_residue = residue_bytes();
    let released = before_occupancy.saturating_sub(after_occupancy);
    let productive = released >= IDLE_COMPACT_PRODUCTIVE_MIN_BYTES;

    ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    RELEASED_BYTES.fetch_add(released as u64, Ordering::Relaxed);
    PAUSE_US_TOTAL.fetch_add(pause_us, Ordering::Relaxed);
    PAUSE_US_MAX.fetch_max(pause_us, Ordering::Relaxed);
    if productive {
        PRODUCTIVE.fetch_add(1, Ordering::Relaxed);
    }
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.attempts += 1;
        st.last_compact_ms = now;
        st.fulls_since_compact = 0;
        st.backoff_shift = if productive {
            0
        } else {
            (st.backoff_shift + 1).min(IDLE_COMPACT_MAX_BACKOFF_SHIFT)
        };
        if gc_diag_enabled() {
            eprintln!(
                "[gc-idle-compact] done old_in_use={before_occupancy}->{after_occupancy} released={released} \
                 reusable={before_residue}->{after_residue} freed={freed} pause_us={pause_us} \
                 productive={productive} backoff_shift={}",
                st.backoff_shift
            );
        }
    });

    // The released blocks were deallocated into mimalloc, which holds pages
    // until something asks. Nobody is waiting; ask now (see
    // `cycle_malloc_trim::purge_mimalloc_cache`).
    let _ = super::cycle_malloc_trim::run_malloc_trim(GcProgressKind::NormalIncremental);
    true
}

/// `PERRY_GC_DIAG=1` exit line.
pub(super) fn emit_diag() {
    eprintln!(
        "[gc-idle-compact] enabled={} attempts={} productive={} released_bytes={} \
         pause_us_total={} pause_us_max={} wake_declined={} backoff_shift={}",
        idle_compact_enabled(),
        idle_compact_attempts(),
        idle_compact_productive(),
        idle_compact_released_bytes(),
        idle_compact_pause_us_total(),
        idle_compact_pause_us_max(),
        idle_compact_wake_declined(),
        idle_compact_backoff_shift(),
    );
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    /// Pin the kill switch for this thread (`None` restores the env read).
    pub(crate) fn set_test_enabled(enabled: Option<bool>) {
        TEST_ENABLED.with(|c| c.set(enabled));
    }

    /// Forget this thread's compaction history.
    pub(crate) fn reset_state() {
        STATE.with(|s| *s.borrow_mut() = IdleCompactState::default());
    }

    /// Force the gate answer for this thread (`None` restores the gates).
    pub(crate) fn set_test_force_owed(owed: Option<bool>) {
        TEST_FORCE_OWED.with(|c| c.set(owed));
    }

    pub(crate) fn thread_attempts() -> u64 {
        STATE.with(|s| s.borrow().attempts)
    }

    /// Whether the gates say a compaction is owed at `now` — the decision
    /// under test, separated from running one.
    pub(crate) fn gates_say_compact(now: u64) -> bool {
        should_compact(now)
    }

    /// The pure gate decision, so every boundary can be tested in both
    /// directions without an 8 MiB fragmented heap per case.
    pub(crate) fn owed(inputs: CompactionGateInputs) -> bool {
        compaction_owed(inputs)
    }

    /// Gate inputs that pass every gate, as the starting point a case then
    /// perturbs in exactly one dimension.
    pub(crate) fn passing_inputs() -> CompactionGateInputs {
        CompactionGateInputs {
            residue: IDLE_COMPACT_MIN_RESIDUE_BYTES,
            occupancy: IDLE_COMPACT_MIN_RESIDUE_BYTES * 4,
            fulls_since_compact: 1,
            attempts: 0,
            last_compact_ms: 0,
            backoff_shift: 0,
            now: IDLE_COMPACT_MIN_INTERVAL_MS,
        }
    }

    pub(crate) fn maybe_compact_at(now: u64) -> bool {
        maybe_compact(now)
    }

    /// Restores every override on drop.
    pub(crate) struct IdleCompactTestGuard;

    impl IdleCompactTestGuard {
        pub(crate) fn new() -> Self {
            reset_state();
            set_test_enabled(Some(true));
            set_test_force_owed(None);
            Self
        }
    }

    impl Drop for IdleCompactTestGuard {
        fn drop(&mut self) {
            set_test_enabled(None);
            set_test_force_owed(None);
            reset_state();
        }
    }
}
