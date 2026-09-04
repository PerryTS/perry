//! Arena capacity right-sizing for idle heaps.
//!
//! Reclaiming dead objects and returning arena capacity are deliberately two
//! different operations. General-arena blocks are released only after two
//! full collection observations: the first resets a proven-empty block and
//! the second proves that the mutator did not reuse it. The idle reducer used
//! to subtract its own fulls from its activity clock, so a burst followed by
//! complete silence received one full and then parked forever. Empty blocks
//! stopped at `dead_cycles == 1`, even when live bytes occupied a small
//! fraction of reserved arena capacity.
//!
//! This module turns sustained post-collection slack into a bounded debt that
//! [`super::idle_reclaim`] may service without new mutator activity:
//!
//! * capacity must be above [`ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES`];
//! * live bytes must be at or below [`ARENA_RIGHT_SIZE_TRIGGER_PCT`] percent
//!   for [`ARENA_RIGHT_SIZE_LOW_COLLECTIONS`] consecutive collections;
//! * the episode asks for enough idle fulls to reach
//!   [`ARENA_RIGHT_SIZE_FULL_OBSERVATIONS`] full observations, counting fulls
//!   already present in that low-utilization streak;
//! * it stops early in the target band and is bounded even when fragmentation
//!   prevents a block release.
//!
//! The start and stop bands are intentionally different. After an episode
//! stops below the re-arm watermark, another cannot begin until utilization
//! rises to [`ARENA_RIGHT_SIZE_REARM_PCT`] percent or reserved capacity grows
//! materially. That hysteresis is the burst-then-idle-then-burst protection:
//! stable low occupancy cannot buy a full every timer interval, while a real
//! new peak can earn another right-size episode.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Small heaps do not buy extra whole-heap work merely to save a few blocks.
pub const ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES: usize = 32 * 1024 * 1024;

/// Open a right-size episode at or below this live/capacity percentage.
pub const ARENA_RIGHT_SIZE_TRIGGER_PCT: usize = 50;

/// Stop an episode once live bytes reach this share of reserved capacity.
pub const ARENA_RIGHT_SIZE_TARGET_PCT: usize = 60;

/// A completed episode below this utilization stays disarmed until the heap
/// grows materially. This gap from the trigger is the utilization hysteresis.
pub const ARENA_RIGHT_SIZE_REARM_PCT: usize = 70;

/// Number of consecutive low-utilization collection results required.
pub const ARENA_RIGHT_SIZE_LOW_COLLECTIONS: u32 = 2;

/// Full observations needed for the arena's two-cycle empty-block release.
pub const ARENA_RIGHT_SIZE_FULL_OBSERVATIONS: u8 = 2;

/// Capacity growth that re-arms a disarmed episode is at least this many
/// bytes, as well as at least [`ARENA_RIGHT_SIZE_REARM_GROWTH_PCT`] percent of
/// the capacity at which it disarmed.
pub const ARENA_RIGHT_SIZE_REARM_GROWTH_MIN_BYTES: usize = 8 * 1024 * 1024;

/// See [`ARENA_RIGHT_SIZE_REARM_GROWTH_MIN_BYTES`].
pub const ARENA_RIGHT_SIZE_REARM_GROWTH_PCT: usize = 25;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ArenaUsage {
    pub(super) live_bytes: usize,
    pub(super) capacity_bytes: usize,
}

#[derive(Default)]
struct ArenaRightSizeState {
    /// Consecutive post-collection samples in the trigger band.
    low_collections: u32,
    /// Full samples among `low_collections`, capped at the required count.
    low_full_observations: u8,
    /// Full collections still owed by the active episode.
    fulls_remaining: u8,
    /// A bounded episode finished without returning to the re-arm band.
    disarmed: bool,
    /// Capacity at disarm, used to recognize a later, material new peak.
    disarmed_capacity_bytes: usize,
    /// Capacity at the beginning of an active episode.
    episode_start_capacity_bytes: usize,
    /// Last post-collection sample, for diagnostics.
    last_usage: ArenaUsage,
}

