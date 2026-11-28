//! Major (full-cycle) pacing: yield backoff, the retaining-heap multiplier,
//! arena-occupancy readings and the arena-growth escalation threshold.

use super::*;

/// The multiplier on the byte-pacing full's mature-growth term: 1, or
/// `MAJOR_PACING_LIVE_GROWTH_RETAINING_MULTIPLIER` while the last copying minor
/// measured a retaining heap. The next full is due once the mature heap has
/// grown by the last full's measured live bytes times this factor.
pub(in crate::gc) fn major_pacing_live_growth_multiplier() -> usize {
    if GC_MAJOR_PACING_RETAINING.with(|c| c.get()) {
        MAJOR_PACING_LIVE_GROWTH_RETAINING_MULTIPLIER
    } else {
        1
    }
}

/// Whether the last copying minor measured a retaining heap. Trace/test
/// observability — a gate that cannot see this cannot prove which arm paced a
/// given run.
#[cfg(any(perry_diagnostics, test))]
pub(in crate::gc) fn major_pacing_retaining() -> bool {
    GC_MAJOR_PACING_RETAINING.with(|c| c.get())
}

/// Old-gen bytes the mutator placed since the last full collection finished,
/// in the units of the post-full baseline [`finish_full_old_reclaim_baseline`]
/// records: block growth and swept-hole reuse both count, promotion a minor
/// proved live is credited to the baseline and does not. The idle reducer
/// reads it as the evidence that a follow-up full could find something new.
pub(in crate::gc) fn old_gen_growth_since_last_full_bytes() -> usize {
    old_gen_reclaimable_pressure_bytes()
        .saturating_add(external_side_old_reclaim_pressure_bytes())
        .saturating_sub(GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(TriggerInput::get))
}

/// Percent of the pre-full live set a full must reclaim to count as productive.
/// Below this the next arena-growth escalation is pushed out (see
/// `GC_MAJOR_PACING_BACKOFF_SHIFT`).
/// Deliberately low. A full that reclaims 20% of the heap is still doing real
/// work, and pushing its successor out would carry that garbage twice as long;
/// the shape this exists for reclaims single-digit percent (5.9% and 0.0% on
/// `retain.ts`). Raising it trades RSS for pause time.
pub(super) const MAJOR_PACING_PRODUCTIVE_YIELD_PCT: usize = 20;

/// Cap on the backoff shift: the escalation multiplier tops out at
/// `growth_num << 2`, i.e. 8× with the default `growth_num` of 2. Bounded so a
/// long run of low-yield fulls cannot disable arena-growth pacing outright.
const MAJOR_PACING_BACKOFF_SHIFT_MAX: u32 = 2;

/// Young-survival ratio (permille) at or above which the heap is treated as
/// RETAINING, i.e. growing by data that is alive rather than by garbage.
///
/// Measured on the GC-benchmark corpus, this separates by two orders of
/// magnitude rather than marginally — the two populations do not overlap:
///
/// | workload | young survival (permille) |
/// |---|---|
/// | `churn`, `churn_alloc`, `push_cls` | 0 – 4 |
/// | `cycles` | 0 |
/// | `shapes` | 713 – 920 |
/// | `retain`, `retain_wide`, `deeplist` | 999 – 1000 |
pub(super) const MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE: u64 = 900;

/// Extra growth allowed before escalating while the heap is RETAINING, on top
/// of `PERRY_GC_MAJOR_PACING_GROWTH` (so 8× with the default 2).
///
/// This is the survival-adaptive growing factor every generational collector
/// needs and this one lacked. `growth_num = 2` means "escalate when the arena
/// doubles". On a heap where **everything allocated stays alive**, doubling is
/// not evidence of garbage — it is the program working — so the fixed 2×
/// scheduled a full mark-sweep per doubling, each of which marked a bigger
/// all-live heap and freed almost nothing (`retain.ts` 11.9%, `retain_wide.ts`
/// 6.8% then 9.6%, `deeplist.ts` **0.0%**; against `tree.ts` 87.8% and
/// `tree_wide.ts` 92.3%, which run no minors at all and are therefore
/// untouched by this).
///
/// It is the prospective twin of `MAJOR_PACING_PRODUCTIVE_YIELD_PCT`'s
/// retrospective backoff, and it exists because retrospection cannot help a
/// monotonically growing live heap: every full costs O(live) and delaying one
/// only makes the next bigger, so the useless full has to be *predicted*, not
/// priced after the fact.
pub(super) const MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER: usize = 4;

