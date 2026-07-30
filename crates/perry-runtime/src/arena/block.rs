use super::*;

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
pub(crate) const BLOCK_SIZE: usize = 1024 * 1024;
pub(crate) const FRESH_GENERAL_BLOCK_MIN_USED_BYTES: usize = 256 * 1024;

/// Create a block of at least the given size (for oversized allocations)
#[inline]
fn block_size_for(min_size: usize) -> usize {
    if min_size <= BLOCK_SIZE {
        BLOCK_SIZE
    } else {
        // Round up to next multiple of BLOCK_SIZE
        min_size.div_ceil(BLOCK_SIZE) * BLOCK_SIZE
    }
}

/// One raw block allocation. NEVER collects, so it is safe under an `&mut
/// Arena` borrow. `injectable` marks the single call site a test is allowed to
/// force a failure at (see [`force_next_block_alloc_failure`]) — the collection
/// that `reserve_arena_block` then runs allocates blocks of its own through the
/// non-injectable path, so the injected refusal cannot be consumed by the wrong
/// allocation.
fn try_alloc_block(min_size: usize, injectable: bool) -> Option<ArenaBlock> {
    let size = block_size_for(min_size);
    let layout = Layout::from_size_align(size, 16).unwrap();
    #[cfg(test)]
    if injectable && FORCE_BLOCK_ALLOC_FAILURE.with(|f| f.replace(false)) {
        return None;
    }
    #[cfg(not(test))]
    let _ = injectable;
    let data = unsafe { alloc(layout) };
    if data.is_null() {
        return None;
    }
    Some(ArenaBlock {
        data,
        size,
        offset: 0,
        dead_cycles: 0,
    })
}

/// Reserve a block, running one emergency full collection if the OS refuses
/// memory — idle-block dealloc and the malloc sweep can return real pages.
///
/// **NO `&mut Arena` BORROW MAY BE LIVE ACROSS THIS CALL (#7022).** The
/// emergency collection allocates into the arenas exactly like the
/// allocation-point trigger does, so it can grow `self.blocks` underneath a
/// borrow held by the frame that is extending the arena — the same aliasing
/// violation [`arena_cell_alloc`] exists to avoid, on the out-of-memory path.
/// `arena_cell_alloc` is the only caller; every `&mut self` path uses
/// [`alloc_block_no_gc`].
pub(crate) fn reserve_arena_block(min_size: usize) -> ArenaBlock {
    if let Some(block) = try_alloc_block(min_size, true) {
        return block;
    }
    note_gc_trigger_arena_borrow_depth();
    crate::gc::gc_try_emergency_reclaim();
    if let Some(block) = try_alloc_block(min_size, false) {
        return block;
    }
    panic!(
        "Failed to allocate arena block of {} bytes (heap exhausted after emergency GC)",
        block_size_for(min_size)
    );
}

/// Block allocation for the `&mut self` paths — `Arena::alloc`, the C4b
/// evacuation path's `alloc_excluding_pages`, and `arena_start_fresh_general_block`.
/// Deliberately does NOT run the emergency reclaim: those callers hold a live
/// arena borrow, and two of the three are already executing inside a collection,
/// where starting another one is precisely what must not happen. The mutator
/// allocation path — the one where heap exhaustion actually surfaces — keeps the
/// reclaim via [`reserve_arena_block`].
fn alloc_block_no_gc(min_size: usize) -> ArenaBlock {
    try_alloc_block(min_size, false).unwrap_or_else(|| {
        panic!(
            "Failed to allocate arena block of {} bytes (heap exhausted)",
            block_size_for(min_size)
        )
    })
}

/// A single arena block
pub(crate) struct ArenaBlock {
    pub(crate) data: *mut u8,
    pub(crate) size: usize,
    pub(crate) offset: usize,
    /// Issue #73: number of consecutive GC cycles this block has been
    /// observed with zero live objects. Reset requires TWO consecutive
    /// dead observations so a block can't be reclaimed on the same
    /// cycle its last live pointer slipped off the conservative scan
    /// (e.g. LLVM dropped a `samples` handle from a caller-saved FP
    /// reg after the IndexSet store). On the next cycle either the
    /// scan finds the pointer (counter resets to 0) or the block is
    /// truly dead and resets.
    pub(crate) dead_cycles: u32,
}

