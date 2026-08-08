use super::*;

pub(crate) const GENERATION_PAGE_SHIFT: usize = 12;
// Generation classification wants exact range answers, but it does
// not need a separate hash entry for every 4 KiB remembered-set card.
// A 1 MiB bucket matches the arena block scale, keeps lookup bounded,
// and avoids thousands of metadata entries for low-pressure nursery
// churn before the first GC.
pub(crate) const GENERATION_CLASS_SHIFT: usize = 20;
pub(crate) const GENERATION_PAGE_SIZE: usize = 1 << GENERATION_PAGE_SHIFT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapGeneration {
    Unknown,
    Nursery,
    Longlived,
    Old,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapSpace {
    Unknown,
    NurseryEden,
    Survivor0,
    Survivor1,
    Longlived,
    Old,
}

impl HeapSpace {
    #[inline]
    pub(crate) fn is_nursery(self) -> bool {
        matches!(
            self,
            HeapSpace::NurseryEden | HeapSpace::Survivor0 | HeapSpace::Survivor1
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PageGenerationRange {
    base: usize,
    end: usize,
    generation: HeapGeneration,
    space: HeapSpace,
}

impl PageGenerationRange {
    #[inline]
    fn contains(self, addr: usize) -> bool {
        addr >= self.base && addr < self.end
    }
}

#[derive(Clone, Debug)]
enum PageGenerationSlot {
    Single(PageGenerationRange),
    Multiple(Vec<PageGenerationRange>),
}

impl PageGenerationSlot {
    #[inline]
    fn find(&self, addr: usize) -> Option<PageGenerationRange> {
        match self {
            PageGenerationSlot::Single(range) => range.contains(addr).then_some(*range),
            PageGenerationSlot::Multiple(ranges) => {
                ranges.iter().copied().find(|range| range.contains(addr))
            }
        }
    }

    fn insert(&mut self, range: PageGenerationRange) {
        match self {
            PageGenerationSlot::Single(existing) => {
                if *existing == range {
                    return;
                }
                *self = PageGenerationSlot::Multiple(vec![*existing, range]);
            }
            PageGenerationSlot::Multiple(ranges) => {
                if !ranges.contains(&range) {
                    ranges.push(range);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct PageGenerationCache {
    key: usize,
    range: PageGenerationRange,
    valid: bool,
}

impl PageGenerationCache {
    const fn empty() -> Self {
        Self {
            key: 0,
            range: PageGenerationRange {
                base: 0,
                end: 0,
                generation: HeapGeneration::Unknown,
                space: HeapSpace::Unknown,
            },
            valid: false,
        }
    }
}

/// Ways in the [`PageGenerationCacheSet`] below.
///
/// #7469: this was a **one**-entry cache, and the write barrier classifies at
/// least two unrelated addresses per store — the child being written and the
/// object being written into. On `churn.ts` those live in different 1 MiB
/// generation classes, so consecutive classifications evicted each other and
/// the "cache" missed on essentially every call: 71 self samples in the
/// authoritative map lookup that the cache exists to avoid. Four ways covers
/// the barrier's working set (child, parent, array payload, header) with room
/// to spare; it is still a fixed-size array probed linearly, so a hit is a few
/// compares off one cache line.
const PAGE_GENERATION_CACHE_WAYS: usize = 4;

/// Small direct-probed cache in front of [`PageGenerationMap`].
///
/// Pure accelerator: a miss, a stale way, or a full set all fall through to
/// the authoritative map, so the only thing correctness depends on is that
/// every invalidation clears **all** ways — which is why
/// [`invalidate_generation_cache`] resets the whole set rather than one entry.
///
/// Stored behind an `UnsafeCell`, not a `Cell`: `Cell::get` returns a **copy**,
/// and copying ~200 bytes on every classification cost more than the map lookup
/// the cache exists to avoid (measured as a ~2% regression on `retain.ts`
/// before this was switched). Access is single-threaded by construction — the
/// cache is thread-local and no path holds a reference across a call that could
/// re-enter classification.
#[derive(Clone, Copy)]
struct PageGenerationCacheSet {
    ways: [PageGenerationCache; PAGE_GENERATION_CACHE_WAYS],
    /// Round-robin victim for the next insert.
    next: usize,
}

impl PageGenerationCacheSet {
    const fn empty() -> Self {
        Self {
            ways: [PageGenerationCache::empty(); PAGE_GENERATION_CACHE_WAYS],
            next: 0,
        }
    }

    #[inline(always)]
    fn lookup(&self, key: usize, addr: usize) -> Option<PageGenerationRange> {
        for way in self.ways.iter() {
            if way.valid && way.key == key && way.range.contains(addr) {
                return Some(way.range);
            }
        }
        None
    }

    #[inline]
    fn insert(&mut self, key: usize, range: PageGenerationRange) {
        let slot = self.next % PAGE_GENERATION_CACHE_WAYS;
        self.ways[slot] = PageGenerationCache {
            key,
            range,
            valid: true,
        };
        self.next = slot.wrapping_add(1);
    }
}

/// #7187: this map used to carry a bespoke identity hasher (`write_usize`
/// stored the key verbatim). `HashMap` is hashbrown, which takes the bucket
/// index from the hash's LOW bits and the SIMD control byte from
/// `hash >> 57`. Keys here are `addr >> GENERATION_CLASS_SHIFT` — around 2^26
/// for a typical heap address — so the top seven bits were zero for **every**
/// entry in the table, and every group probe matched every occupied slot in
/// the group. Each of those matches costs a real key comparison (a scattered
/// load into a bucket) before the right one is found, on a lookup that the
/// write barrier performs several times per heap store.
///
/// `fast_hash::PtrHasher` is the project's existing answer to exactly this —
/// see its module doc, and the `mix(h) = h ^ (h >> 32)` avalanche step whose
/// comment records a 455 ms → 830 ms regression from omitting it. The map is
/// only ever point-queried (`get` / `get_mut` / `insert` / `remove`; the
/// `first_key..=last_key` loops walk key *ranges*, not the map), so iteration
/// order is not observable and this carries no determinism exposure.
type PageGenerationMap = crate::fast_hash::PtrHashMap<usize, PageGenerationSlot>;
type OldGenPageObjectMap = crate::fast_hash::PtrHashMap<usize, Vec<usize>>;
type OldGenPageMetaMap = crate::fast_hash::PtrHashMap<usize, OldPageMeta>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OldPageMeta {
    pub(crate) page_base: usize,
    pub(crate) page_end: usize,
    pub(crate) allocated_bytes: usize,
    pub(crate) live_bytes: usize,
    pub(crate) dead_bytes: usize,
    pub(crate) object_count: usize,
    pub(crate) live_object_count: usize,
    pub(crate) dead_object_count: usize,
    pub(crate) pinned_bytes: usize,
    pub(crate) pinned_object_count: usize,
    pub(crate) dirty_slots: usize,
    /// Epoch stamp for `dirty_slots` (#6181). `dirty_slots` is a per-cycle
    /// count that used to be zeroed for every old page at the start of each
    /// GC cycle by `old_pages_begin_gc_cycle` — an O(number of old pages)
    /// walk on every minor whose cost grows with old-gen size. Instead of
    /// the eager reset, each page records the epoch its `dirty_slots` was
    /// last stamped in; the cycle-start "reset" is now a single O(1) bump of
    /// the thread-local `OLD_GEN_PAGE_DIRTY_EPOCH`, and a page whose stamp is
    /// stale reads as zero (`effective_dirty_slots`). Mirrors the
    /// generation-counter lazy-invalidation pattern in `native_arena.rs`.
    pub(crate) dirty_slots_epoch: u64,
    pub(crate) dirty: bool,
    pub(crate) evacuation_eligible: bool,
}

impl OldPageMeta {
    #[inline]
    fn zero_for_page(page: usize) -> Self {
        let page_base = generation_page_base(page);
        Self {
            page_base,
            page_end: page_base + GENERATION_PAGE_SIZE,
            allocated_bytes: 0,
            live_bytes: 0,
            dead_bytes: 0,
            object_count: 0,
            live_object_count: 0,
            dead_object_count: 0,
            pinned_bytes: 0,
            pinned_object_count: 0,
            dirty_slots: 0,
            // Epoch 0 never matches a live cycle epoch (which starts at 1), so
            // a freshly materialized page reads zero dirty slots until it is
            // stamped by the remembered-set scan (#6181).
            dirty_slots_epoch: 0,
            dirty: false,
            evacuation_eligible: false,
        }
    }

    /// `dirty_slots` scoped to `current_epoch`: a page last stamped in an
    /// earlier cycle (its `dirty_slots_epoch` is stale) has no dirty slots
    /// this cycle. This is what makes the O(1) epoch bump in
    /// `old_pages_begin_gc_cycle` equivalent to the old per-page reset (#6181).
    #[inline]
    pub(crate) fn effective_dirty_slots(&self, current_epoch: u64) -> usize {
        if self.dirty_slots_epoch == current_epoch {
            self.dirty_slots
        } else {
            0
        }
    }

    #[inline]
    fn reset_cycle_sweep_accounting(&mut self) {
        self.live_bytes = 0;
        self.dead_bytes = 0;
        self.pinned_bytes = 0;
        self.live_object_count = 0;
        self.dead_object_count = 0;
        self.pinned_object_count = 0;
        self.evacuation_eligible = false;
    }

    #[inline]
    fn refresh_policy_bits(&mut self) {
        self.evacuation_eligible = self.allocated_bytes > 0
            && self.live_bytes > 0
            && self.dead_bytes > 0
            && self.pinned_bytes == 0;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OldPageSummary {
    pub(crate) pages: usize,
    pub(crate) allocated_bytes: usize,
    pub(crate) live_bytes: usize,
    pub(crate) dead_bytes: usize,
    pub(crate) reusable_bytes: usize,
    pub(crate) returned_bytes: usize,
    pub(crate) pinned_bytes: usize,
    pub(crate) object_count: usize,
    pub(crate) live_object_count: usize,
    pub(crate) dead_object_count: usize,
    pub(crate) pinned_object_count: usize,
    pub(crate) dirty_pages: usize,
    pub(crate) dirty_slots: usize,
    pub(crate) fragmented_pages: usize,
    pub(crate) evacuation_eligible_pages: usize,
}

#[derive(Default)]
pub(crate) struct OldArenaSourceBlockSelection {
    pub(crate) block_indices: crate::fast_hash::PtrHashSet<usize>,
    pub(crate) pages: crate::fast_hash::PtrHashSet<usize>,
}

thread_local! {
    static PAGE_GENERATIONS: RefCell<PageGenerationMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    static PAGE_GENERATION_CACHE: UnsafeCell<PageGenerationCacheSet> =
        const { UnsafeCell::new(PageGenerationCacheSet::empty()) };

    static OLD_GEN_PAGE_OBJECTS: RefCell<OldGenPageObjectMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    static OLD_GEN_PAGE_META: RefCell<OldGenPageMetaMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    pub(crate) static OLD_GEN_RECLAIM_REUSABLE_BYTES: Cell<usize> = const { Cell::new(0) };
    pub(crate) static OLD_GEN_RECLAIM_RETURNED_BYTES: Cell<usize> = const { Cell::new(0) };

    /// Monotonic per-cycle epoch for old-page `dirty_slots` (#6181). Bumped
    /// once per GC cycle by `old_pages_begin_gc_cycle` instead of walking every
    /// old page to zero its `dirty_slots`. Starts at 1 so a freshly allocated
    /// page (stamp 0, see `OldPageMeta::zero_for_page`) reads as having no
    /// dirty slots. `u64` never wraps in practice (one bump per collection).
    static OLD_GEN_PAGE_DIRTY_EPOCH: Cell<u64> = const { Cell::new(1) };
}

// --- #7469 hot-TLS address providers. See `crate::tls_hot`. ---

/// Address of this thread's `PAGE_GENERATION_CACHE`.
pub(crate) fn page_generation_cache_hot_addr() -> *mut u8 {
    PAGE_GENERATION_CACHE.with(|c| c.get() as *mut u8)
}

/// Address of this thread's `PAGE_GENERATIONS`.
pub(crate) fn page_generations_hot_addr() -> *mut u8 {
    PAGE_GENERATIONS.with(|p| p as *const _ as *mut u8)
}

/// [`PAGE_GENERATION_CACHE`] without a TLS resolution — see `crate::tls_hot`.
#[inline(always)]
fn hot_page_generation_cache() -> *mut PageGenerationCacheSet {
    // SAFETY: the slot is filled from `page_generation_cache_hot_addr` above,
    // and `tls_hot::tests::cached_addresses_match_thread_locals` asserts the
    // pairing.
    crate::tls_hot::hot().page_generation_cache as *mut PageGenerationCacheSet
}

/// [`PAGE_GENERATIONS`] without a TLS resolution — see `crate::tls_hot`.
#[inline(always)]
fn hot_page_generations() -> &'static RefCell<PageGenerationMap> {
    // SAFETY: as above, paired with `page_generations_hot_addr`.
    unsafe { &*(crate::tls_hot::hot().page_generations as *const RefCell<PageGenerationMap>) }
}

#[inline]
fn old_gen_page_dirty_epoch() -> u64 {
    OLD_GEN_PAGE_DIRTY_EPOCH.with(|epoch| epoch.get())
}

#[inline]
pub(crate) fn generation_page_for_addr(addr: usize) -> usize {
    addr >> GENERATION_PAGE_SHIFT
}

#[inline]
fn generation_class_key_for_addr(addr: usize) -> usize {
    addr >> GENERATION_CLASS_SHIFT
}

#[inline]
pub(crate) fn generation_page_base(page: usize) -> usize {
    page << GENERATION_PAGE_SHIFT
}

#[inline]
fn invalidate_generation_cache() {
    // Every way, not one — a stale way is exactly what this guards against.
    // SAFETY: thread-local, single-threaded.
    PAGE_GENERATION_CACHE.with(|cache| unsafe { *cache.get() = PageGenerationCacheSet::empty() });
}

fn register_old_block_pages(base: usize, size: usize) {
    if base == 0 || size == 0 {
        return;
    }
    let end = base + size;
    let first_page = generation_page_for_addr(base);
    let last_page = generation_page_for_addr(end - 1);
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for page in first_page..=last_page {
            meta.entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
        }
    });
}

pub(crate) fn unregister_old_block_pages(pages: &[usize]) {
    if pages.is_empty() {
        return;
    }
    // #7625 REMOVER: a deferred entry for one of these pages must be folded in
    // BEFORE the page is dropped, or the flush would re-add it afterwards and
    // hand a later walk a header inside a recycled block.
    flush_deferred_old_page_registrations();
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for &page in pages {
            meta.remove(&page);
        }
    });
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for &page in pages {
            index.remove(&page);
        }
    });
    // #7187 Phase B: the other place a page's dirty stamp stops existing — the
    // metadata entry itself is gone. A cached page whose metadata was dropped
    // is no longer a complete recording, so drop the cache.
    crate::gc::dirty_page_cache_invalidate();
}

#[inline]
pub(crate) fn address_span_overlaps_pages(
    start: usize,
    size: usize,
    pages: &crate::fast_hash::PtrHashSet<usize>,
) -> bool {
    if start == 0 || size == 0 || pages.is_empty() {
        return false;
    }
    let Some(end) = start.checked_add(size) else {
        return true;
    };
    let first_page = generation_page_for_addr(start);
    let last_page = generation_page_for_addr(end - 1);
    (first_page..=last_page).any(|page| pages.contains(&page))
}

pub(crate) fn register_block_space(
    base: usize,
    size: usize,
    generation: HeapGeneration,
    space: HeapSpace,
) {
    if base == 0 || size == 0 || matches!(generation, HeapGeneration::Unknown) {
        return;
    }
    let end = base + size;
    let range = PageGenerationRange {
        base,
        end,
        generation,
        space,
    };
    let first_key = generation_class_key_for_addr(base);
    let last_key = generation_class_key_for_addr(end - 1);
    PAGE_GENERATIONS.with(|pages| {
        let mut pages = pages.borrow_mut();
        for key in first_key..=last_key {
            match pages.entry(key) {
                Entry::Occupied(mut entry) => entry.get_mut().insert(range),
                Entry::Vacant(entry) => {
                    entry.insert(PageGenerationSlot::Single(range));
                }
            }
        }
    });
    if matches!(generation, HeapGeneration::Old) {
        register_old_block_pages(base, size);
    }
    invalidate_generation_cache();
}

pub(crate) fn unregister_block_generation(base: usize, size: usize) {
    if base == 0 || size == 0 {
        return;
    }
    let end = base + size;
    let first_key = generation_class_key_for_addr(base);
    let last_key = generation_class_key_for_addr(end - 1);
    let mut removed_old_block = false;
    PAGE_GENERATIONS.with(|pages| {
        let mut pages = pages.borrow_mut();
        for key in first_key..=last_key {
            let mut remove_page = false;
            let mut replacement = None;
            if let Some(slot) = pages.get_mut(&key) {
                match slot {
                    PageGenerationSlot::Single(range) => {
                        if range.base == base && range.end == end {
                            removed_old_block |= matches!(range.generation, HeapGeneration::Old);
                            remove_page = true;
                        }
                    }
                    PageGenerationSlot::Multiple(ranges) => {
                        ranges.retain(|range| {
                            let remove = range.base == base && range.end == end;
                            if remove && matches!(range.generation, HeapGeneration::Old) {
                                removed_old_block = true;
                            }
                            !remove
                        });
                        if ranges.is_empty() {
                            remove_page = true;
                        } else if ranges.len() == 1 {
                            replacement = Some(PageGenerationSlot::Single(ranges[0]));
                        }
                    }
                }
            }
            if remove_page {
                pages.remove(&key);
            } else if let Some(slot) = replacement {
                pages.insert(key, slot);
            }
        }
    });
    if removed_old_block {
        let first_page = generation_page_for_addr(base);
        let last_page = generation_page_for_addr(end - 1);
        let old_pages_to_unregister: Vec<usize> = (first_page..=last_page).collect();
        unregister_old_block_pages(&old_pages_to_unregister);
    }
    invalidate_generation_cache();
}

/// #7469: split so the **cache-hit** arm is small enough to actually inline
/// into its callers. A single `js_write_barrier_slot` classifies twice (child
/// then parent) and `write_barrier_decoded_parent` classifies again; with the
/// miss path inlined alongside, the whole thing stayed out of line and each
/// call paid its own `_tlv_get_addr`. Out-of-lining the miss lets the hit arm
/// inline, and LLVM then CSEs the one remaining hot-TLS resolution across
/// every classification in the barrier.
#[inline(always)]
pub(crate) fn classify_heap_generation(addr: usize) -> HeapGeneration {
    if addr == 0 {
        return HeapGeneration::Unknown;
    }
    let key = generation_class_key_for_addr(addr);
    // Both tables come off the one cached hot-TLS base — this runs on every
    // `decode_heap_addr`, i.e. every heap store the write barrier sees, and was
    // the single largest `_tlv_get_addr` caller on `churn.ts` (116 of 653
    // attributed samples) precisely because it resolved two distinct
    // thread-locals per call.
    // SAFETY: thread-local, single-threaded, and the borrow ends here.
    if let Some(range) = unsafe { (*hot_page_generation_cache()).lookup(key, addr) } {
        return range.generation;
    }
    classify_heap_generation_uncached(addr, key)
}

/// Cache-miss arm of [`classify_heap_generation`]: consult the page map and
/// re-prime the one-entry cache.
#[inline(never)]
fn classify_heap_generation_uncached(addr: usize, key: usize) -> HeapGeneration {
    let found = {
        let pages = hot_page_generations().borrow();
        pages.get(&key).and_then(|slot| slot.find(addr))
    };
    if let Some(range) = found {
        // SAFETY: as above.
        unsafe { (*hot_page_generation_cache()).insert(key, range) };
        range.generation
    } else {
        HeapGeneration::Unknown
    }
}

#[inline]
pub(crate) fn classify_heap_space(addr: usize) -> HeapSpace {
    if addr == 0 {
        return HeapSpace::Unknown;
    }
    let key = generation_class_key_for_addr(addr);
    // SAFETY: thread-local, single-threaded, and the borrow ends here.
    if let Some(range) = unsafe { (*hot_page_generation_cache()).lookup(key, addr) } {
        return range.space;
    }

    let found = {
        let pages = hot_page_generations().borrow();
        pages.get(&key).and_then(|slot| slot.find(addr))
    };
    if let Some(range) = found {
        // SAFETY: as above.
        unsafe { (*hot_page_generation_cache()).insert(key, range) };
        range.space
    } else {
        HeapSpace::Unknown
    }
}

pub(crate) fn old_object_page_overlaps(
    header_addr: usize,
    total_size: usize,
) -> Vec<(usize, usize)> {
    if header_addr == 0 || total_size == 0 {
        return Vec::new();
    }
    let object_end = header_addr + total_size;
    let first_page = generation_page_for_addr(header_addr);
    let last_page = generation_page_for_addr(object_end - 1);
    let mut overlaps = Vec::with_capacity(last_page - first_page + 1);
    for page in first_page..=last_page {
        let page_base = generation_page_base(page);
        let page_end = page_base + GENERATION_PAGE_SIZE;
        let overlap_start = header_addr.max(page_base);
        let overlap_end = object_end.min(page_end);
        if overlap_start < overlap_end {
            overlaps.push((page, overlap_end - overlap_start));
        }
    }
    overlaps
}

fn update_old_page_meta_for_object(page_updates: &[(usize, usize)], adding: bool) {
    if page_updates.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for &(page, bytes) in page_updates {
            let page_meta = meta
                .entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
            if adding {
                page_meta.allocated_bytes = page_meta.allocated_bytes.saturating_add(bytes);
                page_meta.object_count = page_meta.object_count.saturating_add(1);
            } else {
                page_meta.allocated_bytes = page_meta.allocated_bytes.saturating_sub(bytes);
                page_meta.object_count = page_meta.object_count.saturating_sub(1);
                if page_meta.allocated_bytes == 0 && page_meta.object_count == 0 {
                    page_meta.reset_cycle_sweep_accounting();
                }
            }
            page_meta.refresh_policy_bits();
        }
    });
}

pub(crate) fn register_old_object_pages(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    let mut added_pages = Vec::with_capacity(overlaps.len());
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for &(page, bytes) in &overlaps {
            let headers = index.entry(page).or_insert_with(Vec::new);
            if !headers.contains(&header_addr) {
                headers.push(header_addr);
                added_pages.push((page, bytes));
            }
        }
    });
    update_old_page_meta_for_object(&added_pages, true);
}

