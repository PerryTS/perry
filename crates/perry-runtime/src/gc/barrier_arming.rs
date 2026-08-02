//! #7187 Phase A — lazy write-barrier arming.
//!
//! Split out of `barrier.rs`, which is at the 2 000-line cap
//! `scripts/check_file_size.sh` enforces.

use super::*;

/// #7187 Phase A — is the remembered-set half of the write barrier armed?
///
/// The remembered set has exactly one consumer: a collection, and every
/// collector reaches it through [`remembered_dirty_snapshot`] (see that
/// function for why that is the *sole* read path). Until the first such read,
/// maintaining the log buys nothing — and on `batch.ts` it is the program's
/// single largest cost: #7170 measured `classify_heap_generation` at 19.03%
/// (657M instructions) with **zero collections running**, reached from this
/// barrier on the sort's own element stores.
///
/// So the log starts unarmed and records nothing. The exchange, and the whole
/// soundness argument, lives on the *reading* side: the first snapshot on a
/// thread does not trust the log at all — it reconstructs the complete
/// old→young edge set from the heap
/// ([`arm_and_reconstruct_remembered_set_if_unarmed`]) and arms the barrier on
/// the way past. From then on the barrier maintains it incrementally exactly
/// as before.
///
/// **The naive premise "nothing has collected, so there is no old generation"
/// is false and is deliberately not used here.** `arena_alloc_gc` routes any
/// allocation over `LARGE_OBJECT_THRESHOLD_BYTES` straight to
/// `arena_alloc_gc_old`, so old-gen exists from the first large allocation —
/// `batch.ts`'s own 40 000-element arrays are born old while every record they
/// hold is nursery. `note_array_slot_layout_only`'s comment in
/// `array/header.rs` is the postmortem of the last time that was got wrong
/// ("155 edges, all born-old array→young object"). The reconstruct, not the
/// flag, is what makes this sound.
///
/// A process-global `AtomicBool` rather than a thread-local: arming is
/// monotone and one-way, and conservative in the early direction (a thread
/// that arms early merely stops being free), while a `thread_local!` costs a
/// `_tlv_get_addr` *call* on macOS on a path that runs on every heap store.
/// Per-thread state — "has *this* thread reconstructed yet" — stays in
/// [`REMEMBERED_SET_RECONSTRUCTED`], where a wrong answer would matter.
///
/// **In `cfg(test)` builds this starts ARMED**, and so does
/// [`REMEMBERED_SET_RECONSTRUCTED`]. The runtime's existing barrier unit tests
/// assert the armed steady state — they hand-build an old parent and a young
/// child and expect the store to be logged — and they are right to: that is
/// the state every program is in after its first collection. Letting them
/// observe the unarmed window instead would have deleted their subject
/// wholesale, and made the suite order-dependent besides (the first test that
/// collects arms the process for every later test). The unarmed window gets
/// dedicated tests that opt back out via [`reset_barrier_arming_for_tests`],
/// plus end-to-end coverage from every compiled binary the parity/gap suites
/// run — those start unarmed for real.
static BARRIER_REMEMBERING_ARMED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(test));

#[inline]
pub(super) fn barrier_remembering_armed() -> bool {
    BARRIER_REMEMBERING_ARMED.load(Ordering::Relaxed)
}

/// What the unarmed-window reconstruct recovered. The #7187 gate asserts on
/// these rather than on "nothing crashed": a reconstruct that recovers no
/// edges on a workload built from born-old parents means the walk never ran or
/// never looked, which is precisely the `PERRY_GC_FORCE_EVACUATE` failure mode
/// (#6942/#6946) — a green test whose subject was inert.
#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
pub(super) struct RememberedReconstructCensus {
    /// Reconstructs run on this thread: 0 (never collected) or 1.
    pub(super) reconstructs: u64,
    /// Old-gen pages re-derived from the heap — the old→young edges the
    /// unarmed barrier deliberately did not log.
    pub(super) recovered_old_pages: u64,
    /// External (malloc-backed) slot-page entries re-derived from the heap.
    pub(super) recovered_external_pages: u64,
}

