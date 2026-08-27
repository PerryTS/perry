//! Per-object meta-record accessors and the cell-generic `meta` edge (#8891).
//!
//! Split out of `object/mod.rs` for the 2,000-line file gate.

use super::*;

pub(crate) unsafe fn object_meta_ensure_for_cell(user_ptr: usize) -> Option<*mut ObjectMeta> {
    let slot = cell_meta_slot(user_ptr)?;
    if !(*slot).is_null() {
        return Some(*slot);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(user_ptr as *mut u8);
    let meta = arena_alloc_gc(
        std::mem::size_of::<ObjectMeta>(),
        8,
        crate::gc::GC_TYPE_OBJECT_META,
    ) as *mut ObjectMeta;
    let user_ptr = owner.get_raw_mut_ptr::<u8>() as usize;
    let slot = cell_meta_slot(user_ptr)?;
    if !(*slot).is_null() {
        // A re-entrant path installed one while we allocated; keep it.
        return Some(*slot);
    }
    (*meta).prototype = 0;
    (*meta).attr_key_bits = 0;
    (*meta).accessor_key_bits = 0;
    (*meta).flags = 0;
    (*meta).spill = 0;
    (*meta).private_evaluation_brand = 0;
    (*meta).array_subclass_named_prefix_token = 0;
    (*meta).array_tail_object_hot = 0;
    (*meta).array_subclass_dense_key = 0;
    (*meta).array_subclass_dense_slots = 0;
    (*meta).array_subclass_dense_bounds = 0;
    (*meta).expando = 0;
    // GC_STORE_AUDIT(BARRIERED): header-slot store followed by an object-slot
    // barrier, exactly as `object_meta_ensure` does for an `ObjectHeader`.
    *slot = meta;
    crate::gc::runtime_write_barrier_slot(user_ptr, slot as usize, meta as u64);
    Some(meta)
}

pub(crate) unsafe fn object_meta_ensure(obj: *mut ObjectHeader) -> *mut ObjectMeta {
    if !(*obj).meta.is_null() {
        return (*obj).meta;
    }
    // Root the owner across the allocation: `arena_alloc_gc` can trigger a
    // copied-minor that MOVES `obj`, and the header store below must land
    // in the live copy, not the stale from-space one. Reload through the
    // handle after the allocation. (The fresh `meta` record itself cannot
    // move before the store — no allocation happens in between.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let meta = arena_alloc_gc(
        std::mem::size_of::<ObjectMeta>(),
        8,
        crate::gc::GC_TYPE_OBJECT_META,
    ) as *mut ObjectMeta;
    let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    if !(*obj).meta.is_null() {
        // A GC-triggered re-entrant path installed one meanwhile; keep it
        // (the fresh record above is unreferenced and dies with the cycle).
        return (*obj).meta;
    }
    (*meta).prototype = 0;
    (*meta).attr_key_bits = 0;
    (*meta).accessor_key_bits = 0;
    (*meta).flags = 0;
    (*meta).spill = 0;
    (*meta).private_evaluation_brand = 0;
    (*meta).array_subclass_named_prefix_token = 0;
    (*meta).array_tail_object_hot = 0;
    (*meta).array_subclass_dense_key = 0;
    (*meta).array_subclass_dense_slots = 0;
    (*meta).array_subclass_dense_bounds = 0;
    (*meta).expando = 0;
    // GC_STORE_AUDIT(BARRIERED): meta-record edge is a header-slot store
    // followed by an object-slot barrier, mirroring `set_object_keys_array`.
    (*obj).meta = meta;
    crate::gc::runtime_write_barrier_slot(
        obj as usize,
        &(*obj).meta as *const _ as usize,
        meta as u64,
    );
    meta
}

/// GC slot accessor for the `meta` header edge (#6759 Phase B): a raw-pointer
/// child slot. The GC type table calls this
/// only for `GC_TYPE_OBJECT`; RegExp uses its dedicated slot descriptor.
pub(crate) unsafe fn gc_object_meta_slot(user_ptr: usize) -> Option<*mut u64> {
    if user_ptr == 0 {
        return None;
    }
    let obj = user_ptr as *mut ObjectHeader;
    if (*obj).meta.is_null() {
        return None;
    }
    Some(&mut (*obj).meta as *mut _ as *mut u64)
}
