//! Teeth for seeded GC-schedule fuzzing (`PERRY_GC_SCHEDULE_SEED`,
//! `PERRY_GC_SCHEDULE_RATE`).
//!
//! The mode's value rests on exactly two claims, so both are asserted directly
//! rather than inferred:
//!
//! 1. **A failing seed reproduces.** The schedule is a pure function of
//!    `(seed, counter)`, so the same seed selects the same safepoints — checked
//!    over 100k ordinals, not three.
//! 2. **Unset is inert.** Per CLAUDE.md's GC knob kill-policy, every knob's OFF
//!    state is exercised: `PERRY_GC_FORCE_EVACUATE` was inert for every
//!    `gc()`-driven test for months because only its ON arm was ever asserted,
//!    and the ON arm did nothing.
//!
//! A third claim — that the density is actually tunable — is what separates this
//! from zeal, so the realised rate is measured against the requested one instead
//! of being taken on trust from the arithmetic.

use super::super::schedule::*;
use super::super::*;
use super::support::*;

// ---------------------------------------------------------------------------
// Knob parsing — pure, so both states of both knobs are asserted without
// touching the process environment (the live reader caches in a `OnceLock`, so
// a test that set an env var would be at the mercy of which test ran first).
// ---------------------------------------------------------------------------

#[test]
fn seed_knob_reads_as_off_unless_it_parses_as_u64() {
    // OFF is the default and every unparseable spelling. A typo must not
    // silently enable a mode that changes when the collector runs — and it must
    // not silently become seed 0 either, because then a mistyped sweep would
    // report N runs of one schedule as N distinct seeds.
    for raw in [
        None,
        Some(""),
        Some("   "),
        Some("banana"),
        Some("-1"),
        Some("1.5"),
        Some("0x10"),
        Some("18446744073709551616"), // u64::MAX + 1
    ] {
        assert_eq!(
            parse_seed(raw),
            None,
            "{raw:?} must leave seeded GC-schedule fuzzing OFF"
        );
    }
    assert_eq!(parse_seed(Some("0")), Some(0));
    assert_eq!(parse_seed(Some("42")), Some(42));
    assert_eq!(
        parse_seed(Some(" 42 ")),
        Some(42),
        "surrounding space is fine"
    );
    assert_eq!(parse_seed(Some("18446744073709551615")), Some(u64::MAX));
}

#[test]
fn rate_knob_defaults_and_clamps() {
    for raw in [None, Some(""), Some("banana"), Some("nan")] {
        assert_eq!(
            parse_rate(raw),
            DEFAULT_SCHEDULE_RATE,
            "{raw:?} must fall back to the documented default"
        );
    }
    assert_eq!(parse_rate(Some("0")), 0.0);
    assert_eq!(parse_rate(Some("1")), 1.0);
    assert_eq!(parse_rate(Some("0.25")), 0.25);
    // Clamped, not rejected: a `2` should read as "everything", not silently
    // revert to 5% and leave the operator believing they turned the mode up.
    assert_eq!(parse_rate(Some("2")), 1.0);
    assert_eq!(parse_rate(Some("-3")), 0.0);
    assert_eq!(parse_rate(Some("inf")), 1.0);
}

#[test]
fn rate_zero_never_collects_and_rate_one_always_does() {
    let never = rate_threshold(0.0);
    let always = rate_threshold(1.0);
    assert_eq!(never, 0, "rate 0 must be expressible as a threshold of 0");
    for counter in 0..10_000u64 {
        assert!(
            !schedule_hit(12345, counter, never),
            "rate 0 must select nothing (counter={counter})"
        );
        assert!(
            schedule_hit(12345, counter, always),
            "rate 1 must select every safepoint — zeal density, exactly \
             (counter={counter})"
        );
    }
}

// ---------------------------------------------------------------------------
// Determinism: the property the whole mode exists for.
// ---------------------------------------------------------------------------