// ---------------------------------------------------------------------------
// #7625: deferred old-object page registration.
//
// `register_old_object_pages` above is written for the OCCASIONAL old-gen
// birth it was introduced for. Per call it pays two `RefCell` borrows, two
// `Vec` allocations, a hash lookup, and a **linear `contains` scan of the
// page's object list** — and that scan grows as the page fills, so a burst of
// births into one 4 KiB page is quadratic in the objects it lands there.
//
// #7613's promote-on-first-copy made that path hot on ordinary workloads: a
// copying minor promotes straight into old-gen (`gc/copying.rs`'s `move_young`
// → `arena_alloc_gc_old`), so json_pipeline pushes ~113 MB of promotions per
// run through it. Deferring lets the whole burst be registered in ONE batch,
// where the per-page list length is captured once and the dedup scan only has
// to cover the entries that predate the batch — zero comparisons for the fresh
// pages a bump-allocated promotion burst actually fills.
//
// SOUNDNESS. The deferral is invisible because **every reader and every
// remover of `OLD_GEN_PAGE_OBJECTS`/`OLD_GEN_PAGE_META` flushes first**, so
// the index is exactly what eager registration would have left at each
// observation point. Ordering matters in both directions: a remover that ran
// before the flush would be a no-op and the flush would then RESURRECT a dead
// entry, which is why removers flush too and not only readers. The flush sites
// are enumerated in `deferred_registration_flush_sites` in `arena/tests.rs`,
// which fails if a new toucher of either table appears without one.
// ---------------------------------------------------------------------------

