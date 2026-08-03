//! #7187 Phase B — the write barrier's "this page is already dirty" cache.
//!
//! Split out of `barrier.rs`, which is at the 2 000-line cap
//! `scripts/check_file_size.sh` enforces.
//!
//! # What this removes
//!
//! [`super::barrier::mark_dirty_old_page`] is the tail of the remembered-set
//! half of the write barrier: it inserts the written slot's 4 KiB page number
//! into the thread's `DIRTY_OLD_PAGES` modbuf and mirrors the fact into the
//! arena's per-page metadata. Two thread-local accesses and two hash
//! operations, on every old→young store.
//!
//! Measured on `benchmarks/app-patterns/kernels/batch.ts` with the barrier
//! armed: **1 774 374 calls producing 517 distinct pages** — 99.971% of the
//! work is re-inserting a page that is already in the set. #7170's ranked
//! profile puts `mark_dirty_old_page` at 6.73% of that whole program.
//!
//! # Why a one-entry cache, and not something cleverer
//!
//! Because that is what the page sequence says. Simulating cache shapes over
//! the exact sequence `mark_dirty_old_page` sees on `batch.ts` (armed):
//!
//! | shape | hit rate | calls left |
//! |---|---:|---:|
//! | **1-entry (this)** | **99.7817%** | **3 873** |
//! | 2-entry LRU | 99.8730% | 2 253 |
//! | 4-entry LRU | 99.8730% | 2 253 |
//! | 512-entry direct-mapped | 99.9423% | 1 023 |
//! | 2048-entry direct-mapped | 99.9531% | 832 |
//!
//! The stores arrive in long same-page runs (3 872 runs over 1 774 374 calls;
//! mean run 458, longest 13 803), so the whole redundancy is *consecutive*
//! repetition and one entry captures it. Every larger shape buys ≤0.17
//! percentage points for more state, an index computation, and — for the
//! direct-mapped variants — kilobytes of thread-local storage on a path that
//! runs on every heap store. One `usize` and one compare is the mechanism the
//! data supports.
//!
//! # The invariant
//!
//! > **If `LAST_DIRTY_OLD_PAGE` holds page `P` (i.e. is not [`NO_PAGE`]) then,
//! > on this thread, `P ∈ DIRTY_OLD_PAGES` *and* `P`'s `OldPageMeta.dirty` is
//! > already `true`.**
//!
//! Both halves are exactly what `mark_dirty_old_page(P)` establishes, so under
//! the invariant that call is a pure no-op and skipping it cannot lose an
//! old→young edge. The remembered set stays **complete**: the cache can only
//! suppress a *repeat* of a recording that already happened, never a first one.
//!
//! It is maintained by three rules, and the deliberate narrowness of the first
//! is the whole soundness argument:
//!
//! 1. **Only [`note_dirty_old_page_marked`] populates it, and only after both
//!    halves have just been established** — including the arena stamp, which is
//!    conditional (`old_page_mark_dirty` silently does nothing for a page with
//!    no metadata entry). A page recorded in the modbuf but not in the metadata
//!    is deliberately *not* cached, so the metadata can never drift behind.
//! 2. **[`invalidate`] runs on every path that can falsify either half.** For
//!    `DIRTY_OLD_PAGES` that is `clear_one_dirty_old_page` — the sole removal
//!    (every other touch is an insert, a read, or the snapshot). For the
//!    metadata it is `arena::old_page_clear_dirty` and
//!    `arena::unregister_old_block_pages`, the only two places a `dirty` bit
//!    goes false or a page's metadata disappears.
//! 3. **It is thread-local, like both things it summarises.** `DIRTY_OLD_PAGES`
//!    and `OLD_GEN_PAGE_META` are per-thread; a process-global cache would let
//!    thread A's mark suppress thread B's, dropping the page from B's modbuf
//!    entirely — a missed edge, i.e. heap corruption, not a slow program.
//!
//! # Interaction with Phase A (#7250)
//!
//! Phase A leaves the remembered-set half of the barrier **unarmed** until the
//! first read of the log, and an unarmed barrier never reaches
//! `mark_dirty_old_page` at all — so in the unarmed state this cache is simply
//! never consulted and never populated. The reconstruct that arms the barrier
//! (`arm_and_reconstruct_remembered_set_if_unarmed`) rebuilds the log by
//! calling `StickyRememberedSet::restore`, which goes through
//! `mark_dirty_old_page` like everything else: the cache is populated by the
//! reconstruct exactly as it would be by the barrier, and, since the
//! reconstruct only ever *inserts*, it cannot falsify the invariant.

use std::cell::Cell;

/// "Nothing cached". Not a reachable page number: pages are `addr >> 12`, so
/// `usize::MAX` would need a 76-bit address.
const NO_PAGE: usize = usize::MAX;

thread_local! {
    static LAST_DIRTY_OLD_PAGE: Cell<usize> = const { Cell::new(NO_PAGE) };
}

/// Is `page` known to be recorded already? See the module invariant.
#[inline]
pub(super) fn dirty_old_page_already_marked(page: usize) -> bool {
    debug_assert_ne!(page, NO_PAGE, "page number collides with the empty marker");
    LAST_DIRTY_OLD_PAGE.with(Cell::get) == page
}

/// Record that `page` is now in `DIRTY_OLD_PAGES` **and** stamped dirty in the
/// arena page metadata. Callers must have established both immediately before.
#[inline]
pub(super) fn note_dirty_old_page_marked(page: usize) {
    LAST_DIRTY_OLD_PAGE.with(|cell| cell.set(page));
}

/// Drop the cached page. Called from every path that can remove a page from
/// `DIRTY_OLD_PAGES` or un-stamp / discard its arena metadata — see rule 2 in
/// the module doc. Cheap enough (one thread-local store) that these callers do
/// not check whether the page they touched is the cached one.
pub(crate) fn invalidate() {
    LAST_DIRTY_OLD_PAGE.with(|cell| cell.set(NO_PAGE));
}

/// Test-only: is the cache currently empty? Lets the #7187 Phase B tests assert
/// that an invalidation really happened rather than that nothing broke.
#[cfg(test)]
pub(super) fn is_empty_for_tests() -> bool {
    LAST_DIRTY_OLD_PAGE.with(Cell::get) == NO_PAGE
}
