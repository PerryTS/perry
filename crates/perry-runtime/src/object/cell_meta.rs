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
