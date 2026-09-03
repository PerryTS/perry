//! Idle-time old-gen compaction (`gc/idle_compact.rs`): the gate decision at
//! every boundary in both directions, and the two pieces of routing the whole
//! mechanism depends on — the collection must decline the copying fast path,
//! and it must arm old-page defrag selection without turning it on for the
//! throughput path (#7917).
//!
//! The "does it hand memory back" proof is a live subject with a real
//! fragmented heap and real roots, in
//! `crates/perry/tests/issue_9644_idle_compaction_releases_blocks.rs`. Marks
//! here would have to be hand-written, and a collection that recomputes them
//! from roots would call every hand-marked object dead — which would release
//! the block for the wrong reason and prove nothing about compaction.

use super::super::idle_compact::test_support::*;
use super::super::idle_compact::{
    IDLE_COMPACT_MIN_INTERVAL_MS, IDLE_COMPACT_MIN_RESIDUE_BYTES, IDLE_COMPACT_MIN_RESIDUE_PCT,
};
use super::super::*;
use super::support::*;

/// Every gate case is this, perturbed in exactly one dimension.
fn passing() -> super::super::idle_compact::CompactionGateInputs {
    passing_inputs()
}

#[test]
fn gates_pass_when_every_condition_is_met() {
    assert!(owed(passing()), "the fixture's own premise");
}

#[test]
fn residue_below_the_floor_is_not_worth_a_moving_pause() {
    let mut i = passing();
    i.residue = IDLE_COMPACT_MIN_RESIDUE_BYTES - 1;
    i.occupancy = 0; // percentage gate cannot be what refuses it
    assert!(!owed(i), "one byte under the floor");

    i.residue = IDLE_COMPACT_MIN_RESIDUE_BYTES;
    assert!(owed(i), "exactly the floor is enough");
}

#[test]
fn a_small_hole_in_a_large_live_heap_is_left_alone() {
    let mut i = passing();
    // Over the byte floor, under the percentage: a 10 MiB free list inside a
    // 1 GiB old gen is not fragmentation worth moving objects for.
    i.residue = IDLE_COMPACT_MIN_RESIDUE_BYTES + 2 * 1024 * 1024;
    i.occupancy = 1024 * 1024 * 1024;
    assert!(!owed(i));

    i.occupancy = i.residue * 100 / IDLE_COMPACT_MIN_RESIDUE_PCT;
    assert!(owed(i), "at exactly the percentage bar it is owed");
}

#[test]
fn freshness_requires_a_reducer_full_since_the_last_compaction() {
    let mut i = passing();
    i.fulls_since_compact = 0;
    assert!(!owed(i), "selection would read stale page metadata");
    i.fulls_since_compact = 1;
    assert!(owed(i));
}

#[test]
fn the_backoff_doubles_the_freshness_requirement() {
    let mut i = passing();
    i.backoff_shift = 3;
    i.fulls_since_compact = 7;
    assert!(!owed(i), "2^3 fulls are owed, 7 have completed");
    i.fulls_since_compact = 8;
    assert!(owed(i));
}

#[test]
fn the_rate_gate_spaces_compactions_but_never_blocks_the_first() {
    let mut i = passing();
    i.attempts = 1;
    i.last_compact_ms = 1_000;
    i.now = 1_000 + IDLE_COMPACT_MIN_INTERVAL_MS - 1;
    assert!(!owed(i), "one ms short of the interval");
    i.now = 1_000 + IDLE_COMPACT_MIN_INTERVAL_MS;
    assert!(owed(i));

    // The first compaction has no predecessor to be spaced from.
    i.attempts = 0;
    i.now = 0;
    assert!(owed(i));
}

#[test]
fn the_kill_switch_maps_the_documented_values() {
    assert!(idle_compact_enabled_from_value(None));
    assert!(idle_compact_enabled_from_value(Some("1")));
    for off in ["0", "off", "false", "no"] {
        assert!(
            !idle_compact_enabled_from_value(Some(off)),
            "PERRY_GC_IDLE_COMPACT={off} must disable"
        );
    }
}