impl ArenaBlock {
    /// The initial block of a thread's arena, built during `Arena::new` — i.e.
    /// while the arena does not exist yet, so nothing may collect here.
    fn new() -> Self {
        alloc_block_no_gc(BLOCK_SIZE)
    }

    /// Try to allocate within this block, respecting alignment
    #[inline]
    pub(crate) fn alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
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
        // Check arithmetic on the bump path explicitly. `allocation_start`
        // already uses checked_add for the excluded-pages slow path; this
        // mirrors that on the fast path so a hostile `size`/`align` pair
        // can never wrap and hand back an in-bounds pointer for an
        // out-of-bounds region.
        let aligned_offset = self.offset.checked_add(pad - 1)? & !(pad - 1);
        let bumped = aligned_offset.checked_add(size)?;
        if bumped > self.size {
            return None;
        }

        let ptr = unsafe { self.data.add(aligned_offset) };
        let next = bumped.checked_add(pad - 1)? & !(pad - 1);
        self.offset = next;
        Some(ptr)
    }

    #[inline]
    fn allocation_start(&self, size: usize, align: usize) -> Option<usize> {
        if self.data.is_null() {
            return None;
        }
        let pad = align.max(8);
        let aligned_offset = (self.offset + pad - 1) & !(pad - 1);
        let bumped = aligned_offset.checked_add(size)?;
        if bumped > self.size {
            return None;
        }
        (self.data as usize).checked_add(aligned_offset)
    }

    #[inline]
    pub(crate) fn alloc_excluding_pages(
        &mut self,
        size: usize,
        align: usize,
        excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
    ) -> Option<*mut u8> {
        let start = self.allocation_start(size, align)?;
        if address_span_overlaps_pages(start, size, excluded_pages) {
            return None;
        }
        self.alloc(size, align)
    }
}

/// Thread-local arena allocator
///
/// When a thread exits (e.g., worker threads from `perry/thread`), the Drop
/// impl frees all arena blocks so memory isn't leaked.
pub(crate) struct Arena {
    pub(crate) blocks: Vec<ArenaBlock>,
    pub(crate) current: usize,
    pub(crate) generation: HeapGeneration,
    pub(crate) space: HeapSpace,
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
                // #4665: in test builds keep freed blocks mapped (no munmap) so
                // unit tests holding raw GC pointers across a collection read stale
                // bytes instead of SIGSEGV-ing on an unmapped page.
                if !cfg!(test) {
                    std::alloc::dealloc(block.data, layout);
                }
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

    /// Lazy variant of `new`: starts with a single tombstone block
    /// (`data = null, size = 0`) instead of an eagerly-mapped 1 MB
    /// block, so JS-touching threads that never allocate in this
    /// region (spawn workers, tokio callers) don't pay the block.
    /// The tombstone shape is exactly the one C4b-δ dealloc leaves
    /// behind, so every walker/reset/alloc path already handles it:
    /// the first `alloc` misses the tombstone, and the slow path's
    /// `install_fresh_block` replaces the tombstone slot in place.
    /// Only used for the non-Eden regions — Eden must stay eager
    /// because `js_inline_arena_state` hands its current block to
    /// codegen's inline bump allocator at thread start.
    fn new_lazy(generation: HeapGeneration, space: HeapSpace) -> Self {
        Arena {
            blocks: vec![ArenaBlock {
                data: std::ptr::null_mut(),
                size: 0,
                offset: 0,
                dead_cycles: 0,
            }],
            current: 0,
            generation,
            space,
        }
    }

    /// Bump-allocate in `blocks[idx]`, delta-maintaining the cached
    /// old-gen in-use counter (see `OLD_GEN_IN_USE_BYTES`). Every
    /// successful offset advance of an old-arena block MUST go through
    /// here — a bypassed site silently skews the OldReclaim trigger.
    #[inline]
    fn try_block_alloc(&mut self, idx: usize, size: usize, align: usize) -> Option<*mut u8> {
        let before = self.blocks[idx].offset;
        let ptr = self.blocks[idx].alloc(size, align)?;
        if self.generation == HeapGeneration::Old {
            old_gen_in_use_bytes_add(self.blocks[idx].offset - before);
        }
        Some(ptr)
    }

