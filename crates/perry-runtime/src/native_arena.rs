//! NativeArena uses the ordinary out-of-line byte owner and sixteen-byte views.
use crate::typedarray::{self, TypedArrayHeader};
use std::ptr;

pub type NativeArenaOwnerHeader = crate::buffer::BufferHeader;
pub type NativeTypedViewHeader = crate::buffer::BufferHeader;

#[repr(C)]
pub struct NativePodViewHeader {
    pub owner: *mut NativeArenaOwnerHeader,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub record_count: u64,
    pub stride: u32,
    pub alignment: u32,
    pub layout_id: u64,
}

fn strip_nanbox(raw: u64) -> usize {
    if raw >> 48 >= 0x7ff8 {
        (raw & crate::value::POINTER_MASK) as usize
    } else {
        raw as usize
    }
}

#[cold]
fn throw_type_error(message: &[u8]) -> ! {
    typedarray::throw_type_error(message)
}
#[cold]
fn throw_range_error(message: &[u8]) -> ! {
    typedarray::throw_range_error(message)
}
#[cold]
pub(crate) fn throw_native_arena_disposed() -> ! {
    throw_type_error(b"NativeArena has been disposed")
}

fn owner_is_registered(owner: *const NativeArenaOwnerHeader) -> bool {
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(owner as usize) }
        .is_some_and(|h| unsafe { h.as_ref() }.obj_type == crate::gc::GC_TYPE_NATIVE_ARENA_OWNER)
}

#[inline]
pub(crate) fn is_native_typed_view(ta: *const TypedArrayHeader) -> bool {
    let addr = ta as usize;
    if !crate::buffer::header::byte_cell_type(addr)
        .is_some_and(|t| crate::gc::is_byte_view_type(t) && crate::gc::is_typed_array_type(t))
    {
        return false;
    }
    unsafe {
        (*crate::buffer::store::header(crate::buffer::store::owner(addr))).obj_type
            == crate::gc::GC_TYPE_NATIVE_ARENA_OWNER
    }
}

#[inline]
pub(crate) fn is_native_pod_view(view: *const NativePodViewHeader) -> bool {
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(view as usize) }
        .is_some_and(|h| unsafe { h.as_ref() }.obj_type == crate::gc::GC_TYPE_NATIVE_POD_VIEW)
}

pub(crate) unsafe fn native_view_from_typed_array(
    ta: *const TypedArrayHeader,
) -> *const NativeTypedViewHeader {
    ta.cast()
}

unsafe fn clean_owner_ptr(raw: u64) -> *mut NativeArenaOwnerHeader {
    let owner = strip_nanbox(raw) as *mut NativeArenaOwnerHeader;
    if owner_is_registered(owner) {
        owner
    } else {
        ptr::null_mut()
    }
}

pub(crate) unsafe fn validate_owner_alive(owner: *mut NativeArenaOwnerHeader) {
    if !owner_is_registered(owner) {
        throw_type_error(b"Invalid NativeArena owner");
    }
    if crate::buffer::is_detached_buffer(owner as usize) {
        throw_native_arena_disposed();
    }
}

pub(crate) unsafe fn validate_view_alive(view: *const NativeTypedViewHeader) {
    if is_native_typed_view(view) {
        validate_owner_alive(
            crate::buffer::store::owner(view as usize) as *mut NativeArenaOwnerHeader
        );
    }
}

pub(crate) unsafe fn validate_pod_view_alive(view: *const NativePodViewHeader) {
    if !is_native_pod_view(view) {
        throw_type_error(b"Invalid NativePodView");
    }
    validate_owner_alive((*view).owner);
}

unsafe fn dispose_owner(owner: *mut NativeArenaOwnerHeader) {
    if owner.is_null() {
        return;
    }
    crate::buffer::detach_array_buffer(owner as usize);
}

pub(crate) unsafe fn release_disposed_bytes(owner: *mut NativeArenaOwnerHeader) {
    if crate::buffer::is_detached_buffer(owner as usize) {
        drop(crate::buffer::header::take_owned_backing(owner as usize));
    }
}

#[no_mangle]
pub extern "C" fn js_native_arena_alloc(byte_length: i64) -> *mut NativeArenaOwnerHeader {
    if byte_length < 0 || byte_length > crate::object::shape_rule3::MAX_PLUS_FOUR_WORD as i64 {
        throw_range_error(b"NativeArena byteLength is out of range");
    }
    let owner = crate::buffer::buffer_alloc_owned(byte_length as u32, byte_length as u32);
    unsafe {
        (*crate::buffer::store::header(owner as usize)).obj_type =
            crate::gc::GC_TYPE_NATIVE_ARENA_OWNER;
    }
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
    if kind > typedarray::KIND_FLOAT16 {
        throw_range_error(b"NativeArena view kind is invalid");
    }
    if length > crate::object::shape_rule3::MAX_PLUS_FOUR_WORD as i64 {
        throw_range_error(b"NativeArena view length exceeds Perry's maximum");
    }
    let size = typedarray::elem_size_for_kind(kind) as u64;
    let offset = byte_offset as u64;
    if !offset.is_multiple_of(size) {
        throw_range_error(b"NativeArena view byteOffset is unaligned");
    }
    let owner = unsafe { clean_owner_ptr(owner_raw) };
    unsafe {
        validate_owner_alive(owner);
        let end = (length as u64)
            .checked_mul(size)
            .and_then(|n| offset.checked_add(n))
            .unwrap_or_else(|| throw_range_error(b"NativeArena view is out of bounds"));
        if end > (*owner).length as u64 {
            throw_range_error(b"NativeArena view is out of bounds");
        }
        crate::buffer::store::new_view(
            typedarray::type_for_kind(kind),
            owner as usize,
            offset as u32,
            length as u32,
            false,
        )
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
        if end > (*owner).length as u64 {
            throw_range_error(b"NativePodView is out of bounds");
        }
        let view = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<NativePodViewHeader>(),
            8,
            crate::gc::GC_TYPE_NATIVE_POD_VIEW,
        ) as *mut NativePodViewHeader;
        (*view).owner = owner;
        (*view).byte_offset = byte_offset;
        (*view).byte_length = byte_length;
        (*view).record_count = record_count;
        (*view).stride = stride as u32;
        (*view).alignment = alignment as u32;
        (*view).layout_id = layout_id as u64;
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
        throw_type_error(b"Expected NativePodView");
    }
    let view = raw_ptr as *const NativePodViewHeader;
    unsafe {
        validate_pod_view_alive(view);
        if expected_layout_id != 0 && (*view).layout_id != expected_layout_id {
            throw_type_error(b"NativePodView layout does not match manifest pod+count parameter");
        }
    }
    view
}

