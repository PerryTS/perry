//! Derived identity index for WeakMap/WeakSet (#10057).
//!
//! The GC-managed entry array remains the only storage of keys and values.
//! This cache contains untraced key bits and integer array offsets: it must
//! never keep a key, entry, or collection alive. The dead-owner fan-out clears
//! it before any collection can recycle addresses; the weak-holder metadata
//! scanner also clears it before relocation. The next operation rebuilds from
//! the current entry slots once, including tombstones created by the GC.
//!
//! Budgeted non-moving weak processing can tombstone an entry after a rebuild.
//! Every hit therefore validates the actual weak slot before returning it.
//! A stale hit is removed and its slot becomes reusable. Other newly dead
//! slots join the free list on the next rebuild, without a scan per insertion.

use super::{
    object_field_bits, weak_entry_at, ArrayHeader, ObjectHeader, TAG_UNDEFINED,
    WEAK_ENTRY_KEY_FIELD,
};
use crate::fast_hash::{new_ptr_hash_map, PtrHashMap};
use std::cell::RefCell;

struct WeakEntryIndex {
    keys: PtrHashMap<u64, u32>,
    free: Vec<u32>,
    len: u32,
}

thread_local! {
    static WEAK_COLLECTION_INDEXES: RefCell<PtrHashMap<usize, WeakEntryIndex>> =
        RefCell::new(new_ptr_hash_map());
}

pub(crate) fn clear_weak_collection_indexes() {
    WEAK_COLLECTION_INDEXES.with(|indexes| indexes.borrow_mut().clear());
}

impl WeakEntryIndex {
    unsafe fn build(entries: *mut ArrayHeader) -> Self {
        let len = crate::array::js_array_length(entries);
        let mut index = Self {
            keys: new_ptr_hash_map(),
            free: Vec::new(),
            len,
        };
        for slot in 0..len {
            let entry = weak_entry_at(entries, slot as usize);
            let key = if entry.is_null() {
                TAG_UNDEFINED
            } else {
                object_field_bits(entry, WEAK_ENTRY_KEY_FIELD)
            };
            if key == TAG_UNDEFINED {
                index.free.push(slot);
            } else {
                index.keys.insert(key, slot);
            }
        }
        index
    }

    unsafe fn find(&mut self, entries: *mut ArrayHeader, key: u64) -> Option<u32> {
        let slot = *self.keys.get(&key)?;
        let entry = weak_entry_at(entries, slot as usize);
        if !entry.is_null() && object_field_bits(entry, WEAK_ENTRY_KEY_FIELD) == key {
            return Some(slot);
        }
        // A sliced weak pass cleared this entry after the cache was built.
        self.keys.remove(&key);
        self.free.push(slot);
        None
    }
}

unsafe fn with_index<R>(
    map: *mut ObjectHeader,
    entries: *mut ArrayHeader,
    f: impl FnOnce(&mut WeakEntryIndex) -> R,
) -> R {
    WEAK_COLLECTION_INDEXES.with(|indexes| {
        let mut indexes = indexes.borrow_mut();
        let index = indexes
            .entry(map as usize)
            .or_insert_with(|| WeakEntryIndex::build(entries));
        if index.len != crate::array::js_array_length(entries) {
            *index = WeakEntryIndex::build(entries);
        }
        f(index)
    })
}

pub(super) unsafe fn find(
    map: *mut ObjectHeader,
    entries: *mut ArrayHeader,
    key: u64,
) -> Option<u32> {
    if crate::array::js_array_length(entries) == 0 {
        return None;
    }
    with_index(map, entries, |index| index.find(entries, key))
}

pub(super) unsafe fn take_free(map: *mut ObjectHeader, entries: *mut ArrayHeader) -> Option<u32> {
    with_index(map, entries, |index| index.free.pop())
}

/// Publish only AFTER the allocating entry/array stores, using reloaded roots.
/// If those stores collected, the cache is gone and includes the new entry
/// when rebuilt. Otherwise the reserved free slot has already been popped.
pub(super) unsafe fn inserted(
    map: *mut ObjectHeader,
    entries: *mut ArrayHeader,
    key: u64,
    slot: u32,
) {
    WEAK_COLLECTION_INDEXES.with(|indexes| {
        let mut indexes = indexes.borrow_mut();
        if let Some(index) = indexes.get_mut(&(map as usize)) {
            index.keys.insert(key, slot);
            index.len = crate::array::js_array_length(entries);
        } else {
            indexes.insert(map as usize, WeakEntryIndex::build(entries));
        }
    });
}

pub(super) unsafe fn deleted(
    map: *mut ObjectHeader,
    entries: *mut ArrayHeader,
    key: u64,
    slot: u32,
) {
    with_index(map, entries, |index| {
        index.keys.remove(&key);
        index.free.push(slot);
    });
}

#[cfg(test)]
pub(crate) fn cached_collections() -> usize {
    WEAK_COLLECTION_INDEXES.with(|indexes| indexes.borrow().len())
}
