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
    GC_SAFEPOINT_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    GC_SAFEPOINT_PENDING.with(|pending| pending.set(false));
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
}

#[test]
fn old_reclaim_defers_to_a_precise_safepoint_instead_of_scanning() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();
    reset_scan_fallback_counters();

    arm_old_reclaim();
    gc_check_trigger();

    // Before #7148 this call ran a full mark-sweep here, at the allocation
    // point, behind `force_full_scan()`.
    assert_eq!(
        scan_fallback_count(ConservativeScanSite::OldReclaimSlackValve),
        0,
        "old-gen reclaim must defer, not scan, while it is within slack"
    );
    assert!(
        GC_SAFEPOINT_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "the deferral must be recorded in the old-gen unit"
    );
    assert!(
        GC_SAFEPOINT_PENDING.with(std::cell::Cell::get),
        "`js_gc_loop_safepoint` polls GC_SAFEPOINT_PENDING, so the old-gen \
         deferral must set it too or nothing drains the request"
    );
    assert!(
        GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "deferring must not retire the request — the trigger has to stay due \
         so the safepoint finds it"
    );

    // Drain at the precise safepoint the deferral promised.
    js_gc_loop_safepoint();

    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::OldReclaim),
        1,
        "LIVE SUBJECT: the deferred full mark-sweep must actually have run at \
         the safepoint — 'nothing scanned' is worthless if nothing collected"
    );
    assert_eq!(
        scan_fallback_count(ConservativeScanSite::OldReclaimSlackValve),
        0,
        "the safepoint path must not force the scan"
    );
    assert!(
        !GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "the safepoint collection retires the request"
    );
    assert!(
        !GC_SAFEPOINT_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "the safepoint collection clears its own deferral flag"
    );

    clear_old_reclaim_state();
}

#[test]
fn old_reclaim_slack_valve_fires_when_the_deferral_never_drains() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();
    reset_scan_fallback_counters();

    // The other direction. A deferral whose slack cannot expire is #7024's bug
    // wearing new clothes: RSS grows without bound and the branch that was
    // supposed to bound it is dead. The shipped slack is 32 MB, so crossing it
    // for real would mean committing 32 MB of old-gen inside a unit test;
    // shrink it instead so the *real* branch in `gc_check_trigger` runs
    // deterministically. Slack 0 with the baseline at the current pressure is
    // the exact state "this deferral has used up its whole allowance".
    let _slack = crate::gc::policy::force_old_reclaim_defer_slack(0);
    arm_old_reclaim();
    let pressure = old_reclaim_pressure_bytes();
    GC_SAFEPOINT_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    GC_SAFEPOINT_OLD_RECLAIM_BASE.with(|base| base.set(pressure));

    gc_check_trigger();

    assert_eq!(
        scan_fallback_count(ConservativeScanSite::OldReclaimSlackValve),
        1,
        "past the slack the conservative valve MUST fire — it is the only \
         thing bounding old-gen growth when no safepoint is reachable"
    );
    assert!(
        !GC_SAFEPOINT_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "the valve retires the deferral; a stale exceeded baseline would \
         disable deferral for the rest of the process"
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
