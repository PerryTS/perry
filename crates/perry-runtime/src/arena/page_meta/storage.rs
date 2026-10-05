//! Dense page facts without a hash bucket per 4 KiB page.
//!
//! Both page metadata and object indexes have neighboring entries. Share a
//! chunk directory while choosing chunk widths for their respective density.
//! Missing pages remain missing, and removing a page drops its payload.

use super::compact::StoredPageMeta;

struct PageChunk<T, const N: usize> {
    pages: [T; N],
    present: u64,
}

impl<T: Default, const N: usize> Default for PageChunk<T, N> {
    fn default() -> Self {
        assert!(N.is_power_of_two() && N <= 64);
        Self {
            pages: std::array::from_fn(|_| T::default()),
            present: 0,
        }
    }
}

pub(super) struct PageMap<T, const N: usize> {
    chunks: crate::fast_hash::PtrHashMap<usize, Box<PageChunk<T, N>>>,
    len: usize,
}

pub(super) type PageMetaMap = PageMap<StoredPageMeta, 64>;

impl<T, const N: usize> Default for PageMap<T, N> {
    fn default() -> Self {
        Self {
            chunks: crate::fast_hash::new_ptr_hash_map(),
            len: 0,
        }
    }
}

impl<T: Default, const N: usize> PageMap<T, N> {
    pub(super) fn get_or_insert(&mut self, page: usize) -> &mut T {
        let chunk = self.chunks.entry(page / N).or_default();
        let slot = page % N;
        let bit = 1u64 << slot;
        if chunk.present & bit == 0 {
            chunk.present |= bit;
            self.len += 1;
        }
        &mut chunk.pages[slot]
    }

    pub(super) fn get_mut(&mut self, page: &usize) -> Option<&mut T> {
        let chunk = self.chunks.get_mut(&(page / N))?;
        let slot = page % N;
        if chunk.present & (1u64 << slot) == 0 {
            return None;
        }
        Some(&mut chunk.pages[slot])
    }

    pub(super) fn get(&self, page: &usize) -> Option<&T> {
        let chunk = self.chunks.get(&(page / N))?;
        let slot = page % N;
        (chunk.present & (1u64 << slot) != 0).then_some(&chunk.pages[slot])
    }

