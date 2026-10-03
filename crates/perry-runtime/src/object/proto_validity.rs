//! Prototype-mutation validity for cached key-add store verdicts.
//!
//! A chain-store verdict records that no inherited setter or non-writable
//! property blocks a key add. It may depend on prototype hops, so the
//! [[Prototype]] installation funnel and chain-store prime mark those objects.
//! A structural mutation of a marked hop, or a semantic property event,
//! advances the global word. The receiver's own ShapeId covers changes to
//! its own keys and prototype edge. A plain value overwrite does not change
//! the chain-store verdict; readers of inherited values load the holder slot
//! under holder-shape guards and do not use this word.
//!
//! The remaining global word can be replaced by per-hop shape facts in the
//! key-add store path (design decision D-A2).

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
    #[cfg_attr(not(test), export_name = "PERRY_PROTO_VALIDITY")]
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

/// Invalidate every cached chain-store verdict. Callers are rare, cold paths by
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
    if let Some(meta) = ensure_meta_for_mark(obj, crate::object::OBJECT_META_FLAG_IS_PROTOTYPE) {
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

/// Serials below this are reserved for INTRINSIC prototypes, assigned at
/// creation (`assign_intrinsic_prototype_serial`) so a receiver kind's base
/// shape can name its prototype before that object exists
/// (`closure::shape::INTRINSIC_SERIAL_*`).
pub(crate) const FIRST_DYNAMIC_PROTOTYPE_SERIAL: u64 = 64;

/// Next prototype serial. 0 means "none assigned"; `1..64` are intrinsic; a
/// `u64` counter cannot be exhausted by any real program.
static PROTOTYPE_SERIAL_NEXT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(FIRST_DYNAMIC_PROTOTYPE_SERIAL);

/// Mark the freshly created intrinsic prototype `obj` and give it the
/// reserved `serial` (below [`FIRST_DYNAMIC_PROTOTYPE_SERIAL`]). Every shape
/// minted for a receiver inheriting from it — a base Function shape minted
/// before `obj` existed, or an ordinary object whose `meta.prototype` is
/// `obj` — then names the same `proto_id`.
///
/// # Safety
/// `obj` is a live `ObjectHeader` with no serial assigned yet.
pub(crate) unsafe fn assign_intrinsic_prototype_serial(obj: usize, serial: u64) {
    debug_assert!(serial != 0 && serial < FIRST_DYNAMIC_PROTOTYPE_SERIAL);
    if let Some(meta) = ensure_meta_for_mark(obj, crate::object::OBJECT_META_FLAG_IS_PROTOTYPE) {
        ANY_PROTOTYPE_MARKED.store(true, Ordering::Relaxed);
        // GC_STORE_AUDIT(POINTER_FREE): scalar classification bit.
        (*meta).flags |= crate::object::OBJECT_META_FLAG_IS_PROTOTYPE;
        debug_assert!(
            (*meta).proto_serial == 0 || (*meta).proto_serial == serial,
            "intrinsic prototype already carried serial {}",
            (*meta).proto_serial
        );
        // GC_STORE_AUDIT(POINTER_FREE): a scalar serial, never a reference.
        (*meta).proto_serial = serial;
    }
}

/// The serial a `[[Prototype]]` of NULL stands for. Distinct from every
/// assigned serial, and from 0 ("none").
pub(crate) const NULL_PROTOTYPE_SERIAL: u64 = u64::MAX;

/// Mark a receiver a shape-keyed read cache must refuse whatever its ShapeId
/// says: `process.env` or an `arguments` object, whose reads are answered by
/// something other than the object's shape. Called from the single writer of
/// each of those two facts (the `process.env` registry insert, the
/// `ObjectMeta::arguments` store), in the same breath as it records them, so
/// "is one" and "carries the flag" are one statement, not two that can drift.
///
/// # Safety
/// As [`mark_object_as_prototype`]: allocates, and may move the owner.
pub(crate) unsafe fn mark_exotic_read_receiver(obj: usize) {
    if obj == 0 || !crate::value::addr_class::is_plausible_heap_addr(obj) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj as *mut crate::object::ObjectHeader);
    let (meta, object) = handle.across_mut::<crate::object::ObjectHeader, _>(|| {
        ensure_meta_for_mark(obj, crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER)
    });
    if let Some(meta) = meta {
        // GC_STORE_AUDIT(POINTER_FREE): scalar classification bit.
        (*meta).flags |= crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER;
        // The flag makes the receiver's [[Prototype]] identity its own
        // (`shapes::object_proto_id`), and the identity is part of the shape:
        // move it to a shape that says so. That makes "its reads are not
        // answered by its shape" a SHAPE fact for a memo keyed on the
        // receiver's ShapeId alone (`method_site::read_holder`).
        let _ = handle.across_mut::<crate::object::ObjectHeader, _>(|| {
            crate::object::shapes::restamp_object_proto_id(object)
        });
    }
}

/// Does this object carry [`mark_exotic_read_receiver`]'s flag? For the
/// `debug_assert`s that keep the flag and the two facts it summarizes from
/// drifting apart; a read cache that has already loaded `meta` tests the bit directly.
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
/// A receiver that does not yet carry `flag` first moves onto a PRIVATE shape
/// lineage (`transition_object_shape_semantics`: a counter-unique semantic
/// generation, which every later append, delete and descriptor transition
/// inherits). "This object is a prototype" and "this object's reads are not
/// answered by its shape" are thereby facts of its SHAPE: no ShapeId a marked
/// object carries is ever carried by an unmarked one, so a shape-keyed site
/// memo primed on an unmarked receiver can never match a marked one, and one
/// that refuses to prime on a marked receiver never learns a marked shape.
/// The transition runs BEFORE the flag is set, so the stamp funnel does not
/// count it as a structural change of a marked prototype: nothing recorded a
/// verdict through this object yet, so no validity word needs to move.
///
/// # Safety
/// `obj` is a live heap address, or 0. This ALLOCATES and may move the owner,
/// so callers must not be holding bare pointers across it.
unsafe fn ensure_meta_for_mark(obj: usize, flag: u64) -> Option<*mut crate::object::ObjectMeta> {
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
    let (meta, object) = handle
        .across_mut::<crate::object::ObjectHeader, _>(|| crate::object::object_meta_ensure(object));
    if meta.is_null() {
        return None;
    }
    if (*meta).flags & flag == 0 {
        // The transition may allocate a descriptor, and so move the owner;
        // the meta record is reached through the owner again afterwards.
        let (_, object) = handle.across_mut::<crate::object::ObjectHeader, _>(|| {
            crate::object::shapes::transition_object_shape_semantics(object)
        });
        let meta = (*object).meta;
        return (!meta.is_null()).then_some(meta);
    }
    Some(meta)
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
/// The chain-store verdict records through MARKED ordinary prototype hops;
/// `object::chain_store` marks each hop it depends on before recording. What such a mutation can
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
/// CHANGED on a MARKED object invalidates every cached chain-store verdict.
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