/// The retaining multiplier of the byte-pacing full's live-derived mature-growth
/// term (OWNER 2026-10-29, lane GCT factor sweep: 2 keeps no pipeline gain, 4
/// costs the most string-build RSS; 3 keeps the whole pipeline gain).
const MAJOR_PACING_LIVE_GROWTH_RETAINING_MULTIPLIER: usize = 3;

/// Record what the just-finished full reclaimed and adjust the pacing backoff.
///
/// `pre` is the reclaimable old reading captured when the full cycle started
/// (`note_full_cycle_started`); a full that shrinks it by less than
/// `MAJOR_PACING_PRODUCTIVE_YIELD_PCT` shifts the next escalation threshold
/// left by one, a productive full resets the shift to 0. Deliberately measured
/// on the SAME metric the escalation gate reads (reclaimable old bytes) so the
/// two cannot disagree about whether a full helped.
pub(super) fn update_major_pacing_backoff(post_in_use: usize) {
    let pre_in_use = GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES.with(|bytes| bytes.get());
    if pre_in_use == 0 {
        // No start reading (a full driven from a path that does not announce
        // itself): leave the shift alone rather than guess a yield.
        return;
    }
    GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES.with(|bytes| bytes.set(0));
    let reclaimed = pre_in_use.saturating_sub(post_in_use);
    let productive =
        reclaimed.saturating_mul(100) / pre_in_use >= MAJOR_PACING_PRODUCTIVE_YIELD_PCT;
    GC_MAJOR_PACING_BACKOFF_SHIFT.with(|shift| {
        if productive {
            shift.set(0);
        } else {
            shift.set(
                shift
                    .get()
                    .saturating_add(1)
                    .min(MAJOR_PACING_BACKOFF_SHIFT_MAX),
            );
        }
    });
}

/// Announce the start of an ESCALATED full mark-sweep so
/// `update_major_pacing_backoff` can price what it reclaimed. Called only from
/// `arena_growth_full_escalation_due`, so an explicit `gc()` — whose yield says
/// nothing about arena-growth pacing, and whose repeated use would otherwise
/// drive the shift to its cap — never moves the backoff.
fn note_full_cycle_started() {
    GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES.with(|bytes| bytes.set(pacing_old_in_use_bytes()));
}

/// One old-generation basis for the escalation predicate, its post-full
/// baseline and its yield. Nursery occupancy and block capacity are not old
/// growth. Swept holes are already reusable and do not count as pressure.
pub(super) fn pacing_old_in_use_bytes() -> usize {
    #[cfg(test)]
    if let Some(bytes) = TEST_PACING_ARENA_IN_USE.with(|cell| cell.get()) {
        return bytes;
    }
    old_gen_reclaimable_pressure_bytes()
}

/// Whole-arena live census, used only by the arena right-sizer.
pub(in crate::gc) fn pacing_arena_in_use_bytes() -> usize {
    #[cfg(test)]
    if let Some(bytes) = TEST_PACING_ARENA_IN_USE.with(|cell| cell.get()) {
        return bytes;
    }
    crate::arena::arena_live_allocated_bytes()
}

