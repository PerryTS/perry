//! Adaptive tenuring threshold for the copying (scavenging) minor collector.
//!
//! The copying minor ages nursery survivors through the survivor semispaces
//! and promotes them to old-gen after a fixed number of copies
//! (`GC_COPY_PROMOTION_SURVIVALS`, 4). That fixed age is correct for the
//! generational-hypothesis workloads the scavenge was tuned on (tiny live
//! sets, near-empty survivor spaces) and pathological for large live sets:
//! a program whose survivors essentially never die in the survivor space
//! pays `age × influx` bytes of pure re-copying — measured 4.39 GB / 61 M
//! object-copies on a binary-tree benchmark whose entire live set was
//! ~35 MB, because the same ~3.15 MB survivor cohort ping-ponged between
//! the semispaces on every one of 1427 collections.
//!
//! This module is the survivor-occupancy feedback loop (HotSpot's adaptive
//! `TenuringThreshold`, restated for elastic semispaces). Per copying cycle
//! the collector reports the **Eden survivor influx** — bytes moved out of
//! Eden that were live (copied to a survivor space or promoted). At a
//! threshold of S survivals, steady-state survivor occupancy is
//! `(S−1) × influx` (S−1 resident age cohorts), so the next cycle's
//! threshold is the largest S whose projected occupancy fits the desired
//! survivor size:
//!
//! ```text
//! S = min(4, 1 + desired / influx)
//! ```
//!
//! `desired` is 1/16 of the effective scavenge nursery cap, never less than
//! 1/16 of the base cap (1 MB at the default 16 MB base; see
//! [`desired_survivor_bytes`]) — the same effective ratio HotSpot's defaults produce
//! (SurvivorRatio=8, TargetSurvivorRatio=50% ⇒ Eden/16).
//!
//! The influx signal is deliberately *threshold-invariant*: live Eden bytes
//! get moved somewhere at any S, so the feedback loop has a fixed point
//! instead of the oscillation that a post-hoc survivor-occupancy signal
//! produces (at S=1 the survivor space is empty, which would read as "no
//! pressure" and snap the threshold straight back up).
//!
//! Asymmetric response: the threshold *drops* to the computed target
//! immediately (every extra cycle at a too-high threshold re-copies the
//! whole resident cohort), but *rises* one step at a time and only after
//! the target has been above the current value for two consecutive cycles
//! (a single quiet cycle — e.g. a malloc-count trigger firing before Eden
//! filled — must not flush the aging pipeline into a copy burst).
//!
//! The occupancy rule alone is not sufficient: it optimises survivor
//! *space*, not copy *work*. A workload whose influx sits just under
//! `desired` (tree.ts measures 1,048,536 B against the 1,048,576 B
//! default — 40 bytes under) settles at S=2 and still copies every
//! surviving byte exactly once for nothing, because 100% of each cohort
//! survives its survivor round and gets promoted a cycle later anyway.
//! The **survival-rate lock** closes that: when last cycle's survivor
//! intake (`copied_bytes`) was substantial and ≥90% of it came back out
//! alive this cycle (`survivor_live_bytes`), the aging round demonstrably
//! filters nothing, so the threshold locks to 1 (promote on first copy)
//! until the influx goes quiet. The lock's exit signal — influx below
//! `desired/4` for two consecutive cycles — stays measurable while
//! locked, unlike survivor occupancy, which is zero at S=1 and would
//! leave the loop blind.
//!
//! There is no env knob here (see CLAUDE.md's GC knob kill-policy): the
//! loop is always on, and its neutral state — influx below `desired`,
//! survivor cohorts that die in place — computes S=4, which is
//! bit-for-bit the previous fixed behaviour.
//!
//! ## Seeding the lock from a non-copying collection (#7598)
//!
//! The survival-rate lock above is correct but **one cycle late by
//! construction**: it keys on `prev_copied`, so a *previous copying minor*
//! must already have filled the survivor space. On a workload with one
//! long-lived burst (`json_pipeline`: `out.push({…})` 500k times) the first
//! copying minor therefore always pays the wasted copy — measured 268 MB
//! Eden→survivor on cycle 3 and the same 268 MB survivor→old on cycle 4,
//! ~3.9 s of a 5.1 s phase spent copying one cohort twice.
//!
//! Both blind spots named on #7592 are the same shape: `retune_after_scavenge`
//! is fed **only** by copying minors, so full mark-sweeps and non-copying
//! minor fallbacks feed the loop nothing, and once a workload escalates to
//! those the loop goes blind exactly when it is needed.
//!
//! [`seed_promote_lock_from_sweep`] closes that. Every cycle that reaches the
//! mark-sweep path — a full, or a non-copying minor fallback — already walks
//! every Eden header and classifies it live or dead. That walk yields the two
//! numbers the lock wants, one collection *earlier* than the survivor
//! round-trip can produce them.
//!
//! ### Why this signal is not a fixed point of the policy reading it
//!
//! This issue has already produced three self-referential signals, so the
//! argument is spelled out rather than assumed:
//!
//! * The Eden-side probe reached for `promoted_bytes`, which is **zero by
//!   construction at S=4** — "was the promotion rate high?" can never answer
//!   yes while S=4 holds, so it could never leave S=4.
//! * #7596's first nursery cap gated from-space occupancy on a total that
//!   *included* from-space, so the cap was a bound from-space could never
//!   cross and scavenging stopped entirely.
//! * #7594's handoff scheduled a *non-moving* full to relieve pressure only a
//!   *moving* cycle can relieve, so the predicate was true again next cycle.
//!
//! The Eden live/dead split has none of that structure. It is produced by the
//! mark-sweep's own arena walk: marks come from reachability from roots, and
//! neither the mark phase nor the walk reads `tenuring_survivals()` — the
//! threshold is consulted only by `copying.rs`'s per-object move, which this
//! path does not run. Concretely, at both endpoints the signal stays
//! measurable and keeps its meaning:
//!
//! | state | what Eden holds at the sweep | signal |
//! |---|---|---|
//! | S=4 (state to leave) | everything that has not aged out | high live fraction on a retaining workload ⇒ says "lock" |
//! | S=1 (state to stay in) | only Eden allocated since the last promotion | still the mutator's retention of recent Eden ⇒ still measurable |
//!
//! Exit is deliberately **not** this signal. Entering hands over to the
//! existing `PROMOTE_LOCK`, whose unlock condition (Eden influx below
//! `desired/4` for two consecutive copying minors) is already
//! threshold-invariant and already tested — so the seed cannot introduce an
//! enter/exit oscillation of its own.
//!
//! ### Determinism (#7432)
//!
//! #7432 forbids re-deciding S *while objects are being moved*: the
//! copied/promoted split would then depend on root traversal order. The seed
//! is written at the END of a completed sweep and read at the ENTRY of a later
//! copying minor (`CopyingNurseryCollector::new` snapshots it once), so every
//! object in a cycle still sees exactly one threshold and the counters stay a
//! pure function of (heap state at entry, threshold at entry).
//!
//! Two exclusions at the callsite keep the *input* deterministic too, and both
//! are refusals rather than tuning:
//!
//! * **Budgeted cycles.** Whole-cycle allocate-black marks every mid-cycle
//!   birth, and this walk reads MARKED as live — a churn workload's births
//!   would read as a ~100% live Eden. Same reason the age-bump is suppressed
//!   there (`cycle.rs`).
//! * **Cycles that ran the conservative native-stack scan.** That scan retains
//!   whatever the stack happens to look like a pointer to, by an amount that
//!   varies run to run (`benchmarks/gc_ratchet/README.md`). A liveness
//!   *measurement* taken under it is not sound, and feeding it into a policy
//!   would make the gated copy/promote counters non-deterministic.

use super::*;

/// Ceiling and previous fixed threshold.
pub(super) const GC_TENURING_SURVIVALS_MAX: u8 = GC_COPY_PROMOTION_SURVIVALS;
// The largest young age fits the two-bit field. The adaptive threshold may
// request one further survival: move_young compares next_age before encoding
// it and sets TENURED instead of storing a young age of 4.
pub(super) const GC_MAX_PROMOTION_AGE: u8 = super::layout::GC_COPY_YOUNG_AGE_MAX;
const _: () = assert!(GC_MAX_PROMOTION_AGE <= 3);
const _: () = assert!(GC_TENURING_SURVIVALS_MAX <= GC_MAX_PROMOTION_AGE + 1);

