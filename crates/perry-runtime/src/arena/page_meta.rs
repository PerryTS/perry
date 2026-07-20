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

#[derive(Default)]
struct IdentityHasher(u64);

impl Hasher for IdentityHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = 0u64;
        for (idx, byte) in bytes.iter().take(8).enumerate() {
            hash |= (*byte as u64) << (idx * 8);
        }
        self.0 = hash;
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.0 = value as u64;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

type PageGenerationMap = HashMap<usize, PageGenerationSlot, BuildHasherDefault<IdentityHasher>>;
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
        RefCell::new(HashMap::with_hasher(BuildHasherDefault::<IdentityHasher>::default()));

    static PAGE_GENERATION_CACHE: Cell<PageGenerationCache> =
        const { Cell::new(PageGenerationCache::empty()) };

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
    PAGE_GENERATION_CACHE.with(|cache| cache.set(PageGenerationCache::empty()));
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

#[inline]
pub(crate) fn classify_heap_generation(addr: usize) -> HeapGeneration {
    if addr == 0 {
        return HeapGeneration::Unknown;
    }
    let key = generation_class_key_for_addr(addr);
    if let Some(generation) = PAGE_GENERATION_CACHE.with(|cache| {
        let cached = cache.get();
        (cached.valid && cached.key == key && cached.range.contains(addr))
            .then_some(cached.range.generation)
    }) {
        return generation;
    }

    let found = PAGE_GENERATIONS.with(|pages| {
        let pages = pages.borrow();
        pages.get(&key).and_then(|slot| slot.find(addr))
    });
    if let Some(range) = found {
        PAGE_GENERATION_CACHE.with(|cache| {
            cache.set(PageGenerationCache {
                key,
                range,
                valid: true,
            });
        });
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
    if let Some(space) = PAGE_GENERATION_CACHE.with(|cache| {
        let cached = cache.get();
        (cached.valid && cached.key == key && cached.range.contains(addr))
            .then_some(cached.range.space)
    }) {
        return space;
    }

    let found = PAGE_GENERATIONS.with(|pages| {
        let pages = pages.borrow();
        pages.get(&key).and_then(|slot| slot.find(addr))
    });
    if let Some(range) = found {
        PAGE_GENERATION_CACHE.with(|cache| {
            cache.set(PageGenerationCache {
                key,
                range,
                valid: true,
            });
        });
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

#[allow(dead_code)]
pub(crate) fn unregister_old_object_pages(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
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
        Self {
            pages: pages.iter().copied().collect(),
            page_cursor: 0,
            header_cursor: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<usize> {
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

pub(crate) fn old_page_mark_dirty(page: usize) {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = true;
        }
    });
}

pub(crate) fn old_page_clear_dirty(page: usize) {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = false;
        }
    });
}

#[cfg(test)]
pub(crate) fn old_arena_page_index_clear_for_tests() {
    // Wiping page metadata makes real old-arena objects unclassifiable —
    // stand the #6179 differential verifier down for this thread's test.
    crate::gc::CLASSIFIER_VERIFY_SUPPRESSED.with(|c| c.set(true));
    OLD_GEN_PAGE_OBJECTS.with(|index| index.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn old_page_meta_for_tests(page: usize) -> Option<OldPageMeta> {
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        meta.borrow()
            .get(&page)
            .copied()
            .map(|page_meta| normalize_dirty_slots_for_epoch(page_meta, current_epoch))
    })
}
