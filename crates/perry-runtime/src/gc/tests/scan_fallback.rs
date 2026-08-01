//! #7148: the conservative-scan fallback must be *unreachable*, not imprecise.
//!
//! Each test below asserts BOTH halves of a deferral claim, because only the
//! first half is cheap to fake:
//!
//! 1. the conservative valve did **not** fire (`scan_fallback_count == 0`), and
//! 2. the precise safepoint collection that replaced it **did**
//!    (`safepoint_drain_count > 0`).
//!
//! CLAUDE.md's fourth way a gate cannot fail is "the gate runs but its subject
//! never did". A test that only checked (1) would pass on a tree where the
//! trigger never armed at all — the strongest possible regression reported as
//! a pass. Every deferral test here therefore ends on a live-subject counter.

use super::super::*;
use super::support::*;

/// Make `gc_budgeted_due_trigger()` report `OldReclaim` without allocating
/// 48 MB of old-gen: `GC_OLD_RECLAIM_PENDING` is the sticky "a full old-gen
/// reclaim is owed" flag it reads first (the same lever
/// `budgeted_step_api.rs` uses).
fn arm_old_reclaim() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
}

fn clear_old_reclaim_state() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    GC_SAFEPOINT_PENDING.with(|pending| pending.set(false));
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
}

#[test]
fn old_reclaim_runs_precisely_at_a_safepoint() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();
    reset_scan_fallback_counters();

    // `gc_safepoint_moving_minor` used to bail on OldReclaim ("stays on its
    // existing full mark-sweep path"), so the ONLY place an old-gen reclaim
    // could happen was the allocation point, behind `force_full_scan()`.
    arm_old_reclaim();
    GC_SAFEPOINT_PENDING.with(|p| p.set(true));
    js_gc_loop_safepoint();

    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::OldReclaim),
        1,
        "LIVE SUBJECT: the full mark-sweep must actually have run at the \
         safepoint — 'nothing scanned' is worthless if nothing collected"
    );
    assert_eq!(
        automatic_scan_fallback_total(),
        0,
        "the safepoint path has precise roots, so it must not force the scan"
    );
    assert!(
        !GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "the safepoint collection retires the request, so the allocation-point \
         arm finds nothing due and never runs a second, conservative cycle"
    );

    clear_old_reclaim_state();
}

#[test]
fn old_reclaim_alloc_point_still_completes_immediately_and_is_counted() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();
    reset_scan_fallback_counters();

    // The other direction, and the reason this site is NOT deferred. #5476's
    // own regression test asserts that a single `gc_check_trigger` call — what
    // every allocation does — drives the reclaim to completion, because the
    // workload it was filed for is a compute-only loop that reaches no host
    // step. Deferring here would turn "RSS climbs unbounded" back on for one
    // growth quantum. What #7148 changes is only that the cost is now visible.
    let collections_before = gc_collection_count();
    arm_old_reclaim();
    gc_check_trigger();

    assert!(
        gc_collection_count() > collections_before,
        "a single gc_check_trigger call must still complete the reclaim (#5476)"
    );
    assert_eq!(
        scan_fallback_count(ConservativeScanSite::OldReclaimAllocPoint),
        1,
        "and it is still a conservative-scan collection — counted, not hidden"
    );

    clear_old_reclaim_state();
}

#[test]
fn old_reclaim_is_unchanged_when_moving_loop_polls_are_off() {
    // #7161 proposes flipping `PERRY_GC_MOVING_LOOP_POLLS` OFF as a stopgap for
    // #7154. The allocation-point arm must not consult that gate at all, so
    // that the flip can never make this site behave differently — inert, not
    // unsound. (The precise safepoint path is simply reached less often, which
    // shows up as a higher census count, not as a correctness change.)
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();
    reset_scan_fallback_counters();

    let collections_before = gc_collection_count();
    arm_old_reclaim();
    gc_check_trigger();

    assert!(
        gc_collection_count() > collections_before,
        "polls off must not stop the allocation-point reclaim completing"
    );
    assert_eq!(
        scan_fallback_count(ConservativeScanSite::OldReclaimAllocPoint),
        1,
        "identical behaviour and identical census to the polls-on arm"
    );

    clear_old_reclaim_state();
}