thread_local! {
    /// Old-object page registrations not yet folded into `OLD_GEN_PAGE_OBJECTS`.
    /// Entries are `(header_addr, total_size)`; nothing here is dereferenced, so
    /// a deferred entry never keeps an object alive and is not a GC root — and
    /// the flush discipline above means the buffer is provably EMPTY at every
    /// point a collector could observe it (asserted by
    /// `deferred_buffer_is_empty_after_every_cycle_constructor`).
    static DEFERRED_OLD_PAGE_REGISTRATIONS: RefCell<Vec<(usize, usize)>> =
        const { RefCell::new(Vec::new()) };
}

/// Cap chosen so the buffer's worst-case footprint (16 B/entry × 64k = 1 MB)
/// stays a rounding error while flushes stay rare on an allocation burst.
pub(crate) const DEFERRED_OLD_PAGE_REGISTRATION_CAP: usize = 65_536;

/// Record `header_addr`'s page registration for the next flush instead of
/// performing it now. Callers must be old-gen births; see the module comment
/// for why this is invisible to every consumer of the index.
#[inline]
pub(crate) fn defer_old_object_page_registration(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let full = DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.push((header_addr, total_size));
        buf.len() >= DEFERRED_OLD_PAGE_REGISTRATION_CAP
    });
    if full {
        flush_deferred_old_page_registrations_batch();
    }
}

