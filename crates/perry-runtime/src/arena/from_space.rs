//! Young-generation occupancy without a block walk.
//!
//! `copying_from_space_in_use_bytes()` is the basis of the scavenge nursery
//! cap, and `gc_budgeted_due_trigger()` reads it on every runtime safepoint
//! poll and every `gc_malloc`. Summing `block.offset` over every Eden block and
//! every active-survivor block made each of those reads O(blocks), and the
//! young generation can hold hundreds of blocks once the cap scales with the
//! tenured set.
//!
//! # What changes between two reads
//!
//! Split the sum into the Eden block allocation is currently bumping
//! (`blocks[current]`) and everything else, the *sealed* bytes. Between two
//! reads the sealed bytes can change in only three ways:
//!
//! 1. **A free or move.** Every reset, detach, release, evacuation and survivor
//!    flip runs inside a [`HeapChange`](crate::gc::heap_generation::HeapChange)
//!    scope, which advances the heap generation when it opens and again when it
//!    closes. The funnel is enforced: the reset and evacuation primitives call
//!    `debug_assert_heap_change_open`.
//! 2. **The allocator moves `current`.** Filling a block and continuing in
//!    another one (a fresh block, a reused tombstone slot, or a partly-used
//!    block with room for a smaller request) happens outside any scope. Every
//!    such move goes through [`Arena::set_current`], which invalidates the
//!    cache. Comparing the index alone would not do: `current` can leave
//!    block 3 for block 5 and come back to block 3 once block 5 is full.
//! 3. **The active survivor space changes.** That is a flip, so it is case 1.
//!
//! Survivor allocation fills the *inactive* space, which is not from-space
//! until the flip, and the flip happens inside the copying minor's scope.
//!
//! So the cache holds the sealed bytes keyed on the heap generation, and a read
//! is `sealed + blocks[current].offset`. Eden's `current` and the active
//! survivor index are deliberately not part of the key: every move of either is
//! already covered by rule 2 or rule 1, and each extra key field is one more
//! checked thread-local lookup on a path the due check takes on every poll.
//! Nothing is stored while a scope is open, because the generation does not
//! move inside one; a value stored before the scope opened stops matching when
//! it opens.
//!
//! Debug builds compare every cached answer with the walk, so a mutation site
//! that bypasses both rules fails the test that reaches it rather than skewing
//! the nursery cap.

use super::*;

#[derive(Clone, Copy)]
struct SealedYoungBytes {
    valid: bool,
    heap_generation: u64,
    /// Eden bytes outside the current block, plus every byte of the active
    /// survivor space.
    sealed: usize,
}

impl SealedYoungBytes {
    const INVALID: Self = Self {
        valid: false,
        heap_generation: 0,
        sealed: 0,
    };
}

crate::perry_thread_local! {
    static SEALED_YOUNG_BYTES: Cell<SealedYoungBytes> = const { Cell::new(SealedYoungBytes::INVALID) };
}

#[cfg(test)]
crate::perry_thread_local! {
    /// Fault injection for the invalidation test: while set, `Arena::set_current`
    /// leaves the cache alone.
    static SKIP_SET_CURRENT_INVALIDATION: Cell<bool> = const { Cell::new(false) };
}

/// Forget the cached sealed bytes. Called when an arena moves `current`.
#[inline]
pub(crate) fn invalidate_sealed_young_bytes() {
    #[cfg(test)]
    if SKIP_SET_CURRENT_INVALIDATION.with(Cell::get) {
        return;
    }
    SEALED_YOUNG_BYTES.with(|cache| cache.set(SealedYoungBytes::INVALID));
}

/// Bytes currently allocated in Eden plus the active survivor from-space.
#[inline]
pub(crate) fn copying_from_space_in_use_bytes() -> usize {
    let current_bytes = synced_current_eden_block_bytes();
    let generation = crate::gc::heap_generation::heap_generation();
    let cached = SEALED_YOUNG_BYTES.with(Cell::get);
    if cached.valid && cached.heap_generation == generation {
        let bytes = cached.sealed + current_bytes;
        debug_assert_eq!(
            bytes,
            copying_from_space_in_use_bytes_walked(),
            "cached from-space occupancy drifted from the block walk: a young block offset \
             or Eden's `current` changed outside a HeapChange scope without \
             `Arena::set_current` (see arena/from_space.rs)"
        );
        return bytes;
    }
    walk_and_store(generation, current_bytes)
}

/// `sync_inline_arena_state`, then the current Eden block's offset. The same
/// write and the same sampling note, through the hot-TLS addresses rather than
/// two `thread_local!` resolutions.
#[inline(always)]
fn synced_current_eden_block_bytes() -> usize {
    // SAFETY: this thread's inline state and nursery arena; no `&mut` borrow of
    // either is live across this call.
    unsafe {
        let inline = &*hot_inline_state();
        let eden = &mut *hot_arena();
        let current = eden.current;
        if !inline.data.is_null() {
            super::alloc_sample::note_inline_sync(eden.blocks[current].offset, inline.offset);
            eden.blocks[current].offset = inline.offset;
        }
        eden.blocks[current].offset
    }
}

#[cold]
#[inline(never)]
fn walk_and_store(generation: u64, current_bytes: usize) -> usize {
    let bytes = copying_from_space_in_use_bytes_walked();
    if !crate::gc::heap_generation::heap_change_open() {
        SEALED_YOUNG_BYTES.with(|cache| {
            cache.set(SealedYoungBytes {
                valid: true,
                heap_generation: generation,
                sealed: bytes - current_bytes,
            })
        });
    }
    bytes
}

