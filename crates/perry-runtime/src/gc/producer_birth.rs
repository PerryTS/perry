//! Birth generation of the children a runtime producer allocates for a fresh
//! container it owns.
//!
//! A producer such as `String.prototype.split` sizes its result once and then
//! fills it with children it allocates itself. When that result is large
//! enough to be born old (see `large_object_threshold_for_type`), children
//! born young make every one of its stores an old→young edge: each one is
//! remembered, and every minor collection until the result dies re-scans,
//! copies and finally promotes them all. The 5M-word split of the `words`
//! benchmark spent ~71% of its window in exactly that work and 7.6% in the
//! split itself.
//!
//! The rule is decided ONCE per producer call, from the container's own birth
//! generation: if the container was born old, the children the producer
//! allocates for it are born old too, through the same old-generation birth a
//! large object takes (`arena_alloc_gc_old_born_tenured`). A store of such a
//! child into such a container is old→old and owes the remembered set
//! nothing. There is no cache, no side table and no per-element test.
//!
//! What the old birth inherits, unchanged, from that shared path:
//! * **Marking.** It reads the live birth color (`gc_birth_extra_flags`) after
//!   any collection the allocation itself runs, so a child born while a cycle
//!   is marking is born MARKED (allocate-black), and seeded if it has edges.
//!   During a sweep, birth flags are clear and the old free list is withheld
//!   (`old_free_forget_until_rebuild`), so the sweep cannot reach the birth.
//! * **Movement.** Old-generation pages are never evacuated by a minor
//!   collection; only a full collection's old-page defrag can move them, and
//!   it rewrites every reference, the producer's rooted handles included.
//!
//! The store keeps the incremental mark shade. A child born during a cycle is
//! already marked, so the shade answers at its first flag test; outside a
//! cycle it is one relaxed load of a static.

use super::barrier::incremental_mark_barrier_value;

/// Where the children of one producer call are born. See the module docs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProducerChildBirth {
    /// The container is young (or not arena-resident): children are born in
    /// the nursery and every store keeps its full write barrier.
    Young,
    /// The container was born old: children are born old and stores into the
    /// container skip the old→young remembering.
    Old,
}

impl ProducerChildBirth {
    /// Decide the birth generation for the children of `container_user`.
    ///
    /// `container_user` must be the producer's OWN fresh allocation, examined
    /// before any safepoint and before it can escape. Only stores into that
    /// container may use [`fresh_old_container_store_barrier`]; a container a
    /// collection later promotes was decided `Young` here and keeps the full
    /// barrier on every store.
    #[inline]
    pub(crate) fn for_fresh_container(container_user: usize) -> Self {
        if matches!(
            crate::arena::classify_heap_generation(container_user),
            crate::arena::HeapGeneration::Old
        ) {
            Self::Old
        } else {
            Self::Young
        }
    }
}

/// The barrier for one store into a fresh container decided
/// [`ProducerChildBirth::Old`], of a child the producer allocated for it under
/// that decision. Both ends are old, so there is no old→young slot to
/// remember; only the incremental mark shade remains.
///
/// Any other store (a child the producer did not allocate under the decision,
/// or a container that has escaped) must use the ordinary barrier.
#[inline(always)]
pub(crate) fn fresh_old_container_store_barrier(child_bits: u64) {
    let _ = incremental_mark_barrier_value(child_bits);
}
