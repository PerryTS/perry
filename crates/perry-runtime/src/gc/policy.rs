use super::heap_budget::*;
use super::*;

pub(super) const GC_FLAG_IN_ALLOC: u8 = 0b01;
/// Bit 1 of GC_FLAGS — suppression flag (JSON.parse).
pub(super) const GC_FLAG_SUPPRESSED: u8 = 0b10;

thread_local! {
    pub(super) static GC_FLAGS: Cell<u8> = const { Cell::new(0) };
}

/// Threshold: run GC when total arena bytes exceed this.
///
/// Current app-pattern tuning: 128 MB. The earlier 64 MB setting reduced
/// peak RSS on JSON round-trip style workloads, but it also forced a
/// collection in `buffer_transcode` while the benchmark still held a large
/// live set of rows/strings/buffers. That collection could not reclaim enough
/// and pushed the benchmark past the 30s smoke timeout. Returning the initial
/// trigger to 128 MB keeps allocation-heavy transcode and ECS-style bursts out
/// of mid-run GC while JSON parse/stringify remain below the 1.5x Bun gap in
/// the app-pattern matrix. The absolute ceiling below still bounds later
/// adaptive trigger growth at 128 MB after collections have started.
pub(super) const GC_THRESHOLD_INITIAL_BYTES: usize = 128 * 1024 * 1024; // 128 MB
/// Sanity bound on the adaptive step itself. Step growth past 1 GB is
/// only theoretically possible on multi-day services where GC fires
/// rarely; we keep the cap loose here since the *real* peak-RSS
/// guardrail is `GC_TRIGGER_ABSOLUTE_CEILING` below.
pub(super) const GC_THRESHOLD_MAX_BYTES: usize = 1024 * 1024 * 1024; // 1 GB

/// Hard ceiling on the next-GC trigger (arena_total bytes), independent
/// of how productive recent sweeps have been. Without this, the
/// >90%-freed branch doubles the step on every productive collection,
/// > and `next_trigger = new_total + step` lets peak nursery occupancy
/// > grow unboundedly even when most of what we collected was garbage.
/// > On `bench_json_roundtrip` direct (50 iters × ~5 MB / iter, GC fires
/// > 3 times), the step doubled from 64 MB → 67 MB → 134 MB and the
/// > trigger followed it, so peak nursery hit 115 MB at GC #3 — the
/// > dealloc pass from C4b-δ then returned 91 MB to the OS, but the
/// > peak-RSS damage was already done. Capping the trigger at the
/// > initial threshold prevents that runaway: after GC, trigger ≤ 128 MB
/// > regardless of how much step adapted, so peak nursery stays bounded
/// > to roughly initial + one iter's allocation buffer + headroom for
/// > non-arena overhead.
///
/// Floor: even if `arena_total` is already near or past the ceiling
/// (large old-gen + longlived combined live set), keep at least the
/// 16 MB step floor as headroom — `next_trigger = max(new_total + 16 MB,
/// min(new_total + step, ceiling))`. This avoids GC thrash when the
/// non-nursery component of arena_total alone exceeds the ceiling.
///
/// 2026-05-02 raise from 64 MB → 128 MB: ECS perf-comprehensive's
/// allocation-heavy benches (10k two-comp + sync, 5k × 3 cmds) hit
/// the 64 MB cap mid-round, then the >25%-freed branch halved the
/// step to 16 MB, so the next trigger landed ~16 MB above the post-
/// GC working set — well within a single round's allocation budget.
/// Result: 1-2 mid-round GCs per bench, the worst of which spent
/// 60 ms inside `mark_block_persisting_arena_objects` force-marking
/// + tracing 40 k newly-allocated objects in the recent window.
/// Doubling the cap lets productive sweeps accumulate full
/// `step` headroom (up to 128 MB) before the next trigger, which
/// shifts those GC events out of the measured rounds entirely.
/// `bench_json_roundtrip`-class workloads still bounded — they
/// finish under 128 MB peak and fire ≤2 GCs total.
///
/// Workloads unaffected: `07_object_create` / `12_binary_trees` /
/// `bench_gc_pressure` all fit their working sets under 64 MB and
/// fire GC at most once. The cap only changes behavior when the step
/// would otherwise have pushed the trigger past the initial threshold,
/// which is exactly the bench-RSS scenario this is targeting.
pub(super) const GC_TRIGGER_ABSOLUTE_CEILING: usize = 128 * 1024 * 1024;

// Device-derived heap budget: see gc/heap_budget.rs (split out for the
// 2000-line file lint).

/// The arena-bytes trigger as the collector should compare it: the raw
/// cell while armed (explicit re-arms/bumps may legitimately exceed the
/// ceiling — headroom floor over a big live set, medium-parse bumps), the
/// device-derived ceiling while the cell still holds its desktop-default
/// const initializer.
/// The adaptive/armed arena trigger WITHOUT the scavenge nursery cap:
/// compared against `arena_total_bytes()` (all generations), as it always
/// was. The cap is deliberately not part of this value — it is
/// young-generation-scoped and lives in [`young_scavenge_cap_due`].
pub(super) fn next_arena_trigger_base() -> usize {
    if GC_TRIGGER_ARMED.with(|a| a.get()) {
        GC_NEXT_TRIGGER_BYTES.with(|c| c.get())
    } else {
        GC_NEXT_TRIGGER_BYTES
            .with(|c| c.get())
            .min(gc_trigger_absolute_ceiling_bytes())
    }
}

/// True when the young generation (Eden + active survivor space) has
/// reached the effective scavenge nursery cap.
///
/// The basis is young-gen occupancy, NOT `arena_total_bytes()`. Comparing
/// the cap against the total put every old-gen byte on the young budget:
/// once old-gen in-use crossed 16 MB, every fresh 1 MB Eden block
/// re-crossed the trigger, degenerating the scavenge cadence to
/// once-per-block on any program with a large tenured set. That cadence is
/// the actual mechanism behind the tree.ts survivor-saturation regression
/// — objects were scavenged ~1 MB of allocation after birth, so almost
/// nothing had time to die: measured 1.05 MB of survivors per 1 MB block
/// (near-zero infant mortality), a saturated survivor space, and 1427
/// collections for a run that allocates ~1.4 GB.
pub(super) fn young_scavenge_cap_due() -> bool {
    if !nursery_cap_active() {
        return false;
    }
    crate::arena::copying_from_space_in_use_bytes()
        >= super::tenuring::scavenge_nursery_cap_effective_bytes()
}

/// Is the scavenge nursery cap in force?
///
/// **Only when the collection it schedules can EVACUATE**, which for nursery
/// pressure means only when `gc_moving_loop_polls_enabled()` routes it to a
/// precise-root safepoint. #7056's own 2x2 says the cap and the evacuating
/// minor "ship together, because either alone is a bad trade"; this is that
/// sentence made load-bearing rather than advisory, and #7682 is the bill for
/// its being advisory.
///
/// The cap's basis is `copying_from_space_in_use_bytes()`, and **a non-moving
/// minor does not reduce it** — it sweeps in place into per-block free lists
/// and from-space stays occupied. So a capped trigger that fires a non-moving
/// minor is due again the instant the next block is taken: one whole-arena
/// collection per 1 MB allocated, O(n^2) in the live set. That is not the
/// "+23% wall for -33% RSS" the cap-only cell of the 2x2 measured (every
/// collection there still evacuated) — measured on the quiet host after #7682
/// forced the alloc-point minor non-moving, `test_gap_gc_index_get_receiver_rooting`
/// went 0.66 s -> 6.6 s, and with the cap lifted it runs in 0.13 s. It is the
/// same livelock shape as #7592, whose fix was likewise to key a band on
/// something a collection actually moves.
///
/// So this restores the pre-#7056 gating, deliberately and with a different
/// argument than #7056 removed it under. #7056 decoupled the cap because both
/// gates were off in shipped builds and the cap was therefore dead — a fair
/// reading of a world in which the alloc-point minor evacuated. It no longer
/// does. When `PERRY_GC_MOVING_LOOP_POLLS` goes default-ON again the cap comes
/// back with it, automatically and in the configuration it was measured in.
fn nursery_cap_active() -> bool {
    #[cfg(test)]
    if GC_NURSERY_CAP_TEST_SUPPRESSED.with(Cell::get) {
        return false;
    }
    gc_moving_loop_polls_enabled()
}

pub(super) fn effective_next_arena_trigger() -> usize {
    let base = next_arena_trigger_base();
    // A minor is O(live) — it copies ~1k live objects out of millions
    // allocated — so the 128 MB-and-doubling adaptive trigger, tuned for the
    // OLD world where a minor was an expensive O(heap) sweep and the advice was
    // "collect rarely", is exactly backwards. Capping the nursery small makes
    // collections fire often and keeps the young arena's high-water mark near
    // the cap instead of ballooning to 128–260 MB between the ~8 collections
    // the adaptive trigger otherwise allows.
    //
    // APPLIED WHEN THE COLLECTION IT SCHEDULES CAN EVACUATE — see
    // [`nursery_cap_active`], which is where that condition and its evidence
    // live. #7056 applied it unconditionally on the reading that the
    // alloc-point minor evacuated; since #7682 it does not, and a capped
    // trigger firing a non-moving minor is a livelock rather than a trade.
    //
    // Re-derived on the statepoint-default collector, 8 gc_ratchet probes,
    // as a full 2x2 rather than a single comparison — because the one-armed
    // version of this measurement says something false:
    //
    //                    no scavenge        scavenge
    //     no cap        799,604,736      799,604,736   (+0%)
    //     cap 16 MB     537,165,824      245,006,336
    //                        (-33%)           (-69%)
    //
    // Read the row and the column, not one cell. Scavenge ON ITS OWN buys
    // exactly nothing — the top row is identical to the byte — which is why an
    // isolation that only varies the cap *within* the scavenge-on world
    // concludes "the cap is the whole effect". It is not. The two INTERACT: the
    // cap makes collections fire often, and scavenge makes those collections
    // evacuating (O(live) copying) so the nursery is actually reclaimed rather
    // than merely swept.
    //
    // Both halves ship together, because either alone is a bad trade: the cap
    // alone costs +23% wall for -33% RSS, and scavenge alone moves nothing.
    // See `gc::gc_scavenge_enabled` for the full 2x2 and why they interact.
    //
    // Wall time was flat in aggregate across the same probes (2052 ms ->
    // 2032 ms) and every probe stayed byte-identical to the pinned Node
    // oracle.
    //
    // `PERRY_GC_SCAVENGE_NURSERY_MB` still tunes the value; it is a
    // measurement dial, not an on/off mode, so it needs no kill-policy arm.
    if !nursery_cap_active() {
        return base;
    }
    // The cap the clamp applies is the *effective* one: the configured base
    // times the influx-driven scale from gc/tenuring.rs, which grows it
    // (bounded, ×2 steps) on live-set-bound workloads where a fixed 16 MB
    // multiplies the per-collection fixed cost by an enormous collection
    // count. Small-live-set workloads never leave the base value.
    base.min(super::tenuring::scavenge_nursery_cap_effective_bytes())
}

/// Nursery high-water cap used only when `PERRY_GC_SCAVENGE` is on (default
/// 16 MB; override with `PERRY_GC_SCAVENGE_NURSERY_MB`). See
/// `effective_next_arena_trigger`.
pub(super) fn gc_scavenge_nursery_cap_bytes() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("PERRY_GC_SCAVENGE_NURSERY_MB")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|&mb| mb > 0)
            .unwrap_or(16)
            .saturating_mul(1024 * 1024)
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only suppression of the nursery cap, so `force_legacy_gc_pacing`
    /// can restore genuinely legacy pacing.
    ///
    /// The cap used to hang off `gc_moving_loop_polls_enabled()`, so pinning
    /// that flag off was enough to un-cap the trigger. It is unconditional now
    /// (#7056), which silently broke that escape hatch: 22 `gc::tests` that
    /// legitimately assert raw-cell trigger arithmetic started failing against
    /// the capped value. The guard has to suppress the cap directly.
    static GC_NURSERY_CAP_TEST_SUPPRESSED: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    /// Lower bound for the next GC trigger. Bumped after each
    /// `gc_collect_inner` based on collection effectiveness (see the
    /// adaptive logic in `gc_check_trigger`).
    ///
    /// The initial value is `GC_THRESHOLD_INITIAL_BYTES` (128MB —
    /// chosen so that the 96MB working set of a 1M-iter object_create
    /// or binary_trees benchmark fits under the threshold and pays
    /// zero GC cost). After every collection, if the sweep freed >75%
    /// of arena bytes, the per-program "step" is doubled (capped at
    /// 1GB) so subsequent allocation bursts don't pay GC overhead just
    /// because they re-cross the same line. For hot `new ClassName()`
    /// loops where every object dies between GC cycles, this means
    /// the FIRST burst pays for at most one collection and the rest
    /// run GC-free.
    ///
    /// If a sweep frees <25%, the step is halved (down to a 16MB
    /// floor) so live-set-bound programs don't grow their working
    /// set unboundedly between collections.
    pub(super) static GC_NEXT_TRIGGER_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Whether GC_NEXT_TRIGGER_BYTES has been explicitly set on this thread
    /// (re-arm after a collection, parse bump, tiny-parse lowering). While
    /// false the cell still holds the desktop-default const initializer and
    /// `effective_next_arena_trigger` substitutes the device-derived ceiling
    /// instead — an ARMED trigger above the ceiling is legitimate (big live
    /// set headroom floor, medium-parse bumps) and must not be clamped.
    pub(super) static GC_TRIGGER_ARMED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// Per-program adaptive GC step. Doubles (up to MAX) when sweeps
    /// are mostly-garbage; halves (down to 16MB) when sweeps reclaim
    /// little. Used to compute the next trigger after each GC as
    /// `post_total + step`.
    pub(super) static GC_STEP_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Lower bound for the next malloc-count-based GC trigger. After each
    /// collection, this is reset to `survivor_count + GC_MALLOC_COUNT_STEP`
    /// so that programs with large legitimate live sets (>10k tracked
    /// malloc objects) don't GC-thrash on every subsequent allocation.
    /// See `gc_check_trigger` for the update rule.
    pub(super) static GC_NEXT_MALLOC_TRIGGER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(100_000) };

    /// Issue #745: track whether a medium-or-larger parse already
    /// raised `GC_NEXT_TRIGGER_BYTES` this GC cycle. Cleared in
    /// `gc_collect_inner` whenever a real collection runs.
    pub(super) static GC_TRIGGER_BUMPED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// Issue #745: snapshot of `arena_total_bytes()` at the most
    /// recent `gc_suppress` call. Used by `gc_bump_malloc_trigger`
    /// to compute the suppressed window's arena growth.
    pub(super) static GC_PRE_SUPPRESS_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };

    /// Non-generational full GC cannot compact a block that still contains
    /// the just-returned parse result. When tiny parse churn crosses the
    /// in-use pressure guard, collect at the next parse boundary instead of
    /// immediately after the current parse, so the previous result has had a
    /// chance to fall out of the shadow roots.
    pub(super) static GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

pub(super) const GC_SUPPRESSED_TINY_PARSE_BYTES: usize = 1024 * 1024;
pub(super) const GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES: usize = 48 * 1024 * 1024;
pub(super) const GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES: usize = 24 * 1024 * 1024;

pub(super) fn gc_suppressed_parse_is_tiny(parse_growth: usize) -> bool {
    parse_growth <= GC_SUPPRESSED_TINY_PARSE_BYTES
}

pub(super) fn gc_bump_arena_trigger_target(
    bytes_now: usize,
    step: usize,
    is_tiny_parse: bool,
) -> usize {
    let bytes_step = step.min(gc_trigger_absolute_ceiling_bytes());
    let target = bytes_now.saturating_add(bytes_step);
    if is_tiny_parse {
        target.min(gc_trigger_absolute_ceiling_bytes())
    } else {
        target
    }
}

/// Initial step for the malloc-count-based GC trigger. Adaptive: doubles
/// when >75% of malloc objects are garbage (loop-scoped temporaries),
/// halves when <25% are garbage (large live set). Capped at
/// `GC_MALLOC_COUNT_STEP_MAX` to bound memory between collections.
///
/// Originally a single hardcoded threshold (`GC_MALLOC_COUNT_THRESHOLD`);
/// issue #34 showed that triggering GC from `gc_malloc` (needed for
/// malloc-heavy workloads that don't push arena blocks — e.g.
/// @perry/postgres's `parseBigIntDecimal` bigint chain) combined with a
/// hardcoded threshold would thrash for any program whose live set
/// exceeded the threshold. Making it a per-cycle step fixes that.
///
/// Issue #58: the constant 10k step caused ~100 GC cycles for 500k-iter
/// string-concat loops where almost every object is dead. Adaptive
/// doubling ramps the step to 160k+ after a few mostly-garbage sweeps,
/// cutting GC cycles from ~100 to ~10.
pub(super) const GC_MALLOC_COUNT_STEP_INITIAL: usize = 100_000;
pub(super) const GC_MALLOC_COUNT_STEP_MAX: usize = 2_000_000;
pub(super) const GC_MALLOC_COUNT_STEP_MIN: usize = 10_000;

thread_local! {
    /// Per-program adaptive malloc-count step. Mirrors `GC_STEP_BYTES`
    /// behaviour: doubles when mostly-garbage, halves when mostly-live.
    pub(super) static GC_MALLOC_COUNT_STEP: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_MALLOC_COUNT_STEP_INITIAL) };
}

/// #6010: external side-buffer GC pressure. Map entry arrays and Set element
/// arrays are raw `std::alloc` allocations reachable only through a tiny
/// arena header — invisible to every trigger input (arena bytes, malloc
/// object count, old-gen bytes). A workload that churns large Maps/Sets
/// without arena pressure therefore never collected, and once a dead
/// header was conservatively pinned across two cycles it tenured into the
/// old generation, whose reclaim pressure counted only its 16 header bytes
/// — the multi-megabyte buffer leaked for the life of the process (issue
/// #6010: 1.4 GB RSS across a benchmark suite whose live heap never left
/// ~20 MB).
///
/// Two counters close the hole:
/// - ALLOC CHURN (`GC_EXTERNAL_SIDE_ALLOC_PENDING`): every
///   `GC_EXTERNAL_SIDE_ALLOC_STEP` bytes of fresh external allocation pokes
///   `gc_check_trigger()`, so collections happen at all in arena-quiet
///   Map/Set workloads.
/// - LIVE BYTES (`GC_EXTERNAL_SIDE_LIVE_BYTES`): maintained by the
///   alloc/realloc sites and the GC finalizers, and added to `old_in_use`
///   wherever old-reclaim pressure is computed — so tenured-then-dead
///   collections escalate to the full mark-sweep (whose old-gen sweep runs
///   the Map/Set side-allocation finalizers) instead of leaking.
const GC_EXTERNAL_SIDE_ALLOC_STEP: usize = 16 * 1024 * 1024;

