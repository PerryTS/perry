//! The idle-time reclaim (`gc/idle_reclaim.rs`): the park-site hook opens a
//! full budgeted cycle when one is owed, steps it in wake-checked slices,
//! prices it, backs off when it did not pay, and never keeps the loop from
//! sleeping. Every gate is asserted in both directions, and every "it ran"
//! claim is a counter, not an output.
//!
//! Clock discipline: the reducer observes collections at hook calls, and the
//! quiet window starts at the observation. In production the hook runs at
//! every park, so observation is immediate; here each external collection is
//! followed by one hook call at a pinned time before the clock advances.

use super::super::idle_reclaim::test_support::*;
use super::super::*;
use super::support::*;

/// Enough dead old-gen bytes to clear the productivity bar from any
/// plausible pre-existing old-gen occupancy in the test process.
const DEAD_OLD_BYTES_TARGET: usize = 12 * 1024 * 1024;

fn litter_old_gen_with_dead_promises() -> usize {
    let each = std::mem::size_of::<crate::promise::Promise>();
    let count = DEAD_OLD_BYTES_TARGET / each + 1;
    for _ in 0..count {
        let _ = unsafe { alloc_old_test_promise() };
    }
    count * each
}

/// The hook told the loop to park (it did nothing, or hit its work cap).
fn parks(verdict: ParkVerdict) -> bool {
    matches!(verdict, ParkVerdict::Park(_))
}

/// The hook told the loop not to park: it did collector work or saw a wake.
fn resumes(verdict: ParkVerdict) -> bool {
    verdict == ParkVerdict::Resume
}

/// One collection the reducer did not start, observed by the hook at `now`.
/// Asserts the collection actually ran (a deferral would make every gate test
/// vacuous) and that the observing call cannot itself be owed — the quiet
/// window has just restarted.
fn external_collection_observed_at(now: u64) {
    let before = gc_collection_count();
    gc_collect_minor();
    assert!(
        gc_collection_count() > before,
        "LIVE SUBJECT: the external collection must have run"
    );
    set_test_now_ms(Some(now));
    assert!(
        parks(idle_reclaim_park_hook(1000)),
        "the observing call restarts the quiet window and cannot be owed"
    );
}

fn drive_until_idle(now_ms: u64, budget_ms: u64) {
    set_test_now_ms(Some(now_ms));
    for _ in 0..10_000 {
        if !gc_budgeted_cycle_active() {
            return;
        }
        let _ = idle_reclaim_park_hook(budget_ms);
    }
    panic!("idle reclaim cycle did not complete");
}

#[test]
fn idle_reclaim_runs_a_full_at_the_park_when_owed() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    let littered = litter_old_gen_with_dead_promises();
    external_collection_observed_at(0);
    // The external minor may have MOVED the young survivor; the reducer's full
    // is non-moving, so its address from here on is the one that must hold.
    let rooted = js_shadow_slot_get(0) & POINTER_MASK;
    assert_ne!(rooted, 0, "premise: the survivor is still rooted");
    let old_before = crate::arena::old_gen_in_use_bytes();
    let completions_before = idle_reclaim_completions();
    let attempts_before = idle_reclaim_attempts();

    // Gate 2: quiet.
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS - 1));
    assert!(parks(idle_reclaim_park_hook(1000)), "one ms short of quiet");
    assert_eq!(thread_attempts(), 0);
    assert!(!gc_budgeted_cycle_active());

    // Quiet reached: the hook opens the cycle and reports work.
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "owed full must run at the park"
    );
    assert_eq!(thread_attempts(), 1);
    assert_eq!(idle_reclaim_attempts(), attempts_before + 1);
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);

    assert_eq!(
        idle_reclaim_completions(),
        completions_before + 1,
        "the reducer's full must reach the finisher under its own trigger kind"
    );
    let old_after = crate::arena::old_gen_in_use_bytes();
    assert!(
        old_before.saturating_sub(old_after) >= littered / 2,
        "the dead old-gen litter must have been swept: before={old_before} after={old_after} littered={littered}"
    );
    assert_eq!(
        idle_reclaim_backoff_shift(),
        0,
        "a productive full resets the backoff"
    );
    assert_eq!(
        js_shadow_slot_get(0) & POINTER_MASK,
        rooted,
        "rooted survivor intact and unmoved"
    );

    // Gate 1: activity. Nothing collected since — no second attempt, ever.
    set_test_now_ms(Some(
        IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS + 1,
    ));
    assert!(
        parks(idle_reclaim_park_hook(1000)),
        "no activity since the last full"
    );
    assert_eq!(thread_attempts(), 1);

    // One external collection re-arms activity; quiet restarts at it.
    let t = IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS + 1;
    external_collection_observed_at(t);
    set_test_now_ms(Some(t + IDLE_RECLAIM_QUIET_MS - 1));
    assert!(
        parks(idle_reclaim_park_hook(1000)),
        "quiet restarts at the new collection"
    );
    set_test_now_ms(Some(t + IDLE_RECLAIM_QUIET_MS));
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "active + quiet + spaced: owed again"
    );
    assert_eq!(thread_attempts(), 2);
    drive_until_idle(t + IDLE_RECLAIM_QUIET_MS, 1000);
}