/// Make the page-objects index complete. Cheap (one thread-local read) when
/// nothing is pending, which is the case at all but a handful of GC-time calls.
#[inline]
pub(crate) fn flush_deferred_old_page_registrations() {
    if DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().is_empty()) {
        return;
    }
    flush_deferred_old_page_registrations_batch();
}

/// The batched drain. Equivalent to `register_old_object_pages` per entry, but
/// holding one borrow of each table for the whole batch and — the part that
/// removes the quadratic term — scanning only the portion of a page's object
/// list that PREDATES this batch.
///
/// Skipping the in-batch entries is sound because they are pairwise distinct:
/// each comes from a live allocation, and an address cannot be handed out twice
/// without an intervening free, which cannot happen without a flush (every
/// remover flushes). Hole reuse — the reason the dedup exists at all — hands
/// back an address registered BEFORE the batch, so it is still covered.
#[cold]
fn flush_deferred_old_page_registrations_batch() {
    let pending =
        DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
    if pending.is_empty() {
        return;
    }
    let mut meta_updates: Vec<(usize, usize)> = Vec::with_capacity(pending.len());
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        // Entries arrive in allocation order, so consecutive ones share a page;
        // cache that page's pre-batch length across the run.
        let mut run_page: Option<usize> = None;
        let mut run_base_len: usize = 0;
        for &(header_addr, total_size) in &pending {
            let object_end = header_addr + total_size;
            let first_page = generation_page_for_addr(header_addr);
            let last_page = generation_page_for_addr(object_end - 1);
            for page in first_page..=last_page {
                let page_base = generation_page_base(page);
                let page_end = page_base + GENERATION_PAGE_SIZE;
                let overlap_start = header_addr.max(page_base);
                let overlap_end = object_end.min(page_end);
                if overlap_start >= overlap_end {
                    continue;
                }
                let headers = index.entry(page).or_insert_with(Vec::new);
                if run_page != Some(page) {
                    run_page = Some(page);
                    run_base_len = headers.len();
                }
                if !headers[..run_base_len.min(headers.len())].contains(&header_addr) {
                    headers.push(header_addr);
                    meta_updates.push((page, overlap_end - overlap_start));
                }
            }
        }
    });
    update_old_page_meta_for_object(&meta_updates, true);
}