thread_local! {
    static GC_EXTERNAL_SIDE_ALLOC_PENDING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static GC_EXTERNAL_SIDE_LIVE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Live bytes currently held by external Map/Set side buffers on this thread.
#[inline]
pub(super) fn external_side_live_bytes() -> usize {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(Cell::get)
}

/// Record `bytes` of fresh external side-buffer allocation (Map entries /
/// Set elements — creation or growth delta) and poke the trigger check when
/// the accumulated churn window fills. Callers must invoke this only when
/// the owning collection header is in a consistent state: a triggered cycle
/// scans conservatively at this call point (`gc_check_trigger`'s direct
/// arms use `force_full_scan`), which also keeps it non-moving, so raw
/// header pointers held by the caller stay valid across the call.
pub(crate) fn gc_note_external_side_alloc(bytes: usize) {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(|c| c.set(c.get().saturating_add(bytes)));
    let due = GC_EXTERNAL_SIDE_ALLOC_PENDING.with(|c| {
        let now = c.get().saturating_add(bytes);
        if now >= GC_EXTERNAL_SIDE_ALLOC_STEP {
            c.set(0);
            true
        } else {
            c.set(now);
            false
        }
    });
    if due {
        gc_check_trigger();
    }
}

/// Record that a Map/Set side buffer of `bytes` was freed (GC finalizer).
pub(crate) fn gc_note_external_side_free(bytes: usize) {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(|c| c.set(c.get().saturating_sub(bytes)));
}

#[inline]
/// The moving (copying) minor at the precise-root event-loop safepoint —
/// **Perry's default GC.** At the outermost microtask-pump boundary the JS stack
/// has unwound, so the copying minor runs with precise, rewritable roots and
/// moves survivors (compacting, O(survivors), no sweep). `PERRY_GC_MOVING_SAFEPOINT=0`
/// is a kill switch that reverts to the non-moving path (for bisecting a
/// regression while we harden). Making the moving minor primary INSIDE loops
/// (back-edge polls + alloc-point deferral) is the separate opt-in
/// `PERRY_GC_MOVING_LOOP_POLLS` — off by default because the poll defeats loop
/// vectorization until it is emitted only for allocating loops.
pub(crate) fn gc_moving_safepoint_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    // Default ON; the kill switch is an explicit `=0`/`off`/`false`.
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_GC_MOVING_SAFEPOINT").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Phase 4 of the moving-GC project: gate the INCREMENTAL old-gen collector (the
/// budgeted stepper). **DEFAULT ON since the #6180 flip**; `PERRY_GC_INCREMENTAL=0`
/// (or `off`/`false`) is the kill switch. Perry has a full budgeted
/// mark/sweep stepper which, before that flip, never ran: every compiled program
/// registers unbudgeted mutable root scanners and
/// `registered_root_scanners_block_budgeted_gc()` blocked the cycle from ever
/// starting. When this is on, the stepper is allowed to start and runs those
/// unbudgeted scanners SYNCHRONOUSLY in its initial root-scan step (a bounded
/// initial-mark pause), then marks/sweeps the old gen incrementally across
/// safepoints — the standard "initial-mark + incremental-mark" design. Off ⇒
/// exactly the non-incremental GC (the whole path is skipped). Independent
/// of `PERRY_GC_MOVING_SAFEPOINT`; this is the concurrency layer that reduces
/// old-gen pause time.
///
/// ★ This default is load-bearing for rooting arguments, not just for pause
/// times: with the stepper on, budgeted cycles skip the conservative
/// stack-scan subphase *structurally* (`gc/cycle.rs`, classifier mode), so a
/// compiled program completes precise-roots-only cycles in its shipped
/// configuration. Reasoning that assumes "the automatic arms force a
/// conservative scan" is therefore wrong by default — that mistake was made in
/// #6972 while this doc comment still claimed the gate was off (#6987).
pub(crate) fn gc_incremental_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        // DEFAULT ON (#6180 flip): ordinary allocation pressure is collected
        // by the budgeted incremental stepper — debt-paced assists, sound
        // across mutator windows (mark barrier + allocate-black + final
        // remark + drain-before-synchronous), census-free, RSS-parity on
        // realistic workloads with ~5x lower worst pause. Bounded pauses by
        // default; the synchronous collector remains for manual gc(),
        // emergency reclaim, and as the PERRY_GC_INCREMENTAL=0 escape hatch
        // (bisection / max-throughput batch workloads).
        !matches!(
            std::env::var("PERRY_GC_INCREMENTAL").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Make the moving minor PRIMARY inside loops: defer the alloc-point nursery
/// collection to a codegen loop back-edge poll (`js_gc_loop_safepoint`) instead
/// of collecting non-moving mid-expression, so reallocation-heavy loops evacuate
/// (bounded RSS) instead of leaking.
///
/// **DEFAULT ON.** The kill switch is `PERRY_GC_MOVING_LOOP_POLLS=0`/`off`/`false`.
/// See [`moving_loop_polls_enabled_from_env`] for the decision and its evidence;
/// #7161's stopgap default-OFF (pending #7154) is discharged there.
///
/// MUST match codegen `moving_safepoint_polls_enabled` (same env) so the deferral
/// and the polls that drain it stay coherent — a runtime default that disagrees
/// with the codegen default would defer collections that never drain (or drain
/// collections that were never deferred). That disagreement is not hypothetical:
/// it shipped. #7690 wrote the default-ON argument into the doc below and left
/// both bodies matching `1|on|true`, so the runtime deferred nursery pressure to
/// a safepoint codegen never emitted. Combined with #7687 (the alloc-point minor
/// must not move), the shipped collector had NO nursery evacuation at all —
/// `churn_alloc` ran 13 whole-arena full collections where it had run 105 copying
/// minors, and `tree` spent 4.1 s of its 5.1 s wall in GC pause. Both predicates
/// are now pinned by tests, and `polls_default_matches_codegen_mirror` pins that
/// they agree.
pub(crate) fn gc_moving_loop_polls_enabled() -> bool {
    // Test-only mode override (see `force_legacy_gc_pacing`). Consulted BEFORE
    // the process-wide OnceLock so a single test can pin a specific pacing mode
    // for its duration even though the process default is off. Compiled out
    // entirely in release builds.
    #[cfg(test)]
    if let Some(forced) = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(Cell::get) {
        return forced;
    }

    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        moving_loop_polls_enabled_from_env(
            std::env::var("PERRY_GC_MOVING_LOOP_POLLS").ok().as_deref(),
        )
    })
}

/// Pure env→enable decision for the moving-loop minor, factored out so the
/// default is unit-testable without touching process env / the cached `OnceLock`.
///
/// **Default ON since #7682.** The kill switch is `0`/`off`/`false`; anything
/// else, including unset, selects the moving-loop path. Codegen's
/// `moving_safepoint_polls_enabled` mirrors this exactly (same env, same
/// predicate) — they MUST agree, or a deferred collection has no drain.
///
/// #7161 flipped this OFF as a stopgap, and named both conditions for putting
/// it back. Both are met:
///
///  * **Its correctness reason is closed.** #7161's own title is "pending
///    #7154"; #7154 closed on 2026-08-01. The class it belongs to now has a
///    static gate (`gc-root-dominance.yml` over
///    `scripts/gc_root_dominance_corpus.sh`) whose allowlist is EMPTY, so a new
///    instance is a red build rather than a field report.
///  * **Its codegen-quality reason is discharged.** The other half of the
///    stopgap was that a poll at every back-edge defeats auto-vectorization;
///    `emit_gc_loop_safepoint` now emits one only where
///    `loop_purity::loop_may_allocate` says the body can allocate, so
///    numeric/vectorizable loops stay call-free. A loop that cannot allocate
///    cannot arm a trigger, so skipping it there is not a coverage hole.
///
/// And leaving it off had become the more dangerous state, which is the actual
/// reason this moves now. Nursery pressure has exactly two precise collection
/// points — this poll and the outermost microtask-pump boundary — and a
/// compute-only program reaches neither with polls off. Every nursery
/// collection therefore happened at the register-imprecise allocation point,
/// where #7682 showed it must not move. So "polls off" does not mean "collect
/// later, precisely"; it means "never collect precisely at all".
///
/// #7690 wrote every paragraph above and then did not change this line, and
/// nothing failed — the function was factored out expressly to make the default
/// "unit-testable without touching process env", and no test ever pinned it in
/// either direction. `polls_default_is_on` and its codegen mirror exist so that
/// the next edit to this predicate has to be deliberate.
pub(crate) fn moving_loop_polls_enabled_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some("0") | Some("off") | Some("false"))
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`gc_moving_loop_polls_enabled`]. When `Some(v)`,
    /// the getter returns `v` before consulting the process-wide OnceLock. This
    /// is the ONLY way a unit test can select GC pacing mode per-test: the
    /// OnceLock caches the env-derived default once for the whole process, so
    /// the entire test binary otherwise runs in a single mode. Because the
    /// nursery-cap in `effective_next_arena_trigger`, the alloc-point routing in
    /// `gc_check_trigger`, and the eager malloc-registry build in
    /// `CopyingPointerSet::new` all consult `gc_moving_loop_polls_enabled()`
    /// (and `gc_scavenge_enabled()` is env-gated OFF by default in tests), this
    /// single override flips all of the moving-mode behavior coherently.
    static GC_MOVING_LOOP_POLLS_TEST_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// RAII guard that pins LEGACY (non-moving, budgeted/direct, 128 MiB-ceiling) GC
/// pacing for the tests that assert the budgeted/direct pacer + trigger
/// arithmetic — the mechanism that, since the moving-nursery default-on flip,
/// lives behind the `PERRY_GC_MOVING_LOOP_POLLS=0` kill switch rather than the
/// default path. Restores the previous override state on drop. Test-only.
#[cfg(test)]
pub(super) struct LegacyGcPacingGuard {
    previous: Option<bool>,
    cap_previous: bool,
    scavenge_previous: Option<bool>,
}

#[cfg(test)]
impl Drop for LegacyGcPacingGuard {
    fn drop(&mut self) {
        GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.set(self.previous));
        GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.set(self.cap_previous));
        super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.set(self.scavenge_previous));
    }
}

/// Pin moving-loop polls ON for the duration of the returned guard, so a test
/// can drive `js_gc_loop_safepoint` without depending on the process-wide
/// default (which the `OnceLock` fixes from the environment once per test
/// binary — exactly the ambient dependency #7728's own regression hid behind).
#[cfg(test)]
pub(super) struct MovingLoopPollsGuard(Option<bool>);

#[cfg(test)]
impl MovingLoopPollsGuard {
    pub(super) fn on() -> Self {
        Self(GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.replace(Some(true))))
    }
}

#[cfg(test)]
impl Drop for MovingLoopPollsGuard {
    fn drop(&mut self) {
        GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

/// Pin legacy GC pacing (moving-loop polls OFF) for the duration of the returned
/// guard. See [`LegacyGcPacingGuard`] and [`gc_moving_loop_polls_enabled`].
#[cfg(test)]
pub(super) fn force_legacy_gc_pacing() -> LegacyGcPacingGuard {
    let previous = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| {
        let previous = cell.get();
        cell.set(Some(false));
        previous
    });
    // Legacy pacing now means THREE things, not one. Pinning the polls flag
    // used to be sufficient because both the nursery cap and the deferral
    // branch hung off it; #7056 made the cap unconditional and scavenge
    // default-on, so a guard that only touched the polls flag silently stopped
    // pinning anything. That is what broke 23 gc:: tests — they were correct,
    // and their guard had quietly become a no-op.
    let cap_previous = GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.replace(true));
    let scavenge_previous = super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.replace(Some(false)));
    LegacyGcPacingGuard {
        previous,
        cap_previous,
        scavenge_previous,
    }
}

/// Pin moving GC pacing (moving-loop polls ON) for the duration of the returned
/// guard — the shipped default, pinned explicitly so a #7148 deferral test
/// asserts against a *declared* pacing mode instead of inheriting whatever the
/// process-wide `PERRY_GC_MOVING_LOOP_POLLS` OnceLock resolved to. A test that
/// silently ran under legacy pacing would find the deferral branch dead and
/// pass for the wrong reason.
#[cfg(test)]
pub(super) fn force_moving_gc_pacing() -> LegacyGcPacingGuard {
    let previous = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| {
        let previous = cell.get();
        cell.set(Some(true));
        previous
    });
    let cap_previous = GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.replace(false));
    let scavenge_previous = super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.replace(Some(true)));
    LegacyGcPacingGuard {
        previous,
        cap_previous,
        scavenge_previous,
    }
}

/// Pin the one pacing combination in which nursery pressure reaches the DIRECT
/// allocation-point minor: moving-loop polls OFF (so nothing defers) and
/// scavenge ON (so the arm in `gc_check_trigger` is open at all).
///
/// **This is the `PERRY_GC_MOVING_LOOP_POLLS=0` kill-switch configuration, and
/// it is deliberately NOT called "shipped default" any more.** It *was* the
/// shipped default — polls OFF since #7161, scavenge ON since #7056 — and it is
/// the combination #7682 was found in. The follow-up that turned polls back ON
/// made that name a lie in the same PR that introduced it, which is the kind of
/// stale claim this whole line of work is about. A test naming this guard is
/// asserting something about the kill switch; a test that wants the default
/// must take no pacing guard at all.
///
/// The third combination is what needed a guard in the first place.
/// [`force_legacy_gc_pacing`] pins polls OFF *and* scavenge OFF;
/// [`force_moving_gc_pacing`] pins both ON. Every test in this crate therefore
/// declared a pacing mode in which the two flags AGREED — and the
/// alloc-point/deferral interaction that broke is precisely the one where they
/// disagree: scavenge routes nursery pressure to the direct alloc-point minor,
/// while the deferral that was supposed to move that collection to a precise
/// safepoint is gated on the polls flag.
#[cfg(test)]
pub(super) fn force_alloc_point_minor_pacing() -> LegacyGcPacingGuard {
    let previous = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.replace(Some(false)));
    let cap_previous = GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.replace(false));
    let scavenge_previous = super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.replace(Some(true)));
    LegacyGcPacingGuard {
        previous,
        cap_previous,
        scavenge_previous,
    }
}

