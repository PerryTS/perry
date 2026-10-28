use super::*;

const MB: usize = 1024 * 1024;

// Existing cases describe completed Eden windows. Early survivor-driven
// collections exercise the explicit fourth input below.
fn retune_after_scavenge(eden_live: usize, copied: usize, first_round: usize) {
    super::retune_after_scavenge(eden_live, copied, first_round, true);
}

#[test]
fn partial_eden_quiet_cycles_do_not_unlock_a_measured_promotion_lock() {
    reset_for_test();
    let d = desired_survivor_bytes();
    retune_after_scavenge(2 * d, 2 * d, 0);
    retune_after_scavenge(2 * d, 2 * d, 2 * d);
    assert_eq!(tenuring_survivals(), 1);
    for _ in 0..3 {
        super::retune_after_scavenge(0, 0, 0, false);
    }
    assert_eq!(
        tenuring_survivals(),
        1,
        "survivor-only minors are not quiet Eden windows"
    );
    super::retune_after_scavenge(0, 0, 0, true);
    assert_eq!(tenuring_survivals(), 1);
    super::retune_after_scavenge(0, 0, 0, false);
    assert_eq!(
        tenuring_survivals(),
        1,
        "an early minor cannot advance the streak"
    );
    super::retune_after_scavenge(0, 0, 0, true);
    assert_eq!(tenuring_survivals(), 2);
    reset_for_test();
}

#[test]
fn pending_first_growth_preserves_parents_until_the_sample_is_measured() {
    reset_for_test();
    let d = desired_survivor_bytes();
    assert!(!PromotionPolicy::snapshot().parent_probe_active());
    retune_after_scavenge(2 * d, 2 * d, 0);
    let probing = PromotionPolicy::snapshot();
    assert!(probing.parent_probe_active());
    assert_eq!(probing.for_type(GC_TYPE_ARRAY), OCCUPANCY_MIN_SURVIVALS + 1);
    assert_eq!(
        probing.for_type(GC_TYPE_OBJECT),
        OCCUPANCY_MIN_SURVIVALS + 1
    );
    assert_eq!(probing.for_type(GC_TYPE_STRING), OCCUPANCY_MIN_SURVIVALS);
    // A real first round consumes the uncertainty. The existing promotion
    // lock and cap feedback then proceed from the measured cohort.
    retune_after_scavenge(2 * d, 2 * d, 2 * d);
    assert!(!PromotionPolicy::snapshot().parent_probe_active());
    assert_eq!(tenuring_survivals(), 1);
    // A collection's snapshot cannot change as later feedback is written.
    assert_eq!(probing.for_type(GC_TYPE_ARRAY), OCCUPANCY_MIN_SURVIVALS + 1);
    reset_for_test();
}

#[test]
fn a_diagnostic_age_pin_overrides_the_parent_probe() {
    reset_for_test();
    let d = desired_survivor_bytes();
    retune_after_scavenge(2 * d, 2 * d, 0);
    assert!(PromotionPolicy::snapshot().parent_probe_active());
    let _pin = set_survivals_for_test(2);
    let pinned = PromotionPolicy::snapshot();
    assert!(!pinned.parent_probe_active());
    assert_eq!(pinned.for_type(GC_TYPE_ARRAY), 2);
    reset_for_test();
}

/// #7929: the constant band must buy a CONSTANT NUMBER OF OBJECTS.
///
/// The discriminating quantity is deliberately `band / mean` (the object
/// budget), not "the band changed". A test that only asserted the band
/// moved would pass under any scaling at all — including one that made the
/// mismatch worse. Only a band proportional to the mean holds this ratio
/// fixed, and the final assertion shows the *un*-denominated band does not:
/// without the term this test cannot pass.
#[test]
fn constant_band_buys_a_constant_object_count() {
    reset_for_test();
    let base = gc_scavenge_nursery_cap_bytes();
    let band_of = |mean: usize| base * nursery_cap_object_scale_permille(mean) / 1000;

    // The two representations #7928 moved between.
    let reference = NURSERY_CAP_REFERENCE_OBJECT_BYTES; // 72 B
    let shrunk = 56; // #7928's two-field literal
    let objects_at_reference = band_of(reference) / reference;
    let objects_at_shrunk = band_of(shrunk) / shrunk;
    let drift = objects_at_reference.abs_diff(objects_at_shrunk) * 1000 / objects_at_reference;
    assert!(
        drift <= 5,
        "object budget must be representation-invariant: {objects_at_reference} objects at \
             {reference} B vs {objects_at_shrunk} at {shrunk} B ({drift} permille drift)"
    );

    // The band this replaces does NOT hold the ratio: 72 -> 56 is exactly
    // the 1.286x the issue measured. This is the sabotage arm inline — it
    // is what the assertion above is distinguishing itself from.
    let undenominated = base / shrunk * 1000 / (base / reference);
    assert!(
        undenominated >= 1250,
        "a byte-denominated band must inflate the object budget by ~72/56; measured \
             {undenominated} permille — if this is 1000 the two arms are the same and the \
             assertion above proves nothing"
    );
    reset_for_test();
}

/// The clamp is one-sided, and each endpoint is a named failure it exists
/// to prevent.
#[test]
fn object_scale_clamps_are_one_sided() {
    // No measurement yet: exactly today's band.
    assert_eq!(nursery_cap_object_scale_permille(0), 1000);
    // The calibration anchor is a no-op by construction.
    assert_eq!(
        nursery_cap_object_scale_permille(NURSERY_CAP_REFERENCE_OBJECT_BYTES),
        1000
    );
    // Shrunk representations scale proportionally.
    assert_eq!(nursery_cap_object_scale_permille(56), 777);
    assert_eq!(nursery_cap_object_scale_permille(54), 750);
    // Larger-than-reference is NOT scaled up: retain_wide (104 B) keeps
    // the band it has today...
    assert_eq!(nursery_cap_object_scale_permille(104), 1000);
    // ...and push_num's 3600 B array-dominated mean — the documented
    // #7929 blocker — cannot explode the cap ~50x.
    assert_eq!(nursery_cap_object_scale_permille(3600), 1000);
    // The floor bounds how far a small-survivor workload pulls the band
    // down (tree_wide measures 32.8 B; unclamped that is 456 permille).
    assert_eq!(nursery_cap_object_scale_permille(32), 500);
    assert_eq!(nursery_cap_object_scale_permille(8), 500);
}

