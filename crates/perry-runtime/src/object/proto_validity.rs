//! Prototype-mutation validity: one word that stands for "no object anybody
//! inherits from has changed structurally since you looked".
//!
//! # The problem this replaces
//!
//! A mutation of an object that is used as a prototype is invisible to the
//! objects that inherit from it. Perry does not record that an ordinary object
//! is somebody's prototype, so `proto.b = 1` transitions `proto`'s shape and
//! nothing else: no epoch moves, no instance is touched, and a cache that
//! remembers "key `a` lives on `proto` at slot 3" has no way to notice.
//!
//! The inherited-read cache's first answer (#10834) was to re-prove the chain
//! on every hit: one ShapeId compare per hop, up to four dependent loads
//! through prototype objects that are usually cold. That is correct, and it
//! has two costs. It is proportional to the DEPTH of the chain, so a deep
//! chain pays for its depth on every read; and, decisively for the emitted
//! sequence this campaign is aiming at, it is a LOOP — compiled code at a
//! property-read site cannot emit a variable number of compares, so as long as
//! the chain check is per-hop the hit can only live behind a call.
//!
//! # The mechanism
//!
//! This is V8's prototype validity cell, collapsed to one global counter:
//!
//! 1. An object is MARKED (`OBJECT_META_FLAG_IS_PROTOTYPE` in its
//!    `ObjectMeta`) by the `[[Prototype]]` INSTALL funnel — every link kind in
//!    `prototype_chain::object_set_static_prototype_impl`, plus
//!    `class_prototype_object_root_store`. The inherited-read cache does NOT
//!    mark: it REFUSES to record a hop that is not already marked.
//!
//!    The cache's prime is the SAFETY NET, not the authority: when it meets an
//!    unmarked hop it marks that hop and ABANDONS the walk without recording
//!    anything, so the next read of the pair primes normally. Coverage is
//!    therefore self-healing — an install route this funnel misses costs one
//!    declined read, not a permanent loss — while the invariant that matters
//!    still holds absolutely: **no entry is ever recorded through a hop that
//!    was not already marked before the walk began.**
//!
//!    That polarity is the point. The first version of this flag was set BY
//!    the prime, for the hop it was about to record, which made the coverage
//!    argument trivial but meant any way of losing a mark produced a STALE
//!    VALUE. It lived in `GcHeader::_reserved` bit 13 — which
//!    `layout::set_layout_state` CLEARS on transitions that have nothing to do
//!    with prototypes, so marks were erasable and the failure was invisible.
//!    Both halves of that are fixed here: the flag moved to `ObjectMeta`,
//!    where nothing else writes it, and the prime no longer records through a
//!    hop it marked in the same breath.
//! 2. Any STRUCTURAL mutation of a marked object bumps [`proto_validity`].
//!    Structural means "the shape word changed": key add, key delete,
//!    descriptor install, attribute change, `setPrototypeOf`. Every one of
//!    those publishes through
//!    `shapes::stamp_object_shape_id_with_carrier_note`, the runtime's single
//!    structural-mutation publication funnel, where [`note_object_shape_stamped`]
//!    sits.
//! 3. The same counter is bumped by `prop_plan::prop_plan_epoch_bump`, so it
//!    also stands for everything the semantic property epoch stands for
//!    (descriptor installs and clears, `delete`, per-instance prototype
//!    recording, class-prototype-object registration, parent-static linking).
//!    A descriptor install or clear, or a `delete`, bumps it only when its
//!    owner can be a hop of a recorded chain — a MARKED ordinary object, or
//!    one that cannot be classified ([`mutation_owner_may_be_a_recorded_hop`]).
//!    Every consumer records through marked hops only, and the receiver's own
//!    such mutations transition its ShapeId, so an unmarked object's
//!    descriptors cannot change a recorded answer. Without that gate every
//!    `Function.prototype.bind` (which names its result through descriptor
//!    installs) invalidated every cached verdict in the process. Folding the two
//!    into one word is what lets a cached entry re-prove itself with ONE load
//!    and ONE compare instead of two of each.
//!
//! A plain value store to an existing key deliberately does NOT invalidate: a
//! cache entry records (holder, slot) and LOADS the value on every hit, so a
//! new value is seen without any invalidation at all.
//!
//! # Why the counter is global, and what that costs
//!
//! Per-prototype validity would need a word per prototype object and a load of
//! it per hop — which is the per-hop walk again, one indirection shallower. A
//! single global word is one load, covers a chain of ANY depth, and is the
//! only form an emitted inline check can use.
//!
//! What it buys in over-invalidation is real but small, because the bump is
//! gated on the mark: mutating an ordinary object — the overwhelming majority
//! of all mutation — bumps nothing. Only mutating an object that something
//! actually inherits from does, and then it invalidates every cached inherited
//! read rather than the ones that name it. Measured (`/root/pv`, one
//! structural mutation of an UNRELATED prototype per 64 inherited reads, which
//! a per-prototype design would not invalidate at all): see the PR body.
//!
//! # Cost when nothing is marked
//!
//! [`note_object_shape_stamped`] is on the structural-mutation path, which a
//! program that builds objects in a loop runs hot. Until something is marked
//! it is one relaxed `bool` load and a predictable not-taken branch: the
//! latch is only ever set by a prime of the inherited-read cache, so a program
//! with no inherited reads never pays the header load.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

