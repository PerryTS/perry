//! Fast bump allocator for short-lived objects
//!
//! Uses thread-local bump allocation for fast object creation.
//! Objects allocated here are not individually freed - the entire arena
//! can be reset at once (e.g., at end of program or during GC).

use std::alloc::{alloc, Layout};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::{hash_map::Entry, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

/// Size of each arena block (1 MB — issue #179 tier 1 #1).
///
/// Formerly 8 MB. The recent-5-blocks safety window (where LLVM caller-
/// saved registers might still hold uncaptured handles; see
/// `BLOCK_PERSIST_WINDOW` in gc.rs and `keep_low` in
/// `arena_reset_empty_blocks`) now reserves 5 × 1 MB = 5 MB of
/// non-reclaimable headroom instead of 5 × 8 MB = 40 MB. Combined with
/// the age-restricted block-persist from v0.5.193 this closes the
/// remaining `bench_json_roundtrip` RSS gap to within 5% of Node's
/// numbers without a speed regression.
///
/// Measured on `bench_json_roundtrip` (best-of-5, macOS ARM64):
///   8 MB blocks (v0.5.193): 384 ms / 213 MB
///   2 MB blocks:            325 ms / 208 MB
///   1 MB blocks:            320 ms / 199 MB
///   512 KB blocks:          318 ms / 200 MB  (diminishing returns)
///
/// Picked 1 MB: RSS essentially tied with 512 KB, block-count overhead
/// 2× smaller, `bench_gc_pressure` / `object_create` unchanged.
///
/// Trade-offs:
/// - More blocks in the arena for the same total bytes → walker loops
///   pay more per-block overhead. Measured: negligible — the walker is
///   O(objects), not O(blocks), once inside a block.
/// - More frequent "block full, advance to next" transitions in the
///   inline bump allocator's slow path. The slow path is a function
///   call; on `object_create` the cost is amortized across hundreds of
///   thousands of allocs per block before GC resets it. Measured:
///   `07_object_create` 0-1 ms unchanged.
/// - Large single allocations (Buffer.alloc(3 MB), big arena strings)
///   get a custom-sized block via `alloc_block(min_size)` that rounds
///   up to a BLOCK_SIZE multiple — unchanged mechanics, just rounds to
///   1 MB granularity now.
/// - The GC's adaptive step (gc.rs `GC_THRESHOLD_INITIAL_BYTES = 128
///   MB`) is unchanged; the workload still needs 128 MB of total arena
///   to trigger the first GC. With 1 MB blocks that's 128 blocks, and
///   `bench_json_roundtrip` hits that point at roughly the same
///   iteration as it did with 16 × 8 MB blocks — the adaptive step
///   shrinks appropriately on the first productive collection.
const BLOCK_SIZE: usize = 1024 * 1024;
const GENERATION_PAGE_SHIFT: usize = 12;
// Generation classification wants exact range answers, but it does
// not need a separate hash entry for every 4 KiB remembered-set card.
// A 1 MiB bucket matches the arena block scale, keeps lookup bounded,
// and avoids thousands of metadata entries for low-pressure nursery
// churn before the first GC.
const GENERATION_CLASS_SHIFT: usize = 20;

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

thread_local! {
    static PAGE_GENERATIONS: RefCell<PageGenerationMap> =
        RefCell::new(HashMap::with_hasher(BuildHasherDefault::<IdentityHasher>::default()));

    static PAGE_GENERATION_CACHE: Cell<PageGenerationCache> =
        const { Cell::new(PageGenerationCache::empty()) };

    static OLD_GEN_PAGE_OBJECTS: RefCell<OldGenPageObjectMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
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
fn invalidate_generation_cache() {
    PAGE_GENERATION_CACHE.with(|cache| cache.set(PageGenerationCache::empty()));
}

fn register_block_space(base: usize, size: usize, generation: HeapGeneration, space: HeapSpace) {
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
    invalidate_generation_cache();
}

fn unregister_block_generation(base: usize, size: usize) {
    if base == 0 || size == 0 {
        return;
    }
    let end = base + size;
    let first_key = generation_class_key_for_addr(base);
    let last_key = generation_class_key_for_addr(end - 1);
    PAGE_GENERATIONS.with(|pages| {
        let mut pages = pages.borrow_mut();
        for key in first_key..=last_key {
            let mut remove_page = false;
            let mut replacement = None;
            if let Some(slot) = pages.get_mut(&key) {
                match slot {
                    PageGenerationSlot::Single(range) => {
                        remove_page = range.base == base && range.end == end;
                    }
                    PageGenerationSlot::Multiple(ranges) => {
                        ranges.retain(|range| !(range.base == base && range.end == end));
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

fn register_old_object_pages(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let first_page = generation_page_for_addr(header_addr);
    let last_page = generation_page_for_addr(header_addr + total_size - 1);
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for page in first_page..=last_page {
            let headers = index.entry(page).or_insert_with(Vec::new);
            if !headers.contains(&header_addr) {
                headers.push(header_addr);
            }
        }
    });
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

#[cfg(test)]
pub(crate) fn old_arena_page_index_clear_for_tests() {
    OLD_GEN_PAGE_OBJECTS.with(|index| index.borrow_mut().clear());
}

/// Create a block of at least the given size (for oversized allocations)
fn alloc_block(min_size: usize) -> ArenaBlock {
    let size = if min_size <= BLOCK_SIZE {
        BLOCK_SIZE
    } else {
        // Round up to next multiple of BLOCK_SIZE
        min_size.div_ceil(BLOCK_SIZE) * BLOCK_SIZE
    };
    let layout = Layout::from_size_align(size, 16).unwrap();
    let data = unsafe { alloc(layout) };
    if data.is_null() {
        panic!("Failed to allocate arena block of {} bytes", size);
    }
    ArenaBlock {
        data,
        size,
        offset: 0,
        dead_cycles: 0,
    }
}

/// A single arena block
struct ArenaBlock {
    data: *mut u8,
    size: usize,
    offset: usize,
    /// Issue #73: number of consecutive GC cycles this block has been
    /// observed with zero live objects. Reset requires TWO consecutive
    /// dead observations so a block can't be reclaimed on the same
    /// cycle its last live pointer slipped off the conservative scan
    /// (e.g. LLVM dropped a `samples` handle from a caller-saved FP
    /// reg after the IndexSet store). On the next cycle either the
    /// scan finds the pointer (counter resets to 0) or the block is
    /// truly dead and resets.
    dead_cycles: u32,
}

impl ArenaBlock {
    fn new() -> Self {
        alloc_block(BLOCK_SIZE)
    }

    /// Try to allocate within this block, respecting alignment
    #[inline]
    fn alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        // Always preserve at least 8-byte alignment between calls so
        // the codegen inline bump-allocator fast path
        // (`crates/perry-codegen/src/lower_call.rs`'s inline-keys
        // allocator) can safely advance `offset += total_size`
        // without re-aligning. Pre-fix, an odd-sized string
        // allocation (`StringHeader=20` + N-byte payload via
        // `arena_alloc_gc`) left `offset` misaligned; the next
        // inline `new ClassName()` inherited the misalignment, the
        // returned user_ptr had low bits set, `arena_walk_objects`
        // (which iterates at 8-aligned positions) skipped it,
        // `build_valid_pointer_set` never inserted it, and the GC
        // mark phase rejected it as "not in valid_ptrs". The
        // archetype's `componentData` Map went unmarked and got
        // swept; the freed entries buffer was reused for a new alloc;
        // the first f64 key drifted to a denormal (~1.086e-311).
        let pad = align.max(8);
        let aligned_offset = (self.offset + pad - 1) & !(pad - 1);
        if aligned_offset + size > self.size {
            return None;
        }

        let ptr = unsafe { self.data.add(aligned_offset) };
        let bumped = aligned_offset + size;
        self.offset = (bumped + pad - 1) & !(pad - 1);
        Some(ptr)
    }
}

/// Thread-local arena allocator
///
/// When a thread exits (e.g., worker threads from `perry/thread`), the Drop
/// impl frees all arena blocks so memory isn't leaked.
struct Arena {
    blocks: Vec<ArenaBlock>,
    current: usize,
    generation: HeapGeneration,
    space: HeapSpace,
}

impl Drop for Arena {
    fn drop(&mut self) {
        for block in &self.blocks {
            // Skip tombstoned slots (gen-GC Phase C4b-δ): C4b-δ
            // deallocates fully-idle nursery blocks back to the OS
            // and leaves a `data = null, size = 0` tombstone in the
            // Vec to keep block-index semantics stable across GC
            // cycles. `dealloc(null, …)` is UB.
            if block.data.is_null() {
                continue;
            }
            let layout = std::alloc::Layout::from_size_align(block.size, 16).unwrap();
            unsafe {
                std::alloc::dealloc(block.data, layout);
            }
        }
    }
}

impl Arena {
    fn new(generation: HeapGeneration, space: HeapSpace) -> Self {
        let initial = ArenaBlock::new();
        register_block_space(initial.data as usize, initial.size, generation, space);
        ARENA_TOTAL_BYTES.with(|t| t.set(t.get() + initial.size));
        Arena {
            blocks: vec![initial],
            current: 0,
            generation,
            space,
        }
    }

    #[inline]
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // Try current block first
        if let Some(ptr) = self.blocks[self.current].alloc(size, align) {
            return ptr;
        }

        // Current block is full. Check GC trigger first — if it fires
        // and reclaims at least one fully-empty block (via
        // `arena_reset_empty_blocks`), we may be able to reuse that
        // block instead of pushing a new one.
        crate::gc::gc_check_trigger();

        // Retry the (possibly newly-reset) current block. arena.current
        // may have been changed by arena_reset_empty_blocks to point
        // at the lowest reset block.
        if let Some(ptr) = self.blocks[self.current].alloc(size, align) {
            return ptr;
        }

        // Scan forward for any other block with space — the GC may
        // have reset blocks we haven't tried yet. Without this scan,
        // we'd push a fresh block on the very first overflow even
        // though blocks `current+1..n_blocks` are all empty.
        for i in 0..self.blocks.len() {
            if i == self.current {
                continue;
            }
            if let Some(ptr) = self.blocks[i].alloc(size, align) {
                self.current = i;
                // Resync inline state to the new current block.
                INLINE_STATE.with(|s| unsafe {
                    let inline = &mut *s.get();
                    if !inline.data.is_null() {
                        let block = &self.blocks[self.current];
                        inline.data = block.data;
                        inline.offset = block.offset;
                        inline.size = block.size;
                    }
                });
                return ptr;
            }
        }

        // Still no room anywhere — need a fresh block. C4b-δ:
        // prefer reusing a tombstoned slot (a block deallocated by
        // `arena_reset_empty_blocks` after staying idle past the
        // dealloc threshold) over growing the Vec, so block_idx
        // semantics stay bounded even on workloads that churn
        // through nursery blocks.
        let fresh = alloc_block(size);
        let fresh_size = fresh.size;
        let fresh_base = fresh.data as usize;
        register_block_space(fresh_base, fresh_size, self.generation, self.space);
        let mut tomb_idx: Option<usize> = None;
        for i in 0..self.blocks.len() {
            if self.blocks[i].data.is_null() {
                tomb_idx = Some(i);
                break;
            }
        }
        let new_idx = match tomb_idx {
            Some(i) => {
                self.blocks[i] = fresh;
                i
            }
            None => {
                self.blocks.push(fresh);
                self.blocks.len() - 1
            }
        };
        self.current = new_idx;
        ARENA_TOTAL_BYTES.with(|t| t.set(t.get() + fresh_size));

        self.blocks[self.current]
            .alloc(size, align)
            .expect("Fresh block should have space")
    }
}

thread_local! {
    /// Cached running sum of `block.size` across every arena (general,
    /// longlived, old-gen). `arena_total_bytes()` previously walked
    /// every block of every arena summing this on every call — and
    /// `gc_check_trigger()` calls it on every `gc_malloc`, so for an
    /// 80-block working set the per-allocation overhead was ~250 ns
    /// just to recompute a total that almost never changes (only on
    /// fresh-block alloc and tombstone dealloc). Maintained via deltas
    /// at the four mutation sites (Arena::new initial block, fresh
    /// alloc into a tombstone slot or the end, and dealloc inside
    /// `arena_reset_empty_blocks`).
    static ARENA_TOTAL_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    static ARENA: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new(HeapGeneration::Nursery, HeapSpace::NurseryEden));

    /// Segregated long-lived arena (issue #179). Holds objects that are
    /// intentionally pinned for the lifetime of the program by explicit
    /// root scanners — `PARSE_KEY_CACHE` interned strings, class/object
    /// shape-cache `keys_array`s + their string element pointers. Keeping
    /// these out of the general arena prevents block-persistence
    /// cascades: without segregation, those long-lived allocations
    /// co-locate with the first few iterations' fresh parse output in
    /// general-arena block 0, block-persist marks every adjacent dead
    /// iter-0 object live, those dead objects' field values anchor
    /// fresh-block objects, and the "live set" snowballs.
    ///
    /// Longlived blocks are never reset by `arena_reset_empty_blocks` /
    /// `arena_reset_all_blocks_to_zero`, and are never fed into the
    /// inline bump allocator (no `INLINE_STATE` entanglement). Walkers
    /// still traverse them so mark/trace reach cached objects; root
    /// scanners (`scan_parse_roots`, `scan_shape_cache_roots`,
    /// `scan_transition_cache_roots`) keep them marked.
    static LONGLIVED_ARENA: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new(HeapGeneration::Longlived, HeapSpace::Longlived));

    /// Copying nursery survivor semispaces. At most one is the active
    /// from-space at the start of a copying minor GC; the other is reset
    /// and used as to-space for fresh Eden survivors.
    static SURVIVOR_ARENA_0: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new(HeapGeneration::Nursery, HeapSpace::Survivor0));
    static SURVIVOR_ARENA_1: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new(HeapGeneration::Nursery, HeapSpace::Survivor1));
    static ACTIVE_SURVIVOR: Cell<usize> = const { Cell::new(0) };

    /// Generational-GC old-generation arena (gen-GC Phase B per
    /// `docs/generational-gc-plan.md`). Holds objects PROMOTED from
    /// the nursery (= the existing `ARENA`, treated as nursery in
    /// the gen-GC model). Empty in Phase B — Phase C's minor GC
    /// will populate it via the evacuation path. Same `Arena`
    /// shape as the others; same walker / tracer integration so
    /// every existing pass already covers it once `arena_walk_*`
    /// extends to a third region.
    ///
    /// Old-arena blocks are never reset by `arena_reset_empty_blocks`
    /// (same lifetime contract as longlived blocks — promotion
    /// implies "expected to live indefinitely"), and never feed
    /// the inline bump allocator. Major GC will eventually mark-
    /// sweep them in Phase C+; today they accumulate forever
    /// because nothing allocates into them.
    static OLD_ARENA: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new(HeapGeneration::Old, HeapSpace::Old));

    /// Inline allocator state — a cache of the current arena block's
    /// `(data, offset, size)` triple, exposed via a stable pointer so
    /// codegen can emit inline bump-allocate IR without going through
    /// any function call or `LocalKey::with` wrapper.
    ///
    /// `#[repr(C)]` on `InlineArenaState` keeps the field offsets stable
    /// (data=0, offset=8, size=16). The codegen reads/writes these fields
    /// directly via fixed GEPs, so changing the struct layout would
    /// silently break every emitted `new ClassName()`.
    static INLINE_STATE: UnsafeCell<InlineArenaState> = const { UnsafeCell::new(InlineArenaState {
        data: std::ptr::null_mut(),
        offset: 0,
        size: 0,
    }) };
}

/// Inline bump-allocator state. The codegen emits inline LLVM IR that
/// reads `data` and `offset`, computes `aligned + size`, checks against
/// `size`, stores the new offset, and returns `data + aligned`. The
/// underlying thread-local Arena is the source of truth between
/// inline-alloc bursts; this state is the source of truth during them.
///
/// Field offsets are load-bearing — the codegen GEPs into this struct
/// at hard-coded byte offsets (0/8/16). Do not reorder.
#[repr(C)]
pub struct InlineArenaState {
    pub data: *mut u8, // offset  0  — current block's data pointer
    pub offset: usize, // offset  8  — bump pointer (mutated inline)
    pub size: usize,   // offset 16  — current block's size
}

/// Get the per-thread inline arena state pointer. Called once per JS
/// function entry; the codegen caches the result in a stack slot and
/// reuses it for every `new ClassName()` in that function. The address
/// is stable for the lifetime of the thread, so caching is safe.
///
/// First call on each thread lazy-syncs from the underlying ARENA.
#[no_mangle]
pub extern "C" fn js_inline_arena_state() -> *mut InlineArenaState {
    INLINE_STATE.with(|s| {
        let state = unsafe { &mut *s.get() };
        if state.data.is_null() {
            // Lazy init: copy from underlying ARENA's current block.
            ARENA.with(|a| unsafe {
                let arena = &*a.get();
                let block = &arena.blocks[arena.current];
                state.data = block.data;
                state.offset = block.offset;
                state.size = block.size;
            });
        }
        state as *mut InlineArenaState
    })
}

/// Slow path for inline bump alloc. Called from emitted IR when the
/// fast-path bump check fails (would overflow the current block).
///
/// Sequence:
///   1. Sync inline state's offset back to the underlying ARENA block
///      (so the alloc that's about to push a new block sees the right
///      "current" offset, and so any concurrent GC walk sees all live
///      objects from the inline-alloc burst).
///   2. Allocate via the existing `Arena::alloc` path — handles new
///      block + GC trigger via `alloc_slow`.
///   3. Resync inline state to point at whichever block the alloc
///      landed in (may be the same block if there was leftover space,
///      or a fresh block from `alloc_slow`).
///
/// Returns the raw pointer (the codegen writes the GcHeader at this
/// address and the ObjectHeader at +8 — same layout the inline path
/// produces).
#[no_mangle]
pub extern "C" fn js_inline_arena_slow_alloc(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
) -> *mut u8 {
    let state_ref = unsafe { &mut *state };
    ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        // Sync inline-state offset back to underlying block (so
        // arena_walk_objects and the slow-path GC trigger see the
        // post-burst offset).
        arena.blocks[arena.current].offset = state_ref.offset;
        // Allocate via existing path (may push a new block + run GC).
        let ptr = arena.alloc(size, align);
        // Resync inline state to the (possibly new) current block.
        let block = &arena.blocks[arena.current];
        state_ref.data = block.data;
        state_ref.offset = block.offset;
        state_ref.size = block.size;
        ptr
    })
}