/// Record post-collection old bytes for major escalation, and independently
/// feed the whole-arena live census to the existing idle right-sizer.
/// Called once at the end of every cycle, minor and full alike. The copying
/// fast path publishes directly; non-copying cycles publish from
/// `GcCycle::publish_reclaim_outcome` after their sweep census.
///
/// **Three post-collection quantities, three units — do not conflate them.**
/// This funnel is the one place a reader can see all three at once, which is
/// why the list lives here:
///
/// | cell | unit | read by |
/// |---|---|---|
/// | `GC_LAST_COLLECTION_POST_OLD_IN_USE_BYTES` | `pacing_old_in_use_bytes()` — reclaimable OLD object bytes, test-injectable | `arena_growth_full_escalation_due` |
/// | `GC_TINY_PARSE_PRESSURE_BASE_BYTES` (#9831) | `arena_in_use_bytes()` — BUMP OFFSETS, the same reading the guard takes at each parse boundary | `tiny_parse_pressure_due_with` |
/// | `GC_NEXT_TRIGGER_BYTES` (#9840) | `arena_total_bytes()` — COMMITTED bytes, which is what `next_arena_trigger_base()` is compared against | `gc_budgeted_due_trigger`'s `ArenaBytes` arm |
///
/// The third is *not* written here, and that is deliberate rather than an
/// omission: re-arming the arena trigger needs the collection's productivity
/// score (`outcome.freed_bytes` and `pre_in_use`, which price `GC_STEP_BYTES`)
/// and the once-consumed `take_promoted_young_capacity_credit()`, neither of
/// which exists at this point in a cycle — and it must skip fulls, which this
/// funnel deliberately does not. It is written by
/// [`gc_rebaseline_arena_trigger_after_collection`], which both nursery-
/// collection finishers call; #9840 is the change that made that "both"
/// true, and it is the same symmetry #9831 applied to the cell above.
/// Mixing the units — re-baselining a committed-bytes trigger from a bump-
/// offset or live-census base — would arm it below the arena's own total and
/// make the arm due the instant it re-armed.
pub(in crate::gc) fn note_collection_finished_arena_occupancy(full: bool) {
    let bytes = pacing_arena_in_use_bytes();
    GC_LAST_COLLECTION_POST_OLD_IN_USE_BYTES.with(|cell| cell.set(pacing_old_in_use_bytes()));
    // #9831: the same moment, in the units the tiny-parse guard reads.
    GC_TINY_PARSE_PRESSURE_BASE_BYTES.with(|cell| cell.set(crate::arena::arena_in_use_bytes()));
    // Medium-parse pacing (2026-09-14): and in the units the parse-boundary
    // side-allocation band reads. This is the site that makes the band self-correcting: whatever the sweep
    // and the from-space pass just released has already been subtracted from
    // `external_side_live_bytes`, so a collection that freed the tapes
    // re-bases at ~0 and one that could not re-bases at the surviving value.
    GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES.with(|cell| cell.set(external_side_live_bytes()));
    super::arena_right_size::note_collection_finished(bytes, full);
}

/// The old-generation reading [`arena_growth_full_escalation_due`] tests — see
/// [`GC_LAST_COLLECTION_POST_OLD_IN_USE_BYTES`].
///
/// Zero before any collection has finished, so the very first collection of a
/// process is never escalated: there is no evidence yet that a minor would
/// fail to reclaim, and the whole point of the pacing is to fire on that
/// evidence rather than on allocation volume.
///
/// Keeps the `#[cfg(test)]` seam in front, so the existing positive-direction
/// tests that force a `true` verdict out of the real predicate keep working
/// without a 32 MB live heap.
pub(in crate::gc) fn pacing_escalation_reading_bytes() -> usize {
    #[cfg(test)]
    if let Some(bytes) = TEST_PACING_ARENA_IN_USE.with(|cell| cell.get()) {
        return bytes;
    }
    GC_LAST_COLLECTION_POST_OLD_IN_USE_BYTES.with(|cell| cell.get())
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`pacing_arena_in_use_bytes`]. Thread-local, so
    /// concurrently-running tests cannot see each other's value.
    static TEST_PACING_ARENA_IN_USE: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Current arena-growth escalation backoff shift.
#[cfg(any(perry_diagnostics, test))]
pub(in crate::gc) fn major_pacing_backoff_shift() -> u32 {
    GC_MAJOR_PACING_BACKOFF_SHIFT.with(|shift| shift.get())
}

/// `(post-full old baseline bytes, backoff shift, reclaimable old bytes at or above
/// which the next minor escalates to a full)` — emitted in the GC trace so a
/// gate can prove the backoff actually engaged rather than merely that nothing
/// threw.
///
/// The third element is [`major_pacing_escalation_threshold_bytes`] verbatim,
/// i.e. the boundary `arena_growth_full_escalation_due` actually tests, **floor
/// included**. `None` means arena-growth pacing is disabled
/// (`PERRY_GC_MAJOR_PACING_FLOOR_MB=0`) and no reading escalates; a zero
/// baseline (no full yet) reports the floor, which is the reading that
/// escalates — not `0`, which is what it used to say.
// `test` as well as `diagnostics` (matching `major_pacing_backoff_shift`), so
// the test that pins snapshot-vs-predicate agreement still builds under
// `--no-default-features`, where the trace itself is compiled out.
#[cfg(any(perry_diagnostics, test))]
pub(in crate::gc) fn major_pacing_snapshot() -> (usize, u32, Option<usize>) {
    let baseline = GC_LAST_FULL_OLD_IN_USE_BYTES.with(|bytes| bytes.get());
    let shift = major_pacing_backoff_shift();
    (baseline, shift, major_pacing_escalation_threshold_bytes())
}

