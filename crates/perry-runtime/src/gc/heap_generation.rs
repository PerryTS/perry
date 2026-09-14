//! A per-thread heap generation that advances whenever heap memory is freed or
//! moved.
//!
//! An address observed while the generation reads `G` still names the same
//! object for as long as the generation still reads `G`: freeing that object
//! (so its address can be handed to a new allocation) or relocating it both
//! advance the generation first. RegExp's cross-call search position (#10164)
//! uses this to recognise that a string it searched on a previous call is the
//! same string, without adding a traced edge or any per-object state.
//!
//! # The funnel
//!
//! Every free or move of heap memory runs inside a [`HeapChange`] scope, which
//! advances the generation when it opens and again when it closes. Opening
//! covers observers that recorded an address before the event; closing covers
//! any observer that recorded one while the event was running (a JS callback
//! reached from inside a collection). Scopes may nest.
//!
//! The primitives that make an object's memory reusable or give it a new
//! address call [`debug_assert_heap_change_open`]: the arena region and block
//! resets, the old-generation free-list rebuild, dead-object reclaim, the
//! malloc sweep, promotion's young reset, evacuation (copying minor, tenured
//! nursery, selected old pages), forwarding-stub release and `gc_realloc`.
//! A free or move reached outside every scope panics in debug builds, so a new
//! path cannot silently bypass the generation.
//!
//! Recycling a block that is already empty (the block pool, the from-space
//! quarantine ring, an arena dropped at thread exit) needs no scope: the
//! objects that lived there were freed or moved by an event that already
//! advanced the generation, and nothing has been allocated there since.

use std::cell::Cell;

crate::perry_thread_local! {
    static HEAP_GENERATION: Cell<u64> = const { Cell::new(0) };
    static OPEN_HEAP_CHANGES: Cell<u32> = const { Cell::new(0) };
}

/// What kind of event a [`HeapChange`] scope covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum HeapChangeKind {
    /// A copying (evacuating) minor: from-space reset, young moves, promotion.
    CopyingMinor = 0,
    /// A non-moving or full sweep step: arena resets, malloc frees, dead-object
    /// reclaim, free-list rebuild.
    Sweep = 1,
    /// An incremental reclaim step of a budgeted cycle.
    Reclaim = 2,
    /// Minor-prelude evacuation of tenured nursery objects and forwarding-stub
    /// release.
    Evacuation = 3,
    /// Old-generation compaction or defragmentation.
    Compaction = 4,
    /// Promotion of the young generation outside a copying minor.
    Promotion = 5,
    /// A malloc-tracked object reallocated to a new address.
    Realloc = 6,
}

#[cfg(test)]
const HEAP_CHANGE_KINDS: usize = 7;

#[cfg(test)]
crate::perry_thread_local! {
    static HEAP_CHANGES_BY_KIND: Cell<[u64; HEAP_CHANGE_KINDS]> =
        const { Cell::new([0; HEAP_CHANGE_KINDS]) };
}

/// This thread's current heap generation.
#[inline]
pub(crate) fn heap_generation() -> u64 {
    HEAP_GENERATION.with(Cell::get)
}

#[inline]
fn advance() {
    // `try_with`: a scope can close while thread-locals are being destroyed.
    let _ = HEAP_GENERATION.try_with(|g| g.set(g.get().wrapping_add(1)));
}

/// A region of code that may free or move heap memory. See the module docs.
#[must_use = "a HeapChange covers only the code that runs while it is held"]
pub(crate) struct HeapChange {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl HeapChange {
    #[inline]
    pub(crate) fn begin(kind: HeapChangeKind) -> Self {
        advance();
        let _ = OPEN_HEAP_CHANGES.try_with(|n| n.set(n.get() + 1));
        #[cfg(test)]
        let _ = HEAP_CHANGES_BY_KIND.try_with(|c| {
            let mut counts = c.get();
            counts[kind as usize] += 1;
            c.set(counts);
        });
        #[cfg(not(test))]
        let _ = kind;
        Self {
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Drop for HeapChange {
    #[inline]
    fn drop(&mut self) {
        let _ = OPEN_HEAP_CHANGES.try_with(|n| n.set(n.get().saturating_sub(1)));
        advance();
    }
}

/// Is a [`HeapChange`] scope open on this thread? A cache keyed on the heap
/// generation must not store while one is, because the generation does not
/// advance inside a scope.
#[inline]
pub(crate) fn heap_change_open() -> bool {
    OPEN_HEAP_CHANGES.try_with(Cell::get).unwrap_or(1) > 0
}

/// Called by every primitive that frees or moves heap memory.
#[inline]
#[track_caller]
pub(crate) fn debug_assert_heap_change_open() {
    #[cfg(debug_assertions)]
    {
        let open = OPEN_HEAP_CHANGES.try_with(Cell::get).unwrap_or(1);
        assert!(
            open > 0,
            "heap memory freed or moved outside a HeapChange scope; the heap generation \
             would not advance and an address-keyed observer could confuse two objects"
        );
    }
}

/// How many scopes of `kind` have opened on this thread.
#[cfg(test)]
pub(crate) fn heap_changes_of_kind(kind: HeapChangeKind) -> u64 {
    HEAP_CHANGES_BY_KIND.with(|c| c.get()[kind as usize])
}
