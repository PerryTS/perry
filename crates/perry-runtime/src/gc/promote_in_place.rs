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

/// Young-survival ratio, in permille, at or above which a promoting cycle may
/// skip the TRACE as well as the copy (#7888).
///
/// Deliberately stricter than [`PROMOTE_SURVIVAL_THRESHOLD_PERMILLE`]: a
/// promoting cycle that still traces knows exactly which objects are garbage
/// and pays only footprint for a misprediction, while an untraced one assumes
/// the whole young generation is live. Only the regime the measurement calls
/// *fully* live earns that; the bimodal population's live mode sits at
/// **992–1000** and its garbage mode at 0–4, three orders of magnitude apart.
///
/// # Why this is 990 and not the 999 it was introduced at (#7888)
///
/// 999 was read off `retain`/`retain_wide`/`deeplist`, whose every cycle
/// measured exactly 999 or 1000 — and **that reading was partly an artifact of
/// the flat 16 KB born-tenured threshold.** `all.push(rec)` grows its backing
/// store by doubling, and every intermediate store over 16 KB was born in
/// old-gen, so the garbage those growths abandon was never in the young
/// generation to be counted. With pointer-bearing objects nursery-resident up
/// to 128 KB the abandoned stores land where they belong, and `retain`'s FIRST
/// cycle now measures **992** — deterministically, on all four `retain*`
/// variants and on `phase_flip`. Every LATER cycle measures 1000 rather than
/// 999, i.e. the estimator is strictly more confident once the startup garbage
/// has actually been collected.
///
/// At 999 that one permille costs exactly one traced cycle per program, worth
/// `retain1` +15.7% and `retain_wide1` +11.3% on the quiet mini. The threshold
/// was over-fitted to a measurement that has since become more accurate.
///
/// The exposure this widens is small and already bounded twice.
/// [`note_untraced_promotion`] charges `promoted × (1000 − permille) / 1000`
/// against [`PROMOTED_DEAD_BUDGET_BYTES`], so an untraced run capped at
/// [`UNTRACED_PROMOTION_FLOOR_BYTES`] (128 MB) carries at most **1.28 MB** of
/// assumed-live-but-dead bytes here against 0.128 MB at 999 — both far under
/// the 32 MB cap, which means the binding bound is the untraced-bytes budget in
/// either case, and that is unchanged.
///
/// #7902: that arithmetic is what the code does only because `permille` is now
/// CLAMPED to this threshold (see [`implied_dead_bytes`]). Taken from the last
/// measurement verbatim, a stationary 1000‰ reading charged zero and the
/// paragraph above described a bound nothing enforced. The untraced-bytes
/// budget remains the binding bound, and it is now itself capped — see
/// [`untraced_promotion_budget_bytes`].
pub(super) const UNTRACED_PROMOTION_SURVIVAL_PERMILLE: u64 = 990;

/// Floor for the untraced-promotion budget on an UNCONSTRAINED heap — see
/// [`untraced_promotion_budget_bytes`].
///
/// Two young-cap ceilings (2 × 64 MB). The traced path's own misprediction
/// bound is "at most ONE nursery of retained garbage before the policy turns
/// itself off" (see the module docs); this is that bound doubled, which is
/// what buys a fully-live workload a whole run of free cycles instead of
/// re-measuring every other one.
///
/// #7902: this is a DEFAULT, not a minimum. A process with a configured heap
/// budget gets a quarter of it instead — 128 MB of assumed-live retention is
/// larger than some intended device heaps outright, and the floor was being
/// applied unconditionally.
pub(super) const UNTRACED_PROMOTION_FLOOR_BYTES: usize = 128 * 1024 * 1024;

