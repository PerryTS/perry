//! The Map's store: an ordinary young, movable GC object holding the inline
//! entry pairs and owning the native indexes. The header's `store` word is its
//! only edge; the header's `entries` alias is re-derived by the Map's rewrite
//! hook whenever the store moves. A store that dies young releases its native
//! buffers in the copying minor (`arena/owned_stores.rs`), an older one in the
//! ordinary sweep through its finalize hook.
use super::*;
use crate::fast_hash::{new_ptr_hash_map, PtrHashMap};

#[repr(C)]
pub(crate) struct MapStore {
    // Internal holders must never expose a ShapeId at payload +4 (rule 3).
    _receiver_word: u64,
    pub(super) entries: *mut f64,
    pub(super) capacity: usize,
    pub(super) used: usize,
    pub(super) numeric: NumericIndex,
    pub(super) strings: StringIndex,
    pub(super) pointers: PtrHashMap<MapPtrKey, u32>,
    pub(super) compaction: Option<MapCompactionLog>,
    #[cfg(test)]
    pub(super) pointer_rebuilds: usize,
}

/// Index payload bytes follow reserved capacity, not the number of live keys.
#[inline]
pub(super) fn hash_index_bytes<K, V>(index: &PtrHashMap<K, V>) -> usize {
    index.capacity() * std::mem::size_of::<(K, V)>()
}

/// Like SetIndex growth accounting, this records debt for the next safepoint;
/// it must not introduce a collection while callers hold raw Map pointers.
#[inline]
pub(super) fn note_index_bytes_changed(before: usize, after: usize) {
    if after > before {
        crate::gc::gc_note_external_side_alloc(after - before);
    } else if before > after {
        crate::gc::gc_note_external_side_free(before - after);
    }
}

impl MapStore {
    pub(super) fn index_bytes(&self) -> usize {
        self.numeric.byte_len() + self.strings.byte_len() + hash_index_bytes(&self.pointers)
    }

    pub(super) fn new(entries: *mut f64, capacity: usize) -> Self {
        Self {
            _receiver_word: 0,
            entries,
            capacity,
            used: 0,
            numeric: NumericIndex::new(),
            strings: StringIndex::default(),
            pointers: new_ptr_hash_map(),
            compaction: None,
            #[cfg(test)]
            pointer_rebuilds: 0,
        }
    }
}

/// Bounded native publication: a creating caller allocates the store before
/// anything unrooted is live; a growing caller keeps its receiver and values in
/// roots and holds a no-move scope until the new store is linked.
pub(super) unsafe fn alloc_store(capacity: usize) -> *mut MapStore {
    alloc_store_in::<false>(capacity)
}

/// `OLD` births a tenured store: test fixtures that model an old collection
/// pair it with an old header. Production stores are always born young.
pub(super) unsafe fn alloc_store_in<const OLD: bool>(capacity: usize) -> *mut MapStore {
    let size = std::mem::size_of::<MapStore>()
        .checked_add(entries_layout(capacity).size())
        .filter(|size| *size <= u32::MAX as usize - crate::gc::GC_HEADER_SIZE - 8)
        .unwrap_or_else(|| crate::error::throw_allocation_failed());
    let store = if OLD {
        crate::arena::arena_alloc_gc_old_born_tenured(
            size,
            std::mem::align_of::<MapStore>(),
            crate::gc::GC_TYPE_MAP_STORE,
        )
    } else {
        crate::arena::arena_alloc_gc(
            size,
            std::mem::align_of::<MapStore>(),
            crate::gc::GC_TYPE_MAP_STORE,
        )
    }
    .cast::<MapStore>();
    crate::arena::note_young_owned_store(store.cast());
    let entries = store
        .cast::<u8>()
        .add(std::mem::size_of::<MapStore>())
        .cast::<f64>();
    // GC_STORE_AUDIT(INIT): Fresh MapStore header owns empty indexes and entries points to its own inline payload. used=0 and payload is zeroed before return; no existing managed child is copied. Caller roots/no-move contract is stated at alloc_store.
    std::ptr::write(store, MapStore::new(entries, capacity));
    std::ptr::write_bytes(entries, 0, capacity * 2);
    store
}