/// The lowest threshold the **occupancy rule** may select.
///
/// Not a tuned number: it is the lowest S at which `copied_bytes > 0`, i.e. the
/// lowest value that still PRODUCES the survivor-round measurement. At S=1
/// nothing enters the survivor space; at S=2 exactly one cohort does.
///
/// Why the occupancy rule must not reach 1 (#9851). A threshold of 1 is a claim
/// about **lifetime** — "this cohort will not die, promote it on first copy" —
/// and the occupancy rule measures **space**: `(S-1) * influx <= desired` asks
/// only whether one cohort fits in the desired survivor size. When it does not,
/// S=1 does not reduce the surviving data; it relocates it, from the survivor
/// space (where the next minor re-examines it for free) to the old generation
/// (which only a full can reclaim). The formula has no term for that.
///
/// Worse, S=1 is **self-sealing**: with nothing copied, `copied_bytes` is 0, so
/// next cycle `prev_copied` is 0, so the survival-rate lock's guard
/// (`prev_copied >= substantial`) is false forever. The state destroys the only
/// measurement that could refute it, and both remaining exits — the occupancy
/// recompute and `PROMOTE_LOCK`'s unlock — are *quiet-influx* exits, which say
/// nothing about lifetime.
///
/// Measured on the compiled claude-code TUI, 4 streamed turns in one process,
/// both arms from one binary via `PERRY_GC_TENURING_SURVIVALS` (3300-char):
///
/// | | adaptive | pinned S=2 |
/// |---|---|---|
/// | minors at S=1 | 351 of 352, carrying 100 % of promotion | 0 |
/// | survivor-round mortality samples | **1** | **393** |
/// | median mortality | **0.9 %** — the first minor of the process | **26.1 %** |
/// | ...in steady turns 2 / 3 / 4 | not measurable | 26.1 / 26.1 / 26.1 % |
/// | promoted | 1057 MB | 792 MB |
///
/// The loop takes its one and only mortality sample on the first minor of the
/// process — before any steady state, when the cohort really is immortal —
/// concludes "nothing dies", and can never sample again. In steady state an
/// aging round filters about **a quarter** of the cohort.
///
/// Reaching 1 still belongs to the two paths that actually MEASURE mortality:
/// the survival-rate lock (`prev_copied` substantial and >=90 % of it came back
/// alive) and the sweep seed (the mark-sweep's own Eden live/dead split). Both
/// are untouched. So the rule is self-limiting: on a workload whose cohort
/// genuinely does not die, the lock fires after one cohort's copy and takes the
/// loop back to 1 — measured 5 of 358 substantial cohorts under that threshold
/// on cc, which is why the clamp sticks there rather than oscillating.
pub(super) const OCCUPANCY_MIN_SURVIVALS: u8 = 2;

/// Consecutive cycles the computed target must exceed the current threshold
/// before it is raised (by one step).
const RAISE_DEBOUNCE_CYCLES: u8 = 2;

/// `NURSERY_CAP_SCALE` counts quarters of the base cap: this value is ×1.
const NURSERY_CAP_SCALE_UNIT: u8 = 4;

/// Floor of the survival-driven nursery ladder: a quarter of the base cap,
/// 16 MB / 4 = 4 MB. Every thread starts here and returns here while its
/// minors find little alive (#11549).
///
/// # Why the floor is below the base
///
/// The base cap was the size every low-survival program ran its young
/// generation at, and on those programs a 16 MB Eden is 12 MB of garbage kept
/// resident between minors for nothing: a minor there copies almost nothing,
/// so its cost is its fixed cost, and #11634 cut that to ~100k instructions.
/// At that price collecting 4× as often is cheaper than it was to collect at
/// 16 MB before the cut, and it lowers peak RSS on every such program.
///
/// Where survivors are a heavy share of each minor the opposite holds —
/// copying survivors, not the fixed cost, dominates, and a small Eden gives
/// them no time to die (qs stringify at a fixed 4 MB: 105 minors instead of
/// 24, +7% instructions). That is what the ladder's existing debounced
/// 4 %/1 % influx rule is for, and it now simply starts two steps lower.
/// [`retune_nursery_cap_scale`] takes all the steps a reading justifies in one
/// debounced move, so such a program leaves the floor after its second minor
/// and settles at the same size it settled at from 16 MB before.
///
/// Only the effective nursery cap starts low. Everything else that is sized
/// from the base cap — the survivor target ([`desired_survivor_bytes`]), the
/// promoted-cohort floor, the allocation-census seed point, the JSON-leaf
/// routing gate — keeps the 16 MB base, because each of them measures a
/// LIFETIME or a volume in bytes allocated, which a smaller Eden must not
/// shorten. Measured before that decoupling (everything keyed on one 4 MB
/// base): validator's aging cohorts were promoted after 8 MB of allocation
/// instead of dying in the survivor space (+26% peak RSS), and a 5 MB startup
/// tree tripped a promoted-cohort full at 4 MB (+45% instructions).
const NURSERY_CAP_SCALE_MIN: u8 = 1;

/// Ceiling of the ladder: 16 MB × 4 = 64 MB, unchanged. Bounds the young-gen
/// RSS contribution on live-set-bound workloads while still cutting their
/// collection count (each collection carries a fixed root-scan/remembered-set/
/// eligibility cost that dominates once the adaptive threshold has eliminated
/// the re-copying).
const NURSERY_CAP_SCALE_MAX: u8 = 16;
const _: () = assert!(NURSERY_CAP_SCALE_MAX == 4 * NURSERY_CAP_SCALE_UNIT);

crate::perry_thread_local! {
    /// Power-on threshold. This is `OCCUPANCY_MIN_SURVIVALS`, not the ceiling:
    /// see `SURVIVOR_ROUND_MEASURED`. Starting at the ceiling is a claim that
    /// young objects live long, made before a single object has been given the
    /// chance to die, and it is the expensive direction of that claim -- every
    /// survivor is copied three times before it can be promoted.
    static TENURING_SURVIVALS: Cell<u8> = const { Cell::new(OCCUPANCY_MIN_SURVIVALS) };
    /// Has any survivor round been RATED yet on this thread -- i.e. did some
    /// cycle put a cohort into the survivor space that the next cycle could
    /// then follow? Until this is true the loop has no lifetime evidence of
    /// any kind, and the occupancy rule may not move off the floor.
    static SURVIVOR_ROUND_MEASURED: Cell<bool> = const { Cell::new(false) };
    static RAISE_STREAK: Cell<u8> = const { Cell::new(0) };
    /// Survival-rate lock: promote-on-first-copy until influx goes quiet.
    static PROMOTE_LOCK: Cell<bool> = const { Cell::new(false) };
    static UNLOCK_STREAK: Cell<u8> = const { Cell::new(0) };
    /// Bytes the previous copying minor put into the to-survivor space —
    /// the denominator of this cycle's survival rate.
    static PREV_COPIED_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Influx-driven multiplier applied to the scavenge nursery cap, in
    /// quarters of it (`NURSERY_CAP_SCALE_UNIT` is ×1): 1, 2, 4, 8 or 16, i.e.
    /// ×¼ … ×4. Power of two; starts at the floor and grows/shrinks one step
    /// at a time, debounced.
    static NURSERY_CAP_SCALE: super::TriggerInput<u8> =
        const { super::TriggerInput::new(NURSERY_CAP_SCALE_MIN) };
    static CAP_GROW_STREAK: Cell<u8> = const { Cell::new(0) };
    static CAP_SHRINK_STREAK: Cell<u8> = const { Cell::new(0) };
    /// The influx readings of the ladder's current phase: their sum and their
    /// count. A phase starts at a move, or at a reading on the far side of
    /// the band from the phase's mean; see [`retune_nursery_cap_scale`].
    static CAP_PHASE_INFLUX_BYTES: Cell<u64> = const { Cell::new(0) };
    static CAP_PHASE_MINORS: Cell<u64> = const { Cell::new(0) };
    /// The phase's whole cycles: whether every cohort since the move died,
    /// the last two readings and the sums of the closed and the open cycle.
    /// See [`CapPhaseCycles`].
    static CAP_PHASE_CYCLES: Cell<CapPhaseCycles> = const { Cell::new(CapPhaseCycles::START) };
    /// #7929: mean size of the objects the last copying minor moved. Seeded at
    /// the calibration reference so a process with no completed copying minor
    /// paces exactly as it did before the object denomination existed — until
    /// #8122's allocation census (below) replaces the seed with a measurement
    /// of THIS program's objects, halfway to the first cap.
    static MEAN_SURVIVING_OBJECT_BYTES: super::TriggerInput<usize> =
        const { super::TriggerInput::new(NURSERY_CAP_REFERENCE_OBJECT_BYTES) };
    /// #8122: has ANY census — the collector's survivor census or the one-time
    /// allocation census — replaced the seed? Once true the allocation probe
    /// never runs again (its walk is paid at most once per process).
    static OBJECT_CENSUS_SEEDED: super::TriggerInput<bool> = const { super::TriggerInput::new(false) };
}