/// The census is the only writer, and a cycle that moved nothing must not
/// be read as "mean size zero" — `tree`/`cycles` move single digits of
/// objects per minor and skip cycles entirely.
#[test]
fn census_carries_forward_across_a_cycle_that_moved_nothing() {
    reset_for_test();
    // #11549: a fresh thread paces at the ladder's floor, a quarter of the
    // base, and the object denomination scales that.
    let base = nursery_cap_floor_bytes();
    assert_eq!(
        influx_driven_nursery_cap_bytes(),
        base,
        "unmeasured process must pace exactly as the pre-#7929 collector did"
    );

    note_surviving_object_census(56 * 1000, 1000);
    let after_measurement = influx_driven_nursery_cap_bytes();
    assert_eq!(after_measurement, base * 777 / 1000);

    note_surviving_object_census(0, 0);
    note_surviving_object_census(4096, 0);
    assert_eq!(
        influx_driven_nursery_cap_bytes(),
        after_measurement,
        "a cycle with no moved objects is a missing measurement, not a zero one"
    );
    reset_for_test();
    assert_eq!(influx_driven_nursery_cap_bytes(), base);
}

/// #8122: the allocation census seeds the mean ONCE, only past half the
/// base cap, and never after a census of either kind exists.
///
/// Pure-policy half. The walk itself (a real nursery, real headers) is
/// driven in `gc::tests::copying::adaptive_tenuring`; this pins the gate
/// logic that decides whether the walk runs, in isolation from the heap.
#[test]
fn allocation_census_seed_is_gated_and_one_shot() {
    reset_for_test();
    let base = gc_scavenge_nursery_cap_bytes();
    assert!(!object_census_seeded_for_test());

    // Below half the base cap: nothing happens, the seed stays armed.
    maybe_seed_object_census_from_allocation(base / 2 - 1);
    assert!(
        !object_census_seeded_for_test(),
        "the probe must not fire below half the base cap"
    );
    assert_eq!(
        mean_surviving_object_bytes(),
        NURSERY_CAP_REFERENCE_OBJECT_BYTES
    );

    // A collector census arriving first disarms the probe for good: the
    // survivor mean is strictly better information than the allocation
    // mean, and the walk must never overwrite it.
    note_surviving_object_census(56 * 1000, 1000);
    assert!(object_census_seeded_for_test());
    maybe_seed_object_census_from_allocation(base);
    assert_eq!(
        mean_surviving_object_bytes(),
        56,
        "an allocation census must never replace a survivor census"
    );
    reset_for_test();
    assert!(
        !object_census_seeded_for_test(),
        "reset must re-arm the probe"
    );
}

/// The tenured-proportional term is representation-invariant by
/// cancellation (`tenured_bytes / 2` is `tenured_objects / 2` objects), so
/// the object denomination must apply to the constant band ONLY. If it
/// leaked into `scavenge_nursery_cap_from` the proportional arm would be
/// scaled twice.
#[test]
fn only_the_constant_band_is_re_denominated() {
    reset_for_test();
    note_surviving_object_census(56 * 1000, 1000);
    let tenured = 512 * MB;
    assert_eq!(
        scavenge_nursery_cap_from(influx_driven_nursery_cap_bytes(), tenured),
        tenured / TENURED_EDEN_DIVISOR,
        "the proportional arm must reach the max() unscaled"
    );
    reset_for_test();
}

#[test]
fn target_formula_matches_projected_occupancy() {
    // Largest S with (S-1) * influx <= desired.
    assert_eq!(compute_target_survivals(0, MB), 4);
    assert_eq!(compute_target_survivals(20 * 1024, MB), 4); // churn-like
    assert_eq!(compute_target_survivals(MB / 3 - 1, MB), 4);
    assert_eq!(compute_target_survivals(MB / 2, MB), 3);
    assert_eq!(compute_target_survivals(MB, MB), 2);
    assert_eq!(compute_target_survivals(MB + MB / 20, MB), 1); // tree-like
    assert_eq!(compute_target_survivals(16 * MB, MB), 1); // retain-like
}

#[test]
fn drops_immediately_and_rises_debounced() {
    reset_for_test();
    let desired = desired_survivor_bytes();
    // Power-on is the FLOOR now, not the ceiling (startup follow-up): the
    // ladder may not claim a lifetime in either direction without evidence.
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);

    // Give the loop its evidence, because this test is about the ladder's
    // ASYMMETRY and not about the startup gate. Two cycles with a cohort
    // that fully dies: the second rates the first, so a survivor round has
    // been measured, and 0 % survival keeps the lock out of it.
    retune_after_scavenge(desired * 2, 3 * desired, 0);
    retune_after_scavenge(desired * 2, 3 * desired, 0);
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);

    // Heavy influx: instant drop, no debounce. #9851 changed the FLOOR this
    // lands on (2, not 1 — the occupancy rule may not claim a lifetime), not
    // the asymmetry this test is named for: 4 -> 2 in one cycle is the same
    // "drops immediately" property that 4 -> 1 was. Heavy relative to the
    // desired size at the TOP of the cap ladder, which the warm-up above
    // has already climbed (#11549: a ladder move takes all its steps at
    // once), so the drop is to the floor whatever size the cap reached.
    let heavy = desired * 16;
    retune_after_scavenge(heavy, 0, 0);
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);

    // One quiet cycle: no rise yet (debounce).
    retune_after_scavenge(0, 0, 0);
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);
    // Second quiet cycle: rise by exactly one step, not to the target.
    retune_after_scavenge(0, 0, 0);
    assert_eq!(tenuring_survivals(), 3);

    // Heavy again: streak resets and threshold drops straight back.
    retune_after_scavenge(heavy, 0, 0);
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);

    // Sustained quiet recovers to the ceiling two cycles per step.
    for _ in 0..6 {
        retune_after_scavenge(0, 0, 0);
    }
    assert_eq!(tenuring_survivals(), 4);
    reset_for_test();
}