per_test_global! {
    // #10944: a test asserts this counter's value, and libtest runs tests
    // in one process — any sibling touching the same path made the
    // assertion fail by one. `per_test_global!` gives each test thread its
    // own instance in a TEST build and expands to the plain `static`,
    // byte for byte, outside one.
    /// Monotonic counter standing for "nothing structural has changed on any
    /// object somebody inherits from, and no semantic property event has
    /// happened". Starts at 1 so a zeroed cache entry never matches.
    /// Read by emitted code (the key-add hit, `perry-codegen`'s
    /// `put_value_store_ic.rs`) as `@PERRY_PROTO_VALIDITY`.
    #[export_name = "PERRY_PROTO_VALIDITY"]
    static PROTO_VALIDITY: AtomicU64 = AtomicU64::new(1);
}

per_test_global! {
    // #10944: a test asserts this counter's value, and libtest runs tests
    // in one process — any sibling touching the same path made the
    // assertion fail by one. `per_test_global!` gives each test thread its
    // own instance in a TEST build and expands to the plain `static`,
    // byte for byte, outside one.
    /// Has any object ever been marked as a prototype? Until it has,
    /// [`note_object_shape_stamped`] cannot possibly need to bump, so it does not
    /// read the object's `GcHeader` at all.
    static ANY_PROTOTYPE_MARKED: AtomicBool = AtomicBool::new(false);
}

/// The current validity word. One relaxed load.
#[inline]
pub(crate) fn proto_validity() -> u64 {
    PROTO_VALIDITY.load(Ordering::Relaxed)
}

/// Invalidate every cached inherited read. Callers are rare, cold paths by
/// construction: a structural mutation of an object used as a prototype, or a
/// semantic property event.
#[inline]
pub(crate) fn bump_proto_validity() {
    PROTO_VALIDITY.fetch_add(1, Ordering::Relaxed);
}

/// Whether anything has been marked. Exposed for the shape hook's gate and
/// for tests.
#[inline]
pub(crate) fn any_prototype_marked() -> bool {
    ANY_PROTOTYPE_MARKED.load(Ordering::Relaxed)
}