/// Sync the inline arena state's offset back to the underlying arena
/// block. Call before any code path that walks the arena (GC scan,
/// `arena_walk_objects`, allocation accounting) so the block's offset
/// reflects the inline-burst's true high-water mark.
///
/// Cheap when no inline allocs have happened yet (state.data is null);
/// otherwise it's a thread-local read + a single store.
pub fn sync_inline_arena_state() {
    INLINE_STATE.with(|s| unsafe {
        let state = &*s.get();
        if !state.data.is_null() {
            ARENA.with(|a| {
                let arena = &mut *(*a).get();
                arena.blocks[arena.current].offset = state.offset;
            });
        }
    });
}

/// Allocate memory from the thread-local arena
/// This is very fast - just a pointer bump in the common case
///
/// Coexists with the inline allocator: every call here syncs the
/// inline state's offset back to the underlying block first (so we
/// don't overwrite inline-allocated memory), then allocates, then
/// resyncs the inline state to the post-alloc state of the block.
/// The two extra TLS reads cost ~5-10ns per call, which is fine
/// because non-inline allocations (`js_string_from_bytes`,
/// `js_closure_alloc`, etc.) are infrequent compared to the
/// per-class-instance hot path that uses the inline allocator.
#[inline]
pub fn arena_alloc(size: usize, align: usize) -> *mut u8 {
    INLINE_STATE.with(|inline_s| unsafe {
        let inline = &mut *inline_s.get();
        ARENA.with(|a| {
            let arena = &mut *(*a).get();
            // Sync inline → block before allocating, if the inline
            // state has been initialized.
            if !inline.data.is_null() {
                arena.blocks[arena.current].offset = inline.offset;
            }
            let ptr = arena.alloc(size, align);
            // Resync block → inline (may have advanced to a new block).
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
            ptr
        })
    })
}

