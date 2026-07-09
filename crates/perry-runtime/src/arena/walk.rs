use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArenaWalkOrder {
    BlockIndex,
    Address,
}

#[derive(Clone, Copy)]
struct ArenaWalkBlock {
    data: usize,
    offset: usize,
    size: usize,
    block_idx: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ArenaBlockSnapshot {
    pub(crate) data: usize,
    pub(crate) offset: usize,
    pub(crate) size: usize,
}

/// Resumable arena object walker used by the GC cycle state machine.
///
/// The cursor owns block base pointers and offsets gathered by
/// `ArenaObjectCursorBuilder`, then yields at most one walkable GC object per
/// `next()` call. It deliberately does not hold a borrow into any arena TLS
/// slot across calls.
pub(crate) struct ArenaObjectCursor {
    blocks: ArenaObjectCursorBlocks,
    current_block: Option<ArenaWalkBlock>,
    block_pos: usize,
    offset: usize,
    finished: bool,
}

enum ArenaObjectCursorBlocks {
    BlockIndex(Vec<ArenaWalkBlock>),
    Address(std::collections::btree_map::IntoValues<usize, ArenaWalkBlock>),
}

pub(crate) struct ArenaObjectCursorBuilder {
    order: ArenaWalkOrder,
    blocks: Vec<ArenaWalkBlock>,
    address_blocks: std::collections::BTreeMap<usize, ArenaWalkBlock>,
    initialized: bool,
    region_lengths: [usize; ARENA_CURSOR_REGION_COUNT],
    region_bases: [usize; ARENA_CURSOR_REGION_COUNT],
    region: usize,
    block_pos: usize,
    inspected_blocks: usize,
}

const ARENA_CURSOR_REGION_COUNT: usize = 5;
const ARENA_CURSOR_GENERAL: usize = 0;
const ARENA_CURSOR_SURVIVOR0: usize = 1;
const ARENA_CURSOR_SURVIVOR1: usize = 2;
const ARENA_CURSOR_LONGLIVED: usize = 3;
const ARENA_CURSOR_OLD: usize = 4;

impl ArenaObjectCursorBuilder {
    pub(crate) fn new(order: ArenaWalkOrder) -> Self {
        Self {
            order,
            blocks: Vec::new(),
            address_blocks: std::collections::BTreeMap::new(),
            initialized: false,
            region_lengths: [0; ARENA_CURSOR_REGION_COUNT],
            region_bases: [0; ARENA_CURSOR_REGION_COUNT],
            region: 0,
            block_pos: 0,
            inspected_blocks: 0,
        }
    }

    pub(crate) fn step(&mut self, remaining: &mut usize) -> Option<ArenaObjectCursor> {
        if *remaining == 0 {
            return None;
        }
        if !self.initialized {
            self.initialize();
        }

        while *remaining > 0 && self.region < ARENA_CURSOR_REGION_COUNT {
            let region = self.region;
            let block_pos = self.block_pos;
            let block_idx = self.region_bases[region] + block_pos;

            self.block_pos += 1;
            self.inspected_blocks = self.inspected_blocks.saturating_add(1);
            *remaining -= 1;

            if let Some(block) = snapshot_cursor_block(region, block_pos, block_idx) {
                self.push_block(block);
            }

            while self.region < ARENA_CURSOR_REGION_COUNT
                && self.block_pos >= self.region_lengths[self.region]
            {
                self.region += 1;
                self.block_pos = 0;
            }
        }

        if self.region >= ARENA_CURSOR_REGION_COUNT {
            return Some(ArenaObjectCursor {
                blocks: self.finish_blocks(),
                current_block: None,
                block_pos: 0,
                offset: 0,
                finished: false,
            });
        }

        None
    }