    /// `try_block_alloc` twin for the page-excluding path.
    #[inline]
    fn try_block_alloc_excluding_pages(
        &mut self,
        idx: usize,
        size: usize,
        align: usize,
        excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
    ) -> Option<*mut u8> {
        let before = self.blocks[idx].offset;
        let ptr = self.blocks[idx].alloc_excluding_pages(size, align, excluded_pages)?;
        if self.generation == HeapGeneration::Old {
            old_gen_in_use_bytes_add(self.blocks[idx].offset - before);
        }
        Some(ptr)
    }

    #[inline]
    fn resync_inline_to_current(&self) {
        // `INLINE_STATE` mirrors ONLY the general nursery-Eden arena — the
        // one the codegen inline bump-allocator (`js_inline_arena_state`)
        // targets. The old-gen, survivor, and longlived arenas reuse this
        // same `Arena::alloc` body, whose block-reuse forward-scan
        // (`self.current = i`) calls back here. Without this guard, an
        // old-gen/survivor allocation that forward-scans to reuse a block
        // would repoint `INLINE_STATE` at a non-Eden block; the next Eden
        // `arena_alloc` then writes that foreign block's offset into the
        // real current Eden block, rewinding it and allocating fresh objects
        // over still-live ones (#1824: a large-JSON `await` allocation that
        // landed in old-gen clobbered a suspended async-step closure, whose
        // bytes were then read as a garbage function pointer → SIGSEGV on
        // resume; reproduced with `full_gc=0`, so no collection involved).
        if self.space != HeapSpace::NurseryEden {
            return;
        }
        INLINE_STATE.with(|s| unsafe {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &self.blocks[self.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
        });
    }

    /// Reserve **and** install a block. Never collects — see
    /// [`alloc_block_no_gc`] for why.
    pub(crate) fn install_fresh_block(&mut self, size: usize) {
        self.install_reserved_block(alloc_block_no_gc(size));
    }

    /// Install a block that was reserved with no arena borrow live (#7022).
    pub(crate) fn install_reserved_block(&mut self, fresh: ArenaBlock) {
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
    }

    fn alloc_fresh_block(&mut self, size: usize, align: usize) -> *mut u8 {
        self.install_fresh_block(size);
        self.try_block_alloc(self.current, size, align)
            .expect("Fresh block should have space")
    }

    fn alloc_fresh_block_excluding_pages(
        &mut self,
        size: usize,
        align: usize,
        excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
    ) -> *mut u8 {
        loop {
            self.install_fresh_block(size);
            if let Some(ptr) =
                self.try_block_alloc_excluding_pages(self.current, size, align, excluded_pages)
            {
                return ptr;
            }
        }
    }

    /// Fast path only: bump within the current block. Never collects, never
    /// pushes a block — so it is safe to call under a short `&mut` borrow.
    #[inline]
    pub(crate) fn try_alloc_current(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        self.try_block_alloc(self.current, size, align)
    }

    /// Everything `alloc` does *after* the GC trigger: retry the (possibly
    /// newly reset) current block, scan the other blocks, then install a fresh
    /// one. Split out of `alloc` for #7022 — see [`arena_cell_alloc`].
    #[inline]
    pub(crate) fn alloc_after_gc(&mut self, size: usize, align: usize) -> *mut u8 {
        if let Some(ptr) = self.try_alloc_after_gc(size, align) {
            return ptr;
        }
        // Still no room anywhere — need a fresh block. C4b-δ:
        // prefer reusing a tombstoned slot (a block deallocated by
        // `arena_reset_empty_blocks` after staying idle past the
        // dealloc threshold) over growing the Vec, so block_idx
        // semantics stay bounded even on workloads that churn
        // through nursery blocks.
        self.alloc_fresh_block(size, align)
    }

    /// The part of [`Self::alloc_after_gc`] that can be satisfied from blocks
    /// the arena already owns. Returns `None` when a fresh block is needed —
    /// which `arena_cell_alloc` reserves with no borrow live, because that
    /// reservation can collect (#7022).
    #[inline]
    pub(crate) fn try_alloc_after_gc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        // Retry the (possibly newly-reset) current block. arena.current
        // may have been changed by arena_reset_empty_blocks to point
        // at the lowest reset block.
        if let Some(ptr) = self.try_block_alloc(self.current, size, align) {
            return Some(ptr);
        }

        // Scan forward for any other block with space — the GC may
        // have reset blocks we haven't tried yet. Without this scan,
        // we'd push a fresh block on the very first overflow even
        // though blocks `current+1..n_blocks` are all empty.
        for i in 0..self.blocks.len() {
            if i == self.current {
                continue;
            }
            if let Some(ptr) = self.try_block_alloc(i, size, align) {
                self.current = i;
                // Resync inline state to the new current block.
                self.resync_inline_to_current();
                return Some(ptr);
            }
        }
        None
    }