#[test]
fn steady_heavy_influx_is_a_fixed_point() {
    reset_for_test();
    // Heavy relative to the cap-scale CEILING (desired is cap/16, so at
    // the ×4 ceiling desired is base/4): the threshold must pin at 1 on
    // every cycle even while the cap scale walks up underneath it. An
    // influx only marginally above the base desired is a different case:
    // the growing cap re-classifies it as moderate, which is correct.
    // #9851: the fixed point is now the occupancy floor (2) rather than 1.
    // Fixed-POINTNESS is what this test protects — no oscillation while the
    // cap scale walks up underneath — and that is unchanged.
    let heavy = gc_scavenge_nursery_cap_bytes();
    for _ in 0..10 {
        retune_after_scavenge(heavy, 0, 0);
        assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);
    }
    assert_eq!(
        scavenge_nursery_cap_effective_bytes(),
        gc_scavenge_nursery_cap_bytes() * NURSERY_CAP_SCALE_MAX as usize
            / NURSERY_CAP_SCALE_UNIT as usize,
        "sustained heavy influx must also walk the cap to its ceiling"
    );
    reset_for_test();
}

/// #9851, both halves of the rule in one test, in the #7909 two-phase shape
/// so the decline is ATTRIBUTED rather than merely absent.
///
/// Phase 1 — the occupancy rule alone, on an influx far above `desired`,
/// must stop at 2 and NOT claim promote-on-first-copy. That is the whole
/// change: 2 is the lowest threshold that still puts a cohort through the
/// survivor space, so the loop keeps producing the measurement that could
/// refute it.
///
/// Phase 2 — the same heap, once a substantial cohort HAS come back fully
/// alive, must still reach 1 through the survival-rate lock. The rule
/// removes an unmeasured conclusion, not the measured one, and this half is
/// what makes it self-limiting rather than a blanket floor.
///
/// Sabotage: drop the `.max(OCCUPANCY_MIN_SURVIVALS)` in
/// `retune_after_scavenge` and phase 1 fails (the loop reports 1 with no
/// evidence). Drop the lock instead and phase 2 fails.
#[test]
fn occupancy_alone_never_claims_promote_on_first_copy_but_the_lock_still_can() {
    reset_for_test();
    let d = desired_survivor_bytes();

    // Phase 1: influx 16x the desired survivor size — the occupancy formula
    // computes 1 (integer division: 1 + desired/influx). No cohort has been
    // rated yet, so there is NO lifetime evidence on this heap.
    assert_eq!(
        compute_target_survivals(16 * d, d),
        1,
        "precondition: the occupancy ARITHMETIC still computes 1 — this \
             change clamps what the loop may do with it, not the formula"
    );
    for _ in 0..5 {
        retune_after_scavenge(16 * d, 0, 0);
        assert_eq!(
            tenuring_survivals(),
            OCCUPANCY_MIN_SURVIVALS,
            "occupancy measures SPACE and must not conclude promote-on-first-copy"
        );
    }
    assert!(
        !PROMOTE_LOCK.with(Cell::get),
        "and it must not have taken the lock's route to get there"
    );

    // Phase 2: now a substantial cohort goes through the survivor space and
    // comes back fully alive. THAT is lifetime evidence, and it must still
    // reach 1.
    //
    // TWO cycles, deliberately: the lock rates the fresh cohort's survival
    // against the PREVIOUS cycle's `eden_copied_bytes` (`PREV_COPIED_BYTES`),
    // so the first call is what puts a cohort in the survivor space and the
    // second is what reports that same cohort coming back alive. Both
    // arguments here are cohort-scoped, which is what the follow-up to
    // #9851 made them: the whole survivor space is a different population
    // once the threshold rises above 2. Phase 1 above copied nothing, so
    // there is nothing to rate until this pair runs — which is precisely the
    // blindness the change is about, here as a test mechanic.
    retune_after_scavenge(16 * d, 3 * d, 0);
    assert_eq!(
        tenuring_survivals(),
        OCCUPANCY_MIN_SURVIVALS,
        "one cohort into the survivor space is not yet evidence about it"
    );
    retune_after_scavenge(16 * d, 3 * d, 3 * d);
    assert_eq!(
        tenuring_survivals(),
        1,
        "a substantial intake that fully survives its round must still lock \
             promote-on-first-copy — the measured path is untouched"
    );
    assert!(PROMOTE_LOCK.with(Cell::get), "...through the lock");
    reset_for_test();
}