    fn initialize(&mut self) {
        sync_inline_arena_state();

        let general_n = ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
        let survivor0_n = SURVIVOR_ARENA_0.with(|a| unsafe { (*a.get()).blocks.len() });
        let survivor1_n = SURVIVOR_ARENA_1.with(|a| unsafe { (*a.get()).blocks.len() });
        let longlived_n = LONGLIVED_ARENA.with(|a| unsafe { (*a.get()).blocks.len() });
        let old_n = OLD_ARENA.with(|a| unsafe { (*a.get()).blocks.len() });

        self.region_lengths = [general_n, survivor0_n, survivor1_n, longlived_n, old_n];
        self.region_bases = [
            0,
            general_n,
            general_n + survivor0_n,
            general_n + survivor0_n + survivor1_n,
            general_n + survivor0_n + survivor1_n + longlived_n,
        ];
        self.initialized = true;

        while self.region < ARENA_CURSOR_REGION_COUNT && self.region_lengths[self.region] == 0 {
            self.region += 1;
        }
    }

    fn push_block(&mut self, block: ArenaWalkBlock) {
        match self.order {
            ArenaWalkOrder::BlockIndex => self.blocks.push(block),
            ArenaWalkOrder::Address => {
                self.address_blocks.insert(block.data, block);
            }
        }
    }

