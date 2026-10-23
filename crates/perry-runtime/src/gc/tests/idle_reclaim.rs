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

    // Gate 1: activity. Nothing collected since, and this test guard keeps the
    // arena right-size gate below its capacity floor, so no second attempt.
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

/// #9709: general-arena block release deliberately needs two full collection
/// observations. One external collection plus the ordinary idle-reducer full
/// establishes sustained low utilization; that full is observation one, and
/// the right-sizer must grant observation two without demanding new mutator
/// activity. The episode then disarms, so the bypass cannot become a periodic
/// full-GC loop.
#[test]
fn sustained_arena_slack_gets_one_bounded_followup_without_mutator_activity() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);
    let capacity = 100 * 1024 * 1024;
    super::super::arena_right_size::test_support::set_test_usage(Some(
        super::super::arena_right_size::ArenaUsage {
            live_bytes: capacity / 2,
            capacity_bytes: capacity,
        },
    ));
    let right_size_starts_before = arena_right_size_starts();

    // First low sample, and the only mutator-driven collection in this test.
    external_collection_observed_at(0);
    assert_eq!(
        super::super::arena_right_size::test_support::state_snapshot().0,
        1
    );

    // The ordinary activity arm starts the first full. Its post-collection
    // sample opens the episode and counts as the first full observation.
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1);
    assert_eq!(
        super::super::arena_right_size::test_support::state_snapshot().1,
        1,
        "one full observation remains"
    );

    // The normal activity gate is not re-armed. The capacity debt alone must
    // cross the same rate floor and start the second full.
    set_test_now_ms(Some(
        IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS - 1,
    ));
    assert!(parks(idle_reclaim_park_hook(1000)));
    assert_eq!(thread_attempts(), 1, "the rate floor still applies");

    let followup_at = IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_MIN_INTERVAL_MS;
    set_test_now_ms(Some(followup_at));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    assert_eq!(thread_attempts(), 2);
    assert_eq!(
        arena_right_size_starts(),
        right_size_starts_before + 1,
        "LIVE SUBJECT: the follow-up must identify the capacity debt as its reason"
    );
    drive_until_idle(followup_at, 1000);

    let (_, fulls_remaining, disarmed, _) =
        super::super::arena_right_size::test_support::state_snapshot();
    assert_eq!(fulls_remaining, 0);
    assert!(
        disarmed,
        "unchanged low utilization ends the bounded episode"
    );

    set_test_now_ms(Some(followup_at + IDLE_RECLAIM_MIN_INTERVAL_MS + 1));
    assert!(parks(idle_reclaim_park_hook(1000)));
    assert_eq!(
        thread_attempts(),
        2,
        "no third full without utilization or material-capacity hysteresis"
    );
}

#[test]
fn a_parked_heap_is_re_armed_by_elapsed_idle_alone() {
    // #9831. The activity requirement is denominated in collections the
    // reducer did not start. On a quiet heap the only such collections are the
    // compactor's, and the compactor runs only once the reducer has already
    // moved the residue ratio past the compactor's gate — so when that gate
    // declines, the decline is permanent and the heap parks. Measured on the
    // claude-code TUI: 23.7 % against a 25 % gate, one reclaim attempt, and
    // 221 MB never returned. Elapsed idle must be able to start attempt 2 with
    // no new collection anywhere.
    //
    // Sabotage: delete the `StartReason::IdleElapsed` arm from `start_reason`
    // and this test fails at "a second attempt must start" — attempts stay 1
    // forever, which is exactly the production symptom.
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);
    // Dead old-gen litter so the full is PRODUCTIVE and the shift stays 0:
    // this test is about the re-arm, and the backoff is the next test's
    // subject. (The guard has already pinned arena usage to live == capacity,
    // so the `arena_right_size` arm cannot be what starts anything here.)
    litter_old_gen_with_dead_promises();

    // The ordinary activity arm starts attempt 1. This is the ONLY external
    // collection in the whole test.
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1);
    assert_eq!(
        idle_reclaim_backoff_shift(),
        0,
        "the litter must have made this full productive, or the wait below is \
         doubled and this test is measuring the backoff instead of the re-arm"
    );
    let elapsed_before = idle_reclaim_elapsed_starts();

    // Past the rate floor (10 s) but short of the re-arm (15 s), so the ONLY
    // thing that can hold the reducer here is the new gate.
    assert!(IDLE_RECLAIM_REARM_MS > IDLE_RECLAIM_MIN_INTERVAL_MS);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_REARM_MS - 1));
    assert!(parks(idle_reclaim_park_hook(1000)));
    assert_eq!(
        thread_attempts(),
        1,
        "elapsed idle must not re-arm before the wait"
    );

    // At the wait: a second attempt, with no collection having happened.
    let at = IDLE_RECLAIM_QUIET_MS + IDLE_RECLAIM_REARM_MS;
    set_test_now_ms(Some(at));
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "a second attempt must start on elapsed idle alone"
    );
    assert_eq!(thread_attempts(), 2);
    assert_eq!(
        idle_reclaim_elapsed_starts(),
        elapsed_before + 1,
        "LIVE SUBJECT: the follow-up must identify ELAPSED IDLE as its reason, \
         not activity and not arena debt"
    );
}