#[test]
fn idle_reclaim_rate_floor_holds_between_two_owed_fulls() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    litter_old_gen_with_dead_promises();
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1);

    // Activity and quiet both satisfied well before the spacing floor.
    let observed = IDLE_RECLAIM_QUIET_MS + 1;
    external_collection_observed_at(observed);
    let quiet_again = observed + IDLE_RECLAIM_QUIET_MS;
    assert!(
        quiet_again < IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS,
        "test premise: two quiet windows fit inside the spacing floor"
    );
    set_test_now_ms(Some(quiet_again));
    assert!(
        parks(idle_reclaim_park_hook(1000)),
        "spacing floor must hold"
    );
    assert_eq!(thread_attempts(), 1);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS));
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "spacing floor passed"
    );
    assert_eq!(thread_attempts(), 2);
    drive_until_idle(IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS, 1000);
}

#[test]
fn idle_reclaim_yields_to_a_pending_wake_and_resumes() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    litter_old_gen_with_dead_promises();
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));

    // One stepper call per slice, one slice per hook call: the cycle cannot
    // complete inside a single park.
    set_test_slice_us(Some(0));
    set_test_max_slices(Some(1));
    let slices_before = idle_reclaim_slices();
    assert!(resumes(idle_reclaim_park_hook(1000)));
    assert!(
        gc_budgeted_cycle_active(),
        "one slice must leave the cycle open"
    );
    assert_eq!(idle_reclaim_slices(), slices_before + 1);

    // A wake arrives: the next park must not step at all.
    let yields_before = idle_reclaim_yields();
    crate::event_pump::js_notify_main_thread();
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "a pending wake is reported as 'do not park'"
    );
    assert_eq!(idle_reclaim_yields(), yields_before + 1);
    assert_eq!(
        idle_reclaim_slices(),
        slices_before + 1,
        "no slice ran behind a wake"
    );
    assert!(gc_budgeted_cycle_active());

    // The loop consumes the notify (fast path), then the cycle resumes and
    // finishes across later parks.
    crate::event_pump::js_wait_for_event();
    set_test_max_slices(None);
    set_test_slice_us(None);
    let completions_before = idle_reclaim_completions();
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(idle_reclaim_completions(), completions_before + 1);
    assert_eq!(thread_attempts(), 1, "resuming is not a second attempt");
}

#[test]
fn idle_reclaim_backs_off_after_an_unproductive_full() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    // No dead old-gen litter: the full has nothing to reclaim.
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1);
    assert_eq!(
        idle_reclaim_backoff_shift(),
        1,
        "an unproductive full doubles the requirement"
    );

    // One external collection no longer suffices; two do.
    let t0 = IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS;
    external_collection_observed_at(t0);
    set_test_now_ms(Some(t0 + IDLE_RECLAIM_QUIET_MS));
    assert!(
        parks(idle_reclaim_park_hook(1000)),
        "backoff: one collection is not enough"
    );
    assert_eq!(thread_attempts(), 1);
    let t1 = t0 + IDLE_RECLAIM_QUIET_MS;
    external_collection_observed_at(t1);
    set_test_now_ms(Some(t1 + IDLE_RECLAIM_QUIET_MS));
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "backoff: two collections re-arm"
    );
    assert_eq!(thread_attempts(), 2);
    drive_until_idle(t1 + IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(
        idle_reclaim_backoff_shift(),
        2,
        "still unproductive: the shift grows"
    );
    assert!(IDLE_RECLAIM_MAX_BACKOFF_SHIFT >= 2);
}

