//! Private GC arrays for per-Promise combinator and reaction records.
//! The metadata edge owns a fixed four-slot bag; each slot owns one flat list.
use super::*;

pub(super) const ALL_STATES: u32 = 0;
pub(super) const LISTENERS: u32 = 1;
pub(super) const OVERFLOW: u32 = 2;

pub(super) unsafe fn state_list(
    promise: *mut Promise,
    slot: u32,
) -> *mut crate::array::ArrayHeader {
    if promise.is_null() || (*promise).meta.is_null() {
        return ptr::null_mut();
    }
    let bag = (*(*promise).meta).owned_state as *const crate::array::ArrayHeader;
    if bag.is_null() {
        return ptr::null_mut();
    }
    let bits = crate::array::js_array_get(bag, slot).bits();
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        (bits & crate::value::POINTER_MASK) as *mut crate::array::ArrayHeader
    } else {
        ptr::null_mut()
    }
}

pub(super) unsafe fn push_record(promise: *mut Promise, slot: u32, words: &[f64]) {
    if slot != 3 {
        super::operation_owner::observe(promise);
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let meta = crate::object::object_meta_ensure_for_cell(
        promise as usize,
        crate::object::MetaReason::OwnedState,
    )
    .expect("Promise metadata edge");
    if (*meta).owned_state == 0 {
        let bag = crate::array::js_array_alloc_with_length(4);
        crate::gc::runtime_store_gc_heap_word_slot(
            meta as usize,
            std::ptr::addr_of_mut!((*meta).owned_state) as usize,
            bag as u64,
        );
    }
    let bag = (*meta).owned_state as *mut crate::array::ArrayHeader;
    let mut list = state_list(promise, slot);
    if list.is_null() {
        list = crate::array::js_array_alloc(words.len() as u32);
    }
    for &word in words {
        list = crate::array::js_array_push(list, crate::value::JSValue::from_bits(word.to_bits()));
    }
    crate::array::js_array_set(bag, slot, crate::value::JSValue::pointer(list.cast()));
}

pub(super) unsafe fn take_list(promise: *mut Promise, slot: u32) -> *mut crate::array::ArrayHeader {
    let list = state_list(promise, slot);
    if !list.is_null() {
        crate::array::js_array_set(
            (*(*promise).meta).owned_state as *mut crate::array::ArrayHeader,
            slot,
            crate::value::JSValue::undefined(),
        );
    }
    list
}

pub(super) fn pointer_word<T>(ptr: *const T) -> f64 {
    f64::from_bits(crate::value::POINTER_TAG | ptr as u64)
}

/// Capture native token custody in an old collecting holder. Empty snapshots
/// need no cell and retain their explicit empty-context meaning when decoded.
pub(super) fn context_word(snapshot: AsyncContextSnapshot) -> f64 {
    if snapshot.is_empty() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        pointer_word(crate::async_context::alloc_snapshot_holder(snapshot))
    }
}

pub(super) unsafe fn context_at(
    list: *const crate::array::ArrayHeader,
    index: u32,
) -> AsyncContextSnapshot {
    let bits = crate::array::js_array_get(list, index).bits();
    if bits == crate::value::TAG_UNDEFINED {
        AsyncContextSnapshot::default()
    } else {
        (*((bits & crate::value::POINTER_MASK) as *const crate::async_context::AsyncSnapshotHolder))
            .snapshot
            .clone()
    }
}

#[cfg(test)]
pub(crate) unsafe fn record_count(promise: *mut Promise, slot: u32, width: u32) -> usize {
    let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(promise as usize)
    else {
        return 0;
    };
    if (*header.as_ptr()).obj_type != crate::gc::GC_TYPE_PROMISE
        || (*header.as_ptr()).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return 0;
    }
    let list = state_list(promise, slot);
    if list.is_null() {
        0
    } else {
        (*list).length as usize / width as usize
    }
}