pub(super) fn gc_trace_enabled() -> bool {
    #[cfg(test)]
    if GC_TRACE_TEST_FORCE.with(Cell::get) {
        return true;
    }

    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_TRACE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

#[cfg(test)]
thread_local! {
    static GC_TRACE_TEST_FORCE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(super) struct TestGcTraceCaptureGuard {
    previous: bool,
}

#[cfg(test)]
impl TestGcTraceCaptureGuard {
    pub(super) fn force_enabled() -> Self {
        let previous = GC_TRACE_TEST_FORCE.with(|force| {
            let previous = force.get();
            force.set(true);
            previous
        });
        clear_test_last_gc_trace_json();
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestGcTraceCaptureGuard {
    fn drop(&mut self) {
        GC_TRACE_TEST_FORCE.with(|force| force.set(self.previous));
        clear_test_last_gc_trace_json();
    }
}

#[derive(Clone, Copy)]
pub(super) enum GcCollectionKind {
    Minor,
    Full,
}

impl GcCollectionKind {
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            GcCollectionKind::Minor => "minor",
            GcCollectionKind::Full => "full",
        }
    }

    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            GcCollectionKind::Minor => 1,
            GcCollectionKind::Full => 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum GcTriggerKind {
    ArenaBytes,
    MallocCount,
    OldGenBytes,
    SurvivorPromotionBytes,
    Emergency,
    Manual,
    Direct,
}

impl GcTriggerKind {
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            GcTriggerKind::ArenaBytes => "arena_bytes",
            GcTriggerKind::MallocCount => "malloc_count",
            GcTriggerKind::OldGenBytes => "old_gen_bytes",
            GcTriggerKind::SurvivorPromotionBytes => "survivor_promotion_bytes",
            GcTriggerKind::Emergency => "emergency",
            GcTriggerKind::Manual => "manual",
            GcTriggerKind::Direct => "direct",
        }
    }

    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            GcTriggerKind::ArenaBytes => 1,
            GcTriggerKind::MallocCount => 2,
            GcTriggerKind::OldGenBytes => 3,
            GcTriggerKind::SurvivorPromotionBytes => 4,
            GcTriggerKind::Manual => 5,
            GcTriggerKind::Direct => 6,
            GcTriggerKind::Emergency => 7,
        }
    }

    #[inline]
    pub(super) const fn progress_kind(self, collection_kind: GcCollectionKind) -> GcProgressKind {
        match (self, collection_kind) {
            (GcTriggerKind::Emergency, GcCollectionKind::Full) => GcProgressKind::EmergencyFull,
            (GcTriggerKind::Manual, GcCollectionKind::Full) => GcProgressKind::ExplicitFull,
            (GcTriggerKind::Manual, GcCollectionKind::Minor) => GcProgressKind::ExplicitSynchronous,
            (
                GcTriggerKind::ArenaBytes
                | GcTriggerKind::MallocCount
                | GcTriggerKind::OldGenBytes
                | GcTriggerKind::SurvivorPromotionBytes
                | GcTriggerKind::Emergency
                | GcTriggerKind::Direct,
                _,
            ) => GcProgressKind::LegacySynchronous,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DeferredGcRequest {
    None,
    CheckTrigger,
    DirectMinor,
    Collect(GcTriggerKind),
}

impl DeferredGcRequest {
    #[inline]
    pub(super) fn merge(self, next: DeferredGcRequest) -> DeferredGcRequest {
        use DeferredGcRequest::*;
        match (self, next) {
            (None, request) => request,
            (request, None) => request,
            (Collect(GcTriggerKind::Manual), _) | (_, Collect(GcTriggerKind::Manual)) => {
                Collect(GcTriggerKind::Manual)
            }
            (Collect(kind), _) => Collect(kind),
            (_, Collect(kind)) => Collect(kind),
            (DirectMinor, _) | (_, DirectMinor) => DirectMinor,
            (CheckTrigger, CheckTrigger) => CheckTrigger,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
pub(super) struct GcStepSnapshot {
    pub(super) arena_step_bytes: usize,
    pub(super) next_arena_trigger_bytes: usize,
    pub(super) malloc_step: usize,
    pub(super) next_malloc_trigger: usize,
    pub(super) trigger_bumped: bool,
}

impl GcStepSnapshot {
    #[inline]
    pub(super) fn current() -> Self {
        Self {
            arena_step_bytes: GC_STEP_BYTES.with(|c| c.get()),
            next_arena_trigger_bytes: GC_NEXT_TRIGGER_BYTES.with(|c| c.get()),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|c| c.get()),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|c| c.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|c| c.get()),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GcTriggerSnapshot {
    pub(super) kind: GcTriggerKind,
    pub(super) steps_before: Option<GcStepSnapshot>,
}

impl GcTriggerSnapshot {
    #[inline]
    pub(super) fn capture(kind: GcTriggerKind) -> Self {
        Self {
            kind,
            steps_before: gc_trace_enabled().then(GcStepSnapshot::current),
        }
    }
}

thread_local! {
    pub(super) static GC_DEFERRED_REQUEST: Cell<DeferredGcRequest> =
        const { Cell::new(DeferredGcRequest::None) };
    pub(super) static GC_OLD_RECLAIM_PENDING: Cell<bool> = const { Cell::new(false) };
    pub(super) static GC_LAST_OLD_RECLAIM_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Live allocated arena bytes measured right after the last FULL
    /// mark-sweep — the baseline for major-GC pacing
    /// (`arena_growth_full_escalation_due`).
    pub(super) static GC_LAST_FULL_ARENA_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Live allocated arena bytes measured when the last FULL mark-sweep STARTED.
    /// Paired with `GC_LAST_FULL_ARENA_IN_USE_BYTES` to price what that full
    /// actually reclaimed — see `GC_MAJOR_PACING_BACKOFF_SHIFT`.
    pub(super) static GC_FULL_CYCLE_PRE_IN_USE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Live allocated arena bytes measured at the END of the most recent
    /// collection of ANY kind — the reading arena-growth pacing tests against
    /// its boundary (#7865).
    ///
    /// The pacing baseline (`GC_LAST_FULL_ARENA_IN_USE_BYTES`) is a *post*-full
    /// reading, i.e. LIVE bytes. Before #7879, testing it against
    /// `arena_in_use_bytes()` at the moment a trigger fired compared it against
    /// allocation high-water — the
    /// entire un-collected nursery, most of which is garbage a minor is about
    /// to reclaim for free. On `gc-handoff/bench/tree.ts` that reading is
    /// 37.7 MB against a 32 MB floor on **every** cycle, so all 40 collections
    /// escalated to a whole-heap mark-sweep and the copying minor was never
    /// even attempted (`copying_nursery.eligible: false`,
    /// `fallback_reason: "not_attempted"`). The escalation then perpetuates
    /// itself: `note_copying_minor_young_survival` is the only thing that can
    /// widen the band, and it only runs when a copying minor runs.
    ///
    /// A post-collection reading is the same *kind* of quantity as the
    /// baseline, and it says exactly what the escalation exists to detect:
    /// **bytes the last minor could not reclaim.** Array-growth forwarding
    /// stubs — the hazard `arena_growth_full_escalation_due` was written for —
    /// pin their blocks through a non-moving minor, so they are still in this
    /// reading and still escalate. Nursery garbage is not.
    pub(super) static GC_LAST_COLLECTION_POST_IN_USE_BYTES: Cell<usize> =
        const { Cell::new(0) };
    /// Yield-adaptive backoff for major-GC pacing (#7726).
    ///
    /// `arena_growth_full_escalation_due` escalates a minor to a full once the
    /// arena's live bytes pass K× the last full's live set. On a workload whose
    /// live set genuinely GROWS — every record retained, nothing to reclaim —
    /// that gate fires on the growth itself and buys nothing: measured on
    /// `gc-handoff/bench/retain.ts`, the two escalated fulls cost 644 ms of a
    /// 1.31 s run and moved arena in-use by 4 MB total, the second one by zero.
    ///
    /// So price each full by what it reclaimed and shift K left when the answer
    /// is "almost nothing". A churn workload's fulls reclaim most of the heap,
    /// keep the shift at 0, and pace exactly as before. The shift is capped and
    /// resets on the first productive full, and it does not touch the
    /// `OldReclaim` escalation — old-gen garbage still forces a full through
    /// `old_reclaim_pressure_due` regardless of this backoff.
    pub(super) static GC_MAJOR_PACING_BACKOFF_SHIFT: Cell<u32> = const { Cell::new(0) };
    /// Survival-adaptive arm of major-GC pacing: `true` once a copying minor
    /// has measured a young-survival ratio at or above
    /// `MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE`, cleared by any minor that
    /// measures less. See `MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER`.
    ///
    /// Deliberately the LAST minor's verdict rather than a running maximum: a
    /// heap that stops retaining must pace tightly again on its very next
    /// collection, not after a decay window.
    pub(super) static GC_MAJOR_PACING_RETAINING: Cell<bool> = const { Cell::new(false) };
    /// Re-entrancy guard for the #5476 direct old-gen reclaim driven from
    /// `gc_check_trigger`: the full collection must not recursively trigger
    /// another reclaim if a hook it runs allocates.
    pub(super) static GC_OLD_RECLAIM_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
    /// #7592: a survivor-promotion handoff full has run and the copying minor
    /// it was scheduled for has not. See
    /// `survivor_promotion_handoff_awaiting_minor`.
    static SURVIVOR_HANDOFF_AWAITING_MINOR: Cell<bool> = const { Cell::new(false) };
    /// Count of handoffs the latch has suppressed — see
    /// `survivor_promotion_handoff_suppressions`.
    static SURVIVOR_HANDOFF_SUPPRESSIONS: Cell<u64> = const { Cell::new(0) };
    /// Phase 2/3 of the moving-GC project: set when an alloc-point nursery
    /// trigger fires while moving mode is on, deferring the collection to the
    /// next precise-root safepoint (event-loop boundary or a codegen loop
    /// back-edge poll) so the copying minor can MOVE survivors instead of the
    /// conservative non-moving minor running mid-expression.
    ///
    /// **Write it only through [`super::set_safepoint_pending`].** The poll's
    /// fast path cannot afford to read a thread-local (on Darwin that is a call
    /// to `_tlv_get_addr`), so this `Cell` has a process-global shadow —
    /// `gc::poll_arm::PERRY_GC_POLL_ARMED` — that codegen loads inline to decide
    /// whether the poll is worth calling. A `set` that bypasses the helper
    /// leaves the shadow reading zero, and a deferred collection whose drain
    /// point has been optimised away is stranded until the next event-loop
    /// boundary.
    pub(super) static GC_SAFEPOINT_PENDING: Cell<bool> = const { Cell::new(false) };
    /// `arena_total_bytes()` sampled at the moment `GC_SAFEPOINT_PENDING` was
    /// last set — the baseline the deferral slack is measured from (#7024).
    /// Meaningless while `GC_SAFEPOINT_PENDING` is false.
    pub(super) static GC_SAFEPOINT_DEFER_ARENA_BASE: Cell<usize> = const { Cell::new(0) };
    /// True while a DECLARED safepoint drain is running: a loop back-edge
    /// poll, the outermost microtask-pump moving minor, or an explicit
    /// `gc()`. Consumed by the `PERRY_GC_SAFEPOINT_ONLY` contract assert in
    /// the root-scan subphase.
    pub(super) static GC_AT_DECLARED_SAFEPOINT: Cell<bool> = const { Cell::new(false) };
}

/// `PERRY_GC_SAFEPOINT_ONLY` — research contract for the native-root modes
/// (`exp/stackmap-viability`): a collection that skips the conservative stack
/// scan consumes only precise roots, and with native stack maps active those
/// roots exist only at mapped PCs — so such a collection may begin only at a
/// declared safepoint; anywhere else it must scan conservatively. Codegen
/// reads the same env to stop emitting statepoints around audited
/// allocate-but-never-reenter helpers; the enforcement in `cycle.rs` is what
/// turns the property from emergent (every possibly-collecting call happens
/// to be mapped) into enforced.
///
/// `1`/`on`/`true` — HEAL: an undeclared precise-root cycle has the
/// conservative scan forced for that cycle (sound: the scan restores
/// liveness, and a conservatively-scanned cycle is non-moving). This is the
/// measuring mode: alloc-point full collections are legitimate today and
/// simply pay the scan.
/// `strict` — PANIC on any undeclared precise-root cycle. This is the gate
/// mode that proves the enforcement is live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SafepointOnlyContract {
    Off,
    Heal,
    Strict,
}

pub(super) fn gc_safepoint_only_contract() -> SafepointOnlyContract {
    use std::sync::OnceLock;
    static CACHED: OnceLock<SafepointOnlyContract> = OnceLock::new();
    *CACHED.get_or_init(
        || match std::env::var("PERRY_GC_SAFEPOINT_ONLY").as_deref() {
            Ok("1") | Ok("on") | Ok("true") => SafepointOnlyContract::Heal,
            Ok("strict") => SafepointOnlyContract::Strict,
            _ => SafepointOnlyContract::Off,
        },
    )
}

/// Contract enforcement chokepoint, called once at every synchronous
/// collection entry. When an undeclared precise-root collection is about to
/// begin, heal mode returns a scan-override guard that must be held for the
/// WHOLE collection: it flips the thread-local override that every consumer
/// of `conservative_stack_scan_decision()` reads — the root-scan subphase,
/// copying-minor eligibility, and the evacuation verifier alike. A previous
/// revision healed by overriding a local variable inside the root-scan
/// subphase only; copying-minor eligibility still read the global decision,
/// concluded there were no conservative roots to pin, and forced evacuation
/// moved objects that raw native-stack words still pointed at.
pub(super) fn contract_scan_heal_guard() -> Option<super::roots::ManualGcScanGuard> {
    if gc_safepoint_only_contract() == SafepointOnlyContract::Off {
        return None;
    }
    if !super::roots::native_stack_maps_active() || GC_AT_DECLARED_SAFEPOINT.with(Cell::get) {
        return None;
    }
    if matches!(
        super::roots::conservative_stack_scan_decision(),
        super::roots::ConservativeStackScanDecision::Scan
    ) {
        return None;
    }
    if gc_safepoint_only_contract() == SafepointOnlyContract::Strict {
        panic!(
            "PERRY_GC_SAFEPOINT_ONLY: precise-root collection began outside \
             a declared safepoint"
        );
    }
    Some(super::roots::ManualGcScanGuard::force_full_scan(
        super::ConservativeScanSite::SafepointContractHeal,
    ))
}

/// RAII marker for a declared-safepoint drain. Nesting-safe: restores the
/// previous value so a poll firing inside a manual `gc()` cannot clear it.
pub(super) struct DeclaredSafepointGuard {
    prev: bool,
}

impl DeclaredSafepointGuard {
    pub(super) fn enter() -> Self {
        let prev = GC_AT_DECLARED_SAFEPOINT.with(|flag| flag.replace(true));
        Self { prev }
    }
}

impl Drop for DeclaredSafepointGuard {
    fn drop(&mut self) {
        let prev = self.prev;
        GC_AT_DECLARED_SAFEPOINT.with(|flag| flag.set(prev));
    }
}

/// Committed arena bytes a deferred nursery trigger may allocate **past the
/// point at which it was deferred** before the alloc-point non-moving minor
/// runs as the safety valve (Phase 2/3). Loop back-edge polls drain the pending
/// flag every iteration, so the arena never grows near this in normal code; the
/// slack bounds RSS for code that reaches no safepoint before the next trigger —
/// a synchronous loop on a specialized lowering path that doesn't yet emit the
/// poll, or a single mega-expression.
///
/// ★ #7024: this is a SLACK (a delta from the deferral point), not an absolute
/// arena size, and that is the whole point. It was an absolute cap derived by
/// `budget_scaled(_, 1, 4, 2 MB)` — **the same formula as
/// `gc_trigger_absolute_ceiling_bytes()`**. `gc_budgeted_due_trigger()` reports
/// `ArenaBytes` due exactly when `arena_total_bytes() >= trigger`, and the
/// deferral required `arena_total_bytes() < cap`; under any explicit
/// `PERRY_GC_HEAP_LIMIT` the two collapsed to the same number, so the two
/// predicates became exact complements and the deferral was *unreachable* — the
/// copying minor could never run under the very pressure setting the stress
/// matrix used to provoke it (`default` arm: 0 copying minors on all 22 corpus
/// rows). A delta cannot collapse into the trigger, at any heap budget: the
/// first deferral of a cycle is always taken and the arena is allowed a bounded
/// amount of growth to reach a poll.
pub(super) const GC_MOVING_DEFER_SLACK_BYTES: usize = 64 * 1024 * 1024;

/// Whether an alloc-point nursery trigger may (still) be deferred to the next
/// precise-root safepoint.
///
/// `deferred_at` is `Some(arena_total_at_the_first_deferral)` while a deferral
/// is outstanding, `None` when none is. The first deferral of a cycle is
/// unconditional — deferring is the *sound* path (it collects with precise,
/// rewritable roots at a real safepoint) and the alloc-point fallback exists
/// only to bound growth when nothing drains the deferral. See
/// `GC_MOVING_DEFER_SLACK_BYTES` for why this is a delta and not an absolute
/// cap (#7024).
#[inline]
pub(super) fn moving_defer_within_slack(
    arena_total: usize,
    deferred_at: Option<usize>,
    slack: usize,
) -> bool {
    match deferred_at {
        None => true,
        Some(base) => arena_total < base.saturating_add(slack),
    }
}

/// RAII guard that marks a #5476 direct old-gen reclaim in progress so a nested
/// `gc_check_trigger` can't re-enter it. See `GC_OLD_RECLAIM_IN_PROGRESS`.
struct OldReclaimReentryGuard;

impl OldReclaimReentryGuard {
    fn enter() -> Self {
        GC_OLD_RECLAIM_IN_PROGRESS.with(|p| p.set(true));
        Self
    }
}

impl Drop for OldReclaimReentryGuard {
    fn drop(&mut self) {
        GC_OLD_RECLAIM_IN_PROGRESS.with(|p| p.set(false));
    }
}

pub(super) const GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES: usize = 48 * 1024 * 1024;
pub(super) const GC_OLD_GEN_RECLAIM_GROWTH_BYTES: usize = 32 * 1024 * 1024;
pub(super) const GC_COPY_PROMOTION_HANDOFF_MIN_BYTES: usize = 24 * 1024 * 1024;

#[inline]
pub(super) fn defer_gc_request(request: DeferredGcRequest) -> bool {
    let locked = GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0);
    if locked {
        GC_DEFERRED_REQUEST.with(|pending| {
            pending.set(pending.get().merge(request));
        });
    }
    locked
}

pub(super) fn take_deferred_gc_request() -> DeferredGcRequest {
    GC_DEFERRED_REQUEST.with(|pending| {
        let request = pending.get();
        pending.set(DeferredGcRequest::None);
        request
    })
}

pub(super) fn flush_deferred_gc_request() {
    if std::thread::panicking() {
        let _ = take_deferred_gc_request();
        return;
    }
    match take_deferred_gc_request() {
        DeferredGcRequest::None => {}
        DeferredGcRequest::CheckTrigger => gc_check_trigger(),
        DeferredGcRequest::DirectMinor => {
            if gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
                .emit_after_current();
        }
        DeferredGcRequest::Collect(GcTriggerKind::Manual) => {
            if manual_gc_blocked_by_unsafe_zone() {
                return;
            }
            manual_gc_collect_now();
        }
        DeferredGcRequest::Collect(kind) => {
            if gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(kind)).emit_after_current();
        }
    }
}

pub fn gc_suppress() {
    if !gen_gc_enabled()
        && crate::arena::arena_in_use_bytes() >= gc_tiny_parse_full_gc_in_use_trigger_dyn_bytes()
    {
        crate::arena::arena_start_fresh_general_block();
    }
    // Issue #745: snapshot arena_total at suppress-start so the
    // matching `gc_bump_malloc_trigger` can size the suppressed
    // window's parse growth and gate the bytes-trigger bump on it.
    GC_PRE_SUPPRESS_BYTES.with(|c| c.set(crate::arena::arena_total_bytes()));
    GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_SUPPRESSED));
}

/// Resume GC triggers after suppression.
pub fn gc_unsuppress() {
    GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_SUPPRESSED));
}

/// True while GC triggers are suppressed (see [`gc_suppress`]).
pub(crate) fn gc_is_suppressed() -> bool {
    GC_FLAGS.with(|f| f.get()) & GC_FLAG_SUPPRESSED != 0
}

/// #6759 Phase C2: RAII no-move window for a TINY allocation. While alive,
/// every GC trigger is blocked (`GC_FLAG_SUPPRESSED` gates `gc_check_trigger`,
/// the budgeted stepper, and the safepoint moving minor), so no heap object
/// can move and raw pointers held anywhere up the caller stack stay valid
/// across the allocation.
///
/// Unlike [`gc_suppress`] this sets ONLY the flag: it skips the fresh-block /
/// pre-suppress-bytes bookkeeping that exists for JSON.parse's multi-megabyte
/// suppressed windows, because the intended use is a few dozen bytes (e.g. an
/// `ObjectMeta` record) where that accounting is pure overhead. Nesting-safe:
/// the previous suppression state is restored on drop, so a scope opened
/// inside an outer `gc_suppress` window does not unsuppress it early.
pub(crate) struct GcSuppressScope {
    was_suppressed: bool,
}

impl GcSuppressScope {
    pub(crate) fn new() -> Self {
        let was_suppressed = gc_is_suppressed();
        if !was_suppressed {
            GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_SUPPRESSED));
        }
        GcSuppressScope { was_suppressed }
    }
}

impl Drop for GcSuppressScope {
    fn drop(&mut self) {
        if !self.was_suppressed {
            GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_SUPPRESSED));
        }
    }
}

/// Rebaseline the malloc-count AND arena-bytes triggers to the current
/// live set so that objects just created during a GC-suppressed window
/// (e.g. JSON.parse) don't immediately trip a collection on the next
/// allocation.
///
/// Pre-fix: only the malloc-count trigger was bumped. JSON.parse on the
/// 108 MB honest_bench fixture lifts arena_total to ~108 MB, the bytes
/// trigger is still at its initial 128 MB threshold, and the iterate+
/// rebuild pass that immediately follows trips bytes-based GC after
/// only ~20 MB of new allocations. The 4 mark/sweep cycles each walk
/// the entire 400 MB live heap (the records tree dominates) and add
/// ~800 ms of overhead to the workload. Bumping the bytes trigger by
/// the per-program step (initially 128 MB, grows up to 1 GB on
/// mostly-garbage sweep evidence) defers the first GC until the
/// post-parse working set itself doubles — for json_pipeline_full
/// that means iterate+rebuild completes inside one GC cycle instead
/// of four.
pub fn gc_bump_malloc_trigger() {
    let current = MALLOC_STATE.with(|s| s.borrow().objects.len());
    use crate::arena::arena_total_bytes;
    let bytes_now = arena_total_bytes();
    let is_tiny_parse = gc_bump_malloc_trigger_with_snapshot(current, bytes_now);
    if is_tiny_parse {
        let use_gen_gc = gen_gc_enabled();
        let in_use_trigger = if use_gen_gc {
            gc_tiny_parse_in_use_trigger_dyn_bytes()
        } else {
            gc_tiny_parse_full_gc_in_use_trigger_dyn_bytes()
        };
        if crate::arena::arena_in_use_bytes() < in_use_trigger {
            return;
        }
        if use_gen_gc {
            if gc_blocked_by_unsafe_zone() {
                GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
                return;
            }
            GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
            GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                if trigger.get() > bytes_now {
                    trigger.set(bytes_now);
                    GC_TRIGGER_ARMED.with(|a| a.set(true));
                }
            });
            gc_check_trigger();
        } else {
            crate::arena::arena_start_fresh_general_block();
            GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
        }
    }
}

