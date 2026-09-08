//! Tests for `page_meta`, split out for the 2000-line file cap.

#[cfg(test)]
mod page_generation_hasher_tests {
    use super::super::*;
    use std::collections::HashSet;
    use std::hash::BuildHasher;

    /// #7187 regression guard for `PageGenerationMap`'s hasher.
    ///
    /// `HashMap` is hashbrown: the bucket index comes from the hash's low bits,
    /// but the SIMD control byte — the filter that decides whether a group
    /// probe needs a real key comparison — is `hash >> 57`. Generation class
    /// keys are `addr >> GENERATION_CLASS_SHIFT`, so an identity hasher (which
    /// this map carried until #7187) produces a value around 2^26 whose top
    /// seven bits are zero for **every** key in the table. Every occupied slot
    /// in a probed group then matches, and each match costs a scattered load
    /// plus a key comparison — on a lookup the write barrier performs several
    /// times per heap store.
    ///
    /// This asserts the property directly rather than asserting "we call
    /// `PtrHasher`": reinstating any non-mixing hasher collapses the control
    /// byte to a single value and fails here.
    #[test]
    fn control_byte_is_spread_across_generation_class_keys() {
        let map = PageGenerationMap::default();
        let build = map.hasher();

        // Realistic 48-bit heap addresses, one per 1 MiB generation bucket —
        // the exact key population `classify_heap_generation` looks up.
        let base: usize = 0x0000_7f31_0000_0000;
        let control_bytes: HashSet<u64> = (0..64)
            .map(|i| {
                let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
                (build.hash_one(generation_class_key_for_addr(addr)) >> 57) & 0x7f
            })
            .collect();

        assert!(
            control_bytes.len() >= 32,
            "hashbrown control byte must vary across generation class keys, got {} \
             distinct values from 64 consecutive buckets (an identity hasher yields 1)",
            control_bytes.len()
        );
    }

    /// The bucket index (low bits) must stay well spread too — mixing that put
    /// all the entropy in the high bits and left the low bits constant would
    /// trade a control-byte collision for a far worse bucket collision. This is
    /// the failure `fast_hash`'s `mix` step exists for.
    #[test]
    fn bucket_index_is_spread_across_generation_class_keys() {
        let map = PageGenerationMap::default();
        let build = map.hasher();

        let base: usize = 0x0000_7f31_0000_0000;
        let low_bits: HashSet<u64> = (0..64)
            .map(|i| {
                let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
                build.hash_one(generation_class_key_for_addr(addr)) & 0x3f
            })
            .collect();

        assert!(
            low_bits.len() >= 32,
            "bucket index must vary across generation class keys, got {} distinct \
             values from 64 consecutive buckets",
            low_bits.len()
        );
    }

    /// The map must still answer correctly after the hasher change — a
    /// point-query round trip over many buckets, which is the only way this map
    /// is ever used.
    #[test]
    fn point_queries_round_trip_across_many_buckets() {
        let mut map = PageGenerationMap::default();
        let base: usize = 0x0000_7f31_0000_0000;
        for i in 0..256usize {
            let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
            map.insert(
                generation_class_key_for_addr(addr),
                PageGenerationSlot::Single(PageGenerationRange {
                    base: addr,
                    end: addr + (1 << GENERATION_CLASS_SHIFT),
                    generation: HeapGeneration::Old,
                    space: HeapSpace::Old,
                    object_starts: std::ptr::null_mut(),
                }),
            );
        }
        for i in 0..256usize {
            let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
            let found = map
                .get(&generation_class_key_for_addr(addr))
                .and_then(|slot| slot.find(addr + 0x40))
                .expect("every inserted bucket must be found by point query");
            assert_eq!(found.generation, HeapGeneration::Old);
            assert_eq!(found.base, addr);
        }
        assert_eq!(map.len(), 256);
    }
}

#[cfg(test)]
mod block_range_tests {
    use super::super::old_arena_block_range_index;

    /// `old_arena_block_range_index` is the whole reason #9772's selection can
    /// group pages by block, so it gets a test that can fail: gaps between
    /// blocks must not be attributed to the block below them.
    #[test]
    fn block_range_lookup_respects_gaps_and_ends() {
        // Two 1 MiB blocks with a 1 MiB hole between them.
        let ranges = vec![
            (0x1000_0000, 0x1010_0000, 7, 0x10_0000),
            (0x1020_0000, 0x1030_0000, 9, 0x10_0000),
        ];
        assert_eq!(old_arena_block_range_index(&ranges, 0x1000_0000), Some(0));
        assert_eq!(old_arena_block_range_index(&ranges, 0x100F_FFFF), Some(0));
        // One past the end of block 0 is the gap, not block 0.
        assert_eq!(old_arena_block_range_index(&ranges, 0x1010_0000), None);
        assert_eq!(old_arena_block_range_index(&ranges, 0x1018_0000), None);
        assert_eq!(old_arena_block_range_index(&ranges, 0x1020_0000), Some(1));
        assert_eq!(old_arena_block_range_index(&ranges, 0x102F_FFFF), Some(1));
        // Above every block, and below every block.
        assert_eq!(old_arena_block_range_index(&ranges, 0x1030_0000), None);
        assert_eq!(old_arena_block_range_index(&ranges, 0x0FFF_FFFF), None);
        assert_eq!(old_arena_block_range_index(&[], 0x1000_0000), None);
    }
}
