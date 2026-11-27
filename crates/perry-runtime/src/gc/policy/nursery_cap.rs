//! The arena trigger the collector compares and the scavenge nursery cap that
//! bounds it: when the young generation is due, and the cap's size and test
//! pins.

use super::*;

/// The arena-bytes trigger as the collector should compare it: the raw
/// cell while armed (explicit re-arms/bumps may legitimately exceed the
/// ceiling — headroom floor over a big live set, medium-parse bumps), the
/// device-derived ceiling while the cell still holds its desktop-default
/// const initializer.
/// The adaptive/armed arena trigger WITHOUT the scavenge nursery cap:
/// compared against `arena_total_bytes()` (all generations), as it always
/// was. The cap is deliberately not part of this value — it is
/// young-generation-scoped and lives in [`young_scavenge_cap_due`].
pub(in crate::gc) fn next_arena_trigger_base() -> usize {
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
pub(in crate::gc) fn young_scavenge_cap_due() -> bool {
    young_scavenge_cap_due_with_old_reclaimable(old_gen_reclaimable_pressure_bytes)
}

/// [`young_scavenge_cap_due`] with the old-gen reclaimable-pressure read
/// supplied by the caller. `gc_budgeted_due_trigger` has already read that
/// value for its old-reclaim arm, and nothing between that read and this one
/// touches old-gen, so it passes a closure returning the value it holds rather
/// than paying two more thread-local reads. The read happens where it always
/// did: after the census seed, which changes only the young-side factor.
fn young_scavenge_cap_due_with_old_reclaimable(old_reclaimable: impl FnOnce() -> usize) -> bool {
    young_scavenge_cap_probe(old_reclaimable).due
}

/// [`young_scavenge_cap_due_with_old_reclaimable`]'s answer plus what it read
/// to reach it — `None` when the cap is not in force and nothing was read.
pub(super) struct YoungScavengeCapProbe {
    pub(super) due: bool,
    pub(super) reading: Option<super::trigger_watermark::YoungArmReading>,
}

pub(super) fn young_scavenge_cap_probe(
    old_reclaimable: impl FnOnce() -> usize,
) -> YoungScavengeCapProbe {
    if !nursery_cap_active() {
        return YoungScavengeCapProbe {
            due: false,
            reading: None,
        };
    }
    // One read of the cached occupancy also says whether no young leaf lives.
    let (from_space_in_use, leaf_free) =
        crate::arena::copying_from_space_in_use_bytes_and_leaf_free();
    // The current Eden block's share of that reading: the inline offset,
    // whenever the inline allocator is initialized (see
    // `synced_current_eden_block_bytes`). Read here, before anything else
    // runs, so it is the value the occupancy was summed from.
    let inline_offset = {
        // SAFETY: this thread's own inline allocator state, read only.
        let inline = unsafe { &*crate::arena::hot_inline_state() };
        (!inline.data.is_null()).then_some(inline.offset)
    };
    // #8122: before the first copying minor has measured survivors, denominate
    // the FIRST cap in this program's objects too (one header walk, once per
    // process, halfway to the base cap). Not while a collection is in
    // progress or a budgeted cycle is active — the young generation is being
    // rewritten then and the walk would read forwarding stubs.
    //
    // The seeded test comes first because the seed can happen only once: in
    // steady state this is one thread-local read instead of three.
    if !super::tenuring::object_census_seeded()
        && GC_FLAGS.with(|f| f.get()) & GC_FLAG_IN_ALLOC == 0
        && !gc_budgeted_cycle_active()
    {
        super::tenuring::maybe_seed_object_census_from_allocation(from_space_in_use);
    }
    let cap = scavenge_nursery_cap_dueness_bytes_with(old_reclaimable);
    // #10698: while the census is unseeded, the first reading at or past the
    // seed point seeds it and re-denominates the cap, so "not due" only
    // holds below whichever comes first.
    let limit = if super::tenuring::object_census_seeded() {
        cap
    } else {
        cap.min(super::tenuring::object_census_seed_point_bytes())
    };
    // Growing the inline small allocation can cross the leaf window before
    // the copying cap. Publish the same residual bound to the fast shortcut.
    // With no young leaf alive this is the not-taken branch of a flag the
    // occupancy read already produced.
    let (leaf_due, limit) = if leaf_free {
        (false, limit)
    } else {
        let (due, small_limit) = leaf_memory_due_at_cap(cap, from_space_in_use);
        (due, limit.min(small_limit))
    };
    YoungScavengeCapProbe {
        due: from_space_in_use >= cap || leaf_due,
        reading: Some(super::trigger_watermark::YoungArmReading {
            from_space_in_use,
            limit,
            inline_offset,
        }),
    }
}

/// #10169: does the young generation hold at least one BASE nursery cap of
/// bytes — a minor's worth of data, whatever the adaptive cap currently says?
/// Read by `string::json_leaf_prefers_arena` from inside a suppressed
/// construction window. The base cap rather than [`young_scavenge_cap_due`]'s
/// adaptive one on purpose: after a fully-live minor the adaptive cap scales
/// past the very tree that made it scale, and a gate keyed on it would flip
/// the next leaf back to the malloc registry, where one tracked leaf is enough
/// to veto the untraced promotion the route exists to enable.
pub(crate) fn young_generation_holds_a_nursery() -> bool {
    nursery_cap_active()
        && crate::arena::copying_from_space_in_use_bytes() >= gc_scavenge_nursery_cap_bytes()
}

/// The cap value [`young_scavenge_cap_due`] compares against.
///
/// Split out only so a test can make the cap due without allocating the real
/// 16 MB — a PER-THREAD override next to the reader, the shape support.rs
/// mandates for anything a test needs to move (never the process environment,
/// which is shared by every libtest thread; see #7946). It deliberately does
/// NOT feed `effective_next_arena_trigger`: this is about *dueness*, and a test
/// that also moved the trigger clamp would be changing two things at once.
pub(super) fn scavenge_nursery_cap_dueness_bytes() -> usize {
    scavenge_nursery_cap_dueness_bytes_with(old_gen_reclaimable_pressure_bytes)
}

fn scavenge_nursery_cap_dueness_bytes_with(old_reclaimable: impl FnOnce() -> usize) -> usize {
    #[cfg(test)]
    if let Some(bytes) = GC_NURSERY_CAP_TEST_DUE_BYTES.with(TriggerInput::get) {
        return bytes;
    }
    let influx_driven = super::tenuring::ladder_nursery_cap_bytes();
    let old_reclaimable = old_reclaimable();
    debug_assert_eq!(
        old_reclaimable,
        old_gen_reclaimable_pressure_bytes(),
        "the old-gen pressure a due check reuses must still be the current value"
    );
    super::tenuring::scavenge_nursery_cap_from(influx_driven, old_reclaimable)
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`scavenge_nursery_cap_dueness_bytes`].
    static GC_NURSERY_CAP_TEST_DUE_BYTES: TriggerInput<Option<usize>> = const { TriggerInput::new(None) };
}