/// Mark `obj` as an object somebody inherits from, so a later structural
/// mutation of it invalidates cached inherited reads.
///
/// Set-only, and monotone for an allocation: a prototype that stops being one
/// keeps the mark and costs one extra counter bump per structural mutation,
/// which is conservative in the safe direction. A FRESH allocation's
/// `_reserved` is zero, so an address recycled by the collector does not
/// inherit the mark of whatever lived there before.
///
/// The latch is stored BEFORE the bit, so any thread that can observe the bit
/// can already observe the latch, and a mutation that reads the latch as
/// `false` is one that happened before this object could be in any cache
/// entry — where the chain walk itself, not the counter, is what sees it.
///
/// # Safety
/// `obj` must be a live heap address whose `GcHeader` precedes it; callers
/// have already read that header to classify the object.
///
/// Returns the prototype's stable serial (#10868 lever iv), assigning one on
/// first mark. It is read from the meta pointer `ensure_meta_for_mark` returns
/// AFTER its allocation, so a caller can carry it as a plain `u64` without
/// re-reading through a pointer the allocation may have moved.
#[inline]
pub(crate) unsafe fn mark_object_as_prototype(obj: usize) -> Option<u64> {
    if let Some(meta) = ensure_meta_for_mark(obj) {
        ANY_PROTOTYPE_MARKED.store(true, Ordering::Relaxed);
        // GC_STORE_AUDIT(POINTER_FREE): scalar classification bit in the meta
        // record's flags word, never a heap reference.
        (*meta).flags |= crate::object::OBJECT_META_FLAG_IS_PROTOTYPE;
        // GC_STORE_AUDIT(POINTER_FREE): a scalar serial, never a reference.
        if (*meta).proto_serial == 0 {
            (*meta).proto_serial = PROTOTYPE_SERIAL_NEXT.fetch_add(1, Ordering::Relaxed);
        }
        let serial = (*meta).proto_serial;
        #[cfg(feature = "shape-mint-diag")]
        crate::object::shape_mint_census::note_event("object marked prototype");
        return Some(serial);
    }
    None
}

/// Next prototype serial. Starts at 1 so 0 can mean "none assigned"; a `u64`
/// counter cannot be exhausted by any real program.
static PROTOTYPE_SERIAL_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The serial a `[[Prototype]]` of NULL stands for. Distinct from every
/// assigned serial, and from 0 ("none").
pub(crate) const NULL_PROTOTYPE_SERIAL: u64 = u64::MAX;

/// Mark a receiver a shape-keyed read cache must refuse whatever its ShapeId
/// says: `process.env` or an `arguments` object, whose reads are answered by
/// something other than the object's shape. Called from the single writer of
/// each of those two registries, in the same breath as the insert, so "in the
/// registry" and "carries the flag" are one statement, not two that can drift.
///
/// # Safety
/// As [`mark_object_as_prototype`]: allocates, and may move the owner.
pub(crate) unsafe fn mark_exotic_read_receiver(obj: usize) {
    if let Some(meta) = ensure_meta_for_mark(obj) {
        // GC_STORE_AUDIT(POINTER_FREE): scalar classification bit.
        (*meta).flags |= crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER;
    }
}

/// Does this object carry [`mark_exotic_read_receiver`]'s flag? For the
/// `debug_assert`s that keep the flag and the two registries from drifting
/// apart; a read cache that has already loaded `meta` tests the bit directly.
///
/// # Safety
/// `obj` is a live heap address, or 0.
#[inline]
pub(crate) unsafe fn object_is_exotic_read_receiver(obj: usize) -> bool {
    meta_flag_is_set(obj, crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER)
}

