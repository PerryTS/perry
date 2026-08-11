//! Policy for whole-block in-place promotion (#7742).
//!
//! The mechanism lives in `arena/promote.rs`; this is the decision layer, and
//! the decision is made from a **measurement**, not a guess.
//!
//! # The measurement
//!
//! Every copying minor already computes `live_from_bytes` — the bytes of the
//! young generation that survived — and the from-space size it started with.
//! Their ratio is the young-survival ratio. Measured on the GC benchmark set
//! (`gc-handoff/bench`, best-of-5 on the pinned M1 mini):
//!
//! | workload | copying minors | young-survival ratio |
//! |---|--:|--:|
//! | `retain`, `retain1`, `retain_wide` | 5–7 | **0.999 – 1.000** |
//! | `deeplist` | 3 | **1.000** |
//! | `churn`, `churn_alloc`, `push_cls` | 105 | 0.000 – 0.004 |
//! | `push_num`, `cycles` | 16–18 | 0.000 |
//! | `tree`, `tree_wide`, `churn_read` | 0 | — (no copying minor runs) |
//!
//! The population is bimodal with a gap of three orders of magnitude, so the
//! threshold is not a tuning knob in any interesting sense — anything in
//! `[0.01, 0.99]` classifies this set identically. It is set at
//! [`PROMOTE_SURVIVAL_THRESHOLD_PERMILLE`] = 95% purely to bound the footprint
//! cost, which is what the constant's value is actually justified against.
//!
//! # Why a per-cycle decision is safe
//!
//! A block's liveness is not knowable before the trace, and Eden blocks are
//! recycled at offset 0 so per-block history means nothing. The decision is
//! therefore per cycle, taken from the PREVIOUS cycle's ratio. The thing that
//! makes that sound rather than optimistic is that a promoting cycle **still
//! traces**, so it measures the ratio too: the feedback never goes stale, and a
//! workload that flips from live to garbage pays at most ONE nursery of
//! retained garbage before the policy turns itself off.
//!
//! Two further bounds:
//!
//! * [`PROMOTED_DEAD_BUDGET_BYTES`] caps the running total of dead bytes
//!   promoted since the last full collection. Reaching it disables in-place
//!   promotion until a full runs and actually reclaims them, so a workload
//!   that sits just above the threshold forever cannot bleed footprint
//!   indefinitely.
//! * `PERRY_GC_FORCE_EVACUATE` — and every mode that implies it, a resolved
//!   `PERRY_GC_SCHEDULE_SEED` included — turns it off outright. Those
//!   knobs exist to make objects MOVE; a promoting cycle moves nothing, and an
//!   instrument that silently stops exercising its subject is exactly the
//!   failure mode CLAUDE.md's "a gate must assert its subject was live" rule
//!   is about.

use super::*;

/// Young-survival ratio, in permille, at or above which the next copying minor
/// promotes the whole young generation in place instead of evacuating it.
///
/// Chosen for footprint, not for classification (see the module docs): at 95%
/// a mispredicted cycle retains at most 5% of the young generation as old-gen
/// garbage, which against the 64 MB young-cap ceiling is ≤ 3.2 MB before the
/// re-measured ratio turns the policy off.
pub(super) const PROMOTE_SURVIVAL_THRESHOLD_PERMILLE: u64 = 950;

/// Running cap on dead bytes promoted in place since the last full collection.
///
/// The per-cycle bound above is self-correcting; this bounds the pathological
/// steady state it does NOT cover — a workload whose ratio sits just above the
/// threshold every single cycle, where each cycle is individually "fine" and
/// the total still grows without limit. 32 MB is half the 64 MB young-cap
/// ceiling: it lets a fully-live workload promote unboundedly (it produces zero
/// dead bytes) while capping the marginal case at a fraction of one nursery
/// before a full collection has to justify continuing.
pub(super) const PROMOTED_DEAD_BUDGET_BYTES: usize = 32 * 1024 * 1024;

