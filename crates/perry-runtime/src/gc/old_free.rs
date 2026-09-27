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
//! - **One chain per size, not one scanned list.** The pathological case
//!   has hundreds of thousands of holes; a per-allocation linear scan would
//!   put an O(holes) tax on every promotion. A take pops its size's head.
//! - **Intrusive, no per-hole side storage (#11505).** The list is threaded
//!   through the holes themselves: a listed hole's first payload word (the
//!   word right after its invalidated header) holds the user pointer of the
//!   next listed hole of the same size, `0` ending the chain. The only state
//!   outside the heap is one head per size class — a direct-indexed array for
//!   the small sizes promoted cohorts are made of, and a size -> head map for
//!   the rest, holding one entry per distinct size rather than one per hole.
//!   The earlier shape kept a `Vec` of hole addresses per size: every swept
//!   hole cost a push (plus that vector's growth reallocations), and every
//!   rebuild freed the vectors and grew them again.
//!
//!   The header stays exactly as `invalidate_dead_old_arena_header` left it
//!   (`obj_type == 0`, `size` intact), so everything that classifies a
//!   header sees the same dead hole it always did: the walkable-gated
//!   walkers skip `obj_type == 0` without reading the payload, the raw-header
//!   walker the rebuild uses reads headers only, address lookups reject
//!   `obj_type == 0`, and nothing traces a dead object's payload. Only the
//!   first payload word changes. A hole too small to carry the link (`size <
//!   OLD_FREE_MIN_HOLE`) is simply not listed.
//!
//! `OLD_ARENA_FREE_BYTES` tracks the total. It is deliberately NOT
//! subtracted from `OLD_GEN_IN_USE_BYTES` — that cache is defined (and
//! debug-asserted) as the sum of old block offsets, which hole reuse does
//! not change. Consumers that want old-generation pressure subtract
//! [`old_free_bytes`]. The live-allocation census used by
//! `process.memoryUsage().heapUsed` and major pacing excludes these holes at
//! collection time and observes later reuse through the decrease in this
//! counter; before this, dead-but-reusable bytes counted as pressure, so
//! reclaim kept re-firing full collections that could not lower the number it
//! was watching.
//!
//! Entries are only pushed for dead objects in blocks that still hold a
//! live object (fully-dead blocks go through block reclaim, which is
//! strictly better). A pushed entry's block can still die on a LATER
//! cycle, so every old-block reset/dealloc site must unlink the holes in
//! the range it is about to recycle ([`old_free_filter_range`]) BEFORE the
//! bytes are reused or released — the chain runs through those bytes, so a
//! hole left listed in a recycled block corrupts every chain it is on, not
//! just its own entry. Unlinking walks every chain, and a walk now chases
//! links through the heap instead of scanning a dense vector, so the rebuild
//! also records on each old block whether it listed a hole there
//! (`ArenaBlock::old_free_holes`); the reset sites filter only a block whose
//! bit is set, and every other dead block costs nothing.

use super::*;

/// A listed hole's link: the user pointer of the next hole of the same
/// size, stored in the hole's first payload word.
const OLD_FREE_LINK_BYTES: usize = std::mem::size_of::<usize>();

/// Smallest hole that can carry its link. Smaller holes stay dead capacity
/// until their block dies.
pub(super) const OLD_FREE_MIN_HOLE: usize = GC_HEADER_SIZE + OLD_FREE_LINK_BYTES;

/// Largest total size with a direct-indexed head. Sizes above it (and any
/// size that is not a multiple of 8) keep their head in the size -> head
/// map instead.
pub(super) const OLD_FREE_SMALL_MAX: usize = 2048;
const OLD_FREE_SMALL_CLASSES: usize = OLD_FREE_SMALL_MAX / 8 + 1;

/// Per-size chain heads. A head is the user pointer of the most recently
/// listed hole of that size, `0` when none is listed.
struct OldFreeHeads {
    small: [usize; OLD_FREE_SMALL_CLASSES],
    large: crate::fast_hash::PtrHashMap<usize, usize>,
}

impl OldFreeHeads {
    fn new() -> Self {
        Self {
            small: [0; OLD_FREE_SMALL_CLASSES],
            large: crate::fast_hash::new_ptr_hash_map(),
        }
    }