crate::perry_thread_local! {
    static STATE: RefCell<ArenaRightSizeState> =
        RefCell::new(ArenaRightSizeState::default());
    #[cfg(test)]
    static TEST_USAGE: Cell<Option<ArenaUsage>> = const { Cell::new(None) };
}

static EPISODES: AtomicU64 = AtomicU64::new(0);
static STARTS: AtomicU64 = AtomicU64::new(0);
static RELEASED_CAPACITY_BYTES: AtomicU64 = AtomicU64::new(0);

/// Sustained-low-utilization episodes opened in this process.
pub fn arena_right_size_episodes() -> u64 {
    EPISODES.load(Ordering::Relaxed)
}

/// Idle fulls started specifically to service arena right-size debt.
pub fn arena_right_size_starts() -> u64 {
    STARTS.load(Ordering::Relaxed)
}

/// Reserved arena bytes removed over completed right-size episodes.
pub fn arena_right_size_released_capacity_bytes() -> u64 {
    RELEASED_CAPACITY_BYTES.load(Ordering::Relaxed)
}

#[inline]
fn utilization_at_most(usage: ArenaUsage, pct: usize) -> bool {
    (usage.live_bytes as u128) * 100 <= (usage.capacity_bytes as u128) * (pct as u128)
}

#[inline]
fn utilization_at_least(usage: ArenaUsage, pct: usize) -> bool {
    (usage.live_bytes as u128) * 100 >= (usage.capacity_bytes as u128) * (pct as u128)
}

fn in_trigger_band(usage: ArenaUsage) -> bool {
    usage.capacity_bytes > ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES
        && utilization_at_most(usage, ARENA_RIGHT_SIZE_TRIGGER_PCT)
}

fn at_target(usage: ArenaUsage) -> bool {
    usage.capacity_bytes <= ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES
        || utilization_at_least(usage, ARENA_RIGHT_SIZE_TARGET_PCT)
}

fn materially_regrew(usage: ArenaUsage, disarmed_capacity_bytes: usize) -> bool {
    let relative = disarmed_capacity_bytes.saturating_mul(ARENA_RIGHT_SIZE_REARM_GROWTH_PCT) / 100;
    let required = relative.max(ARENA_RIGHT_SIZE_REARM_GROWTH_MIN_BYTES);
    usage.capacity_bytes.saturating_sub(disarmed_capacity_bytes) >= required
}

fn rearm_reached(usage: ArenaUsage, disarmed_capacity_bytes: usize) -> bool {
    utilization_at_least(usage, ARENA_RIGHT_SIZE_REARM_PCT)
        || materially_regrew(usage, disarmed_capacity_bytes)
}

fn current_usage(live_bytes: usize) -> ArenaUsage {
    #[cfg(test)]
    if let Some(usage) = TEST_USAGE.with(Cell::get) {
        return usage;
    }
    ArenaUsage {
        live_bytes,
        capacity_bytes: crate::arena::arena_total_bytes(),
    }
}

fn reset_low_streak(st: &mut ArenaRightSizeState) {
    st.low_collections = 0;
    st.low_full_observations = 0;
}

fn finish_episode(st: &mut ArenaRightSizeState, usage: ArenaUsage) {
    let released = st
        .episode_start_capacity_bytes
        .saturating_sub(usage.capacity_bytes);
    RELEASED_CAPACITY_BYTES.fetch_add(released as u64, Ordering::Relaxed);
    st.fulls_remaining = 0;
    st.episode_start_capacity_bytes = 0;
    reset_low_streak(st);
    st.disarmed = !utilization_at_least(usage, ARENA_RIGHT_SIZE_REARM_PCT);
    st.disarmed_capacity_bytes = if st.disarmed { usage.capacity_bytes } else { 0 };
}

