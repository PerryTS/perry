//! The Set's store: an ordinary young, movable GC object holding the inline
//! element slots and owning the native index. The header's `store` word is its
//! only edge; the header's `elements` alias is re-derived by the Set's rewrite
//! hook whenever the store moves. A store that dies young releases its native
//! buffers in the copying minor (`arena/owned_stores.rs`), an older one in the
//! ordinary sweep through its finalize hook.
use super::*;

pub(crate) type SetStore =
    crate::codegen_abi::SetStoreHeader<Box<index::SetIndex>, Box<SetCompactionLog>>;

const _: () = {
    assert!(
        std::mem::size_of::<SetStore>()
            == if cfg!(target_pointer_width = "64") {
                48
            } else {
                32
            }
    );
    assert!(std::mem::offset_of!(SetStore, elements) == 8);
};

pub(super) unsafe fn alloc_store(capacity: usize) -> *mut SetStore {
    alloc_store_in::<false>(capacity)
}

/// `OLD` births a tenured store: test fixtures that model an old collection
/// pair it with an old header. Production stores are always born young.
pub(super) unsafe fn alloc_store_in<const OLD: bool>(capacity: usize) -> *mut SetStore {
    let size = std::mem::size_of::<SetStore>()
        .checked_add(elements_layout(capacity).size())
        .filter(|size| *size <= u32::MAX as usize - crate::gc::GC_HEADER_SIZE - 8)
        .unwrap_or_else(|| crate::error::throw_allocation_failed());
    let store = if OLD {
        crate::arena::arena_alloc_gc_old_born_tenured(
            size,
            std::mem::align_of::<SetStore>(),
            crate::gc::GC_TYPE_SET_STORE,
        )
    } else {
        crate::arena::arena_alloc_gc(
            size,
            std::mem::align_of::<SetStore>(),
            crate::gc::GC_TYPE_SET_STORE,
        )
    }
    .cast::<SetStore>();
    crate::arena::note_young_owned_store(store.cast());
    let elements = store
        .cast::<u8>()
        .add(std::mem::size_of::<SetStore>())
        .cast::<f64>();
    // GC_STORE_AUDIT(INIT): Fresh SetStore owns its own inline elements pointer, capacity,used=0 and None indexes; payload is zeroed before return. No existing child edge is copied.
    ptr::write(
        store,
        SetStore {
            _receiver_word: 0,
            elements,
            capacity,
            used: 0,
            index: None,
            compaction: None,
        },
    );
    ptr::write_bytes(elements, 0, capacity);
    store
}

/// Move hook: the copy owns the native index now. Its elements alias points
/// at its own inline payload, and the abandoned original holds no buffer a
/// later sweep or exit walk could release a second time. The forwarding word
/// covers payload bytes 0..8 only (the reserved receiver word).
pub(crate) unsafe fn owned_store_moved_for_gc(old_user: usize, new_user: usize) {
    if old_user == new_user {
        return;
    }
    let new = new_user as *mut SetStore;
    if !(*new).elements.is_null() {
        (*new).elements = (new_user + std::mem::size_of::<SetStore>()) as *mut f64;
    }
    // GC_STORE_AUDIT(POINTER_FREE): ownership moved to the copy.
    (*(old_user as *mut SetStore)).elements = ptr::null_mut();
    crate::arena::note_owned_store_copied(new_user);
}

/// The header's elements alias names its store's inline payload; refreshed by
/// the Set's rewrite hook after a pass rewrote the `store` edge.
pub(crate) unsafe fn refresh_elements_alias(set: *mut SetHeader) {
    if let Some(store) = (*set).store.as_ref() {
        // GC_STORE_AUDIT(ROOT): Only caller is gc::types::run_gc_rewrite_hook SetElements after rewriting traced set.store (types.rs1314). elements re-derived from live store inline payload; no independent edge.
        (*set).elements = store.elements;
    }
}

/// Values are part of the GC allocation; only native buffers need a drop.
/// Idempotent cleanup also serves arena destruction without consulting TLS.
pub(crate) unsafe fn finalize_owned_store(store: *mut u8, accounting: bool) {
    let store = &mut *store.cast::<SetStore>();
    if store.elements.is_null() {
        return;
    }
    if accounting {
        crate::gc::gc_note_external_side_free(store.index.as_ref().map_or(0, |i| i.byte_len()));
    }
    drop(store.index.take());
    drop(store.compaction.take());
    note_test_set_side_deallocation(elements_layout(store.capacity).size());
    store.elements = ptr::null_mut();
    store.capacity = 0;
    store.used = 0;
}

pub(crate) unsafe fn owned_element_slot_range(
    user_ptr: *mut u8,
) -> Option<crate::gc::HeapSlotRange> {
    let store = &*user_ptr.cast::<SetStore>();
    if store.elements.is_null() || store.used > store.capacity {
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        store.elements.cast(),
        store.used,
    ))
}