    #[inline]
    fn small_class(total_size: usize) -> Option<usize> {
        (total_size <= OLD_FREE_SMALL_MAX && total_size % 8 == 0).then_some(total_size / 8)
    }

    /// The head for `total_size`, if one exists. `0` means an empty chain.
    #[inline]
    fn head_mut(&mut self, total_size: usize) -> Option<&mut usize> {
        match Self::small_class(total_size) {
            Some(class) => Some(&mut self.small[class]),
            None => self.large.get_mut(&total_size),
        }
    }

    fn head_mut_or_insert(&mut self, total_size: usize) -> &mut usize {
        match Self::small_class(total_size) {
            Some(class) => &mut self.small[class],
            None => self.large.entry(total_size).or_insert(0),
        }
    }

    /// Forget every chain. The map keeps its capacity, so a rebuild after
    /// warm-up allocates nothing.
    fn clear(&mut self) {
        self.small.fill(0);
        self.large.clear();
    }

    /// Call `f(total_size, head)` for every non-empty chain.
    fn for_each_chain(&mut self, mut f: impl FnMut(usize, &mut usize)) {
        for (class, head) in self.small.iter_mut().enumerate() {
            if *head != 0 {
                f(class * 8, head);
            }
        }
        for (&total_size, head) in self.large.iter_mut() {
            if *head != 0 {
                f(total_size, head);
            }
        }
    }
}

crate::perry_thread_local! {
    /// Size -> head of the intrusive chain of swept holes of exactly that
    /// size. The holes themselves carry the rest of each chain.
    static OLD_FREE_MAP: RefCell<OldFreeHeads> = RefCell::new(OldFreeHeads::new());
    static OLD_FREE_BYTES: super::TriggerInput<usize> = const { super::TriggerInput::new(0) };
    static OLD_FREE_NONEMPTY: Cell<bool> = const { Cell::new(false) };
}

/// Read a listed hole's link.
///
/// # Safety
/// `hole` is the user pointer of a hole currently on the list: its block is
/// mapped and not yet recycled (the reset sites unlink before recycling),
/// and it is at least [`OLD_FREE_MIN_HOLE`] bytes, so the link word lies
/// inside it.
#[inline]
unsafe fn old_free_link(hole: usize) -> usize {
    *(hole as *const usize)
}

/// Write a hole's link. Same contract as [`old_free_link`]; the word is dead
/// payload, so the store needs no barrier and nothing else reads it.
#[inline]
unsafe fn old_free_set_link(hole: usize, next: usize) {
    *(hole as *mut usize) = next;
}

/// Unlink every hole in the chain at `head` for which `keep` returns false,
/// returning how many were unlinked. `keep` receives the hole's header
/// address.
///
/// # Safety
/// Every hole on the chain satisfies [`old_free_link`]'s contract.
unsafe fn old_free_retain_chain(head: &mut usize, mut keep: impl FnMut(usize) -> bool) -> usize {
    let mut removed = 0usize;
    // `link` is the word that points at `hole`: the head, then each kept
    // hole's own link word.
    let mut link: *mut usize = head;
    loop {
        let hole = *link;
        if hole == 0 {
            return removed;
        }
        let next = old_free_link(hole);
        if keep(hole - GC_HEADER_SIZE) {
            link = hole as *mut usize;
        } else {
            *link = next;
            removed += 1;
        }
    }
}

fn old_free_sync_nonempty() {
    let nonempty = OLD_FREE_BYTES.with(TriggerInput::get) != 0;
    OLD_FREE_NONEMPTY.with(|c| c.set(nonempty));
}

/// Total bytes currently sitting in reusable old-gen holes.
pub(crate) fn old_free_bytes() -> usize {
    OLD_FREE_BYTES.with(TriggerInput::get)
}

/// Hot-cache slot claimed by `OLD_FREE_BYTES`, which
/// `gc_budgeted_due_trigger` reads on every `gc_malloc`. Liveness
/// instrumentation for `gc::tests::trigger_path_tls`.
#[cfg(test)]
pub(crate) fn old_free_bytes_slot_index() -> u32 {
    OLD_FREE_BYTES.slot_index()
}