/// Entries awaiting a flush. Tests only — the buffer is an implementation
/// detail everywhere else.
#[cfg(test)]
pub(crate) fn deferred_old_page_registrations_len() -> usize {
    DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().len())
}

#[allow(dead_code)]
pub(crate) fn unregister_old_object_pages(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    // #7625 REMOVER: see `unregister_old_block_pages`. Removing before the
    // flush would leave the flush to resurrect this object.
    flush_deferred_old_page_registrations();
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    let mut removed_pages = Vec::with_capacity(overlaps.len());
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for &(page, bytes) in &overlaps {
            let mut remove_page = false;
            if let Some(headers) = index.get_mut(&page) {
                if let Some(pos) = headers.iter().position(|&addr| addr == header_addr) {
                    headers.swap_remove(pos);
                    removed_pages.push((page, bytes));
                }
                remove_page = headers.is_empty();
            }
            if remove_page {
                index.remove(&page);
            }
        }
    });
    update_old_page_meta_for_object(&removed_pages, false);
}

pub(crate) fn old_pages_begin_gc_cycle() {
    // #7625 CYCLE START: all three cycle constructors route through here
    // (`gc/mod.rs`'s minor, `gc/cycle.rs`'s `new_full`, `gc/policy.rs`'s
    // budgeted), so every collection begins with a complete page-objects index
    // and an EMPTY deferral buffer.
    flush_deferred_old_page_registrations();
    // #6181: the per-page `dirty_slots` reset used to iterate every old page
    // here (O(old pages) on every minor, growing with old-gen size). It is now
    // a single epoch bump — a page whose `dirty_slots_epoch` predates the new
    // epoch reads as zero via `effective_dirty_slots`, and the scan re-stamps
    // it on first touch this cycle (`old_page_account_dirty_slot`).
    OLD_GEN_PAGE_DIRTY_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
    OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.set(0));
    OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.set(0));
}