impl RememberedReconstructCensus {
    pub(super) const fn zero() -> Self {
        Self {
            reconstructs: 0,
            recovered_old_pages: 0,
            recovered_external_pages: 0,
        }
    }
}

thread_local! {
    /// Has this thread already reconstructed its remembered set from the heap?
    /// Set by the first [`remembered_dirty_snapshot`] on the thread. Threads
    /// are independent here: [`BARRIER_REMEMBERING_ARMED`] going true because
    /// some *other* thread collected does not excuse this thread from its own
    /// reconstruct, because this thread's stores before that moment were never
    /// logged. The two flags are a superset and a no-op respectively, so there
    /// is no window in which an edge is neither logged nor reconstructed.
    static REMEMBERED_SET_RECONSTRUCTED: Cell<bool> = const { Cell::new(cfg!(test)) };

    static RECONSTRUCT_CENSUS: Cell<RememberedReconstructCensus> =
        const { Cell::new(RememberedReconstructCensus::zero()) };
}

pub(super) fn remembered_reconstruct_census() -> RememberedReconstructCensus {
    RECONSTRUCT_CENSUS.with(Cell::get)
}

/// Arm the barrier and rebuild this thread's remembered set from the heap, if
/// that has not happened yet. Called by [`remembered_dirty_snapshot`] — i.e.
/// on the read side, before any collector observes the log.
///
/// Ordering is load-bearing: **arm first, walk second.** A store that lands
/// while the walk is in flight must be logged, because the walk may already
/// have passed its parent. Arming first makes the two coverages overlap; the
/// reverse order leaves a hole.
pub(super) fn arm_and_reconstruct_remembered_set_if_unarmed() {
    if REMEMBERED_SET_RECONSTRUCTED.with(Cell::get) {
        return;
    }
    // Set before the walk: the walk touches layout side tables and GC rewrite
    // hooks, and must not be able to recurse into a second reconstruct.
    REMEMBERED_SET_RECONSTRUCTED.with(|cell| cell.set(true));
    BARRIER_REMEMBERING_ARMED.store(true, Ordering::Relaxed);

    // `require_marked = false`: nothing is marked yet when a collector first
    // asks for the log, so the walk must consider every retained old parent.
    // That over-approximates (a dead-but-unswept old parent's young children
    // are kept one extra cycle) in the same direction the barrier itself
    // over-approximates — an edge logged while the parent was alive also
    // outlives the parent's death.
    let sticky = super::verify::rebuild_minor_old_to_young_remembered_set();
    let recovered_old_pages = sticky.old_pages.len() as u64;
    let recovered_external_pages = sticky.external_pages.len() as u64;
    sticky.restore();

    RECONSTRUCT_CENSUS.with(|cell| {
        let mut census = cell.get();
        census.reconstructs = census.reconstructs.saturating_add(1);
        census.recovered_old_pages = census
            .recovered_old_pages
            .saturating_add(recovered_old_pages);
        census.recovered_external_pages = census
            .recovered_external_pages
            .saturating_add(recovered_external_pages);
        cell.set(census);
    });
}

/// Test-only: return the barrier to its process-start unarmed state so a test
/// can exercise the unarmed window deliberately. Production has no way back —
/// arming is one-way.
#[cfg(test)]
pub(super) fn reset_barrier_arming_for_tests() {
    BARRIER_REMEMBERING_ARMED.store(false, Ordering::Relaxed);
    REMEMBERED_SET_RECONSTRUCTED.with(|cell| cell.set(false));
    RECONSTRUCT_CENSUS.with(|cell| cell.set(RememberedReconstructCensus::zero()));
}

/// Test-only: arm the barrier without a reconstruct, for the barrier tests
/// that assert the ARMED steady state and build their own heap shapes
/// directly.
#[cfg(test)]
pub(super) fn arm_barrier_for_tests() {
    BARRIER_REMEMBERING_ARMED.store(true, Ordering::Relaxed);
    REMEMBERED_SET_RECONSTRUCTED.with(|cell| cell.set(true));
}