/// RAII override making the young-gen scavenge cap due at `bytes` of from-space
/// occupancy on this thread (#7909).
#[cfg(test)]
pub(in crate::gc) struct ScavengeNurseryCapTestGuard {
    previous: Option<usize>,
}

#[cfg(test)]
impl ScavengeNurseryCapTestGuard {
    pub(in crate::gc) fn due_at_bytes(bytes: usize) -> Self {
        let previous = GC_NURSERY_CAP_TEST_DUE_BYTES.with(|cell| {
            let previous = cell.get();
            cell.set(Some(bytes));
            previous
        });
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for ScavengeNurseryCapTestGuard {
    fn drop(&mut self) {
        GC_NURSERY_CAP_TEST_DUE_BYTES.with(|cell| cell.set(self.previous));
    }
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
/// does. Polls are on by default, so the cap is on with them, in the
/// configuration it was measured in; `PERRY_GC_MOVING_LOOP_POLLS=0` turns both
/// off.
fn nursery_cap_active() -> bool {
    #[cfg(test)]
    if GC_NURSERY_CAP_TEST_SUPPRESSED.with(TriggerInput::get) {
        return false;
    }
    gc_moving_loop_polls_enabled()
}

pub(in crate::gc) fn effective_next_arena_trigger() -> usize {
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

/// Default base nursery high-water cap, in MiB. Named rather than inline
/// because `docs/src/internals/garbage-collector.md` documents it and
/// `scripts/check_gc_doc_claims.py` re-derives the documented number from this
/// definition.
pub(in crate::gc) const SCAVENGE_NURSERY_CAP_DEFAULT_MB: usize = 16;

/// Nursery high-water cap, applied while moving-loop polls are on
/// (`PERRY_GC_MOVING_LOOP_POLLS`, gated by `nursery_cap_active`). Default
/// [`SCAVENGE_NURSERY_CAP_DEFAULT_MB`]; `PERRY_GC_SCAVENGE_NURSERY_MB` overrides
/// the size. See `effective_next_arena_trigger`.
pub(in crate::gc) fn gc_scavenge_nursery_cap_bytes() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        std::env::var("PERRY_GC_SCAVENGE_NURSERY_MB")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|&mb| mb > 0)
            .unwrap_or(SCAVENGE_NURSERY_CAP_DEFAULT_MB)
            .saturating_mul(1024 * 1024)
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only suppression of the nursery cap, so `force_legacy_gc_pacing`
    /// can restore genuinely legacy pacing.
    ///
    /// Pinning polls off also un-caps the trigger, but a test that keeps polls
    /// on still needs the raw-cell trigger arithmetic: while #7056 made the cap
    /// unconditional, 22 `gc::tests` failed against the capped value. The
    /// guard suppresses the cap directly, whatever the poll mode.
    pub(super) static GC_NURSERY_CAP_TEST_SUPPRESSED: TriggerInput<bool> = const { TriggerInput::new(false) };
}