thread_local! {
    /// Young-survival ratio of the most recent copying minor, in permille.
    /// `None` until one has run — the first copying minor is always a real
    /// evacuation, because nothing has been measured yet.
    static LAST_YOUNG_SURVIVAL_PERMILLE: Cell<Option<u64>> = const { Cell::new(None) };
    /// Dead bytes promoted in place since the last full collection.
    static PROMOTED_DEAD_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Cycle counters, for the trace and for tests that need to prove the
    /// subject actually ran.
    static IN_PLACE_PROMOTION_CYCLES: Cell<u64> = const { Cell::new(0) };
    static IN_PLACE_PROMOTED_OBJECTS: Cell<u64> = const { Cell::new(0) };
    /// Young capacity the last promotion handed to old-gen, owed back to the
    /// next arena-bytes trigger as headroom. See
    /// `arena::InPlacePromotion::reserved_bytes`.
    static YOUNG_CAPACITY_CREDIT: Cell<usize> = const { Cell::new(0) };
}

/// Record that `bytes` of young capacity became old-gen, so the next
/// arena-bytes trigger restores the allocation runway a copying minor would
/// have kept by recycling those same blocks.
pub(super) fn note_promoted_young_capacity(bytes: usize) {
    YOUNG_CAPACITY_CREDIT.with(|c| c.set(c.get().saturating_add(bytes)));
}

/// Consume the credit. Read exactly once, by the post-collection trigger
/// rebaseline — leaving it set would compound across cycles into unbounded
/// headroom, which is the failure `gc_bump_arena_trigger_target`'s own comment
/// records (a trigger that ratcheted hundreds of MB above the live set and
/// never fired again).
pub(super) fn take_promoted_young_capacity_credit() -> usize {
    YOUNG_CAPACITY_CREDIT.with(|c| c.replace(0))
}

/// `PERRY_GC_PROMOTE_IN_PLACE=0|off|false` reverts to object-by-object
/// evacuation on every cycle. Bisection escape hatch for a change that alters
/// where every surviving object lives; its OFF state is asserted by
/// `gc::tests::promote_in_place::promote_in_place_knob_parses_both_states`.
pub(super) fn promote_in_place_enabled() -> bool {
    parse_promote_in_place(std::env::var("PERRY_GC_PROMOTE_IN_PLACE").ok().as_deref())
}

/// Pure knob parse, so both states are asserted without touching the process
/// environment (see `gc/tests/fromspace_protect.rs` for why the live readers
/// are never poked directly from a test).
///
/// Default is ON. Only the three explicit off-spellings turn it off — a typo
/// must not silently change which collector a bisect is measuring.
pub(super) fn parse_promote_in_place(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false"
        ),
        None => true,
    }
}

/// Should this copying minor promote the young generation whole, in place?
pub(super) fn should_promote_young_in_place() -> bool {
    // In test builds the path is opt-in per thread. The unit suite drives
    // `gc_collect_minor` directly and asserts object IDENTITY across it
    // ("the survivor is at a new address"), so a policy keyed on the whole
    // thread's Eden liveness would make those assertions depend on which other
    // test allocated first. `in_place_promotion_opt_in_for_tests` turns it on
    // for the tests that are ABOUT this path — and those assert the live-
    // subject counters, so the production behaviour is genuinely exercised, not
    // merely compiled.
    #[cfg(test)]
    if !TEST_OPT_IN.with(Cell::get) {
        return false;
    }
    if !promote_in_place_enabled() {
        return false;
    }
    // Forced evacuation exists to make objects move — and it is what every
    // stress mode implies (a resolved schedule seed included), so this one
    // predicate covers them all. Leave them a copier to drive.
    if gc_force_evacuate_enabled() {
        return false;
    }
    if PROMOTED_DEAD_BYTES.with(Cell::get) >= PROMOTED_DEAD_BUDGET_BYTES {
        return false;
    }
    LAST_YOUNG_SURVIVAL_PERMILLE
        .with(Cell::get)
        .is_some_and(|permille| permille >= PROMOTE_SURVIVAL_THRESHOLD_PERMILLE)
}