#[test]
fn the_kill_switch_refuses_the_gates_whatever_the_heap_says() {
    let _guard = IdleCompactTestGuard::new();
    // The guard turns it ON; the gates then answer on the live heap. Turn it
    // OFF and the answer must be no regardless of what that heap is.
    set_test_enabled(Some(false));
    assert!(!gates_say_compact(u64::MAX));
    assert_eq!(thread_attempts(), 0);
}

/// The routing the mechanism stands on: old-page defrag is selected ONLY on
/// the non-copying fallback, and on a TUI workload the copying fast path is
/// eligible every time. A compacting collection that let it run would sweep
/// the nursery and compact nothing.
#[test]
fn the_compacting_collection_declines_the_copying_fast_path() {
    let _isolation = copying_nursery_isolation_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let trace = super::super::gc_collect_compacting_minor(GcTriggerSnapshot {
        kind: GcTriggerKind::IdleCompact,
        steps_before: Some(GcStepSnapshot::current()),
    })
    .trace
    .expect("test requested GC trace capture");

    assert!(
        !trace.copying_nursery.eligible,
        "the compacting collection must reach the non-copying fallback"
    );
    assert_eq!(
        trace.copying_nursery.fallback_reason,
        CopiedMinorFallbackReason::IdleCompaction,
        "and it must say why, so a real decline is not mistaken for this one"
    );
}

/// The arm is scoped to the compaction and does not turn defrag on for the
/// allocation-triggered collections #7917 left opt-in.
#[test]
fn the_defrag_arm_is_scoped_to_one_collection() {
    let _isolation = copying_nursery_isolation_lock();
    let _off = OldDefragTestDisable::new();
    assert!(
        select_old_page_defrag_pages(true).candidate_pages == 0,
        "premise: with defrag disabled, selection short-circuits before the snapshot"
    );
    drop(_off);

    {
        let _armed = super::super::oldgen_defrag::IdleCompactDefragArm::new();
        // Armed: selection runs (it may still select nothing — what is under
        // test is that the env gate no longer short-circuits it).
        let _selection = select_old_page_defrag_pages(false);
    }
    let _off_again = OldDefragTestDisable::new();
    assert_eq!(
        select_old_page_defrag_pages(true).candidate_pages,
        0,
        "the arm must not outlive its collection"
    );
}

/// The compaction is a MOVING collection and cannot be sliced: a wake that is
/// already pending means this is not idle time any more, and the hook must not
/// start one. Without this check a keystroke that arrived while the hook was
/// deciding would wait for a whole compaction.
#[test]
fn a_pending_wake_declines_an_owed_compaction() {
    let _isolation = copying_nursery_isolation_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _guard = IdleCompactTestGuard::new();
    set_test_force_owed(Some(true));

    let declined_before = idle_compact_wake_declined();
    let attempts_before = idle_compact_attempts();
    crate::event_pump::js_notify_main_thread();

    assert!(!maybe_compact_at(0), "a pending wake must decline");
    assert_eq!(
        idle_compact_wake_declined(),
        declined_before + 1,
        "and it must say so in a counter, not silently"
    );
    assert_eq!(idle_compact_attempts(), attempts_before);

    crate::event_pump::clear_main_thread_notified_for_test();
}

/// The run path end to end: an owed compaction with no wake pending runs a
/// collection, counts itself, and records the pause it cost.
#[test]
fn an_owed_compaction_runs_and_records_its_pause() {
    let _isolation = copying_nursery_isolation_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _guard = IdleCompactTestGuard::new();
    set_test_force_owed(Some(true));
    crate::event_pump::clear_main_thread_notified_for_test();

    let attempts_before = idle_compact_attempts();
    assert!(maybe_compact_at(0), "nothing is blocking an owed compaction");
    assert_eq!(idle_compact_attempts(), attempts_before + 1);
    assert_eq!(thread_attempts(), 1);
    assert!(
        idle_compact_pause_us_max() > 0,
        "a real collection ran, so its pause must have been measured"
    );

    // The rate gate now applies to the next one.
    set_test_force_owed(None);
    assert!(!gates_say_compact(IDLE_COMPACT_MIN_INTERVAL_MS - 1));
}
