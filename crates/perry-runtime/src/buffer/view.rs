//! Sixteen-byte views resolve their one owner edge at access time.
use super::*;

#[derive(Copy, Clone, Debug)]
pub(crate) struct ViewInfo {
    pub backing: usize,
    pub offset: u32,
}

#[inline]
pub(crate) fn lookup(addr: usize) -> Option<ViewInfo> {
    if !store::is_view(addr) {
        return None;
    }
    Some(unsafe {
        ViewInfo {
            backing: store::owner(addr),
            offset: super::store::capacity(addr),
        }
    })
}

#[inline]
pub(crate) fn backing_of(addr: usize) -> usize {
    unsafe { store::owner(addr) }
}

#[inline]
pub(crate) fn byte_offset_of(addr: usize) -> u32 {
    if addr == 0 || !store::is_view(addr) || is_detached_buffer(addr) || is_out_of_bounds_view(addr)
    {
        return 0;
    }
    unsafe { super::store::capacity(addr) }
}

#[inline]
pub(crate) fn is_out_of_bounds_view(addr: usize) -> bool {
    unsafe { store::out_of_bounds(addr) }
}

pub(crate) fn mark_length_tracking(addr: usize) {
    unsafe {
        if is_resizable_buffer(store::owner(addr)) {
            (*store::header(addr))._reserved |= crate::codegen_abi::BYTES_LENGTH_TRACKING;
        }
    }
}

pub(crate) fn is_length_tracking(addr: usize) -> bool {
    store::is_view(addr)
        && unsafe {
            (*store::header(addr))._reserved & crate::codegen_abi::BYTES_LENGTH_TRACKING != 0
        }
}

pub(crate) unsafe fn resolve_data_ptr(ptr: *const BufferHeader) -> *const u8 {
    store::data(ptr as usize)
}
pub(crate) unsafe fn data_view_data_ptr(ptr: *mut BufferHeader) -> *mut u8 {
    store::data(ptr as usize)
}

pub(crate) fn alloc(backing: *const BufferHeader, offset: u32, length: u32) -> *mut BufferHeader {
    store::new_view(
        crate::gc::GC_TYPE_BUFFER,
        backing as usize,
        offset,
        length,
        false,
    )
}

pub(crate) fn alloc_data_view(
    backing: *const BufferHeader,
    offset: u32,
    length: u32,
) -> *mut BufferHeader {
    store::new_view(
        crate::gc::GC_TYPE_BUFFER_DATA_VIEW,
        backing as usize,
        offset,
        length,
        false,
    )
}