#[no_mangle]
pub extern "C" fn js_native_pod_view_length(value: f64) -> f64 {
    let view = strict_pod_view_from_value(value, 0);
    unsafe { (*view).record_count as f64 }
}

#[no_mangle]
pub extern "C" fn js_native_abi_check_pod_view_data_ptr(
    value: f64,
    expected_layout_id: i64,
) -> *const u8 {
    let view = strict_pod_view_from_value(value, expected_layout_id as u64);
    unsafe {
        crate::buffer::store::owner_data((*view).owner as usize).add((*view).byte_offset as usize)
            as *const u8
    }
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

// Byte owner/view finalization is shared with every other byte cell.

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed_ptr(ptr: *const u8) -> f64 {
        f64::from_bits(crate::value::JSValue::pointer(ptr).bits())
    }

    fn undefined() -> f64 {
        f64::from_bits(crate::value::JSValue::undefined().bits())
    }

    fn catch_runtime_throw(f: impl FnOnce()) -> bool {
        crate::exception::catch_js_throw(f).is_err()
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
            assert_eq!(
                crate::buffer::store::owner(view as usize) as *mut NativeArenaOwnerHeader,
                owner
            );
        }
        unsafe {
            dispose_owner(owner);
        }
    }

    #[test]
    fn native_pod_view_validates_bounds_alignment_layout_and_dispose() {
        let owner = js_native_arena_alloc(64);
        let view = js_native_pod_view(owner as u64, 8, 3, 8, 8, 0x1234);
        unsafe {
            assert_eq!((*view).owner, owner);
            assert_eq!(
                js_native_abi_check_pod_view_data_ptr(boxed_ptr(view.cast()), 0x1234),
                crate::buffer::store::owner_data(owner as usize).add(8)
            );
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
            unsafe { crate::buffer::store::owner_data(owner as usize).add(8) as *const u8 }
        );
        assert_eq!(js_native_abi_check_pod_view_record_count(boxed, 0x1234), 3);
        assert_eq!(js_native_pod_view_length(boxed), 3.0);

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
        }
        assert_eq!(
            crate::gc::test_gc_rewrite_slot_count(view as usize),
            Some(1),
            "NativePodView must expose only its owner slot to the GC"
        );
        unsafe {
            dispose_owner(owner);
        }
    }

    #[test]
    fn native_uint8array_helpers_read_and_write_backing_bytes() {
        let owner = js_native_arena_alloc(8);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 0, 8);
        let ta = view as *mut TypedArrayHeader;
        unsafe {
            *crate::buffer::store::owner_data(owner as usize).add(3) = 41;
        }
        assert_eq!(crate::typedarray::js_uint8array_get(ta, 3), 41);
        assert_eq!(crate::typedarray::js_uint8array_get(ta, 99), 0);

        crate::typedarray::js_uint8array_set(ta, 4, 300);
        unsafe {
            assert_eq!(*crate::buffer::store::owner_data(owner as usize).add(4), 44);
            assert_eq!(*crate::buffer::store::owner_data(owner as usize).add(7), 0);
        }
        crate::typedarray::js_uint8array_set(ta, 99, 11);
        unsafe {
            assert_eq!(*crate::buffer::store::owner_data(owner as usize).add(7), 0);
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
            assert_eq!(
                *(crate::buffer::store::owner_data(owner as usize).add(4) as *const u32),
                7
            );
            assert_eq!(
                *(crate::buffer::store::owner_data(owner as usize).add(8) as *const u32),
                7
            );
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
                *crate::buffer::store::owner_data(owner as usize).add(i) = (i + 1) as u8;
            }
        }

        crate::typedarray::js_native_memory_copy(dst as u64, src as u64);

        unsafe {
            let bytes =
                std::slice::from_raw_parts(crate::buffer::store::owner_data(owner as usize), 8);
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
    fn random_fill_sync_native_uint8_view_preserves_metadata() {
        let owner = js_native_arena_alloc(96);
        let view = js_native_arena_view(owner as u64, typedarray::KIND_UINT8 as i32, 8, 64);
        let target = boxed_ptr(view as *const u8);
        let before = unsafe {
            (
                (*view).link,
                crate::buffer::store::data(view as usize),
                (*view).capacity,
                (*view).length,
            )
        };

        let returned = unsafe { dispatch_random_fill_sync(view) };
        assert_eq!(returned.to_bits(), target.to_bits());

        unsafe {
            assert_eq!((*view).link, before.0);
            assert_eq!(crate::buffer::store::data(view as usize), before.1);
            assert_eq!((*view).capacity, before.2);
            assert_eq!((*view).length, before.3);
            let bytes = std::slice::from_raw_parts(
                crate::buffer::store::data(view as usize),
                (*view).length as usize,
            );
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
