use super::{
    root_home_size_candidate, root_relocation_estimate, spill_live_root_count,
    DEFAULT_ROOT_HOME_RELOCATIONS, DEFAULT_ROOT_SPILL_RELOCATIONS,
};

/// Exactly what `maybe_spill_roots_to_shadow_frame` computes, so these
/// endpoint tests track the production formula instead of a stale copy of
/// it (#8633 changed the composition; before this helper the tests still
/// asserted on the pre-#8633 `slot_count x sites`).
fn production_estimate(slot_count: usize, sites: usize) -> usize {
    root_relocation_estimate(spill_live_root_count(slot_count, sites), sites)
}

/// #8620's catastrophic ceiling and #11926's measured code-size crossover
/// are independent defaults. Change either only with a fresh size/runtime
/// A/B and a compile-time fan-out measurement respectively.
#[test]
fn defaults_cover_code_size_and_catastrophic_fanout() {
    assert_eq!(DEFAULT_ROOT_HOME_RELOCATIONS, 2_048);
    assert_eq!(DEFAULT_ROOT_SPILL_RELOCATIONS, 32_000_000);
}

/// #11926's q25 witness is the smallest measured point: 77 named/temp
/// slots plus 25 call-result temporaries gives 102 live roots. It must use
/// stable frame homes; leaving it on RS4GC is the quadratic-code bug.
#[test]
fn quadratic_code_witness_uses_stable_frame_homes() {
    let est = production_estimate(77, 25);
    assert_eq!(est, 2_550);
    assert!(
        root_home_size_candidate(77, 25, est),
        "q25 estimate {est} must select stable homes",
    );
}

/// Call-heavy functions are outside the low crossover: adding one
/// hypothetical root per call is intentionally conservative for the 32M
/// compile-time backstop, but is not code-size evidence. These are the
/// real Zod shapes that exposed the distinction while validating #11926.
#[test]
fn call_heavy_functions_stay_on_native_statepoints_below_the_hard_cap() {
    assert!(!root_home_size_candidate(105, 108, 23_004));
    assert!(!root_home_size_candidate(22, 114, 15_504));
    assert!(!root_home_size_candidate(48, 54, 5_508));
    assert!(!root_home_size_candidate(79, 21, 2_100));
}

/// The genuinely-catastrophic case (Claude Code `cli.js` `@main`,
/// ~795 slots × ~106k safepoints ≈ 8.4e7, never finishes at `-Os`) must
/// still spill under the new default.
#[test]
fn catastrophic_fan_out_still_spills() {
    let est = production_estimate(795, 106_000);
    assert!(
        est > DEFAULT_ROOT_SPILL_RELOCATIONS,
        "catastrophic estimate {est} must exceed the default (should spill)",
    );
}
