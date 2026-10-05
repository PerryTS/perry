//! Release directory capacity after a full sweep has removed most of a burst's
//! pages. Leave growth headroom and shrink only after a substantial drop, so
//! small occupancy fluctuations do not repeatedly rehash the directories.

use super::*;

pub(super) fn shrink_sparse_map<V>(map: &mut crate::fast_hash::PtrHashMap<usize, V>) {
    let target = map.len().saturating_mul(2).max(32);
    if map.capacity() > target.saturating_mul(2) {
        map.shrink_to(target);
    }
}

/// Full-cycle reclaim has finished all index removals. No map-entry references
/// escape the RefCell borrows; classifiers cache page facts, not bucket addresses.
/// Pending registrations must be published before sizing their directories.
pub(crate) fn shrink_page_tables() {
    flush_deferred_old_page_registrations();
    OLD_GEN_PAGE_OBJECTS.with(|m| m.borrow_mut().shrink_directory());
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|m| shrink_sparse_map(&mut m.borrow_mut()));
    PAGE_GENERATIONS.with(|m| shrink_sparse_map(&mut m.borrow_mut()));
    OLD_GEN_PAGE_META.with(|m| m.borrow_mut().shrink_directory());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_directory_releases_burst_capacity_and_preserves_surviving_entries() {
        let mut map = crate::fast_hash::new_ptr_hash_map();
        let keys = [0, 63, 64, 4096, usize::MAX - 1, usize::MAX];
        for i in 0..32768 {
            map.insert(i, i ^ 83);
        }
        for key in keys {
            map.insert(key, key ^ 83);
        }
        map.retain(|k, _| keys.contains(k));
        let capacity = map.capacity();
        shrink_sparse_map(&mut map);
        assert!(
            map.capacity() < capacity / 16,
            "most empty buckets must be released"
        );
        assert_eq!(map.len(), keys.len());
        for key in keys {
            assert_eq!(map.get(&key), Some(&(key ^ 83)));
        }
        for i in 65..1000 {
            map.insert(i, i ^ 83);
        }
        for key in keys {
            assert_eq!(map.get(&key), Some(&(key ^ 83)));
        }
    }

    #[test]
    fn stable_directory_keeps_growth_headroom() {
        let mut map = crate::fast_hash::new_ptr_hash_map();
        for i in 0..1000 {
            map.insert(i, i);
        }
        for i in 0..100 {
            map.remove(&i);
        }
        // HashMap's reported capacity can change when removals leave tombstones.
        // Measure after those removals to isolate what the shrink helper does.
        let capacity = map.capacity();
        shrink_sparse_map(&mut map);
        assert_eq!(map.capacity(), capacity);
    }
}
