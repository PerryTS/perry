//! Node-API metadata is plain native data owned by one collecting payload.
//! The private bag edge follows the object's ordinary property holder; no
//! owner address is registered, scanned, rekeyed or pruned.
use super::*;
use crate::native_handle::NativeHandleHeader;
const SLOT: u32 = 3;
const NAME: &str = "Perry.NodeApi.ObjectMetadata";
const TYPE_ID: u64 = {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let bytes = NAME.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
};
static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_metadata,
    stream: None,
};
unsafe extern "C" fn drop_metadata(resource: *mut c_void, _hint: *mut c_void) {
    let metadata = Box::from_raw(resource.cast::<ObjectMetadata>());
    // Only enqueue native records. User callbacks run later in the event pump.
    // The queue can already be destroyed at thread teardown; environment
    // shutdown has drained live metadata before closing its addon images.
    enqueue_metadata_finalizers(*metadata);
}
unsafe fn bag(owner: usize, create: bool) -> *mut crate::array::ArrayHeader {
    crate::object::owned_state_bag::bag(owner, create, SLOT)
}
pub(super) fn cell(owner: usize, create: bool) -> Option<*mut NativeHandleHeader> {
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let bag = bag(owner, create);
        if bag.is_null() {
            return None;
        }
        let value = crate::array::js_array_get(bag, SLOT);
        if value.is_pointer() {
            let cell = value.as_pointer::<NativeHandleHeader>() as *mut NativeHandleHeader;
            if !payload(cell).is_null() {
                return Some(cell);
            }
        }
        if !create {
            return None;
        }
        let resource = Box::into_raw(Box::new(ObjectMetadata::default())).cast();
        let cell = crate::native_handle::native_handle_new_rust_payload(
            resource, TYPE_ID, &VTABLE, NAME, false,
        );
        crate::array::js_array_set(bag, SLOT, JSValue::pointer(cell.cast()));
        Some(cell)
    }
}
pub(super) unsafe fn payload(cell: *mut NativeHandleHeader) -> *mut ObjectMetadata {
    crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID).cast()
}
pub(super) fn with_metadata<R>(
    owner: usize,
    create: bool,
    f: impl FnOnce(&mut ObjectMetadata) -> R,
) -> Option<R> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cell = cell(owner, create)?;
    unsafe { payload(cell).as_mut().map(f) }
}
pub(super) fn weak_link(owner: usize) -> Option<u64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cell = scope.root_raw_mut_ptr(cell(owner, false)?);
    let reference = crate::weakref::js_weakref_new(crate::value::js_nanbox_pointer(
        cell.get_raw_mut_ptr::<NativeHandleHeader>() as i64,
    ));
    Some(crate::value::js_nanbox_pointer(reference as i64).to_bits())
}
pub(super) fn cancel_link(link: u64, id: u64) {
    let cell = crate::weakref::js_weakref_deref(f64::from_bits(link));
    if cell.to_bits() & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        unsafe {
            let cell = (cell.to_bits() & crate::value::POINTER_MASK) as *mut NativeHandleHeader;
            if let Some(metadata) = payload(cell).as_mut() {
                metadata.finalizers.retain(|finalizer| finalizer.id != id);
            }
        }
    }
    cancel_pending(id);
}
pub(super) fn enqueue_all() {
    // Environment shutdown, never a collection scan. Existing malloc custody
    // supplies the live native cells; no per-object owner registry is needed.
    crate::gc::walk_malloc_headers_for_native_shutdown(&mut |header| unsafe {
        if (*header).obj_type != crate::gc::GC_TYPE_NATIVE_HANDLE {
            return;
        }
        let cell = header
            .cast::<u8>()
            .add(crate::gc::GC_HEADER_SIZE)
            .cast::<NativeHandleHeader>();
        if (*cell).type_id != TYPE_ID || (*cell).finalized != 0 {
            return;
        }
        if let Some(metadata) = payload(cell).as_mut() {
            enqueue_metadata_finalizers(std::mem::take(metadata));
        }
    });
}
#[cfg(test)]
pub(crate) fn erase(owner: usize) {
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let bag = bag(owner, false);
        if !bag.is_null() {
            crate::array::js_array_set(bag, SLOT, JSValue::undefined());
        }
    }
}
