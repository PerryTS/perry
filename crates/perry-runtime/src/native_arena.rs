//! Hidden runtime intrinsics for native-owned typed-array views.
//!
//! These are internal `__perry_native_arena_*` intrinsics, not a public API.
//! TypeScript receives opaque owner/view handles; the raw byte pointer stays
//! in runtime-owned GC payloads.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::cell::RefCell;
use std::ptr;

use crate::typedarray::{self, TypedArrayHeader};

const NATIVE_ARENA_DISPOSED_MESSAGE: &[u8] = b"NativeArena has been disposed";

#[repr(C)]
pub struct NativeArenaOwnerHeader {
    pub byte_length: u64,
    pub data: *mut u8,
    pub generation: u32,
    pub disposed: u8,
    pub _pad: [u8; 3],
}

#[repr(C)]
pub struct NativeTypedViewHeader {
    // Prefix must match TypedArrayHeader exactly.
    pub length: u32,
    pub capacity: u32,
    pub kind: u8,
    pub elem_size: u8,
    pub _pad: [u8; 6],

    pub owner: *mut NativeArenaOwnerHeader,
    pub data: *mut u8,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub generation: u32,
    pub _pad2: u32,
}

#[repr(C)]
pub struct NativePodViewHeader {
    pub owner: *mut NativeArenaOwnerHeader,
    pub data: *mut u8,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub record_count: u64,
    pub stride: u32,
    pub alignment: u32,
    pub layout_id: u64,
    pub generation: u32,
    pub _pad: u32,
}

thread_local! {
    static OWNER_REGISTRY: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
    static VIEW_REGISTRY: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
    static POD_VIEW_REGISTRY: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

#[inline]
fn strip_nanbox(raw: u64) -> usize {
    if (raw >> 48) >= 0x7FF8 {
        (raw & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        raw as usize
    }
}

#[inline]
fn byte_layout(byte_length: u64) -> Layout {
    let size = (byte_length as usize).max(1);
    Layout::from_size_align(size, 8).expect("native arena byte layout")
}

#[cold]
fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cold]
fn throw_range_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cold]
pub(crate) fn throw_native_arena_disposed() -> ! {
    throw_type_error(NATIVE_ARENA_DISPOSED_MESSAGE)
}

fn register_owner(owner: *mut NativeArenaOwnerHeader) {
    OWNER_REGISTRY.with(|r| {
        r.borrow_mut().insert(owner as usize);
    });
}

fn unregister_owner(owner: *mut NativeArenaOwnerHeader) {
    OWNER_REGISTRY.with(|r| {
        r.borrow_mut().remove(&(owner as usize));
    });
}

fn register_view(view: *mut NativeTypedViewHeader) {
    VIEW_REGISTRY.with(|r| {
        r.borrow_mut().insert(view as usize);
    });
    typedarray::register_typed_array(view as *const TypedArrayHeader, unsafe { (*view).kind });
}

fn unregister_view(view: *mut NativeTypedViewHeader) {
    VIEW_REGISTRY.with(|r| {
        r.borrow_mut().remove(&(view as usize));
    });
    typedarray::unregister_typed_array(view as *const TypedArrayHeader);
}

fn register_pod_view(view: *mut NativePodViewHeader) {
    POD_VIEW_REGISTRY.with(|r| {
        r.borrow_mut().insert(view as usize);
    });
}

fn unregister_pod_view(view: *mut NativePodViewHeader) {
    POD_VIEW_REGISTRY.with(|r| {
        r.borrow_mut().remove(&(view as usize));
    });
}

#[inline]
fn owner_is_registered(owner: *const NativeArenaOwnerHeader) -> bool {
    OWNER_REGISTRY.with(|r| r.borrow().contains(&(owner as usize)))
}

#[inline]
pub(crate) fn is_native_typed_view(ta: *const TypedArrayHeader) -> bool {
    VIEW_REGISTRY.with(|r| r.borrow().contains(&(ta as usize)))
}

#[inline]
pub(crate) fn is_native_pod_view(view: *const NativePodViewHeader) -> bool {
    POD_VIEW_REGISTRY.with(|r| r.borrow().contains(&(view as usize)))
}

#[inline]
pub(crate) unsafe fn native_view_from_typed_array(
    ta: *const TypedArrayHeader,
) -> *const NativeTypedViewHeader {
    ta as *const NativeTypedViewHeader
}

