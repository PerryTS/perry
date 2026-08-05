//! Old-generation hole free list (#7437).
//!
//! Old-gen allocation was pure bump: a swept dead old object stayed dead
//! capacity until its *entire block* died, and a block with even one live
//! object never resets. A workload that promotes a large cohort and keeps
//! a scattered subset (every-64th node in the `12_large_live_set` ratchet
//! probe) therefore retained 105 MB of blocks for a ~1 MB live set — the
//! final full collection freed 87 MB of objects and reclaimed nothing,
//! because 49 of 50 blocks still held at least one live object. The same
//! mechanism is a large slice of tree.ts's old-gen churn high-water
//! (#7438): every dropped tree leaves holes in blocks pinned live by the
//! next tree's nodes.
//!
//! This module gives the old generation what the general arena has had
//! all along (`ARENA_FREE_LIST`): swept holes become reusable. Shape
//! differences are deliberate:
//!
//! - **Exact fit only, keyed by total (header-inclusive, padded) size.**
//!   The general list best-fits into larger slots and keeps the slot's
//!   original `GcHeader::size`, which is fine there because nothing else
//!   accounts those bytes. Old-gen promotion *does* account per-object
//!   sizes (`old_page_account_promoted_object`), so a reused slot must
//!   have exactly the size the caller asked for or the page live-byte
//!   accounting diverges from the header. Promoted cohorts are dominated
//!   by uniform class-instance sizes, so exact fit hits where the
//!   pathology lives.
//! - **Size-bucketed map, not a scanned Vec.** The pathological case has
//!   hundreds of thousands of holes; a per-allocation linear scan would
//!   put an O(holes) tax on every promotion.
//!
//! `OLD_ARENA_FREE_BYTES` tracks the total. It is deliberately NOT
//! subtracted from `OLD_GEN_IN_USE_BYTES` — that cache is defined (and
//! debug-asserted) as the sum of old block offsets, which hole reuse does
//! not change. Consumers that want *live* old pressure (the old-reclaim
//! pacing arms, `process.memoryUsage().heapUsed`) subtract
//! [`old_free_bytes`] instead; before this, dead-but-unreclaimable bytes
//! counted as pressure, so old-reclaim kept re-firing full collections
//! that could not actually lower the number they were watching.
//!
//! Entries are only pushed for dead objects in blocks that still hold a
//! live object (fully-dead blocks go through block reclaim, which is
//! strictly better). A pushed entry's block can still die on a LATER
//! cycle, so every old-block reset/dealloc site must call
//! [`old_free_filter_range`] for the range it is about to recycle.

use super::*;

thread_local! {
    /// total_size -> user_ptrs of swept holes of exactly that size.
    static OLD_FREE_MAP: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<usize>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    static OLD_FREE_BYTES: Cell<usize> = const { Cell::new(0) };
    static OLD_FREE_NONEMPTY: Cell<bool> = const { Cell::new(false) };
}

/// Total bytes currently sitting in reusable old-gen holes.
pub(crate) fn old_free_bytes() -> usize {
    OLD_FREE_BYTES.with(Cell::get)
}

fn old_free_push(user_ptr: usize, total_size: usize) {
    if user_ptr == 0 || total_size < GC_HEADER_SIZE {
        return;
    }
    OLD_FREE_MAP.with(|m| {
        m.borrow_mut().entry(total_size).or_default().push(user_ptr);
    });
    OLD_FREE_BYTES.with(|c| c.set(c.get().saturating_add(total_size)));
    OLD_FREE_NONEMPTY.with(|c| c.set(true));
}