/// The O(blocks) sum the cache stands in for. The source of truth.
pub(crate) fn copying_from_space_in_use_bytes_walked() -> usize {
    sync_inline_arena_state();
    let eden = ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        arena.blocks.iter().map(|b| b.offset).sum::<usize>()
    });
    let active = ACTIVE_SURVIVOR.with(|active| active.get());
    let survivor = with_survivor_arena(active, |arena| {
        arena.blocks.iter().map(|b| b.offset).sum::<usize>()
    });
    eden + survivor
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SkipInvalidation;

    impl SkipInvalidation {
        fn new() -> Self {
            SKIP_SET_CURRENT_INVALIDATION.with(|skip| skip.set(true));
            Self
        }
    }

    impl Drop for SkipInvalidation {
        fn drop(&mut self) {
            SKIP_SET_CURRENT_INVALIDATION.with(|skip| skip.set(false));
            invalidate_sealed_young_bytes();
        }
    }

    /// The cached answer if the cache matches, without the debug cross-check,
    /// so the fault-injection test can observe a stale value instead of
    /// panicking inside the read.
    fn cache_hit() -> Option<usize> {
        let current_bytes = synced_current_eden_block_bytes();
        let cached = SEALED_YOUNG_BYTES.with(Cell::get);
        (cached.valid && cached.heap_generation == crate::gc::heap_generation::heap_generation())
            .then(|| cached.sealed + current_bytes)
    }

    fn eden_current() -> usize {
        ARENA.with(|a| unsafe { (*a.get()).current })
    }

    fn eden_block(idx: usize) -> (usize, usize) {
        ARENA.with(|a| {
            let arena = unsafe { &*a.get() };
            let block = &arena.blocks[idx];
            (block.offset, block.size)
        })
    }

    /// Prime the cache on Eden's only block with 64 bytes of headroom, continue
    /// in a fresh block, fill it, and return to the first block through the
    /// allocator's forward scan: `current` ends at the index it was primed at,
    /// while a whole block of bytes landed elsewhere. Every step asserts where
    /// the allocator went, so a change in allocation policy fails the test
    /// instead of making it vacuous.
    fn return_to_the_primed_block() {
        let _no_gc = crate::gc::GcSuppressScope::new();
        // libtest runs each test on a fresh thread, so this is a fresh arena.
        assert_eq!(
            ARENA.with(|a| unsafe { (*a.get()).blocks.len() }),
            1,
            "fixture needs a fresh thread-local Eden"
        );
        let start = eden_current();
        let (used, size) = eden_block(start);
        let _ = crate::arena::arena_alloc(size - used - 64, 8);
        assert_eq!(eden_block(start).0, size - 64);

        let primed = copying_from_space_in_use_bytes();
        assert_eq!(
            cache_hit(),
            Some(primed),
            "the first read must prime the cache"
        );

        // 128 bytes do not fit the 64-byte headroom: a fresh block is installed.
        let _ = crate::arena::arena_alloc(128, 8);
        let other = eden_current();
        assert_ne!(other, start);
        let (other_used, other_size) = eden_block(other);
        let _ = crate::arena::arena_alloc(other_size - other_used, 8);
        assert_eq!(
            eden_block(other).0,
            other_size,
            "the fresh block must be full"
        );

        // Only the primed block can serve 32 bytes now.
        let _ = crate::arena::arena_alloc(32, 8);
        assert_eq!(
            eden_current(),
            start,
            "the forward scan must return to the primed block"
        );
        assert!(copying_from_space_in_use_bytes_walked() >= primed + other_size);
    }

    #[test]
    fn cached_occupancy_matches_the_walk_after_returning_to_the_primed_block() {
        return_to_the_primed_block();
        assert_eq!(
            cache_hit(),
            None,
            "set_current must have invalidated the cache"
        );
        assert_eq!(
            copying_from_space_in_use_bytes(),
            copying_from_space_in_use_bytes_walked()
        );
    }

    /// Fault injection for the invalidation in `Arena::set_current`: without it
    /// the same sequence leaves a cache that matches every key and is short by
    /// the fresh block's bytes.
    #[test]
    fn without_set_current_invalidation_the_cache_goes_stale() {
        let _skip = SkipInvalidation::new();
        return_to_the_primed_block();
        let walked = copying_from_space_in_use_bytes_walked();
        let stale = cache_hit().expect("with invalidation skipped the primed entry still matches");
        assert!(
            stale < walked,
            "the stale cache must miss the fresh block's bytes (stale {stale}, walked {walked})"
        );
    }

    /// The other half of the key: a collection opens a `HeapChange`, and the
    /// generation it advances retires the cached entry.
    #[test]
    fn a_heap_change_scope_retires_the_cached_entry() {
        let _no_gc = crate::gc::GcSuppressScope::new();
        let _ = crate::arena::arena_alloc(256, 8);
        let primed = copying_from_space_in_use_bytes();
        assert_eq!(cache_hit(), Some(primed));
        {
            let _scope = crate::gc::heap_generation::HeapChange::begin(
                crate::gc::heap_generation::HeapChangeKind::Sweep,
            );
            assert_eq!(cache_hit(), None);
            // Reads inside the scope walk and store nothing.
            assert_eq!(copying_from_space_in_use_bytes(), primed);
            assert_eq!(cache_hit(), None);
        }
        assert_eq!(cache_hit(), None);
        assert_eq!(copying_from_space_in_use_bytes(), primed);
        assert_eq!(cache_hit(), Some(primed));
    }
}
