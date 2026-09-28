//! RFC deferred collection S5 — the allocation-point invariant (D2).
//!
//! These tests hold the runtime to "an allocation never starts a phase that
//! reads frame roots precisely, and never starts a moving phase", and show the
//! declared poll picks up what the allocation point declined. Each asserts its
//! subject was live (the phase really was reached, the poll really served it)
//! rather than only that nothing broke.

use super::super::alloc_point;
use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

fn live_test_string(bytes: &'static [u8]) -> usize {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

fn cycle_phase() -> Option<u32> {
    let mut status = JsGcStepResult::default();
    (js_gc_step_status(&mut status) == JS_GC_STEP_STATUS_ACTIVE).then_some(status.phase)
}

/// Start a budgeted (legacy-pacing) cycle from an allocation point and walk it
/// with allocation-point assists alone until it stops advancing. Returns the
/// phase it parked at.
fn assist_until_parked(max_assists: usize) -> u32 {
    let mut last = None;
    let mut stalled = 0;
    for _ in 0..max_assists {
        gc_check_trigger();
        let phase = cycle_phase().expect("the budgeted cycle must still be active");
        if Some(phase) == last {
            stalled += 1;
            if stalled >= 3 && alloc_point::root_phase_parked() {
                return phase;
            }
        } else {
            stalled = 0;
        }
        last = Some(phase);
    }
    panic!("allocation-point assists never parked the cycle (last phase {last:?})");
}

fn start_assist_cycle(label: &'static [u8]) -> GcTriggerThresholdTestGuard {
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    let live = live_test_string(label);
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();
    gc_check_trigger();
    assert!(
        gc_budgeted_cycle_active(),
        "the allocation point must have started a budgeted cycle (the subject of every test here)"
    );
    trigger_guard
}

/// The whole contract, end to end: allocation-point assists advance the
/// heap-only phases, park at BOTH frame-root phases (`RootScan` and the final
/// remark) with the poll armed, and a declared poll serves each one; the cycle
/// then completes and the rooted value survives.
#[test]
fn assists_park_at_both_root_phases_and_the_poll_serves_them() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    alloc_point::reset_alloc_point_counters();
    set_safepoint_pending(false);
    let _trigger = start_assist_cycle(b"d2_both_root_phases_live");

    // Phase 1: the root scan.
    let parked = assist_until_parked(10_000);
    assert_eq!(
        parked,
        GcCyclePhase::RootScan.ffi_code(),
        "the first frame-root phase an assist reaches is the root scan"
    );
    assert!(
        GC_SAFEPOINT_PENDING.with(std::cell::Cell::get),
        "parking must arm the poll, or nothing ever serves the phase"
    );
    let parked_count = alloc_point::alloc_point_counters().root_phases_parked;
    assert!(parked_count >= 1);
    // More allocation does not move it.
    for _ in 0..32 {
        gc_check_trigger();
    }
    assert_eq!(cycle_phase(), Some(GcCyclePhase::RootScan.ffi_code()));

    assert!(
        gc_safepoint_moving_minor(),
        "the poll must handle the parked phase"
    );
    let served = alloc_point::alloc_point_counters().root_phases_served_at_poll;
    assert_eq!(served, 1, "the poll served exactly the parked root scan");
    assert_ne!(cycle_phase(), Some(GcCyclePhase::RootScan.ffi_code()));
    assert!(!alloc_point::root_phase_parked());

    // Phase 2: the remark. Assists carry the cycle through marking and park
    // again before the remark.
    let parked = assist_until_parked(500_000);
    assert_eq!(
        parked,
        GcCyclePhase::AtomicFinalize.ffi_code(),
        "the second frame-root phase is the final remark"
    );
    assert!(gc_safepoint_moving_minor());
    assert_eq!(
        alloc_point::alloc_point_counters().root_phases_served_at_poll,
        2
    );

    // The rest is heap-only and completes from assists alone.
    let before = gc_collection_count();
    for _ in 0..500_000 {
        gc_check_trigger();
        if !gc_budgeted_cycle_active() {
            break;
        }
    }
    assert!(
        !gc_budgeted_cycle_active(),
        "the heap-only tail completes from assists"
    );
    assert!(gc_collection_count() > before);
    assert_eq!(alloc_point::alloc_point_counters().parked_valve_fires, 0);
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"d2_both_root_phases_live");
    }
}