/// List one hole. Returns whether it was listed: a hole too small to carry
/// its link is not.
fn old_free_push(user_ptr: usize, total_size: usize) -> bool {
    crate::gc::heap_generation::debug_assert_heap_change_open();
    if user_ptr == 0 || total_size < OLD_FREE_MIN_HOLE {
        return false;
    }
    OLD_FREE_MAP.with(|m| {
        let mut heads = m.borrow_mut();
        let head = heads.head_mut_or_insert(total_size);
        // SAFETY: the caller hands a hole inside a mapped old block, at
        // least OLD_FREE_MIN_HOLE bytes long.
        unsafe { old_free_set_link(user_ptr, *head) };
        *head = user_ptr;
    });
    OLD_FREE_BYTES.with(|c| c.set(c.get().saturating_add(total_size)));
    OLD_FREE_NONEMPTY.with(|c| c.set(true));
    true
}

/// Rebuild the hole list from the heap itself: every invalidated dead
/// header (`obj_type == 0` — only `invalidate_dead_old_arena_header`
/// produces those; no live object has type 0) inside an old block that
/// still holds a live object. Called at the completion point of every
/// old-reclaiming sweep, replacing whatever the list held.
///
/// Rebuilding beats accumulating a staging vector during the sweep walk on
/// two counts, both measured on `12_large_live_set` (~700k dead old
/// objects): the staging vector alone added ~17 MB of peak RSS to the very
/// number this feature exists to lower, and rebuild is idempotent — a hole
/// consumed by reuse gets a real `obj_type` and drops out, a hole whose
/// block died is never visited, so no cross-sweep dedup bookkeeping can
/// drift. The walk is block-filtered (live old blocks only), so its cost
/// is O(objects in surviving old blocks), paid only on reclaim sweeps.
/// Threading the list through the holes makes it allocation-free as well:
/// the walk already has each hole's header in cache, and listing it is one
/// store to the word beside it.
pub(super) fn old_free_rebuild_from_live_old_blocks(
    block_has_live: &[bool],
    old_block_start: usize,
) {
    old_free_rebuild_from_old_blocks(|block_idx| {
        block_idx >= old_block_start && block_has_live.get(block_idx).copied().unwrap_or(false)
    });
}

/// [`old_free_rebuild_from_live_old_blocks`] over the old blocks `parse`
/// selects (global block indices).
pub(super) fn old_free_rebuild_from_old_blocks(parse: impl FnMut(usize) -> bool) {
    old_free_clear();
    // The raw-headers walker is load-bearing: the walkable-gated walkers
    // (`arena_walk_objects_filtered` and friends) step over invalidated
    // headers WITHOUT invoking the callback, so a rebuild written against
    // them silently records zero holes. It also maintains each old block's
    // `old_free_holes` bit from the callback's answer, which is what lets
    // the block reset sites skip the chain walk for a block with no hole.
    crate::arena::old_arena_walk_all_headers_listing_holes(parse, |header_ptr, _block_idx| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            if (*header).obj_type == 0 {
                let total_size = (*header).size as usize;
                return old_free_push(header as usize + GC_HEADER_SIZE, total_size);
            }
        }
        false
    });
}

/// Drop every listed hole. The holes stay invalidated in the heap, so the
/// next rebuild finds them again.
fn old_free_clear() {
    OLD_FREE_MAP.with(|m| m.borrow_mut().clear());
    OLD_FREE_BYTES.with(|c| c.set(0));
    OLD_FREE_NONEMPTY.with(|c| c.set(false));
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
        let mut heads = m.borrow_mut();
        let head = heads.head_mut(total_size)?;
        match excluded_pages {
            None => {
                let hole = *head;
                if hole == 0 {
                    return None;
                }
                // SAFETY: `hole` is listed.
                *head = unsafe { old_free_link(hole) };
                Some(hole)
            }
            Some(excluded) => {
                // Unlink the first hole wholly off the excluded pages and
                // stop: the rest of the chain is not walked.
                let mut link: *mut usize = head;
                // SAFETY: every hole on the chain is listed.
                unsafe {
                    loop {
                        let hole = *link;
                        if hole == 0 {
                            return None;
                        }
                        let header = hole - GC_HEADER_SIZE;
                        let first = crate::arena::generation_page_for_addr(header);
                        let last = crate::arena::generation_page_for_addr(header + total_size - 1);
                        if !(first..=last).any(|page| excluded.contains(&page)) {
                            *link = old_free_link(hole);
                            return Some(hole);
                        }
                        link = hole as *mut usize;
                    }
                }
            }
        }
    })?;
    OLD_FREE_BYTES.with(|c| c.set(c.get().saturating_sub(total_size)));
    old_free_sync_nonempty();
    Some(taken)
}