    pub(super) fn remove(&mut self, page: &usize) {
        let key = page / N;
        let Some(chunk) = self.chunks.get_mut(&key) else {
            return;
        };
        let slot = page % N;
        let bit = 1u64 << slot;
        if chunk.present & bit != 0 {
            chunk.present &= !bit;
            self.len -= 1;
            // Release owned per-page buffers even while neighboring pages live.
            chunk.pages[slot] = T::default();
        }
        if chunk.present == 0 {
            self.chunks.remove(&key);
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.chunks.iter().flat_map(|(&key, chunk)| {
            chunk
                .pages
                .iter()
                .enumerate()
                .filter_map(move |(slot, meta)| {
                    (chunk.present & (1u64 << slot) != 0).then_some((key * N + slot, meta))
                })
        })
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &T> {
        self.iter().map(|(_, meta)| meta)
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.chunks.values_mut().flat_map(|chunk| {
            let present = chunk.present;
            chunk
                .pages
                .iter_mut()
                .enumerate()
                .filter_map(move |(slot, meta)| (present & (1u64 << slot) != 0).then_some(meta))
        })
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        self.chunks.clear();
        self.len = 0;
    }

    pub(super) fn shrink_directory(&mut self) {
        super::maintenance::shrink_sparse_map(&mut self.chunks);
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        crate::gc::census::hash_table_bytes(
            self.chunks.capacity(),
            size_of::<(usize, Box<PageChunk<T, N>>)>(),
        ) + self.chunks.len() * size_of::<PageChunk<T, N>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn owned_payload_is_released_on_remove_and_clear() {
        use std::sync::Arc;
        let owner = Arc::new(());
        let mut map = PageMap::<Option<Arc<()>>, 8>::default();
        for page in [7, 8, 9, usize::MAX] {
            *map.get_or_insert(page) = Some(owner.clone());
        }
        assert_eq!(Arc::strong_count(&owner), 5);
        map.remove(&8);
        assert_eq!(Arc::strong_count(&owner), 4);
        assert!(map.get_or_insert(8).is_none());
        assert!(map.get(&9).unwrap().is_some());
        map.remove(&usize::MAX);
        assert_eq!(Arc::strong_count(&owner), 3);
        map.clear();
        assert_eq!(Arc::strong_count(&owner), 1);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn distant_and_high_page_keys_do_not_alias() {
        let mut map = PageMetaMap::default();
        let keys = [0, 63, 64, 1 << 20, usize::MAX - 1, usize::MAX];
        for (i, page) in keys.into_iter().enumerate() {
            map.get_or_insert(page).object_count = i as u32;
        }
        let actual: BTreeMap<_, _> = map.iter().map(|(p, m)| (p, m.object_count)).collect();
        let expected: BTreeMap<_, _> = keys
            .into_iter()
            .enumerate()
            .map(|(i, p)| (p, i as u32))
            .collect();
        assert_eq!(actual, expected);
        assert!(map.get(&65).is_none());
        map.remove(&usize::MAX);
        assert_eq!(map.get(&(usize::MAX - 1)).unwrap().object_count, 4);
    }

    #[test]
    fn page_chunks_match_independent_map_through_boundary_reuse() {
        let mut map = PageMetaMap::default();
        let mut oracle = BTreeMap::new();
        for round in 0..16usize {
            for page in 61..323 {
                let value = ((page + round) % 4097) as u32;
                map.get_or_insert(page).allocated_bytes = value;
                oracle.insert(page, value);
            }
            // Includes slot 63, slot 0 of the next chunk and sparse interiors.
            for page in (61..323).filter(|p| (p + round) % 3 != 0) {
                map.remove(&page);
                map.remove(&page); // removal is idempotent
                oracle.remove(&page);
                assert!(map.get(&page).is_none());
                assert!(map.get_mut(&page).is_none());
            }
            let actual: BTreeMap<_, _> = map.iter().map(|(p, m)| (p, m.allocated_bytes)).collect();
            assert_eq!(actual, oracle);
            assert_eq!(map.len(), oracle.len());
            assert_eq!(map.values().count(), oracle.len());
            for value in map.values_mut() {
                value.dirty_slots = usize::MAX;
            }
            for &page in oracle.keys() {
                assert_eq!(map.get(&page).unwrap().dirty_slots, usize::MAX);
            }
        }
    }

    #[test]
    fn last_page_removal_releases_storage_and_reuse_is_clean() {
        let mut map = PageMetaMap::default();
        for page in 0..128 {
            map.get_or_insert(page).pinned_bytes = 32;
        }
        assert_eq!(map.chunks.len(), 2);
        let before = map.allocated_bytes();
        for page in 0..64 {
            map.remove(&page);
        }
        assert_eq!(map.chunks.len(), 1);
        assert_eq!(
            before - map.allocated_bytes(),
            size_of::<PageChunk<StoredPageMeta, 64>>()
        );
        assert_eq!(map.get(&64).unwrap().pinned_bytes, 32);
        map.remove(&64);
        assert_eq!(map.get_or_insert(64).pinned_bytes, 0);
        for page in 64..128 {
            map.remove(&page);
        }
        assert_eq!(map.len(), 0);
        assert_eq!(map.chunks.len(), 0);
        assert_eq!(map.iter().count(), 0);
        assert_eq!(map.get_or_insert(0).pinned_bytes, 0);
    }

    /// `get_or_insert` does not reset a slot when it becomes present: it relies
    /// on every non-present slot holding `T::default()`. `remove` is the only
    /// path that clears a present bit, so it must leave the slot empty. A stale
    /// object list here would hand the old-generation sweep the header offsets
    /// of dead objects on a recycled page.
    #[test]
    fn reinserted_object_page_never_inherits_removed_headers() {
        use super::super::{generation_page_base, PageObjects};
        for keep_neighbor in [false, true] {
            let mut map = PageMap::<PageObjects, 8>::default();
            let page = 4100;
            let base = generation_page_base(page);
            if keep_neighbor {
                // Keeps the chunk alive, so re-insertion reuses the same slot.
                map.get_or_insert(page + 1)
                    .push(page + 1, generation_page_base(page + 1) + 32);
            }
            let objects = map.get_or_insert(page);
            objects.push(page, base - 48); // an object entering from below
            objects.push(page, base + 16);
            objects.push(page, base + 64);
            assert_eq!(map.get(&page).unwrap().len(), 3);
            map.remove(&page);
            assert!(map.get(&page).is_none());
            let objects = map.get_or_insert(page);
            assert!(objects.is_empty(), "keep_neighbor={keep_neighbor}");
            assert_eq!(objects.iter(page).count(), 0);
            // A different object may now enter from below without tripping the
            // one-entering-object invariant left behind by the dead one.
            objects.push(page, base - 8);
            assert_eq!(objects.iter(page).collect::<Vec<_>>(), [base - 8]);
            assert_eq!(map.len(), 1 + keep_neighbor as usize);
        }
    }

    #[test]
    fn dense_pages_do_not_pay_per_page_hash_capacity() {
        let mut map = PageMetaMap::default();
        for page in 0..4096 {
            map.get_or_insert(page);
        }
        let payload = 4096 * size_of::<StoredPageMeta>();
        assert!(
            map.allocated_bytes() < payload + payload / 16,
            "directory and presence bits must add less than 6.25% to dense metadata"
        );
        assert_eq!(map.len(), 4096);
    }
}