/// Rebuild the hole map from the heap itself: every invalidated dead
/// header (`obj_type == 0` — only `invalidate_dead_old_arena_header`
/// produces those; no live object has type 0) inside an old block that
/// still holds a live object. Called at the completion point of every
/// old-reclaiming sweep, replacing whatever the map held.
///
/// Rebuilding beats accumulating a staging vector during the sweep walk on
/// two counts, both measured on `12_large_live_set` (~700k dead old
/// objects): the staging vector alone added ~17 MB of peak RSS to the very
/// number this feature exists to lower, and rebuild is idempotent — a hole
/// consumed by reuse gets a real `obj_type` and drops out, a hole whose
/// block died is never visited, so no cross-sweep dedup bookkeeping can
/// drift. The walk is block-filtered (live old blocks only), so its cost
/// is O(objects in surviving old blocks), paid only on reclaim sweeps.
pub(super) fn old_free_rebuild_from_live_old_blocks(
    block_has_live: &[bool],
    old_block_start: usize,
) {
    OLD_FREE_MAP.with(|m| m.borrow_mut().clear());
    OLD_FREE_BYTES.with(|c| c.set(0));
    OLD_FREE_NONEMPTY.with(|c| c.set(false));
    // The raw-headers walker is load-bearing: the walkable-gated walkers
    // (`arena_walk_objects_filtered` and friends) step over invalidated
    // headers WITHOUT invoking the callback, so a rebuild written against
    // them silently records zero holes.
    crate::arena::old_arena_walk_all_headers_filtered(
        |block_idx| {
            block_idx >= old_block_start && block_has_live.get(block_idx).copied().unwrap_or(false)
        },
        |header_ptr, _block_idx| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).obj_type == 0 {
                    let total_size = (*header).size as usize;
                    old_free_push(header as usize + GC_HEADER_SIZE, total_size);
                }
            }
        },
    );
}

/// Take a hole of exactly `total_size` bytes, if one exists. When
/// `excluded_pages` is non-empty the caller is mid-defrag and must not
/// allocate on the pages it is evacuating; holes on those pages are
/// skipped (and retained).
pub(crate) fn old_free_take_exact(
    total_size: usize,
    excluded_pages: Option<&crate::fast_hash::PtrHashSet<usize>>,
) -> Option<usize> {
    if !OLD_FREE_NONEMPTY.with(Cell::get) {
        return None;
    }
    let taken = OLD_FREE_MAP.with(|m| {
        let mut map = m.borrow_mut();
        let bucket = map.get_mut(&total_size)?;
        let taken = match excluded_pages {
            None => bucket.pop(),
            Some(excluded) => {
                let idx = bucket.iter().rposition(|&ptr| {
                    let header = ptr - GC_HEADER_SIZE;
                    let first = crate::arena::generation_page_for_addr(header);
                    let last = crate::arena::generation_page_for_addr(header + total_size - 1);
                    (first..=last).all(|page| !excluded.contains(&page))
                })?;
                Some(bucket.swap_remove(idx))
            }
        };
        if bucket.is_empty() {
            map.remove(&total_size);
        }
        taken
    })?;
    OLD_FREE_BYTES.with(|c| c.set(c.get().saturating_sub(total_size)));
    OLD_FREE_MAP.with(|m| {
        if m.borrow().is_empty() {
            OLD_FREE_NONEMPTY.with(|c| c.set(false));
        }
    });
    Some(taken)
}

/// Drop every hole inside `[base, base + size)`. Called by the old-block
/// reset/dealloc paths before they recycle a block's bytes — a stale
/// entry would otherwise hand out a pointer into memory the bump
/// allocator is about to overwrite (or that has been returned to the OS).
pub(crate) fn old_free_filter_range(base: usize, size: usize) {
    if !OLD_FREE_NONEMPTY.with(Cell::get) || size == 0 {
        return;
    }
    let end = base.saturating_add(size);
    let mut removed_bytes = 0usize;
    OLD_FREE_MAP.with(|m| {
        let mut map = m.borrow_mut();
        map.retain(|&slot_size, bucket| {
            bucket.retain(|&ptr| {
                let header = ptr - GC_HEADER_SIZE;
                let inside = header >= base && header < end;
                if inside {
                    removed_bytes = removed_bytes.saturating_add(slot_size);
                }
                !inside
            });
            !bucket.is_empty()
        });
        if map.is_empty() {
            OLD_FREE_NONEMPTY.with(|c| c.set(false));
        }
    });
    OLD_FREE_BYTES.with(|c| c.set(c.get().saturating_sub(removed_bytes)));
}

#[cfg(test)]
pub(super) fn old_free_reset_for_test() {
    OLD_FREE_MAP.with(|m| m.borrow_mut().clear());
    OLD_FREE_BYTES.with(|c| c.set(0));
    OLD_FREE_NONEMPTY.with(|c| c.set(false));
}

#[cfg(test)]
pub(super) fn old_free_entry_count() -> usize {
    OLD_FREE_MAP.with(|m| m.borrow().values().map(|b| b.len()).sum())
}