#[test]
fn a_retaining_heap_gets_one_full_and_then_stops() {
    // GCI2: a heap that keeps its data. The first full finds nothing, and on
    // an idle heap nothing dies after it, so every later full would re-trace
    // the same live set for the same nothing. Measured on the opencode v2.1.2
    // server: reducer fulls at 24/84/204/445 s freed 0.5-2.8 MB each for ~1.6G
    // instructions. The unproductive full ENDS the sequence: no elapsed-idle
    // follow-up at any wait, until the mutator does something.
    //
    // Sabotage: drop the `followup_justified` term from the elapsed arm in
    // `start_reason` and this fails at the first doubled wait (the old 15/30/
    // 60/120/240 s streak comes back).
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);
    // No litter: the full is unproductive.

    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1, "LIVE SUBJECT: the one full ran");
    assert_eq!(idle_reclaim_backoff_shift(), 1, "and it was unproductive");
    assert!(
        super::super::policy::old_gen_growth_since_last_full_bytes()
            < IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES,
        "premise: nothing grew old-gen since the full"
    );

    let elapsed_before = idle_reclaim_elapsed_starts();
    let mut now = IDLE_RECLAIM_QUIET_MS;
    for shift in 1..=IDLE_RECLAIM_MAX_BACKOFF_SHIFT {
        now += IDLE_RECLAIM_REARM_MS << shift;
        set_test_now_ms(Some(now));
        assert!(
            parks(idle_reclaim_park_hook(1000)),
            "an unproductive full ends the sequence: no follow-up at the {shift}-shift wait"
        );
    }
    let far = now + (IDLE_RECLAIM_REARM_MS << IDLE_RECLAIM_MAX_BACKOFF_SHIFT) * 16;
    set_test_now_ms(Some(far));
    assert!(parks(idle_reclaim_park_hook(1000)));
    assert_eq!(thread_attempts(), 1, "one full, then silence");
    assert_eq!(idle_reclaim_elapsed_starts(), elapsed_before);

    // The mutator allocating again (collections it completed) re-arms it.
    external_collection_observed_at(far);
    external_collection_observed_at(far + 1);
    set_test_now_ms(Some(far + 1 + IDLE_RECLAIM_QUIET_MS));
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "mutator activity re-arms the reducer through the activity arm"
    );
    assert_eq!(thread_attempts(), 2);
    drive_until_idle(far + 1 + IDLE_RECLAIM_QUIET_MS, 1000);
}

#[test]
fn a_heap_with_real_garbage_is_asked_while_it_pays() {
    // The other direction: while each full frees past the bar, the next one is
    // justified and runs at the plain re-arm wait, with no mutator collection
    // anywhere. The first full that does not pay ends the sequence.
    //
    // Sabotage: make `followup_justified` return false and this fails at the
    // first follow-up; make it return true and it fails at the final park.
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    litter_old_gen_with_dead_promises();
    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1);
    assert_eq!(
        idle_reclaim_backoff_shift(),
        0,
        "the litter made it productive"
    );

    let elapsed_before = idle_reclaim_elapsed_starts();
    let mut now = IDLE_RECLAIM_QUIET_MS;
    const PAYING_FOLLOWUPS: u64 = 3;
    for round in 1..=PAYING_FOLLOWUPS {
        // Garbage the previous full could not have seen.
        litter_old_gen_with_dead_promises();
        now += IDLE_RECLAIM_REARM_MS;
        set_test_now_ms(Some(now));
        assert!(
            resumes(idle_reclaim_park_hook(1000)),
            "round {round}: the previous full paid, so the follow-up is justified"
        );
        drive_until_idle(now, 1000);
        assert_eq!(thread_attempts(), 1 + round);
        assert_eq!(
            idle_reclaim_backoff_shift(),
            0,
            "round {round}: the follow-up found the new garbage"
        );
    }

    // No new garbage: the next follow-up is still justified by the last
    // yield, runs, and finds nothing.
    now += IDLE_RECLAIM_REARM_MS;
    set_test_now_ms(Some(now));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(now, 1000);
    assert_eq!(thread_attempts(), 2 + PAYING_FOLLOWUPS);
    assert_eq!(
        idle_reclaim_backoff_shift(),
        1,
        "the last full found nothing"
    );
    assert_eq!(
        idle_reclaim_elapsed_starts(),
        elapsed_before + PAYING_FOLLOWUPS + 1,
        "LIVE SUBJECT: every follow-up came from the elapsed arm"
    );

    // ...and that ends it.
    let far = now + (IDLE_RECLAIM_REARM_MS << IDLE_RECLAIM_MAX_BACKOFF_SHIFT) * 16;
    set_test_now_ms(Some(far));
    assert!(parks(idle_reclaim_park_hook(1000)));
    assert_eq!(thread_attempts(), 2 + PAYING_FOLLOWUPS);
}