#[test]
fn host_pressure_collects_precisely_when_no_generated_frame_is_live() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    clear_old_reclaim_state();
    reset_shadow_stack();
    reset_scan_fallback_counters();

    assert!(
        !crate::gc::roots::shadow_stack_has_active_frame(),
        "precondition: the run-loop-boundary case has an empty shadow stack"
    );

    let result = js_gc_memory_pressure(1);

    assert_eq!(result, 2, "collected synchronously");
    assert_eq!(
        automatic_scan_fallback_total(),
        0,
        "with no generated frame live the precise root set is complete, so \
         the host-pressure collection must not force the scan (the site has no \
         conservative arm left at all — this catches its reintroduction)"
    );
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::HostPressure),
        1,
        "LIVE SUBJECT: the precise collection ran"
    );

    clear_old_reclaim_state();
}

#[test]
fn host_pressure_defers_when_a_generated_frame_is_live() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();
    reset_shadow_stack();
    reset_scan_fallback_counters();

    let frame = js_shadow_frame_push(2);
    assert!(crate::gc::roots::shadow_stack_has_active_frame());

    // Critical level: the owed collection must be a FULL cycle, so the
    // deferral has to arm the sticky old-gen-reclaim request rather than only
    // the nursery one.
    let result = js_gc_memory_pressure(2);
    js_shadow_frame_pop(frame);

    assert_eq!(
        result, 1,
        "documented return code for 'trigger lowered, collection deferred'"
    );
    assert_eq!(
        automatic_scan_fallback_total(),
        0,
        "a live generated frame means a safepoint is reachable — defer to it \
         instead of scanning conservatively"
    );
    assert!(GC_SAFEPOINT_PENDING.with(std::cell::Cell::get));
    assert!(
        GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "level >= 2 must owe a FULL cycle, not a minor"
    );

    // The frame is gone; drain the promise.
    js_gc_loop_safepoint();
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::OldReclaim),
        1,
        "LIVE SUBJECT: the deferred critical-pressure full cycle ran at the \
         safepoint"
    );

    clear_old_reclaim_state();
}

#[test]
fn explicit_gc_still_scans_and_the_census_attributes_it() {
    let _isolation = GcTestIsolationGuard::new();
    clear_old_reclaim_state();
    reset_scan_fallback_counters();

    js_gc_collect();

    // #7148 deliberately does NOT defer explicit `gc()`: it is a user request
    // with synchronous semantics. What changes is that its cost is now
    // attributable — every `gc_ratchet` probe ends with one of these, so a
    // census that lumped it in with the automatic sites would misread.
    assert!(
        scan_fallback_count(ConservativeScanSite::ManualCollect) >= 1,
        "explicit gc() keeps the scan and must be counted as non-automatic"
    );
    assert_eq!(
        automatic_scan_fallback_total(),
        0,
        "an explicit gc() must not be counted against the automatic-site total \
         that #7148 is driving to zero"
    );

    clear_old_reclaim_state();
}

#[test]
fn conservative_scan_env_full_does_not_override_a_pinned_test_mode() {
    // `PERRY_CONSERVATIVE_STACK_SCAN=full` failed 134 of 1574 runtime tests
    // because the env value beat the per-thread override, so a test that had
    // declared its roots precise still got the conservative scan and its
    // "this should have been collected" assertion broke. The env var may now
    // make the scan LESS aggressive than a test declared, never more.
    let _env = EnvVarGuard::set("PERRY_CONSERVATIVE_STACK_SCAN", "full");
    let previous = crate::gc::roots::set_conservative_stack_scan_override(None);

    // Unpinned: the ops escape hatch still works. This is the arm the
    // `gc_ratchet` sensitivity run depends on — production binaries have no
    // pinned override, so `=full` still forces the scan there.
    assert_eq!(
        crate::gc::roots::conservative_stack_scan_decision(),
        ConservativeStackScanDecision::Scan,
        "with nothing pinned, =full must still force the scan"
    );

    // Pinned by a test isolation guard: the declared mode wins.
    crate::gc::roots::set_conservative_stack_scan_override(Some(ConservativeStackScanMode::Auto));
    assert_eq!(
        crate::gc::roots::conservative_stack_scan_decision(),
        ConservativeStackScanDecision::SkipDisabled,
        "a pinned Auto must beat an ambient =full"
    );

    crate::gc::roots::set_conservative_stack_scan_override(Some(
        ConservativeStackScanMode::Disabled,
    ));
    assert_eq!(
        crate::gc::roots::conservative_stack_scan_decision(),
        ConservativeStackScanDecision::SkipDisabled,
        "a pinned Disabled must beat an ambient =full"
    );

    crate::gc::roots::set_conservative_stack_scan_override(previous);
}