/// Record the young-survival ratio a copying minor just measured. Called on
/// EVERY copying minor, promoting or evacuating — that is what keeps the
/// predictor from going stale under repeated promotion.
pub(super) fn note_young_survival(young_bytes: usize, live_bytes: usize) {
    if young_bytes == 0 {
        return;
    }
    let permille = (live_bytes as u64)
        .saturating_mul(1000)
        .checked_div(young_bytes as u64)
        .unwrap_or(0)
        .min(1000);
    LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(Some(permille)));
}

/// Charge the dead bytes an in-place promotion just moved into old-gen against
/// the running budget.
pub(super) fn note_in_place_promotion(
    young_bytes: usize,
    live_bytes: usize,
    promoted_objects: usize,
) {
    let dead = young_bytes.saturating_sub(live_bytes);
    PROMOTED_DEAD_BYTES.with(|c| c.set(c.get().saturating_add(dead)));
    IN_PLACE_PROMOTION_CYCLES.with(|c| c.set(c.get().saturating_add(1)));
    IN_PLACE_PROMOTED_OBJECTS.with(|c| c.set(c.get().saturating_add(promoted_objects as u64)));
}

/// A full collection reclaimed old-gen, so the retained-garbage budget starts
/// over.
pub(super) fn note_full_collection_reclaimed_old_gen() {
    PROMOTED_DEAD_BYTES.with(|c| c.set(0));
}

/// How many cycles promoted in place, and how many objects they promoted.
/// The "did the subject actually run?" counters — a green benchmark that never
/// entered the path proves nothing.
pub fn in_place_promotion_cycles() -> u64 {
    IN_PLACE_PROMOTION_CYCLES.with(Cell::get)
}

pub fn in_place_promoted_objects() -> u64 {
    IN_PLACE_PROMOTED_OBJECTS.with(Cell::get)
}

/// Last measured young-survival ratio in permille, or `None` before the first
/// copying minor. Trace/test observability.
pub(crate) fn last_young_survival_permille() -> Option<u64> {
    LAST_YOUNG_SURVIVAL_PERMILLE.with(Cell::get)
}

pub(crate) fn promoted_dead_bytes_since_full() -> usize {
    PROMOTED_DEAD_BYTES.with(Cell::get)
}

#[cfg(test)]
thread_local! {
    static TEST_OPT_IN: Cell<bool> = const { Cell::new(false) };
}

/// Opt this thread's copying minors into the in-place promotion path, and
/// restore the previous state (plus the whole policy state) on drop.
#[cfg(test)]
pub(super) struct InPlacePromotionTestGuard {
    previous_opt_in: bool,
    previous_survival: Option<u64>,
    previous_dead: usize,
}

#[cfg(test)]
impl InPlacePromotionTestGuard {
    pub(super) fn enabled(survival_permille: u64) -> Self {
        let guard = Self {
            previous_opt_in: TEST_OPT_IN.replace(true),
            previous_survival: LAST_YOUNG_SURVIVAL_PERMILLE.get(),
            previous_dead: PROMOTED_DEAD_BYTES.get(),
        };
        LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(Some(survival_permille)));
        PROMOTED_DEAD_BYTES.with(|c| c.set(0));
        guard
    }
}

#[cfg(test)]
impl Drop for InPlacePromotionTestGuard {
    fn drop(&mut self) {
        TEST_OPT_IN.with(|c| c.set(self.previous_opt_in));
        LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(self.previous_survival));
        PROMOTED_DEAD_BYTES.with(|c| c.set(self.previous_dead));
    }
}

/// Put the thread back in the state it boots in: no copying minor has run, so
/// nothing has been measured. Distinct from seeding 0 permille — that is a
/// MEASUREMENT of "almost nothing survived", and only this exercises the
/// `None` arm of the decision.
#[cfg(test)]
pub(super) fn clear_young_survival_for_tests() {
    LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(None));
}

#[cfg(test)]
pub(super) fn seed_young_survival_for_tests(permille: u64) {
    LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(Some(permille)));
}

#[cfg(test)]
pub(super) fn seed_promoted_dead_bytes_for_tests(bytes: usize) {
    PROMOTED_DEAD_BYTES.with(|c| c.set(bytes));
}