#[inline]
pub(crate) unsafe fn native_view_from_typed_array_mut(
    ta: *mut TypedArrayHeader,
) -> *mut NativeTypedViewHeader {
    ta as *mut NativeTypedViewHeader
}

unsafe fn clean_owner_ptr(raw: u64) -> *mut NativeArenaOwnerHeader {
    let addr = strip_nanbox(raw);
    if addr < 0x1000 {
        return ptr::null_mut();
    }
    let owner = addr as *mut NativeArenaOwnerHeader;
    if owner_is_registered(owner) {
        owner
    } else {
        ptr::null_mut()
    }
}

pub(crate) unsafe fn validate_owner_alive(owner: *mut NativeArenaOwnerHeader) {
    if owner.is_null() || !owner_is_registered(owner) {
        throw_type_error(b"Invalid NativeArena owner");
    }
    if (*owner).disposed != 0 {
        throw_native_arena_disposed();
    }
}

pub(crate) unsafe fn validate_view_alive(view: *const NativeTypedViewHeader) {
    if view.is_null() || !is_native_typed_view(view as *const TypedArrayHeader) {
        return;
    }
    let owner = (*view).owner;
    validate_owner_alive(owner);
    if (*view).generation != (*owner).generation {
        throw_native_arena_disposed();
    }
}

pub(crate) unsafe fn validate_pod_view_alive(view: *const NativePodViewHeader) {
    if view.is_null() || !is_native_pod_view(view) {
        throw_type_error(b"Invalid NativePodView");
    }
    let owner = (*view).owner;
    validate_owner_alive(owner);
    if (*view).generation != (*owner).generation {
        throw_native_arena_disposed();
    }
}

#[inline]
pub(crate) unsafe fn native_view_data_ptr(ta: *const TypedArrayHeader) -> *const u8 {
    let view = native_view_from_typed_array(ta);
    validate_view_alive(view);
    (*view).data as *const u8
}

#[inline]
pub(crate) unsafe fn native_view_data_ptr_mut(ta: *mut TypedArrayHeader) -> *mut u8 {
    let view = native_view_from_typed_array_mut(ta);
    validate_view_alive(view);
    (*view).data
}

unsafe fn dispose_owner(owner: *mut NativeArenaOwnerHeader) {
    if owner.is_null() || (*owner).disposed != 0 {
        return;
    }
    let data = (*owner).data;
    if !data.is_null() {
        dealloc(data, byte_layout((*owner).byte_length));
        (*owner).data = ptr::null_mut();
    }
    (*owner).disposed = 1;
    (*owner).generation = (*owner).generation.wrapping_add(1);
}

#[no_mangle]
pub extern "C" fn js_native_arena_alloc(byte_length: i64) -> *mut NativeArenaOwnerHeader {
    if byte_length < 0 {
        throw_range_error(b"NativeArena byteLength is out of range");
    }
    let byte_length = byte_length as u64;
    let data = if byte_length == 0 {
        ptr::null_mut()
    } else {
        unsafe {
            let raw = alloc_zeroed(byte_layout(byte_length));
            if raw.is_null() {
                panic!("js_native_arena_alloc OOM");
            }
            raw
        }
    };
    let owner = crate::gc::gc_malloc(
        std::mem::size_of::<NativeArenaOwnerHeader>(),
        crate::gc::GC_TYPE_NATIVE_ARENA_OWNER,
    ) as *mut NativeArenaOwnerHeader;
    unsafe {
        (*owner).byte_length = byte_length;
        (*owner).data = data;
        (*owner).generation = 1;
        (*owner).disposed = 0;
        (*owner)._pad = [0; 3];
    }
    register_owner(owner);
    owner
}