/// Get (allocating if needed) the meta record that carries this module's two
/// classification flags.
///
/// The flags live in `ObjectMeta::flags`, NOT in `GcHeader::_reserved`. The
/// header word has no free bits, and the two that looked free are owned by
/// `gc/layout.rs`, which CLEARS them on layout-state transitions that have
/// nothing to do with either fact — a flag placed there is silently ERASED and
/// its reader then answers `false` for an object the writer marked. The
/// complete map is on `gc::OBJ_FLAG_RESERVED_BIT_MAP_SEE_DOC`.
///
/// # Safety
/// `obj` is a live heap address, or 0. This ALLOCATES and may move the owner,
/// so callers must not be holding bare pointers across it.
unsafe fn ensure_meta_for_mark(obj: usize) -> Option<*mut crate::object::ObjectMeta> {
    if obj == 0 || !crate::value::addr_class::is_plausible_heap_addr(obj) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(obj)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let object = obj as *mut crate::object::ObjectHeader;
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(object);
    let (meta, _obj) = handle
        .across_mut::<crate::object::ObjectHeader, _>(|| crate::object::object_meta_ensure(object));
    if meta.is_null() {
        None
    } else {
        Some(meta)
    }
}

/// # Safety
/// `obj` is a live heap address, or 0.
#[inline]
unsafe fn meta_flag_is_set(obj: usize, flag: u64) -> bool {
    if obj == 0 || !crate::value::addr_class::is_plausible_heap_addr(obj) {
        return false;
    }
    match crate::value::addr_class::try_read_gc_header(obj) {
        Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {
            let meta = (*(obj as *const crate::object::ObjectHeader)).meta;
            !meta.is_null() && (*meta).flags & flag != 0
        }
        _ => false,
    }
}

/// Is `obj` marked? Only meaningful for a `GC_TYPE_OBJECT`.
///
/// # Safety
/// As [`mark_object_as_prototype`].
#[inline]
pub(crate) unsafe fn object_is_marked_prototype(obj: usize) -> bool {
    meta_flag_is_set(obj, crate::object::OBJECT_META_FLAG_IS_PROTOTYPE)
}

/// Can a descriptor install or clear, or a `delete`, on `owner` change what
/// a cached chain verdict answers?
///
/// Every consumer of [`proto_validity`] records a verdict only through
/// prototype hops that are MARKED ordinary objects (the inherited-read cache
/// refuses an unmarked or non-object hop; `object::chain_store` marks every
/// hop its verdict depends on before it records). What such a mutation can
/// change on its RECEIVER is covered by the receiver's own ShapeId, which
/// every descriptor install, clear and delete transitions. So the mutation
/// matters to a verdict only when `owner` can be one of those hops: a marked
/// ordinary object, or something this function cannot classify. An unmarked
/// ordinary object is not a hop of any recorded chain yet — linking it into
/// one marks it first — and a function object is never recorded as a hop.
///
/// This is what keeps `Function.prototype.bind` (which names every bound
/// function through descriptor installs) from invalidating every cached
/// inherited verdict in the process on every call.
///
/// # Safety
/// `owner` is a live heap address, or 0.
pub(crate) unsafe fn mutation_owner_may_be_a_recorded_hop(owner: usize) -> bool {
    if owner == 0 || !crate::value::addr_class::is_plausible_heap_addr(owner) {
        return true;
    }
    match crate::value::addr_class::try_read_gc_header(owner) {
        Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {
            object_is_marked_prototype(owner)
        }
        Some(header) if header.obj_type == crate::gc::GC_TYPE_CLOSURE => false,
        _ => true,
    }
}

/// The hook in the structural-mutation publication funnel
/// (`shapes::stamp_object_shape_id_with_carrier_note`): a shape word that
/// CHANGED on a MARKED object invalidates every cached inherited read.
///
/// `previous == published` is the re-publication of an unchanged descriptor —
/// the read-side `lookup_ways` restamp, and a birth stamp that agrees with the
/// allocator — and changes nothing a cached entry claims.
///
/// # Safety
/// `obj` is the receiver the funnel has just stamped, so its `GcHeader`
/// precedes it.
#[inline]
pub(crate) unsafe fn note_object_shape_stamped(obj: usize, previous: u32, published: u32) {
    if previous == published || !any_prototype_marked() {
        return;
    }
    if object_is_marked_prototype(obj) {
        bump_proto_validity();
    }
}

#[cfg(test)]
#[path = "proto_validity_tests.rs"]
mod tests;