#[cfg(test)]
pub(in crate::gc) fn test_reset_major_pacing_backoff() {
    GC_MAJOR_PACING_BACKOFF_SHIFT.with(|shift| shift.set(0));
    GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES.with(|bytes| bytes.set(0));
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(false));
}

/// The pre-full old reading `arena_growth_full_escalation_due` recorded, or 0
/// if it declined to escalate. Non-zero is the proof that the escalation is
/// PRICED — without it the backoff cannot fire and the pacing silently reverts
/// to the unconditional K× rule.
#[cfg(test)]
pub(in crate::gc) fn test_major_pacing_pre_in_use_bytes() -> usize {
    GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES.with(|bytes| bytes.get())
}

/// Override the post-collection occupancy the escalation predicate reads.
/// Returns the previous value so a test can restore it.
#[cfg(test)]
pub(in crate::gc) fn test_set_collection_post_in_use_bytes(bytes: usize) -> usize {
    GC_LAST_COLLECTION_POST_OLD_IN_USE_BYTES.with(|cell| {
        let previous = cell.get();
        cell.set(bytes);
        previous
    })
}

#[cfg(test)]
pub(in crate::gc) fn test_set_major_pacing_baseline(bytes: usize) -> usize {
    GC_LAST_FULL_OLD_IN_USE_BYTES.with(|cell| {
        let previous = cell.get();
        cell.set(bytes);
        previous
    })
}

/// Override the arena reading arena-growth pacing sees, for the duration of a
/// test. `None` restores the real live-allocation reading. Returns the previous
/// value so a test can restore it rather than assume it was unset.
#[cfg(test)]
pub(in crate::gc) fn test_set_pacing_arena_in_use(bytes: Option<usize>) -> Option<usize> {
    TEST_PACING_ARENA_IN_USE.with(|cell| {
        let previous = cell.get();
        cell.set(bytes);
        previous
    })
}

#[cfg(test)]
pub(in crate::gc) fn test_note_full_cycle_reclaimed(pre_in_use: usize, post_in_use: usize) {
    GC_FULL_CYCLE_PRE_OLD_IN_USE_BYTES.with(|bytes| bytes.set(pre_in_use));
    update_major_pacing_backoff(post_in_use);
}

/// #6893-followup: major-GC pacing. A non-moving minor sweep cannot free
/// array-growth forwarding stubs (`Array.prototype.push` reallocations leave a
/// stub per growth), so churn that grows arrays accumulates stubs that pin every
/// arena block → unbounded RSS; only a FULL mark-sweep reclaims them. Escalate a
/// minor to a full once the arena's live bytes exceed K× the clean live set
/// measured after the last full. Gated by an absolute floor so small heaps never
/// pay for a full, and by the K× ratio so a workload with a legitimately large
/// *stable* live set (retain-style) does not over-escalate — its arena hovers
/// near its own baseline, well under K×.
/// `(floor_bytes, growth_num)` for major-GC pacing.
///
/// Parsed ONCE — this runs on the minor-GC path, so no per-call env lookup /
/// parse / String alloc. The env vars are for tuning and measurement (read at
/// process start); defaults chosen so churn oscillates ~baseline..2×baseline
/// and stays below node's peak.
pub(in crate::gc) fn major_pacing_config() -> (usize, usize) {
    use std::sync::OnceLock;
    static CONFIG: OnceLock<(usize, usize)> = OnceLock::new();
    let &(floor_bytes, growth_num) = crate::once_init::get_or_init(&CONFIG, || {
        const DEFAULT_FLOOR_MB: usize = 32;
        const DEFAULT_GROWTH_NUM: usize = 2;
        let floor_bytes = std::env::var("PERRY_GC_MAJOR_PACING_FLOOR_MB")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_FLOOR_MB)
            .saturating_mul(1024 * 1024);
        let growth_num = std::env::var("PERRY_GC_MAJOR_PACING_GROWTH")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|&n| n >= 1)
            .unwrap_or(DEFAULT_GROWTH_NUM);
        (floor_bytes, growth_num)
    });
    (floor_bytes, growth_num)
}

