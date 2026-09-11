//! Shared Buffer/Uint8Array storage without changing the BufferHeader ABI.
//!
//! Views allocate only a header. All byte access resolves to the ultimate
//! backing plus an offset; generated inline reads admit only non-view buffers.
//! The GC traces the backing from a live view, never the reverse index, so dead
//! views and otherwise unreachable backing/ArrayBuffer identity cycles die.

use super::*;
use crate::fast_hash::{new_ptr_hash_map, new_ptr_hash_set, PtrHashMap, PtrHashSet};
use std::cell::RefCell;

#[derive(Copy, Clone, Debug)]
pub(crate) struct ViewInfo {
    pub backing: usize,
    pub offset: u32,
}

crate::perry_thread_local! {
    // Boxed records keep GC slot addresses stable across registry growth.
    // Keys and backing addresses are non-moving GC_TYPE_BUFFER objects.
    static VIEW_REGISTRY: RefCell<PtrHashMap<usize, Box<ViewInfo>>> =
        RefCell::new(new_ptr_hash_map());
    // Weak reverse index, used for detach and death pruning, never for writes.
    static BACKING_TO_VIEWS: RefCell<PtrHashMap<usize, PtrHashSet<usize>>> =
        RefCell::new(new_ptr_hash_map());
}

#[inline]
pub(crate) fn lookup(view_ptr: usize) -> Option<ViewInfo> {
    VIEW_REGISTRY.with(|r| r.borrow().get(&view_ptr).map(|info| **info))
}

#[inline]
pub(crate) fn backing_of(buf_ptr: usize) -> usize {
    lookup(buf_ptr).map(|v| v.backing).unwrap_or(buf_ptr)
}

#[inline]
pub(crate) fn byte_offset_of(buf_ptr: usize) -> u32 {
    lookup(buf_ptr)
        .map(|v| {
            if super::detach::is_detached_buffer(v.backing) {
                0
            } else {
                v.offset
            }
        })
        .unwrap_or(0)
}

/// `buf_ptr` must be a live buffer. All runtime/native span consumers use the
/// same canonical bytes as indexed reads and writes.
pub(crate) unsafe fn resolve_data_ptr(buf_ptr: *const BufferHeader) -> *const u8 {
    buffer_data(buf_ptr)
}

pub(crate) fn for_each_view<F: FnMut(usize, ViewInfo)>(backing_ptr: usize, mut f: F) {
    BACKING_TO_VIEWS.with(|m| {
        if let Some(views) = m.borrow().get(&backing_ptr) {
            for &view in views {
                if let Some(info) = lookup(view) {
                    f(view, info);
                }
            }
        }
    });
}

/// O(1) per dead view, with no tombstones. If the backing is finalized first,
/// remove its dead children in one pass; total sweep work stays linear.
pub(crate) fn remove_entries_for_dead_buffer(addr: usize) {
    let info = VIEW_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    BACKING_TO_VIEWS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(info) = info {
            if let Some(views) = m.get_mut(&info.backing) {
                views.remove(&addr);
                if views.is_empty() {
                    m.remove(&info.backing);
                }
            }
        }
        if let Some(views) = m.remove(&addr) {
            VIEW_REGISTRY.with(|r| {
                let mut r = r.borrow_mut();
                for view in views {
                    r.remove(&view);
                }
            });
        }
    });
}

/// Allocate a header-only view, retaining its offset even when it is empty.
pub(crate) fn alloc(backing: *const BufferHeader, offset: u32, length: u32) -> *mut BufferHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_const_ptr(backing);
    let view = buffer_alloc(0);
    unsafe {
        (*view).length = length;
        // capacity describes the physical inline allocation, not the span.
        owner.with_const_ptr::<BufferHeader, _>(|backing| {
            register(view as usize, backing as usize, offset);
        });
    }
    view
}

fn register(view_ptr: usize, backing_ptr: usize, offset: u32) {
    let (backing, offset) = lookup(backing_ptr)
        .map(|parent| (parent.backing, parent.offset + offset))
        .unwrap_or((backing_ptr, offset));
    super::header::u8_inline_cache_invalidate(view_ptr);
    let mut info = Box::new(ViewInfo { backing, offset });
    crate::gc::runtime_write_barrier_external_slot(
        view_ptr,
        &mut info.backing as *mut usize as usize,
        backing as u64,
    );
    VIEW_REGISTRY.with(|r| {
        r.borrow_mut().insert(view_ptr, info);
    });
    BACKING_TO_VIEWS.with(|m| {
        m.borrow_mut()
            .entry(backing)
            .or_insert_with(new_ptr_hash_set)
            .insert(view_ptr);
    });
}

/// Called by the buffer GC descriptor only for the object being traced.
/// Backings are old/non-moving; exposing a stable slot also makes the edge
/// visible to the collector's rewrite and verification walks.
pub(crate) fn visit_backing_slot(addr: usize, mut visit: impl FnMut(*mut u64)) {
    VIEW_REGISTRY.with(|r| {
        if let Some(info) = r.borrow_mut().get_mut(&addr) {
            visit(&mut info.backing as *mut usize as *mut u64);
        }
    });
}

#[cfg(test)]
pub(crate) fn registry_sizes() -> (usize, usize, usize) {
    let views = VIEW_REGISTRY.with(|r| r.borrow().len());
    BACKING_TO_VIEWS.with(|m| {
        let m = m.borrow();
        (views, m.len(), m.values().map(|v| v.len()).sum())
    })
}