    /// GC-free allocation. Used by paths that already run inside a collection
    /// (`alloc_excluding_pages`) and by the tests; the collecting entry point
    /// is [`arena_cell_alloc`].
    #[inline]
    pub(crate) fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        if let Some(ptr) = self.try_alloc_current(size, align) {
            return ptr;
        }
        self.alloc_after_gc(size, align)
    }

    pub(crate) fn alloc_excluding_pages(
        &mut self,
        size: usize,
        align: usize,
        excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
    ) -> *mut u8 {
        if excluded_pages.is_empty() {
            return self.alloc(size, align);
        }
        if let Some(ptr) =
            self.try_block_alloc_excluding_pages(self.current, size, align, excluded_pages)
        {
            return ptr;
        }
        for i in 0..self.blocks.len() {
            if i == self.current {
                continue;
            }
            if let Some(ptr) = self.try_block_alloc_excluding_pages(i, size, align, excluded_pages)
            {
                self.current = i;
                self.resync_inline_to_current();
                return ptr;
            }
        }
        self.alloc_fresh_block_excluding_pages(size, align, excluded_pages)
    }
}

/// Allocate from an arena that lives behind a thread-local `UnsafeCell`,
/// running the allocation-point GC trigger **between** two disjoint borrows.
///
/// # Why the borrow has to be split (#7022)
///
/// `gc_check_trigger()` can run a full mark-sweep or an evacuating minor, and
/// both of those *allocate into the arenas*: promotion and C4b evacuation call
/// `arena_alloc_gc_old`, the copying minor fills a survivor semispace, and
/// either may reach `install_fresh_block` → `self.blocks.push(..)`. When the
/// trigger was called from inside `Arena::alloc(&mut self, ..)`, that push ran
/// against the **same** `Arena` the caller was holding a live `&mut` to: a
/// `Vec` growth then frees the buffer the outer frame goes on to index
/// (`self.blocks[idx]`), and `&mut` carries `noalias`, so the outer frame is
/// also entitled to have cached `blocks.ptr`/`len` across the call. Measured on
/// the #7022 reproducer: `install_fresh_block gen=Old space=Old` fires while
/// `Arena::alloc` on the old arena holds the borrow, and `self.blocks`'s length
/// changes underneath it.
///
/// #7019 (default-on evacuating young-gen scavenge) is what made this latent
/// hazard live: before it, a collection triggered from an allocation point did
/// not itself allocate arena blocks anywhere near this often.
///
/// Taking a `*mut Arena` and re-deriving a short-lived `&mut` per statement
/// keeps the two borrows disjoint, so the collector may freely mutate the arena
/// while it runs.
///
/// # Safety
/// `arena` must be the `UnsafeCell` payload of a live thread-local `Arena` for
/// the current thread.
#[inline]
pub(crate) unsafe fn arena_cell_alloc(arena: *mut Arena, size: usize, align: usize) -> *mut u8 {
    // Try current block first, under a borrow that ends with this statement.
    {
        let _borrow = ArenaBorrowGuard::new();
        if let Some(ptr) = (*arena).try_alloc_current(size, align) {
            return ptr;
        }
    }

    // Current block is full. Check the GC trigger first — if it fires and
    // reclaims at least one fully-empty block (via `arena_reset_empty_blocks`),
    // we may be able to reuse that block instead of pushing a new one.
    //
    // Threshold pressure is paid through bounded mutator-assist work. A
    // completed assist cycle may reset blocks before we retry; an incomplete
    // cycle leaves the debt active for later host or allocator steps.
    //
    // NO ARENA BORROW IS LIVE HERE. See the function docs.
    note_gc_trigger_arena_borrow_depth();
    crate::gc::gc_check_trigger();

    {
        let _borrow = ArenaBorrowGuard::new();
        if let Some(ptr) = (*arena).try_alloc_after_gc(size, align) {
            return ptr;
        }
    }

    // A fresh block is needed. `reserve_arena_block` runs an emergency full
    // collection when the OS refuses memory, and that collection allocates into
    // the arenas exactly like the trigger above — so it gets the same treatment:
    // NO ARENA BORROW IS LIVE HERE either. (CodeRabbit caught this second path
    // on the first cut of #7022, where the reservation still happened inside
    // `alloc_fresh_block` under the borrow.)
    let fresh = reserve_arena_block(size);

    let _borrow = ArenaBorrowGuard::new();
    (*arena).install_reserved_block(fresh);
    (*arena)
        .try_alloc_current(size, align)
        .expect("freshly installed block should have space")
}