/// Sabotage for the synchronous chokepoint: a precise collection started from
/// inside an allocation-point evaluation must fail loudly, not run. (Every
/// real allocation-point arm forces the conservative scan first, so the only
/// way to reach this is to plant the violation.)
#[test]
#[should_panic(expected = "invariant D2 violated")]
fn a_precise_collection_begun_at_an_allocation_point_panics() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _disabled = ConservativeScanDisabledGuard::new();
    let _alloc_point = alloc_point::AllocationPointGuard::enter();
    let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

/// The same chokepoint does NOT fire for the arms D2 allows: a conservative
/// collection at an allocation point runs normally.
#[test]
fn a_conservative_collection_at_an_allocation_point_is_allowed() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let before = gc_collection_count();
    {
        let _alloc_point = alloc_point::AllocationPointGuard::enter();
        // The request is what D2 checks; the isolation guard above has pinned
        // the scan decision itself, exactly as it does for every A-old/valve
        // test in this crate.
        let _scan =
            ManualGcScanGuard::force_full_scan(ConservativeScanSite::NurseryChurnSlackValve);
        let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    }
    assert!(gc_collection_count() > before);
    assert_eq!(alloc_point::alloc_point_counters().d2_violations, 0);
}

/// "D stops collecting": a collection requested while a root lock is held is
/// not run at the lock exit (an arbitrary point inside a runtime helper) but
/// handed to the next declared poll.
#[test]
fn root_lock_exit_hands_an_owed_collection_to_the_poll() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    alloc_point::reset_alloc_point_counters();
    set_safepoint_pending(false);
    let before = gc_collection_count();
    super::super::roots::enter_gc_root_lock();
    assert_eq!(gc_collect_minor(), 0, "a locked collection defers");
    super::super::roots::exit_gc_root_lock();
    assert_eq!(
        gc_collection_count(),
        before,
        "the lock exit must not collect (it is not a declared poll)"
    );
    assert!(super::super::policy::poll_owed_request_pending());
    assert!(GC_SAFEPOINT_PENDING.with(std::cell::Cell::get));
    assert_eq!(alloc_point::alloc_point_counters().owed_requests_routed, 1);

    assert!(gc_safepoint_moving_minor());
    assert!(
        gc_collection_count() > before,
        "the poll ran the owed collection"
    );
    assert!(!super::super::policy::poll_owed_request_pending());
    assert_eq!(alloc_point::alloc_point_counters().owed_requests_served, 1);
}

/// The parked-cycle valve: an allocation point that has grown the slack past
/// the park point with no poll serves the phase itself, and says so.
#[test]
fn parked_cycle_valve_fires_after_the_slack_and_is_counted() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    alloc_point::reset_alloc_point_counters();
    set_safepoint_pending(false);
    let _trigger = start_assist_cycle(b"d2_parked_valve_live");
    assert_eq!(
        assist_until_parked(10_000),
        GcCyclePhase::RootScan.ffi_code()
    );

    // Pretend the program allocated the whole slack since parking.
    alloc_point::test_make_parked_valve_due();
    gc_check_trigger();
    alloc_point::test_clear_parked_valve_override();

    let counters = alloc_point::alloc_point_counters();
    assert_eq!(counters.parked_valve_fires, 1, "the valve fired once");
    assert_ne!(
        cycle_phase(),
        Some(GcCyclePhase::RootScan.ffi_code()),
        "the valve served the phase"
    );
    assert!(counters.d2_violations == 0);
    assert!(alloc_point::alloc_point_exit_line().contains("parked_valve_fires=1"));
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
}

/// Decision 10: growth inside an unsafe zone is counted (diagnostic only).
#[test]
fn block_growth_inside_an_unsafe_zone_is_counted() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    alloc_point::reset_alloc_point_counters();
    let before = alloc_point::alloc_point_counters();
    assert_eq!(before.unsafe_zone_growth_events, 0);
    let previous =
        super::super::policy::unsafe_zone_test_override::set_unsafe_zone_blocked_for_test(Some(
            true,
        ));
    crate::gc::note_block_if_unsafe_zone(1 << 20);
    super::super::policy::unsafe_zone_test_override::set_unsafe_zone_blocked_for_test(Some(false));
    crate::gc::note_block_if_unsafe_zone(1 << 20);
    super::super::policy::unsafe_zone_test_override::set_unsafe_zone_blocked_for_test(previous);
    let after = alloc_point::alloc_point_counters();
    assert_eq!(
        after.unsafe_zone_growth_events, 1,
        "only the in-zone block counts"
    );
    assert_eq!(after.unsafe_zone_growth_bytes, 1 << 20);
}