#[cfg(test)]
thread_local! {
    /// Scoped threshold pin for tests of mechanisms that require a particular
    /// promotion age. This is thread-local for the same reason as the adaptive
    /// state: runtime tests share one process and may run on different threads.
    static TENURING_SURVIVALS_TEST_OVERRIDE: Cell<Option<u8>> = const { Cell::new(None) };
}

/// The survivals threshold the next copying minor should promote at:
/// `next_age >= tenuring_survivals()` tenures. In `1..=4`; 4 is the
/// original fixed policy, 1 promotes every live nursery object on first
/// copy. Diagnostic pins accept 0 (also promotes on the first copy) through
/// `GC_MAX_PROMOTION_AGE`; larger u8 pins saturate with one process warning.
pub(super) fn tenuring_survivals() -> u8 {
    #[cfg(test)]
    if let Some(forced) = TENURING_SURVIVALS_TEST_OVERRIDE.with(Cell::get) {
        return forced;
    }
    if let Some(forced) = tenuring_survivals_override() {
        return forced;
    }
    TENURING_SURVIVALS.with(Cell::get)
}

/// An immutable per-minor promotion decision. A pending nursery-growth sample
/// must be rated before its pointer-bearing parents become old roots: promoting
/// them while measuring their first round makes their later-dead remembered
/// edges retain arbitrary amounts of new Eden data. One extra young round keeps
/// those parents collectible until the sample reaches the controller. Leaf
/// objects cannot retain a younger graph and keep the ordinary threshold.
#[derive(Clone, Copy)]
pub(super) struct PromotionPolicy {
    survivals: u8,
    parent_probe: bool,
}

impl PromotionPolicy {
    pub(super) fn snapshot() -> Self {
        #[cfg(test)]
        let pinned = TENURING_SURVIVALS_TEST_OVERRIDE.with(Cell::get).is_some();
        #[cfg(not(test))]
        let pinned = false;
        let pinned = pinned || tenuring_survivals_override().is_some();
        Self {
            survivals: tenuring_survivals(),
            parent_probe: !pinned
                && !PROMOTE_LOCK.with(Cell::get)
                && !SURVIVOR_ROUND_MEASURED.with(Cell::get)
                && PREV_COPIED_BYTES.with(Cell::get) != 0
                && CAP_GROW_STREAK.with(Cell::get) != 0,
        }
    }

    pub(super) fn survivals(self) -> u8 {
        self.survivals
    }

    pub(super) fn parent_probe_active(self) -> bool {
        self.parent_probe
    }

    #[inline]
    pub(super) fn for_type(self, obj_type: u8) -> u8 {
        if self.parent_probe && gc_type_info(obj_type).is_none_or(|info| !info.pointer_free) {
            // The usual floor produces a first-round sample; one further age
            // keeps its parents young until that sample can guide a later GC.
            self.survivals.max(OCCUPANCY_MIN_SURVIVALS + 1)
        } else {
            self.survivals
        }
    }
}

/// Pin the promotion age for a threshold-sensitive test on this thread.
/// Restores the previous pin on drop; the adaptive policy continues to run
/// underneath it, but every copying minor snapshots the explicitly pinned age.
#[cfg(test)]
pub(super) fn set_survivals_for_test(survivals: u8) -> TenuringSurvivalsTestGuard {
    assert!((1..=GC_TENURING_SURVIVALS_MAX).contains(&survivals));
    TenuringSurvivalsTestGuard(
        TENURING_SURVIVALS_TEST_OVERRIDE.with(|cell| cell.replace(Some(survivals))),
    )
}

#[cfg(test)]
pub(super) struct TenuringSurvivalsTestGuard(Option<u8>);