/// Allocate from the longlived arena (issue #179). Unlike `arena_alloc`,
/// this never touches the inline allocator state — the longlived arena
/// is reserved for explicit-call allocations from cache builders
/// (`js_string_from_bytes_longlived`, `js_array_alloc_with_length_longlived`),
/// not hot-path `new ClassName()` bump allocations.
pub fn arena_alloc_longlived(size: usize, align: usize) -> *mut u8 {
    LONGLIVED_ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        arena.alloc(size, align)
    })
}

/// Allocate a GcHeader-prefixed object from the longlived arena (issue #179).
/// Same header layout as `arena_alloc_gc` so every walker, tracer, and
/// NaN-boxed-pointer resolver works unchanged — these objects are simply
/// not subject to block reset, so their backing storage is stable for the
/// lifetime of the thread.
///
/// No free-list reuse: longlived objects are never swept individually
/// (the cache's root scanner keeps them marked), so there's nothing to
/// re-add to the free list.
pub fn arena_alloc_gc_longlived(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    // Same alignment-preservation rationale as `arena_alloc_gc`: pad
    // `total` to a multiple of `max(align, 8)` so the next caller's
    // bumped offset stays aligned. The codegen inline fast path
    // assumes this invariant.
    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let raw = arena_alloc_longlived(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA;
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }
    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate from the old-generation arena (gen-GC Phase B per
/// `docs/generational-gc-plan.md`). Reserved for objects PROMOTED
/// from the nursery (= the general `ARENA`) by Phase C's minor GC.
/// No caller in Phase B — the promotion path lands in Phase C.
/// Same layout as `arena_alloc_gc` so every walker/tracer/sweep
/// already covers it via the `arena_walk_*` family extensions
/// below.
///
/// Routes through a non-inline allocator path (no `INLINE_STATE`
/// touch) so codegen's hot bump-pointer loop on `new ClassName()`
/// stays exclusively pinned to the nursery.
pub fn arena_alloc_old(size: usize, align: usize) -> *mut u8 {
    OLD_ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        arena.alloc(size, align)
    })
}

/// GcHeader-prefixed counterpart of `arena_alloc_old`. See
/// `arena_alloc_gc_longlived` for the same shape on the longlived
/// arena — only the backing region differs.
pub fn arena_alloc_gc_old(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    // Same alignment-preservation rationale as `arena_alloc_gc`.
    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let raw = arena_alloc_old(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA;
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }
    register_old_object_pages(raw as usize, total);

    unsafe { raw.add(GC_HEADER_SIZE) }
}

fn inactive_survivor_index() -> usize {
    ACTIVE_SURVIVOR.with(|active| 1 - active.get())
}

fn with_survivor_arena_mut<R>(idx: usize, f: impl FnOnce(&mut Arena) -> R) -> R {
    match idx {
        0 => SURVIVOR_ARENA_0.with(|a| unsafe { f(&mut *a.get()) }),
        1 => SURVIVOR_ARENA_1.with(|a| unsafe { f(&mut *a.get()) }),
        _ => unreachable!("invalid survivor arena index"),
    }
}

fn with_survivor_arena<R>(idx: usize, f: impl FnOnce(&Arena) -> R) -> R {
    match idx {
        0 => SURVIVOR_ARENA_0.with(|a| unsafe { f(&*a.get()) }),
        1 => SURVIVOR_ARENA_1.with(|a| unsafe { f(&*a.get()) }),
        _ => unreachable!("invalid survivor arena index"),
    }
}

/// Allocate into the inactive survivor semispace. The copying minor GC
/// resets this space before use and flips it active after from-space reset.
pub(crate) fn arena_alloc_gc_survivor(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let idx = inactive_survivor_index();
    let raw = with_survivor_arena_mut(idx, |arena| arena.alloc(total, align));

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA;
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate from arena with a GcHeader prepended.
/// Returns pointer to usable memory AFTER the GcHeader.
/// The object is NOT added to any tracking list — arena objects are discovered
/// by walking arena blocks linearly.
///
/// `#[inline(always)]` so the bitcode-link path can fully inline
/// this into user IR — the bump-pointer pattern is small enough
/// (~10 instructions on the fast path) that inlining is a clear win
/// and the slow path (free-list walk + new arena block) is gated
/// behind a cold branch.
#[inline(always)]
pub fn arena_alloc_gc(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    // Hot path: bump-allocate from the current arena block, skipping the
    // free-list walk entirely. The free-list-nonempty `Cell` is a single
    // unboxed load (no `RefCell::borrow_mut` cost) and is `false` for the
    // first GC cycle of every benchmark — which is when allocation-heavy
    // micro-benchmarks like object_create / binary_trees run their tight
    // loops. Walking an empty Vec was costing ~10ns per alloc (borrow,
    // iterate, drop) for nothing; this `Cell` check is ~1ns.
    let reused = if crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.get()) {
        crate::gc::ARENA_FREE_LIST.with(|fl| {
            let mut fl = fl.borrow_mut();
            // Find a slot that fits (exact or slightly larger)
            let mut best_idx = None;
            let mut best_waste = usize::MAX;
            for (idx, &(_, slot_size)) in fl.iter().enumerate() {
                if slot_size >= size && slot_size - size < best_waste {
                    best_waste = slot_size - size;
                    best_idx = Some(idx);
                    if best_waste == 0 {
                        break; // Perfect fit
                    }
                }
            }
            if let Some(idx) = best_idx {
                let (ptr, _slot_size) = fl.swap_remove(idx);
                if fl.is_empty() {
                    crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
                }
                Some(ptr)
            } else {
                None
            }
        })
    } else {
        None
    };

    if let Some(user_ptr) = reused {
        // Reusing a free-list slot: the GcHeader is already in place (before user_ptr)
        // Just update it
        unsafe {
            let header = user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = GC_FLAG_ARENA;
            (*header)._reserved = 0;
            // size field already set from original allocation
        }
        return user_ptr;
    }

    // Pad `total` up to a multiple of 8 so the arena's offset stays
    // 8-aligned after each GC alloc. The codegen inline bump-allocator
    // fast path in `crates/perry-codegen/src/lower_call.rs` reads the
    // current offset, adds `total_size`, and stores back without
    // re-aligning — its "every allocation is a multiple of 8"
    // invariant is only valid if every `arena_alloc_gc` caller
    // honors it. Strings (`StringHeader=20` bytes + N-byte payload)
    // routinely allocate odd sizes, which left the offset misaligned
    // for the next inline class allocation. Symptoms: `new World()`
    // returned a misaligned user_ptr; `arena_walk_objects` (which
    // walks at 8-aligned positions) skipped the World object;
    // `build_valid_pointer_set` therefore never inserted World;
    // `try_mark_value` rejected the World pointer found in the
    // shadow stack; mark phase missed every reachable Map / Array
    // hanging off World; sweep freed the archetype's componentData
    // entries buffer; the next allocation reused that slab and the
    // first componentData key drifted to a denormal (~1.086e-311),
    // throwing "Component type 1 is not in this archetype" on the
    // next query.
    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let raw = arena_alloc(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA;
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate an object of known size from the arena
/// Returns a properly aligned pointer
#[no_mangle]
pub extern "C" fn js_arena_alloc(size: u32) -> *mut u8 {
    arena_alloc(size as usize, 8)
}

/// Get total bytes reserved across all arena blocks (general + longlived
/// + old-gen). Reads the running sum maintained via deltas at the four
/// mutation sites — one TLS load instead of an O(blocks) walk on the
/// gc-trigger hot path.
#[inline]
pub fn arena_total_bytes() -> usize {
    ARENA_TOTAL_BYTES.with(|t| t.get())
}

/// Get bytes currently in use (sum of `block.offset` across blocks).
/// Used by adaptive GC to measure how much actual data the program is
/// holding live, separately from how much arena space we've reserved.
/// After a GC sweep that resets empty blocks, in-use bytes drop
/// dramatically while reserved bytes stay constant.
pub fn arena_in_use_bytes() -> usize {
    sync_inline_arena_state();
    let mut used: usize = 0;
    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset;
        }
    });
    LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset;
        }
    });
    SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset;
        }
    });
    SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset;
        }
    });
    // Phase B: include old-gen in-use bytes.
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset;
        }
    });
    used
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ArenaRegionTelemetry {
    pub(crate) in_use_bytes: usize,
    pub(crate) reserved_bytes: usize,
    pub(crate) block_count: usize,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ArenaTelemetrySnapshot {
    pub(crate) arena: ArenaRegionTelemetry,
    pub(crate) survivor0: ArenaRegionTelemetry,
    pub(crate) survivor1: ArenaRegionTelemetry,
    pub(crate) longlived: ArenaRegionTelemetry,
    pub(crate) old: ArenaRegionTelemetry,
    pub(crate) total_in_use_bytes: usize,
    pub(crate) total_reserved_bytes: usize,
    pub(crate) total_block_count: usize,
}

#[derive(Clone, Copy, Default)]
pub struct ArenaResetStats {
    pub reset_blocks: usize,
    pub deallocated_blocks: usize,
    pub deallocated_bytes: usize,
}

fn arena_region_telemetry(arena: &Arena) -> ArenaRegionTelemetry {
    ArenaRegionTelemetry {
        in_use_bytes: arena.blocks.iter().map(|b| b.offset).sum(),
        reserved_bytes: arena.blocks.iter().map(|b| b.size).sum(),
        block_count: arena.blocks.iter().filter(|b| !b.data.is_null()).count(),
    }
}

pub(crate) fn arena_telemetry_snapshot() -> ArenaTelemetrySnapshot {
    sync_inline_arena_state();
    let arena = ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena_region_telemetry(arena)
    });
    let longlived = LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena_region_telemetry(arena)
    });
    let survivor0 = SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena_region_telemetry(arena)
    });
    let survivor1 = SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena_region_telemetry(arena)
    });
    let old = OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena_region_telemetry(arena)
    });
    ArenaTelemetrySnapshot {
        arena,
        survivor0,
        survivor1,
        longlived,
        old,
        total_in_use_bytes: arena.in_use_bytes
            + survivor0.in_use_bytes
            + survivor1.in_use_bytes
            + longlived.in_use_bytes
            + old.in_use_bytes,
        total_reserved_bytes: arena.reserved_bytes
            + survivor0.reserved_bytes
            + survivor1.reserved_bytes
            + longlived.reserved_bytes
            + old.reserved_bytes,
        total_block_count: arena.block_count
            + survivor0.block_count
            + survivor1.block_count
            + longlived.block_count
            + old.block_count,
    }
}