pub(crate) fn old_pages_reset_sweep_accounting() {
    // #7625 READER (`OLD_GEN_PAGE_META`): closes the promote → sweep window
    // inside a full cycle. The per-object accounting that follows calls
    // `refresh_policy_bits`, which reads `allocated_bytes`; flushing here means
    // it never recomputes a page's bits from a count that is missing this
    // cycle's promotions.
    flush_deferred_old_page_registrations();
    OLD_GEN_PAGE_META.with(|meta| {
        for page_meta in meta.borrow_mut().values_mut() {
            page_meta.reset_cycle_sweep_accounting();
        }
    });
}

pub(crate) fn old_page_account_swept_object(
    header_addr: usize,
    total_size: usize,
    live: bool,
    pinned: bool,
) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for (page, bytes) in overlaps {
            let page_meta = meta
                .entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
            if live {
                page_meta.live_bytes = page_meta.live_bytes.saturating_add(bytes);
                page_meta.live_object_count = page_meta.live_object_count.saturating_add(1);
                if pinned {
                    page_meta.pinned_bytes = page_meta.pinned_bytes.saturating_add(bytes);
                    page_meta.pinned_object_count = page_meta.pinned_object_count.saturating_add(1);
                }
            } else {
                page_meta.dead_bytes = page_meta.dead_bytes.saturating_add(bytes);
                page_meta.dead_object_count = page_meta.dead_object_count.saturating_add(1);
            }
            page_meta.refresh_policy_bits();
        }
    });
}

pub(crate) fn old_page_account_promoted_object(
    header_addr: usize,
    total_size: usize,
    pinned: bool,
) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for (page, bytes) in overlaps {
            let page_meta = meta
                .entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
            page_meta.live_bytes = page_meta.live_bytes.saturating_add(bytes);
            page_meta.live_object_count = page_meta.live_object_count.saturating_add(1);
            if pinned {
                page_meta.pinned_bytes = page_meta.pinned_bytes.saturating_add(bytes);
                page_meta.pinned_object_count = page_meta.pinned_object_count.saturating_add(1);
            }
            page_meta.refresh_policy_bits();
        }
    });
}

pub(crate) fn old_page_account_dirty_slot(slot_addr: usize) {
    if slot_addr == 0 {
        return;
    }
    let page = generation_page_for_addr(slot_addr);
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            // First dirty slot seen this cycle re-stamps and starts from 1
            // (the lazy equivalent of the old per-cycle reset-to-zero);
            // subsequent slots on the same page accumulate (#6181).
            if page_meta.dirty_slots_epoch == current_epoch {
                page_meta.dirty_slots = page_meta.dirty_slots.saturating_add(1);
            } else {
                page_meta.dirty_slots_epoch = current_epoch;
                page_meta.dirty_slots = 1;
            }
        }
    });
}

pub(crate) fn old_page_summary() -> OldPageSummary {
    // #7625 READER (`OLD_GEN_PAGE_META`): a deferred registration also owes
    // this table an `allocated_bytes`/`object_count` update, so the summary
    // would under-report a mid-cycle promotion burst without the flush.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        let meta = meta.borrow();
        let mut summary = OldPageSummary {
            pages: meta.len(),
            ..OldPageSummary::default()
        };
        for page_meta in meta.values() {
            summary.allocated_bytes = summary
                .allocated_bytes
                .saturating_add(page_meta.allocated_bytes);
            summary.live_bytes = summary.live_bytes.saturating_add(page_meta.live_bytes);
            summary.dead_bytes = summary.dead_bytes.saturating_add(page_meta.dead_bytes);
            summary.pinned_bytes = summary.pinned_bytes.saturating_add(page_meta.pinned_bytes);
            summary.object_count = summary.object_count.saturating_add(page_meta.object_count);
            summary.live_object_count = summary
                .live_object_count
                .saturating_add(page_meta.live_object_count);
            summary.dead_object_count = summary
                .dead_object_count
                .saturating_add(page_meta.dead_object_count);
            summary.pinned_object_count = summary
                .pinned_object_count
                .saturating_add(page_meta.pinned_object_count);
            let dirty_slots = page_meta.effective_dirty_slots(current_epoch);
            if page_meta.dirty || dirty_slots > 0 {
                summary.dirty_pages = summary.dirty_pages.saturating_add(1);
            }
            summary.dirty_slots = summary.dirty_slots.saturating_add(dirty_slots);
            if page_meta.live_bytes > 0 && page_meta.dead_bytes > 0 {
                summary.fragmented_pages = summary.fragmented_pages.saturating_add(1);
            }
            if page_meta.evacuation_eligible {
                summary.evacuation_eligible_pages =
                    summary.evacuation_eligible_pages.saturating_add(1);
            }
        }
        summary.reusable_bytes = OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.get());
        summary.returned_bytes = OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.get());
        summary
    })
}