/// Run a full collection that was armed by tiny JSON parse churn.
///
/// This is separate from the raise-only post-parse trigger bump. Full
/// mark-sweep needs the collection to happen before the next suppressed parse,
/// not immediately after the previous one, otherwise the parse result is still
/// rooted and every churn block looks partially live.
pub fn gc_collect_pending_suppressed_parse() {
    let pending = GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| {
        let was_pending = pending.get();
        pending.set(false);
        was_pending
    });
    if !pending {
        return;
    }
    if GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
    {
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
        return;
    }

    let total = crate::arena::arena_total_bytes();
    GC_NEXT_TRIGGER_BYTES.with(|trigger| {
        if trigger.get() > total {
            trigger.set(total);
            GC_TRIGGER_ARMED.with(|a| a.set(true));
        }
    });
    gc_check_trigger();
}

/// Schedule a collection for the next JSON.parse boundary.
///
/// Direct parse + stringify churn creates a full JS object graph, then walks it
/// immediately. If the arena trigger fires during that stringify, copied-minor
/// has to copy the just-parsed tree even though it dies at the end of the loop
/// body. Deferring the collection to the next parse boundary lets the caller's
/// loop-scope roots clear first, so the collector reclaims the previous tree
/// without promoting or repeatedly copying transient JSON data.
pub fn gc_schedule_parse_boundary_collection_if_pressure() {
    if !gen_gc_enabled() {
        return;
    }
    if crate::arena::arena_in_use_bytes() < gc_tiny_parse_in_use_trigger_dyn_bytes() {
        return;
    }
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
}

/// Old-gen pressure the reclaim arms act on: block-offset in-use minus the
/// swept holes the free list can already hand back (#7437). Before hole
/// reuse existed, dead-but-unreclaimable bytes counted as pressure, so
/// old-reclaim kept re-firing full collections that could not actually
/// lower the number they were watching (probe 12: 49/50 blocks pinned by
/// scattered survivors, in-use immovable at ~105 MB).
pub(super) fn old_gen_reclaimable_pressure_bytes() -> usize {
    crate::arena::old_gen_in_use_bytes().saturating_sub(super::old_free_bytes())
}

/// #7592: divisor for the proportional old-reclaim growth band — the next
/// full reclaim fires when old-gen has grown `baseline / 2` (50%) past the
/// post-reclaim baseline, Go's GOGC shape.
const OLD_RECLAIM_GROWTH_DIVISOR: usize = 2;

/// #7592: how much old-gen may grow past the post-reclaim baseline before the
/// next full reclaim is due. The constant `gc_old_gen_reclaim_growth_dyn_bytes`
/// band survives as the floor, but a *constant* band cannot be the whole
/// answer: each full reclaim costs O(live), so a fixed-bytes cadence makes
/// total major-GC work quadratic in the live set — the same shape #7594
/// removed one generation down, just paced by promotion instead of
/// allocation. A band proportional to the baseline makes the major count
/// logarithmic in heap growth (`∫ dL/(L/2) = 2·ln(Lmax/L0)`), so total major
/// work stays linear in the final live set.
///
/// This is also strictly better on the #7437 failure mode (a reclaim that
/// cannot actually lower the number it watches, e.g. pinned survivors): the
/// futile reclaim resets the baseline to the still-high value, so the next
/// band is *larger*, spacing futile repeats out instead of re-firing every
/// constant step.
///
/// Shared by `old_reclaim_pressure_due` and `gc_old_reclaim_debt_bytes` so
/// the "is it due" predicate and the debt arithmetic cannot diverge (#7024's
/// two-predicates-collapse family).
pub(super) fn gc_old_reclaim_growth_band_bytes(baseline: usize) -> usize {
    let band = gc_old_gen_reclaim_growth_dyn_bytes().max(baseline / OLD_RECLAIM_GROWTH_DIVISOR);
    // Survival-adaptive, the same signal and the same multiplier the
    // arena-growth escalation uses (`MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER`).
    //
    // `credit_promoted_bytes_to_old_baseline` already exempts old-gen growth
    // that a minor PROVED live, but a large object is allocated straight into
    // old-gen and never passes through promotion, so its bytes are uncredited
    // growth even when they are the program's live data. On `retain.ts` that is
    // the element array itself: with the arena-growth escalation correctly
    // declining, this band became the binding constraint and fired a 452 ms
    // full that reclaimed 7.6% — the same futile-full shape one trigger over,
    // reached by the same route. While the young generation is not dying, old
    // growth is priced as live here too.
    if GC_MAJOR_PACING_RETAINING.with(|c| c.get()) {
        return band.saturating_mul(MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER);
    }
    band
}

#[inline]
pub(super) fn old_reclaim_pressure_due(old_in_use: usize, baseline: usize) -> bool {
    (old_in_use >= gc_old_gen_reclaim_threshold_dyn_bytes()
        && baseline < gc_old_gen_reclaim_threshold_dyn_bytes())
        || old_in_use.saturating_sub(baseline) >= gc_old_reclaim_growth_band_bytes(baseline)
}

/// Whether an imminent promotion justifies a full old reclaim FIRST.
///
/// #7592: the pressure test is on the CURRENT old-gen occupancy only. It used
/// to be `old_reclaim_pressure_due(old_in_use + promotable_bytes, _)` — a
/// *prediction* of where old-gen would land after the promotion — which is
/// the #7594 mistake in another coat: the promotable bytes are in the
/// survivor space, where a full mark-sweep can neither reclaim them (they are
/// live) nor reclaim the old-gen space they have not yet occupied. A handoff
/// scheduled on predicted pressure over a near-empty old-gen is guaranteed
/// futile — measured on #7592's `json_pipeline` at 500k records as a 1,015 ms
/// full over 4.2 MB of old-gen that freed nothing. The only useful work a
/// handoff can do is clear CURRENT old garbage so the promotion lands in
/// reused holes; when old-gen has no reclaimable pressure of its own, the
/// promotion should simply proceed and grow it.
///
/// `promotable_bytes` remains the reason to *bother* checking: below the
/// handoff minimum the upcoming promotion is too small to be worth a full
/// collection under any old-gen state.
#[inline]
pub(super) fn copied_minor_promotion_handoff_pressure_due(
    promotable_bytes: usize,
    old_in_use: usize,
    baseline: usize,
) -> bool {
    promotable_bytes >= gc_copy_promotion_handoff_min_dyn_bytes()
        && old_reclaim_pressure_due(old_in_use, baseline)
}

pub(super) fn copied_minor_promotable_active_survivor_bytes() -> usize {
    // Pure measurement pass over the active survivor semispace only. Use the
    // block-filtered walk so blocks outside the active range are skipped in
    // O(n_blocks) instead of iterating every object in Eden/longlived/old-gen
    // just to discard it (#6181) — both walkers visit the regions in the same
    // order with the same global block-index bases, so the filter is exactly
    // equivalent to the previous in-callback range check.
    let active_range = crate::arena::active_survivor_block_index_range();
    let mut promotable = 0usize;
    crate::arena::arena_walk_objects_filtered(
        |block_idx| active_range.contains(&block_idx),
        |header_ptr, _block_idx| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                let flags = (*header).gc_flags;
                if flags & GC_FLAG_FORWARDED != 0 {
                    return;
                }
                let prior_age = copied_survival_age((*header)._reserved, flags);
                let next_age = prior_age.saturating_add(1);
                // Mirror move_young's promotion predicate, including the
                // adaptive threshold (gc/tenuring.rs), so this pacing estimate
                // matches what the next copying minor will actually promote.
                if flags & GC_FLAG_TENURED != 0 || next_age >= tenuring_survivals() {
                    promotable = promotable.saturating_add((*header).size as usize);
                }
            }
        },
    );
    promotable
}

/// #7592: whether a survivor-promotion handoff full has run without the copying
/// minor it exists to enable having run since.
///
/// The handoff replaces a minor with a full mark-sweep to make room in old-gen
/// for survivors that are about to be promoted. But a full mark-sweep is
/// **non-moving — it promotes nothing**, so it cannot itself relieve the
/// pressure it was scheduled for: the survivor space still holds the same
/// bytes, the reclaim baseline it resets does not count them, and the predicate
/// is immediately true again. Without this latch the next minor is intercepted
/// too, and the collector livelocks on full collections that free nothing —
/// measured on #7592's `json_pipeline` as 19 consecutive fulls, each freeing
/// 0.0 MB at ~400 ms, which was 7.6 s of an 8.6 s phase.
///
/// One handoff per copying minor is the invariant: the handoff makes room, the
/// minor does the promotion that consumes it.
pub(super) fn survivor_promotion_handoff_awaiting_minor() -> bool {
    SURVIVOR_HANDOFF_AWAITING_MINOR.with(Cell::get)
}

pub(super) fn note_survivor_promotion_handoff_full() {
    SURVIVOR_HANDOFF_AWAITING_MINOR.with(|flag| flag.set(true));
}

/// How many handoffs the latch has suppressed. Every one of these was a full
/// mark-sweep that would have freed nothing — on `main` this counter would have
/// read 18 for #7592's 200k `json_pipeline` run. It exists so the suppression
/// itself is observable: the latch short-circuits before the arena inspection,
/// so a test with an empty heap cannot otherwise distinguish "suppressed" from
/// "there was no pressure anyway".
pub(super) fn survivor_promotion_handoff_suppressions() -> u64 {
    SURVIVOR_HANDOFF_SUPPRESSIONS.with(Cell::get)
}

/// Clear the latch — called only when a *copying* minor completes, since only
/// that collector promotes. A non-moving minor fallback promotes nothing, so
/// re-arming on one would reinstate the livelock at half rate.
pub(super) fn note_copying_minor_completed() {
    SURVIVOR_HANDOFF_AWAITING_MINOR.with(|flag| flag.set(false));
}

pub(super) fn copied_minor_promotion_handoff_due(trigger_kind: GcTriggerKind) -> bool {
    if !matches!(
        trigger_kind,
        GcTriggerKind::ArenaBytes | GcTriggerKind::MallocCount
    ) {
        return false;
    }
    // A handoff full has already run and promoted nothing; let the copying
    // minor it was scheduled for actually happen (#7592). Placed before the
    // survivor walk below so a suppressed handoff also skips that O(n) pass.
    if survivor_promotion_handoff_awaiting_minor() {
        SURVIVOR_HANDOFF_SUPPRESSIONS.with(|n| n.set(n.get().saturating_add(1)));
        return false;
    }
    if crate::arena::copying_active_survivor_in_use_bytes()
        < gc_copy_promotion_handoff_min_dyn_bytes()
    {
        return false;
    }
    let promotable = copied_minor_promotable_active_survivor_bytes();
    let old_in_use =
        old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    copied_minor_promotion_handoff_pressure_due(promotable, old_in_use, baseline)
}

/// #7592: credit a copying minor's promoted bytes to the old-reclaim
/// baseline.
///
/// Promoted bytes are live *by construction* — only marked-live objects get
/// copied — so a full mark-sweep fired the moment promotion pushes old-gen
/// over a threshold is guaranteed to find them all live and free nothing
/// (measured: a 2,100 ms full over 274 MB of just-promoted objects, 0.0 MB
/// freed). Treating the promotion delta as part of the "clean" baseline means
/// the next reclaim fires only after old-gen has *grown or churned* past it —
/// the proportional band (`gc_old_reclaim_growth_band_bytes`) then prices the
/// reclaim off the real live set. Pre-existing old garbage is unaffected: the
/// credit is exactly the promoted delta, never a resync to current in-use.
///
/// The trade is the standard GOGC one: bytes that are promoted and then die
/// quickly now wait for the growth band instead of the next threshold
/// crossing. That is deliberate — a promoted-then-dead cohort big enough to
/// matter moves the band by its own size.
pub(super) fn credit_promoted_bytes_to_old_baseline(promoted_bytes: usize) {
    if promoted_bytes == 0 {
        return;
    }
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES
        .with(|bytes| bytes.set(bytes.get().saturating_add(promoted_bytes)));
}

/// Feed a copying minor's measured young-survival ratio to arena-growth pacing.
///
/// Two effects, both gated on the same measurement:
///
/// * It arms/disarms `GC_MAJOR_PACING_RETAINING`, which widens the escalation
///   growth band (see `MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER`).
/// * While retaining, it re-baselines arena-growth pacing on the occupancy that
///   *survived this collection*. Without that the band has nothing to scale:
///   before the first full the baseline is 0, so the boundary degenerates to
///   the absolute `PERRY_GC_MAJOR_PACING_FLOOR_MB` and **any** program that
///   retains more than 32 MB pays a whole-heap mark-sweep for doing so.
///
/// The re-baseline is a ratchet (`max`), never a decrease, so a minor cannot
/// pull the boundary in below what the last full established — and it is
/// skipped entirely when the heap is not retaining, which is what keeps
/// `churn`/`cycles`/`push_cls` (0–4 permille survival) bit-identical to the
/// previous policy: their baseline stays 0 and their boundary stays the floor.
///
/// Note the direction of the whole change: because the baseline only ever
/// ratchets UP and the multiplier is ≥ 1, the escalation boundary is never
/// *lower* than it was before. This can only make fulls rarer, never more
/// frequent — the exposure is deferred reclamation (RSS), not extra pauses.
pub(super) fn note_copying_minor_young_survival(survival_permille: u64) {
    let retaining = survival_permille >= MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE;
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(retaining));
    if !retaining {
        return;
    }
    let survived = pacing_arena_in_use_bytes();
    GC_LAST_FULL_ARENA_IN_USE_BYTES.with(|bytes| bytes.set(bytes.get().max(survived)));
}

/// Whether the last copying minor measured a retaining heap. Trace/test
/// observability — a gate that cannot see this cannot prove which arm paced a
/// given run.
#[cfg(any(feature = "diagnostics", test))]
pub(super) fn major_pacing_retaining() -> bool {
    GC_MAJOR_PACING_RETAINING.with(|c| c.get())
}

pub(super) fn maybe_schedule_old_reclaim_after_copied_minor() {
    // #6010: external Map/Set side buffers count toward old-gen pressure —
    // a tenured-then-dead Map holds its multi-MB buffer until a full
    // reclaim's old-gen sweep finalizes it, so the buffer bytes must be
    // able to escalate that reclaim.
    let old_in_use =
        old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    if old_reclaim_pressure_due(old_in_use, baseline) {
        GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    }
}

pub(super) fn finish_full_old_reclaim_baseline() {
    // Baseline includes external side-buffer bytes (#6010) so the growth
    // delta in `old_reclaim_pressure_due` stays unit-consistent.
    let old_in_use =
        old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    // Record the TOTAL post-full live set for major-GC pacing (young+old): the
    // full sweep is the only collection that frees forwarding stubs, so this is
    // the "clean" size the arena returns to and the base for the K× growth gate.
    let post_in_use = crate::arena::arena_live_allocated_bytes();
    GC_LAST_FULL_ARENA_IN_USE_BYTES.with(|bytes| bytes.set(post_in_use));
    update_major_pacing_backoff(post_in_use);
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    // #7742: the dead bytes that whole-block promotion parked in old-gen are
    // exactly what this collection just reclaimed, so the running budget that
    // caps them starts over.
    super::note_full_collection_reclaimed_old_gen();
}

/// Percent of the pre-full live set a full must reclaim to count as productive.
/// Below this the next arena-growth escalation is pushed out (see
/// `GC_MAJOR_PACING_BACKOFF_SHIFT`).
/// Deliberately low. A full that reclaims 20% of the heap is still doing real
/// work, and pushing its successor out would carry that garbage twice as long;
/// the shape this exists for reclaims single-digit percent (5.9% and 0.0% on
/// `retain.ts`). Raising it trades RSS for pause time.
const MAJOR_PACING_PRODUCTIVE_YIELD_PCT: usize = 20;

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
const MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE: u64 = 900;

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
const MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER: usize = 4;

/// Record what the just-finished full reclaimed and adjust the pacing backoff.
///
/// `pre` is the arena in-use reading captured when the full cycle started
/// (`note_full_cycle_started`); a full that shrinks it by less than
/// `MAJOR_PACING_PRODUCTIVE_YIELD_PCT` shifts the next escalation threshold
/// left by one, a productive full resets the shift to 0. Deliberately measured
/// on the SAME metric the escalation gate reads (`arena_in_use_bytes`) so the
/// two cannot disagree about whether a full helped.
fn update_major_pacing_backoff(post_in_use: usize) {
    let pre_in_use = GC_FULL_CYCLE_PRE_IN_USE_BYTES.with(|bytes| bytes.get());
    if pre_in_use == 0 {
        // No start reading (a full driven from a path that does not announce
        // itself): leave the shift alone rather than guess a yield.
        return;
    }
    GC_FULL_CYCLE_PRE_IN_USE_BYTES.with(|bytes| bytes.set(0));
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
    GC_FULL_CYCLE_PRE_IN_USE_BYTES.with(|bytes| bytes.set(pacing_arena_in_use_bytes()));
}

/// The arena reading BOTH halves of arena-growth pacing must use: the
/// escalation predicate's comparison against the boundary, and the pre-full
/// reading `note_full_cycle_started` records for `update_major_pacing_backoff`
/// to price the result against.
///
/// `update_major_pacing_backoff`'s doc already says these are "deliberately
/// measured on the SAME metric ... so the two cannot disagree about whether a
/// full helped". The accessor now deliberately reads live allocated object
/// bytes, not block high-water: fragmentation must not schedule a full or make
/// one look unproductive (#7879).
///
/// It is also the injection point the positive-direction test needs. Forcing a
/// `true` verdict from the REAL predicate otherwise requires an arena above
/// `PERRY_GC_MAJOR_PACING_FLOOR_MB` (32 MB by default), and the floor cannot be
/// lowered per-test: `major_pacing_config` is a process-wide `OnceLock`, so an
/// env var only takes effect if this test happens to run first. A 32 MB live
/// heap in a unit test is what `major_pacing_escalation_threshold_for` was
/// factored out to avoid, so the seam goes here instead — `#[cfg(test)]`, so it
/// compiles out of every shipping build and is not a mode anything can be
/// configured into (CLAUDE.md's GC knob kill-policy is about runtime knobs;
/// this is not one).
pub(super) fn pacing_arena_in_use_bytes() -> usize {
    #[cfg(test)]
    if let Some(bytes) = TEST_PACING_ARENA_IN_USE.with(|cell| cell.get()) {
        return bytes;
    }
    crate::arena::arena_live_allocated_bytes()
}

