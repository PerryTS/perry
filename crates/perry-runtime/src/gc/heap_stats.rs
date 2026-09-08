//! Numeric heap census for `bun:jsc.heapStats`, covering the calling thread.
//!
//! No JS allocations or collection may occur during this walk. Only counters
//! leave the walk, so constructing the JS report afterwards cannot invalidate
//! a saved pointer. As with heap snapshots, uncollected arena residents can
//! appear, but free-list slots and forwarding headers do not.
use super::*;

pub(crate) struct HeapStats {
    pub(crate) arena_used: u64,
    pub(crate) arena_reserved: u64,
    pub(crate) malloc_bytes: u64,
    pub(crate) malloc_count: u64,
    pub(crate) object_count: u64,
    pub(crate) pinned_count: u64,
    pub(crate) types: Vec<(&'static str, u64, u64)>,
}

pub(crate) fn heap_stats() -> HeapStats {
    let mut arena_used = 0;
    let mut arena_reserved = 0;
    crate::arena::js_arena_stats(&mut arena_used, &mut arena_reserved);
    let free_slots: std::collections::HashSet<*mut u8> =
        ARENA_FREE_LIST.with(|slots| slots.borrow().iter().map(|&(ptr, _)| ptr).collect());
    let mut counts = [0u64; GC_TYPE_MAX as usize + 1];
    let mut pinned = [0u64; GC_TYPE_MAX as usize + 1];
    let mut malloc_bytes = 0u64;
    let mut malloc_count = 0u64;
    let mut visit = |ptr: *mut u8, malloc: bool| unsafe {
        let header = &*ptr.cast::<GcHeader>();
        if gc_type_info(header.obj_type).is_none()
            || header.size == 0
            || header.gc_flags & GC_FLAG_FORWARDED != 0
            || free_slots.contains(&ptr)
            || free_slots.contains(&ptr.add(GC_HEADER_SIZE))
        {
            return;
        }
        let index = header.obj_type as usize;
        counts[index] += 1;
        if header.gc_flags & GC_FLAG_PINNED != 0 {
            pinned[index] += 1;
        }
        if malloc {
            malloc_count += 1;
            malloc_bytes = malloc_bytes.saturating_add(header.size as u64);
        }
    };
    crate::arena::arena_walk_objects(|ptr| visit(ptr, false));
    MALLOC_STATE.with(|state| {
        for &header in &state.borrow().objects {
            visit(header.cast(), true);
        }
    });
    HeapStats {
        arena_used,
        arena_reserved,
        malloc_bytes,
        malloc_count,
        object_count: counts.iter().sum(),
        pinned_count: pinned.iter().sum(),
        types: gc_type_infos()
            .filter_map(|info| {
                let index = info.type_id as usize;
                (counts[index] != 0).then_some((info.name, counts[index], pinned[index]))
            })
            .collect(),
    }
}
