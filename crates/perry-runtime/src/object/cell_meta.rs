//! The cell metadata edge: where an exotic cell keeps its `ObjectMeta`
//! pointer, and whether it has one at all.
//!
//! Split out of `object/mod.rs` for the 2000-line cap. Object, Error, Map,
//! Set, RegExp, Promise and Date answer `cell_meta_slot`; anything else
//! returns `None` and keeps its existing side table.

use super::*;

/// Fetch-or-allocate the per-object meta record. Caller must have already
/// established that `obj` is a live `GC_TYPE_OBJECT` allocation
/// (see `prototype_chain::meta_capable_object`).
/// The metadata edge of ANY cell that has one, addressed uniformly.
///
/// #6759 phase 1 (header unification). Cell types declare their fields
/// independently — there is no shared header prefix — so "does this cell own an
/// `ObjectMeta`?" had no single answer and every caller had to know it was
/// holding an `ObjectHeader` before it could ask. That is why per-object state
/// for the exotic types accumulated in side tables keyed by address instead:
/// there was nowhere on the cell to put it.
///
/// This is the one path the migration needs. It returns `None` for a cell type
/// that has no metadata edge yet, so callers degrade to their existing side
/// table rather than mis-reading another layout's bytes as a pointer.
///
/// Every exotic cell type now answers this: Object, Error, Map, Set, RegExp,
/// Promise and Date. Anything else (Temporal, the typed-array views) returns
/// `None` and keeps its existing storage.
pub(crate) unsafe fn cell_meta_slot(user_ptr: usize) -> Option<*mut *mut ObjectMeta> {
    // Canonical validated read rather than an open-coded magnitude test:
    // `try_read_gc_header` applies `is_plausible_heap_addr` AND rejects
    // small-buffer slab addresses, which are heap-plausible but carry no
    // GcHeader — reading one classifies the previous slab entry's bytes as a
    // type tag.
    let Some(gc_hdr) = crate::value::addr_class::try_read_gc_header(user_ptr) else {
        return None;
    };
    cell_meta_slot_for_header(user_ptr, gc_hdr)
}

/// [`cell_meta_slot`] for a caller that already holds `user_ptr`'s header
/// from `try_read_gc_header`.
///
/// # Safety
/// `gc_hdr` is `try_read_gc_header(user_ptr)`'s answer, read in this scope.
#[inline]
pub(crate) unsafe fn cell_meta_slot_for_header(
    user_ptr: usize,
    gc_hdr: &crate::gc::GcHeader,
) -> Option<*mut *mut ObjectMeta> {
    match gc_hdr.obj_type {
        crate::gc::GC_TYPE_OBJECT => {
            Some(&mut (*(user_ptr as *mut ObjectHeader)).meta as *mut *mut ObjectMeta)
        }
        crate::gc::GC_TYPE_ERROR => {
            Some(&mut (*(user_ptr as *mut crate::error::ErrorHeader)).meta as *mut *mut ObjectMeta)
        }
        crate::gc::GC_TYPE_MAP => {
            Some(&mut (*(user_ptr as *mut crate::map::MapHeader)).meta as *mut *mut ObjectMeta)
        }
        crate::gc::GC_TYPE_SET => {
            Some(&mut (*(user_ptr as *mut crate::set::SetHeader)).meta as *mut *mut ObjectMeta)
        }
        crate::gc::GC_TYPE_REGEXP => {
            Some(&mut (*(user_ptr as *mut crate::regex::RegExpHeader)).meta as *mut *mut ObjectMeta)
        }
        crate::gc::GC_TYPE_PROMISE => {
            Some(&mut (*(user_ptr as *mut crate::promise::Promise)).meta as *mut *mut ObjectMeta)
        }
        crate::gc::GC_TYPE_DATE_CELL => {
            Some(&mut (*(user_ptr as *mut crate::date::DateCell)).meta as *mut *mut ObjectMeta)
        }
        // Anything still without a metadata edge answers absence rather than
        // mis-reading its own layout as a pointer.
        _ => None,
    }
}

/// Does `user_ptr` name a cell that can own an `ObjectMeta`? (Exercised by
/// the error-cell tests; production code asks `cell_meta_slot` directly.)
#[cfg(test)]
pub(crate) unsafe fn cell_has_meta_edge(user_ptr: usize) -> bool {
    cell_meta_slot(user_ptr).is_some()
}

/// The named-property bag for a cell that has no inline slot layout of its own,
/// creating it on first write.
///
/// #6759 phase 1. An `ErrorHeader` (and the other exotic cells) cannot hold
/// named properties inline, so they lived in tables keyed by the owner's
/// ADDRESS — `ERROR_USER_PROPS` and friends — which cost four GC hooks
/// (rekey-on-evacuation, finalize, dead-sweep, root scanner) and carried a
/// standing hazard: a recycled address inherits the previous tenant's
/// properties.
///
/// The bag is an ordinary object hanging off `ObjectMeta.expando`, so it is an
/// ordinary child edge — it moves with its owner, dies with its owner, and
/// keeps ECMA-262 insertion order for free because that is what an object's
/// `keys_array` already does.
pub(crate) unsafe fn cell_expando_ensure(user_ptr: usize) -> Option<*mut ObjectHeader> {
    let meta = object_meta_ensure_for_cell(user_ptr)?;
    if (*meta).expando != 0 {
        return Some(
            crate::value::JSValue::from_bits((*meta).expando).as_pointer::<ObjectHeader>()
                as *mut ObjectHeader,
        );
    }
    // `js_object_alloc` allocates and can move the owner, so re-resolve the
    // meta record from the rooted address afterwards.
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(user_ptr as *mut u8);
    let bag = js_object_alloc(0, 0);
    let user_ptr = owner.get_raw_mut_ptr::<u8>() as usize;
    let meta = object_meta_ensure_for_cell(user_ptr)?;
    if (*meta).expando != 0 {
        return Some(
            crate::value::JSValue::from_bits((*meta).expando).as_pointer::<ObjectHeader>()
                as *mut ObjectHeader,
        );
    }
    let boxed = crate::value::js_nanbox_pointer(bag as i64).to_bits();
    // GC_STORE_AUDIT(BARRIERED): metadata-record slot store + object barrier.
    (*meta).expando = boxed;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).expando as *const _ as usize,
        boxed,
    );
    Some(bag)
}

/// The existing bag, or `None` when the owner never took one. Never allocates,
/// so it is safe on read paths.
pub(crate) unsafe fn cell_expando_get(user_ptr: usize) -> Option<*mut ObjectHeader> {
    let slot = cell_meta_slot(user_ptr)?;
    let meta = *slot;
    if meta.is_null() || (*meta).expando == 0 {
        return None;
    }
    Some(
        crate::value::JSValue::from_bits((*meta).expando).as_pointer::<ObjectHeader>()
            as *mut ObjectHeader,
    )
}
