//! The write barrier's leaf-path helpers — the pieces every entry point
//! inlines ahead of the outlined body. A sibling of `barrier/mod.rs` for the
//! 2000-line file-size gate; same module tree, same visibility semantics.

use super::*;

/// The authoritative arena lookup for an address-shaped raw word — the arm of
/// [`decode_heap_addr`] that a subnormal double reaches. Cold and out of line:
/// it is the only part of the decode that is more than a few compares.
#[cold]
#[inline(never)]
pub(in crate::gc) fn decode_raw_pointer_candidate(addr: usize) -> usize {
    if matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Unknown
    ) {
        0
    } else {
        addr
    }
}

/// The barrier's cheapest exit, as an inlinable test the entry points run
/// BEFORE calling into [`write_barrier_decoded_parent`]: an inline slot whose
/// page is the one the dirty-page cache names owes the remembered set nothing
/// (the cache's invariant — see `dirty_page_cache` — is exactly what
/// `remember_old_to_young_inline_slot` would establish for this slot). Hoisted
/// so the second and third push into the same bucket return from a leaf entry,
/// paying neither the outlined function's frame nor either classification.
#[inline(always)]
pub(in crate::gc) fn inline_slot_store_on_cached_dirty_page(
    parent_addr: usize,
    slot_addr: usize,
) -> bool {
    slot_addr != 0
        && slot_addr >= parent_addr
        && super::dirty_page_cache::dirty_old_page_already_marked(
            crate::arena::generation_page_for_addr(slot_addr),
        )
}