/// Walk all GcHeader objects in arena blocks linearly (general arena +
/// longlived arena, in that order — block indices are global with
/// general blocks occupying `0..general_block_count()`).
/// Walk all GC objects across all 3 arenas in ascending data-pointer
/// (block-address) order. Used by `gc::build_valid_pointer_set` so the
/// resulting pointer stream is a single sorted run instead of K
/// (≈ block-count) interspersed sorted runs — the final `sort()` then
/// only has to in-place insertion-sort the small unsorted malloc tail
/// and run one O(N) merge, dropping driftsort's K-way-merge phase
/// (≈ 19 % of total runtime in ECS perf-comprehensive).
///
/// Sorting the (small) block list is microseconds (~200 entries on the
/// ECS bench); the saved sort work over 1.6 M user pointers is on the
/// order of N · log K cache-line-sized comparisons.
pub fn arena_walk_objects_addr_sorted(mut callback: impl FnMut(*mut u8)) {
    use crate::gc::GcHeader;

    sync_inline_arena_state();

    // Collect (data, block.offset, block.size) for every non-tombstone
    // block across all 3 arenas. Using usize for `data` so we can sort
    // by address; cast back to *mut u8 when walking.
    let mut blocks: Vec<(usize, usize, usize)> = Vec::new();
    let collect = |arena: &Arena, blocks: &mut Vec<(usize, usize, usize)>| {
        for b in &arena.blocks {
            if !b.data.is_null() {
                blocks.push((b.data as usize, b.offset, b.size));
            }
        }
    };
    ARENA.with(|a| unsafe { collect(&*a.get(), &mut blocks) });
    SURVIVOR_ARENA_0.with(|a| unsafe { collect(&*a.get(), &mut blocks) });
    SURVIVOR_ARENA_1.with(|a| unsafe { collect(&*a.get(), &mut blocks) });
    LONGLIVED_ARENA.with(|a| unsafe { collect(&*a.get(), &mut blocks) });
    OLD_ARENA.with(|a| unsafe { collect(&*a.get(), &mut blocks) });
    blocks.sort_unstable_by_key(|&(d, _, _)| d);

    for (data, block_offset, block_size) in blocks {
        let mut offset = 0usize;
        while offset < block_offset {
            let aligned = (offset + 7) & !7;
            if aligned >= block_offset {
                break;
            }
            let header_ptr = (data + aligned) as *mut u8;
            let header = header_ptr as *const GcHeader;
            unsafe {
                let total_size = (*header).size as usize;
                if total_size == 0 || total_size > block_size {
                    break;
                }
                let obj_type = (*header).obj_type;
                if (1..=9).contains(&obj_type) {
                    callback(header_ptr);
                }
                offset = aligned + total_size;
            }
        }
    }
}

/// Calls `callback` for each GcHeader pointer found.
/// Objects are discovered by their `size` field (hop from one to the next).
pub fn arena_walk_objects(mut callback: impl FnMut(*mut u8)) {
    use crate::gc::GcHeader;

    // Sync inline state's offset back to the underlying block first,
    // so the walk sees objects that the inline allocator has emitted
    // since the last non-inline alloc. Only the general ARENA has an
    // inline path; the longlived arena is always sync by construction.
    sync_inline_arena_state();

    let mut walk_region = |blocks: &[ArenaBlock]| {
        for block in blocks {
            let mut offset = 0usize;
            while offset < block.offset {
                // Align to 8 bytes (all our allocations are 8-byte aligned)
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }

                let header_ptr = unsafe { block.data.add(aligned) };
                let header = header_ptr as *const GcHeader;

                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        // Invalid header — we've hit uninitialized or non-GC memory.
                        // This can happen because arena_alloc() (without GC) is still
                        // used for some allocations. Skip the rest of this block.
                        break;
                    }

                    // Only process if this looks like a valid GC object
                    let obj_type = (*header).obj_type;
                    if (1..=9).contains(&obj_type) {
                        callback(header_ptr);
                    }

                    offset = aligned + total_size;
                }
            }
        }
    };

    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks);
    });
    SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks);
    });
    SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks);
    });
    LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks);
    });
    // Phase B: walk old-gen blocks too. Empty until Phase C
    // populates them, but the walk is already free in that case
    // (zero blocks → zero iterations).
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks);
    });
}

/// Walk only objects physically allocated in the old-generation arena.
/// Dirty-page remembered scanning uses this to process old-gen modbuf
/// pages without touching nursery or longlived blocks.
pub fn old_arena_walk_objects(mut callback: impl FnMut(*mut u8)) {
    use crate::gc::GcHeader;

    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            let mut offset = 0usize;
            while offset < block.offset {
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }

                let header_ptr = unsafe { block.data.add(aligned) };
                let header = header_ptr as *const GcHeader;

                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        break;
                    }

                    let obj_type = (*header).obj_type;
                    if (1..=9).contains(&obj_type) {
                        callback(header_ptr);
                    }

                    offset = aligned + total_size;
                }
            }
        }
    });
}

/// Like `arena_walk_objects` but also passes the block's global index
/// alongside each header — used by the GC sweep to track per-block live
/// counts in a `Vec<bool>` (O(1) lookups) so it can reset fully-empty
/// blocks back to offset=0 in O(blocks) instead of O(objects).
///
/// Block indices are global across both arenas: `0..general_block_count()`
/// for the general arena, `general_block_count()..arena_block_count()`
/// for the longlived arena (issue #179).
pub fn arena_walk_objects_with_block_index(mut callback: impl FnMut(*mut u8, usize)) {
    use crate::gc::GcHeader;

    sync_inline_arena_state();

    let general_n = ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor0_n = SURVIVOR_ARENA_0.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor1_n = SURVIVOR_ARENA_1.with(|a| unsafe { (*a.get()).blocks.len() });
    let mut walk_region = |blocks: &[ArenaBlock], base: usize| {
        for (i, block) in blocks.iter().enumerate() {
            let block_idx = base + i;
            let mut offset = 0usize;
            while offset < block.offset {
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }
                let header_ptr = unsafe { block.data.add(aligned) };
                let header = header_ptr as *const GcHeader;
                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        break;
                    }
                    let obj_type = (*header).obj_type;
                    if (1..=9).contains(&obj_type) {
                        callback(header_ptr, block_idx);
                    }
                    offset = aligned + total_size;
                }
            }
        }
    };

    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks, 0);
    });
    SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks, general_n);
    });
    SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks, general_n + survivor0_n);
    });
    let longlived_n = LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks, general_n + survivor0_n + survivor1_n);
        arena.blocks.len()
    });
    // Phase B: old-gen blocks. Indices begin at
    // `general_n + longlived_n` per the global block-index plan.
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(
            &arena.blocks,
            general_n + survivor0_n + survivor1_n + longlived_n,
        );
    });
}