/// STARTUP FOLLOW-UP — the occupancy rule may not claim the CEILING either.
///
/// `1 + desired / influx` returns the ceiling for a tiny influx and for a
/// zero one, so on the first minors of a process — heap nearly empty, no
/// cohort ever followed — occupancy claims the maximum. That is the same
/// category error as claiming 1: a statement about LIFETIME derived from a
/// measurement of SPACE, made before any evidence exists. It is also the
/// expensive direction, because every survivor is then copied up to three
/// times before it may be promoted.
///
/// Measured on the compiled claude-code TUI, this was the WHOLE difference
/// between the adaptive loop and a pinned threshold: a single startup
/// excursion `4 -> 2 (occupancy) -> 1 (lock)` inside turn 1, nothing
/// afterwards, worth +0.35..0.45 s at 3300 characters and +50 % at 400.
///
/// Sabotage: delete the `SURVIVOR_ROUND_MEASURED` gate in
/// `retune_after_scavenge` (or restore the power-on value to
/// `GC_TENURING_SURVIVALS_MAX`) and phase 1 fails — the loop reports the
/// ceiling on a heap where nothing has ever been rated.
#[test]
fn occupancy_may_not_claim_the_ceiling_before_any_round_is_measured() {
    reset_for_test();
    let d = desired_survivor_bytes();

    // Precondition: the ARITHMETIC still says "ceiling" for a startup-sized
    // influx. This change gates what the loop may do with that, exactly as
    // #9851 did at the other end of the range.
    assert_eq!(compute_target_survivals(0, d), GC_TENURING_SURVIVALS_MAX);
    assert_eq!(
        compute_target_survivals(d / 64, d),
        GC_TENURING_SURVIVALS_MAX
    );

    // Phase 1: power-on, then many startup-shaped minors — tiny influx,
    // nothing copied, so nothing rateable. The loop must sit at the floor
    // and never climb.
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);
    for _ in 0..8 {
        retune_after_scavenge(d / 64, 0, 0);
        assert_eq!(
            tenuring_survivals(),
            OCCUPANCY_MIN_SURVIVALS,
            "no survivor round has been rated, so occupancy has no lifetime \
                 evidence and may not leave the floor"
        );
    }
    assert!(
        !PROMOTE_LOCK.with(Cell::get),
        "and it must not have reached the floor via the lock either"
    );

    // Phase 2: once a cohort has actually gone through the survivor space
    // and been followed, the ladder is allowed to move again. A cohort that
    // fully dies keeps the lock out, so what is observed here is the
    // occupancy rule being re-enabled and nothing else.
    retune_after_scavenge(d / 64, 3 * d, 0);
    retune_after_scavenge(d / 64, 3 * d, 0);
    for _ in 0..8 {
        retune_after_scavenge(d / 64, 0, 0);
    }
    assert_eq!(
        tenuring_survivals(),
        GC_TENURING_SURVIVALS_MAX,
        "with a round measured and the influx quiet, the debounced rise must \
             still reach the ceiling — the gate delays the claim until there is \
             evidence, it does not remove the ladder"
    );
    reset_for_test();
}

/// #9851: a cohort that DIES in its survivor round must keep the loop at the
/// occupancy floor rather than being locked to 1 — the case cc actually is.
/// Measured there: 26.1 % of each cohort dies in one survivor round, in
/// steady state, on 393 samples; the lock needs >=90 % survival, so it
/// correctly stays out.
///
/// The arguments are the FRESH COHORT's intake and survival (#9851
/// follow-up). Fed the whole survivor space instead — which is what the
/// call site used to pass — this same heap locks, because above a threshold
/// of 2 that space also holds objects already selected for longevity. That
/// is not a hypothetical: on cc the clamp alone left 85 % of promotion at
/// S=1, reached through this lock 8-12 times per four-turn run.
#[test]
fn a_cohort_that_dies_in_its_round_holds_at_the_occupancy_floor() {
    reset_for_test();
    let d = desired_survivor_bytes();
    // Heavy influx (occupancy says 1) AND a substantial cohort of which
    // ~26 % dies — cc's steady state, in miniature.
    for _ in 0..8 {
        retune_after_scavenge(16 * d, 4 * d, 3 * d);
    }
    assert!(
        !PROMOTE_LOCK.with(Cell::get),
        "74 % survival is below the lock's 90 % bar: the lock must stay out"
    );
    assert_eq!(
        tenuring_survivals(),
        OCCUPANCY_MIN_SURVIVALS,
        "so the loop holds at the occupancy floor and keeps aging the cohort"
    );
    reset_for_test();
}

#[test]
fn survival_rate_lock_breaks_a_saturated_pipeline() {
    reset_for_test();
    let d = desired_survivor_bytes();
    // tree.ts steady state: influx sits JUST under desired (occupancy
    // alone settles at S=2), the survivor space holds 3 cohorts, and
    // 100% of every intake comes back out alive.
    let influx = d - 64;
    retune_after_scavenge(influx, 3 * d, 3 * d);
    assert_eq!(
        tenuring_survivals(),
        2,
        "first cycle has no prior intake to rate, so occupancy decides"
    );
    retune_after_scavenge(influx, 3 * d, 3 * d);
    assert_eq!(
        tenuring_survivals(),
        1,
        "a substantial intake that fully survives its round must lock promote-on-first-copy"
    );
    // The lock holds through occupancy readings that would say S=2.
    for _ in 0..5 {
        retune_after_scavenge(influx, 0, 0);
        assert_eq!(tenuring_survivals(), 1);
    }
    // Quiet influx exits the lock after the debounce, resuming the
    // ladder one step up rather than snapping to the ceiling.
    retune_after_scavenge(0, 0, 0);
    assert_eq!(tenuring_survivals(), 1);
    retune_after_scavenge(0, 0, 0);
    assert_eq!(tenuring_survivals(), 2);
    for _ in 0..4 {
        retune_after_scavenge(0, 0, 0);
    }
    assert_eq!(tenuring_survivals(), 4);
    reset_for_test();
}