/// The reproducer guarantee. If this can fail, a "failing seed" is a rumour.
#[test]
fn the_same_seed_selects_the_same_safepoints() {
    let threshold = rate_threshold(0.05);
    for seed in [0u64, 1, 7, 4242, u64::MAX] {
        let first: Vec<u64> = (0..100_000u64)
            .filter(|&counter| schedule_hit(seed, counter, threshold))
            .collect();
        let second: Vec<u64> = (0..100_000u64)
            .filter(|&counter| schedule_hit(seed, counter, threshold))
            .collect();
        assert_eq!(
            first, second,
            "seed {seed} must select an identical safepoint set every time"
        );
        assert!(
            !first.is_empty(),
            "seed {seed} selected nothing at rate 0.05 over 100k safepoints — \
             the determinism check would be vacuous"
        );
    }
}

/// A sweep runs adjacent seeds (`1`, `2`, `3`, …). If adjacent seeds produced
/// near-identical schedules the sweep would be re-running one experiment under
/// different names — the exact failure the mode exists to avoid. The seed is
/// mixed through SplitMix64 before it meets the counter for this reason.
#[test]
fn adjacent_seeds_explore_different_schedules() {
    let threshold = rate_threshold(0.05);
    const N: u64 = 20_000;
    for seed in 1..8u64 {
        let a: Vec<bool> = (0..N).map(|c| schedule_hit(seed, c, threshold)).collect();
        let b: Vec<bool> = (0..N)
            .map(|c| schedule_hit(seed + 1, c, threshold))
            .collect();
        let hits_a = a.iter().filter(|hit| **hit).count();
        let shared = a.iter().zip(&b).filter(|(one, two)| **one && **two).count();
        // Independent schedules at rate r share ~r of each other's hits. Allow
        // generous slack (25%) — the assertion that matters is "not most of
        // them", i.e. the two runs are genuinely different experiments.
        assert!(
            shared * 4 < hits_a,
            "seeds {seed} and {} agree on {shared} of {hits_a} selected \
             safepoints — a sweep over adjacent seeds would be re-running one \
             schedule",
            seed + 1
        );
    }
}

/// The tunable density is what makes this a middle setting rather than a second
/// zeal. Measured, because a threshold computed with the wrong exponent still
/// produces a perfectly deterministic schedule and would pass every test above.
#[test]
fn realised_density_tracks_the_requested_rate() {
    const N: u64 = 200_000;
    for rate in [0.01f64, 0.05, 0.2, 0.5] {
        let threshold = rate_threshold(rate);
        let hits = (0..N)
            .filter(|&counter| schedule_hit(99, counter, threshold))
            .count();
        let realised = hits as f64 / N as f64;
        // ±15% relative. Sampling noise at N=200k and rate 0.01 is ~2% relative
        // (σ = sqrt(r(1-r)/N)/r ≈ 0.7%), so this is loose enough never to flake
        // and tight enough to catch an off-by-a-factor threshold.
        assert!(
            (realised - rate).abs() < rate * 0.15,
            "requested rate {rate}, realised {realised} over {N} safepoints"
        );
    }
}

// ---------------------------------------------------------------------------
// Behaviour at a real safepoint. Both arms, always: the OFF arm is what proves
// the safepoint was genuinely idle — without it a passing ON arm could just be
// ordinary heap pressure.
// ---------------------------------------------------------------------------

#[test]
fn the_schedule_collects_at_a_safepoint_with_no_pressure_due() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_scan_fallback_counters();

    // OFF: an idle safepoint must collect nothing, and must not advance the
    // schedule counter — an inert mode that still ticks would silently desync
    // the reproducer from the run that found the bug.
    let safepoints_before = gc_schedule_safepoints();
    {
        let _schedule = ScheduleGuard::off();
        gc_safepoint_moving_minor();
    }
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        0,
        "test premise: with no trigger due and the mode off, the safepoint must \
         be idle"
    );
    assert_eq!(
        gc_schedule_safepoints(),
        safepoints_before,
        "with no seed set the counter must not advance at all"
    );

    // ON at rate 1: the same idle safepoint must now run a minor.
    let forced_before = gc_schedule_forced_collections();
    {
        let _schedule = ScheduleGuard::set(7, rate_threshold(1.0));
        reset_thread_counter_for_test();
        gc_safepoint_moving_minor();
    }
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        1,
        "a selected safepoint must force a minor"
    );
    assert!(
        gc_schedule_forced_collections() > forced_before,
        "the forced collection must be COUNTED — a run reporting 0 scheduled \
         collections exercised nothing, and a clean verdict from it is vacuous"
    );
    assert_eq!(
        gc_schedule_safepoints(),
        safepoints_before + 1,
        "exactly one handled safepoint must have been ticked"
    );
}