/// Record the post-collection live arena bytes arena-growth pacing tests
/// against. Called once at the end of every cycle, minor and full alike. The
/// copying fast path publishes directly; non-copying cycles publish from
/// `GcCycle::publish_reclaim_outcome` after their sweep census.
pub(super) fn note_collection_finished_arena_occupancy() {
    let bytes = pacing_arena_in_use_bytes();
    GC_LAST_COLLECTION_POST_IN_USE_BYTES.with(|cell| cell.set(bytes));
}

/// The arena reading [`arena_growth_full_escalation_due`] tests — see
/// [`GC_LAST_COLLECTION_POST_IN_USE_BYTES`].
///
/// Zero before any collection has finished, so the very first collection of a
/// process is never escalated: there is no evidence yet that a minor would
/// fail to reclaim, and the whole point of the pacing is to fire on that
/// evidence rather than on allocation volume.
///
/// Keeps the `#[cfg(test)]` seam in front, so the existing positive-direction
/// tests that force a `true` verdict out of the real predicate keep working
/// without a 32 MB live heap.
pub(super) fn pacing_escalation_reading_bytes() -> usize {
    #[cfg(test)]
    if let Some(bytes) = TEST_PACING_ARENA_IN_USE.with(|cell| cell.get()) {
        return bytes;
    }
    GC_LAST_COLLECTION_POST_IN_USE_BYTES.with(|cell| cell.get())
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`pacing_arena_in_use_bytes`]. Thread-local, so
    /// concurrently-running tests cannot see each other's value.
    static TEST_PACING_ARENA_IN_USE: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Current arena-growth escalation backoff shift.
#[cfg(any(feature = "diagnostics", test))]
pub(super) fn major_pacing_backoff_shift() -> u32 {
    GC_MAJOR_PACING_BACKOFF_SHIFT.with(|shift| shift.get())
}

/// `(post-full baseline bytes, backoff shift, live allocated bytes at or above
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
#[cfg(any(feature = "diagnostics", test))]
pub(super) fn major_pacing_snapshot() -> (usize, u32, Option<usize>) {
    let baseline = GC_LAST_FULL_ARENA_IN_USE_BYTES.with(|bytes| bytes.get());
    let shift = major_pacing_backoff_shift();
    (baseline, shift, major_pacing_escalation_threshold_bytes())
}

#[cfg(test)]
pub(super) fn test_reset_major_pacing_backoff() {
    GC_MAJOR_PACING_BACKOFF_SHIFT.with(|shift| shift.set(0));
    GC_FULL_CYCLE_PRE_IN_USE_BYTES.with(|bytes| bytes.set(0));
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(false));
}

/// The pre-full arena reading `arena_growth_full_escalation_due` recorded, or 0
/// if it declined to escalate. Non-zero is the proof that the escalation is
/// PRICED — without it the backoff cannot fire and the pacing silently reverts
/// to the unconditional K× rule.
#[cfg(test)]
pub(super) fn test_major_pacing_pre_in_use_bytes() -> usize {
    GC_FULL_CYCLE_PRE_IN_USE_BYTES.with(|bytes| bytes.get())
}

/// Override the post-collection occupancy the escalation predicate reads.
/// Returns the previous value so a test can restore it.
#[cfg(test)]
pub(super) fn test_set_collection_post_in_use_bytes(bytes: usize) -> usize {
    GC_LAST_COLLECTION_POST_IN_USE_BYTES.with(|cell| {
        let previous = cell.get();
        cell.set(bytes);
        previous
    })
}

#[cfg(test)]
pub(super) fn test_set_major_pacing_baseline(bytes: usize) -> usize {
    GC_LAST_FULL_ARENA_IN_USE_BYTES.with(|cell| {
        let previous = cell.get();
        cell.set(bytes);
        previous
    })
}

/// Override the arena reading arena-growth pacing sees, for the duration of a
/// test. `None` restores the real live-allocation reading. Returns the previous
/// value so a test can restore it rather than assume it was unset.
#[cfg(test)]
pub(super) fn test_set_pacing_arena_in_use(bytes: Option<usize>) -> Option<usize> {
    TEST_PACING_ARENA_IN_USE.with(|cell| {
        let previous = cell.get();
        cell.set(bytes);
        previous
    })
}

#[cfg(test)]
pub(super) fn test_note_full_cycle_reclaimed(pre_in_use: usize, post_in_use: usize) {
    GC_FULL_CYCLE_PRE_IN_USE_BYTES.with(|bytes| bytes.set(pre_in_use));
    update_major_pacing_backoff(post_in_use);
}

pub(super) fn gc_bump_malloc_trigger_with_snapshot(current: usize, bytes_now: usize) -> bool {
    let step = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(current + step));

    let pre_suppress = GC_PRE_SUPPRESS_BYTES.with(|c| c.get());
    let parse_growth = bytes_now.saturating_sub(pre_suppress);

    // Issue #745: gate the bytes-trigger bump on the suppressed
    // window's parse size, with two regimes:
    //
    //   * Tiny parses (<= 1 MB of arena growth) — the
    //     `test_memory_json_churn` shape: 5 k iters × ~13 KB per
    //     parse into a fragmented arena, where a small parse can still
    //     force one fresh 1 MB block while GC is suppressed. Allow
    //     repeated bumps here, but clamp them to the collector's
    //     absolute trigger ceiling so a tiny parse loop cannot keep
    //     ratcheting the next GC beyond the RSS guardrail. If a
    //     suppressed parse crosses the trigger, the next pre-parse or
    //     normal allocation check sees the trigger still due.
    //
    //   * Medium-or-larger parses (> 1 MB) — the
    //     `json_pipeline_full` and `json_polyglot` shapes: once per
    //     GC cycle, bump the trigger to grant the post-parse
    //     workload a `step` of headroom. The flag clears in
    //     `gc_collect_inner` so the next cycle gets its own bump.
    //     This is what was missing in commit 56818086 — each
    //     iteration of `json_polyglot`'s 50-iter loop bumped the
    //     trigger by another `step`, and after productive
    //     step-doubling that grew toward 1 GB the trigger ratcheted
    //     hundreds of MB above the actual live set (~5 MB) and GC
    //     never fired across the entire run. Peak RSS climbed to
    //     254/411 MB on the lazy-tape path.
    //
    // Also cap the effective step at the *initial* value (64 MB) so
    // post-`73a48ced` step-doubling can't make a single bump grant
    // hundreds of MB of headroom. The original optimization measured
    // `step` at INITIAL on the first call (no prior GC), so the cap
    // is a no-op for the `json_pipeline_full` workload.
    let is_tiny_parse = gc_suppressed_parse_is_tiny(parse_growth);
    if !is_tiny_parse && GC_TRIGGER_BUMPED.with(|c| c.get()) {
        return false;
    }

    let bytes_step = GC_STEP_BYTES.with(|c| c.get());
    let bytes_trigger = gc_bump_arena_trigger_target(bytes_now, bytes_step, is_tiny_parse);
    // Only raise — never lower — so this can't accidentally trip a
    // pending collection that the existing trigger had already armed.
    GC_NEXT_TRIGGER_BYTES.with(|c| {
        // Compare against the effective (budget-clamped) trigger, not the
        // raw cell: on a small-budget device the cell's un-armed default
        // (128 MB) would otherwise swallow every legitimate parse bump.
        if bytes_trigger > effective_next_arena_trigger() {
            c.set(bytes_trigger);
            GC_TRIGGER_ARMED.with(|a| a.set(true));
            if !is_tiny_parse {
                GC_TRIGGER_BUMPED.with(|b| b.set(true));
            }
        }
    });
    is_tiny_parse
}

fn gc_rebaseline_malloc_trigger_to_survivors(mstep: usize) {
    let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
}

fn gc_finish_arena_trigger_collection(pre_in_use: usize, outcome: GcCollectOutcome) -> u64 {
    let sweep_freed_bytes = outcome.freed_bytes;
    let malloc_swept = outcome.malloc_swept;
    let post_in_use = crate::arena::arena_in_use_bytes();

    // Adaptive step:
    //   >90% freed → double (almost all dead — `object_create`-style
    //                        hot loops fit their entire working set
    //                        under the threshold; defer.)
    //   10-90% freed → halve (productive collection — real reclaim
    //                         is possible, so collect again sooner
    //                         to keep the working set bounded;
    //                         16MB floor prevents thrash).
    //   <10% freed → double (live set genuinely large, don't thrash).
    //
    // Issue #179: the halve band was formerly 10-25% only. Before
    // the age-restricted block-persist, collections in the 25-90%
    // band were illusory — block-persist re-marked dead neighbors
    // as live, so "freed" over-counted what was actually reclaimable
    // on subsequent cycles. Keeping step flat there was the correct
    // defensive choice. With v0.5.193's block-persist limited to
    // the last 5 general-arena blocks, "freed" now reflects real
    // sweep effectiveness, and widening the halve band lets the
    // trigger fire often enough for middle blocks to actually
    // reset and RSS to stay bounded. `bench_json_roundtrip` moves
    // into this band: first GC frees ~73% → halve → next trigger
    // ~56MB later → second GC frees more → step halves again →
    // RSS stabilizes instead of growing linearly with iters.
    //
    // The >90% and <10% branches retain the existing "don't thrash"
    // protection (Issue #64 follow-up): both extremes mean the
    // live/garbage ratio is such that collecting sooner is wasted
    // work.
    // Adaptive step, driven by the *larger* of sweep-freed-bytes
    // and the block-reset delta (`pre - post`). `freed_bytes` from
    // the sweep surfaces reclaim potential immediately (before the
    // 2-cycle grace completes); `pre - post` reflects actual block
    // resets landing on subsequent cycles. Using the max keeps the
    // step adaptive to both surfaces of productive collection.
    //
    //   >90% freed → double (near-total sweep; `object_create`-style
    //                        hot loops pay one GC then run free).
    //   25-90% freed → halve (productive — reclaim is meaningful,
    //                         collect again sooner to bound RSS).
    //   10-25% freed → keep (marginal — don't thrash vs. churn).
    //   <10% freed → double (live set genuinely large, defer).
    //
    // Issue #179 driver: formerly the halve band was 10-25% only,
    // which never fired on `bench_json_roundtrip` because typical
    // freed-pct there is 50-80%. With the max-of-two metric AND
    // the age-restricted block-persist (v0.5.193), widening the
    // halve band to 25-90% lets the trigger fire often enough for
    // middle blocks to actually reset, without dropping into the
    // 16MB-floor thrash territory that hurts throughput on
    // moderate workloads. `bench_json_roundtrip` lands here on
    // most cycles (60-80% freed) → step halves → GC fires 3-4×
    // across the 50-iter loop → RSS stabilizes around the live-
    // set size plus the 5-block recent-window headroom.
    //
    // The 16MB floor keeps `object_create`-scale hot loops from
    // thrashing: those workloads land in the >90% band on the
    // first GC and immediately double the step, escaping the
    // halve trajectory after a single cycle.
    let block_reclaim = pre_in_use.saturating_sub(post_in_use);
    let freed = std::cmp::max(block_reclaim, sweep_freed_bytes as usize);
    let mut step = GC_STEP_BYTES.with(|c| c.get());
    let old_step = step;
    if pre_in_use > 0 {
        let pct_freed = (freed * 100) / pre_in_use;
        // 2026-05-02: widen the "double" band from `>90% || <10%` to
        // `>=85% || <10%`. ECS perf-comprehensive's two
        // alloc-heavy benches (10k two-comp, 5k × 3 cmds) sweep
        // at 86-89 % freed, which previously landed in the halve
        // band. Step would shrink 64→32→16 MB across the first
        // two benches, then GC fired every ~16 MB of fresh
        // allocations — a 60 ms `mark_block_persisting_arena_objects`
        // outlier landed mid-measured-round on each refire.
        // Promoting 85-90 % to double lets the step grow to the
        // 128 MB ceiling on the first sweep, the trigger jumps
        // out past the bench's full per-iteration allocation
        // budget, and subsequent GCs fire BETWEEN measured rounds
        // (i.e. invisible to the bench's wall-time counter).
        // `bench_json_roundtrip` lands at 50-80 % freed and is
        // unchanged — it still halves and stabilizes at the floor.
        //
        // With INITIAL == ABSOLUTE_CEILING (128 MB), the post-GC
        // `next_trigger` cap below supersedes doubling above the
        // ceiling; the doubling branch is kept for the bisection
        // escape hatch.
        if !(10..=84).contains(&pct_freed) {
            step = (step * 2).min(GC_THRESHOLD_MAX_BYTES);
        } else if pct_freed >= 25 {
            step = (step / 2).max(16 * 1024 * 1024);
        }
        // 10-25% freed → keep step unchanged (marginal churn).
        GC_STEP_BYTES.with(|c| c.set(step));
        if std::env::var_os("PERRY_GC_DIAG").is_some() {
            eprintln!(
                "[gc-step] pre_in_use={} post_in_use={} sweep_freed={} block_reclaim={} pct={}% step={}→{}",
                pre_in_use, post_in_use, sweep_freed_bytes, block_reclaim, pct_freed, old_step, step
            );
        }
    }
    let new_total = crate::arena::arena_total_bytes();
    // C4b-δ-tune: hard cap on next_trigger so the >90%-freed
    // step-doubling can't drive peak nursery past the initial
    // threshold. Floor: at least 16 MB of headroom past
    // `new_total` so a workload whose post-GC live set already
    // approaches the ceiling doesn't thrash on every fresh
    // allocation.
    let stepped = new_total.saturating_add(step);
    let capped = stepped.min(gc_trigger_absolute_ceiling_bytes());
    let floor = new_total.saturating_add(gc_trigger_headroom_floor_bytes());
    // #7742: whole-block promotion hands Eden's blocks to old-gen instead of
    // recycling them, so the free young capacity that would have carried the
    // mutator to the next collection is gone from `new_total`. Give it back as
    // headroom (consumed once) rather than by re-reserving the blocks, which
    // would map memory the program may never reach.
    let next_trigger =
        std::cmp::max(capped, floor).saturating_add(super::take_promoted_young_capacity_credit());
    GC_NEXT_TRIGGER_BYTES.with(|c| c.set(next_trigger));
    GC_TRIGGER_ARMED.with(|a| a.set(true));
    // Rebaseline the malloc-count trigger only if this collection
    // actually swept malloc objects. Copied-minor arena collections
    // may skip the malloc sweep while count pressure is still below
    // its trigger; moving the trigger in that case would postpone
    // reclamation of already-tracked dead malloc churn.
    if malloc_swept {
        let mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        gc_rebaseline_malloc_trigger_to_survivors(mstep);
    }
    outcome.emit_after_current()
}

fn gc_finish_malloc_trigger_collection(pre_count: usize, outcome: GcCollectOutcome) -> u64 {
    debug_assert!(
        outcome.malloc_swept,
        "malloc-count trigger must sweep malloc objects"
    );
    let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
    // Adapt the malloc-count step based on collection effectiveness.
    //
    // Issue #58 insight: in tight allocation loops the conservative
    // stack scanner keeps almost everything alive — GC finds <10%
    // garbage and wastes time walking 100k+ objects. In this regime
    // we should BACK OFF (increase the step) so the loop can finish
    // without GC interference. Once control returns to a higher scope
    // the dead objects will fall off the stack and become collectable.
    //
    // Conversely, when GC reclaims >75% it's working well and can
    // afford to stay at the current cadence or even speed up.
    let mut mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    if pre_count > 0 {
        let freed = pre_count.saturating_sub(survivors);
        let pct_freed = (freed * 100) / pre_count;
        if pct_freed < 15 {
            // GC is nearly useless — quadruple the step to back off fast
            mstep = (mstep * 4).min(GC_MALLOC_COUNT_STEP_MAX);
        } else if pct_freed < 50 {
            // GC is partially effective — double the step
            mstep = (mstep * 2).min(GC_MALLOC_COUNT_STEP_MAX);
        } else if pct_freed > 90 {
            // GC is highly effective — halve the step to collect sooner
            mstep = (mstep / 2).max(GC_MALLOC_COUNT_STEP_MIN);
        }
        // 50-90% freed: keep current step (balanced)
        GC_MALLOC_COUNT_STEP.with(|c| c.set(mstep));
    }
    if outcome.malloc_swept {
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
    }
    outcome.emit_after_current()
}