#[no_mangle]
pub extern "C" fn js_native_arena_view(
    owner_raw: u64,
    kind: i32,
    byte_offset: i64,
    length: i64,
) -> *mut NativeTypedViewHeader {
    if byte_offset < 0 || length < 0 {
        throw_range_error(b"NativeArena view is out of bounds");
    }
    let kind = kind as u8;
    if kind > typedarray::KIND_BIGUINT64 {
        throw_range_error(b"NativeArena view kind is invalid");
    }
    let elem_size = typedarray::elem_size_for_kind(kind) as u64;
    let byte_offset = byte_offset as u64;
    let length = length as u64;
    let byte_length = length
        .checked_mul(elem_size)
        .unwrap_or_else(|| throw_range_error(b"NativeArena view is out of bounds"));
    if !byte_offset.is_multiple_of(elem_size) {
        throw_range_error(b"NativeArena view byteOffset is unaligned");
    }
    let owner = unsafe { clean_owner_ptr(owner_raw) };
    unsafe {
        validate_owner_alive(owner);
        let end = byte_offset
            .checked_add(byte_length)
            .unwrap_or_else(|| throw_range_error(b"NativeArena view is out of bounds"));
        if end > (*owner).byte_length || length > u32::MAX as u64 {
            throw_range_error(b"NativeArena view is out of bounds");
        }
        let view = crate::gc::gc_malloc(
            std::mem::size_of::<NativeTypedViewHeader>(),
            crate::gc::GC_TYPE_NATIVE_TYPED_VIEW,
        ) as *mut NativeTypedViewHeader;
        (*view).length = length as u32;
        (*view).capacity = length as u32;
        (*view).kind = kind;
        (*view).elem_size = elem_size as u8;
        (*view)._pad = [0; 6];
        (*view).owner = owner;
        (*view).data = if byte_length == 0 {
            (*owner).data
        } else {
            (*owner).data.add(byte_offset as usize)
        };
        (*view).byte_offset = byte_offset;
        (*view).byte_length = byte_length;
        (*view).generation = (*owner).generation;
        (*view)._pad2 = 0;
        register_view(view);
        view
    }
}

#[no_mangle]
pub extern "C" fn js_native_pod_view(
    owner_raw: u64,
    byte_offset: i64,
    record_count: i64,
    stride: i64,
    alignment: i64,
    layout_id: i64,
) -> *mut NativePodViewHeader {
    if byte_offset < 0 || record_count < 0 || stride <= 0 || alignment <= 0 || layout_id == 0 {
        throw_range_error(b"NativePodView is out of bounds");
    }
    let byte_offset = byte_offset as u64;
    let record_count = record_count as u64;
    let stride = stride as u64;
    let alignment = alignment as u64;
    if !alignment.is_power_of_two()
        || !byte_offset.is_multiple_of(alignment)
        || !stride.is_multiple_of(alignment)
    {
        throw_range_error(b"NativePodView byteOffset or stride is unaligned");
    }
    let byte_length = record_count
        .checked_mul(stride)
        .unwrap_or_else(|| throw_range_error(b"NativePodView is out of bounds"));
    let owner = unsafe { clean_owner_ptr(owner_raw) };
    unsafe {
        validate_owner_alive(owner);
        let end = byte_offset
            .checked_add(byte_length)
            .unwrap_or_else(|| throw_range_error(b"NativePodView is out of bounds"));
        if end > (*owner).byte_length {
            throw_range_error(b"NativePodView is out of bounds");
        }
        let view = crate::gc::gc_malloc(
            std::mem::size_of::<NativePodViewHeader>(),
            crate::gc::GC_TYPE_NATIVE_POD_VIEW,
        ) as *mut NativePodViewHeader;
        (*view).owner = owner;
        (*view).data = if byte_length == 0 {
            (*owner).data
        } else {
            (*owner).data.add(byte_offset as usize)
        };
        (*view).byte_offset = byte_offset;
        (*view).byte_length = byte_length;
        (*view).record_count = record_count;
        (*view).stride = stride as u32;
        (*view).alignment = alignment as u32;
        (*view).layout_id = layout_id as u64;
        (*view).generation = (*owner).generation;
        (*view)._pad = 0;
        register_pod_view(view);
        view
    }
}

fn strict_pod_view_from_value(value: f64, expected_layout_id: u64) -> *const NativePodViewHeader {
    let bits = value.to_bits();
    let raw_ptr = if crate::value::JSValue::from_bits(bits).is_pointer() {
        (bits & crate::value::POINTER_MASK) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
        bits as usize
    } else {
        0
    };
    if raw_ptr == 0 {
        throw_type_error(b"Expected NativePodView for native pod+count parameter");
    }
    let view = raw_ptr as *const NativePodViewHeader;
    unsafe {
        validate_pod_view_alive(view);
        if (*view).layout_id != expected_layout_id {
            throw_type_error(b"NativePodView layout does not match manifest pod+count parameter");
        }
    }
    view
}

#[no_mangle]
pub extern "C" fn js_native_abi_check_pod_view_data_ptr(
    value: f64,
    expected_layout_id: i64,
) -> *const u8 {
    let view = strict_pod_view_from_value(value, expected_layout_id as u64);
    unsafe { (*view).data as *const u8 }
}