// ---------------------------------------------------------------------------
// #7022 invariant instrumentation: "no `&mut Arena` borrow is live while the
// allocation-point GC trigger runs".
//
// Compiled ONLY under `cfg(test)`, so production allocation pays nothing. The
// unit tests in `arena::tests` read `gc_trigger_arena_borrow_depth()` after
// forcing an arena allocation past its current block; a refactor that puts the
// trigger back inside the borrow makes them red.
// ---------------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    /// One-shot: make the next *injectable* block allocation report failure, so
    /// a test can drive `reserve_arena_block`'s emergency-reclaim path without
    /// needing the OS to actually refuse memory.
    static FORCE_BLOCK_ALLOC_FAILURE: Cell<bool> = const { Cell::new(false) };
    /// Number of `&mut Arena` borrows currently live inside `arena_cell_alloc`.
    static ARENA_BORROW_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// `ARENA_BORROW_DEPTH` sampled immediately before the most recent
    /// `gc_check_trigger()` call made from `arena_cell_alloc`, and how many
    /// such calls have been made.
    static GC_TRIGGER_BORROW_DEPTH: Cell<u32> = const { Cell::new(u32::MAX) };
    static GC_TRIGGER_CALLS: Cell<u32> = const { Cell::new(0) };
}

/// RAII marker for a live `&mut Arena` borrow (test builds only).
pub(crate) struct ArenaBorrowGuard;

impl ArenaBorrowGuard {
    #[inline(always)]
    fn new() -> Self {
        #[cfg(test)]
        ARENA_BORROW_DEPTH.with(|d| d.set(d.get() + 1));
        ArenaBorrowGuard
    }
}

impl Drop for ArenaBorrowGuard {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(test)]
        ARENA_BORROW_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

#[inline(always)]
fn note_gc_trigger_arena_borrow_depth() {
    #[cfg(test)]
    {
        let depth = ARENA_BORROW_DEPTH.with(Cell::get);
        GC_TRIGGER_BORROW_DEPTH.with(|d| d.set(depth));
        GC_TRIGGER_CALLS.with(|c| c.set(c.get() + 1));
    }
}

/// Arena-borrow depth observed at the most recent allocation-point GC trigger,
/// or `u32::MAX` if no trigger has been reached yet on this thread.
#[cfg(test)]
pub(crate) fn gc_trigger_arena_borrow_depth() -> u32 {
    GC_TRIGGER_BORROW_DEPTH.with(Cell::get)
}

/// How many allocation-point GC triggers `arena_cell_alloc` has reached.
#[cfg(test)]
pub(crate) fn gc_trigger_arena_calls() -> u32 {
    GC_TRIGGER_CALLS.with(Cell::get)
}

/// Arm the one-shot block-allocation failure consumed by
/// `reserve_arena_block`'s first attempt.
#[cfg(test)]
pub(crate) fn force_next_block_alloc_failure() {
    FORCE_BLOCK_ALLOC_FAILURE.with(|f| f.set(true));
}

