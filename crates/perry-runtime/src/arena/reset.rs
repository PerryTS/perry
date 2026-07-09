use super::*;

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

fn reset_region_to_zero(arena: &mut Arena) -> (usize, usize) {
    let mut reset_blocks = 0usize;
    let mut reusable_bytes = 0usize;
    for block in arena.blocks.iter_mut() {
        if block.data.is_null() {
            continue;
        }
        if block.offset != 0 {
            reset_blocks += 1;
            reusable_bytes = reusable_bytes.saturating_add(block.offset);
        }
        block.offset = 0;
        block.dead_cycles = 0;
    }
    // Delta-maintain the cached old-gen in-use counter. Only Eden and
    // the survivor semispaces are reset through here today, but this
    // takes any `&mut Arena` — keep the counter honest if an old-gen
    // caller ever appears.
    if arena.generation == HeapGeneration::Old {
        old_gen_in_use_bytes_sub(reusable_bytes);
    }
    arena.current = 0;
    (reset_blocks, reusable_bytes)
}

/// Reset the inactive survivor semispace before a copying minor starts.
pub(crate) fn copying_prepare_to_space() -> usize {
    let idx = inactive_survivor_index();
    with_survivor_arena_mut(idx, reset_region_to_zero).0
}

/// Bytes currently allocated in the active survivor from-space.
pub(crate) fn copying_active_survivor_in_use_bytes() -> usize {
    let active = ACTIVE_SURVIVOR.with(|active| active.get());
    with_survivor_arena(active, |arena| {
        arena.blocks.iter().map(|b| b.offset).sum::<usize>()
    })
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

pub(crate) fn active_survivor_block_index_range() -> std::ops::Range<usize> {
    let general_n = ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor0_n = SURVIVOR_ARENA_0.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor1_n = SURVIVOR_ARENA_1.with(|a| unsafe { (*a.get()).blocks.len() });
    match ACTIVE_SURVIVOR.with(|active| active.get()) {
        0 => general_n..general_n + survivor0_n,
        1 => general_n + survivor0_n..general_n + survivor0_n + survivor1_n,
        _ => general_n..general_n,
    }
}

/// Reset Eden and the active survivor from-space, then flip the survivor
/// roles so the to-space populated by the copying collector becomes active.
pub(crate) fn copying_reset_from_spaces_and_flip() -> ArenaResetStats {
    sync_inline_arena_state();
    let mut reset_blocks = 0usize;
    let mut reusable_bytes = 0usize;
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        let (blocks, bytes) = reset_region_to_zero(arena);
        reset_blocks += blocks;
        reusable_bytes = reusable_bytes.saturating_add(bytes);
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
    let (blocks, bytes) = with_survivor_arena_mut(active, reset_region_to_zero);
    reset_blocks += blocks;
    reusable_bytes = reusable_bytes.saturating_add(bytes);
    ACTIVE_SURVIVOR.with(|active_cell| active_cell.set(1 - active));

    ArenaResetStats {
        reset_blocks,
        reusable_bytes,
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
        let mut reset_block_ranges: Vec<(usize, usize, usize)> = Vec::new();
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
            reset_block_ranges.push((block.data as usize, block.size, block.offset));
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
                        .any(|&(base, size, _)| p >= base && p < base + size)
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
                // #4665: in test builds keep freed blocks mapped (no munmap) so
                // unit tests holding raw GC pointers across a collection read stale
                // bytes instead of SIGSEGV-ing on an unmapped page.
                if !cfg!(test) {
                    std::alloc::dealloc(block.data, layout);
                }
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
        let reusable_bytes: usize = reset_block_ranges
            .iter()
            .filter(|&&(base, _, _)| {
                !deallocated_ranges
                    .iter()
                    .any(|&(deallocated_base, _)| deallocated_base == base)
            })
            .map(|&(_, _, used)| used)
            .sum();
        let stats = ArenaResetStats {
            reset_blocks,
            reusable_bytes,
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

const GENERAL_DEALLOC_DEAD_CYCLES: u32 = 2;

fn filter_free_list_ranges(ranges: &[(usize, usize)]) {
    if ranges.is_empty() {
        return;
    }
    crate::gc::ARENA_FREE_LIST.with(|fl| {
        let mut fl = fl.borrow_mut();
        fl.retain(|&(ptr, _)| {
            let p = ptr as usize;
            !ranges
                .iter()
                .any(|&(base, size)| p >= base && p < base.saturating_add(size))
        });
        if fl.is_empty() {
            crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
        }
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeneralResetSubphase {
    Reset,
    Deallocate,
    Finish,
    Done,
}

pub(crate) struct ArenaResetEmptyBlocksState {
    block_has_live: Vec<bool>,
    snapshots: Vec<ArenaBlockSnapshot>,
    subphase: GeneralResetSubphase,
    cursor: usize,
    changed: bool,
    reset_ranges: Vec<(usize, usize, usize)>,
    deallocated_ranges: Vec<(usize, usize)>,
    stats: ArenaResetStats,
}

impl ArenaResetEmptyBlocksState {
    pub(crate) fn new(block_has_live: &[bool], snapshots: &[ArenaBlockSnapshot]) -> Self {
        Self {
            block_has_live: block_has_live.to_vec(),
            snapshots: snapshots.to_vec(),
            subphase: GeneralResetSubphase::Reset,
            cursor: 0,
            changed: false,
            reset_ranges: Vec::new(),
            deallocated_ranges: Vec::new(),
            stats: ArenaResetStats::default(),
        }
    }

    pub(crate) fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        let mut free_list_ranges = Vec::new();

        while remaining > 0 {
            match self.subphase {
                GeneralResetSubphase::Reset => {
                    let general_n = ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
                    if self.cursor >= general_n {
                        self.cursor = 0;
                        self.subphase = GeneralResetSubphase::Deallocate;
                        continue;
                    }
                    if let Some((base, size, used)) = self.process_reset_block(self.cursor) {
                        self.reset_ranges.push((base, size, used));
                        free_list_ranges.push((base, size));
                    }
                    self.cursor += 1;
                    remaining -= 1;
                }
                GeneralResetSubphase::Deallocate => {
                    let general_n = ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
                    if self.cursor >= general_n {
                        self.cursor = 0;
                        self.subphase = GeneralResetSubphase::Finish;
                        continue;
                    }
                    if let Some((base, size)) = self.process_dealloc_block(self.cursor) {
                        self.deallocated_ranges.push((base, size));
                        free_list_ranges.push((base, size));
                    }
                    self.cursor += 1;
                    remaining -= 1;
                }
                GeneralResetSubphase::Finish => {
                    self.finish();
                    self.subphase = GeneralResetSubphase::Done;
                    break;
                }
                GeneralResetSubphase::Done => break,
            }
        }

        filter_free_list_ranges(&free_list_ranges);
        self.subphase == GeneralResetSubphase::Done
    }

    pub(crate) fn stats(&self) -> ArenaResetStats {
        self.stats
    }

    fn process_reset_block(&mut self, block_idx: usize) -> Option<(usize, usize, usize)> {
        let snapshot = self.snapshots.get(block_idx).copied().unwrap_or_default();
        if snapshot.data == 0 {
            return None;
        }

        ARENA.with(|arena| unsafe {
            let arena = &mut *arena.get();
            let current = arena.current;
            let keep_low = current.saturating_sub(4);
            let block = arena.blocks.get_mut(block_idx)?;
            if block.data.is_null()
                || block.data as usize != snapshot.data
                || block.size != snapshot.size
                || block.offset != snapshot.offset
            {
                return None;
            }
            if block.offset == 0
                || self.block_has_live.get(block_idx).copied().unwrap_or(false)
                || (block_idx >= keep_low && block_idx <= current)
            {
                return None;
            }

            let base = block.data as usize;
            let size = block.size;
            let used = block.offset;
            block.offset = 0;
            self.changed = true;
            Some((base, size, used))
        })
    }

    fn process_dealloc_block(&mut self, block_idx: usize) -> Option<(usize, usize)> {
        let snapshot = self.snapshots.get(block_idx).copied().unwrap_or_default();
        if snapshot.data == 0 {
            return None;
        }

        ARENA.with(|arena| unsafe {
            let arena = &mut *arena.get();
            let current = arena.current;
            let keep_low = current.saturating_sub(4);
            let block = arena.blocks.get_mut(block_idx)?;
            if block.data.is_null()
                || block.data as usize != snapshot.data
                || block.size != snapshot.size
            {
                return None;
            }
            if block_idx == current || (block_idx >= keep_low && block_idx <= current) {
                block.dead_cycles = 0;
                return None;
            }
            if self.block_has_live.get(block_idx).copied().unwrap_or(false) {
                block.dead_cycles = 0;
                return None;
            }
            if block.offset != 0 {
                block.dead_cycles = 0;
                return None;
            }

            block.dead_cycles = block.dead_cycles.saturating_add(1);
            if block.dead_cycles < GENERAL_DEALLOC_DEAD_CYCLES {
                return None;
            }

            let base = block.data as usize;
            let size = block.size;
            let layout = Layout::from_size_align(block.size, 16).unwrap();
            unregister_block_generation(base, size);
            // #4665: in test builds keep freed blocks mapped (no munmap) so
            // unit tests holding raw GC pointers across a collection read stale
            // bytes instead of SIGSEGV-ing on an unmapped page.
            if !cfg!(test) {
                std::alloc::dealloc(block.data, layout);
            }
            ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
            block.data = std::ptr::null_mut();
            block.size = 0;
            block.offset = 0;
            block.dead_cycles = 0;
            self.changed = true;
            Some((base, size))
        })
    }

    fn finish(&mut self) {
        let deallocated_blocks = self.deallocated_ranges.len();
        let deallocated_bytes: usize = self.deallocated_ranges.iter().map(|&(_, size)| size).sum();
        let reusable_bytes: usize = self
            .reset_ranges
            .iter()
            .filter(|&&(base, _, _)| {
                !self
                    .deallocated_ranges
                    .iter()
                    .any(|&(deallocated_base, _)| deallocated_base == base)
            })
            .map(|&(_, _, used)| used)
            .sum();

        self.stats = ArenaResetStats {
            reset_blocks: self.reset_ranges.len(),
            reusable_bytes,
            deallocated_blocks,
            deallocated_bytes,
        };

        if !self.changed {
            return;
        }

        ARENA.with(|arena| unsafe {
            let arena = &mut *arena.get();
            let mut new_current = arena.current;
            for (i, block) in arena.blocks.iter().enumerate() {
                if !block.data.is_null() && block.offset == 0 {
                    new_current = i;
                    break;
                }
            }
            if arena
                .blocks
                .get(new_current)
                .map(|block| !block.data.is_null())
                .unwrap_or(false)
            {
                arena.current = new_current;
            }
            INLINE_STATE.with(|s| {
                let inline = &mut *s.get();
                if !inline.data.is_null() {
                    if let Some(block) = arena.blocks.get(arena.current) {
                        if !block.data.is_null() {
                            inline.data = block.data;
                            inline.offset = block.offset;
                            inline.size = block.size;
                        }
                    }
                }
            });
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegionReclaimSubphase {
    Reclaim,
    Finish,
    Done,
}

struct SurvivorArenaReclaimState {
    arena_idx: usize,
    block_start: usize,
    block_has_live: Vec<bool>,
    snapshots: Vec<ArenaBlockSnapshot>,
    cursor: usize,
    subphase: RegionReclaimSubphase,
    changed: bool,
    stats: ArenaResetStats,
}

impl SurvivorArenaReclaimState {
    fn new(
        arena_idx: usize,
        block_start: usize,
        block_has_live: &[bool],
        snapshots: &[ArenaBlockSnapshot],
    ) -> Self {
        Self {
            arena_idx,
            block_start,
            block_has_live: block_has_live.to_vec(),
            snapshots: snapshots.to_vec(),
            cursor: 0,
            subphase: RegionReclaimSubphase::Reclaim,
            changed: false,
            stats: ArenaResetStats::default(),
        }
    }

    fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        while remaining > 0 {
            match self.subphase {
                RegionReclaimSubphase::Reclaim => {
                    let block_count =
                        with_survivor_arena(self.arena_idx, |arena| arena.blocks.len());
                    if self.cursor >= block_count {
                        self.subphase = RegionReclaimSubphase::Finish;
                        continue;
                    }
                    self.process_block(self.cursor);
                    self.cursor += 1;
                    remaining -= 1;
                }
                RegionReclaimSubphase::Finish => {
                    self.finish();
                    self.subphase = RegionReclaimSubphase::Done;
                    break;
                }
                RegionReclaimSubphase::Done => break,
            }
        }
        self.subphase == RegionReclaimSubphase::Done
    }

    fn process_block(&mut self, local_idx: usize) {
        let global_idx = self.block_start + local_idx;
        let snapshot = self.snapshots.get(global_idx).copied().unwrap_or_default();
        if snapshot.data == 0 {
            return;
        }

        with_survivor_arena_mut(self.arena_idx, |arena| unsafe {
            let keep_idx = arena
                .blocks
                .get(arena.current)
                .filter(|block| !block.data.is_null())
                .map(|_| arena.current)
                .or_else(|| {
                    arena
                        .blocks
                        .iter()
                        .enumerate()
                        .find(|(_, block)| !block.data.is_null())
                        .map(|(i, _)| i)
                });
            let Some(block) = arena.blocks.get_mut(local_idx) else {
                return;
            };
            if block.data.is_null()
                || block.data as usize != snapshot.data
                || block.size != snapshot.size
                || block.offset != snapshot.offset
            {
                return;
            }
            if self
                .block_has_live
                .get(global_idx)
                .copied()
                .unwrap_or(false)
            {
                block.dead_cycles = 0;
                return;
            }

            let used = block.offset;
            if used != 0 {
                self.stats.reset_blocks = self.stats.reset_blocks.saturating_add(1);
            }
            block.offset = 0;
            block.dead_cycles = 0;
            self.changed = true;

            if Some(local_idx) == keep_idx {
                self.stats.reusable_bytes = self.stats.reusable_bytes.saturating_add(used);
                return;
            }

            let base = block.data as usize;
            let size = block.size;
            let layout = Layout::from_size_align(size, 16).unwrap();
            unregister_block_generation(base, size);
            // #4665: in test builds keep freed blocks mapped (no munmap) so
            // unit tests holding raw GC pointers across a collection read stale
            // bytes instead of SIGSEGV-ing on an unmapped page.
            if !cfg!(test) {
                std::alloc::dealloc(block.data, layout);
            }
            ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
            block.data = std::ptr::null_mut();
            block.size = 0;
            block.offset = 0;
            block.dead_cycles = 0;
            self.stats.deallocated_blocks = self.stats.deallocated_blocks.saturating_add(1);
            self.stats.deallocated_bytes = self.stats.deallocated_bytes.saturating_add(size);
        });
    }

    fn finish(&mut self) {
        if !self.changed {
            return;
        }
        with_survivor_arena_mut(self.arena_idx, |arena| {
            if let Some((idx, _)) = arena
                .blocks
                .iter()
                .enumerate()
                .find(|(_, block)| !block.data.is_null() && block.offset == 0)
            {
                arena.current = idx;
            } else if arena
                .blocks
                .get(arena.current)
                .map(|block| block.data.is_null())
                .unwrap_or(true)
            {
                if let Some((idx, _)) = arena
                    .blocks
                    .iter()
                    .enumerate()
                    .find(|(_, block)| !block.data.is_null())
                {
                    arena.current = idx;
                }
            }
        });
    }
}

pub(crate) struct SurvivorArenaReclaimDeadBlocksState {
    state0: SurvivorArenaReclaimState,
    state1: SurvivorArenaReclaimState,
    active: usize,
    stats: ArenaResetStats,
}

impl SurvivorArenaReclaimDeadBlocksState {
    pub(crate) fn new(block_has_live: &[bool], snapshots: &[ArenaBlockSnapshot]) -> Self {
        let general_n = ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
        let survivor0_n = SURVIVOR_ARENA_0.with(|a| unsafe { (*a.get()).blocks.len() });
        Self {
            state0: SurvivorArenaReclaimState::new(0, general_n, block_has_live, snapshots),
            state1: SurvivorArenaReclaimState::new(
                1,
                general_n + survivor0_n,
                block_has_live,
                snapshots,
            ),
            active: 0,
            stats: ArenaResetStats::default(),
        }
    }

    pub(crate) fn step(&mut self, budget: usize) -> bool {
        if budget == 0 {
            return false;
        }
        match self.active {
            0 => {
                let before = self.state0.stats;
                if self.state0.step(budget) {
                    self.stats = self.add_delta(self.stats, before, self.state0.stats);
                    self.active = 1;
                }
                false
            }
            1 => {
                let before = self.state1.stats;
                if self.state1.step(budget) {
                    self.stats = self.add_delta(self.stats, before, self.state1.stats);
                    self.active = 2;
                    return true;
                }
                false
            }
            _ => true,
        }
    }

    pub(crate) fn stats(&self) -> ArenaResetStats {
        self.stats
    }

    fn add_delta(
        &self,
        mut total: ArenaResetStats,
        before: ArenaResetStats,
        after: ArenaResetStats,
    ) -> ArenaResetStats {
        total.reset_blocks = total
            .reset_blocks
            .saturating_add(after.reset_blocks.saturating_sub(before.reset_blocks));
        total.reusable_bytes = total
            .reusable_bytes
            .saturating_add(after.reusable_bytes.saturating_sub(before.reusable_bytes));
        total.deallocated_blocks = total.deallocated_blocks.saturating_add(
            after
                .deallocated_blocks
                .saturating_sub(before.deallocated_blocks),
        );
        total.deallocated_bytes = total.deallocated_bytes.saturating_add(
            after
                .deallocated_bytes
                .saturating_sub(before.deallocated_bytes),
        );
        total
    }
}

pub(crate) struct OldArenaReclaimDeadBlocksState {
    block_has_live: Vec<bool>,
    snapshots: Vec<ArenaBlockSnapshot>,
    selected_old_blocks: Option<crate::fast_hash::PtrHashSet<usize>>,
    cursor: usize,
    subphase: RegionReclaimSubphase,
    changed: bool,
    stats: ArenaResetStats,
}

impl OldArenaReclaimDeadBlocksState {
    pub(crate) fn new_full(block_has_live: &[bool], snapshots: &[ArenaBlockSnapshot]) -> Self {
        Self::new(block_has_live, snapshots, None)
    }

    pub(crate) fn new_selected(
        block_has_live: &[bool],
        snapshots: &[ArenaBlockSnapshot],
        selected_old_blocks: &crate::fast_hash::PtrHashSet<usize>,
    ) -> Self {
        Self::new(block_has_live, snapshots, Some(selected_old_blocks.clone()))
    }

    fn new(
        block_has_live: &[bool],
        snapshots: &[ArenaBlockSnapshot],
        selected_old_blocks: Option<crate::fast_hash::PtrHashSet<usize>>,
    ) -> Self {
        Self {
            block_has_live: block_has_live.to_vec(),
            snapshots: snapshots.to_vec(),
            selected_old_blocks,
            cursor: 0,
            subphase: RegionReclaimSubphase::Reclaim,
            changed: false,
            stats: ArenaResetStats::default(),
        }
    }

    pub(crate) fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        while remaining > 0 {
            match self.subphase {
                RegionReclaimSubphase::Reclaim => {
                    let block_count =
                        OLD_ARENA.with(|arena| unsafe { (*arena.get()).blocks.len() });
                    if self.cursor >= block_count {
                        self.subphase = RegionReclaimSubphase::Finish;
                        continue;
                    }
                    self.process_block(self.cursor);
                    self.cursor += 1;
                    remaining -= 1;
                }
                RegionReclaimSubphase::Finish => {
                    self.finish();
                    OLD_GEN_RECLAIM_REUSABLE_BYTES
                        .with(|bytes| bytes.set(self.stats.reusable_bytes));
                    OLD_GEN_RECLAIM_RETURNED_BYTES
                        .with(|bytes| bytes.set(self.stats.deallocated_bytes));
                    self.subphase = RegionReclaimSubphase::Done;
                    break;
                }
                RegionReclaimSubphase::Done => break,
            }
        }
        self.subphase == RegionReclaimSubphase::Done
    }

    pub(crate) fn stats(&self) -> ArenaResetStats {
        self.stats
    }

    fn process_block(&mut self, local_idx: usize) {
        let old_block_start = longlived_end();
        let block_idx = old_block_start + local_idx;
        if self
            .selected_old_blocks
            .as_ref()
            .is_some_and(|selected| !selected.contains(&block_idx))
        {
            return;
        }

        let snapshot = self.snapshots.get(block_idx).copied().unwrap_or_default();
        if snapshot.data == 0 {
            return;
        }

        OLD_ARENA.with(|arena| unsafe {
            let arena = &mut *arena.get();
            let original_current = arena.current;
            let Some(block) = arena.blocks.get_mut(local_idx) else {
                return;
            };
            if block.data.is_null()
                || block.data as usize != snapshot.data
                || block.size != snapshot.size
                || block.offset != snapshot.offset
            {
                return;
            }
            if self.block_has_live.get(block_idx).copied().unwrap_or(false) {
                block.dead_cycles = 0;
                return;
            }

            let base = block.data as usize;
            let size = block.size;
            let used = block.offset;
            let first_page = generation_page_for_addr(base);
            let last_page = generation_page_for_addr(base + size - 1);
            let pages: Vec<usize> = (first_page..=last_page).collect();
            unregister_old_block_pages(&pages);

            if used != 0 {
                self.stats.reset_blocks = self.stats.reset_blocks.saturating_add(1);
            }
            block.offset = 0;
            block.dead_cycles = 0;
            old_gen_in_use_bytes_sub(used);
            self.changed = true;

            if local_idx == original_current {
                self.stats.reusable_bytes = self.stats.reusable_bytes.saturating_add(used);
                return;
            }

            let layout = Layout::from_size_align(size, 16).unwrap();
            unregister_block_generation(base, size);
            // #4665: in test builds keep freed blocks mapped (no munmap) so
            // unit tests holding raw GC pointers across a collection read stale
            // bytes instead of SIGSEGV-ing on an unmapped page.
            if !cfg!(test) {
                std::alloc::dealloc(block.data, layout);
            }
            ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
            block.data = std::ptr::null_mut();
            block.size = 0;
            block.offset = 0;
            block.dead_cycles = 0;
            self.stats.deallocated_blocks = self.stats.deallocated_blocks.saturating_add(1);
            self.stats.deallocated_bytes = self.stats.deallocated_bytes.saturating_add(size);
        });
    }

    fn finish(&mut self) {
        if !self.changed {
            return;
        }
        OLD_ARENA.with(|arena| unsafe {
            let arena = &mut *arena.get();
            if let Some((idx, _)) = arena
                .blocks
                .iter()
                .enumerate()
                .find(|(_, block)| !block.data.is_null() && block.offset == 0)
            {
                arena.current = idx;
            } else if arena
                .blocks
                .get(arena.current)
                .map(|block| block.data.is_null())
                .unwrap_or(true)
            {
                if let Some((idx, _)) = arena
                    .blocks
                    .iter()
                    .enumerate()
                    .find(|(_, block)| !block.data.is_null())
                {
                    arena.current = idx;
                }
            }
        });
    }
}

pub(crate) fn old_arena_reclaim_dead_blocks(block_has_live: &[bool]) -> ArenaResetStats {
    let old_block_start = longlived_end();
    let stats = OLD_ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        let original_current = arena.current;
        let mut stats = ArenaResetStats::default();
        let mut changed = false;

        for (i, block) in arena.blocks.iter_mut().enumerate() {
            if block.data.is_null() {
                continue;
            }

            let block_idx = old_block_start + i;
            if block_has_live.get(block_idx).copied().unwrap_or(false) {
                block.dead_cycles = 0;
                continue;
            }

            let base = block.data as usize;
            let size = block.size;
            let used = block.offset;
            let first_page = generation_page_for_addr(base);
            let last_page = generation_page_for_addr(base + size - 1);
            let pages: Vec<usize> = (first_page..=last_page).collect();
            unregister_old_block_pages(&pages);

            if used != 0 {
                stats.reset_blocks = stats.reset_blocks.saturating_add(1);
            }
            block.offset = 0;
            block.dead_cycles = 0;
            old_gen_in_use_bytes_sub(used);
            changed = true;

            // Keep the current old allocation target mapped and reusable.
            // Arena::alloc assumes `current` points at a non-tombstone block.
            if i == original_current {
                stats.reusable_bytes = stats.reusable_bytes.saturating_add(used);
                continue;
            }

            let layout = Layout::from_size_align(size, 16).unwrap();
            unregister_block_generation(base, size);
            // #4665: in test builds keep freed blocks mapped (no munmap) so
            // unit tests holding raw GC pointers across a collection read stale
            // bytes instead of SIGSEGV-ing on an unmapped page.
            if !cfg!(test) {
                std::alloc::dealloc(block.data, layout);
            }
            ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
            block.data = std::ptr::null_mut();
            block.size = 0;
            block.offset = 0;
            block.dead_cycles = 0;
            stats.deallocated_blocks = stats.deallocated_blocks.saturating_add(1);
            stats.deallocated_bytes = stats.deallocated_bytes.saturating_add(size);
        }

        if changed {
            if let Some((idx, _)) = arena
                .blocks
                .iter()
                .enumerate()
                .find(|(_, block)| !block.data.is_null() && block.offset == 0)
            {
                arena.current = idx;
            } else if arena
                .blocks
                .get(arena.current)
                .map(|block| block.data.is_null())
                .unwrap_or(true)
            {
                if let Some((idx, _)) = arena
                    .blocks
                    .iter()
                    .enumerate()
                    .find(|(_, block)| !block.data.is_null())
                {
                    arena.current = idx;
                }
            }
        }

        stats
    });

    OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.set(stats.reusable_bytes));
    OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.set(stats.deallocated_bytes));
    stats
}

pub(crate) fn old_arena_reclaim_selected_dead_blocks(
    block_has_live: &[bool],
    selected_old_blocks: &crate::fast_hash::PtrHashSet<usize>,
) -> ArenaResetStats {
    if selected_old_blocks.is_empty() {
        return ArenaResetStats::default();
    }

    let old_block_start = longlived_end();
    let stats = OLD_ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        let original_current = arena.current;
        let mut stats = ArenaResetStats::default();
        let mut changed = false;

        for (i, block) in arena.blocks.iter_mut().enumerate() {
            if block.data.is_null() {
                continue;
            }

            let block_idx = old_block_start + i;
            if !selected_old_blocks.contains(&block_idx) {
                continue;
            }
            if block_has_live.get(block_idx).copied().unwrap_or(false) {
                block.dead_cycles = 0;
                continue;
            }

            let base = block.data as usize;
            let size = block.size;
            let used = block.offset;
            let first_page = generation_page_for_addr(base);
            let last_page = generation_page_for_addr(base + size - 1);
            let pages: Vec<usize> = (first_page..=last_page).collect();
            unregister_old_block_pages(&pages);

            if used != 0 {
                stats.reset_blocks = stats.reset_blocks.saturating_add(1);
            }
            block.offset = 0;
            block.dead_cycles = 0;
            old_gen_in_use_bytes_sub(used);
            changed = true;

            if i == original_current {
                stats.reusable_bytes = stats.reusable_bytes.saturating_add(used);
                continue;
            }

            let layout = Layout::from_size_align(size, 16).unwrap();
            unregister_block_generation(base, size);
            // #4665: in test builds keep freed blocks mapped (no munmap) so
            // unit tests holding raw GC pointers across a collection read stale
            // bytes instead of SIGSEGV-ing on an unmapped page.
            if !cfg!(test) {
                std::alloc::dealloc(block.data, layout);
            }
            ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
            block.data = std::ptr::null_mut();
            block.size = 0;
            block.offset = 0;
            block.dead_cycles = 0;
            stats.deallocated_blocks = stats.deallocated_blocks.saturating_add(1);
            stats.deallocated_bytes = stats.deallocated_bytes.saturating_add(size);
        }

        if changed {
            if let Some((idx, _)) = arena
                .blocks
                .iter()
                .enumerate()
                .find(|(_, block)| !block.data.is_null() && block.offset == 0)
            {
                arena.current = idx;
            } else if arena
                .blocks
                .get(arena.current)
                .map(|block| block.data.is_null())
                .unwrap_or(true)
            {
                if let Some((idx, _)) = arena
                    .blocks
                    .iter()
                    .enumerate()
                    .find(|(_, block)| !block.data.is_null())
                {
                    arena.current = idx;
                }
            }
        }

        stats
    });

    OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.set(stats.reusable_bytes));
    OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.set(stats.deallocated_bytes));
    stats
}

