//! Arena right-sizing policy: sustained-low-utilization detection, full-pass
//! accounting, and the hysteresis that bounds idle work (#9709).

use super::super::arena_right_size::test_support::*;
use super::super::arena_right_size::*;

const MIB: usize = 1024 * 1024;

fn usage(live_bytes: usize, capacity_bytes: usize) -> ArenaUsage {
    ArenaUsage {
        live_bytes,
        capacity_bytes,
    }
}

fn low_usage(capacity_bytes: usize) -> ArenaUsage {
    usage(
        capacity_bytes * ARENA_RIGHT_SIZE_TRIGGER_PCT / 100,
        capacity_bytes,
    )
}

#[test]
fn low_utilization_must_persist_and_the_capacity_floor_is_strict() {
    let _guard = ArenaRightSizeTestGuard::new();
    let capacity = 100 * MIB;

    observe(low_usage(capacity), false);
    assert!(!owed(), "one low collection is only a transient");
    assert_eq!(state_snapshot().0, 1);

    observe(low_usage(capacity), false);
    assert!(owed(), "two consecutive low collections open an episode");
    assert_eq!(state_snapshot().1, ARENA_RIGHT_SIZE_FULL_OBSERVATIONS);

    reset_state();
    let floor = ARENA_RIGHT_SIZE_MIN_CAPACITY_BYTES;
    observe(low_usage(floor), false);
    observe(low_usage(floor), false);
    assert!(!owed(), "capacity exactly at the floor is left alone");

    reset_state();
    let one_over_trigger = usage(capacity * ARENA_RIGHT_SIZE_TRIGGER_PCT / 100 + 1, capacity);
    observe(one_over_trigger, false);
    observe(one_over_trigger, false);
    assert!(
        !owed(),
        "one byte above the utilization trigger resets the streak"
    );
}

#[test]
fn fulls_already_in_the_low_streak_count_toward_block_release() {
    let _guard = ArenaRightSizeTestGuard::new();
    let low = low_usage(100 * MIB);

    observe(low, false);
    observe(low, true);
    assert_eq!(
        state_snapshot().1,
        ARENA_RIGHT_SIZE_FULL_OBSERVATIONS - 1,
        "the full that established sustained slack is the first release observation"
    );

    reset_state();
    observe(low, false);
    observe(low, false);
    assert_eq!(
        state_snapshot().1,
        ARENA_RIGHT_SIZE_FULL_OBSERVATIONS,
        "minor samples establish utilization but do not impersonate full sweeps"
    );

    reset_state();
    observe(low, true);
    observe(low, true);
    assert!(
        !owed(),
        "two full observations already paid the bounded episode"
    );
    assert!(state_snapshot().2, "unchanged low utilization disarms it");
}

#[test]
fn an_episode_is_bounded_and_utilization_hysteresis_rearms_it() {
    let _guard = ArenaRightSizeTestGuard::new();
    let capacity = 100 * MIB;
    let low = low_usage(capacity);

    observe(low, false);
    observe(low, false);
    assert_eq!(state_snapshot().1, 2);
    observe(low, true);
    assert_eq!(state_snapshot().1, 1);
    observe(low, true);
    assert!(!owed());
    assert!(state_snapshot().2, "two fulls are the hard work bound");

    for _ in 0..8 {
        observe(low, false);
    }
    assert!(
        !owed(),
        "a stable low heap must not buy another episode on periodic minors"
    );

    let rearmed = usage(capacity * ARENA_RIGHT_SIZE_REARM_PCT / 100, capacity);
    observe(rearmed, false);
    assert!(!state_snapshot().2, "the re-arm watermark ends hysteresis");
    observe(low, false);
    observe(low, false);
    assert!(owed(), "a later low-utilization epoch gets its own episode");
}

#[test]
fn material_capacity_regrowth_rearms_without_requiring_a_large_live_set() {
    let _guard = ArenaRightSizeTestGuard::new();
    let capacity = 100 * MIB;
    let low = low_usage(capacity);

    // Two low FULL samples consume the episode immediately and disarm it at
    // `capacity`, without needing to start a synthetic collector in this pure
    // policy test.
    observe(low, true);
    observe(low, true);
    assert!(state_snapshot().2);

    let growth = (capacity * ARENA_RIGHT_SIZE_REARM_GROWTH_PCT / 100)
        .max(ARENA_RIGHT_SIZE_REARM_GROWTH_MIN_BYTES);
    observe(low_usage(capacity + growth - 1), false);
    assert!(state_snapshot().2, "one byte short of material growth");
    observe(low_usage(capacity + growth), false);
    assert!(!state_snapshot().2, "a real new capacity peak re-arms");
    assert_eq!(
        state_snapshot().0,
        1,
        "the re-arm sample starts the new streak"
    );
    observe(low_usage(capacity + growth), false);
    assert!(owed());
}

#[test]
fn reaching_the_target_stops_early_and_records_capacity_released() {
    let _guard = ArenaRightSizeTestGuard::new();
    let start_capacity = 100 * MIB;
    let low = low_usage(start_capacity);
    let released_before = arena_right_size_released_capacity_bytes();

    observe(low, false);
    observe(low, false);
    assert!(owed());

    let target_capacity = 70 * MIB;
    let target = usage(
        target_capacity * ARENA_RIGHT_SIZE_TARGET_PCT / 100,
        target_capacity,
    );
    observe(target, true);

    assert!(!owed(), "the target band cancels the remaining full");
    assert_eq!(
        arena_right_size_released_capacity_bytes(),
        released_before + (start_capacity - target_capacity) as u64
    );
    assert!(
        state_snapshot().2,
        "target is deliberately below the higher re-arm watermark"
    );
}
