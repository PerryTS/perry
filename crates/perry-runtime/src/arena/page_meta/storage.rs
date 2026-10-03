//! Dense page facts without a hash bucket per 4 KiB page.
//!
//! Old arena blocks register all their pages, so neighboring metadata entries
//! are dense even when few objects are live. A 64-page chunk needs one hash
//! entry and one presence bitmap. Missing pages remain missing; dropping the
//! last registered page releases the chunk. Only the small chunk directory
//! retains HashMap spare capacity after a large heap shrinks.

use super::compact::StoredPageMeta;

const CHUNK_SHIFT: usize = 6;
const CHUNK_PAGES: usize = 1 << CHUNK_SHIFT;
const CHUNK_MASK: usize = CHUNK_PAGES - 1;

struct PageChunk {
    pages: [StoredPageMeta; CHUNK_PAGES],
    present: u64,
}

impl Default for PageChunk {
    fn default() -> Self {
        Self {
            pages: [StoredPageMeta::default(); CHUNK_PAGES],
            present: 0,
        }
    }
}

#[derive(Default)]
pub(super) struct PageMetaMap {
    chunks: crate::fast_hash::PtrHashMap<usize, Box<PageChunk>>,
    len: usize,
}

impl PageMetaMap {
    pub(super) fn get_or_insert(&mut self, page: usize) -> &mut StoredPageMeta {
        let chunk = self.chunks.entry(page >> CHUNK_SHIFT).or_default();
        let slot = page & CHUNK_MASK;
        let bit = 1u64 << slot;
        if chunk.present & bit == 0 {
            chunk.pages[slot] = StoredPageMeta::default();
            chunk.present |= bit;
            self.len += 1;
        }
        &mut chunk.pages[slot]
    }

    pub(super) fn get_mut(&mut self, page: &usize) -> Option<&mut StoredPageMeta> {
        let chunk = self.chunks.get_mut(&(page >> CHUNK_SHIFT))?;
        let slot = page & CHUNK_MASK;
        if chunk.present & (1u64 << slot) == 0 {
            return None;
        }
        Some(&mut chunk.pages[slot])
    }

    #[cfg(test)]
    pub(super) fn get(&self, page: &usize) -> Option<&StoredPageMeta> {
        let chunk = self.chunks.get(&(page >> CHUNK_SHIFT))?;
        let slot = page & CHUNK_MASK;
        (chunk.present & (1u64 << slot) != 0).then_some(&chunk.pages[slot])
    }

    pub(super) fn remove(&mut self, page: &usize) {
        let key = page >> CHUNK_SHIFT;
        let Some(chunk) = self.chunks.get_mut(&key) else {
            return;
        };
        let bit = 1u64 << (page & CHUNK_MASK);
        if chunk.present & bit != 0 {
            chunk.present &= !bit;
            self.len -= 1;
        }
        if chunk.present == 0 {
            self.chunks.remove(&key);
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &StoredPageMeta)> {
        self.chunks.iter().flat_map(|(&key, chunk)| {
            chunk
                .pages
                .iter()
                .enumerate()
                .filter_map(move |(slot, meta)| {
                    (chunk.present & (1u64 << slot) != 0)
                        .then_some(((key << CHUNK_SHIFT) + slot, meta))
                })
        })
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &StoredPageMeta> {
        self.iter().map(|(_, meta)| meta)
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut StoredPageMeta> {
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

    pub(super) fn shrink_directory(&mut self) {
        super::maintenance::shrink_sparse_map(&mut self.chunks);
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        crate::gc::census::hash_table_bytes(
            self.chunks.capacity(),
            size_of::<(usize, Box<PageChunk>)>(),
        ) + self.chunks.len() * size_of::<PageChunk>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
        assert_eq!(before - map.allocated_bytes(), size_of::<PageChunk>());
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