/// Move hook: the copy owns the native indexes now. Its entries alias points
/// at its own inline payload, and the abandoned original holds no buffer a
/// later sweep or exit walk could release a second time. The forwarding word
/// covers payload bytes 0..8 only (the reserved receiver word).
pub(crate) unsafe fn owned_store_moved_for_gc(old_user: usize, new_user: usize) {
    if old_user == new_user {
        return;
    }
    let new = new_user as *mut MapStore;
    if !(*new).entries.is_null() {
        (*new).entries = (new_user + std::mem::size_of::<MapStore>()) as *mut f64;
    }
    // GC_STORE_AUDIT(POINTER_FREE): ownership moved to the copy.
    (*(old_user as *mut MapStore)).entries = ptr::null_mut();
    crate::arena::note_owned_store_copied(new_user);
}

/// The header's entries alias names its store's inline payload; refreshed by
/// the Map's rewrite hook after a pass rewrote the `store` edge.
pub(crate) unsafe fn refresh_entries_alias(map: *mut MapHeader) {
    if let Some(store) = (*map).store.as_ref() {
        // GC_STORE_AUDIT(ROOT): Only caller is gc::types::run_gc_rewrite_hook MapEntries after rewriting traced map.store (types.rs1311). entries is re-derived interior payload address from new live store, not an independent ownership edge.
        (*map).entries = store.entries;
    }
}

/// Idempotent native cleanup. Values belong to the GC allocation itself;
/// only Rust index/history buffers need a destructor. Exit must avoid TLS.
pub(crate) unsafe fn finalize_owned_store(store: *mut u8, accounting: bool) {
    let store = &mut *store.cast::<MapStore>();
    if store.entries.is_null() {
        return;
    }
    if accounting {
        crate::gc::gc_note_external_side_free(store.index_bytes());
    }
    drop(std::mem::replace(&mut store.numeric, NumericIndex::new()));
    drop(std::mem::take(&mut store.strings));
    drop(std::mem::take(&mut store.pointers));
    drop(store.compaction.take());
    note_test_map_side_deallocation(entries_layout(store.capacity).size());
    store.entries = ptr::null_mut();
    store.capacity = 0;
    store.used = 0;
}

/// Most content hashes have exactly one entry: store its offset inline.
/// Allocate collision vectors only for actual collisions, never per key.
#[derive(Default)]
pub(super) struct StringIndex {
    first: PtrHashMap<u64, u32>,
    collisions: PtrHashMap<u64, Vec<u32>>,
}

impl StringIndex {
    pub(super) fn byte_len(&self) -> usize {
        hash_index_bytes(&self.first)
            + hash_index_bytes(&self.collisions)
            + self
                .collisions
                .values()
                .map(|rest| rest.capacity() * std::mem::size_of::<u32>())
                .sum::<usize>()
    }

    pub(super) fn insert(&mut self, hash: u64, index: u32) {
        let before = hash_index_bytes(&self.first) + hash_index_bytes(&self.collisions);
        let mut vector_growth = 0;
        match self.first.entry(hash) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(index);
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                let rest = self.collisions.entry(hash).or_default();
                let capacity = rest.capacity();
                rest.push(index);
                vector_growth = (rest.capacity() - capacity) * std::mem::size_of::<u32>();
            }
        }
        note_index_bytes_changed(
            before,
            hash_index_bytes(&self.first) + hash_index_bytes(&self.collisions) + vector_growth,
        );
    }

    /// Check the inline candidate before consulting overflow. The predicate
    /// compares the authoritative entry, so hash collisions remain exact.
    #[inline]
    pub(super) fn find(&self, hash: u64, matches: impl Fn(u32) -> bool) -> Option<u32> {
        let first = *self.first.get(&hash)?;
        if matches(first) {
            return Some(first);
        }
        self.collisions
            .get(&hash)?
            .iter()
            .copied()
            .find(|&i| matches(i))
    }

    pub(super) fn candidates(&self, hash: u64) -> impl Iterator<Item = u32> + '_ {
        self.first
            .get(&hash)
            .copied()
            .into_iter()
            // Most hashes have one candidate. Probe overflow only if the
            // caller consumes past it, after checking authoritative key bytes.
            .chain(
                std::iter::once_with(move || self.collisions.get(&hash))
                    .flatten()
                    .flatten()
                    .copied(),
            )
    }

    pub(super) fn remove(&mut self, hash: u64, index: u32) {
        let before = hash_index_bytes(&self.first)
            + hash_index_bytes(&self.collisions)
            + self
                .collisions
                .get(&hash)
                .map_or(0, |rest| rest.capacity() * std::mem::size_of::<u32>());
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
        let after = hash_index_bytes(&self.first)
            + hash_index_bytes(&self.collisions)
            + self
                .collisions
                .get(&hash)
                .map_or(0, |rest| rest.capacity() * std::mem::size_of::<u32>());
        note_index_bytes_changed(before, after);
    }

    pub(super) fn clear(&mut self) {
        let before = self.byte_len();
        self.first.clear();
        self.collisions.clear();
        note_index_bytes_changed(before, self.byte_len());
    }
}