/// Check if automatic GC pressure should pay a bounded assist step.
///
/// Arena and malloc thresholds are heap goals. Crossing them starts or resumes
/// a budgeted cycle. Allocation-side assists spend at most
/// `GC_MUTATOR_ASSIST_WORK_UNITS` and only enter phases that already consume
/// that budget; unsliced phases stay active for host-driven budgeted steps.
pub fn gc_check_trigger() {
    if GC_BUDGETED_STEP_ACTIVE.with(Cell::get) {
        return;
    }
    // Issue #62: single TLS access covers both `in_alloc` and `suppressed`.
    let flags = GC_FLAGS.with(|f| f.get());
    if flags & GC_FLAG_SUPPRESSED != 0 {
        return;
    }
    if !gc_budgeted_cycle_active() && flags & GC_FLAG_IN_ALLOC != 0 {
        return;
    }
    if gc_blocked_by_unsafe_zone() {
        return;
    }
    if defer_gc_request(DeferredGcRequest::CheckTrigger) {
        return;
    }

    // #5476: a workload that churns *large* temporaries (>16 KB, born directly
    // in the old arena) grows the old generation without ever exercising the
    // nursery. Old-gen reclaim pressure schedules a budgeted full cycle that
    // *would* return the dead old blocks to the OS — but the budgeted stepper is
    // blocked whenever synchronous-only root scanners are registered (the common
    // case in a compiled program), and even when it runs it only advances through
    // bounded mutator-assist steps that a compute-only loop never drives to
    // completion (no event-loop safepoint ever runs). Either way no collection
    // completes and RSS climbs unbounded. When old-gen reclaim pressure is what's
    // due — a rare event, gated by the ~32 MB growth / 48 MB absolute baseline, so
    // this never fires on the common nursery-churn path — run a direct full
    // mark-sweep to completion here, the same non-budgeted collection an explicit
    // `gc()` performs. The conservative native-stack scan (`force_full_scan`)
    // keeps it safe: anything still referenced from the stack/registers at this
    // allocation point (e.g. the temporary currently being built) is retained;
    // only genuinely unreachable old blocks are returned.
    //
    // ★ #7148 disposition: **keep, justified, observable — and add a precise
    // path that beats it to the punch.** The first attempt at this site was a
    // deferral like the nursery arm's. It was wrong, and the reasoning is kept
    // because the shape recurs.
    //
    // 1. **The headline RSS argument does not apply here.** A conservative scan
    //    costs +364%..+5371% `heap_used_bytes` on the ratchet probes *because
    //    it makes the copying minor ineligible* — `minor_cycles` → 0. This arm
    //    runs a FULL mark-sweep, which is non-moving with or without the scan.
    //    What the scan costs here is conservative *retention* for one cycle,
    //    not the loss of evacuation. Much smaller, and unmeasured.
    // 2. **Deferring it breaks a tested RSS guarantee.** #5476's regression test
    //    (`check_trigger_drives_old_reclaim_to_completion_without_host_stepping`)
    //    asserts that *a single* `gc_check_trigger` call — what every allocation
    //    does — drives the reclaim to completion, because the workload that
    //    motivated it is a compute-only loop that never reaches a host step. A
    //    deferral makes that "within one 32 MB growth quantum" instead, on the
    //    exact workload whose bug report was titled *RSS climbs unbounded*.
    //    Trading conservative retention for bounded-but-real extra old-gen
    //    residency is not obviously a win, and nothing here measured that it is.
    //
    // So this arm is unchanged, and `gc_safepoint_moving_minor` instead gained
    // the SAME full mark-sweep with precise roots (#7148). Programs that reach a
    // safepoint — every event-loop program — now get their old-gen reclaim
    // precisely and *no later* than before; nothing is delayed for anyone. The
    // fallback is attacked by adding a competing earlier precise path, not by
    // postponing the collection. `ConservativeScanSite::OldReclaimAllocPoint`
    // counts how often the alloc point still gets there first.
    //
    // Default-robustness (#7161 proposes flipping `PERRY_GC_MOVING_LOOP_POLLS`
    // OFF): this arm does not consult that gate at all, so it behaves
    // identically either way. The precise safepoint path is reached from the
    // microtask pump under `gc_moving_safepoint_enabled` (a different knob,
    // untouched by #7161) and from `js_gc_loop_safepoint` only while polls are
    // on. Polls off ⇒ the precise path is reached less often ⇒ this arm fires
    // more often ⇒ the census counter rises. Inert, not unsound.
    if !gc_budgeted_cycle_active()
        && matches!(
            gc_budgeted_due_trigger(),
            Some(BudgetedGcTrigger::OldReclaim)
        )
        && !GC_OLD_RECLAIM_IN_PROGRESS.with(Cell::get)
    {
        let _reentry = OldReclaimReentryGuard::enter();
        GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
        let _scan = super::roots::ManualGcScanGuard::force_full_scan(
            super::ConservativeScanSite::OldReclaimAllocPoint,
        );
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::OldGenBytes,
        ))
        .emit_after_current();
        return;
    }

    // The NURSERY-churn triggers (ArenaBytes / MallocCount) have the same
    // hole #5476 patched for OldReclaim: whenever synchronous-only root
    // scanners are registered — every compiled program, since codegen
    // registers sync scanners at startup — `gc_budgeted_start_blocked()`
    // holds for the life of the process, the mutator-assist step below can
    // never START a cycle, and allocation pressure accumulates without
    // bound (probe: 4M small allocations → 1.9 GB RSS with ZERO collection
    // cycles; the 64 MB arena trigger was due after the first ~64 blocks
    // and simply never fired). When the budgeted machinery is structurally
    // unavailable, run the direct synchronous minor the pre-budgeted
    // block-alloc trigger used to run, then re-baseline the arming trigger
    // below (the budgeted finisher never runs on this arm).
    // `gc_collect_minor_with_trigger` carries its own re-entrancy guard
    // (GC_FLAG_IN_ALLOC). `force_full_scan` mirrors the OldReclaim
    // arm: at an arbitrary allocation point a value mid-construction may
    // live only in registers, so the conservative native scan retains it —
    // which also makes copied-minor ineligible for THIS cycle, so the
    // non-moving minor runs (no relocation hazards at alloc points).
    // PERRY_GC_SCAVENGE (Phase-1 de-risking, OFF by default): when the budgeted
    // stepper is NOT blocked (all scanners budgeted), the nursery-churn triggers
    // fall through to the budgeted mutator-assist step below, which is
    // deliberately non-moving (`low_pause_non_moving = is_budgeted()`), so a
    // reallocation-heavy loop's minors free nothing. Route those triggers to the
    // direct (non-budgeted, atomic) minor here instead, so the collection is an
    // atomic minor that actually reclaims rather than a budgeted step that
    // does not. It is NOT an evacuating one: since #7682 the guard below is
    // unconditional, so a collection that happens here is always non-moving.
    // Scavenge is a PACING knob and nothing more; it used to also skip that
    // guard, which is the bug.
    // `gc_moving_loop_polls_enabled()`: the SOUND moving-nursery path. When loop
    // polls are on, entering this block routes nursery pressure AWAY from the
    // budgeted non-moving stepper (which would otherwise own it and free nothing
    // on reallocation loops) and into the defer arm below, which sets
    // GC_SAFEPOINT_PENDING and returns — the collection then runs as an
    // evacuating MOVING minor at the next precise loop back-edge safepoint
    // (`js_gc_loop_safepoint` → `gc_safepoint_moving_minor`), NOT here at the
    // register-imprecise alloc point. That deferral is the ONLY route by which
    // nursery pressure becomes a moving collection, and it is why the polls
    // flag and the scavenge flag are not interchangeable.
    //
    // #7280: that used to read "so it is sound by construction". IT IS NOT, and
    // the overclaim is the kind that stops the next person looking. What
    // deferring to `js_gc_loop_safepoint` buys is precise *codegen* roots — the
    // loop body has completed, so every live value the COMPILED frame holds is a
    // named local on the shadow stack. It buys nothing for a value parked in a
    // RUNTIME (Rust) frame, which the precise walk does not visit at all: no
    // shadow slot, no temp root, no registered scanner. A back-edge poll that
    // fires while `js_new_function_construct` is midway through a user
    // constructor body relocates the instance that helper is holding in a plain
    // `let`, and no safepoint's root set covers it. That was measured, not
    // argued — four reproducers in
    // `test-files/test_gap_gc_dynamic_construct_receiver_rooting.ts`, 200/200
    // iterations wrong per route before the `RuntimeHandleScope` routing in
    // `object/class_registry/construct.rs`. The correct statement is: the
    // loop-polls route makes the COLLECTION POINT precise; keeping runtime
    // frames rooted across it is a separate obligation, discharged by
    // `RuntimeHandleScope`, and every runtime helper that calls back into user
    // JS owes it.
    if !gc_budgeted_cycle_active()
        && (super::gc_scavenge_enabled()
            || gc_moving_loop_polls_enabled()
            || super::roots::registered_root_scanners_block_budgeted_gc())
    {
        let direct_kind = match gc_budgeted_due_trigger() {
            Some(BudgetedGcTrigger::ArenaBytes) => Some(GcTriggerKind::ArenaBytes),
            Some(BudgetedGcTrigger::MallocCount) => Some(GcTriggerKind::MallocCount),
            _ => None,
        };
        if let Some(kind) = direct_kind {
            // Phase 2/3: with moving mode on, DEFER this alloc-point collection
            // to the next precise-root safepoint (event-loop boundary or a
            // codegen loop back-edge poll) so the copying minor MOVES survivors
            // instead of the conservative non-moving minor running here at a
            // register-imprecise point. Safety valve: once the arena has grown
            // `gc_moving_defer_slack_dyn_bytes()` PAST the point at which the
            // collection was deferred (a mega-expression that reached no poll),
            // fall through and collect non-moving here so growth stays bounded.
            //
            // #7024: the allowance is measured from the deferral point, not
            // against an absolute arena size. The absolute cap shared
            // `budget_scaled(_, 1, 4, 2 MB)` with the trigger ceiling, so under
            // an explicit PERRY_GC_HEAP_LIMIT "a trigger is due" and "the
            // deferral is allowed" became exact complements and this branch was
            // dead — see `GC_MOVING_DEFER_SLACK_BYTES`.
            if gc_moving_loop_polls_enabled() {
                let arena_total = crate::arena::arena_total_bytes();
                let already_deferred = GC_SAFEPOINT_PENDING.with(Cell::get);
                let deferred_at =
                    already_deferred.then(|| GC_SAFEPOINT_DEFER_ARENA_BASE.with(Cell::get));
                if moving_defer_within_slack(
                    arena_total,
                    deferred_at,
                    gc_moving_defer_slack_dyn_bytes(),
                ) {
                    if !already_deferred {
                        GC_SAFEPOINT_DEFER_ARENA_BASE.with(|base| base.set(arena_total));
                        set_safepoint_pending(true);
                    }
                    return;
                }
                // The deferral never drained. The direct minor below IS the
                // collection that was owed, so retire the request — leaving it
                // pending would pin `GC_SAFEPOINT_DEFER_ARENA_BASE` at a stale,
                // already-exceeded baseline and disable deferral for the rest of
                // the process (the same "the branch is dead" shape as #7024).
                set_safepoint_pending(false);
            }
            let pre_in_use = crate::arena::arena_in_use_bytes();
            let pre_malloc_count = malloc_object_count();
            // THE ALLOC POINT IS REGISTER-IMPRECISE, SO THIS MINOR MUST NOT
            // MOVE. Unconditional, and the unconditionality is the fix for
            // #7682.
            //
            // Reaching this line means the collection is happening HERE, at an
            // arbitrary allocation point inside a half-built expression — not
            // at a declared safepoint. Neither root lowering describes that
            // point: the shadow stack only names values codegen has already
            // stored to a slot, and RS4GC only relocates values it can type as
            // `ptr addrspace(1)`, which a NaN-boxed `double` operand in an SSA
            // register is not. A value that exists ONLY in a register here is
            // therefore invisible to both, so an evacuating minor relocates the
            // object and leaves the register naming the pre-move address. The
            // conservative native-stack scan is what covers exactly that gap:
            // it retains such values AND makes the copying minor ineligible
            // (`CopiedMinorFallbackReason::ConservativeStack`), so the
            // non-moving in-place minor runs and nothing relocates.
            //
            // ★ #7148 disposition: **keep as the bounded valve, now counted.**
            // The deferral above is the primary path and is sound by
            // construction; reaching here means the slack expired without the
            // program touching a single loop back-edge poll or microtask-pump
            // boundary — a mega-expression, or a synchronous recursion, that
            // allocated `gc_moving_defer_slack_dyn_bytes()` past the deferral
            // point. There is no safepoint to defer to in that state, so the
            // answer to "what if pressure spikes before a safepoint is
            // reached" is: this arm runs, and it is the reason RSS stays
            // bounded. Making it *imprecise* instead (collecting without the
            // scan) is the one thing #7148 rules out — it would trade a cost
            // problem for a soundness problem.
            //
            // #7682 is that trade, shipped. `PERRY_GC_SCAVENGE` used to gate
            // this guard off, on the strength of a doc comment claiming the
            // flag was "OFF by default … for measurement only" and a body
            // comment saying it also "defers alloc-point collections to a
            // precise safepoint". Neither held in the shipped configuration:
            // the flag has been ON by default since #7056, and the deferral
            // above is gated on `gc_moving_loop_polls_enabled()`, which is OFF
            // by default since #7161. So the default build collected — and
            // EVACUATED — right here, with no scan and no deferral. A
            // tree-walking interpreter (`test_gap_gc_alloc_point_no_move.ts`)
            // then read a relocated heap string out of a stale register and
            // silently returned the wrong number.
            //
            // The scan-skip cannot be recovered by asking "is scavenge on?":
            // that question is about pacing, and the precondition being
            // asserted here is about the PRECISION OF THIS PROGRAM POINT,
            // which no pacing knob can change. Scavenge keeps its other job —
            // routing nursery-churn triggers to this direct minor instead of
            // the budgeted non-moving stepper — and the moving minor keeps
            // running at the precise safepoints, where the root set is real.
            let _scan = super::roots::ManualGcScanGuard::force_full_scan(
                super::ConservativeScanSite::NurseryChurnSlackValve,
            );
            let outcome = super::gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(kind));
            // Re-baseline the arming trigger after the direct minor, mirroring
            // `gc_finish_budgeted_cycle`. This arm is taken whenever
            // synchronous-only root scanners block the budgeted stepper — i.e.
            // every compiled program — so it, not the budgeted finisher, is the
            // completion path for nursery collections there. Emitting the
            // outcome without re-baselining left `GC_NEXT_TRIGGER_BYTES` /
            // `GC_NEXT_MALLOC_TRIGGER` at the value that armed THIS collection.
            // The non-moving minor reclaims dead objects into per-block free
            // lists but does not lower `arena_total` (committed blocks), so a
            // workload holding a large live set above the trigger — e.g.
            // building an object graph that stays reachable while churning
            // transient allocations — keeps `gc_budgeted_due_trigger` reporting
            // the same trigger as due, and every fresh block re-arms a whole-
            // arena mark/sweep. That is one O(arena) collection per block
            // allocated: O(n^2) in the graph size, a ~100% CPU stall with a
            // bounded live set that never makes progress. The finish helpers
            // raise the trigger past the retained set (adapting the step),
            // exactly as the budgeted and full-GC paths do on completion.
            match kind {
                GcTriggerKind::MallocCount => {
                    gc_finish_malloc_trigger_collection(pre_malloc_count, outcome);
                }
                _ => {
                    gc_finish_arena_trigger_collection(pre_in_use, outcome);
                }
            }
            return;
        }
    }

    if !gc_budgeted_cycle_active() && gc_budgeted_due_trigger().is_none() {
        return;
    }

    let _ = gc_mutator_assist_step_work_units_inner_with_progress(
        gc_mutator_assist_scaled_work_units(),
        GcProgressKind::MutatorAssist,
    );
}

/// Debt-proportional assist pacing (#6180 Stage 2, measured 2026-07-10).
///
/// A FIXED per-assist budget lets a tight allocation loop outrun the
/// collector: on a 10M-allocation ring benchmark the budgeted cycle NEVER
/// completed (0 collections vs the synchronous default's 7) and RSS grew
/// unbounded (6-22× the synchronous collector's) — the cycle crawled at 256
/// units per arena-block allocation against a heap growing by ~16k objects
/// per block. `GcDebtSnapshot` already measured exactly this shortfall but
/// fed telemetry only.
///
/// Scale the budget linearly with the measured debt instead. Debt is how far
/// allocation has run past the armed triggers, so between two block-alloc
/// assists it grows by ~one block while the budget grows with total debt —
/// the controller self-stabilizes at the equilibrium where collection keeps
/// pace with allocation, instead of falling behind forever.
///
/// No explicit cap is needed: the budget is a CEILING on work, not a pause
/// floor — `GcCycleState::step` stops the moment the cycle completes, and a
/// cycle's remaining work is bounded by the heap. The worst case is therefore
/// finishing the whole cycle in one assist: exactly the pause the synchronous
/// collector takes on every collection today. Under extreme allocation
/// pressure incremental degrades gracefully toward synchronous behavior
/// rather than toward unbounded memory.
pub(super) fn gc_mutator_assist_scaled_work_units() -> usize {
    let debt = GcDebtSnapshot::current();
    let arena_units = (debt.arena_debt_bytes / GC_ASSIST_DEBT_BYTES_PER_WORK_UNIT) as usize;
    // Malloc-registry work is per-object (mark/sweep touches each header
    // once), so malloc debt converts 1:1.
    let malloc_units = debt.malloc_debt_objects as usize;
    GC_MUTATOR_ASSIST_WORK_UNITS
        .saturating_add(arena_units)
        .saturating_add(malloc_units)
}

pub const JS_GC_STEP_STATUS_IDLE: u32 = 0;
pub const JS_GC_STEP_STATUS_ACTIVE: u32 = 1;
pub const JS_GC_STEP_STATUS_COMPLETED: u32 = 2;
pub const JS_GC_STEP_STATUS_SKIPPED: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct JsGcStepResult {
    pub status: u32,
    pub phase: u32,
    pub collection_kind: u32,
    pub trigger_kind: u32,
    pub active: u32,
    pub completed: u32,
    pub arena_debt_bytes: u64,
    pub malloc_debt_objects: u64,
    pub old_reclaim_debt_bytes: u64,
}

#[derive(Clone, Copy)]
enum BudgetedGcRebaseline {
    ArenaBytes { pre_in_use: usize },
    MallocCount { pre_count: usize },
    OldReclaim,
}

struct BudgetedGcCycle {
    state: GcCycleState,
    trigger_kind: GcTriggerKind,
    collection_kind: GcCollectionKind,
    rebaseline: BudgetedGcRebaseline,
}

#[derive(Clone, Copy, Debug)]
enum BudgetedGcTrigger {
    OldReclaim,
    ArenaBytes,
    MallocCount,
}

thread_local! {
    static GC_BUDGETED_CYCLE: RefCell<Option<BudgetedGcCycle>> = const { RefCell::new(None) };
    static GC_BUDGETED_CYCLE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static GC_BUDGETED_STEP_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn gc_budgeted_cycle_active() -> bool {
    GC_BUDGETED_CYCLE_ACTIVE.with(Cell::get)
}

fn gc_budgeted_start_blocked() -> bool {
    GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || registered_root_scanners_block_budgeted_gc()
}

fn gc_budgeted_resume_blocked() -> bool {
    GC_FLAGS.with(|f| f.get()) & GC_FLAG_SUPPRESSED != 0
        || gc_blocked_by_unsafe_zone()
        || GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0)
        || registered_root_scanners_block_budgeted_gc()
}

pub(super) fn gc_old_reclaim_debt_bytes(old_in_use: usize, baseline: usize) -> u64 {
    let trigger = if baseline < gc_old_gen_reclaim_threshold_dyn_bytes() {
        gc_old_gen_reclaim_threshold_dyn_bytes()
    } else {
        // Same proportional band as `old_reclaim_pressure_due` (#7592) —
        // debt and dueness must share one trigger or they diverge.
        baseline.saturating_add(gc_old_reclaim_growth_band_bytes(baseline))
    };
    old_in_use.saturating_sub(trigger) as u64
}

