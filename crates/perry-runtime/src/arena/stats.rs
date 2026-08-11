use super::*;

/// Exact live-object census at the end of the most recent collection, plus
/// the allocation state needed to advance it without taxing the inline bump
/// allocator. Between collections arena offsets can only grow, while a
/// free-list allocation consumes a hole without moving an offset; those two
/// deltas therefore account for every new arena allocation.
#[derive(Clone, Copy, Default)]
struct ArenaLiveCensus {
    live_bytes: usize,
    high_water_bytes: usize,
    arena_free_bytes: usize,
    old_free_bytes: usize,
    valid: bool,
}

crate::perry_thread_local! {
    static ARENA_LIVE_CENSUS: Cell<ArenaLiveCensus> =
        const { Cell::new(ArenaLiveCensus {
            live_bytes: 0,
            high_water_bytes: 0,
            arena_free_bytes: 0,
            old_free_bytes: 0,
            valid: false,
        }) };
}

fn arena_free_bytes() -> usize {
    crate::gc::ARENA_FREE_LIST
        .with(|free| free.borrow().iter().map(|&(_, slot_size)| slot_size).sum())
}

/// Record the exact header-inclusive live bytes found by a completed GC walk.
/// This is called after block reset/reclaim, so its high-water and hole
/// snapshots describe the same instant as `live_bytes`.
pub(crate) fn record_arena_live_census(live_bytes: usize) {
    let high_water_bytes = arena_in_use_bytes();
    ARENA_LIVE_CENSUS.with(|census| {
        census.set(ArenaLiveCensus {
            live_bytes,
            high_water_bytes,
            arena_free_bytes: arena_free_bytes(),
            old_free_bytes: crate::gc::old_free_bytes(),
            valid: true,
        });
    });
}

/// Header-inclusive bytes occupied by live arena objects.
///
/// A GC publishes an exact object census. Until the next collection, new bump
/// allocations are the increase in block high-water and old/general hole reuse
/// is the decrease in free-list bytes. This keeps `heapUsed` and major-GC
/// pacing exact after a collection without adding a counter update to the
/// generated inline allocation fast path.
pub fn arena_live_allocated_bytes() -> usize {
    let high_water_bytes = arena_in_use_bytes();
    let arena_free_bytes = arena_free_bytes();
    let old_free_bytes = crate::gc::old_free_bytes();
    ARENA_LIVE_CENSUS.with(|census_cell| {
        let census = census_cell.get();
        if !census.valid {
            return high_water_bytes
                .saturating_sub(arena_free_bytes)
                .saturating_sub(old_free_bytes);
        }
        if high_water_bytes < census.high_water_bytes {
            // Before the first census all bytes below the bump pointers are
            // allocated except known holes. A decreasing high-water means a
            // direct reset happened outside the normal collect/publish path;
            // invalidate the stale census so later growth cannot accidentally
            // cross its old high-water and revive it.
            census_cell.set(ArenaLiveCensus {
                valid: false,
                ..ArenaLiveCensus::default()
            });
            return high_water_bytes
                .saturating_sub(arena_free_bytes)
                .saturating_sub(old_free_bytes);
        }
        census
            .live_bytes
            .saturating_add(high_water_bytes - census.high_water_bytes)
            .saturating_add(census.arena_free_bytes.saturating_sub(arena_free_bytes))
            .saturating_add(census.old_free_bytes.saturating_sub(old_free_bytes))
    })
}

/// Get arena memory statistics: (heap_used, heap_total).
/// `heap_used` is live allocated object bytes; `heap_total` is total reserved
/// block capacity. Allocation high-water remains separately available through
/// [`arena_in_use_bytes`].
#[no_mangle]
pub extern "C" fn js_arena_stats(out_used: *mut u64, out_total: *mut u64) {
    let used = arena_live_allocated_bytes() as u64;
    let mut total: u64 = 0;
    ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            total += block.size as u64;
        }
    });
    LONGLIVED_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            total += block.size as u64;
        }
    });
    SURVIVOR_ARENA_0.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            total += block.size as u64;
        }
    });
    SURVIVOR_ARENA_1.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
            total += block.size as u64;
        }
    });
    // Old-generation arena. Large objects (>16 KB — typed arrays, big arrays /
    // strings) are born here, and minor-GC survivors are promoted here.
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for block in &arena.blocks {
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

/// Bytes currently allocated in the old-gen arena (gen-GC Phase C).
/// Read by `gc_budgeted_due_trigger()` on every `gc_check_trigger` —
/// i.e. on every `gc_malloc` and every nursery block fill — so this
/// returns the delta-maintained cache (`OLD_GEN_IN_USE_BYTES`) instead
/// of recomputing an O(old-blocks) sum each time. Debug builds
/// cross-check the cache against the recompute so a missed mutation
/// site fails tests instead of silently skewing the OldReclaim trigger.
pub fn old_gen_in_use_bytes() -> usize {
    let cached = OLD_GEN_IN_USE_BYTES.with(|c| c.get());
    debug_assert_eq!(
        cached,
        old_gen_in_use_bytes_recomputed(),
        "OLD_GEN_IN_USE_BYTES cache drifted from the per-block recompute — \
         an old-arena offset mutation site is missing its delta update \
         (see the mutation-site inventory on OLD_GEN_IN_USE_BYTES in arena/block.rs)"
    );
    cached
}

/// O(blocks) recompute of the old-gen in-use total — the cross-check /
/// resync source of truth for the delta-maintained cache. Not for hot
/// paths.
pub(crate) fn old_gen_in_use_bytes_recomputed() -> usize {
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena.blocks.iter().map(|b| b.offset).sum()
    })
}

/// Test-only: force the cache back in sync after a test hand-mutates
/// old-arena block offsets without going through the tracked paths.
#[cfg(test)]
pub(crate) fn old_gen_in_use_bytes_resync() {
    let recomputed = old_gen_in_use_bytes_recomputed();
    OLD_GEN_IN_USE_BYTES.with(|c| c.set(recomputed));
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
