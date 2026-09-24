//! Native storage owned by a Map header. Header copies transfer ownership;
//! only an unforwarded dead header may destroy it.
use super::*;
use crate::fast_hash::{new_ptr_hash_map, PtrHashMap};

// Keep Map-free programs off the bitmap walk and brand-probe path. This is
// only an empty fast path, never an ownership registry or liveness authority.
static MAP_STORE_EVER_ALLOCATED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn map_stores_never_allocated() -> bool {
    !MAP_STORE_EVER_ALLOCATED.load(std::sync::atomic::Ordering::Relaxed)
}

pub(super) struct MapStore {
    pub(super) entries: *mut f64,
    pub(super) capacity: usize,
    pub(super) numeric: NumericIndex,
    pub(super) strings: StringIndex,
    pub(super) pointers: PtrHashMap<MapPtrKey, u32>,
    pub(super) compaction: Option<MapCompactionLog>,
    #[cfg(test)]
    pub(super) pointer_rebuilds: usize,
}

impl MapStore {
    pub(super) fn new(entries: *mut f64, capacity: usize) -> Self {
        MAP_STORE_EVER_ALLOCATED.store(true, std::sync::atomic::Ordering::Relaxed);
        Self {
            entries,
            capacity,
            numeric: NumericIndex::new(),
            strings: StringIndex::default(),
            pointers: new_ptr_hash_map(),
            compaction: None,
            #[cfg(test)]
            pointer_rebuilds: 0,
        }
    }
}

impl Drop for MapStore {
    fn drop(&mut self) {
        if !self.entries.is_null() && self.capacity != 0 {
            let layout = entries_layout(self.capacity);
            unsafe {
                dealloc(self.entries.cast(), layout);
            }
            note_test_map_side_deallocation(layout.size());
        }
    }
}

/// Most content hashes have exactly one entry: store its offset inline.
/// Allocate collision vectors only for actual collisions, never per key.
#[derive(Default)]
pub(super) struct StringIndex {
    first: PtrHashMap<u64, u32>,
    collisions: PtrHashMap<u64, Vec<u32>>,
}

impl StringIndex {
    pub(super) fn insert(&mut self, hash: u64, index: u32) {
        match self.first.entry(hash) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(index);
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                self.collisions.entry(hash).or_default().push(index);
            }
        }
    }

    pub(super) fn candidates(&self, hash: u64) -> impl Iterator<Item = u32> + '_ {
        self.first
            .get(&hash)
            .copied()
            .into_iter()
            .chain(self.collisions.get(&hash).into_iter().flatten().copied())
    }

    pub(super) fn remove(&mut self, hash: u64, index: u32) {
        if self.first.get(&hash) == Some(&index) {
            if let Some(replacement) = self.collisions.get_mut(&hash).and_then(Vec::pop) {
                self.first.insert(hash, replacement);
            } else {
                self.first.remove(&hash);
            }
        } else if let Some(rest) = self.collisions.get_mut(&hash) {
            rest.retain(|&candidate| candidate != index);
        }
        if self.collisions.get(&hash).is_some_and(Vec::is_empty) {
            self.collisions.remove(&hash);
        }
    }

    pub(super) fn clear(&mut self) {
        self.first.clear();
        self.collisions.clear();
    }
}

/// Brand checks accept only an allocation-authored Map start, then its type.
/// Range membership precedes every dereference, including for garbage inputs.
pub(super) fn is_live_map(addr: usize) -> bool {
    if map_stores_never_allocated() || crate::value::addr_class::is_handle_band(addr) {
        return false;
    }
    let Some(header_addr) = addr.checked_sub(crate::gc::GC_HEADER_SIZE) else {
        return false;
    };
    let Some((_, base, bitmap)) = crate::arena::classify_heap_space_in_range(addr) else {
        return false;
    };
    if !crate::arena::arena_header_is_object_start(header_addr, base, bitmap) {
        return false;
    }
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(addr) else {
            return false;
        };
        let header = header.as_ref();
        header.obj_type == crate::gc::GC_TYPE_MAP
            && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && header.size as usize == crate::gc::GC_HEADER_SIZE + std::mem::size_of::<MapHeader>()
            && !(*(addr as *const MapHeader)).store.is_null()
    }
}