/// Every caller acts on a `true` by running a FULL immediately, so the pre-full
/// arena reading is recorded HERE rather than at the call sites — that is what
/// keeps the escalation and the pricing of its result from drifting apart.
///
/// The first cut of #7726 wired the two `gc_start_budgeted_cycle_for_pressure`
/// sites by hand and missed the one in `gc::gc_collect_minor_with_trigger_inner`
/// — which is the site the shipped safepoint path actually takes. The backoff
/// then never fired, and the whole change measured as a 30 ms no-op on
/// `retain.ts` while every test still passed. Recording inside the predicate
/// makes a future call site correct by construction.
pub(in crate::gc) fn arena_growth_full_escalation_due() -> bool {
    let due = arena_growth_full_escalation_due_inner();
    if due {
        note_full_cycle_started();
    }
    due
}

fn arena_growth_full_escalation_due_inner() -> bool {
    match major_pacing_escalation_threshold_bytes() {
        // `PERRY_GC_MAJOR_PACING_FLOOR_MB=0` disables the pacing: no reading
        // escalates, so there is no boundary to compare against.
        None => false,
        Some(threshold) => pacing_escalation_reading_bytes() >= threshold,
    }
}

/// The smallest live-allocation reading that escalates the next minor to
/// a full, or `None` when arena-growth pacing is disabled outright and no
/// reading escalates.
///
/// **This is the only definition of that boundary.**
/// `arena_growth_full_escalation_due_inner` is now literally
/// `in_use >= this`, and `major_pacing_snapshot` reports exactly this — so the
/// decision and the diagnostic cannot drift apart, rather than merely agreeing
/// today. #7733 added the snapshot so the pacing subject could be asserted live
/// in the GC trace, and it recomputed the boundary as `baseline × growth` with
/// the floor dropped on the ground (`let (_floor, growth_num) = …`). Wherever
/// the floor dominates — the entire pre-first-full phase, where the old
/// snapshot reported `0`, and any baseline below `floor / growth` — the trace
/// named a boundary the collector does not use. A probe that misreports the
/// quantity it exists to prove is this repo's most expensive recurring bug, so
/// the fix is one source of truth, not two that match.
fn major_pacing_escalation_threshold_bytes() -> Option<usize> {
    let (floor_bytes, growth_num) = major_pacing_config();
    let baseline = GC_LAST_FULL_OLD_IN_USE_BYTES.with(|bytes| bytes.get());
    // Yield-adaptive: a full that reclaimed almost nothing pushes the next
    // escalation out (`GC_MAJOR_PACING_BACKOFF_SHIFT`). Shift the multiplier,
    // not the baseline, so one productive full restores the original pacing.
    let shift = GC_MAJOR_PACING_BACKOFF_SHIFT.with(|shift| shift.get());
    // Survival-adaptive: fold the retaining multiplier into `growth_num` rather
    // than adding a parameter, so `major_pacing_escalation_threshold_for` stays
    // the single pure `(config, state) → boundary` the snapshot and the
    // predicate both read (#7733's divergence).
    let growth_num = if GC_MAJOR_PACING_RETAINING.with(|c| c.get()) {
        growth_num.saturating_mul(MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER)
    } else {
        growth_num
    };
    major_pacing_escalation_threshold_for(floor_bytes, growth_num, baseline, shift)
}

/// Pure `(config, state) → boundary`, factored out so the floor/growth
/// interaction is unit-testable without a 32 MB live heap (which is what left
/// the original divergence untested: every reachable unit test sat below the
/// floor, where the two formulas' disagreement is invisible to a `bool`).
///
/// `None` means "no arena reading escalates" — either pacing is off, or the
/// growth term overflowed `usize`, which is the same statement about the world.
pub(in crate::gc) fn major_pacing_escalation_threshold_for(
    floor_bytes: usize,
    growth_num: usize,
    baseline: usize,
    shift: u32,
) -> Option<usize> {
    if floor_bytes == 0 {
        return None; // PERRY_GC_MAJOR_PACING_FLOOR_MB=0 disables the pacing
    }
    // No full yet (baseline 0): bound the initial growth once we clear the
    // floor, so the floor alone is the boundary.
    if baseline == 0 {
        return Some(floor_bytes);
    }
    let growth = growth_num.saturating_mul(1usize << shift);
    // `+1` because the growth clause is a strict `>` while the floor clause is
    // a `>=`; taking the max of the two is what the snapshot used to omit.
    // `checked_*` rather than `saturating_*`: a growth term that does not fit
    // in a `usize` is a boundary no arena reading can reach, which is exactly
    // what `None` says — saturating would report `usize::MAX` and then claim an
    // arena of `usize::MAX` escalates, which the `>` clause never would.
    let growth_boundary = baseline.checked_mul(growth)?.checked_add(1)?;
    Some(floor_bytes.max(growth_boundary))
}