#[cfg(test)]
pub(crate) fn reset_gc_trigger_arena_probe() {
    FORCE_BLOCK_ALLOC_FAILURE.with(|f| f.set(false));
    GC_TRIGGER_BORROW_DEPTH.with(|d| d.set(u32::MAX));
    GC_TRIGGER_CALLS.with(|c| c.set(0));
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
    pub(crate) static ARENA_TOTAL_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    /// Cached running sum of `block.offset` across the old-gen arena —
    /// the delta-maintained twin of `ARENA_TOTAL_BYTES` above, same
    /// rationale: `gc_budgeted_due_trigger()` reads the old-gen in-use
    /// total on every `gc_check_trigger` (i.e. every `gc_malloc` and
    /// every nursery block fill), and recomputing it walked every
    /// old-arena block each time — a linear-in-old-gen tax on every
    /// malloc-class allocation once the old gen holds real data.
    /// Mutation sites (each MUST maintain the delta):
    ///   - `Arena::try_block_alloc` / `try_block_alloc_excluding_pages`
    ///     (the only offset-advance funnel for `Arena::alloc`,
    ///     `alloc_excluding_pages`, and the fresh-block paths),
    ///   - `reset_region_to_zero` (generation-aware, reset.rs),
    ///   - the old-arena reclaim family in reset.rs
    ///     (`old_arena_reclaim_dead_blocks`,
    ///     `old_arena_reclaim_selected_dead_blocks`,
    ///     `OldArenaReclaimDeadBlocksState::process_block`) which zero
    ///     old block offsets on sweep/defrag.
    /// Block install/dealloc paths don't touch it: fresh blocks start
    /// at offset 0 and blocks are only deallocated after their offset
    /// was already zeroed. `old_gen_in_use_bytes()` (stats.rs)
    /// debug-asserts this cache against the O(blocks) recompute so a
    /// missed mutation site fails tests instead of silently skewing
    /// the OldReclaim trigger.
    pub(crate) static OLD_GEN_IN_USE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    pub(crate) static ARENA: UnsafeCell<Arena> =
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
    pub(crate) static LONGLIVED_ARENA: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new_lazy(HeapGeneration::Longlived, HeapSpace::Longlived));

    /// Copying nursery survivor semispaces. At most one is the active
    /// from-space at the start of a copying minor GC; the other is reset
    /// and used as to-space for fresh Eden survivors.
    pub(crate) static SURVIVOR_ARENA_0: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new_lazy(HeapGeneration::Nursery, HeapSpace::Survivor0));
    pub(crate) static SURVIVOR_ARENA_1: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new_lazy(HeapGeneration::Nursery, HeapSpace::Survivor1));
    pub(crate) static ACTIVE_SURVIVOR: Cell<usize> = const { Cell::new(0) };

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
    /// (same lifetime contract as longlived blocks from the nursery
    /// reset path), and never feed the inline bump allocator. Full
    /// mark-sweep can reclaim completely dead old blocks through the
    /// dedicated old-arena reset/deallocation path.
    pub(crate) static OLD_ARENA: UnsafeCell<Arena> =
        UnsafeCell::new(Arena::new_lazy(HeapGeneration::Old, HeapSpace::Old));

    /// Inline allocator state — a cache of the current arena block's
    /// `(data, offset, size)` triple, exposed via a stable pointer so
    /// codegen can emit inline bump-allocate IR without going through
    /// any function call or `LocalKey::with` wrapper.
    ///
    /// `#[repr(C)]` on `InlineArenaState` keeps the field offsets stable
    /// (data=0, offset=8, size=16). The codegen reads/writes these fields
    /// directly via fixed GEPs, so changing the struct layout would
    /// silently break every emitted `new ClassName()`.
    pub(crate) static INLINE_STATE: UnsafeCell<InlineArenaState> = const { UnsafeCell::new(InlineArenaState {
        data: std::ptr::null_mut(),
        offset: 0,
        size: 0,
    }) };
}

/// Delta-maintenance for `OLD_GEN_IN_USE_BYTES` — see the thread-local's
/// doc comment for the full mutation-site inventory.
#[inline]
pub(crate) fn old_gen_in_use_bytes_add(delta: usize) {
    if delta == 0 {
        return;
    }
    OLD_GEN_IN_USE_BYTES.with(|c| c.set(c.get().saturating_add(delta)));
}

#[inline]
pub(crate) fn old_gen_in_use_bytes_sub(delta: usize) {
    if delta == 0 {
        return;
    }
    OLD_GEN_IN_USE_BYTES.with(|c| c.set(c.get().saturating_sub(delta)));
}
