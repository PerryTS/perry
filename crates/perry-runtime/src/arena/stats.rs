use super::*;

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
    // Old-generation arena. Large objects (>16 KB — typed arrays, big arrays /
    // strings) are born here, and minor-GC survivors are promoted here. Without
    // this region `heapUsed` / `heapTotal` collapse toward 0 while RSS climbs,
    // since the live/large heap lives entirely in old-gen. Mirrors the old-gen
    // phase of `arena_in_use_bytes` (walk.rs) so the two accounts agree.
    OLD_ARENA.with(|arena| {
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