#[test]
fn conservative_scan_env_off_still_beats_a_forced_scan() {
    // The other precedence direction, unchanged by #7148: `=0` is the
    // bisection escape hatch, and it must keep winning over
    // `ManualGcScanGuard::force_full_scan()` (which pins `Full`). Narrowing
    // the #7148 rule to "env asking for Full" is what preserves this.
    let _env = EnvVarGuard::set("PERRY_CONSERVATIVE_STACK_SCAN", "0");
    let previous = crate::gc::roots::set_conservative_stack_scan_override(None);

    crate::gc::roots::set_conservative_stack_scan_override(Some(ConservativeStackScanMode::Full));
    assert_eq!(
        crate::gc::roots::conservative_stack_scan_decision(),
        ConservativeStackScanDecision::SkipDisabled,
        "=0 must still disable the scan even when a guard pinned Full"
    );

    crate::gc::roots::set_conservative_stack_scan_override(previous);
}

#[test]
fn host_pressure_precise_collection_does_not_depend_on_moving_loop_polls() {
    // The host-pressure win that matters — collecting with precise roots when
    // no generated frame is live — is reached without consulting
    // `PERRY_GC_MOVING_LOOP_POLLS` at all, so #7161's proposed flip cannot make
    // this site scan conservatively again.
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_legacy_gc_pacing();
    clear_old_reclaim_state();
    reset_shadow_stack();
    reset_scan_fallback_counters();

    let result = js_gc_memory_pressure(1);

    assert_eq!(result, 2, "collected synchronously with polls off");
    assert_eq!(
        automatic_scan_fallback_total(),
        0,
        "still no conservative scan with polls off"
    );
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::HostPressure),
        1,
        "LIVE SUBJECT: the precise collection ran"
    );

    clear_old_reclaim_state();
}

#[test]
fn host_pressure_deferral_still_has_a_drain_when_moving_loop_polls_are_off() {
    // With polls off `js_gc_loop_safepoint` is a no-op, so the frame-live
    // deferral must not depend on it. Two backstops survive #7161: the
    // outermost microtask-pump boundary (gated by `PERRY_GC_MOVING_SAFEPOINT`,
    // a different knob) and the lowered arena trigger, which makes the ordinary
    // allocation-point arm collect at the next check. This test pins the second
    // one, because it is the one that holds even for a program that never
    // yields to the event loop.
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_legacy_gc_pacing();
    clear_old_reclaim_state();
    reset_shadow_stack();
    reset_scan_fallback_counters();

    let frame = js_shadow_frame_push(2);
    let result = js_gc_memory_pressure(2);
    js_shadow_frame_pop(frame);

    assert_eq!(result, 1, "deferred, as with polls on");
    assert_eq!(
        automatic_scan_fallback_total(),
        0,
        "deferring never scans, whatever the pacing mode"
    );
    assert!(
        GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "the owed FULL cycle is still armed, so the next allocation-point \
         trigger check collects it even with no safepoint machinery running"
    );

    // The polls-off backstop: an ordinary trigger check completes it.
    let collections_before = gc_collection_count();
    gc_check_trigger();
    assert!(
        gc_collection_count() > collections_before,
        "LIVE SUBJECT: the deferred critical-pressure cycle actually ran"
    );

    clear_old_reclaim_state();
}