#[cfg(test)]
impl Drop for TenuringSurvivalsTestGuard {
    fn drop(&mut self) {
        TENURING_SURVIVALS_TEST_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

/// `PERRY_GC_TENURING_SURVIVALS=<u8>` pins the promotion age, overriding the
/// adaptive threshold (#7432). Effective range 0..=3; larger u8 values
/// saturate to `GC_MAX_PROMOTION_AGE` with one stderr warning per process.
/// Diagnostic only; unset means adaptive.
///
/// # Why (#7803)
///
/// Three independent measurements say this bug gets LESS likely as collections
/// get denser — paced ~30-50%, interpreter safepoints on 2/8 vs 6/8, unpaced
/// (9.4x the cycles) passing. That is backwards for a value held unrooted
/// across a collection point, and it is what four separate rooting fixes
/// failing to move the rate looks like.
///
/// `moved_objects` explains it: 892k unpaced against 862k paced, despite 9.4x
/// the cycles. The extra collections are not relocating MORE, they are
/// promoting the same objects SOONER — and an old-gen object is not moved by a
/// minor. So denser collections mean fewer relocations per object.
///
/// If the defect needs an object RELOCATED while a stale reference to it
/// exists, this knob tests it directly instead of through the schedule:
///
///   `=1`   promote on the first minor  -> fewest relocations -> predicts the
///          failure disappears;
///   `=3`   promote on the third minor -> up to two survivor relocations.
///
/// Historical `=255` experiments disabled promotion by age. Those pins now
/// saturate to 3 so every young age remains representable in the header.
///
/// Pair it with `PERRY_GC_SCHEDULE_ALLOC_KB=0`, which pins the schedule
/// exactly (63,941 safepoints, reproduced to the digit), so a change in
/// outcome at a fixed seed is attributable to this knob and nothing else.
fn tenuring_survivals_override() -> Option<u8> {
    use std::sync::OnceLock;
    static OVERRIDE: OnceLock<Option<u8>> = OnceLock::new();
    *crate::once_init::get_or_init(&OVERRIDE, || {
        std::env::var("PERRY_GC_TENURING_SURVIVALS")
            .ok()
            .and_then(|v| v.trim().parse::<u8>().ok())
            .map(|pin| {
                let effective = saturated_tenuring_pin(pin);
                if effective != pin {
                    eprintln!(
                        "[gc] PERRY_GC_TENURING_SURVIVALS={pin} exceeds {GC_MAX_PROMOTION_AGE}; using {effective}"
                    );
                }
                effective
            })
    })
}

/// Shared bound for diagnostic pins; the adaptive loop retains its existing
/// compare-before-store fourth-survival endpoint.
#[inline]
pub(super) fn saturated_tenuring_pin(pin: u8) -> u8 {
    pin.min(GC_MAX_PROMOTION_AGE)
}

#[cfg(test)]
pub(super) fn set_env_pin_for_test(pin: u8) -> TenuringSurvivalsTestGuard {
    TenuringSurvivalsTestGuard(
        TENURING_SURVIVALS_TEST_OVERRIDE
            .with(|cell| cell.replace(Some(saturated_tenuring_pin(pin)))),
    )
}

/// The effective scavenge nursery cap: the configured base
/// (`PERRY_GC_SCAVENGE_NURSERY_MB`, default 16 MB) times the influx-driven
/// scale. A fixed cap sets collection frequency independently of how much
/// survives; on live-set-bound workloads (tree/retain/deeplist shapes) that
/// multiplies the per-collection fixed cost by an enormous collection
/// count AND promotes objects that a larger Eden would have let die young.
/// The scale starts at a quarter of the base and grows only while survivor
/// influx stays a heavy fraction of Eden, so the small-live-set workloads
/// #7377 fixed never leave the 4 MB floor ([`NURSERY_CAP_SCALE_MIN`]).
///
/// #7592: the influx-driven product is the floor, not the whole answer — it
/// is bounded by `base × 4` (64 MB), and any *constant*
/// cap sets collection cadence independently of how much is live while each
/// collection's fixed cost is O(old-gen) (#6181: full-region sweep walk,
/// whole-heap remembered-set rebuild). Total young-GC work is then
/// `(alloc / cap) × O(live)` — quadratic in the live set. The effective cap
/// is therefore also proportional to the TENURED live set, which keeps work
/// per allocated byte bounded as the heap grows.
///
/// The proportional term deliberately keys on old-gen occupancy, NOT total
/// arena in-use: the cap gates `young_scavenge_cap_due()` against from-space
/// occupancy, and from-space is part of arena in-use — a cap defined by a
/// total that includes the young generation is a fixed point from-space can
/// never cross, and scavenging stops entirely (measured on #7592: 0 copying
/// minors at 200k records, the nursery never evacuated). Old-gen is outside
/// the young generation, so no self-reference. Reclaimable pressure (in-use
/// minus free-list holes, #7437) rather than raw in-use, so dead-but-swept
/// old bytes do not inflate Eden.
pub(super) fn scavenge_nursery_cap_effective_bytes() -> usize {
    scavenge_nursery_cap_from(
        influx_driven_nursery_cap_bytes(),
        old_gen_reclaimable_pressure_bytes(),
    )
}

/// The influx-driven half of the cap on its own: the configured base times
/// the debounced `NURSERY_CAP_SCALE`, **re-denominated in objects** by
/// [`nursery_cap_object_scale_permille`]. Named so the composition below reads
/// as the two-term policy it is.
pub(super) fn influx_driven_nursery_cap_bytes() -> usize {
    influx_driven_nursery_cap_bytes_at(NURSERY_CAP_SCALE.with(TriggerInput::get))
}

/// [`influx_driven_nursery_cap_bytes`] at a given ladder scale, so the retune
/// can price a candidate step before taking it.
fn influx_driven_nursery_cap_bytes_at(scale: u8) -> usize {
    let constant_band = gc_scavenge_nursery_cap_bytes().saturating_mul(scale as usize)
        / NURSERY_CAP_SCALE_UNIT as usize;
    // The multiply is done in u64 deliberately. `usize::saturating_mul` on an
    // ILP32 target (watchOS/visionOS are 32-bit) would saturate a 64 MB band
    // against a 1000-per-mille factor at `u32::MAX` and the following divide
    // would then hand back ~4 MB — a silent 16x cap collapse on exactly the
    // devices with the least headroom. A 64-bit intermediate cannot overflow:
    // the band is bounded by 4 GB and the factor by 1000.
    let scaled = constant_band as u64
        * nursery_cap_object_scale_permille(mean_surviving_object_bytes()) as u64
        / 1000;
    scaled.min(usize::MAX as u64) as usize
}

/// #7929: the mean size of the objects the last copying minor actually moved,
/// in bytes — `(copied + promoted) bytes / (copied + promoted) objects`.
///
/// Seeded at [`NURSERY_CAP_REFERENCE_OBJECT_BYTES`] so a process that has never
/// completed a copying minor paces bit-identically to the pre-#7929 collector.
pub(super) fn mean_surviving_object_bytes() -> usize {
    MEAN_SURVIVING_OBJECT_BYTES.with(TriggerInput::get)
}

/// Mean surviving object size the 16 MB constant band was calibrated against.
///
/// #7056 (the cap) and #7377/#7592 (its scale ladder) were measured on a heap
/// whose two-field object literal was **72 bytes**; #7928 right-sized that to
/// 56. The constant is the *calibration anchor*, not a claim about any current
/// representation — moving it re-tunes the cap for every program at once.
pub(super) const NURSERY_CAP_REFERENCE_OBJECT_BYTES: usize = 72;
/// Floor for the object-denomination factor (per mille). At the corpus's
/// smallest measured mean (32.8 B on `tree_wide`) the unclamped factor is 456;
/// the floor bounds how far a shrinking representation may pull the cap down,
/// so the collection *count* cannot run away on a workload whose survivors are
/// atypically small.
const NURSERY_CAP_OBJECT_SCALE_MIN_PERMILLE: usize = 500;

/// #7929: how much of the byte-denominated constant band this representation
/// should get, in per mille, so the band buys a **constant number of objects**.
///
/// The collector's trigger is denominated in bytes and its per-cycle cost is
/// per object, so a fixed byte band silently buys more collector work as
/// objects shrink: #7928 took a two-field object 72 B → 56 B and every minor
/// then moved 1.286× (= 72/56) as many objects for the same bytes, costing
/// `deeplist` +10.5% and `retain1` +8.8% wall. Scaling the band by
/// `mean / reference` restores `band / mean` — the object budget — to what the
/// band was calibrated to buy.
///
/// **Deliberately one-sided.** The factor is clamped at 1000, so a
/// representation *larger* than the reference gets the unchanged band rather
/// than a proportionally larger one. Two reasons, both measured:
///
/// * The byte-weighted mean is not a representative object size on a workload
///   whose survivors are dominated by large allocations — `push_num`'s mean is
///   **3600 B** because it survives arrays, not objects (the documented blocker
///   on #7929). One-sided clamping turns that from a ×50 cap explosion into
///   exactly today's behaviour.
/// * The corpus response to the cap is threshold-dominated (which cycle *kinds*
///   fire), so *raising* a cap is the risky direction: it is how a program
///   lands in a #7909 budgeted stall. (Until #10928 this also named
///   `GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES` as a crossing a program could be
///   pushed over; that threshold no longer gates anything - the live input
///   is `gc_old_reclaim_growth_band_bytes`.) Every program at or above the reference — `retain_wide`,
///   `retain_wide1`, `push_num`, `shapes` — is left bit-identical.
pub(super) fn nursery_cap_object_scale_permille(mean_surviving_object_bytes: usize) -> usize {
    if mean_surviving_object_bytes == 0 {
        return 1000;
    }
    (mean_surviving_object_bytes * 1000 / NURSERY_CAP_REFERENCE_OBJECT_BYTES)
        .clamp(NURSERY_CAP_OBJECT_SCALE_MIN_PERMILLE, 1000)
}

/// Feed one finished copying minor's move census into the object denomination.
///
/// Called from the copying minor immediately before [`retune_after_scavenge`],
/// so everything that end-of-cycle computes is policy for the *next* cycle and
/// sees one consistent factor. A cycle that moved nothing carries the previous
/// estimate forward rather than resetting it — `tree`/`cycles` move single
/// digits of objects per minor and a zero denominator there is a missing
/// measurement, not a measurement of zero.
pub(super) fn note_surviving_object_census(moved_bytes: usize, moved_objects: usize) {
    if moved_objects == 0 {
        return;
    }
    let mean = moved_bytes / moved_objects;
    if mean == 0 {
        return;
    }
    OBJECT_CENSUS_SEEDED.with(|seeded| seeded.set(true));
    let previous = MEAN_SURVIVING_OBJECT_BYTES.with(|c| c.replace(mean));
    if previous != mean && crate::gc::gc_diag_enabled() {
        eprintln!(
            "[gc-tenuring] nursery cap object denomination: mean_surviving_object_bytes {} -> {} \
             (scale {} permille, band {} B)",
            previous,
            mean,
            nursery_cap_object_scale_permille(mean),
            influx_driven_nursery_cap_bytes()
        );
    }
}

/// #8122: seed the object denomination from an ALLOCATION census, once, before
/// the first copying minor exists to measure survivors.
///
/// # Why
///
/// The constant band is denominated in objects (#7929) only once a copying
/// minor has reported what it moved. Until then `MEAN_SURVIVING_OBJECT_BYTES`
/// holds the 72 B calibration reference, so the FIRST minor fires on the raw
/// 16 MB byte band whatever the program's objects actually weigh: for a 56 B
/// object that is 300k objects, for a 48 B one 350k, against the 233k the band
/// was calibrated to buy. That first minor is also the one that has to TRACE
/// (no survival estimate exists yet, so it cannot promote untraced), and a
/// traced in-place-promotion cycle costs ~1,600 instructions per object.
/// Measured on `deeplist`/`retain1` when the two-field object went 56 → 48 B:
/// the whole +8–9% instruction regression was those extra first-cycle objects;
/// the mutator with no collection at all was byte-identical.
///
/// # What it does
///
/// Halfway to the configured base cap (8 MB of from-space by default) — a
/// point every program that will ever reach the cap passes exactly once — hop
/// the young generation's headers (`arena::young_allocation_census`, ~1M
/// instructions for 8 MB of small objects, paid ONCE per process) and install
/// `bytes / objects` as the mean. The first minor is then object-denominated
/// like every later one, and a smaller representation no longer buys the
/// collector a bigger first trace. The one-sided clamp still applies: a mean
/// above the reference leaves the 16 MB band untouched, so an array-dominated
/// allocation stream cannot raise the cap. The collector's survivor census
/// overwrites this seed at the first minor, so steady state is unchanged.
///
/// Returns without walking when the base cap is not yet half full, when a
/// census (either kind) already exists, or when the nursery is empty.
/// Has the object denomination been seeded, by an allocation census or by a
/// copying minor's survivor census? Once true it stays true for the process
/// (tests reset it).
#[inline]
pub(super) fn object_census_seeded() -> bool {
    OBJECT_CENSUS_SEEDED.with(TriggerInput::get)
}

/// The young-generation occupancy at which the one-time allocation census is
/// taken: halfway to the base cap. Shared with the trigger watermark (#10698),
/// which must not let a fast-path answer carry past the reading that seeds.
pub(super) fn object_census_seed_point_bytes() -> usize {
    super::policy::gc_scavenge_nursery_cap_bytes() / 2
}

pub(super) fn maybe_seed_object_census_from_allocation(from_space_in_use_bytes: usize) {
    if OBJECT_CENSUS_SEEDED.with(TriggerInput::get) {
        return;
    }
    if from_space_in_use_bytes < object_census_seed_point_bytes() {
        return;
    }
    // Take the walk at most once even if it yields nothing (an empty or
    // header-less nursery): the flag is the bound on its cost.
    OBJECT_CENSUS_SEEDED.with(|seeded| seeded.set(true));
    let (bytes, objects) = crate::arena::young_allocation_census();
    if objects == 0 {
        return;
    }
    let mean = bytes / objects;
    if mean == 0 {
        return;
    }
    let previous = MEAN_SURVIVING_OBJECT_BYTES.with(|c| c.replace(mean));
    if crate::gc::gc_diag_enabled() {
        eprintln!(
            "[gc-tenuring] nursery cap object denomination: allocation census seeded \
             mean_object_bytes {} -> {} ({} objects / {} B in from-space; scale {} permille, band {} B)",
            previous,
            mean,
            objects,
            bytes,
            nursery_cap_object_scale_permille(mean),
            influx_driven_nursery_cap_bytes()
        );
    }
}

/// The cap policy as a **pure function of its two inputs**, so it is testable
/// without arranging a heap state.
///
/// Splitting this out is not cosmetic. `old_gen_reclaimable_pressure_bytes()`
/// reads live arena state, and in a unit-test thread old-gen is ~empty — so
/// every assertion made against `scavenge_nursery_cap_effective_bytes()`
/// directly would pass with the proportional term deleted. That is the #7024
/// shape: a green test whose subject never ran. The two terms are exercised
/// here explicitly instead.
pub(super) fn scavenge_nursery_cap_from(influx_driven: usize, tenured_reclaimable: usize) -> usize {
    influx_driven.max(tenured_reclaimable / TENURED_EDEN_DIVISOR)
}

/// #7592: divisor for the tenured-proportional nursery cap — Eden may grow to
/// half the tenured live set before a scavenge is forced. Peak young-gen RSS
/// contribution is therefore bounded at `tenured / 2`; the young collection
/// count is logarithmic in heap growth on promote-heavy workloads
/// (`old_{n+1} ≈ old_n × (1 + 1/2)`) instead of linear in bytes allocated.
const TENURED_EDEN_DIVISOR: usize = 2;

/// Target steady-state survivor occupancy: 1/16 of the effective nursery
/// cap, so the tenuring dials track both the configured base and the
/// influx-driven scale — but never less than 1/16 of the BASE cap.
///
/// The floor is #11549's. Below the base the ladder shrinks Eden to save
/// resident garbage, and the survivor target must not shrink with it: it sets
/// how long a cohort may age before promotion, and a quarter-size target
/// would promote cohorts after a quarter of the allocation they had before.
/// validator/batch at a 4 MB Eden with the target following it promoted
/// 27.6 MB of cohorts that die in the survivor space at 16 MB (+26% peak RSS).
/// The survivor space this keeps is bounded by the target itself, 1 MB.
pub(super) fn desired_survivor_bytes() -> usize {
    desired_survivor_bytes_at_cap(scavenge_nursery_cap_effective_bytes())
}

fn desired_survivor_bytes_at_cap(cap: usize) -> usize {
    cap.max(gc_scavenge_nursery_cap_bytes()) / 16
}

/// Fresh allocation needed for one complete Eden lifetime observation.
/// Ordinary survivors already consume part of the nursery's trigger budget.
/// Credit that occupancy only up to its target: excessive carryover must not
/// turn a short allocation interval into another quiet lifetime sample.
pub(super) fn eden_allocation_window_bytes(cap: usize, survivor_bytes: usize) -> usize {
    let survivor_budget =
        desired_survivor_bytes_at_cap(cap).min(cap / usize::from(OCCUPANCY_MIN_SURVIVALS));
    cap.saturating_sub(survivor_bytes.min(survivor_budget))
        .max(1)
}

/// The effective influx-driven cap at the floor of the ladder, before the
/// object denomination: a quarter of the base (4 MB by default).
#[cfg(test)]
pub(super) fn nursery_cap_floor_bytes() -> usize {
    gc_scavenge_nursery_cap_bytes() * NURSERY_CAP_SCALE_MIN as usize
        / NURSERY_CAP_SCALE_UNIT as usize
}

pub(super) fn compute_target_survivals(eden_live_bytes: usize, desired_bytes: usize) -> u8 {
    if eden_live_bytes == 0 {
        return GC_TENURING_SURVIVALS_MAX;
    }
    let target = 1 + desired_bytes / eden_live_bytes;
    target.min(GC_TENURING_SURVIVALS_MAX as usize) as u8
}

/// Feed one finished copying-minor cycle into the feedback loop.
/// `eden_live_bytes` is the cycle's Eden survivor influx (bytes moved out
/// of Eden, whether copied to a survivor space or promoted).
///
/// The other two are **one cohort's** intake and that same cohort's survival,
/// and they must stay that way (#9851 follow-up):
/// `eden_copied_bytes` is what this cycle copied out of *Eden* into the
/// to-survivor space — a fresh cohort, no re-copies — and
/// `first_round_live_bytes` is what came back out of the from-survivor space
/// alive with a stored age of 1, i.e. members of the cohort that the
/// *previous* cycle's `eden_copied_bytes` counted.
///
/// `full_eden_window` says fresh allocation covered the entry Eden budget,
/// crediting carried survivors only within their occupancy target. Excessive
/// survivor pressure or an early trigger cannot advance the quiet unlock clock.
///
/// Why not the whole space. The survivor spaces are a strict semispace pair,
/// so the from-space at cycle N holds exactly what cycle N-1 copied, and
/// `survivor_live_bytes / prev_copied_bytes` is a well-formed survival ratio —
/// of the whole space. But *what that space contains* is set by the very
/// threshold this loop controls: at S<=2 it is one fresh cohort, at S=3-4 it
/// also holds age-2 and age-3 objects, which have already survived a round and
/// are therefore selected for longevity. Rating that mixture and concluding
/// "the aging round filters nothing" applies a measurement of an aged,
/// self-selected population to first-round cohorts. Measured on the compiled
/// claude-code TUI: a fresh cohort survives at 74 %, and the loop still reached
/// the lock's 90 % bar 8-12 times per four-turn run once #9851's clamp let the
/// ladder climb past 2.
pub(super) fn retune_after_scavenge(
    eden_live_bytes: usize,
    eden_copied_bytes: usize,
    first_round_live_bytes: usize,
    full_eden_window: bool,
) {
    retune_nursery_cap_scale(eden_live_bytes, first_round_live_bytes);
    let desired = desired_survivor_bytes();
    let substantial = desired / 4;
    let prev_cohort_copied = PREV_COPIED_BYTES.with(|c| c.replace(eden_copied_bytes));
    // A cohort went into the survivor space last cycle, so THIS cycle is the
    // one that could follow it: from here on the loop has lifetime evidence and
    // the occupancy rule is allowed to move off the floor.
    if prev_cohort_copied > 0 {
        SURVIVOR_ROUND_MEASURED.with(|m| m.set(true));
    }
    let current = TENURING_SURVIVALS.with(Cell::get);

    if PROMOTE_LOCK.with(Cell::get) {
        // A short Eden interval may mainly collect the previous survivors.
        // It cannot rate fresh allocation's lifetime: counting it as quiet
        // lets an extra aging round advance the unlock clock. Only completed
        // Eden allocation windows advance quiet hysteresis. A substantial
        // influx still refutes quietness, even on an early collection.
        if eden_live_bytes < substantial {
            if !full_eden_window {
                return;
            }
            let streak = UNLOCK_STREAK.with(|s| s.get()).saturating_add(1);
            if streak >= RAISE_DEBOUNCE_CYCLES {
                PROMOTE_LOCK.with(|l| l.set(false));
                UNLOCK_STREAK.with(|s| s.set(0));
                RAISE_STREAK.with(|s| s.set(0));
                // Resume the ladder one step up rather than snapping to the
                // ceiling; the normal debounced rise takes it the rest of
                // the way if the workload stays quiet.
                set_survivals(current, 2, eden_live_bytes, "unlock");
            } else {
                UNLOCK_STREAK.with(|s| s.set(streak));
            }
        } else {
            UNLOCK_STREAK.with(|s| s.set(0));
        }
        return;
    }

    // Survival-rate lock: last cycle's FRESH COHORT was substantial and
    // (nearly) all of it came back out alive, so the aging round filters
    // nothing — every copied byte is a byte that will be promoted anyway.
    //
    // Both sides are scoped to that one cohort (#9851 follow-up). Rating the
    // whole survivor space instead makes the ratio rise with the threshold
    // this rule sets, because a higher threshold is precisely what keeps
    // already-aged objects in the space; the rule then reads its own setting
    // back as evidence. See `retune_after_scavenge`'s header.
    if prev_cohort_copied >= substantial
        && first_round_live_bytes.saturating_mul(10) >= prev_cohort_copied * 9
    {
        PROMOTE_LOCK.with(|l| l.set(true));
        UNLOCK_STREAK.with(|s| s.set(0));
        RAISE_STREAK.with(|s| s.set(0));
        set_survivals(current, 1, eden_live_bytes, "lock");
        return;
    }

    // #9851: the occupancy rule measures SPACE and may not conclude 1, which is
    // a claim about LIFETIME — see `OCCUPANCY_MIN_SURVIVALS`. Deliberately
    // clamped HERE and not inside `compute_target_survivals`: that pure function
    // has a second caller, `full_seed_promotes_on_first_copy`, which gates the
    // sweep seed on `... != 1` ("would occupancy alone already promote on first
    // copy?"). Clamping the shared function would silently disarm the sweep
    // seed, which is one of the two paths that IS allowed to reach 1.
    //
    // The startup follow-up makes that rule SYMMETRIC. `1 + desired / influx`
    // returns the ceiling for a tiny influx and for a zero one, so on the first
    // minors of a process — when the heap is nearly empty and no cohort has
    // ever been followed — occupancy claims the maximum. That is the same
    // category error in the other direction: a claim about LIFETIME from a
    // measurement of SPACE, made before any evidence exists, and the expensive
    // one, because every survivor is then copied up to three times before it
    // may be promoted. Measured on the compiled claude-code TUI, the whole
    // adaptive-vs-pinned difference was this startup excursion —
    // `4 -> 2 (occupancy) -> 1 (lock)` inside turn 1 and nothing afterwards,
    // worth +0.35..0.45 s at 3300 chars and +50 % at 400.
    //
    // So until one survivor round has actually been rated, occupancy holds at
    // the floor: the lowest threshold that PRODUCES the measurement it needs to
    // say anything at all. Evidence, not the ladder, is what lets it move.
    let measured = SURVIVOR_ROUND_MEASURED.with(Cell::get);
    let target = if measured {
        compute_target_survivals(eden_live_bytes, desired).max(OCCUPANCY_MIN_SURVIVALS)
    } else {
        OCCUPANCY_MIN_SURVIVALS
    };
    let next = if target < current {
        RAISE_STREAK.with(|s| s.set(0));
        target
    } else if target > current {
        let streak = RAISE_STREAK.with(|s| s.get()).saturating_add(1);
        if streak >= RAISE_DEBOUNCE_CYCLES {
            RAISE_STREAK.with(|s| s.set(0));
            current + 1
        } else {
            RAISE_STREAK.with(|s| s.set(streak));
            current
        }
    } else {
        RAISE_STREAK.with(|s| s.set(0));
        current
    };
    set_survivals(current, next, eden_live_bytes, "occupancy");
}

/// Retune the nursery cap scale from one copying minor's survivor influx.
///
/// Grow when survivor influx exceeds 4% of the current effective cap —
/// objects are surviving because they aren't getting time to die, so a bigger
/// Eden both cuts the collection count and lets them die young. Shrink when
/// influx falls below 1%.
///
/// # What a move needs: the level and its persistence
///
/// A move needs two things, and each answers a different question:
///
/// * **The level**: the MEAN influx of the current phase is beyond the band
///   edge. One minor's reading is a sample of a signal, not its level: a
///   program that builds a structure and drops it has a sawtooth influx, and
///   the reading is wherever on the tooth the minor happens to land. Measured
///   on AWFY Storage (EK, 2026-10-23): at 8 MB its readings climb from 120 KB
///   to 400 KB within each tree and drop to 40 KB when the tree dies, a mean of
///   ~260 KB against a 336 KB edge. Two consecutive readings near the tooth
///   tip clear the edge, so the per-reading rule grew to 16 MB, read the trough
///   there, shrank back, and did that five times per run (arena peak 26 MB
///   instead of 18 MB, +25% RSS). Where the tooth tip lands is decided by the
///   collection phase, so an 8-byte change in one object's size moved the
///   program across the cliff (main climbs the same way at a 12-15 MB base).
///   The phase mean is the level the rule is about, and it does not move with
///   the phase.
/// * **The persistence**: the last [`RAISE_DEBOUNCE_CYCLES`] readings are each
///   beyond the edge too. One heavy cycle — a program's startup cohort, say —
///   moves nothing: a mean of two readings, one of them a startup tree, is
///   the cohort's size and not a rate (measured on AWFY CD: the mean alone
///   leaves the floor at the second minor on a program whose phase mean sits
///   inside the band).
///
/// A phase starts at every move, and at a reading on the far side of the band
/// from the phase's mean (above 4% while the mean is below 1%, or the
/// reverse): that reading refutes the level the phase measured, so a program
/// that goes from quiet to heavy is followed after the same two readings it
/// always was, instead of averaging its new rate against its old one.
///
/// A move takes, at once, every ×2 step that the one-step rule would take in a
/// row on the same readings (#11549): growth continues while the next level's
/// cap still passes both tests — the mean and the current reading above 4% of
/// it — and shrinking while both are still below 1% of it. Pricing the mean
/// alone would carry the startup cohort into the landing: on AWFY Json and CD
/// a 600 KB first minor kept the mean above 4% of 16 MB, so the ladder jumped
/// from the floor to 16 MB where the current reading justified 8 MB, and then
/// stayed there (+16% RSS). So the scale lands exactly where one-step moves would have settled —
/// the steady-state sizes are unchanged — without paying two minors per step
/// to get there. With the floor two steps below the old base, one-step
/// climbing cost a survivor-heavy program up to eight extra minors, each
/// copying a cohort that a big enough Eden would have let die (measured on the
/// 12_large_live_set ratchet probe: 9 minors and +3% instructions against
/// main's 5). A steady influx cannot oscillate: a growth ends above 2% of the
/// new cap (the level below was above 4%), a shrink below 2% (the level above
/// was below 1%), and both are inside the 1%..4% dead band.
///
/// # What a reading at the next level is
///
/// Pricing `next` with the current reading is not an assumption that the
/// influx is independent of Eden size; it is the lower bound of a measured
/// identity. A 2E Eden at allocation instant A holds what an E Eden holds,
/// I_E(A), plus what the E minor at A−E copied that is still alive at A —
/// that cohort's first-round survivors R(A):
///
/// `I_2E(A) = I_E(A) + R(A) >= I_E(A)`.
///
/// Where cohorts die in their first round (R = 0) the reading is the same at
/// every level; elsewhere a move lands low and the next debounced reading
/// moves again, so it costs minors and never overshoots. Measured on pinned
/// rungs (GCL, 2026-10-23): AWFY Storage reads 182 / 184 / 187 KB per minor at
/// 4 / 8 / 16 MB with R = 0 on every minor; Havlak and the large-live-set
/// probe, whose cohorts survive, rise with E.
///
/// # Only a whole cycle is a level
///
/// The phase mean is the level only when the phase holds whole cycles of the
/// signal. When R = 0 no byte a minor copied lives through the next window,
/// so the readings are samples of a population that refills every cycle: a
/// run of rising readings is not a structure accumulating (that would carry
/// bytes into the next round, R > 0) but the minor landing later in a cycle
/// shorter than the window. Measured on AWFY Storage: its young bytes are a
/// sawtooth that rises about 20 KB per 4 MB allocated to about 410 KB and
/// wraps. EK's phase mean grew to 16 MB on the rising half (+24% RSS). Judging
/// the run since the edge was crossed (GCL T1) fixed that, but its far-side
/// restart cut the cycle at every wrap: T1 shrank on a two-reading trough
/// fragment (0.01% and 0.47%, while the cycle's mean at 2/4 is 2.52%) and
/// then held every grow at 1/4, where the cycle's mean is 5.05%.
///
/// So while every reading since the move had its previous cohort followed
/// and dead, the level is the mean of the phase's whole cycles: its readings
/// from the first cycle boundary to the latest one, where a boundary is the
/// reading after a peak (the last two readings did not fall into it and it
/// did not rise above it, compared in bytes, which R = 0 makes independent of
/// the window). The far-side restart does not cut a cycle there, and until a
/// whole cycle has closed the phase moves nothing (see [`CapPhaseCycles`]).
/// One reading whose cohort survived or was not followed makes the phase
/// EK's again, for the rest of the phase. The process's first reading has no
/// followed cohort, so its phase, and every startup move, is EK's.
fn retune_nursery_cap_scale(eden_live_bytes: usize, first_round_live_bytes: usize) {
    let scale = NURSERY_CAP_SCALE.with(TriggerInput::get);
    let old_reclaimable = old_gen_reclaimable_pressure_bytes();
    let cap_at = |scale: u8| {
        scavenge_nursery_cap_from(influx_driven_nursery_cap_bytes_at(scale), old_reclaimable)
    };
    let cap = cap_at(scale);
    // Called before `retune_after_scavenge` replaces it, so this is still the
    // cohort `first_round_live_bytes` follows.
    let cohort_died = PREV_COPIED_BYTES.with(Cell::get) > 0 && first_round_live_bytes == 0;
    let (level, phase_mean) = note_cap_phase_reading(eden_live_bytes, cap, cohort_died);
    if eden_live_bytes > cap / 25 {
        CAP_SHRINK_STREAK.with(|s| s.set(0));
        if scale < NURSERY_CAP_SCALE_MAX {
            let streak = CAP_GROW_STREAK.with(|s| s.get()).saturating_add(1);
            if streak >= RAISE_DEBOUNCE_CYCLES && level.is_some_and(|l| l > cap / 25) {
                let level = level.unwrap_or(phase_mean);
                CAP_GROW_STREAK.with(|s| s.set(0));
                let mut next = scale * 2;
                while next < NURSERY_CAP_SCALE_MAX && level.min(eden_live_bytes) > cap_at(next) / 25
                {
                    next *= 2;
                }
                NURSERY_CAP_SCALE.with(|s| s.set(next));
                start_cap_phase();
                diag_cap_scale(scale, next, eden_live_bytes);
            } else {
                if streak >= RAISE_DEBOUNCE_CYCLES && phase_mean > cap / 25 {
                    diag_cap_cycle_hold(scale, "grow", level, eden_live_bytes);
                }
                CAP_GROW_STREAK.with(|s| s.set(streak));
            }
        }
    } else if eden_live_bytes < cap / 100 {
        CAP_GROW_STREAK.with(|s| s.set(0));
        if scale > NURSERY_CAP_SCALE_MIN {
            let streak = CAP_SHRINK_STREAK.with(|s| s.get()).saturating_add(1);
            if streak >= RAISE_DEBOUNCE_CYCLES && level.is_some_and(|l| l < cap / 100) {
                let level = level.unwrap_or(phase_mean);
                CAP_SHRINK_STREAK.with(|s| s.set(0));
                let mut next = scale / 2;
                while next > NURSERY_CAP_SCALE_MIN
                    && level.max(eden_live_bytes) < cap_at(next) / 100
                {
                    next /= 2;
                }
                NURSERY_CAP_SCALE.with(|s| s.set(next));
                start_cap_phase();
                diag_cap_scale(scale, next, eden_live_bytes);
            } else {
                if streak >= RAISE_DEBOUNCE_CYCLES && phase_mean < cap / 100 {
                    diag_cap_cycle_hold(scale, "shrink", level, eden_live_bytes);
                }
                CAP_SHRINK_STREAK.with(|s| s.set(streak));
            }
        }
    } else {
        CAP_GROW_STREAK.with(|s| s.set(0));
        CAP_SHRINK_STREAK.with(|s| s.set(0));
    }
}

/// Which side of the cap's 1%..4% band an influx lies on: 1 above, -1 below,
/// 0 inside.
fn cap_band_side(influx_bytes: usize, cap: usize) -> i8 {
    if influx_bytes > cap / 25 {
        1
    } else if influx_bytes < cap / 100 {
        -1
    } else {
        0
    }
}

/// The ladder phase's whole cycles (see [`retune_nursery_cap_scale`], "Only a
/// whole cycle is a level"). Policy state of the same kind as the streaks:
/// reset at a move, and not at the far-side restart, which would cut a cycle
/// at its wrap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CapPhaseCycles {
    /// Every reading of the phase had its previous cohort followed and dead.
    all_died: bool,
    /// How many of `last` and `before_last` are readings of the phase (0..=2).
    readings: u8,
    last: usize,
    before_last: usize,
    /// A cycle boundary was seen: the readings before the first one are a
    /// fragment, and so are the readings after the latest one.
    bounded: bool,
    /// The readings from the first boundary to the latest one.
    whole_bytes: u64,
    whole_minors: u64,
    /// The readings since the latest boundary.
    open_bytes: u64,
    open_minors: u64,
}

impl CapPhaseCycles {
    const START: Self = CapPhaseCycles {
        all_died: true,
        readings: 0,
        last: 0,
        before_last: 0,
        bounded: false,
        whole_bytes: 0,
        whole_minors: 0,
        open_bytes: 0,
        open_minors: 0,
    };