fn gc_budgeted_due_trigger() -> Option<BudgetedGcTrigger> {
    let old_pending = GC_OLD_RECLAIM_PENDING.with(Cell::get);
    // #6010: external Map/Set side-buffer bytes escalate to OldReclaim too.
    let old_in_use =
        old_gen_reclaimable_pressure_bytes().saturating_add(external_side_live_bytes());
    let old_baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    if old_pending || old_reclaim_pressure_due(old_in_use, old_baseline) {
        return Some(BudgetedGcTrigger::OldReclaim);
    }

    // Two separately-scoped arena arms (see `young_scavenge_cap_due` for why
    // they must not share a basis): the adaptive base trigger against the
    // whole arena, and the scavenge nursery cap against the young generation
    // only.
    let total = crate::arena::arena_total_bytes();
    if total >= next_arena_trigger_base() {
        return Some(BudgetedGcTrigger::ArenaBytes);
    }
    if young_scavenge_cap_due() {
        return Some(BudgetedGcTrigger::ArenaBytes);
    }

    let malloc_count = malloc_object_count();
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
    if malloc_count >= next_malloc_trigger {
        return Some(BudgetedGcTrigger::MallocCount);
    }

    None
}

/// Phase 1 of the moving-GC project: run a copying (moving) minor at a
/// precise-root safepoint — the outermost microtask-pump boundary, where the
/// JS stack has fully unwound so no live heap pointer sits in an unspilled
/// register. Unlike the alloc-point nursery-churn arm, NO `force_full_scan` is
/// taken: `conservative_stack_scan_decision()` stays `SkipDisabled`, so the
/// copying minor is eligible with precise, rewritable roots and actually MOVES
/// (compacting, O(survivors), no sweep) instead of falling back to the
/// non-moving minor. Trigger detection + re-baseline mirror the nursery-churn
/// arm; this is purely additive (the alloc-point fallback is untouched) and
/// gated by `gc_moving_safepoint_enabled` (**default ON**; the kill switch is
/// `PERRY_GC_MOVING_SAFEPOINT=0`).
///
/// Returns whether the safepoint was HANDLED — false when an entry guard
/// blocked it (mid-allocation, suppressed, unsafe FFI zone, non-zero root-lock
/// depth, active budgeted cycle), true otherwise, including when it was handled
/// and nothing was due. A blocked safepoint consumes no schedule slot, so the
/// caller must not charge it a pacing stride either.
pub(crate) fn gc_safepoint_moving_minor() -> bool {
    // Same start guards the budgeted collector uses, minus the (here
    // irrelevant) scanner block: never collect mid-allocation, inside a
    // runtime handle scope, in an unsafe FFI zone, or during a budgeted cycle.
    let flags = GC_FLAGS.with(|f| f.get());
    let in_alloc = flags & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0;
    let unsafe_zone = gc_blocked_by_unsafe_zone();
    let root_lock = GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0);
    let budgeted = gc_budgeted_cycle_active();
    if in_alloc || unsafe_zone || root_lock || budgeted {
        // Blocked right now — leave GC_SAFEPOINT_PENDING set so the next poll
        // retries; do not clear it here.
        return false;
    }
    // We are handling this safepoint (collect or find nothing due): clear the
    // deferral flag set by the alloc-point arm (Phase 2/3).
    //
    // #7154 tooling: this is also the one place the seeded GC-schedule counter
    // advances — after the entry guards, so a safepoint that could not have
    // collected never consumes a schedule slot, and once per handled safepoint
    // whichever arm reached us (loop back-edge poll or microtask-pump boundary).
    // Inert (one cached-`Option` load) unless `PERRY_GC_SCHEDULE_SEED` is set.
    let scheduled = super::schedule::schedule_tick();
    // `set_safepoint_pending`, not a raw `.set(false)`: since #7735 the pending
    // flag is mirrored into the poll arming word, and clearing it behind the
    // mirror would leave the back-edge poll armed forever.
    set_safepoint_pending(false);
    let _declared = DeclaredSafepointGuard::enter();
    let kind = match gc_budgeted_due_trigger() {
        Some(BudgetedGcTrigger::ArenaBytes) => GcTriggerKind::ArenaBytes,
        Some(BudgetedGcTrigger::MallocCount) => GcTriggerKind::MallocCount,
        // ★ #7148: old-gen reclaim used to be the alloc-point arm's business
        // exclusively — it ran a direct full mark-sweep behind a forced
        // conservative scan, because at an allocation point that scan is what
        // makes it sound. Here it is not needed: this is the same precise-root
        // safepoint the nursery minor uses, so run the identical full
        // mark-sweep with `SkipDisabled` roots. The alloc-point arm keeps only
        // the bounded slack valve.
        Some(BudgetedGcTrigger::OldReclaim) => {
            if GC_OLD_RECLAIM_IN_PROGRESS.with(Cell::get) {
                return true;
            }
            let _reentry = OldReclaimReentryGuard::enter();
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
            // No `force_full_scan`: roots are precise at this safepoint.
            gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                GcTriggerKind::OldGenBytes,
            ))
            .emit_after_current();
            super::record_safepoint_drain(super::SafepointDrainKind::OldReclaim);
            return true;
        }
        _ => {
            // No nursery-pressure trigger is due — nothing to collect here,
            // unless the seeded schedule (#7154 tooling) selected this
            // safepoint, in which case the point of the mode is to collect
            // anyway so an unrooted value moves on its first exposure (at the
            // default `PERRY_GC_SCHEDULE_RATE` of 5% that is one handled
            // safepoint in twenty, at rate 1 every one of them).
            // `gc_force_evacuate_enabled()` is true whenever a seed resolved,
            // so this minor MOVES survivors rather than sweeping in place.
            if !scheduled {
                return true;
            }
            super::schedule::note_schedule_forced_collection();
            GcTriggerKind::ArenaBytes
        }
    };
    let pre_in_use = crate::arena::arena_in_use_bytes();
    let pre_malloc_count = malloc_object_count();
    // No `force_full_scan`: roots are precise at this safepoint.
    let outcome = super::gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(kind));
    match kind {
        GcTriggerKind::MallocCount => {
            gc_finish_malloc_trigger_collection(pre_malloc_count, outcome);
        }
        _ => {
            gc_finish_arena_trigger_collection(pre_in_use, outcome);
        }
    }
    // The live-subject counter for every deferral gate: a test that asserts
    // "the conservative valve did not fire" is vacuous unless it can also show
    // the precise collection that replaced it actually ran (CLAUDE.md, four
    // ways a gate cannot fail — #4, the gate runs but its subject never did).
    super::record_safepoint_drain(super::SafepointDrainKind::NurseryMinor);
    true
}

/// The ONLY writer of `GC_SAFEPOINT_PENDING`.
///
/// The flag has a process-global shadow, `gc::poll_arm::PERRY_GC_POLL_ARMED`,
/// because the back-edge poll's fast path may not read a thread-local — on
/// Darwin that is a call to `_tlv_get_addr`, and at 20 M back-edges per
/// `churn_alloc.ts` run it cost 3 ns each (see `gc/poll_arm.rs`). Keeping the
/// two in step is this function's whole job: the `Cell` is the truth for THIS
/// thread, the counter is a conservative superset over all of them, and only
/// transitions move it, so the count is threads-with-a-deferral rather than
/// deferral-events.
pub(crate) fn set_safepoint_pending(pending: bool) {
    GC_SAFEPOINT_PENDING.with(|flag| {
        if flag.get() == pending {
            return;
        }
        flag.set(pending);
        if pending {
            super::arm_poll();
        } else {
            super::disarm_poll();
        }
    });
}

/// Phase 2 of the moving-GC project: codegen emits a call to this at loop
/// back-edges. At a back-edge the loop-body expression has completed, so no
/// heap value lives in an unspilled register (every live value is a named local
/// on the shadow stack): a precise-root safepoint. If moving mode is on and an
/// alloc-point nursery trigger deferred a collection (`GC_SAFEPOINT_PENDING`),
/// drain it here so the copying minor MOVES survivors.
///
/// **The `armed` load is the entire function on the overwhelmingly common
/// path**, and that is a deliberate structure rather than a micro-optimisation.
/// Every allocating loop back-edge in the program lands here — 20 million times
/// in `bench/churn_alloc.ts` — so this is a per-iteration cost of the language,
/// paid whether or not any collection is ever due. #7721 turned the polls on by
/// default (correctly: they are the only precise nursery-collection point a
/// compute-only program reaches) with the body below still doing two `OnceLock`
/// acquire loads, an unconditional atomic increment and a thread-local read,
/// and that cost `churn_alloc` 0.36 s → 0.42, `push_cls` 0.34 → 0.40 and
/// `push_num` 0.13 → 0.17. `gc/poll_arm.rs` carries the measurement and the
/// invariant; `PERRY_GC_POLL_ARMED == 0` is a proof there is nothing to do.
///
/// Codegen normally makes even this call disappear — it loads the same word
/// inline and branches around the call — so reaching this body at all means
/// either the word was armed or the module came from a path that emits the
/// bare call. Both must still work, which is why the check is repeated here
/// rather than delegated to the compiler.
#[no_mangle]
pub extern "C" fn js_gc_loop_safepoint() {
    if !super::poll_armed() {
        return;
    }
    js_gc_loop_safepoint_armed();
}

/// Out of line so the hot entry point above stays a load, a compare and a
/// return — no frame, no spills.
#[inline(never)]
fn js_gc_loop_safepoint_armed() {
    // Releases the startup seed unless a resolved seed wants the poll kept
    // reachable. Must run before the opt-in check below: a build with the polls
    // killed still has to get the word back to zero, or every back-edge keeps
    // paying for the call.
    super::resolve_poll_seed();
    if !gc_moving_loop_polls_enabled() {
        return;
    }
    // #7604: the only reliable answer to "did the compile-time half take
    // effect". Exhaustive exactly under a resolved seed, which is where
    // `schedule_verdict` reads it — see `resolve_poll_seed` and
    // `loop_polls_reached`.
    super::note_loop_poll_reached();
    // The schedule work all sits inside the `!pending` branch on purpose: a
    // default (mode-off) build reaches exactly the same one cached-bool read
    // and return it did before, and the deferral-drain path below is untouched.
    if !GC_SAFEPOINT_PENDING.with(Cell::get) {
        // The seeded schedule (#7154 tooling) considers polls the deferral flag
        // would skip, so it needs this gate bypassed — and needs it here rather
        // than at the decision point: a schedule cannot select a safepoint this
        // gate already returned from. The decision itself, and the ordinal tick
        // it is a function of, happen inside `gc_safepoint_moving_minor`, past
        // the entry guards. A resolved seed cannot conjure a poll codegen never
        // emitted, so the `gc_moving_loop_polls_enabled()` gate above still
        // applies — see `gc/schedule.rs`.
        if !super::schedule::gc_schedule_enabled() {
            return;
        }
        // ★ #7728, ported: "polls", not "EVERY poll". A poll only becomes a
        // candidate the seed can select once `PERRY_GC_SCHEDULE_ALLOC_KB` of
        // new nursery material has accumulated; unpaced, the rate-1 endpoint
        // cost ~511 µs per loop iteration and turned a 19 s program into a
        // 24-minute one. The stride is a bound on candidates, not a heuristic.
        if !super::schedule::schedule_poll_collection_due(
            crate::arena::copying_from_space_in_use_bytes(),
        ) {
            super::schedule::note_schedule_poll_paced();
            return;
        }
        // Rearm ONLY when the safepoint was handled. A blocked safepoint
        // consumes no schedule slot (see `schedule_tick`'s placement past the
        // entry guards), so charging it a full stride would silently drop the
        // realised density below the requested rate — and a loop that polls
        // while a guard is held would lose candidate after candidate with
        // nothing in the exit summary to say so.
        //
        // Rearm from the level measured AFTER the safepoint, so the next
        // candidate costs a full stride of new allocation on top of whatever
        // survived — see `gc/schedule.rs` for why this is a high-water mark and
        // not a delta.
        if gc_safepoint_moving_minor() {
            super::schedule::note_schedule_poll_collection(
                crate::arena::copying_from_space_in_use_bytes(),
            );
        }
        return;
    }
    gc_safepoint_moving_minor();
}

struct BudgetedGcStepGuard;

impl BudgetedGcStepGuard {
    fn enter() -> Option<Self> {
        GC_BUDGETED_STEP_ACTIVE.with(|active| {
            if active.get() {
                None
            } else {
                active.set(true);
                Some(Self)
            }
        })
    }
}

impl Drop for BudgetedGcStepGuard {
    fn drop(&mut self) {
        GC_BUDGETED_STEP_ACTIVE.with(|active| active.set(false));
    }
}

fn gc_start_budgeted_full_cycle(
    trigger_kind: GcTriggerKind,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    let mut state = GcCycleState::new_full(GcTriggerSnapshot::capture(trigger_kind));
    state.set_progress_kind(progress_kind);
    BudgetedGcCycle {
        collection_kind: state.collection_kind(),
        state,
        trigger_kind,
        rebaseline,
    }
}

fn gc_start_budgeted_minor_fallback_cycle(
    trigger_kind: GcTriggerKind,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    gc_start_budgeted_minor_fallback_cycle_with_snapshot(
        GcTriggerSnapshot::capture(trigger_kind),
        rebaseline,
        progress_kind,
    )
}

fn gc_start_budgeted_minor_fallback_cycle_with_snapshot(
    trigger: GcTriggerSnapshot,
    rebaseline: BudgetedGcRebaseline,
    progress_kind: GcProgressKind,
) -> BudgetedGcCycle {
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    if let Some(trace) = trace.as_mut() {
        trace.progress_kind = progress_kind;
    }
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    clear_mark_seeds();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let low_pause_non_moving = progress_kind.is_budgeted();
    // #7611: `low_pause_non_moving` is now the ONLY controller of this flag —
    // the `PERRY_GEN_GC_EVACUATE` conjunct that used to be here was deleted as
    // an untested configuration, and this arm is the one with a behavioural
    // test (`budgeted_low_pause_minor_does_not_evacuate`).
    let evacuation_policy_allowed = !low_pause_non_moving;
    let force_evacuation = !low_pause_non_moving && gc_force_evacuate_enabled();
    let evacuation_policy_disabled_reason = if low_pause_non_moving {
        EVACUATION_POLICY_LOW_PAUSE_NON_MOVING_REASON
    } else {
        EVACUATION_POLICY_DISABLED_REASON
    };
    let old_page_selection = if evacuation_policy_allowed && old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    let old_page_source_blocks =
        crate::arena::old_arena_source_blocks_for_pages(&old_page_selection.pages);
    let state = GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        progress_kind,
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        evacuation_policy_disabled_reason,
        old_page_selection,
        old_page_source_blocks,
    );
    BudgetedGcCycle {
        collection_kind: state.collection_kind(),
        state,
        trigger_kind: trigger.kind,
        rebaseline,
    }
}

#[cfg(test)]
pub(super) fn test_start_budgeted_minor_fallback_state_with_trace(
    trigger_kind: GcTriggerKind,
    progress_kind: GcProgressKind,
) -> GcCycleState {
    let trigger = GcTriggerSnapshot {
        kind: trigger_kind,
        steps_before: Some(GcStepSnapshot::current()),
    };
    let cycle = gc_start_budgeted_minor_fallback_cycle_with_snapshot(
        trigger,
        BudgetedGcRebaseline::ArenaBytes {
            pre_in_use: crate::arena::arena_in_use_bytes(),
        },
        progress_kind,
    );
    cycle.state
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
pub(super) fn major_pacing_config() -> (usize, usize) {
    use std::sync::OnceLock;
    static CONFIG: OnceLock<(usize, usize)> = OnceLock::new();
    let &(floor_bytes, growth_num) = CONFIG.get_or_init(|| {
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
pub(super) fn arena_growth_full_escalation_due() -> bool {
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
    let baseline = GC_LAST_FULL_ARENA_IN_USE_BYTES.with(|bytes| bytes.get());
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
pub(super) fn major_pacing_escalation_threshold_for(
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

fn gc_start_budgeted_cycle_for_pressure(progress_kind: GcProgressKind) -> Option<BudgetedGcCycle> {
    let trigger = gc_budgeted_due_trigger()?;
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    Some(match trigger {
        BudgetedGcTrigger::OldReclaim => {
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
            gc_start_budgeted_full_cycle(
                GcTriggerKind::OldGenBytes,
                BudgetedGcRebaseline::OldReclaim,
                progress_kind,
            )
        }
        BudgetedGcTrigger::ArenaBytes => {
            let rebaseline = BudgetedGcRebaseline::ArenaBytes {
                pre_in_use: crate::arena::arena_in_use_bytes(),
            };
            // Major-GC pacing: escalate to a full when arena live-bytes grew
            // past K× the last full's live set — the non-moving minor can't free
            // array-growth forwarding stubs (see `arena_growth_full_escalation_due`).
            if gen_gc_enabled() && !arena_growth_full_escalation_due() {
                gc_start_budgeted_minor_fallback_cycle(
                    GcTriggerKind::ArenaBytes,
                    rebaseline,
                    progress_kind,
                )
            } else {
                gc_start_budgeted_full_cycle(GcTriggerKind::ArenaBytes, rebaseline, progress_kind)
            }
        }
        BudgetedGcTrigger::MallocCount => {
            let rebaseline = BudgetedGcRebaseline::MallocCount {
                pre_count: malloc_object_count(),
            };
            // Major-GC pacing (malloc-count trigger twin of the ArenaBytes branch).
            if gen_gc_enabled() && !arena_growth_full_escalation_due() {
                gc_start_budgeted_minor_fallback_cycle(
                    GcTriggerKind::MallocCount,
                    rebaseline,
                    progress_kind,
                )
            } else {
                gc_start_budgeted_full_cycle(GcTriggerKind::MallocCount, rebaseline, progress_kind)
            }
        }
    })
}

fn gc_step_result(
    status: u32,
    phase: u32,
    collection_kind: u32,
    trigger_kind: u32,
    active: bool,
    completed: bool,
) -> JsGcStepResult {
    let debt = GcDebtSnapshot::current();
    JsGcStepResult {
        status,
        phase,
        collection_kind,
        trigger_kind,
        active: u32::from(active),
        completed: u32::from(completed),
        arena_debt_bytes: debt.arena_debt_bytes,
        malloc_debt_objects: debt.malloc_debt_objects,
        old_reclaim_debt_bytes: debt.old_reclaim_debt_bytes,
    }
}

fn gc_idle_step_result() -> JsGcStepResult {
    gc_step_result(JS_GC_STEP_STATUS_IDLE, 0, 0, 0, false, false)
}

fn gc_cycle_step_result(status: u32, cycle: &BudgetedGcCycle, completed: bool) -> JsGcStepResult {
    gc_step_result(
        status,
        cycle.state.phase().ffi_code(),
        cycle.collection_kind.ffi_code(),
        cycle.trigger_kind.ffi_code(),
        !completed,
        completed,
    )
}

fn gc_budgeted_status_result() -> JsGcStepResult {
    if !gc_budgeted_cycle_active() {
        return gc_idle_step_result();
    }

    let result = GC_BUDGETED_CYCLE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|cycle| gc_cycle_step_result(JS_GC_STEP_STATUS_ACTIVE, cycle, false))
    });
    match result {
        Some(result) => result,
        None => {
            GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
            gc_idle_step_result()
        }
    }
}

fn gc_budgeted_skipped_result() -> JsGcStepResult {
    if !gc_budgeted_cycle_active() {
        return gc_step_result(JS_GC_STEP_STATUS_SKIPPED, 0, 0, 0, false, false);
    }

    GC_BUDGETED_CYCLE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|cycle| gc_cycle_step_result(JS_GC_STEP_STATUS_SKIPPED, cycle, false))
            .unwrap_or_else(|| gc_step_result(JS_GC_STEP_STATUS_SKIPPED, 0, 0, 0, false, false))
    })
}