pub(crate) fn old_page_meta_snapshot() -> Vec<OldPageMeta> {
    // #7625 READER (`OLD_GEN_PAGE_META`): this one drives real policy —
    // `gc/oldgen_defrag.rs` selects evacuation pages from it.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        let mut snapshot = meta
            .borrow()
            .values()
            .copied()
            .map(|page_meta| normalize_dirty_slots_for_epoch(page_meta, current_epoch))
            .collect::<Vec<_>>();
        snapshot.sort_unstable_by_key(|page_meta| page_meta.page_base);
        snapshot
    })
}

/// Fold a stale `dirty_slots` stamp down to the effective value so a copied
/// `OldPageMeta` handed to a caller always reports this cycle's dirty-slot
/// count directly in `dirty_slots`, without the caller needing the epoch (#6181).
#[inline]
fn normalize_dirty_slots_for_epoch(mut page_meta: OldPageMeta, current_epoch: u64) -> OldPageMeta {
    page_meta.dirty_slots = page_meta.effective_dirty_slots(current_epoch);
    page_meta.dirty_slots_epoch = current_epoch;
    page_meta
}

pub(crate) fn old_arena_source_blocks_for_pages(
    selected_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> OldArenaSourceBlockSelection {
    let mut selection = OldArenaSourceBlockSelection::default();
    if selected_pages.is_empty() {
        return selection;
    }

    let old_block_start = longlived_end();
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for (i, block) in arena.blocks.iter().enumerate() {
            if block.data.is_null() || block.size == 0 {
                continue;
            }
            let base = block.data as usize;
            let first_page = generation_page_for_addr(base);
            let last_page = generation_page_for_addr(base + block.size - 1);
            if !(first_page..=last_page).any(|page| selected_pages.contains(&page)) {
                continue;
            }

            selection.block_indices.insert(old_block_start + i);
            for page in first_page..=last_page {
                selection.pages.insert(page);
            }
        }
    });
    selection
}

pub(crate) fn old_arena_walk_objects_on_pages(
    pages: &crate::fast_hash::PtrHashSet<usize>,
    mut callback: impl FnMut(*mut u8),
) -> usize {
    if pages.is_empty() {
        return 0;
    }

    // #7625 READER: promotions land in old-gen mid-cycle (a copying minor's
    // root scan runs before the remembered-set walk), so this cannot rely on
    // the cycle-start flush alone.
    flush_deferred_old_page_registrations();

    let mut headers = Vec::new();
    let mut seen = crate::fast_hash::new_ptr_hash_set();
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let index = index.borrow();
        for page in pages {
            if let Some(page_headers) = index.get(page) {
                for &header_addr in page_headers {
                    if seen.insert(header_addr) {
                        headers.push(header_addr);
                    }
                }
            }
        }
    });

    let count = headers.len();
    for header_addr in headers {
        callback(header_addr as *mut u8);
    }
    count
}

pub(crate) struct OldArenaPageObjectCursor {
    pages: Vec<usize>,
    page_cursor: usize,
    header_cursor: usize,
}

impl OldArenaPageObjectCursor {
    pub(crate) fn new(pages: &crate::fast_hash::PtrHashSet<usize>) -> Self {
        // #7625 READER: same obligation as `old_arena_walk_objects_on_pages`.
        // The cursor is stepped incrementally by the budgeted cycle, which
        // marks but never allocates into old-gen, so nothing can accumulate
        // between `new` and the last `next`; `next` debug-asserts that rather
        // than paying a thread-local check per object.
        flush_deferred_old_page_registrations();
        Self {
            pages: pages.iter().copied().collect(),
            page_cursor: 0,
            header_cursor: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<usize> {
        // #7625: `new` flushed; the stepping window must not re-dirty the
        // buffer, or this reader would be walking a stale index. Debug-only so
        // the per-object read costs nothing in a shipped collector.
        debug_assert!(
            DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().is_empty()),
            "an old-gen birth happened while a page-object cursor was stepping; \
             this reader is now walking a stale index (#7625)"
        );
        loop {
            let page = *self.pages.get(self.page_cursor)?;
            let header = OLD_GEN_PAGE_OBJECTS.with(|index| {
                index
                    .borrow()
                    .get(&page)
                    .and_then(|headers| headers.get(self.header_cursor).copied())
            });
            if let Some(header) = header {
                self.header_cursor += 1;
                return Some(header);
            }
            self.page_cursor += 1;
            self.header_cursor = 0;
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.page_cursor >= self.pages.len()
    }
}

pub(crate) fn old_arena_page_index_remove_object(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    // #7625 REMOVER: see `unregister_old_block_pages`.
    flush_deferred_old_page_registrations();
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for (page, _) in overlaps {
            let mut remove_page = false;
            if let Some(headers) = index.get_mut(&page) {
                headers.retain(|&addr| addr != header_addr);
                remove_page = headers.is_empty();
            }
            if remove_page {
                index.remove(&page);
            }
        }
    });
}