/// Like `arena_walk_objects_with_block_index` but filters whole blocks
/// up-front via `block_filter(block_idx) -> bool` — returning `false`
/// skips that block's entire object loop. This is O(n_blocks) vs
/// O(n_objects_in_skipped_blocks), which matters a lot when the GC
/// block-persistence pass has 3M dead objects spread across 27 blocks
/// it already knows have no live objects (issue #64 follow-up).
///
/// Block indices are global (general arena first, longlived after).
pub fn arena_walk_objects_filtered(
    mut block_filter: impl FnMut(usize) -> bool,
    mut callback: impl FnMut(*mut u8, usize),
) {
    use crate::gc::GcHeader;

    sync_inline_arena_state();

    let general_n = ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor0_n = SURVIVOR_ARENA_0.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor1_n = SURVIVOR_ARENA_1.with(|a| unsafe { (*a.get()).blocks.len() });
    let walk_region = |blocks: &[ArenaBlock],
                       base: usize,
                       block_filter: &mut dyn FnMut(usize) -> bool,
                       callback: &mut dyn FnMut(*mut u8, usize)| {
        for (i, block) in blocks.iter().enumerate() {
            let block_idx = base + i;
            if !block_filter(block_idx) {
                continue;
            }
            let mut offset = 0usize;
            while offset < block.offset {
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }
                let header_ptr = unsafe { block.data.add(aligned) };
                let header = header_ptr as *const GcHeader;
                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        break;
                    }
                    let obj_type = (*header).obj_type;
                    if (1..=9).contains(&obj_type) {
                        callback(header_ptr, block_idx);
                    }
                    offset = aligned + total_size;
                }
            }
        }
    };

    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks, 0, &mut block_filter, &mut callback);
    });
    SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(&arena.blocks, general_n, &mut block_filter, &mut callback);
    });
    SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(
            &arena.blocks,
            general_n + survivor0_n,
            &mut block_filter,
            &mut callback,
        );
    });
    let longlived_n = LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(
            &arena.blocks,
            general_n + survivor0_n + survivor1_n,
            &mut block_filter,
            &mut callback,
        );
        arena.blocks.len()
    });
    // Phase B: include old-gen blocks at indices
    // `general_n + longlived_n..` per the global block-index plan.
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        walk_region(
            &arena.blocks,
            general_n + survivor0_n + survivor1_n + longlived_n,
            &mut block_filter,
            &mut callback,
        );
    });
}

/// How many arena blocks are currently allocated across general +
/// longlived + old arenas. Used by the sweep to size its per-block
/// live-tracking `Vec<bool>` before walking objects. Block indices
/// are global: `0..general_block_count()` for nursery,
/// `..longlived_end()` for longlived, the rest for old-gen
/// (gen-GC Phase B).
pub fn arena_block_count() -> usize {
    let g = ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let s0 = SURVIVOR_ARENA_0.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let s1 = SURVIVOR_ARENA_1.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let l = LONGLIVED_ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let o = OLD_ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
    g + s0 + s1 + l + o
}

/// Block-index range boundary: block indices `0..general_block_count()`
/// belong to the general arena (eligible for reset), the rest belong to
/// the longlived OR old-gen arenas and must never be reset (issue #179
/// for longlived, gen-GC Phase B for old-gen — both are non-reset
/// regions; only the nursery resets).
#[inline]
pub fn general_block_count() -> usize {
    ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() })
}

/// Boundary between longlived and old-gen blocks. Indices
/// `general_block_count()..longlived_end()` are longlived;
/// `longlived_end()..arena_block_count()` are old-gen (gen-GC Phase B).
#[inline]
pub fn longlived_end() -> usize {
    let g = ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let s0 = SURVIVOR_ARENA_0.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let s1 = SURVIVOR_ARENA_1.with(|arena| unsafe { (*arena.get()).blocks.len() });
    let l = LONGLIVED_ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
    g + s0 + s1 + l
}

/// Fast path for the common case where the entire arena is empty
/// after GC (every object dead). Resets every block's offset to 0,
/// clears the free list, sets `current = 0`, and resyncs the inline
/// state. Avoids the per-block tracking HashMap that
/// `arena_reset_empty_blocks` needs.
///
/// This is what makes tight `new ClassName()` loops competitive with
/// V8: when the workload allocates short-lived class instances and
/// nothing escapes, GC observes that all 700k+ objects from the
/// previous burst are dead and reclaims the entire arena in O(1).
pub fn arena_reset_all_blocks_to_zero() {
    // Only the general arena is reset (issue #179). The longlived arena
    // holds cached data that must not be reclaimed.
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        for block in arena.blocks.iter_mut() {
            block.offset = 0;
        }
        arena.current = 0;
        // Free list is now invalid (all entries point into reset blocks).
        crate::gc::ARENA_FREE_LIST.with(|fl| fl.borrow_mut().clear());
        crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
        // Resync inline state to block 0 (offset 0, full size).
        INLINE_STATE.with(|s| {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &arena.blocks[0];
                inline.data = block.data;
                inline.offset = 0;
                inline.size = block.size;
            }
        });
    });
}

fn reset_region_to_zero(arena: &mut Arena) -> usize {
    let mut reset_blocks = 0usize;
    for block in arena.blocks.iter_mut() {
        if block.data.is_null() {
            continue;
        }
        if block.offset != 0 {
            reset_blocks += 1;
        }
        block.offset = 0;
        block.dead_cycles = 0;
    }
    arena.current = 0;
    reset_blocks
}

/// Reset the inactive survivor semispace before a copying minor starts.
pub(crate) fn copying_prepare_to_space() -> usize {
    let idx = inactive_survivor_index();
    with_survivor_arena_mut(idx, reset_region_to_zero)
}

/// Bytes currently allocated in Eden plus the active survivor from-space.
pub(crate) fn copying_from_space_in_use_bytes() -> usize {
    sync_inline_arena_state();
    let eden = ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena.blocks.iter().map(|b| b.offset).sum::<usize>()
    });
    let active = ACTIVE_SURVIVOR.with(|active| active.get());
    let survivor = with_survivor_arena(active, |arena| {
        arena.blocks.iter().map(|b| b.offset).sum::<usize>()
    });
    eden + survivor
}

/// Reset Eden and the active survivor from-space, then flip the survivor
/// roles so the to-space populated by the copying collector becomes active.
pub(crate) fn copying_reset_from_spaces_and_flip() -> ArenaResetStats {
    sync_inline_arena_state();
    let mut reset_blocks = 0usize;
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        reset_blocks += reset_region_to_zero(arena);
        crate::gc::ARENA_FREE_LIST.with(|fl| fl.borrow_mut().clear());
        crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
        INLINE_STATE.with(|s| {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
        });
    });

    let active = ACTIVE_SURVIVOR.with(|active| active.get());
    reset_blocks += with_survivor_arena_mut(active, reset_region_to_zero);
    ACTIVE_SURVIVOR.with(|active_cell| active_cell.set(1 - active));

    ArenaResetStats {
        reset_blocks,
        deallocated_blocks: 0,
        deallocated_bytes: 0,
    }
}