fn reclaim_dead_survivor_arena_blocks(
    arena_idx: usize,
    block_start: usize,
    block_has_live: &[bool],
) -> ArenaResetStats {
    with_survivor_arena_mut(arena_idx, |arena| unsafe {
        let keep_idx = arena
            .blocks
            .get(arena.current)
            .filter(|block| !block.data.is_null())
            .map(|_| arena.current)
            .or_else(|| {
                arena
                    .blocks
                    .iter()
                    .enumerate()
                    .find(|(_, block)| !block.data.is_null())
                    .map(|(i, _)| i)
            });
        let mut stats = ArenaResetStats::default();
        let mut changed = false;

        for (i, block) in arena.blocks.iter_mut().enumerate() {
            if block.data.is_null() {
                continue;
            }

            let block_idx = block_start + i;
            if block_has_live.get(block_idx).copied().unwrap_or(false) {
                block.dead_cycles = 0;
                continue;
            }

            let used = block.offset;
            if used != 0 {
                stats.reset_blocks = stats.reset_blocks.saturating_add(1);
            }
            block.offset = 0;
            block.dead_cycles = 0;
            changed = true;

            // Keep one allocation target per survivor semispace mapped so
            // Arena::alloc never observes a tombstoned current block.
            if Some(i) == keep_idx {
                stats.reusable_bytes = stats.reusable_bytes.saturating_add(used);
                continue;
            }

            let base = block.data as usize;
            let size = block.size;
            let layout = Layout::from_size_align(size, 16).unwrap();
            unregister_block_generation(base, size);
            // #4665: in test builds keep freed blocks mapped (no munmap) so
            // unit tests holding raw GC pointers across a collection read stale
            // bytes instead of SIGSEGV-ing on an unmapped page.
            if !cfg!(test) {
                std::alloc::dealloc(block.data, layout);
            }
            ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
            block.data = std::ptr::null_mut();
            block.size = 0;
            block.offset = 0;
            block.dead_cycles = 0;
            stats.deallocated_blocks = stats.deallocated_blocks.saturating_add(1);
            stats.deallocated_bytes = stats.deallocated_bytes.saturating_add(size);
        }

        if changed {
            if let Some((idx, _)) = arena
                .blocks
                .iter()
                .enumerate()
                .find(|(_, block)| !block.data.is_null() && block.offset == 0)
            {
                arena.current = idx;
            } else if arena
                .blocks
                .get(arena.current)
                .map(|block| block.data.is_null())
                .unwrap_or(true)
            {
                if let Some((idx, _)) = arena
                    .blocks
                    .iter()
                    .enumerate()
                    .find(|(_, block)| !block.data.is_null())
                {
                    arena.current = idx;
                }
            }
        }

        stats
    })
}

pub(crate) fn survivor_arena_reclaim_dead_blocks(block_has_live: &[bool]) -> ArenaResetStats {
    let general_n = ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
    let survivor0_n = SURVIVOR_ARENA_0.with(|a| unsafe { (*a.get()).blocks.len() });
    let stats0 = reclaim_dead_survivor_arena_blocks(0, general_n, block_has_live);
    let stats1 = reclaim_dead_survivor_arena_blocks(1, general_n + survivor0_n, block_has_live);
    ArenaResetStats {
        reset_blocks: stats0.reset_blocks.saturating_add(stats1.reset_blocks),
        reusable_bytes: stats0.reusable_bytes.saturating_add(stats1.reusable_bytes),
        deallocated_blocks: stats0
            .deallocated_blocks
            .saturating_add(stats1.deallocated_blocks),
        deallocated_bytes: stats0
            .deallocated_bytes
            .saturating_add(stats1.deallocated_bytes),
    }
}