    /// Add one reading. The reading after a peak (`before_last <= last >=
    /// reading`) closes the open cycle and opens the next one.
    fn note(mut self, reading: usize, cohort_died: bool) -> Self {
        self.all_died &= cohort_died;
        if self.readings == 2 && self.before_last <= self.last && self.last >= reading {
            if self.bounded {
                self.whole_bytes = self.whole_bytes.saturating_add(self.open_bytes);
                self.whole_minors += self.open_minors;
            }
            self.bounded = true;
            self.open_bytes = 0;
            self.open_minors = 0;
        }
        self.open_bytes = self.open_bytes.saturating_add(reading as u64);
        self.open_minors += 1;
        self.before_last = self.last;
        self.last = reading;
        self.readings = (self.readings + 1).min(2);
        self
    }

    /// The mean of the whole cycles, or `None` before one has closed.
    fn whole_mean(self) -> Option<usize> {
        (self.whole_minors != 0).then(|| (self.whole_bytes / self.whole_minors) as usize)
    }
}

/// Add one reading to the ladder's current phase. Returns the phase's level
/// and its mean influx: the level is the mean, unless every cohort of the
/// phase died, when it is the mean of the phase's whole cycles (`None` before
/// one has closed). A reading on the far side of the band from the mean so
/// far starts a new mean with itself (see [`retune_nursery_cap_scale`]).
fn note_cap_phase_reading(
    eden_live_bytes: usize,
    cap: usize,
    cohort_died: bool,
) -> (Option<usize>, usize) {
    let minors = CAP_PHASE_MINORS.with(Cell::get);
    let sum = CAP_PHASE_INFLUX_BYTES.with(Cell::get);
    let reading = eden_live_bytes as u64;
    let side = cap_band_side(eden_live_bytes, cap);
    let restart = minors != 0 && side * cap_band_side((sum / minors) as usize, cap) < 0;
    let (sum, minors) = if restart {
        (reading, 1)
    } else {
        (sum.saturating_add(reading), minors + 1)
    };
    CAP_PHASE_INFLUX_BYTES.with(|c| c.set(sum));
    CAP_PHASE_MINORS.with(|c| c.set(minors));
    let mean = (sum / minors) as usize;
    let cycles = CAP_PHASE_CYCLES
        .with(Cell::get)
        .note(eden_live_bytes, cohort_died);
    CAP_PHASE_CYCLES.with(|c| c.set(cycles));
    let level = if cycles.all_died {
        cycles.whole_mean()
    } else {
        Some(mean)
    };
    (level, mean)
}

/// A ladder move ends the phase: the next reading is the first at the new cap.
fn start_cap_phase() {
    CAP_PHASE_INFLUX_BYTES.with(|c| c.set(0));
    CAP_PHASE_MINORS.with(|c| c.set(0));
    CAP_PHASE_CYCLES.with(|c| c.set(CapPhaseCycles::START));
}

/// Minimum Eden survival rate, in tenths, for a mark-sweep to seed the
/// promote-on-first-copy lock: ≥90% of the Eden bytes the sweep classified
/// must have been live. That is the "the aging round would filter nothing"
/// proof, measured directly instead of inferred from a survivor round-trip.
const FULL_SEED_LIVE_TENTHS: usize = 9;

/// Would a completed mark-sweep's Eden census justify promote-on-first-copy?
///
/// Pure function of the census and the survivor target so the policy is
/// testable without arranging a heap (the #7024 shape: a green test whose
/// subject never ran — see the sibling `scavenge_nursery_cap_from`).
///
/// Two independent conditions, both required, each answering a different
/// question:
///
/// 1. **Occupancy** — `compute_target_survivals(...) == 1`, i.e. the surviving
///    cohort alone already exceeds the desired survivor occupancy, so at any
///    S ≥ 2 it cannot fit and would be re-copied. This is deliberately the
///    module's *existing* rule; the only new thing is where the number is read
///    from.
/// 2. **Survival rate** — nearly nothing in Eden died, so a survivor round
///    would filter nothing. Without this an Eden that merely happens to be
///    large would promote its garbage too.
///
/// The stated failure mode (design note on #7592): a normally churn-heavy
/// program whose nursery is atypically mostly-live at one sweep promotes one
/// Eden's worth of short-lived objects and pays an old-gen reclaim to get them
/// back. Exposure is bounded by one nursery cap and by the existing unlock
/// path; requiring BOTH conditions is what keeps it narrow.
pub(super) fn full_seed_promotes_on_first_copy(
    eden_live_bytes: usize,
    eden_dead_bytes: usize,
    desired_bytes: usize,
) -> bool {
    if compute_target_survivals(eden_live_bytes, desired_bytes) != 1 {
        return false;
    }
    let classified = eden_live_bytes.saturating_add(eden_dead_bytes);
    classified > 0
        && eden_live_bytes.saturating_mul(10) >= classified.saturating_mul(FULL_SEED_LIVE_TENTHS)
}

/// Feed one finished mark-sweep's Eden census into the loop. `eden_live_bytes`
/// and `eden_dead_bytes` are the bytes the sweep walk classified live and dead
/// in the general (Eden) blocks.
///
/// Callers must exclude budgeted cycles and cycles that ran the conservative
/// native-stack scan — see the module header for why those two inputs are not
/// sound liveness measurements.
pub(super) fn seed_promote_lock_from_sweep(eden_live_bytes: usize, eden_dead_bytes: usize) {
    let already_locked = PROMOTE_LOCK.with(Cell::get);
    let desired = desired_survivor_bytes();
    let seeds = full_seed_promotes_on_first_copy(eden_live_bytes, eden_dead_bytes, desired);
    // Diagnostic, not a knob: print the census AND the verdict on every
    // mark-sweep, including refusals. A policy that silently declines is
    // indistinguishable from one that never ran (#7024/#7025), and the
    // refusal reason is the number a future tuning decision needs.
    if crate::gc::gc_diag_enabled() {
        let classified = eden_live_bytes.saturating_add(eden_dead_bytes);
        let pct = if classified == 0 {
            0
        } else {
            eden_live_bytes * 100 / classified
        };
        eprintln!(
            "[gc-tenuring] sweep-seed eden_live_bytes={eden_live_bytes} eden_dead_bytes={eden_dead_bytes} live_pct={pct} desired={desired} seeds={seeds} already_locked={already_locked}"
        );
    }
    if already_locked || !seeds {
        return;
    }
    let current = TENURING_SURVIVALS.with(Cell::get);
    PROMOTE_LOCK.with(|l| l.set(true));
    UNLOCK_STREAK.with(|s| s.set(0));
    RAISE_STREAK.with(|s| s.set(0));
    // PREV_COPIED_BYTES is deliberately untouched: it is the survival-rate
    // lock's denominator, owned by the copying path.
    set_survivals(current, 1, eden_live_bytes, "sweep-seed");
}

/// Called from the collector's existing diagnostic block, with no extra
/// diagnostic check or state reads on the disabled path.
pub(super) fn report_nursery_policy(eden_live_bytes: usize) {
    eprintln!(
        "[gc-nursery-policy] next_scale={} next_cap={} influx={eden_live_bytes} mean={} grow_streak={} shrink_streak={} promote_locked={} next_survivals={}",
        NURSERY_CAP_SCALE.with(TriggerInput::get),
        scavenge_nursery_cap_effective_bytes(),
        MEAN_SURVIVING_OBJECT_BYTES.with(TriggerInput::get),
        CAP_GROW_STREAK.with(Cell::get),
        CAP_SHRINK_STREAK.with(Cell::get),
        PROMOTE_LOCK.with(Cell::get),
        TENURING_SURVIVALS.with(Cell::get),
    );
}

fn diag_cap_scale(from: u8, to: u8, eden_live_bytes: usize) {
    if crate::gc::gc_diag_enabled() {
        eprintln!(
            "[gc-tenuring] nursery cap scale {from}/{unit} -> {to}/{unit} of base \
             (band {} B, eden_live_bytes={eden_live_bytes})",
            influx_driven_nursery_cap_bytes_at(to),
            unit = NURSERY_CAP_SCALE_UNIT
        );
    }
}

fn diag_cap_cycle_hold(scale: u8, side: &str, level: Option<usize>, eden_live_bytes: usize) {
    if crate::gc::gc_diag_enabled() {
        let why = match level {
            None => "no whole cycle has closed".to_string(),
            Some(level) => format!("the whole cycles' level is {level} B"),
        };
        eprintln!(
            "[gc-tenuring] nursery cap scale {scale}/{unit} holds on the {side} side: {why} \
             (eden_live_bytes={eden_live_bytes})",
            unit = NURSERY_CAP_SCALE_UNIT
        );
    }
}

fn set_survivals(current: u8, next: u8, eden_live_bytes: usize, why: &str) {
    if next == current {
        return;
    }
    TENURING_SURVIVALS.with(|s| s.set(next));
    if crate::gc::gc_diag_enabled() {
        eprintln!(
            "[gc-tenuring] survivals {} -> {} ({why}, eden_live_bytes={} desired={})",
            current,
            next,
            eden_live_bytes,
            desired_survivor_bytes()
        );
    }
}

#[cfg(test)]
pub(super) fn reset_for_test() {
    TENURING_SURVIVALS.with(|s| s.set(OCCUPANCY_MIN_SURVIVALS));
    SURVIVOR_ROUND_MEASURED.with(|m| m.set(false));
    RAISE_STREAK.with(|s| s.set(0));
    PROMOTE_LOCK.with(|l| l.set(false));
    UNLOCK_STREAK.with(|s| s.set(0));
    PREV_COPIED_BYTES.with(|c| c.set(0));
    NURSERY_CAP_SCALE.with(|s| s.set(NURSERY_CAP_SCALE_MIN));
    CAP_GROW_STREAK.with(|s| s.set(0));
    CAP_SHRINK_STREAK.with(|s| s.set(0));
    start_cap_phase();
    MEAN_SURVIVING_OBJECT_BYTES.with(|s| s.set(NURSERY_CAP_REFERENCE_OBJECT_BYTES));
    OBJECT_CENSUS_SEEDED.with(|s| s.set(false));
}

/// Test-only view of the seed flag (#8122): has any census replaced the
/// calibration seed yet?
#[cfg(test)]
pub(super) fn object_census_seeded_for_test() -> bool {
    OBJECT_CENSUS_SEEDED.with(TriggerInput::get)
}

#[cfg(test)]
#[path = "tenuring/tests.rs"]
mod tests;