/// Reset arena blocks that have zero live objects after a GC sweep.
/// `live_block_data_ptrs` is the set of `block.data` pointers that
/// the sweep observed at least one live (marked or pinned) object in.
/// Any other block — i.e. one with `offset > 0` but no live objects —
/// is reclaimed by setting `offset = 0`. Free-list entries pointing
/// into the reset blocks are filtered out so the next allocation
/// doesn't hand back a stale slot in a region the inline allocator
/// is about to overwrite.
///
/// This is the load-bearing optimization that makes the inline bump
/// allocator perform competitively with V8 on tight `new` loops:
/// without it, every iteration page-faults through fresh memory once
/// the working set crosses ~64MB; with it, GC reclaims empty blocks
/// in place and the inline allocator keeps reusing the same ~8MB
/// arena block forever.
pub fn arena_reset_empty_blocks(block_has_live: &[bool]) -> ArenaResetStats {
    let n_live = block_has_live.iter().filter(|&&b| b).count();
    let n_total = block_has_live.len();
    // Issue #179: only reset general-arena blocks. Longlived-arena blocks
    // (global indices >= general arena block count) are never reclaimed;
    // they hold cached data whose addresses we've handed out to
    // root-tracked caches.
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        let mut reset_block_ranges: Vec<(usize, usize)> = Vec::new();
        // Issue #73: never reset the current block or the four blocks
        // immediately before it. Those are the most recent allocation
        // targets — they contain freshly-allocated objects whose
        // handles LLVM may still be holding in caller-saved registers
        // that the conservative scan didn't capture. Resetting them
        // overwrites those handles' backing stores on the very next
        // allocation and the rest of the program reads garbage.
        // Older blocks are safer: allocations there happened multiple
        // GC cycles ago and any still-live handle would have been
        // re-loaded from a stack slot by now.
        let current = arena.current;
        let keep_low = current.saturating_sub(4);
        for (i, block) in arena.blocks.iter_mut().enumerate() {
            // Tombstoned slot (gen-GC Phase C4b-δ): block was
            // deallocated on a prior cycle. Nothing to reset.
            if block.data.is_null() {
                continue;
            }
            let live = block_has_live.get(i).copied().unwrap_or(false);
            if block.offset == 0 {
                // Already empty before this cycle's sweep — let the
                // dealloc-candidate loop below decide whether to
                // increment `dead_cycles` (offset==0 + outside
                // recent window ⇒ candidate). Don't write dead_cycles
                // here: the dealloc loop is the single source of
                // truth and clearing here would defeat its accumulation.
                continue;
            }
            if live {
                // Live this cycle — dealloc loop sees offset != 0
                // (post-reset still nonzero) and resets dead_cycles=0.
                continue;
            }
            // Recent block — skip this cycle's reset decision.
            // The `keep_low..=current` window matches
            // `BLOCK_PERSIST_WINDOW` on the GC side: these are the
            // blocks where LLVM caller-saved registers might still
            // hold a freshly-allocated handle the conservative scan
            // couldn't capture (issues #43 / #44). Resetting them
            // overwrites those handles' backing stores on the very
            // next allocation.
            if i >= keep_low && i <= current {
                continue;
            }
            // Issue #179: reset OLD observed-dead blocks immediately.
            // The two-cycle grace that used to live here (issue #73)
            // was a blanket safety margin, but for blocks outside the
            // `keep_low..=current` window the register-miss risk has
            // already closed — any allocation whose handle was in a
            // caller-saved reg has been re-loaded from a stable slot
            // (or the register has been repurposed and the handle is
            // gone entirely) by the time 1+ GC cycles have passed.
            // Holding these blocks for an extra cycle just delayed
            // RSS reclaim by a full GC step on memory-pressured
            // workloads like `bench_json_roundtrip`, where the first
            // time a middle block surfaces as dead is often the last
            // time GC fires before the benchmark ends (total bytes
            // allocated ÷ adaptive step ≈ 3-4 cycles). Recent blocks
            // (`keep_low..=current`) still get the full "never reset"
            // protection above, which is where the scan-miss risk
            // actually lives.
            reset_block_ranges.push((block.data as usize, block.size));
            block.offset = 0;
            // Don't write dead_cycles — the dealloc-candidate loop
            // below sees offset==0 + outside-recent-window and
            // increments accordingly. Just-reset blocks therefore
            // start their dead-cycle countdown from this cycle.
        }
        if !reset_block_ranges.is_empty() {
            // Filter the free list: remove entries pointing into any
            // reset block. The bump allocator will overwrite those
            // slots, so the free list must not hand them back.
            crate::gc::ARENA_FREE_LIST.with(|fl| {
                let mut fl = fl.borrow_mut();
                fl.retain(|&(ptr, _)| {
                    let p = ptr as usize;
                    !reset_block_ranges
                        .iter()
                        .any(|&(base, size)| p >= base && p < base + size)
                });
                if fl.is_empty() {
                    crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
                }
            });
        }

        // Gen-GC Phase C4b-δ: deallocate fully-idle blocks back to
        // the OS. A block becomes a dealloc candidate when:
        //   - it's not the current allocator target
        //   - it's outside the `keep_low..=current` register-miss
        //     window (already excluded from reset above for the
        //     same reason — the conservative-scan caller-saved-reg
        //     risk),
        //   - its offset is zero (no active allocations — either
        //     reset this cycle or never used since the prior reset),
        //   - it's not already a tombstone.
        // Each candidate's `dead_cycles` increments per cycle; once
        // it reaches `DEALLOC_DEAD_CYCLES`, we hand the underlying
        // allocation back to glibc/jemalloc/whatever via `dealloc`
        // and leave a `data = null, size = 0` tombstone in the Vec
        // so block-index semantics stay stable for the rest of the
        // GC cycle. Future allocations preferentially reuse
        // tombstoned slots (`Arena::alloc`'s slow path) before
        // pushing new entries onto the Vec, so the index space
        // stays bounded even on workloads that churn nursery blocks.
        //
        // Threshold tuning: 2 cycles. A block resets on cycle N
        // (`dead_cycles=1` after this loop), and on cycle N+1 either
        // gets reused (offset > 0, dead_cycles back to 0) or stays
        // idle (`dead_cycles=2` ⇒ dealloc). Two cycles is the
        // minimum that gives the bump allocator one cycle to reuse
        // a freshly-reset block before declaring it truly idle —
        // catches the `bench_json_roundtrip` case (only 2-3 GCs
        // per run) while still letting tight allocation loops keep
        // hot blocks alive across consecutive resets.
        const DEALLOC_DEAD_CYCLES: u32 = 2;
        let mut deallocated_ranges: Vec<(usize, usize)> = Vec::new();
        for (i, block) in arena.blocks.iter_mut().enumerate() {
            if block.data.is_null() {
                continue;
            }
            if i == current {
                block.dead_cycles = 0;
                continue;
            }
            if i >= keep_low && i <= current {
                block.dead_cycles = 0;
                continue;
            }
            if block.offset != 0 {
                block.dead_cycles = 0;
                continue;
            }
            block.dead_cycles += 1;
            if block.dead_cycles >= DEALLOC_DEAD_CYCLES {
                let base = block.data as usize;
                let size = block.size;
                let layout = Layout::from_size_align(block.size, 16).unwrap();
                unregister_block_generation(base, size);
                deallocated_ranges.push((base, size));
                std::alloc::dealloc(block.data, layout);
                ARENA_TOTAL_BYTES.with(|t| t.set(t.get().saturating_sub(block.size)));
                block.data = std::ptr::null_mut();
                block.size = 0;
                block.offset = 0;
                block.dead_cycles = 0;
            }
        }
        let reset_blocks = reset_block_ranges.len();
        let deallocated_blocks = deallocated_ranges.len();
        let deallocated_bytes: usize = deallocated_ranges.iter().map(|&(_, s)| s).sum();
        let stats = ArenaResetStats {
            reset_blocks,
            deallocated_blocks,
            deallocated_bytes,
        };

        if !deallocated_ranges.is_empty() {
            // Drop free-list entries pointing into deallocated
            // blocks — same reasoning as the reset path, but the
            // memory is now gone, not just reusable.
            crate::gc::ARENA_FREE_LIST.with(|fl| {
                let mut fl = fl.borrow_mut();
                fl.retain(|&(ptr, _)| {
                    let p = ptr as usize;
                    !deallocated_ranges
                        .iter()
                        .any(|&(base, size)| p >= base && p < base + size)
                });
                if fl.is_empty() {
                    crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
                }
            });
            if std::env::var_os("PERRY_GC_DIAG").is_some() {
                eprintln!(
                    "[gc-dealloc] freed {} blocks ({} bytes) back to OS",
                    deallocated_ranges.len(),
                    deallocated_bytes
                );
            }
        }

        if reset_block_ranges.is_empty() && deallocated_ranges.is_empty() {
            stats
        } else {
            // Walk back the `current` index to the first reset block —
            // i.e., one with `offset == 0`. Skip tombstones (data.is_null())
            // — the inline allocator can't bump from a deallocated slot.
            // If we just picked the first block with any free space we'd
            // land on the live block that still has 80 bytes left at the
            // end (not enough for a 96-byte class instance), and the next
            // alloc would push a fresh block. The reset blocks are the
            // whole point of this routine — make sure we actually use one.
            let mut new_current = arena.current;
            for (i, block) in arena.blocks.iter().enumerate() {
                if !block.data.is_null() && block.offset == 0 {
                    new_current = i;
                    break;
                }
            }
            // If `new_current` ended up pointing at a tombstone (the only
            // remaining offset==0 entries are deallocated slots), keep
            // `arena.current` where it was — the next `Arena::alloc` slow
            // path will tombstone-reuse a slot and update `current` then.
            if !arena.blocks[new_current].data.is_null() {
                arena.current = new_current;
            }
            let _ = (n_live, n_total);
            INLINE_STATE.with(|s| {
                let inline = &mut *s.get();
                if !inline.data.is_null() {
                    let block = &arena.blocks[arena.current];
                    if !block.data.is_null() {
                        inline.data = block.data;
                        inline.offset = block.offset;
                        inline.size = block.size;
                    }
                }
            });
            stats
        }
    })
}

/// Get arena memory statistics: (heap_used, heap_total)
/// heap_used = total bytes allocated across all blocks
/// heap_total = total bytes reserved across all blocks
#[no_mangle]
pub extern "C" fn js_arena_stats(out_used: *mut u64, out_total: *mut u64) {
    // Sync inline state so the "used" count reflects the inline-burst
    // high-water mark, not just the last sync point.
    sync_inline_arena_state();
    let mut used: u64 = 0;
    let mut total: u64 = 0;
    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset as u64;
            total += block.size as u64;
        }
    });
    LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset as u64;
            total += block.size as u64;
        }
    });
    SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset as u64;
            total += block.size as u64;
        }
    });
    SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            used += block.offset as u64;
            total += block.size as u64;
        }
    });
    unsafe {
        *out_used = used;
        *out_total = total;
    }
}

/// Bytes currently allocated in the longlived arena (sum of per-block
/// offsets). Diagnostic-only — used by tests and `PERRY_GC_DIAG=1` output
/// to confirm that long-lived allocations are actually routed into the
/// segregated region.
pub fn longlived_in_use_bytes() -> usize {
    LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena.blocks.iter().map(|b| b.offset).sum()
    })
}