#[no_mangle]
pub extern "C" fn js_native_abi_check_pod_view_record_count(
    value: f64,
    expected_layout_id: i64,
) -> usize {
    let view = strict_pod_view_from_value(value, expected_layout_id as u64);
    unsafe { (*view).record_count as usize }
}

#[no_mangle]
pub extern "C" fn js_native_arena_dispose(owner_raw: u64) {
    let owner = unsafe { clean_owner_ptr(owner_raw) };
    if owner.is_null() {
        return;
    }
    unsafe {
        dispose_owner(owner);
    }
}

pub(crate) unsafe fn finalize_native_arena_owner_for_gc(owner: *mut NativeArenaOwnerHeader) {
    dispose_owner(owner);
    unregister_owner(owner);
}

pub(crate) unsafe fn finalize_native_typed_view_for_gc(view: *mut NativeTypedViewHeader) {
    unregister_view(view);
}

pub(crate) unsafe fn finalize_native_pod_view_for_gc(view: *mut NativePodViewHeader) {
    unregister_pod_view(view);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_int;

    fn boxed_ptr(ptr: *const u8) -> f64 {
        f64::from_bits(crate::value::JSValue::pointer(ptr).bits())
    }

    fn undefined() -> f64 {
        f64::from_bits(crate::value::JSValue::undefined().bits())
    }

    fn catch_runtime_throw(f: impl FnOnce()) -> bool {
        let env = crate::exception::js_try_push();
        let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
        if jumped == 0 {
            f();
            crate::exception::js_try_end();
            false
        } else {
            crate::exception::js_try_end();
            crate::exception::js_clear_exception();
            true
        }
    }

    unsafe fn dispatch_random_fill_sync<T>(view: *mut T) -> f64 {
        let module = b"crypto";
        let ns = crate::object::js_create_native_module_namespace(module.as_ptr(), module.len());
        let ns_obj = crate::value::js_nanbox_get_pointer(ns) as *const crate::object::ObjectHeader;
        let args = [boxed_ptr(view as *const u8), undefined(), undefined()];
        crate::object::dispatch_native_module_method(
            ns_obj,
            "randomFillSync",
            args.as_ptr(),
            args.len(),
        )
    }

    #[test]
    fn native_arena_alloc_view_roundtrip_u32() {
        let owner = js_native_arena_alloc(16);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT32 as i32, 4, 2);
        let ta = view as *mut TypedArrayHeader;
        crate::typedarray::js_typed_array_set(ta, 0, 0xAABB_CCDDu32 as f64);
        crate::typedarray::js_typed_array_set(ta, 1, 7.0);
        assert_eq!(crate::typedarray::js_typed_array_length(ta), 2);
        assert_eq!(
            crate::typedarray::js_typed_array_get(ta, 0),
            0xAABB_CCDDu32 as f64
        );
        assert_eq!(crate::typedarray::js_typed_array_get(ta, 1), 7.0);
        js_native_arena_dispose(owner as u64);
        js_native_arena_dispose(owner as u64);
    }

    #[test]
    fn native_view_roots_owner_through_gc_trace_metadata() {
        let owner = js_native_arena_alloc(8);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_FLOAT64 as i32, 0, 1);
        unsafe {
            assert_eq!((*view).owner, owner);
            assert_eq!((*view).generation, (*owner).generation);
        }
        unsafe {
            finalize_native_typed_view_for_gc(view);
            finalize_native_arena_owner_for_gc(owner);
        }
    }

    #[test]
    fn native_pod_view_validates_bounds_alignment_layout_and_dispose() {
        let owner = js_native_arena_alloc(64);
        let view = js_native_pod_view(owner as u64, 8, 3, 8, 8, 0x1234);
        unsafe {
            assert_eq!((*view).owner, owner);
            assert_eq!((*view).data, (*owner).data.add(8));
            assert_eq!((*view).byte_offset, 8);
            assert_eq!((*view).byte_length, 24);
            assert_eq!((*view).record_count, 3);
            assert_eq!((*view).stride, 8);
            assert_eq!((*view).alignment, 8);
            assert_eq!((*view).layout_id, 0x1234);
        }
        let boxed = boxed_ptr(view as *const u8);
        assert_eq!(
            js_native_abi_check_pod_view_data_ptr(boxed, 0x1234),
            unsafe { (*owner).data.add(8) as *const u8 }
        );
        assert_eq!(js_native_abi_check_pod_view_record_count(boxed, 0x1234), 3);

        assert!(catch_runtime_throw(|| {
            let _ = js_native_abi_check_pod_view_data_ptr(boxed, 0x5678);
        }));
        assert!(catch_runtime_throw(|| {
            let _ = js_native_pod_view(owner as u64, 4, 1, 8, 8, 0x1234);
        }));
        assert!(catch_runtime_throw(|| {
            let _ = js_native_pod_view(owner as u64, 48, 3, 8, 8, 0x1234);
        }));
        assert!(catch_runtime_throw(|| {
            let _ = js_native_pod_view(owner as u64, 0, i64::MAX, 8, 8, 0x1234);
        }));

        js_native_arena_dispose(owner as u64);
        assert!(catch_runtime_throw(|| {
            let _ = js_native_abi_check_pod_view_record_count(boxed, 0x1234);
        }));
        assert!(catch_runtime_throw(|| {
            let _ = js_native_pod_view(owner as u64, 0, 1, 8, 8, 0x1234);
        }));
    }

    #[test]
    fn native_pod_view_roots_owner_without_scanning_backing_bytes() {
        let owner = js_native_arena_alloc(32);
        let view = js_native_pod_view(owner as u64, 0, 4, 8, 8, 0x1234);
        unsafe {
            assert_eq!((*view).owner, owner);
            assert_eq!((*view).generation, (*owner).generation);
        }
        assert_eq!(
            crate::gc::test_gc_rewrite_slot_count(view as usize),
            Some(1),
            "NativePodView must expose only its owner slot to the GC"
        );
        unsafe {
            finalize_native_pod_view_for_gc(view);
            finalize_native_arena_owner_for_gc(owner);
        }
    }

    #[test]
    fn native_uint8array_helpers_read_and_write_backing_bytes() {
        let owner = js_native_arena_alloc(8);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 0, 8);
        let ta = view as *mut TypedArrayHeader;
        unsafe {
            *(*owner).data.add(3) = 41;
        }
        assert_eq!(crate::typedarray::js_uint8array_get(ta, 3), 41);
        assert_eq!(crate::typedarray::js_uint8array_get(ta, 99), 0);

        crate::typedarray::js_uint8array_set(ta, 4, 300);
        unsafe {
            assert_eq!(*(*owner).data.add(4), 44);
            assert_eq!(*(*owner).data.add(7), 0);
        }
        crate::typedarray::js_uint8array_set(ta, 99, 11);
        unsafe {
            assert_eq!(*(*owner).data.add(7), 0);
        }
        js_native_arena_dispose(owner as u64);
    }

    #[test]
    fn native_memory_fill_u32_handles_heap_and_native_views() {
        let heap = typedarray::typed_array_alloc(typedarray::KIND_UINT32, 4);
        crate::typedarray::js_native_memory_fill_u32(heap as u64, 0xAABB_CCDDu32 as f64);
        for i in 0..4 {
            assert_eq!(
                crate::typedarray::js_typed_array_get(heap, i),
                0xAABB_CCDDu32 as f64
            );
        }

        let owner = js_native_arena_alloc(16);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT32 as i32, 4, 2);
        crate::typedarray::js_native_memory_fill_u32(view as u64, 7.0);
        unsafe {
            assert_eq!(*((*owner).data.add(4) as *const u32), 7);
            assert_eq!(*((*owner).data.add(8) as *const u32), 7);
        }
        js_native_arena_dispose(owner as u64);
    }

    #[test]
    fn native_memory_copy_uses_raw_overlap_safe_bytes() {
        let owner = js_native_arena_alloc(8);
        let src = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 0, 6);
        let dst = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 2, 6);
        unsafe {
            for i in 0..8 {
                *(*owner).data.add(i) = (i + 1) as u8;
            }
        }

        crate::typedarray::js_native_memory_copy(dst as u64, src as u64);

        unsafe {
            let bytes = std::slice::from_raw_parts((*owner).data, 8);
            assert_eq!(bytes, &[1, 2, 1, 2, 3, 4, 5, 6]);
        }
        js_native_arena_dispose(owner as u64);
    }

    #[test]
    fn native_memory_helpers_validate_kind_and_dispose() {
        let bytes = typedarray::typed_array_alloc(typedarray::KIND_UINT8, 4);
        assert!(catch_runtime_throw(|| {
            crate::typedarray::js_native_memory_fill_u32(bytes as u64, 0.0);
        }));

        let owner = js_native_arena_alloc(8);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 0, 8);
        js_native_arena_dispose(owner as u64);
        assert!(catch_runtime_throw(|| {
            crate::typedarray::js_native_memory_copy(view as u64, view as u64);
        }));
    }

    #[test]
    fn native_memory_copy_rejects_typed_array_registry_forged_to_old_buffer() {
        let buf = crate::buffer::buffer_alloc(crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as u32);
        assert!(crate::arena::pointer_in_old_gen(buf as usize));
        typedarray::register_typed_array(buf as *const TypedArrayHeader, typedarray::KIND_UINT8);

        assert!(catch_runtime_throw(|| {
            crate::typedarray::js_native_memory_copy(buf as u64, buf as u64);
        }));

        typedarray::unregister_typed_array(buf as *const TypedArrayHeader);
    }

    #[test]
    fn native_memory_fill_u32_rejects_typed_array_registry_forged_to_old_buffer() {
        let buf = crate::buffer::buffer_alloc(crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as u32);
        assert!(crate::arena::pointer_in_old_gen(buf as usize));
        typedarray::register_typed_array(buf as *const TypedArrayHeader, typedarray::KIND_UINT32);

        assert!(catch_runtime_throw(|| {
            crate::typedarray::js_native_memory_fill_u32(buf as u64, 1.0);
        }));

        typedarray::unregister_typed_array(buf as *const TypedArrayHeader);
    }

    #[test]
    fn native_memory_copy_rejects_buffer_registry_forged_to_old_non_buffer() {
        let fake = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::buffer::BufferHeader>(),
            8,
            crate::gc::GC_TYPE_OBJECT,
        ) as *mut crate::buffer::BufferHeader;
        assert!(crate::arena::pointer_in_old_gen(fake as usize));
        crate::buffer::register_buffer(fake as *const crate::buffer::BufferHeader);
        crate::buffer::mark_as_uint8array(fake as usize);

        assert!(catch_runtime_throw(|| {
            crate::typedarray::js_native_memory_copy(fake as u64, fake as u64);
        }));
    }

    #[test]
    fn random_fill_sync_rejects_typed_array_registry_forged_to_old_buffer() {
        let buf = crate::buffer::buffer_alloc(crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as u32);
        assert!(crate::arena::pointer_in_old_gen(buf as usize));
        typedarray::register_typed_array(buf as *const TypedArrayHeader, typedarray::KIND_UINT8);

        assert!(catch_runtime_throw(|| unsafe {
            let _ = dispatch_random_fill_sync(buf);
        }));

        typedarray::unregister_typed_array(buf as *const TypedArrayHeader);
    }

    #[test]
    fn random_fill_sync_native_uint8_view_preserves_metadata() {
        let owner = js_native_arena_alloc(96);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 8, 64);
        let target = boxed_ptr(view as *const u8);
        let before = unsafe {
            (
                (*view).owner,
                (*view).data,
                (*view).byte_offset,
                (*view).byte_length,
                (*view).generation,
            )
        };

        let returned = unsafe { dispatch_random_fill_sync(view) };
        assert_eq!(returned.to_bits(), target.to_bits());

        unsafe {
            assert_eq!((*view).owner, before.0);
            assert_eq!((*view).data, before.1);
            assert_eq!((*view).byte_offset, before.2);
            assert_eq!((*view).byte_length, before.3);
            assert_eq!((*view).generation, before.4);
            let bytes = std::slice::from_raw_parts((*view).data, (*view).byte_length as usize);
            assert!(
                bytes.iter().any(|&byte| byte != 0),
                "randomFillSync should mutate native view backing bytes"
            );
        }
        js_native_arena_dispose(owner as u64);
    }

    #[test]
    fn disposed_native_uint8_views_throw_in_fallback_paths() {
        let owner = js_native_arena_alloc(16);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 0, 16);
        let ta = view as *mut TypedArrayHeader;
        js_native_arena_dispose(owner as u64);

        assert!(catch_runtime_throw(|| {
            let _ = crate::typedarray::js_uint8array_get(ta, 0);
        }));
        assert!(catch_runtime_throw(|| {
            crate::typedarray::js_uint8array_set(ta, 0, 1);
        }));
        assert!(catch_runtime_throw(|| unsafe {
            let _ = dispatch_random_fill_sync(view);
        }));
    }
}