/// Stamp `page`'s metadata dirty. Returns whether a metadata entry existed to
/// stamp: #7187 Phase B's "already dirty" cache may only remember a page whose
/// recording is complete in BOTH the modbuf and here, so it has to know.
pub(crate) fn old_page_mark_dirty(page: usize) -> bool {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = true;
            true
        } else {
            false
        }
    })
}

pub(crate) fn old_page_clear_dirty(page: usize) {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = false;
        }
    });
    // #7187 Phase B: one of the two places a page's `dirty` stamp can go false,
    // so one of the places the barrier's cached page can stop being a complete
    // recording. Invalidating here rather than at the callers covers the GC's
    // own clear loop and the tests that reach for this directly.
    crate::gc::dirty_page_cache_invalidate();
}

#[cfg(test)]
pub(crate) fn old_arena_page_index_clear_for_tests() {
    // Wiping page metadata makes real old-arena objects unclassifiable —
    // stand the #6179 differential verifier down for this thread's test.
    crate::gc::CLASSIFIER_VERIFY_SUPPRESSED.with(|c| c.set(true));
    // #7625: DISCARD rather than flush — a caller asking for an empty index
    // would get a repopulated one if the pending burst were folded in first.
    DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow_mut().clear());
    OLD_GEN_PAGE_OBJECTS.with(|index| index.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn old_page_meta_for_tests(page: usize) -> Option<OldPageMeta> {
    // #7625 READER: same rule as `old_page_summary`/`old_page_meta_snapshot`,
    // so a test that allocates and then inspects a page sees what eager
    // registration would have left.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        meta.borrow()
            .get(&page)
            .copied()
            .map(|page_meta| normalize_dirty_slots_for_epoch(page_meta, current_epoch))
    })
}

#[cfg(test)]
mod page_generation_hasher_tests {
    use super::*;
    use std::collections::HashSet;
    use std::hash::BuildHasher;

    /// #7187 regression guard for `PageGenerationMap`'s hasher.
    ///
    /// `HashMap` is hashbrown: the bucket index comes from the hash's low bits,
    /// but the SIMD control byte — the filter that decides whether a group
    /// probe needs a real key comparison — is `hash >> 57`. Generation class
    /// keys are `addr >> GENERATION_CLASS_SHIFT`, so an identity hasher (which
    /// this map carried until #7187) produces a value around 2^26 whose top
    /// seven bits are zero for **every** key in the table. Every occupied slot
    /// in a probed group then matches, and each match costs a scattered load
    /// plus a key comparison — on a lookup the write barrier performs several
    /// times per heap store.
    ///
    /// This asserts the property directly rather than asserting "we call
    /// `PtrHasher`": reinstating any non-mixing hasher collapses the control
    /// byte to a single value and fails here.
    #[test]
    fn control_byte_is_spread_across_generation_class_keys() {
        let map = PageGenerationMap::default();
        let build = map.hasher();

        // Realistic 48-bit heap addresses, one per 1 MiB generation bucket —
        // the exact key population `classify_heap_generation` looks up.
        let base: usize = 0x0000_7f31_0000_0000;
        let control_bytes: HashSet<u64> = (0..64)
            .map(|i| {
                let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
                (build.hash_one(generation_class_key_for_addr(addr)) >> 57) & 0x7f
            })
            .collect();

        assert!(
            control_bytes.len() >= 32,
            "hashbrown control byte must vary across generation class keys, got {} \
             distinct values from 64 consecutive buckets (an identity hasher yields 1)",
            control_bytes.len()
        );
    }

    /// The bucket index (low bits) must stay well spread too — mixing that put
    /// all the entropy in the high bits and left the low bits constant would
    /// trade a control-byte collision for a far worse bucket collision. This is
    /// the failure `fast_hash`'s `mix` step exists for.
    #[test]
    fn bucket_index_is_spread_across_generation_class_keys() {
        let map = PageGenerationMap::default();
        let build = map.hasher();

        let base: usize = 0x0000_7f31_0000_0000;
        let low_bits: HashSet<u64> = (0..64)
            .map(|i| {
                let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
                build.hash_one(generation_class_key_for_addr(addr)) & 0x3f
            })
            .collect();

        assert!(
            low_bits.len() >= 32,
            "bucket index must vary across generation class keys, got {} distinct \
             values from 64 consecutive buckets",
            low_bits.len()
        );
    }

    /// The map must still answer correctly after the hasher change — a
    /// point-query round trip over many buckets, which is the only way this map
    /// is ever used.
    #[test]
    fn point_queries_round_trip_across_many_buckets() {
        let mut map = PageGenerationMap::default();
        let base: usize = 0x0000_7f31_0000_0000;
        for i in 0..256usize {
            let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
            map.insert(
                generation_class_key_for_addr(addr),
                PageGenerationSlot::Single(PageGenerationRange {
                    base: addr,
                    end: addr + (1 << GENERATION_CLASS_SHIFT),
                    generation: HeapGeneration::Old,
                    space: HeapSpace::Old,
                }),
            );
        }
        for i in 0..256usize {
            let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
            let found = map
                .get(&generation_class_key_for_addr(addr))
                .and_then(|slot| slot.find(addr + 0x40))
                .expect("every inserted bucket must be found by point query");
            assert_eq!(found.generation, HeapGeneration::Old);
            assert_eq!(found.base, addr);
        }
        assert_eq!(map.len(), 256);
    }
}
