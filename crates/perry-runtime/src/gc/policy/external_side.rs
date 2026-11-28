//! External side allocations: the live and pending byte counts native buffers
//! report, and the parse and old-reclaim pressure they add.

use super::*;

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

crate::perry_thread_local! {
    static GC_EXTERNAL_SIDE_ALLOC_PENDING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static GC_EXTERNAL_SIDE_LIVE_BYTES: TriggerInput<usize> = const { TriggerInput::new(0) };
    /// Medium-parse pacing (2026-09-14): [`external_side_live_bytes`] as the
    /// last collection ended — the base of the parse-boundary growth band
    /// ([`external_side_parse_pressure_due_with`]). A byte COUNT, never an
    /// address; written only from `note_collection_finished_arena_occupancy`.
    pub(in crate::gc) static GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// Medium-parse pacing (2026-09-14): external side bytes that a
    /// collection or mutator operation has released since the last full — see
    /// [`external_side_old_reclaim_pressure_bytes`]. A byte COUNT, never an
    /// address.
    pub(in crate::gc) static GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL: TriggerInput<usize> =
        const { TriggerInput::new(0) };
}

/// Live bytes currently held by external Map/Set side buffers on this thread.
#[inline]
pub(in crate::gc) fn external_side_live_bytes() -> usize {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(TriggerInput::get)
}

/// Medium-parse pacing (2026-09-14): how many bytes of external side allocation
/// may accumulate past the last collection before a `JSON.parse` boundary is
/// due.
///
/// Deliberately the `max(floor, proportional)` shape of
/// [`gc_old_reclaim_growth_band_bytes`], for the two reasons that shape exists:
///
/// * a program whose live side set is genuinely large (a retained multi-MB
///   `Map`) must not collect once per parse, so the band grows with it; and
/// * a collection that CANNOT lower the number this band watches re-baselines
///   it at the surviving value, so futile repeats space out geometrically
///   instead of firing at a constant step. That is what keeps this arm off the
///   #7437/#7592 livelock: a lazy array whose cluster was born OLD
///   (`json_tape::lazy_cluster_is_old`) keeps its tape through the nursery
///   collection this arm schedules, and the next band is then twice as far
///   away rather than due again at the next parse.
pub(in crate::gc) fn external_side_parse_band_bytes(baseline: usize) -> usize {
    gc_trigger_headroom_floor_bytes().max(baseline)
}

/// [`external_side_parse_pressure_due`] with both readings supplied.
pub(in crate::gc) fn external_side_parse_pressure_due_with(live: usize, baseline: usize) -> bool {
    live >= baseline.saturating_add(external_side_parse_band_bytes(baseline))
}

/// Medium-parse pacing (2026-09-14): has external side-allocation churn earned
/// a parse-boundary collection?
///
/// Every other pacing input a parse boundary reads is denominated in ARENA
/// bytes, and a lazily-parsed document's memory is not in the arena: a 13 KB
/// `records_array_16k` parse puts ~1.1 KB (header + sparse cache + bitmap) in
/// the nursery and ~24 KB of tape in a `json_tape_store` side allocation. So
/// the young generation reads 1/24th of what the process is actually holding,
/// and a parse loop reaches its nursery cap 24x later than the memory says it
/// should. Measured on `records_array_16k:parse` at `origin/main`
/// (`PERRY_GC_DIAG=1`, 11 284 iterations): EIGHT collections, every one of them
/// a full mark-sweep from `alloc_point_old_reclaim`, each firing at
/// `external_side=33.6 MB` with `arena_total` between 3 and 8 MB,
/// `old_in_use=0` and `from_space` never above 7 MB against a 16 MB nursery
/// cap. The only pacing this workload had was the old-reclaim growth band
/// reading those side bytes, i.e. 32 MB of dead tape per cycle.
pub(in crate::gc) fn external_side_parse_pressure_due() -> bool {
    external_side_parse_pressure_due_with(
        external_side_live_bytes(),
        GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES.with(Cell::get),
    )
}

/// Account for external storage at birth. Allocation entries never collect;
/// pressure is consumed at the collector's safepoints.
pub(crate) fn gc_note_external_side_alloc(bytes: usize) {
    super::allocation_pacing::note_allocation(bytes, bytes >= 16 * 1024);
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
        #[cfg(test)]
        if std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("external_base") {
            set_safepoint_pending(true);
            return;
        }
        #[cfg(test)]
        if std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("birth_collect") {
            super::js_gc_collect();
        }
        defer_nursery_cap_to_precise_safepoint();
    }
}

/// Record released external side bytes, from either a collector or a mutator operation.
pub(crate) fn gc_note_external_side_free(bytes: usize) {
    GC_EXTERNAL_SIDE_LIVE_BYTES.with(|c| c.set(c.get().saturating_sub(bytes)));
    GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(|c| c.set(c.get().saturating_add(bytes)));
}

/// The external side-buffer term of OLD-RECLAIM pressure.
///
/// Live bytes plus all reported releases since the last full baseline. The
/// release hook also runs during JSON tape materialization, regex scratch
/// teardown, native-addon adjustments and Map/Set buffer replacement; this
/// accounting is broader than bytes freed by a minor collection.
///
/// Holding released bytes in this term keeps cheap collections from removing
/// the pressure that pays for arena-capacity reclamation. On the measured
/// `records_array_1m:sparse` loop, the live-only alternative reduced fulls from
/// seven to one and raised peak RSS from 63.5 to 73.6 MiB despite freeing more
/// tape. The cumulative term restored that workload's full-collection cadence.
///
/// This is not a general guarantee of identical pacing: a mutator-side release
/// lowers the previous live-only term but leaves this cumulative term unchanged,
/// so other workloads can reach the full-collection threshold earlier. The full
/// baseline resets the released-byte contribution before pricing the next band.
pub(in crate::gc) fn external_side_old_reclaim_pressure_bytes() -> usize {
    external_side_live_bytes()
        .saturating_add(GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(TriggerInput::get))
}