/// Brand checks accept only an allocation-authored Map start, then its type.
/// Range membership precedes every dereference, including for garbage inputs.
pub(super) fn is_live_map(addr: usize) -> bool {
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_tracked_arena_header_of_type(
            addr,
            crate::gc::GC_TYPE_MAP,
        ) else {
            return false;
        };
        header.as_ref().size as usize
            == (crate::gc::GC_HEADER_SIZE + std::mem::size_of::<MapHeader>()).next_multiple_of(8)
            && !(*(addr as *const MapHeader)).store.is_null()
    }
}

#[cfg(test)]
pub(crate) unsafe fn finalize_map_side_allocation_for_gc(map: *mut MapHeader) {
    let store = std::mem::replace(&mut (*map).store, ptr::null_mut());
    if !store.is_null() {
        finalize_owned_store(store.cast(), true);
    }
    // GC_STORE_AUDIT(POINTER_FREE): Reset writes null to map.entries, removing an alias; no new pointer can require a barrier.
    (*map).entries = ptr::null_mut();
    (*map).capacity = 0;
    (*map).size = 0;
    (*map).used = 0;
}

pub(crate) fn map_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == 0 || new_addr == 0 || old_addr == new_addr {
        return;
    }
    // The store is its own GC allocation: only the live header's ordinary
    // edge owns it. Clearing these aliases on the forwarding source avoids
    // stale fixture reads; no header is responsible for freeing the store.
    unsafe {
        let old = old_addr as *mut MapHeader;
        // GC_STORE_AUDIT(POINTER_FREE): retired forwarding source aliases.
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

/// The raw owned-store word of a Map header, read without trusting it. A
/// fixture that builds a `MapHeader` by hand must leave this null, or the
/// thread-exit arena drop frees whatever it holds (#11362).
#[cfg(test)]
pub(crate) unsafe fn test_map_store_word(map: *const MapHeader) -> usize {
    (*map).store as usize
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) unsafe fn test_map_index_bytes(map: *const MapHeader) -> usize {
    (*(*map).store).index_bytes()
}

/// Only the active, initialized extent is traced; cleared/retired stores hold
/// no JS values even while waiting for their ordinary full collection.
pub(crate) unsafe fn owned_entry_slot_range(user: *mut u8) -> Option<crate::gc::HeapSlotRange> {
    let store = &*user.cast::<MapStore>();
    if store.entries.is_null() || store.used == 0 || store.used > store.capacity {
        return None;
    }
    let header = crate::gc::header_from_trusted_user_ptr(user);
    let start = user.add(std::mem::size_of::<MapStore>()) as usize;
    let end = start.checked_add(store.used.checked_mul(16)?)?;
    if store.entries as usize != start
        || end > (header as usize).checked_add((*header).size as usize)?
    {
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        store.entries.cast(),
        store.used * 2,
    ))
}

#[cfg(test)]
pub(crate) unsafe fn test_init_owned_map_store(map: *mut MapHeader, capacity: u32) {
    let store = alloc_store_in::<true>(capacity as usize);
    (*store).used = capacity as usize; // Fixtures initialize the whole zeroed extent.
    (*map).store = store;
    // GC_STORE_AUDIT(INIT): cfg(test) constructor allocates tenured store, zeroes whole payload, sets used=capacity, then initializes test header.store/entries before fixture publication. Store is an old GC allocation; entries only aliases its inline payload. Keep distinction from pointer-free data.
    (*map).entries = (*store).entries;
}