    fn finish_blocks(&mut self) -> ArenaObjectCursorBlocks {
        match self.order {
            ArenaWalkOrder::BlockIndex => {
                ArenaObjectCursorBlocks::BlockIndex(std::mem::take(&mut self.blocks))
            }
            ArenaWalkOrder::Address => ArenaObjectCursorBlocks::Address(
                std::mem::take(&mut self.address_blocks).into_values(),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn inspected_blocks(&self) -> usize {
        self.inspected_blocks
    }
}

fn snapshot_cursor_block(
    region: usize,
    block_pos: usize,
    block_idx: usize,
) -> Option<ArenaWalkBlock> {
    let block = match region {
        ARENA_CURSOR_GENERAL => ARENA.with(|arena| unsafe {
            let blocks = &(*arena.get()).blocks;
            blocks.get(block_pos).map(snapshot_block_fields)
        }),
        ARENA_CURSOR_SURVIVOR0 => SURVIVOR_ARENA_0.with(|arena| unsafe {
            let blocks = &(*arena.get()).blocks;
            blocks.get(block_pos).map(snapshot_block_fields)
        }),
        ARENA_CURSOR_SURVIVOR1 => SURVIVOR_ARENA_1.with(|arena| unsafe {
            let blocks = &(*arena.get()).blocks;
            blocks.get(block_pos).map(snapshot_block_fields)
        }),
        ARENA_CURSOR_LONGLIVED => LONGLIVED_ARENA.with(|arena| unsafe {
            let blocks = &(*arena.get()).blocks;
            blocks.get(block_pos).map(snapshot_block_fields)
        }),
        ARENA_CURSOR_OLD => OLD_ARENA.with(|arena| unsafe {
            let blocks = &(*arena.get()).blocks;
            blocks.get(block_pos).map(snapshot_block_fields)
        }),
        _ => None,
    }?;

    if block.data == 0 {
        return None;
    }

    Some(ArenaWalkBlock { block_idx, ..block })
}

fn snapshot_block_fields(block: &ArenaBlock) -> ArenaWalkBlock {
    ArenaWalkBlock {
        data: block.data as usize,
        offset: block.offset,
        size: block.size,
        block_idx: 0,
    }
}

impl ArenaObjectCursor {
    pub(crate) fn new(order: ArenaWalkOrder) -> Self {
        let mut builder = ArenaObjectCursorBuilder::new(order);
        let mut remaining = usize::MAX;
        builder
            .step(&mut remaining)
            .expect("unbounded arena cursor build must complete")
    }

    pub(crate) fn next_budgeted(&mut self, remaining: &mut usize) -> Option<(*mut u8, usize)> {
        use crate::gc::GcHeader;

        while self.ensure_current_block() {
            let block = self
                .current_block
                .expect("current block exists after ensure_current_block");
            while self.offset < block.offset {
                let aligned = (self.offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }

                if *remaining == 0 {
                    return None;
                }
                *remaining -= 1;

                let header_ptr = (block.data + aligned) as *mut u8;
                let header = header_ptr as *const GcHeader;
                unsafe {
                    let total_size = (*header).size as usize;
                    if total_size == 0 || total_size > block.size {
                        break;
                    }
                    self.offset = aligned + total_size;
                    let obj_type = (*header).obj_type;
                    if crate::gc::gc_type_is_arena_walkable(obj_type) {
                        return Some((header_ptr, block.block_idx));
                    }
                }
            }

            self.current_block = None;
            self.offset = 0;
        }

        None
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    pub(crate) fn next(&mut self) -> Option<(*mut u8, usize)> {
        let mut remaining = usize::MAX;
        self.next_budgeted(&mut remaining)
    }

    fn ensure_current_block(&mut self) -> bool {
        if self.current_block.is_some() {
            return true;
        }
        self.current_block = match &mut self.blocks {
            ArenaObjectCursorBlocks::BlockIndex(blocks) => {
                let block = blocks.get(self.block_pos).copied();
                if block.is_some() {
                    self.block_pos += 1;
                }
                block
            }
            ArenaObjectCursorBlocks::Address(blocks) => blocks.next(),
        };
        if self.current_block.is_none() {
            self.finished = true;
            return false;
        }
        true
    }
}

pub(crate) fn arena_block_snapshots() -> Vec<ArenaBlockSnapshot> {
    sync_inline_arena_state();

    let mut snapshots = Vec::with_capacity(arena_block_count());
    let mut collect = |arena: &Arena| {
        snapshots.extend(arena.blocks.iter().map(|block| ArenaBlockSnapshot {
            data: block.data as usize,
            offset: block.offset,
            size: block.size,
        }));
    };

    ARENA.with(|arena| unsafe { collect(&*arena.get()) });
    SURVIVOR_ARENA_0.with(|arena| unsafe { collect(&*arena.get()) });
    SURVIVOR_ARENA_1.with(|arena| unsafe { collect(&*arena.get()) });
    LONGLIVED_ARENA.with(|arena| unsafe { collect(&*arena.get()) });
    OLD_ARENA.with(|arena| unsafe { collect(&*arena.get()) });

    snapshots
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
#[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
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
    pub reusable_bytes: usize,
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
                if crate::gc::gc_type_is_arena_walkable(obj_type) {
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
                    if crate::gc::gc_type_is_arena_walkable(obj_type) {
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
                    if crate::gc::gc_type_is_arena_walkable(obj_type) {
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
                    if crate::gc::gc_type_is_arena_walkable(obj_type) {
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
                    if crate::gc::gc_type_is_arena_walkable(obj_type) {
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

/// Per-block `size` for the general (nursery-Eden) arena, indexed by
/// general block index (`0..general_block_count()`, tombstones report
/// size 0). Used by the evacuation policy to translate "this block's
/// reset is blocked only by movable candidates" into the block-granule
/// bytes a defrag would actually hand back.
pub(crate) fn general_block_sizes() -> Vec<usize> {
    ARENA.with(|arena| unsafe {
        (*arena.get())
            .blocks
            .iter()
            .map(|block| if block.data.is_null() { 0 } else { block.size })
            .collect()
    })
}

/// Whether a general-arena block is in the caller-saved-register safety
/// window used by `arena_reset_empty_blocks`.
#[inline]
pub(crate) fn general_block_in_recent_window(block_idx: usize) -> bool {
    ARENA.with(|arena| unsafe {
        let arena = &*arena.get();
        if block_idx >= arena.blocks.len() {
            return false;
        }
        let keep_low = arena.current.saturating_sub(4);
        block_idx >= keep_low && block_idx <= arena.current
    })
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
