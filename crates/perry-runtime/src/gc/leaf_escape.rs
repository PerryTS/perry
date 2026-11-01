//! Escape evidence for young dedicated pointer-free leaves.
//!
//! A leaf collection decides a young leaf without reading the remembered set.
//! A leaf is live if a precise root names it, if a young cell names it (the
//! census in `leaf_service`), or if it ESCAPED: an old or malloc parent
//! received it. Escape is recorded on the leaf's own header, so there is no
//! side table. The bit is `GC_FLAG_HAS_SURVIVED`, whose ordinary meaning
//! (one survival of a copied small cell) cannot apply to a dedicated leaf:
//! every collection promotes or frees a young leaf, so no "survived and still
//! young" leaf state exists. `young_large::finish` clears it on promotion.
//!
//! The test is a value test with no load: every leaf user address satisfies
//! `(addr & 4095) == 8` (one cell, header at a page-aligned extent base). A
//! hit goes to a cold, out-of-line confirm. It runs BEFORE every cached
//! dirty-page exit of the write barrier, because the cache answers without
//! looking at the child.
use super::*;

const LEAF_ADDR_MASK: usize = crate::arena::GENERATION_PAGE_SIZE - 1;

#[cfg(test)]
thread_local! {
    /// Test-only sabotage: drop every escape bit the barrier would set.
    static SKIP_ESCAPE_BIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) struct SkipEscapeBit(bool);
#[cfg(test)]
impl SkipEscapeBit {
    pub(crate) fn arm() -> Self {
        Self(SKIP_ESCAPE_BIT.with(|s| s.replace(true)))
    }
}
#[cfg(test)]
impl Drop for SkipEscapeBit {
    fn drop(&mut self) {
        let prior = self.0;
        SKIP_ESCAPE_BIT.with(|s| s.set(prior));
    }
}

/// The store of `child_addr` into `parent_addr` (a decoded heap address)
/// reached the write barrier.
#[inline(always)]
pub(crate) fn probe_store(parent_addr: usize, child_addr: usize) {
    if child_addr & LEAF_ADDR_MASK == GC_HEADER_SIZE {
        note_store(parent_addr, child_addr);
    }
}

/// A slot word of `parent_addr`, which may be a NaN-boxed pointer or a raw word.
#[inline(always)]
pub(crate) fn probe_store_bits(parent_addr: usize, bits: u64) {
    if (bits & crate::value::POINTER_MASK) as usize & LEAF_ADDR_MASK == GC_HEADER_SIZE {
        note_store_bits(parent_addr, bits);
    }
}

/// A slot of a parent already known to be old or malloc (the remembered-set
/// rebuilds): the child needs only the exact-leaf confirm.
#[inline(always)]
pub(crate) fn probe_old_parent_child(child_addr: usize) {
    if child_addr & LEAF_ADDR_MASK == GC_HEADER_SIZE {
        set_escape(child_addr);
    }
}

#[cold]
#[optimize(size)]
#[inline(never)]
fn note_store(parent_addr: usize, child_addr: usize) {
    // A young parent is read by the census; an image parent is a root.
    if barrier::barrier_parent_needs_remembering(parent_addr) {
        set_escape(child_addr);
    }
}

#[cold]
#[optimize(size)]
#[inline(never)]
fn note_store_bits(parent_addr: usize, bits: u64) {
    let child = barrier::decode_heap_addr(bits);
    if child != 0 {
        note_store(parent_addr, child);
    }
}

#[cold]
#[inline(never)]
fn set_escape(child_addr: usize) {
    #[cfg(test)]
    if SKIP_ESCAPE_BIT.with(|s| s.get()) {
        return;
    }
    if let Some(header) = crate::arena::exact_young_leaf_header(child_addr) {
        unsafe {
            if (*header).gc_flags & GC_FLAG_TENURED == 0 {
                (*header).gc_flags |= GC_FLAG_HAS_SURVIVED;
            }
        }
    }
}

/// Has the leaf at `header` escaped to an old or malloc parent?
#[inline]
pub(crate) unsafe fn has_escaped(header: *const GcHeader) -> bool {
    (*header).gc_flags & GC_FLAG_HAS_SURVIVED != 0
}