pub(crate) unsafe fn finalize_map_side_allocation_for_gc(map: *mut MapHeader) {
    if map.is_null() {
        return;
    }
    let store = std::mem::replace(&mut (*map).store, ptr::null_mut());
    if store.is_null() {
        return;
    }
    let store = Box::from_raw(store);
    crate::gc::gc_note_external_side_free(entries_layout(store.capacity).size());
    drop(store);
    // GC_STORE_AUDIT(POINTER_FREE): the native owner and external entries have been freed.
    (*map).entries = ptr::null_mut();
    (*map).capacity = 0;
    (*map).size = 0;
    (*map).used = 0;
}

pub(crate) fn map_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == 0 || new_addr == 0 || old_addr == new_addr {
        return;
    }
    // The copied header already carries the stable store pointer, so the old
    // header must give up ownership here. It cannot rely on staying a
    // forwarding stub: old-generation evacuation (tenured nursery objects
    // and old-page defrag) releases FORWARDED on the original before the
    // sweep, which then reclaims it as an ordinary dead Map, and the exit
    // walks skip only headers that still carry FORWARDED. Either would free
    // the store the live copy uses. The forwarding word covers payload bytes
    // 0..8 only; `entries` (8) and `store` (16) are still ours to clear.
    unsafe {
        let old = old_addr as *mut MapHeader;
        // GC_STORE_AUDIT(POINTER_FREE): ownership moved to the copy; the
        // stale header must hold neither native pointer.
        (*old).store = ptr::null_mut();
        (*old).entries = ptr::null_mut();
    }
    MAP_FOREACH_STACK.with(|stack| {
        for addr in stack.borrow_mut().iter_mut() {
            if *addr == old_addr {
                *addr = new_addr;
            }
        }
    });
}

pub(crate) fn finalize_dead_copied_minor_from_space_maps() -> usize {
    let mut count = 0;
    crate::arena::walk_map_allocations(true, |header| unsafe {
        if (*header).gc_flags
            & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_PINNED)
            == 0
        {
            let map = header
                .cast::<u8>()
                .add(crate::gc::GC_HEADER_SIZE)
                .cast::<MapHeader>();
            if !(*map).store.is_null() {
                finalize_map_side_allocation_for_gc(map);
                count += 1;
            }
        }
    });
    #[cfg(test)]
    TEST_FROM_SPACE_MAP_FINALIZATIONS.with(|c| c.set(c.get() + count));
    count
}

#[cfg(test)]
crate::perry_thread_local! {
    static TEST_FROM_SPACE_MAP_FINALIZATIONS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Dead Maps this thread's copying-minor from-space walk has finalized.
#[cfg(test)]
pub(crate) fn test_from_space_map_finalizations() -> usize {
    TEST_FROM_SPACE_MAP_FINALIZATIONS.with(std::cell::Cell::get)
}

pub(crate) fn release_current_thread_map_side_allocations() {
    crate::arena::walk_map_allocations(false, |header| unsafe {
        if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
            finalize_map_side_allocation_for_gc(
                header.cast::<u8>().add(crate::gc::GC_HEADER_SIZE).cast(),
            );
        }
    });
}

#[cfg(test)]
pub(crate) fn test_map_side_allocation(addr: usize) -> Option<(usize, usize)> {
    if !is_live_map(addr) {
        return None;
    }
    unsafe {
        let store = &*(*(addr as *const MapHeader)).store;
        Some((store.entries as usize, store.capacity))
    }
}

/// TLS destructors cannot access GC accounting TLS, which may already be gone.
pub(crate) unsafe fn drop_map_store_at_thread_exit(map: *mut MapHeader) {
    let store = std::mem::replace(&mut (*map).store, ptr::null_mut());
    if !store.is_null() {
        drop(Box::from_raw(store));
    }
}

#[cfg(test)]
mod tests;