/// Unlink every listed hole whose header `unlink(header, total_size)` selects,
/// returning the bytes removed.
fn old_free_unlink_where(mut unlink: impl FnMut(usize, usize) -> bool) -> usize {
    let mut removed_bytes = 0usize;
    OLD_FREE_MAP.with(|m| {
        m.borrow_mut().for_each_chain(|total_size, head| {
            // SAFETY: every hole on the chain is listed.
            let removed =
                unsafe { old_free_retain_chain(head, |header| !unlink(header, total_size)) };
            removed_bytes = removed_bytes.saturating_add(removed.saturating_mul(total_size));
        });
    });
    OLD_FREE_BYTES.with(|c| c.set(c.get().saturating_sub(removed_bytes)));
    old_free_sync_nonempty();
    removed_bytes
}

/// Unlink every hole inside `[base, base + size)`. Called by the old-block
/// reset/dealloc paths before they recycle a block's bytes — a stale
/// entry would otherwise hand out a pointer into memory the bump
/// allocator is about to overwrite (or that has been returned to the OS),
/// and every chain threaded through it would follow a link that is no
/// longer there. The sites call it only for a block whose
/// `old_free_holes` bit is set; see the module docs.
pub(crate) fn old_free_filter_range(base: usize, size: usize) {
    if !OLD_FREE_NONEMPTY.with(Cell::get) || size == 0 {
        return;
    }
    let end = base.saturating_add(size);
    old_free_unlink_where(|header, _| header >= base && header < end);
}

/// Drop every hole that lies on one of `excluded_pages`, returning the bytes
/// removed.
///
/// An evacuation pass may not reuse a hole on a page it is evacuating, and
/// `old_free_take_exact` enforces that per allocation by scanning the size
/// chain for the first entry that is not excluded. When the excluded pages
/// are exactly the fragmented ones — which is the whole point of a defrag
/// pass — essentially every hole in the list is excluded, so that scan walks
/// the entire chain, fails, and falls through to the bump allocator, once
/// per moved object. Measured on the #9644 fixture before this existed:
/// 235,241 objects evacuated in 139 SECONDS, all of it inside the evacuation
/// phase (`phase_us.evacuation = 139381527`).
///
/// The holes on those pages are unusable for the duration of the pass and the
/// block is released at the end of it, so drop them once here — an O(free
/// list) pass instead of O(moved objects x free list).
pub(crate) fn old_free_filter_pages(excluded_pages: &crate::fast_hash::PtrHashSet<usize>) -> usize {
    if !OLD_FREE_NONEMPTY.with(Cell::get) || excluded_pages.is_empty() {
        return 0;
    }
    old_free_unlink_where(|header, total_size| {
        let first = crate::arena::generation_page_for_addr(header);
        let last = crate::arena::generation_page_for_addr(header + total_size - 1);
        (first..=last).any(|page| excluded_pages.contains(&page))
    })
}

#[cfg(test)]
pub(super) fn old_free_push_for_test(user_ptr: usize, total_size: usize) {
    let _heap_change = crate::gc::heap_generation::HeapChange::begin(
        crate::gc::heap_generation::HeapChangeKind::Sweep,
    );
    if old_free_push(user_ptr, total_size) {
        // A hole listed outside a rebuild must still be unlinked when its
        // block is recycled.
        crate::arena::old_arena_note_listed_hole_for_test(user_ptr - GC_HEADER_SIZE);
    }
}

#[cfg(test)]
pub(super) fn old_free_reset_for_test() {
    old_free_clear();
}

/// Every listed hole's user pointer, chain by chain (each chain head first),
/// with its size.
#[cfg(test)]
pub(super) fn old_free_listed_for_test() -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    OLD_FREE_MAP.with(|m| {
        m.borrow_mut().for_each_chain(|total_size, head| {
            let mut hole = *head;
            while hole != 0 {
                out.push((hole, total_size));
                // SAFETY: `hole` is listed.
                hole = unsafe { old_free_link(hole) };
            }
        });
    });
    out
}

#[cfg(test)]
pub(super) fn old_free_entry_count() -> usize {
    old_free_listed_for_test().len()
}