#[test]
fn old_gen_growth_after_an_unproductive_full_justifies_a_follow_up() {
    // The second justification: after a full that found nothing, old-gen the
    // mutator allocated since (without completing a collection) is what a
    // full could now find dead. Growth past the productive bar re-arms the
    // elapsed arm, at the doubled wait.
    //
    // Sabotage: drop the `growth >= productive_bar(..)` term from
    // `followup_justified` and the follow-up never starts.
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _reducer = IdleReclaimTestGuard::new(0);

    external_collection_observed_at(0);
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS));
    assert!(resumes(idle_reclaim_park_hook(1000)));
    drive_until_idle(IDLE_RECLAIM_QUIET_MS, 1000);
    assert_eq!(thread_attempts(), 1);
    assert_eq!(idle_reclaim_backoff_shift(), 1);

    litter_old_gen_with_dead_promises();
    assert!(
        super::super::policy::old_gen_growth_since_last_full_bytes()
            >= IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES,
        "LIVE SUBJECT: the litter must read as old-gen growth since the full"
    );
    let wait = IDLE_RECLAIM_REARM_MS << 1;
    set_test_now_ms(Some(IDLE_RECLAIM_QUIET_MS + wait - 1));
    assert!(
        parks(idle_reclaim_park_hook(1000)),
        "growth justifies the follow-up, not an earlier one"
    );
    let at = IDLE_RECLAIM_QUIET_MS + wait;
    set_test_now_ms(Some(at));
    let elapsed_before = idle_reclaim_elapsed_starts();
    assert!(
        resumes(idle_reclaim_park_hook(1000)),
        "growth re-arms the elapsed arm"
    );
    assert_eq!(thread_attempts(), 2);
    assert_eq!(idle_reclaim_elapsed_starts(), elapsed_before + 1);
    drive_until_idle(at, 1000);
    assert_eq!(
        idle_reclaim_backoff_shift(),
        0,
        "and the growth was garbage"
    );
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

/// A full is priced by what its SWEEP freed, not by the old-gen occupancy
/// delta. `arena::old_gen_in_use_bytes` is the sum of the live blocks' bump
/// offsets: a non-moving sweep returns dead objects to the old-gen free list
/// and the block keeps its offset, so a cycle that freed megabytes can leave
/// it unchanged. That is not a corner case — it is what the compiled TUI does
/// (2.37 MB freed, occupancy flat at 93.6 MB, scored unproductive, backoff to
/// one attempt per four collections inside a minute of idle).
#[test]
fn a_full_is_productive_on_swept_bytes_when_occupancy_cannot_move() {
    // Priced from the process-wide old-generation size; see the mirror test.
    let Some(_isolated) = crate::test_support::isolated_process_guard() else {
        return;
    };
    let _reducer = IdleReclaimTestGuard::new(0);
    // Price against the live occupancy, so the completion below sees no delta
    // at all — the production shape, reproduced without the fragmentation.
    let before = crate::arena::old_gen_in_use_bytes();
    set_old_in_use_at_start(before);
    let bar = IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES.max(before / 100 * IDLE_RECLAIM_PRODUCTIVE_PCT);
    let productive_before = idle_reclaim_productive();

    super::super::idle_reclaim::note_cycle_completed(bar as u64);

    assert_eq!(
        idle_reclaim_productive(),
        productive_before + 1,
        "a sweep that freed the bar is productive however the block offsets read"
    );
    assert_eq!(
        idle_reclaim_backoff_shift(),
        0,
        "a productive full resets the activity requirement"
    );
}

/// The mirror: freeing less than the bar still backs off, so the pricing
/// change cannot be satisfied by calling every full productive.
#[test]
fn a_full_that_freed_less_than_the_bar_still_backs_off() {
    // The bar is priced from the process-wide old-generation size, which any
    // concurrently running GC test moves between this test reading it and the
    // cycle note reading it again: only an exclusively owned process can pin
    // "one byte short of the bar".
    let Some(_isolated) = crate::test_support::isolated_process_guard() else {
        return;
    };
    let _reducer = IdleReclaimTestGuard::new(0);
    let before = crate::arena::old_gen_in_use_bytes();
    set_old_in_use_at_start(before);
    let bar = IDLE_RECLAIM_PRODUCTIVE_MIN_BYTES.max(before / 100 * IDLE_RECLAIM_PRODUCTIVE_PCT);
    let productive_before = idle_reclaim_productive();

    super::super::idle_reclaim::note_cycle_completed(bar as u64 - 1);

    assert_eq!(
        idle_reclaim_productive(),
        productive_before,
        "one byte short of the bar is not productive"
    );
    assert_eq!(
        idle_reclaim_backoff_shift(),
        1,
        "an unproductive full doubles the activity requirement"
    );
}