/// Bytes currently allocated in the old-gen arena (gen-GC Phase B).
/// Diagnostic-only — empty in Phase B; populated by Phase C's
/// nursery→old promotion path.
pub fn old_gen_in_use_bytes() -> usize {
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena.blocks.iter().map(|b| b.offset).sum()
    })
}

#[inline]
pub(crate) fn active_survivor_space() -> HeapSpace {
    ACTIVE_SURVIVOR.with(|active| match active.get() {
        0 => HeapSpace::Survivor0,
        1 => HeapSpace::Survivor1,
        _ => HeapSpace::Unknown,
    })
}

#[inline]
pub(crate) fn inactive_survivor_space() -> HeapSpace {
    match active_survivor_space() {
        HeapSpace::Survivor0 => HeapSpace::Survivor1,
        HeapSpace::Survivor1 => HeapSpace::Survivor0,
        _ => HeapSpace::Unknown,
    }
}

/// Gen-GC Phase C: is `addr` inside any nursery (= general
/// `ARENA`) block? Hot-path predicate for the write barrier —
/// "is the child of this store a young-gen pointer?". Backed by
/// range side metadata so the runtime barrier does not scan every
/// arena block on each heap store, while avoiding per-card metadata
/// growth on low-pressure nursery churn.
#[inline]
pub fn pointer_in_nursery(addr: usize) -> bool {
    classify_heap_space(addr).is_nursery()
}