/// The complement: mode ON but the schedule declining. This is the arm that
/// distinguishes "seeded schedule" from "collect at every safepoint with extra
/// steps" — if a declined safepoint collected anyway, every seed would behave
/// identically and the rate knob would be decoration.
#[test]
fn a_declined_safepoint_does_not_collect_but_still_ticks() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_scan_fallback_counters();

    let safepoints_before = gc_schedule_safepoints();
    let forced_before = gc_schedule_forced_collections();
    {
        let _schedule = ScheduleGuard::set(7, rate_threshold(0.0));
        reset_thread_counter_for_test();
        gc_safepoint_moving_minor();
    }
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        0,
        "rate 0 must decline: no collection at an idle safepoint"
    );
    assert_eq!(
        gc_schedule_forced_collections(),
        forced_before,
        "a declined safepoint must not be counted as a forced collection"
    );
    assert_eq!(
        gc_schedule_safepoints(),
        safepoints_before + 1,
        "a declined safepoint is still a HANDLED safepoint and must consume its \
         schedule slot — otherwise the ordinal a bug is found at depends on how \
         many collections happened, and the seed stops being a reproducer"
    );
}

/// An entry guard is not a declined safepoint: the collector could not have run,
/// so consuming a schedule slot there would make the ordinal sequence depend on
/// allocation state rather than on the program's safepoint sequence.
#[test]
fn a_blocked_safepoint_consumes_no_schedule_slot() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_scan_fallback_counters();

    let safepoints_before = gc_schedule_safepoints();
    {
        let _schedule = ScheduleGuard::set(7, rate_threshold(1.0));
        reset_thread_counter_for_test();
        super::super::roots::enter_gc_root_lock();
        gc_safepoint_moving_minor();
        super::super::roots::exit_gc_root_lock();
    }
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        0,
        "a non-zero root-lock depth must still block the collection — the mode \
         does not bypass the entry guards"
    );
    assert_eq!(
        gc_schedule_safepoints(),
        safepoints_before,
        "a blocked safepoint must not tick the schedule"
    );
}

/// A scheduled minor that leaves survivors in place would move nothing, so it
/// could not surface a stale-pointer bug at all — the mode would be a knob whose
/// name promises relocation stress and whose effect is sweep pressure.
///
/// (This test originally also asserted the schedule loses to an explicit
/// `PERRY_GEN_GC_EVACUATE=0`. That knob was DELETED in #7611 — its one unique
/// effect was vetoing forced evacuation, i.e. silently disarming exactly the
/// instruments this mode joins — so the veto branch is gone with it.)
#[test]
fn the_schedule_implies_forced_evacuation() {
    let _off = ScheduleGuard::off();
    let off = gc_force_evacuate_enabled();
    let _on = ScheduleGuard::set(7, rate_threshold(1.0));
    assert!(
        gc_force_evacuate_enabled(),
        "a resolved seed must force evacuation (force_off={off})"
    );
}

/// Inertness, stated as the collector sees it: with no seed the two knobs change
/// nothing about evacuation policy, which is the only collector decision this
/// mode reaches into outside the safepoint arm asserted above.
#[test]
fn unset_is_inert_for_evacuation_policy() {
    let _off = ScheduleGuard::off();
    let baseline = matches!(
        std::env::var("PERRY_GC_FORCE_EVACUATE").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    ) || super::super::gc_zeal_enabled();
    assert_eq!(
        gc_force_evacuate_enabled(),
        baseline,
        "with no seed set, forced evacuation must be decided exactly as it was \
         before this mode existed"
    );
}