/// Observe the exact post-collection live census and current reserved block
/// capacity. `full` is true only when this collection supplied a whole-heap
/// sweep observation; copied and non-moving minors are still utilization
/// samples but cannot satisfy the two full observations by themselves.
pub(super) fn note_collection_finished(live_bytes: usize, full: bool) {
    let usage = current_usage(live_bytes);
    STATE.with(|state| {
        let mut st = state.borrow_mut();
        st.last_usage = usage;

        if st.disarmed {
            if !rearm_reached(usage, st.disarmed_capacity_bytes) {
                return;
            }
            st.disarmed = false;
            st.disarmed_capacity_bytes = 0;
            reset_low_streak(&mut st);
        }

        if st.fulls_remaining != 0 {
            if at_target(usage) {
                finish_episode(&mut st, usage);
                return;
            }
            if full {
                st.fulls_remaining = st.fulls_remaining.saturating_sub(1);
                if st.fulls_remaining == 0 {
                    // Fragmentation or the protected recent-block window can
                    // make two full observations unable to hit the target.
                    // End the bounded episode instead of collecting forever.
                    finish_episode(&mut st, usage);
                }
            }
            return;
        }

        if !in_trigger_band(usage) {
            reset_low_streak(&mut st);
            return;
        }

        st.low_collections = st
            .low_collections
            .saturating_add(1)
            .min(ARENA_RIGHT_SIZE_LOW_COLLECTIONS);
        if full {
            st.low_full_observations = st
                .low_full_observations
                .saturating_add(1)
                .min(ARENA_RIGHT_SIZE_FULL_OBSERVATIONS);
        }
        if st.low_collections < ARENA_RIGHT_SIZE_LOW_COLLECTIONS {
            return;
        }

        st.episode_start_capacity_bytes = usage.capacity_bytes;
        st.fulls_remaining =
            ARENA_RIGHT_SIZE_FULL_OBSERVATIONS.saturating_sub(st.low_full_observations);
        reset_low_streak(&mut st);
        EPISODES.fetch_add(1, Ordering::Relaxed);
        if st.fulls_remaining == 0 {
            finish_episode(&mut st, usage);
        }
    });
}

/// Whether the idle reducer may start a full without new mutator collection
/// activity. Quiet-time and rate gates remain the reducer's responsibility.
pub(super) fn owed() -> bool {
    STATE.with(|state| state.borrow().fulls_remaining != 0)
}

/// Record that the idle reducer successfully opened a full for this debt.
pub(super) fn note_started() {
    STARTS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn snapshot() -> (u32, u8, bool, ArenaUsage) {
    STATE.with(|state| {
        let st = state.borrow();
        (
            st.low_collections,
            st.fulls_remaining,
            st.disarmed,
            st.last_usage,
        )
    })
}

/// `PERRY_GC_DIAG=1` exit line.
pub(super) fn emit_diag() {
    let (low_collections, fulls_remaining, disarmed, usage) = snapshot();
    eprintln!(
        "[gc-arena-right-size] episodes={} starts={} released_capacity_bytes={} \
         low_collections={} fulls_remaining={} disarmed={} arena_live={} arena_capacity={}",
        arena_right_size_episodes(),
        arena_right_size_starts(),
        arena_right_size_released_capacity_bytes(),
        low_collections,
        fulls_remaining,
        disarmed,
        usage.live_bytes,
        usage.capacity_bytes,
    );
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    pub(crate) fn set_test_usage(usage: Option<ArenaUsage>) {
        TEST_USAGE.with(|cell| cell.set(usage));
    }

    pub(crate) fn observe(usage: ArenaUsage, full: bool) {
        set_test_usage(Some(usage));
        note_collection_finished(usage.live_bytes, full);
    }

    pub(crate) fn reset_state() {
        STATE.with(|state| *state.borrow_mut() = ArenaRightSizeState::default());
    }

    pub(crate) fn state_snapshot() -> (u32, u8, bool, ArenaUsage) {
        snapshot()
    }

    pub(crate) struct ArenaRightSizeTestGuard;

    impl ArenaRightSizeTestGuard {
        pub(crate) fn new() -> Self {
            reset_state();
            set_test_usage(Some(ArenaUsage {
                live_bytes: ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES,
                capacity_bytes: ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES,
            }));
            Self
        }
    }

    impl Drop for ArenaRightSizeTestGuard {
        fn drop(&mut self) {
            set_test_usage(None);
            reset_state();
        }
    }
}