/// Gen-GC Phase C: is `addr` inside any old-gen arena block?
/// Mirror of `pointer_in_nursery`, also backed by range side
/// metadata.
#[inline]
pub fn pointer_in_old_gen(addr: usize) -> bool {
    matches!(classify_heap_generation(addr), HeapGeneration::Old)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{GcHeader, GC_FLAG_MARKED, GC_HEADER_SIZE, GC_TYPE_ARRAY, GC_TYPE_STRING};

    fn general_block_index_for(addr: usize) -> Option<usize> {
        sync_inline_arena_state();
        ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            arena.blocks.iter().enumerate().find_map(|(idx, block)| {
                if block.data.is_null() {
                    return None;
                }
                let base = block.data as usize;
                let end = base + block.size;
                (addr >= base && addr < end).then_some(idx)
            })
        })
    }

    fn general_block_offset(idx: usize) -> usize {
        sync_inline_arena_state();
        ARENA.with(|a| unsafe { (&*a.get()).blocks[idx].offset })
    }

    fn run_with_fresh_arenas(test: impl FnOnce() + Send + 'static) {
        std::thread::spawn(test)
            .join()
            .expect("arena test panicked");
    }

    fn reset_old_nursery_block(dead_cycles_before: u32) -> (usize, usize, usize, ArenaResetStats) {
        let full_block_payload = BLOCK_SIZE - GC_HEADER_SIZE;
        let mut blocks = Vec::new();
        for _ in 0..7 {
            let ptr = arena_alloc_gc(full_block_payload, 8, GC_TYPE_STRING) as usize;
            let idx = general_block_index_for(ptr).expect("allocation should be in nursery");
            blocks.push(idx);
        }
        blocks.sort_unstable();
        blocks.dedup();
        assert!(
            blocks.len() >= 7,
            "test setup should force seven distinct nursery blocks"
        );

        let current = ARENA.with(|a| unsafe { (&*a.get()).current });
        let keep_low = current.saturating_sub(4);
        let candidate = blocks
            .into_iter()
            .find(|&idx| idx < keep_low)
            .expect("test setup should leave a nursery block outside the keep window");

        let (base, size) = ARENA.with(|a| unsafe {
            let arena = &mut *a.get();
            let block = &mut arena.blocks[candidate];
            assert!(!block.data.is_null());
            assert!(block.offset > 0);
            block.dead_cycles = dead_cycles_before;
            (block.data as usize, block.size)
        });

        let mut block_has_live = vec![false; arena_block_count()];
        block_has_live[current] = true;
        let stats = arena_reset_empty_blocks(&block_has_live);
        (candidate, base, size, stats)
    }

    #[test]
    fn generation_metadata_classifies_arena_regions() {
        run_with_fresh_arenas(|| {
            let nursery = arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
            let longlived = arena_alloc_gc_longlived(32, 8, GC_TYPE_STRING) as usize;
            let old = arena_alloc_gc_old(32, 8, GC_TYPE_STRING) as usize;

            assert_eq!(classify_heap_generation(nursery), HeapGeneration::Nursery);
            assert_eq!(
                classify_heap_generation(longlived),
                HeapGeneration::Longlived
            );
            assert_eq!(classify_heap_generation(old), HeapGeneration::Old);
            assert!(pointer_in_nursery(nursery));
            assert!(!pointer_in_nursery(longlived));
            assert!(!pointer_in_old_gen(longlived));
            assert!(pointer_in_old_gen(old));
        });
    }

    #[test]
    fn generation_metadata_bucket_keeps_exact_range_boundaries() {
        run_with_fresh_arenas(|| {
            let bucket_base = 0x0055_0000_0000usize & !((1usize << GENERATION_CLASS_SHIFT) - 1);
            let nursery_base = bucket_base + 0x1000;
            let old_base = bucket_base + 0x4000;
            let range_size = 0x1000;

            register_block_space(
                nursery_base,
                range_size,
                HeapGeneration::Nursery,
                HeapSpace::NurseryEden,
            );
            register_block_space(old_base, range_size, HeapGeneration::Old, HeapSpace::Old);

            assert_eq!(
                classify_heap_generation(nursery_base + 0x80),
                HeapGeneration::Nursery
            );
            assert_eq!(
                classify_heap_generation(old_base + 0x80),
                HeapGeneration::Old
            );
            assert_eq!(
                classify_heap_generation(bucket_base + 0x3000),
                HeapGeneration::Unknown,
                "same metadata bucket must not classify holes between exact ranges"
            );

            unregister_block_generation(nursery_base, range_size);
            assert_eq!(
                classify_heap_generation(nursery_base + 0x80),
                HeapGeneration::Unknown
            );
            assert_eq!(
                classify_heap_generation(old_base + 0x80),
                HeapGeneration::Old,
                "removing one range must not remove another range in the same bucket"
            );

            unregister_block_generation(old_base, range_size);
            assert_eq!(
                classify_heap_generation(old_base + 0x80),
                HeapGeneration::Unknown
            );
        });
    }

    #[test]
    fn generation_metadata_survives_nursery_block_reset() {
        run_with_fresh_arenas(|| {
            let (idx, base, size, stats) = reset_old_nursery_block(0);
            assert!(
                stats.reset_blocks >= 1,
                "test setup should reset at least one nursery block"
            );
            ARENA.with(|a| unsafe {
                let arena = &*a.get();
                assert!(!arena.blocks[idx].data.is_null());
                assert_eq!(arena.blocks[idx].offset, 0);
            });
            assert_eq!(classify_heap_generation(base), HeapGeneration::Nursery);
            assert_eq!(
                classify_heap_generation(base + size - 1),
                HeapGeneration::Nursery
            );
        });
    }

    #[test]
    fn generation_metadata_removed_on_nursery_block_deallocation() {
        run_with_fresh_arenas(|| {
            let (idx, base, _size, stats) = reset_old_nursery_block(1);
            assert!(
                stats.deallocated_blocks >= 1,
                "test setup should deallocate at least one nursery block"
            );
            ARENA.with(|a| unsafe {
                let arena = &*a.get();
                assert!(arena.blocks[idx].data.is_null());
                assert_eq!(arena.blocks[idx].size, 0);
            });
            assert_eq!(classify_heap_generation(base), HeapGeneration::Unknown);
            assert!(!pointer_in_nursery(base));
        });
    }

    #[test]
    fn generation_metadata_registered_on_tombstone_reuse() {
        run_with_fresh_arenas(|| {
            let (idx, _base, _size, stats) = reset_old_nursery_block(1);
            assert!(
                stats.deallocated_blocks >= 1,
                "test setup should create a nursery tombstone"
            );

            let oversized = arena_alloc_gc(BLOCK_SIZE + 64, 8, GC_TYPE_STRING) as usize;
            ARENA.with(|a| unsafe {
                let arena = &*a.get();
                assert!(!arena.blocks[idx].data.is_null());
                assert!(
                    arena.blocks[idx].size > BLOCK_SIZE,
                    "oversized allocation should replace the tombstone with a fresh block"
                );
            });
            assert_eq!(general_block_index_for(oversized), Some(idx));
            assert_eq!(classify_heap_generation(oversized), HeapGeneration::Nursery);
        });
    }

    /// Issue #179: a longlived-arena allocation must not land inside any
    /// general-arena block. This is the architectural guarantee behind
    /// the "segregated quarantine" design — GP blocks can be reset on
    /// GC without touching cached object pointers, which stay parked in
    /// longlived blocks.
    #[test]
    fn longlived_pointer_is_disjoint_from_general_blocks() {
        // Force a general-arena allocation first so block 0 exists.
        let gen_ptr = arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
        let ll_ptr = arena_alloc_gc_longlived(32, 8, GC_TYPE_STRING) as usize;

        // Collect general-arena block ranges.
        let mut general_ranges: Vec<(usize, usize)> = Vec::new();
        ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            for block in &arena.blocks {
                general_ranges.push((block.data as usize, block.size));
            }
        });

        let in_general = general_ranges
            .iter()
            .any(|&(base, size)| ll_ptr >= base && ll_ptr < base + size);
        assert!(
            !in_general,
            "longlived pointer {ll_ptr:#x} landed inside a general-arena block; \
             segregation is broken"
        );

        // Sanity: general allocation IS in a general block.
        let gen_in_general = general_ranges
            .iter()
            .any(|&(base, size)| gen_ptr >= base && gen_ptr < base + size);
        assert!(
            gen_in_general,
            "general alloc {gen_ptr:#x} not in any general block"
        );
    }

    #[test]
    fn test_arena_reset_reuses_dead_general_block_without_touching_live_block() {
        let full_block_payload = BLOCK_SIZE - GC_HEADER_SIZE;
        let mut dead_blocks = Vec::new();

        for _ in 0..6 {
            let ptr = arena_alloc_gc(full_block_payload, 8, GC_TYPE_STRING) as usize;
            let block_idx =
                general_block_index_for(ptr).expect("dead allocation should land in general arena");
            dead_blocks.push(block_idx);
        }

        dead_blocks.sort_unstable();
        dead_blocks.dedup();
        assert!(
            dead_blocks.len() >= 6,
            "test setup should force six distinct full general blocks"
        );

        let live_ptr = arena_alloc_gc(full_block_payload, 8, GC_TYPE_STRING);
        let live_addr = live_ptr as usize;
        let live_header_addr = live_addr - GC_HEADER_SIZE;
        let live_block =
            general_block_index_for(live_addr).expect("live allocation should be in general arena");
        let current = ARENA.with(|a| unsafe { (&*a.get()).current });
        let keep_low = current.saturating_sub(4);
        let reset_candidate = dead_blocks
            .iter()
            .copied()
            .find(|&idx| idx < keep_low)
            .expect("test setup should leave at least one dead block outside the keep window");

        let before_offset = general_block_offset(reset_candidate);
        assert!(
            before_offset > 0,
            "reset candidate should contain dead allocations before reset"
        );

        unsafe {
            let header = (live_header_addr as *mut u8) as *mut GcHeader;
            (*header).gc_flags |= GC_FLAG_MARKED;
            *(live_ptr as *mut u64) = 0xCAFE_BABE_DEAD_BEEF;
            *(live_ptr.add(8) as *mut u64) = 0x1234_5678_9ABC_DEF0;
        }
        let live_header_size = unsafe { (*(live_header_addr as *const GcHeader)).size };

        ARENA.with(|a| unsafe {
            let arena = &mut *a.get();
            arena.blocks[reset_candidate].dead_cycles = 0;
            arena.blocks[live_block].dead_cycles = 0;
        });

        let mut block_has_live = vec![false; arena_block_count()];
        block_has_live[live_block] = true;
        arena_reset_empty_blocks(&block_has_live);

        assert_eq!(
            general_block_offset(reset_candidate),
            0,
            "dead general block should be reset for reuse"
        );
        assert!(
            general_block_offset(live_block) > 0,
            "live general block should keep its nonzero offset"
        );

        let blocks_after_reset = general_block_count();
        let _reused = arena_alloc_gc(24, 8, GC_TYPE_STRING);
        assert_eq!(
            general_block_count(),
            blocks_after_reset,
            "allocation after reset should reuse existing arena capacity"
        );

        unsafe {
            assert_eq!(*(live_ptr as *const u64), 0xCAFE_BABE_DEAD_BEEF);
            assert_eq!(*(live_ptr.add(8) as *const u64), 0x1234_5678_9ABC_DEF0);
            let header = (live_header_addr as *mut u8) as *mut GcHeader;
            assert_eq!((*header).obj_type, GC_TYPE_STRING);
            assert_eq!(
                (*header).size,
                live_header_size,
                "live header size should not change during reset"
            );
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
    }

    /// Walker + block-index contract: longlived objects get global
    /// block indices at or above `general_block_count()`, so the
    /// `arena_reset_empty_blocks` range check correctly skips them.
    #[test]
    fn longlived_walk_yields_indices_outside_general_range() {
        // Ensure each arena has at least one block with one allocation.
        let _g = arena_alloc_gc(16, 8, GC_TYPE_ARRAY) as usize;
        let ll = arena_alloc_gc_longlived(24, 8, GC_TYPE_STRING) as usize;

        let general_n = general_block_count();
        let mut seen_ll_idx: Option<usize> = None;
        arena_walk_objects_with_block_index(|header_ptr, block_idx| {
            let user_ptr = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) } as usize;
            if user_ptr == ll {
                seen_ll_idx = Some(block_idx);
            }
        });
        let idx = seen_ll_idx.expect("longlived allocation not visited by walker");
        assert!(
            idx >= general_n,
            "longlived block_idx {idx} must be ≥ general_block_count {general_n}"
        );
    }

    /// `arena_reset_empty_blocks` must never reset a longlived block,
    /// even if its block-has-live slot is `false`. This is the load-
    /// bearing correctness guarantee: cache-held pointers into the
    /// longlived arena must survive GC cycles where the cache itself
    /// is the only thing referencing them.
    #[test]
    fn reset_never_clears_longlived_blocks() {
        let ll = arena_alloc_gc_longlived(40, 8, GC_TYPE_STRING) as usize;
        let ll_header_in_block = {
            // The header sits GC_HEADER_SIZE before the user pointer;
            // use the user pointer for range comparison below.
            ll - GC_HEADER_SIZE
        };

        let n_blocks = arena_block_count();
        // Build a block_has_live where EVERY block is marked dead.
        let all_dead = vec![false; n_blocks];
        arena_reset_empty_blocks(&all_dead);

        // The longlived allocation must still be readable (its block
        // wasn't reset, so the bytes are still there).
        let mut found = false;
        LONGLIVED_ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            for block in &arena.blocks {
                let base = block.data as usize;
                if ll_header_in_block >= base && ll_header_in_block < base + block.size {
                    // Block still has nonzero offset (not reset).
                    assert!(
                        block.offset > 0,
                        "longlived block reset to offset=0 despite reset_empty_blocks guard"
                    );
                    found = true;
                }
            }
        });
        assert!(found, "longlived alloc not located in any longlived block");
    }

    /// Gen-GC Phase B: an old-gen allocation must not land inside
    /// any general-arena (= nursery) block. Mirror of
    /// `longlived_pointer_is_disjoint_from_general_blocks`.
    #[test]
    fn old_gen_pointer_is_disjoint_from_nursery_blocks() {
        let _gen_ptr = arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let old_header = old_ptr - GC_HEADER_SIZE;
        ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            for block in &arena.blocks {
                let base = block.data as usize;
                let end = base + block.size;
                assert!(
                    old_header < base || old_header >= end,
                    "old-gen alloc landed inside a nursery block (got {:x}, block [{:x}, {:x}))",
                    old_header,
                    base,
                    end,
                );
            }
        });
    }

    /// Gen-GC Phase B: an old-gen allocation must not land inside
    /// any longlived block either — three regions are pairwise
    /// disjoint.
    #[test]
    fn old_gen_pointer_is_disjoint_from_longlived_blocks() {
        let _ll = arena_alloc_gc_longlived(40, 8, GC_TYPE_STRING) as usize;
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let old_header = old_ptr - GC_HEADER_SIZE;
        LONGLIVED_ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            for block in &arena.blocks {
                let base = block.data as usize;
                let end = base + block.size;
                assert!(
                    old_header < base || old_header >= end,
                    "old-gen alloc landed inside a longlived block",
                );
            }
        });
    }

    /// Gen-GC Phase B: walker must yield indices for old-gen
    /// blocks at `>= longlived_end()`. Confirms the global block-
    /// index plan: nursery first, then longlived, then old-gen.
    #[test]
    fn old_gen_walk_yields_indices_after_longlived() {
        let _gen = arena_alloc_gc(24, 8, GC_TYPE_STRING) as usize;
        let _ll = arena_alloc_gc_longlived(24, 8, GC_TYPE_STRING) as usize;
        let old_ptr = arena_alloc_gc_old(24, 8, GC_TYPE_STRING) as usize;
        let old_header = old_ptr - GC_HEADER_SIZE;
        let boundary = longlived_end();
        let mut found_at_idx: Option<usize> = None;
        arena_walk_objects_with_block_index(|hdr, block_idx| {
            if hdr as usize == old_header {
                found_at_idx = Some(block_idx);
            }
        });
        let idx = found_at_idx.expect("old-gen alloc not yielded by walker");
        assert!(
            idx >= boundary,
            "old-gen block index {} should be >= longlived_end() {}",
            idx,
            boundary,
        );
    }

    /// Gen-GC Phase B: arena_reset_empty_blocks must NEVER touch
    /// an old-gen block, even when every general/longlived/old
    /// block is marked dead. Promotion implies indefinite lifetime.
    #[test]
    fn reset_never_clears_old_gen_blocks() {
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let old_header = old_ptr - GC_HEADER_SIZE;
        let n_blocks = arena_block_count();
        let all_dead = vec![false; n_blocks];
        arena_reset_empty_blocks(&all_dead);
        let mut still_alive = false;
        OLD_ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            for block in &arena.blocks {
                let base = block.data as usize;
                if old_header >= base && old_header < base + block.size {
                    assert!(
                        block.offset > 0,
                        "old-gen block reset to offset=0 despite reset guard",
                    );
                    still_alive = true;
                }
            }
        });
        assert!(
            still_alive,
            "old-gen alloc not located in any old-gen block"
        );
    }
}