#[test]
fn cap_scale_grows_on_heavy_influx_and_shrinks_when_quiet() {
    reset_for_test();
    // #11549: a fresh thread starts at the floor, a quarter of the base.
    let floor = nursery_cap_floor_bytes();
    let base = gc_scavenge_nursery_cap_bytes();
    assert_eq!(floor, base / 4);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    // Influx above 4% of the cap: one debounce cycle, then a ×2 step.
    retune_after_scavenge(floor / 15, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    retune_after_scavenge(floor / 15, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    // Growth halves the observed ratio into the dead band: stable.
    retune_after_scavenge(floor / 15, 0, 0);
    retune_after_scavenge(floor / 15, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    // A heavier influx takes every step the one-step rule would take in a
    // row, in ONE debounced move. 7% of the base is above 4% of 8 MB and
    // of 16 MB but below 4% of 32 MB, so the move lands at 32 MB — where
    // one-step moves would have settled — and not at the ceiling.
    retune_after_scavenge(base * 7 / 100, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    retune_after_scavenge(base * 7 / 100, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 2);
    // Heavier still reaches the ×4-of-base ceiling and stops there.
    for _ in 0..4 {
        retune_after_scavenge(base, 0, 0);
    }
    assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 4);
    // A quiet reading walks back down in one debounced move, as far as
    // the reading stays under 1% of each level: 0.6% of 64 MB is 1.2% of
    // 32 MB, so this move is a single step.
    let reading = base * 4 * 6 / 1000;
    retune_after_scavenge(reading, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 4);
    retune_after_scavenge(reading, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 2);
    // A dead-quiet reading goes all the way to the floor, never below.
    for _ in 0..2 {
        retune_after_scavenge(0, 0, 0);
    }
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    for _ in 0..4 {
        retune_after_scavenge(0, 0, 0);
    }
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    reset_for_test();
}

/// EK (2026-10-23): a sawtooth influx is judged by its phase mean, not by
/// where on the tooth the minors land. The readings follow AWFY Storage's
/// at 8 MB: each tooth climbs past 4% for two minors and drops below 1% for
/// two, and its mean is inside the band, so the ladder must hold in both
/// directions.
#[test]
fn a_sawtooth_influx_is_judged_by_its_phase_mean() {
    reset_for_test();
    let floor = nursery_cap_floor_bytes();
    let two = floor * 2;
    // Leave the floor on a steady 6% so the test starts at 8 MB.
    retune_after_scavenge(floor * 6 / 100, 0, 0);
    retune_after_scavenge(floor * 6 / 100, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), two);
    let tooth = [
        two * 2 / 100,
        two * 25 / 1000,
        two * 3 / 100,
        two * 35 / 1000,
        two * 39 / 1000,
        two * 44 / 1000,
        two * 48 / 1000,
        two / 200,
        two / 150,
        two * 15 / 1000,
    ];
    // The tip and the trough really are two readings beyond each edge: the
    // per-reading rule alone would grow at one and shrink at the other.
    assert!(tooth[5] > two / 25 && tooth[6] > two / 25 && tooth[4] < two / 25);
    assert!(tooth[7] < two / 100 && tooth[8] < two / 100 && tooth[9] > two / 100);
    for _ in 0..6 {
        for &reading in &tooth {
            retune_after_scavenge(reading, 0, 0);
            assert_eq!(scavenge_nursery_cap_effective_bytes(), two);
        }
    }
    reset_for_test();
}

/// EK (2026-10-23): a reading on the far side of the band from the phase
/// mean starts a new phase, so a long quiet phase does not delay the
/// response to a heavy one: two heavy readings still move the ladder.
#[test]
fn a_heavy_phase_after_a_long_quiet_one_moves_after_two_readings() {
    reset_for_test();
    let floor = nursery_cap_floor_bytes();
    for _ in 0..64 {
        retune_after_scavenge(floor / 1000, 0, 0);
    }
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    retune_after_scavenge(floor * 6 / 100, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    retune_after_scavenge(floor * 6 / 100, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    // And back: at 8 MB, a quiet phase after the heavy one shrinks after
    // two quiet readings.
    for _ in 0..2 {
        retune_after_scavenge(floor * 3 / 100, 0, 0);
    }
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    retune_after_scavenge(0, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    retune_after_scavenge(0, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    reset_for_test();
}

/// EK (2026-10-23): one heavy cycle — a startup cohort — moves nothing
/// even while it holds the phase mean past the edge: the readings after
/// it alternate around 4% and are never above it twice in a row.
#[test]
fn a_startup_cohort_alone_does_not_leave_the_floor() {
    reset_for_test();
    let floor = nursery_cap_floor_bytes();
    retune_after_scavenge(floor / 4, 0, 0);
    for _ in 0..8 {
        retune_after_scavenge(floor * 3 / 100, 0, 0);
        retune_after_scavenge(floor * 5 / 100, 0, 0);
        assert_eq!(scavenge_nursery_cap_effective_bytes(), floor);
    }
    reset_for_test();
}

/// EK (2026-10-23): a move lands where the current reading and the mean
/// both put it. A startup cohort keeps the mean high; the landing may not
/// follow it past what the reading justifies (AWFY Json/CD went from the
/// floor straight to 16 MB that way).
#[test]
fn a_startup_cohort_does_not_carry_the_landing() {
    reset_for_test();
    let floor = nursery_cap_floor_bytes();
    // 40% then 6% of the floor: the mean (23%) is above 4% of 16 MB, the
    // reading (6%) only above 4% of the floor.
    retune_after_scavenge(floor * 40 / 100, 0, 0);
    retune_after_scavenge(floor * 6 / 100, 0, 0);
    assert_eq!(scavenge_nursery_cap_effective_bytes(), floor * 2);
    reset_for_test();
}

// GCL (2026-10-23): the whole-cycle rule (T2). These drive the ladder with
// byte readings stated as percentages of the current level's band edge
// (the design states them at a 2/4 grow edge G = 400 000 B). "Dead"
// readings follow a copied cohort none of which survived its first round.
const COHORT: usize = 64 * 1024;

fn cap_scale() -> u8 {
    NURSERY_CAP_SCALE.with(TriggerInput::get)
}

fn pct(edge: usize, percent: usize) -> usize {
    edge * percent / 100
}

fn dead(reading: usize) {
    retune_after_scavenge(reading, COHORT, 0);
}

/// Climb from the floor to `scale` on unfollowed readings (3% of the
/// target level's cap, above 4% of every level below it), the last of
/// which copies a cohort when `seed` is set, so the first reading of the
/// test is followed.
fn start_at(scale: u8, seed: bool) {
    reset_for_test();
    let reading = nursery_cap_floor_bytes() * scale as usize * 3 / 100;
    retune_after_scavenge(reading, 0, 0);
    retune_after_scavenge(reading, if seed { COHORT } else { 0 }, 0);
    assert_eq!(cap_scale(), scale);
}

/// The current level's grow edge (4% of its cap) and shrink edge (1%).
fn grow_edge() -> usize {
    scavenge_nursery_cap_effective_bytes() / 25
}

fn shrink_edge() -> usize {
    scavenge_nursery_cap_effective_bytes() / 100
}

/// Feed dead readings (percent of the edge at the start) and assert the
/// scale after each one.
fn dead_trail(edge: usize, percents: &[usize], scales: &[u8]) {
    assert_eq!(percents.len(), scales.len());
    for (k, (&percent, &scale)) in percents.iter().zip(scales).enumerate() {
        dead(pct(edge, percent));
        assert_eq!(cap_scale(), scale, "after reading {}", k + 1);
    }
}

/// Storage's beat: a run of dead readings that rises through the grow
/// edge and wraps. Its rising half holds a phase mean beyond the edge for
/// two readings in a row (plain EK21 grows at reading 3), but the whole
/// cycle's level is inside the band.
#[test]
fn a_drifting_beat_with_dead_cohorts_is_not_a_level() {
    start_at(2, true);
    let g = grow_edge();
    dead_trail(g, &[90, 105, 120, 30, 90, 105, 120, 30], &[2; 8]);
    reset_for_test();
}

/// The same rise with 10% of each cohort surviving is a structure
/// accumulating, so it is a level and grows as under EK21.
#[test]
fn a_rising_run_whose_cohorts_survive_still_grows() {
    start_at(2, true);
    let g = grow_edge();
    retune_after_scavenge(pct(g, 90), COHORT, COHORT / 10);
    retune_after_scavenge(pct(g, 105), COHORT, COHORT / 10);
    assert_eq!(cap_scale(), 2);
    retune_after_scavenge(pct(g, 120), COHORT, COHORT / 10);
    assert!(cap_scale() > 2);
    reset_for_test();
}

/// A flat heavy phase of dead cohorts is a cycle of one reading: the
/// third reading is its first boundary and the fourth closes it. Only a
/// whole cycle is a level (GCL T2), so it grows at the fourth reading,
/// not at EK21's second.
#[test]
fn a_flat_heavy_phase_of_dead_cohorts_grows_after_one_whole_cycle() {
    start_at(2, true);
    let g = grow_edge();
    for _ in 0..3 {
        dead(pct(g, 105));
        assert_eq!(cap_scale(), 2);
    }
    dead(pct(g, 105));
    assert!(cap_scale() > 2);
    reset_for_test();
}

/// The mirror of the beat: a run of dead readings falling through the
/// shrink edge holds the level; plain EK21 shrinks at reading 3.
#[test]
fn a_falling_run_of_dead_cohorts_does_not_shrink() {
    start_at(4, true);
    let s = shrink_edge();
    dead_trail(s, &[110, 95, 80, 300, 110, 95, 80, 300], &[4; 8]);
    reset_for_test();
}

/// A reading whose previous cohort was not copied measured no lifetime:
/// it is not a death, so the phase is EK21's and the rise grows.
#[test]
fn an_unfollowed_cohort_is_not_a_death() {
    start_at(2, false);
    let g = grow_edge();
    retune_after_scavenge(pct(g, 90), 0, 0);
    retune_after_scavenge(pct(g, 105), 0, 0);
    assert_eq!(cap_scale(), 2);
    retune_after_scavenge(pct(g, 120), 0, 0);
    assert!(cap_scale() > 2);
    reset_for_test();
}

/// A move starts a new phase: the cycles measured at 2/4 are not the
/// level at 4/4. Carried over, they would hold the 4/4 grow.
#[test]
fn the_whole_cycles_start_fresh_at_a_move() {
    start_at(2, true);
    let g = grow_edge();
    dead_trail(g, &[112, 125, 112, 125, 112], &[2, 2, 2, 2, 4]);
    let g = grow_edge();
    dead_trail(g, &[112, 125, 112, 125, 112], &[4, 4, 4, 4, 8]);
    reset_for_test();
}

/// Storage at 1/4 (GCL9): a sawtooth whose trough lies below the shrink
/// edge and whose whole cycle's mean is beyond the grow edge. The far-side
/// restart at each wrap does not cut the cycle, so the phase grows once a
/// whole cycle has closed (T1 held it at every wrap; EK21 grows at
/// reading 5 on the first rising half).
#[test]
fn a_sawtooth_crossing_the_far_edge_grows_on_its_whole_cycle_mean() {
    start_at(2, true);
    let g = grow_edge();
    assert!(pct(g, 20) < shrink_edge());
    let tooth = [20, 62, 105, 148, 190];
    let percents: Vec<usize> = tooth.iter().cycle().take(14).copied().collect();
    let mut scales = [2u8; 14];
    scales[13] = 4;
    dead_trail(g, &percents, &scales);
    reset_for_test();
}

/// Storage at 2/4 (GCL9): the wrap after a heavy half restarts EK21's
/// mean, and the two trough readings that follow are a fragment of the
/// cycle, not its level. T1 and EK21 shrink at reading 5.
#[test]
fn a_trough_fragment_does_not_shrink() {
    start_at(2, true);
    let g = grow_edge();
    assert!(pct(g, 10) < shrink_edge());
    dead_trail(g, &[95, 105, 115, 1, 10], &[2; 5]);
    reset_for_test();
}

/// A phase that starts on a cycle's heavy half: the readings before the
/// first boundary are a fragment (EK21 grows at reading 2), and the first
/// whole cycle is inside the band.
#[test]
fn a_phase_that_starts_mid_cycle_waits_for_its_first_whole_one() {
    start_at(2, true);
    let g = grow_edge();
    dead_trail(g, &[105, 120, 30, 90, 105, 120, 30], &[2; 7]);
    reset_for_test();
}

/// The boundaries compare bytes. The cap a reading is judged against
/// moves between readings with the object census; a flat run of bytes
/// whose share of a growing cap falls once still closes its cycle at the
/// fourth reading.
#[test]
fn a_growing_cap_does_not_move_a_cycle_boundary() {
    start_at(2, true);
    note_surviving_object_census(60 * 1000, 1000);
    let g1 = grow_edge();
    let reading = pct(g1, 110);
    dead(reading);
    note_surviving_object_census(63 * 1000, 1000);
    let g2 = grow_edge();
    // The shape: the same bytes, beyond the new edge, a smaller share.
    assert!(g2 > g1 && reading > g2);
    for _ in 0..2 {
        dead(reading);
        assert_eq!(cap_scale(), 2);
    }
    dead(reading);
    assert!(cap_scale() > 2);
    reset_for_test();
}

/// #11549: a ladder move lands where one-step moves would settle, so a
/// steady influx reaches the same size it always did — and never
/// oscillates. Checked exhaustively over influxes from the floor's dead
/// band to far above the ceiling's.
#[test]
fn multi_step_moves_land_in_the_dead_band_and_hold() {
    let base = gc_scavenge_nursery_cap_bytes();
    let floor = nursery_cap_floor_bytes();
    for permille_of_base in (0..=2000).step_by(7) {
        reset_for_test();
        let influx = base * permille_of_base / 1000;
        for _ in 0..4 {
            retune_after_scavenge(influx, 0, 0);
        }
        let settled = scavenge_nursery_cap_effective_bytes();
        assert!(settled >= floor && settled <= base * 4);
        // In the dead band, unless pinned at an end of the ladder.
        assert!(
            influx <= settled / 25 || settled == base * 4,
            "influx {influx} still above 4% of the {settled} cap it settled at"
        );
        assert!(
            influx >= settled / 100 || settled == floor,
            "influx {influx} still below 1% of the {settled} cap it settled at"
        );
        // The smallest such level: one step down would be above 4%.
        assert!(
            settled == floor || influx > settled / 2 / 25,
            "influx {influx} overshot: {settled} is bigger than the one-step rule settles at"
        );
        // Fixed point: many more cycles of the same influx move nothing.
        for _ in 0..8 {
            retune_after_scavenge(influx, 0, 0);
            assert_eq!(scavenge_nursery_cap_effective_bytes(), settled);
        }
    }
    reset_for_test();
}

#[test]
fn dying_survivor_cohorts_do_not_lock() {
    reset_for_test();
    let d = desired_survivor_bytes();
    // Medium-lived objects: a substantial intake of which only half
    // survives its survivor round. Aging is filtering — the lock must
    // stay out and the occupancy ladder must age from the power-on floor.
    let mut seen = Vec::new();
    for _ in 0..6 {
        retune_after_scavenge(d / 2, d / 2, d / 4);
        assert!(
            !PROMOTE_LOCK.with(Cell::get),
            "50% survival is below the lock's 90% bar"
        );
        seen.push(tenuring_survivals());
    }
    assert_eq!(
        seen,
        [2, 2, 3, 3, 3, 3],
        "power-on is the floor now, not the ceiling: after a survivor round \
             is measured, the debounced occupancy ladder must keep the dying \
             cohort aging rather than claim promote-on-first-copy"
    );
    reset_for_test();
}

// ── #7596's tenured-proportional cap term ───────────────────────────
//
// #7596 added `max(influx_driven, old_gen_reclaimable / 2)` to
// `scavenge_nursery_cap_effective_bytes` with no test of its own. The
// sibling `cap_scale_grows_on_heavy_influx_and_shrinks_when_quiet` looks
// like coverage but is not: it asserts against the effective cap in a
// unit-test thread whose old-gen is ~empty, so the proportional term
// contributes 0 and the test stays green with that term deleted. These
// two exercise the policy directly.

#[test]
fn nursery_cap_keeps_the_influx_floor_below_the_crossover() {
    // #7377's guarantee: a program whose tenured live set is small keeps
    // the configured 16 MB base. If the `.max()` were dropped — leaving
    // only `tenured / 2` — a small-live-set program would collect on a
    // near-zero cap, which is the regression #7377 fixed.
    let base = gc_scavenge_nursery_cap_bytes();
    assert_eq!(scavenge_nursery_cap_from(base, 0), base);
    assert_eq!(scavenge_nursery_cap_from(base, 1), base);
    // Just below the crossover the floor still decides.
    assert_eq!(
        scavenge_nursery_cap_from(base, base * TENURED_EDEN_DIVISOR - 1),
        base
    );
    // Exactly at it the two terms coincide.
    assert_eq!(
        scavenge_nursery_cap_from(base, base * TENURED_EDEN_DIVISOR),
        base
    );
    // The floor tracks the influx-driven scale, not the raw base: at the
    // ×4 ceiling a 3×base tenured set is still below the crossover.
    assert_eq!(scavenge_nursery_cap_from(base * 4, base * 3), base * 4);
}

#[test]
fn nursery_cap_becomes_tenured_proportional_above_the_crossover() {
    // The #7592 shape: `json_pipeline` at 500k promotes ~400 MB. With a
    // constant cap the young collection count is linear in bytes
    // allocated and each collection is O(live) — quadratic total. Above
    // the crossover the cap must BE `tenured / 2`, so `old_{n+1} ≈
    // old_n × 1.5` and the count is logarithmic instead.
    //
    // If the proportional term were deleted, every assertion here would
    // read `base` and fail.
    let base = gc_scavenge_nursery_cap_bytes();
    let tenured = base * 50;
    assert_eq!(
        scavenge_nursery_cap_from(base, tenured),
        tenured / TENURED_EDEN_DIVISOR
    );
    // Strictly above the floor, and exactly the divisor — not merely
    // "bigger than base", which a wrong divisor would also satisfy.
    assert!(scavenge_nursery_cap_from(base, tenured) > base);
    assert_eq!(
        scavenge_nursery_cap_from(base, 400 * 1024 * 1024),
        200 * 1024 * 1024
    );
    // A larger influx-driven term still wins when it is the bigger of
    // the two: the policy is a max, not a takeover.
    assert_eq!(
        scavenge_nursery_cap_from(tenured, tenured),
        tenured,
        "the proportional term must never LOWER the cap"
    );
}

// ── #7598's mark-sweep seed ─────────────────────────────────────────
//
// The lock above cannot engage before the SECOND copying minor. These
// exercise the seed that reads the same proof off a completed mark-sweep,
// one collection earlier.

#[test]
fn sweep_seed_decides_before_the_first_copying_minor_snapshots_the_threshold() {
    // `copying.rs` snapshots `tenuring_survivals()` in
    // `CopyingNurseryCollector::new`, so the only value that can change
    // what the first big minor does is the one standing BEFORE any
    // `retune_after_scavenge` for that cycle has run. That is precisely
    // what the survival-rate lock cannot reach and this seed can.
    reset_for_test();
    let d = desired_survivor_bytes();
    let eden_live = d * 4;
    assert_eq!(
        tenuring_survivals(),
        OCCUPANCY_MIN_SURVIVALS,
        "power-on is the floor now, not the ceiling: with no lifetime \
             evidence the loop may not claim either extreme"
    );

    seed_promote_lock_from_sweep(eden_live, eden_live / 50);
    assert_eq!(
        tenuring_survivals(),
        1,
        "the copying minor must ENTER at S=1, not be retuned to it afterwards"
    );
    reset_for_test();
}

#[test]
fn sweep_seed_refuses_a_churn_eden() {
    // The stated failure mode, guarded: a big Eden is not a survival
    // signal. #7592 recorded exactly this trap ("Eden in-use at cycle
    // start is not a survival signal — churn workloads also overshoot
    // Eden, with ~0% survival"). Occupancy alone would say S=1 here.
    reset_for_test();
    let d = desired_survivor_bytes();
    let eden_live = d * 4;
    let eden_dead = eden_live * 9;
    assert_eq!(
        compute_target_survivals(eden_live, d),
        1,
        "precondition: occupancy alone says lock here, so this test is \
             exercising the survival-rate half and not passing vacuously"
    );
    seed_promote_lock_from_sweep(eden_live, eden_dead);
    assert_eq!(
        tenuring_survivals(),
        OCCUPANCY_MIN_SURVIVALS,
        "10% Eden survival must leave the loop at the power-on floor, not \
             seed promote-on-first-copy by claiming a threshold below 2"
    );
    reset_for_test();
}

#[test]
fn sweep_seed_refuses_a_small_fully_live_eden() {
    // The other half: a nursery that is 100% live but far under the
    // survivor target fits the survivor space, so aging still filters and
    // the ladder must decide. Right after a scavenge this is the NORMAL
    // reading, and seeding off it would lock every program at S=1.
    reset_for_test();
    let d = desired_survivor_bytes();
    seed_promote_lock_from_sweep(d / 8, 0);
    // Unchanged from power-on, which is the floor now rather than the
    // ceiling (startup follow-up). The property under test is that the
    // sweep seed REFUSED — it left the threshold where it found it.
    assert_eq!(tenuring_survivals(), OCCUPANCY_MIN_SURVIVALS);
    reset_for_test();
}

#[test]
fn sweep_seed_rule_is_a_pure_function_of_the_census() {
    // Table over the policy itself, with no heap state involved — the
    // sibling `scavenge_nursery_cap_from` exists for the same reason
    // (#7024: a test whose subject never ran).
    let d = 1024 * 1024;
    // Both conditions met.
    assert!(full_seed_promotes_on_first_copy(4 * d, d / 10, d));
    // Exactly at the 90% survival boundary: 9 live, 1 dead.
    assert!(full_seed_promotes_on_first_copy(9 * d, d, d));
    // One byte under it.
    assert!(!full_seed_promotes_on_first_copy(9 * d - 1, d + 1, d));
    // Occupancy says the cohort fits the survivor space.
    assert!(!full_seed_promotes_on_first_copy(d / 2, 0, d));
    // An empty census decides nothing (and must not divide by zero).
    assert!(!full_seed_promotes_on_first_copy(0, 0, d));
    assert!(!full_seed_promotes_on_first_copy(0, 4 * d, d));
}

#[test]
fn sweep_seed_hands_over_to_the_existing_unlock_path() {
    // The seed sets the lock and nothing else: exit stays the influx
    // signal, which is measurable at S=1 (survivor occupancy is not).
    // Without this the seed would need an exit condition of its own, and
    // the obvious one — "Eden stopped being mostly live" — reads
    // differently at S=1 than at S=4 and would oscillate.
    reset_for_test();
    let d = desired_survivor_bytes();
    seed_promote_lock_from_sweep(d * 4, 0);
    assert_eq!(tenuring_survivals(), 1);

    // Substantial influx holds the lock, exactly as if it had been set by
    // the survivor round-trip.
    for _ in 0..4 {
        retune_after_scavenge(d * 4, 0, 0);
        assert_eq!(tenuring_survivals(), 1);
    }
    // Quiet influx exits after the same debounce, to the same S=2.
    retune_after_scavenge(0, 0, 0);
    assert_eq!(tenuring_survivals(), 1);
    retune_after_scavenge(0, 0, 0);
    assert_eq!(tenuring_survivals(), 2);
    reset_for_test();
}

#[test]
fn effective_nursery_cap_is_the_two_term_policy() {
    // Wiring pin: the effective accessor must be the composition, so a
    // future edit that inlines one term and drops the other cannot pass
    // the two policy tests above while shipping a different cap.
    //
    // Honest about its limits: on a quiescent test thread old-gen is
    // ~empty, so this cannot distinguish the two terms by value — it only
    // proves the accessor and the policy agree on the live inputs, and
    // that the #7377 floor survives whatever old-gen happens to be.
    reset_for_test();
    let expected = scavenge_nursery_cap_from(
        influx_driven_nursery_cap_bytes(),
        old_gen_reclaimable_pressure_bytes(),
    );
    assert_eq!(scavenge_nursery_cap_effective_bytes(), expected);
    assert!(scavenge_nursery_cap_effective_bytes() >= nursery_cap_floor_bytes() / 2);
    reset_for_test();
}
