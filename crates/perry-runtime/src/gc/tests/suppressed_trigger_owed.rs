//! A trigger check that an allocation reaches inside a `GcSuppressScope` is
//! owed, not dropped: the outermost scope runs it as it drops.
//!
//! Found on a loop of small Maps: every arena block boundary of the loop fell
//! inside `ensure_capacity`'s scope, the check there returned early, nothing
//! ran it again, and the program finished its loop with no minor at all.
//!
//! The lever is the one `scan_fallback.rs` uses: `GC_OLD_RECLAIM_PENDING`
//! makes the next allocation point find an old reclaim due, and with moving
//! pacing that point arms the safepoint poll (`GC_SAFEPOINT_PENDING`) instead
//! of collecting. So "the check ran" is observable without a collection at
//! the check itself, and the drain at the poll proves the request was real.

use super::super::*;
use super::support::*;

fn arm_old_reclaim() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
}

fn clear_old_reclaim_state() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    crate::gc::set_safepoint_pending(false);
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
}

fn safepoint_armed() -> bool {
    GC_SAFEPOINT_PENDING.with(Cell::get)
}

fn trigger_owed() -> bool {
    GC_FLAGS.with(Cell::get) & policy::GC_FLAG_TRIGGER_OWED != 0
}

#[test]
fn trigger_reached_inside_suppress_scope_runs_when_the_outermost_scope_drops() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();

    // Control: outside any scope the same check arms the poll, so the
    // assertions below measure the scope and not the lever.
    arm_old_reclaim();
    gc_check_trigger();
    assert!(
        safepoint_armed(),
        "control: the allocation point must arm the poll"
    );
    clear_old_reclaim_state();

    arm_old_reclaim();
    let outer = policy::GcSuppressScope::new();
    {
        let _inner = policy::GcSuppressScope::new();
        // What `arena_cell_alloc` does at a block boundary.
        gc_check_trigger();
        assert!(!safepoint_armed(), "a suppressed check must not act");
        assert!(
            trigger_owed(),
            "a suppressed check must be recorded as owed"
        );
    }
    assert!(
        !safepoint_armed() && trigger_owed(),
        "a nested scope's exit is still inside the outer scope; the check stays owed"
    );
    drop(outer);
    assert!(
        !trigger_owed(),
        "the outermost exit consumes the owed check"
    );
    assert!(
        safepoint_armed(),
        "the outermost exit must run the owed check, or the trigger is lost"
    );

    // LIVE SUBJECT: the request the exit made is a real one; the next poll
    // runs the owed full.
    let before = gc_collection_count();
    js_gc_loop_safepoint();
    assert!(
        gc_collection_count() > before,
        "the poll armed at scope exit must collect"
    );
    assert!(!GC_OLD_RECLAIM_PENDING.with(TriggerInput::get));
    clear_old_reclaim_state();
}

#[test]
fn gc_suppress_window_leaves_its_owed_check_to_the_window_owner() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_old_reclaim_state();

    arm_old_reclaim();
    gc_suppress();
    gc_check_trigger();
    assert!(trigger_owed());
    gc_unsuppress();
    // JSON.parse prices the window itself right after `gc_unsuppress`
    // (`finish_parse_gc_accounting`); the unsuppress neither runs the check
    // nor leaves it for an unrelated scope to run later.
    assert!(!trigger_owed());
    assert!(!safepoint_armed());
    drop(policy::GcSuppressScope::new());
    assert!(!safepoint_armed());
    clear_old_reclaim_state();
}