/// Hard ceiling on the untraced-promotion budget, i.e. on the worst-case
/// retained-garbage exposure of the untraced path (#7902).
///
/// The budget's relative half (`old-gen at the last measurement`) exists so a
/// program with a large genuinely-live old heap can keep running free cycles.
/// But that half was uncapped, so on a multi-GB old generation an abrupt
/// live→dead phase change could park an old-heap-sized cohort of assumed-live
/// garbage before anything re-measured. The exposure is now bounded by
/// `min(max(floor, old-gen-at-last-measurement), this)` — an explicit,
/// statable worst case rather than "whatever the heap happens to be".
///
/// 4 × the unconstrained floor: it keeps the proportional behaviour across the
/// range where it is cheap (old-gen up to 512 MB) and stops it exactly where a
/// misprediction would cost more than one forced traced cycle is worth.
pub(super) const UNTRACED_PROMOTION_CEILING_BYTES: usize = 512 * 1024 * 1024;

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
    /// Bytes promoted by cycles that did NOT trace, since the last cycle that
    /// did (or the last full collection). This is the quantity the untraced
    /// budget bounds: every one of these bytes is *assumed* live.
    static UNTRACED_PROMOTED_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Live-subject counters for the untraced path (#7888). A benchmark that
    /// never entered it proves nothing about it.
    static UNTRACED_PROMOTION_CYCLES: Cell<u64> = const { Cell::new(0) };
    static UNTRACED_PROMOTED_OBJECTS: Cell<u64> = const { Cell::new(0) };
    /// Old-gen occupancy when the last real measurement landed — the base the
    /// untraced budget's relative half is taken against.
    static OLD_GEN_AT_LAST_MEASUREMENT: Cell<usize> = const { Cell::new(0) };
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

/// The untraced-promotion budget, and therefore **the worst-case retained
/// garbage of the untraced path**: every byte it admits is *assumed* live, so
/// after an abrupt live→dead phase change every one of them can be garbage
/// until the forced measuring cycle and a following full collection.
///
/// `min(max(floor, old-gen as it stood at the last MEASUREMENT), ceiling)`:
///
/// * the relative half lets a program with a large genuinely-live old heap keep
///   running free cycles — its exposure is proportional to memory it already
///   holds;
/// * the floor keeps the rule from re-measuring constantly while old-gen is
///   still small, and (#7902) scales to a configured heap budget rather than
///   parking a flat 128 MB on a device heap smaller than that;
/// * the ceiling (#7902) makes the worst case statable. Without it the bound
///   grew with old-gen without limit, so a phase-changing server could park a
///   whole old-heap's worth of dead-but-accounted-live memory.
///
/// It has to be the size at the last measurement, not the size now: the
/// untraced bytes ARE old-gen bytes, so comparing against the current figure
/// compares a quantity with itself and the relative half can never fire.
pub(super) fn untraced_promotion_budget_bytes() -> usize {
    untraced_promotion_budget_with(
        super::gc_heap_budget_bytes(),
        OLD_GEN_AT_LAST_MEASUREMENT.with(Cell::get),
    )
}

/// Pure form of [`untraced_promotion_budget_bytes`], so both the constrained
/// and unconstrained arms are asserted without poking the process environment.
pub(super) fn untraced_promotion_budget_with(
    heap_budget: Option<usize>,
    old_gen_at_last_measurement: usize,
) -> usize {
    // A quarter of a constrained budget; the historical 128 MB otherwise. One
    // young cap (4 MB) is the floor's own floor — below that the policy would
    // re-measure on essentially every cycle and #7888 would not exist.
    let floor = super::budget_scaled_with(
        heap_budget,
        UNTRACED_PROMOTION_FLOOR_BYTES,
        1,
        4,
        4 * 1024 * 1024,
    );
    // The ceiling scales the same way, so a constrained process never admits
    // more retained garbage than its own budget allows.
    let ceiling =
        super::budget_scaled_with(heap_budget, UNTRACED_PROMOTION_CEILING_BYTES, 1, 2, floor)
            .max(floor);
    floor.max(old_gen_at_last_measurement).min(ceiling)
}

/// Should this promoting cycle also skip the trace?
///
/// Caller has already established that the cycle promotes the WHOLE young
/// generation in place ([`should_promote_young_in_place`] plus a non-empty
/// retag), which is what makes the trace's remaining products enumerable —
/// see `gc::copying`'s `untraced` binding for the enumeration and the
/// gates that go with it. This half answers only the policy question.
pub(super) fn should_promote_young_untraced() -> bool {
    // In test builds this path is opt-in per thread, for the same reason
    // `should_promote_young_in_place` is: the unit suite drives
    // `gc_collect_minor` directly and asserts what a TRACING cycle does with
    // its marks (identity, liveness, weak tombstoning). Silently rerouting
    // those to a path that produces no marks would leave the traced path
    // untested wherever this one is admissible. The tests that are ABOUT this
    // path opt in and assert the live-subject counters.
    #[cfg(test)]
    if !TEST_UNTRACED_OPT_IN.with(Cell::get) {
        return false;
    }
    if UNTRACED_PROMOTED_BYTES.with(Cell::get) >= untraced_promotion_budget_bytes() {
        return false;
    }
    LAST_YOUNG_SURVIVAL_PERMILLE
        .with(Cell::get)
        .is_some_and(|permille| permille >= UNTRACED_PROMOTION_SURVIVAL_PERMILLE)
}

/// Charge an untraced promotion against the budgets and count the subject.
///
/// Two charges, against two different bounds:
///
/// * `UNTRACED_PROMOTED_BYTES` bounds how STALE the estimator may get before a
///   cycle has to measure again.
/// * `PROMOTED_DEAD_BYTES` — the footprint cap the traced path already feeds
///   from an exact measurement — is charged the dead bytes the last measurement
///   IMPLIES. Without this an untraced run would disarm that cap entirely
///   (charging zero is not "no garbage", it is "no answer"), and a workload
///   sitting just inside the survival threshold could park garbage indefinitely
///   with each individual cycle looking fine. With it, the two paths share one
///   footprint bound; they differ only in whether the dead figure is measured
///   or extrapolated, and the untraced budget is what bounds the extrapolation.
pub(super) fn note_untraced_promotion(promoted_bytes: usize, promoted_objects: usize) {
    PROMOTED_DEAD_BYTES.with(|c| {
        c.set(c.get().saturating_add(implied_dead_bytes(
            promoted_bytes,
            LAST_YOUNG_SURVIVAL_PERMILLE.with(Cell::get),
        )))
    });
    UNTRACED_PROMOTED_BYTES.with(|c| c.set(c.get().saturating_add(promoted_bytes)));
    UNTRACED_PROMOTION_CYCLES.with(|c| c.set(c.get().saturating_add(1)));
    UNTRACED_PROMOTED_OBJECTS.with(|c| c.set(c.get().saturating_add(promoted_objects as u64)));
}

/// Dead bytes an untraced promotion of `promoted_bytes` implies (#7902).
///
/// The extrapolation is capped at [`UNTRACED_PROMOTION_SURVIVAL_PERMILLE`], not
/// taken from the last measurement verbatim. A stationary 1000‰ measurement
/// says nothing about the cycle being promoted — it is by construction the
/// PREVIOUS cycle's answer — so charging `1000 − 1000 = 0` disarmed
/// [`PROMOTED_DEAD_BUDGET_BYTES`] entirely on exactly the workloads that enter
/// this path. The most optimistic honest assumption is the worst ratio the
/// decision itself admits, which is also the figure
/// [`UNTRACED_PROMOTION_SURVIVAL_PERMILLE`]'s own doc computes its 1.28 MB
/// bound from: before this the doc and the code disagreed.
fn implied_dead_bytes(promoted_bytes: usize, last_survival_permille: Option<u64>) -> usize {
    let assumed_survival = last_survival_permille
        .unwrap_or(0)
        .min(UNTRACED_PROMOTION_SURVIVAL_PERMILLE);
    let dead_permille = 1000u64.saturating_sub(assumed_survival);
    (promoted_bytes as u64)
        .saturating_mul(dead_permille)
        .checked_div(1000)
        .unwrap_or(0) as usize
}

/// Cycles that promoted without tracing, and the objects they promoted.
pub fn untraced_promotion_cycles() -> u64 {
    UNTRACED_PROMOTION_CYCLES.with(Cell::get)
}

pub fn untraced_promoted_objects() -> u64 {
    UNTRACED_PROMOTED_OBJECTS.with(Cell::get)
}

pub(crate) fn untraced_promoted_bytes_since_measurement() -> usize {
    UNTRACED_PROMOTED_BYTES.with(Cell::get)
}

/// Record the young-survival ratio a copying minor just measured. Called on
/// EVERY copying minor that TRACES, promoting or evacuating — that is what
/// keeps the predictor from going stale under repeated promotion. An untraced
/// promotion deliberately does NOT call this: it measured nothing, and
/// recording its assumption as a measurement would turn the feedback loop into
/// a mirror.
pub(super) fn note_young_survival(young_bytes: usize, live_bytes: usize) {
    // A real measurement landed, so the untraced run it ends is settled and the
    // next run's budget is taken against the heap this one leaves behind.
    let untraced_run_bytes = UNTRACED_PROMOTED_BYTES.replace(0);
    OLD_GEN_AT_LAST_MEASUREMENT.with(|c| c.set(crate::arena::old_gen_in_use_bytes()));
    if young_bytes == 0 {
        return;
    }
    let permille = (live_bytes as u64)
        .saturating_mul(1000)
        .checked_div(young_bytes as u64)
        .unwrap_or(0)
        .min(1000);
    LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(Some(permille)));
    // #7902: this measurement is the FIRST evidence about the cohort the
    // preceding untraced cycles promoted on faith. If it contradicts the
    // predictor that admitted them, that cohort is probably garbage sitting in
    // old-gen — and nothing else will look at it, because the traced cycle
    // measures only its own young generation and old-reclaim pacing was told
    // the promoted bytes were live. Ask for the old-gen reclaim now instead of
    // waiting for growth pressure to notice a heap that is not growing.
    if untraced_run_bytes > 0 && permille < UNTRACED_PROMOTION_SURVIVAL_PERMILLE {
        super::request_old_reclaim_for_untraced_promotions(untraced_run_bytes);
    }
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
/// over — and with it the untraced budget, whose whole subject (assumed-live
/// promoted bytes that were never checked) has just been checked by a full
/// trace and either kept or freed.
pub(super) fn note_full_collection_reclaimed_old_gen() {
    PROMOTED_DEAD_BYTES.with(|c| c.set(0));
    UNTRACED_PROMOTED_BYTES.with(|c| c.set(0));
    OLD_GEN_AT_LAST_MEASUREMENT.with(|c| c.set(crate::arena::old_gen_in_use_bytes()));
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
    static TEST_UNTRACED_OPT_IN: Cell<bool> = const { Cell::new(false) };
}

/// Opt this thread's copying minors into the in-place promotion path, and
/// restore the previous state (plus the whole policy state) on drop.
#[cfg(test)]
pub(super) struct InPlacePromotionTestGuard {
    previous_opt_in: bool,
    previous_survival: Option<u64>,
    previous_dead: usize,
    previous_untraced: usize,
    previous_untraced_opt_in: bool,
    previous_old_gen_base: usize,
}

#[cfg(test)]
impl InPlacePromotionTestGuard {
    pub(super) fn enabled(survival_permille: u64) -> Self {
        let guard = Self {
            previous_opt_in: TEST_OPT_IN.replace(true),
            previous_survival: LAST_YOUNG_SURVIVAL_PERMILLE.get(),
            previous_dead: PROMOTED_DEAD_BYTES.get(),
            previous_untraced: UNTRACED_PROMOTED_BYTES.get(),
            previous_untraced_opt_in: TEST_UNTRACED_OPT_IN.get(),
            previous_old_gen_base: OLD_GEN_AT_LAST_MEASUREMENT.get(),
        };
        OLD_GEN_AT_LAST_MEASUREMENT.with(|c| c.set(0));
        LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(Some(survival_permille)));
        PROMOTED_DEAD_BYTES.with(|c| c.set(0));
        guard
    }

    /// Opt into the UNTRACED promotion path as well (#7888): fresh budget, and
    /// a survival ratio in the fully-live regime.
    pub(super) fn untraced() -> Self {
        let guard = Self::enabled(1000);
        UNTRACED_PROMOTED_BYTES.with(|c| c.set(0));
        TEST_UNTRACED_OPT_IN.with(|c| c.set(true));
        guard
    }
}

#[cfg(test)]
impl Drop for InPlacePromotionTestGuard {
    fn drop(&mut self) {
        TEST_OPT_IN.with(|c| c.set(self.previous_opt_in));
        LAST_YOUNG_SURVIVAL_PERMILLE.with(|c| c.set(self.previous_survival));
        PROMOTED_DEAD_BYTES.with(|c| c.set(self.previous_dead));
        UNTRACED_PROMOTED_BYTES.with(|c| c.set(self.previous_untraced));
        TEST_UNTRACED_OPT_IN.with(|c| c.set(self.previous_untraced_opt_in));
        OLD_GEN_AT_LAST_MEASUREMENT.with(|c| c.set(self.previous_old_gen_base));
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

#[cfg(test)]
pub(super) fn seed_untraced_promoted_bytes_for_tests(bytes: usize) {
    UNTRACED_PROMOTED_BYTES.with(|c| c.set(bytes));
}