fn gc_finish_budgeted_cycle(mut cycle: BudgetedGcCycle) -> JsGcStepResult {
    let outcome = cycle
        .state
        .take_outcome()
        .expect("completed budgeted GC cycle must produce an outcome");
    match cycle.rebaseline {
        BudgetedGcRebaseline::ArenaBytes { pre_in_use } => {
            gc_finish_arena_trigger_collection(pre_in_use, outcome);
        }
        BudgetedGcRebaseline::MallocCount { pre_count } => {
            gc_finish_malloc_trigger_collection(pre_count, outcome);
        }
        BudgetedGcRebaseline::OldReclaim => {
            outcome.emit_after_current();
        }
    }
    GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
    gc_step_result(
        JS_GC_STEP_STATUS_COMPLETED,
        GcCyclePhase::Complete.ffi_code(),
        cycle.collection_kind.ffi_code(),
        cycle.trigger_kind.ffi_code(),
        false,
        true,
    )
}

enum BudgetedStepOutcome {
    Result(JsGcStepResult),
    Completed(BudgetedGcCycle),
}

/// Finish any parked budgeted cycle through its own machinery before a
/// synchronous (direct/manual/emergency) collection constructs a fresh
/// `GcCycleState`. Two cycles must never be alive at once: they share
/// `GC_FLAG_MARKED`, the mark-seed queue, the incremental-barrier TLS, and
/// the allocate-black birth-flag lifecycle — a synchronous full mark-sweep
/// landing mid-budgeted-cycle erases the parked cycle's marks, so its
/// eventual sweep frees whatever the interloper didn't re-mark (measured as
/// the manual-`gc()` SIGSEGV escalation of the #6224 stress). One unbounded
/// step drives the parked cycle to completion via the normal finisher.
pub(super) fn gc_drain_active_budgeted_cycle() {
    if !gc_budgeted_cycle_active() {
        return;
    }
    // One `step()` call advances only the CURRENT phase (even with an
    // unbounded budget) — completing the whole cycle takes one call per
    // remaining phase, exactly like `run_to_completion`'s loop. Bound the
    // loop defensively: 8 phases, and a blocked stepper (suppression /
    // unsafe zone / reentrancy guard) returns without progress — bail then
    // rather than spin.
    for _ in 0..64 {
        let result = gc_budgeted_step_work_units_inner(usize::MAX);
        if !gc_budgeted_cycle_active() {
            return;
        }
        if result.status == JS_GC_STEP_STATUS_SKIPPED {
            break;
        }
    }
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        eprintln!("[gc-drain] WARNING: parked budgeted cycle could not be drained before synchronous collection");
    }
}

fn gc_budgeted_step_work_units_inner(work_units: usize) -> JsGcStepResult {
    gc_budgeted_step_work_units_inner_with_progress(work_units, GcProgressKind::NormalIncremental)
}

fn gc_budgeted_step_work_units_inner_with_progress(
    work_units: usize,
    start_progress_kind: GcProgressKind,
) -> JsGcStepResult {
    if work_units == 0 {
        return gc_budgeted_status_result();
    }

    let Some(_guard) = BudgetedGcStepGuard::enter() else {
        return gc_budgeted_skipped_result();
    };

    if !gc_budgeted_cycle_active() {
        if gc_budgeted_due_trigger().is_none() {
            return gc_idle_step_result();
        }
        if gc_budgeted_start_blocked() {
            return gc_budgeted_skipped_result();
        }
        let cycle = gc_start_budgeted_cycle_for_pressure(start_progress_kind)
            .expect("budgeted GC pressure was observed before starting cycle");
        GC_BUDGETED_CYCLE.with(|slot| {
            *slot.borrow_mut() = Some(cycle);
        });
        GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(true));
    }

    if gc_budgeted_resume_blocked() {
        return gc_budgeted_skipped_result();
    }

    let outcome = GC_BUDGETED_CYCLE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(cycle) = slot.as_mut() else {
            GC_BUDGETED_CYCLE_ACTIVE.with(|active| active.set(false));
            return BudgetedStepOutcome::Result(gc_idle_step_result());
        };

        let step = cycle.state.step(GcWorkBudget::bounded(work_units));
        if step.completed {
            BudgetedStepOutcome::Completed(slot.take().expect("active budgeted GC cycle exists"))
        } else {
            BudgetedStepOutcome::Result(gc_cycle_step_result(
                JS_GC_STEP_STATUS_ACTIVE,
                cycle,
                false,
            ))
        }
    });

    match outcome {
        BudgetedStepOutcome::Result(result) => result,
        BudgetedStepOutcome::Completed(cycle) => gc_finish_budgeted_cycle(cycle),
    }
}

/// Allocation-side mutator assist: a bounded (`GC_MUTATOR_ASSIST_WORK_UNITS`)
/// slice of GC work performed from the allocator (`gc_check_trigger`) rather
/// than from a host safepoint. Assists drive **every** resumable phase of the
/// active budgeted cycle, exactly like a host safepoint — the only difference
/// is the smaller per-step budget carried in `work_units`. This is what closes
/// the incremental sweep-parking hole (#6180): a pure compute loop that never
/// reaches the event pump still finishes the cycle (and disables the mark
/// barrier / reclaims memory) purely from the allocations it keeps making, so
/// RSS stays bounded. `AtomicFinalizeSubphase::WeakProcessing` is the one
/// phase step that is not yet internally sliced, so the assist that lands on it
/// runs it whole — a single O(live-weak-holders) spike per cycle; slicing it is
/// a tracked follow-up (pause-quality, not correctness).
fn gc_mutator_assist_step_work_units_inner_with_progress(
    work_units: usize,
    start_progress_kind: GcProgressKind,
) -> JsGcStepResult {
    gc_budgeted_step_work_units_inner_with_progress(work_units, start_progress_kind)
}

pub(crate) fn gc_runtime_safepoint() -> JsGcStepResult {
    let budget = gc_progress_contract().budget_for(GcProgressKind::NormalIncremental);
    let Some(work_units) = budget.work_units else {
        return gc_budgeted_status_result();
    };
    gc_budgeted_step_work_units_inner_with_progress(work_units, GcProgressKind::NormalIncremental)
}

fn write_gc_step_result(out: *mut JsGcStepResult, result: JsGcStepResult) -> u32 {
    if !out.is_null() {
        unsafe {
            *out = result;
        }
    }
    result.status
}

#[no_mangle]
pub extern "C" fn js_gc_step_work_units(work_units: u64, out: *mut JsGcStepResult) -> u32 {
    let work_units = usize::try_from(work_units).unwrap_or(usize::MAX);
    let result = gc_budgeted_step_work_units_inner(work_units);
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_step_us(budget_us: u64, out: *mut JsGcStepResult) -> u32 {
    if budget_us == 0 {
        let result = gc_budgeted_status_result();
        return write_gc_step_result(out, result);
    }

    let start = Instant::now();
    let mut result = gc_budgeted_step_work_units_inner(1);
    while result.status == JS_GC_STEP_STATUS_ACTIVE
        && start.elapsed().as_micros() < u128::from(budget_us)
    {
        result = gc_budgeted_step_work_units_inner(1);
    }
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_step_status(out: *mut JsGcStepResult) -> u32 {
    let result = gc_budgeted_status_result();
    write_gc_step_result(out, result)
}

#[no_mangle]
pub extern "C" fn js_gc_safepoint(out: *mut JsGcStepResult) -> u32 {
    let result = gc_runtime_safepoint();
    write_gc_step_result(out, result)
}

/// Counter tracking "native work holds JSValue roots we can't scan" state.
/// This is for narrow FFI sections where a worker thread may temporarily
/// hold runtime values on a stack the main-thread GC cannot see. Long-lived
/// server adapters should instead queue plain Rust data, allocate JS values
/// on the main thread, and register mutable root scanners for stored callback
/// slots.
///
/// When > 0, the conservative main-thread stack scanner can't see all live
/// roots — collecting could free objects still referenced from worker-thread
/// stacks and SEGV on next access.
///
/// Issue #31: gc() from setInterval in a Fastify+WebSocket server crashed
/// within 60s of the first tick because WS worker threads held live refs
/// to message payload strings on their stacks. This counter lets stdlib
/// features signal "please skip user-initiated gc() while I'm running"
/// without a full stop-the-world mutex.
pub static GC_UNSAFE_ZONES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// One-shot warning so we don't spam stderr on every tick.
pub(super) static GC_UNSAFE_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Manual GC trigger (callable from TypeScript as `gc()`). Skipped when
/// worker threads are active (see GC_UNSAFE_ZONES).
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    if manual_gc_blocked_by_unsafe_zone() {
        return;
    }
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Manual)) {
        return;
    }
    manual_gc_collect_now();
}

/// Run an explicit (`gc()`) full collection, with **precise roots** — the same
/// root set every automatic collection in a production binary already uses
/// (`conservative_stack_scan_mode()` resolves to `Auto`, i.e. `SkipDisabled`).
///
/// ★ #7558: this site used to take `ManualGcScanGuard::force_full_scan`. It no
/// longer does, and the removal is deliberate rather than incidental — read
/// this before adding one back.
///
/// **What the scan was for.** #4977: `const keep = {…}; gc(); keep.nested.deep`
/// read dangling-pointer garbage, because a module-init/top-level local was
/// held only as a native-stack alloca that neither the shadow stack nor the
/// module-var scanners covered. Forcing the conservative scan retained it. That
/// was a *workaround for a precise-rooting hole*, applied at the one collection
/// site that could be made to hide it — not a statement that `gc()` needs a
/// different root set from every other collection.
///
/// **Why it is no longer needed.** The hole was closed by the 2026-06→08
/// rooting campaign, from a different direction: pointer-typed locals get a
/// persistent shadow slot bound in the function-entry setup (#6968's
/// `expr::scalar_slot_root`, #6951/#6972's object-literal rooting), module-level
/// bindings are `@perry_global_*` cells registered with
/// `js_gc_register_global_root`, and `scripts/gc_root_dominance_check.py` gates
/// the invariant that a root store must dominate every collection point — with
/// an **empty** allowlist. `js_gc_collect` is a collection point by that
/// invariant like any other; nothing about it is special. #4977's own repro
/// (`test-files/test_issue_4977_gc_toplevel_locals.ts`) prints the right answer
/// with the scan disabled.
///
/// **What it cost.** A conservative scan retains whatever the native stack
/// happens to look like a pointer to, so the reading every retained-heap number
/// in this project is taken through — `process.memoryUsage()` after `gc()` —
/// carried a stack-residue tax. Measured with `gc_ratchet.py classify` on
/// `main` at `961777904`: non-zero on **nine of the twelve** ratchet probes,
/// and 28.63% / 28.24% / 29.71% / 31.03% of reported retention on
/// `01_nursery_churn`, `05_closure_capture`, `06_string_retention` and
/// `11_collect_at_depth` respectively (13.80%, 8,273,888 bytes, on
/// `12_large_live_set` — the case #7558 was filed for was the *smallest* of the
/// four). It was also non-deterministic run to run, because stack residue is,
/// which is why `benchmarks/gc_ratchet` had to stop gating that cell (#7554)
/// and why retention rows were unbelievable without a manual `classify`
/// cross-check (#7559). And it made this path non-moving, which is why
/// `PERRY_GC_FORCE_EVACUATE` was inert for every `gc()`-driven test
/// (#6942/#6946).
///
/// **A second-order effect worth knowing about.** `gc/tenuring.rs` deliberately
/// refuses to seed the adaptive tenuring threshold from a cycle that ran the
/// conservative scan, so on any `gc()`-driven workload that seed had never
/// fired. It fires now. On two ratchet probes `tenuring_survivals` falls
/// `4 -> 1` and survivors are promoted on first copy rather than copied into
/// survivor space; the copying minor still runs and moves *more* objects.
/// Removing a conservative scan is not only a retention change.
///
/// **What is unchanged.** `gc()` is still synchronous — #7148's disposition
/// that it must not be *deferred* to a safepoint stands, because
/// `gc(); assertFreed()` is the shape every test and every ratchet probe uses.
/// This changes the root set, not the timing.
fn manual_gc_collect_now() {
    // NOTE: pending finalization jobs from earlier AUTOMATIC cycles are NOT
    // cleared here — each record enqueues exactly once (its pending flag is
    // reset at enqueue time), so dropping the vec would lose those callbacks
    // forever. The delivery below simply takes whatever is queued.
    // An explicit `gc()` runs a FULL mark-sweep rather than the generational
    // fast path. With gen-GC on (the default), `gc_collect_inner_with_trigger`
    // dispatches a MINOR cycle, whose sweep skips dead-old-block reclamation
    // (`reclaim_dead_old_blocks = false`) — so dead large/tenured objects (which
    // are born in the old arena, >16 KB) survived an explicit `gc()` and RSS
    // never dropped. A full cycle reclaims them, matching V8/Node `--expose-gc`
    // semantics where `gc()` is a full collection. Automatic threshold-driven
    // minors are unaffected.
    //
    // ★ #6946: under forced evacuation, run an EVACUATING minor FIRST.
    //
    // `PERRY_GC_FORCE_EVACUATE` is read only on the minor path, so every test
    // of the shape `gc(); assertFreed()` under that knob looked like evacuation
    // stress coverage and was a full mark-sweep that moved nothing —
    // CLAUDE.md's hazard 4, and one of the three worked examples in it. Five
    // suites still drive collection exactly that way
    // (`gc_property_key_operand_rooting_6935`,
    // `gc_dynamic_arith_operand_rooting_6655`,
    // `gc_string_coerce_property_key_rooting_6943`,
    // `gc_side_table_roots_evacuation`, and `gc/tests/cycle_state.rs`).
    //
    // What made this impossible before and does not any more: this site used to
    // take `ManualGcScanGuard::force_full_scan`, and a forced conservative scan
    // makes the copying minor ineligible outright
    // (`CopiedMinorFallbackReason::ConservativeStack`). #7657 removed it — see
    // the doc comment above — so `gc()` now runs on precise roots and a copying
    // minor here is exactly as sound as the full mark-sweep below.
    //
    // `FullEscalation::Refused`, because the two throughput-pacing predicates
    // would hand this call a full sweep: a NON-MOVING collection under a knob
    // whose entire name is about relocation, which is the original bug in a new
    // place. The full sweep follows immediately anyway, so refusing the
    // escalation here costs nothing it was protecting.
    //
    // Default-off knob, so nothing about an ordinary `gc()` changes.
    if super::gc_force_evacuate_enabled() {
        super::gc_collect_forced_evacuating_minor(GcTriggerSnapshot::capture(
            GcTriggerKind::Manual,
        ))
        .emit_after_current();
    }
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
        .emit_after_current();
    crate::weakref::queue_pending_finalization_callbacks_after_gc();
}

/// `perry/gc` `collect()` — explicit full collection, same semantics as the
/// global `gc()`. Returns `undefined` so the JS surface has a stable shape.
#[no_mangle]
pub extern "C" fn js_gc_module_collect() -> f64 {
    js_gc_collect();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `perry/gc` `minor()` — synchronous nursery-only collection; returns the
/// freed byte count (0 when skipped: unsafe zone or deferred). Like `gc()`,
/// the callsite may hold live locals only on the native stack, so force the
/// conservative scan (#4977).
///
/// ★ #7148 disposition: **keep, observable** — same reasoning as
/// `manual_gc_collect_now`. `minor()` returns the freed byte count, so its
/// result is only meaningful if the collection ran before it returned;
/// deferring would have to return 0 and silently mean something else.
#[no_mangle]
pub extern "C" fn js_gc_module_minor() -> f64 {
    if manual_gc_blocked_by_unsafe_zone() {
        return 0.0;
    }
    let _scan =
        super::roots::ManualGcScanGuard::force_full_scan(super::ConservativeScanSite::ManualMinor);
    super::gc_collect_minor() as f64
}

/// `perry/gc` `idleHint()` — frame-boundary pacing hint for latency-sensitive
/// programs (games, interactive UIs). If a threshold-driven collection is
/// already due, run it NOW — at a point the caller declared idle — instead of
/// letting it land mid-frame at an arbitrary allocation site. O(1) when
/// nothing is due. Returns whether a collection ran.
#[no_mangle]
pub extern "C" fn js_gc_module_idle_hint() -> f64 {
    let before = gc_total_collection_count();
    gc_check_trigger();
    let ran = gc_total_collection_count() != before;
    f64::from_bits(crate::value::JSValue::bool(ran).bits())
}

pub(super) fn gc_blocked_by_unsafe_zone() -> bool {
    GC_UNSAFE_ZONES.load(std::sync::atomic::Ordering::Acquire) > 0
}

pub(super) fn manual_gc_blocked_by_unsafe_zone() -> bool {
    if gc_blocked_by_unsafe_zone() {
        unsafe_zone_manual_gc_warning();
        return true;
    }
    false
}

pub(super) fn unsafe_zone_manual_gc_warning() {
    if !GC_UNSAFE_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        // One-shot warning — user likely has `setInterval(() => gc(), N)`
        // in a server; we don't want to print every 30s.
        eprintln!(
            "perry: gc() skipped — native work may hold JSValue refs on a \
             worker thread that the main-thread GC can't see. Manual gc() \
             is a no-op until that unsafe work exits."
        );
    }
}

/// Increment GC_UNSAFE_ZONES for a narrow FFI section whose worker thread may
/// hold JSValue roots the main-thread scanner cannot see.
#[no_mangle]
pub extern "C" fn js_gc_enter_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Decrement GC_UNSAFE_ZONES when the matching unsafe FFI section exits.
#[no_mangle]
pub extern "C" fn js_gc_exit_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
}

/// Threshold-based GC trigger (safe for use from the event loop).
/// Only runs collection if arena or malloc thresholds are exceeded.
#[no_mangle]
pub extern "C" fn gc_check_trigger_export() {
    gc_check_trigger();
}