#[test]
fn park_hook_finishes_a_budgeted_cycle_the_pacer_left_open() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    let _dead = young_leaf();
    triggers.make_arena_trigger_due();
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert!(gc_budgeted_cycle_active(), "premise: a pacer cycle is open");

    // Not owed (no quiet window elapsed) — but an open cycle is driven anyway.
    let slices_before = idle_reclaim_slices();
    drive_until_idle(0, 1000);
    assert!(!gc_budgeted_cycle_active());
    assert!(
        idle_reclaim_slices() > slices_before,
        "the park hook did the stepping"
    );
    assert_eq!(
        thread_attempts(),
        0,
        "driving the pacer's cycle is not a reducer attempt"
    );
    assert_eq!(js_shadow_slot_get(0) & POINTER_MASK, live as u64);
}

#[test]
fn work_cap_lets_the_loop_park_while_a_cycle_is_still_open() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    litter_old_gen_with_dead_promises();
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    // Every slice is one stepper call and is charged 200 ms of the 500 ms cap.
    set_test_slice_us(Some(0));
    set_test_work_charge_ms(Some(200));
    let capped_before = idle_reclaim_work_capped();
    let slices_before = idle_reclaim_slices();

    assert_eq!(
        idle_reclaim_park_hook(1000),
        ParkVerdict::Park(1000),
        "three 200 ms slices spend the cap; the loop must then park for its budget"
    );
    assert!(
        gc_budgeted_cycle_active(),
        "the cycle stays open across the park"
    );
    assert_eq!(idle_reclaim_slices(), slices_before + 3);
    assert_eq!(idle_reclaim_work_capped(), capped_before + 1);

    // Still inside the same second: no further work, park again.
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS + 999));
    assert_eq!(idle_reclaim_park_hook(700), ParkVerdict::Park(700));
    assert_eq!(idle_reclaim_slices(), slices_before + 3);

    // A new second: the window rotates and stepping resumes.
    set_test_work_charge_ms(None);
    set_test_slice_us(None);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS + 1000));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS + 1000, 1000);
    assert!(!gc_budgeted_cycle_active());
}

#[test]
fn kill_switch_off_leaves_the_heap_alone() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);
    set_test_enabled(Some(false));

    litter_old_gen_with_dead_promises();
    let before = gc_collection_count();
    gc_collect_minor();
    assert!(gc_collection_count() > before);
    let count_before = gc_collection_count();
    let attempts_before = idle_reclaim_attempts();
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS));
    assert_eq!(
        idle_reclaim_park_hook(1000),
        ParkVerdict::Park(1000),
        "OFF: the hook must be inert"
    );
    assert!(!gc_budgeted_cycle_active());
    assert_eq!(gc_collection_count(), count_before);
    assert_eq!(idle_reclaim_attempts(), attempts_before);
    assert_eq!(thread_attempts(), 0);
}

#[test]
fn kill_switch_is_parsed_by_value_with_the_default_on_vocabulary() {
    assert!(idle_reclaim_enabled_from_value(None), "unset = ON");
    assert!(idle_reclaim_enabled_from_value(Some("1")));
    for off in ["0", "off", "false", "no", " OFF "] {
        assert!(
            !idle_reclaim_enabled_from_value(Some(off)),
            "{off:?} must read as OFF"
        );
    }
}

#[test]
fn idle_reclaim_full_reaches_the_allocator_purge() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);
    crate::gc::cycle_malloc_trim::reset_test_mimalloc_purge_count();
    let post_purges_before = idle_reclaim_post_purges();

    litter_old_gen_with_dead_promises();
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);

    let purges = crate::gc::cycle_malloc_trim::test_mimalloc_purge_count();
    assert_eq!(
        idle_reclaim_post_purges(),
        post_purges_before + 1,
        "the park hook purges once more after the cycle's own bookkeeping is gone"
    );
    if crate::gc::cycle_malloc_trim::mimalloc_purge_available() {
        assert!(
            purges >= 2,
            "the reducer's full must reach mimalloc's purge in its reclaim tail — \
             freed arena blocks otherwise sit in the segment cache and RSS never moves"
        );
    } else {
        assert_eq!(
            purges, 0,
            "no mimalloc on this build: the witness must stay silent"
        );
    }
}
