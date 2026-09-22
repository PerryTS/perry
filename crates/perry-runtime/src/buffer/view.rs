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

/// What the registry stores per view: the hot `ViewInfo` (what every byte
/// access copies out through `lookup`) plus the resizable-backing bookkeeping
/// (#10873), which only `resize` and the reflective getters read. Kept apart so
/// the per-access copy stays the two words it always was.
#[derive(Debug)]
struct ViewRecord {
    info: ViewInfo,
    /// Construction-time byte length. Only consulted when the backing is a
    /// resizable ArrayBuffer: a fixed-length view reads as length 0 while it
    /// does not fit, and gets THIS length back when the buffer grows.
    fixed_len: u32,
    /// Constructed without an explicit length over a resizable ArrayBuffer, so
    /// its length follows the buffer's `byteLength`.
    length_tracking: bool,
    /// ES2024 IsTypedArrayOutOfBounds / IsViewOutOfBounds, as of the last
    /// resize. Always false over a fixed-length backing.
    out_of_bounds: bool,
}

crate::perry_thread_local! {
    // Boxed records keep GC slot addresses stable across registry growth.
    // Keys and backing addresses are non-moving GC_TYPE_BUFFER objects.
    static VIEW_REGISTRY: RefCell<PtrHashMap<usize, Box<ViewRecord>>> =
        RefCell::new(new_ptr_hash_map());
    // Weak reverse index, used for detach and death pruning, never for writes.
    static BACKING_TO_VIEWS: RefCell<PtrHashMap<usize, PtrHashSet<usize>>> =
        RefCell::new(new_ptr_hash_map());
}

#[inline]
pub(crate) fn lookup(view_ptr: usize) -> Option<ViewInfo> {
    VIEW_REGISTRY.with(|r| r.borrow().get(&view_ptr).map(|rec| rec.info))
}

#[inline]
pub(crate) fn backing_of(buf_ptr: usize) -> usize {
    lookup(buf_ptr).map(|v| v.backing).unwrap_or(buf_ptr)
}

#[inline]
pub(crate) fn byte_offset_of(buf_ptr: usize) -> u32 {
    lookup(buf_ptr)
        .map(|v| {
            // A view that no longer fits its resized buffer reports byteOffset
            // 0, like a view over a detached one.
            if super::detach::is_detached_buffer(v.backing) || is_out_of_bounds_view(buf_ptr) {
                0
            } else {
                v.offset
            }
        })
        .unwrap_or(0)
}

/// True when `view_ptr` is a view whose resizable backing has shrunk past it.
/// `resize` has already zeroed its header length; this is what tells a
/// DataView accessor to raise the spec's TypeError rather than a RangeError.
#[inline]
pub(crate) fn is_out_of_bounds_view(view_ptr: usize) -> bool {
    super::any_resizable_buffer()
        && VIEW_REGISTRY.with(|r| {
            r.borrow()
                .get(&view_ptr)
                .is_some_and(|rec| rec.out_of_bounds)
        })
}

/// Mark a just-constructed view as length-tracking (`new Uint8Array(rab)`,
/// `new DataView(rab, off)`). A no-op over a fixed-length backing.
pub(crate) fn mark_length_tracking(view_ptr: usize) {
    if !super::any_resizable_buffer() {
        return;
    }
    VIEW_REGISTRY.with(|r| {
        if let Some(rec) = r.borrow_mut().get_mut(&view_ptr) {
            if super::resizable::is_resizable_buffer(rec.info.backing) {
                rec.length_tracking = true;
            }
        }
    });
}

#[inline]
pub(crate) fn is_length_tracking(view_ptr: usize) -> bool {
    super::any_resizable_buffer()
        && VIEW_REGISTRY.with(|r| {
            r.borrow()
                .get(&view_ptr)
                .is_some_and(|rec| rec.length_tracking)
        })
}

/// `ArrayBuffer.prototype.resize` support: recompute the header length of every
/// Buffer-shaped view (Uint8Array / Buffer / DataView — byte-granular) over
/// `backing`, whose byteLength is now `buffer_len`. Eager, like detach zeroing
/// them, so every reader of a view's length stays oblivious to resizing.
pub(crate) fn relength_views_of_resized_backing(backing: usize, buffer_len: u32) {
    let views: Vec<usize> = BACKING_TO_VIEWS.with(|m| {
        m.borrow()
            .get(&backing)
            .map(|views| views.iter().copied().collect())
            .unwrap_or_default()
    });
    if views.is_empty() {
        return;
    }
    VIEW_REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        for view in views {
            let Some(rec) = r.get_mut(&view) else {
                continue;
            };
            let len = super::resizable::view_length_after_resize(
                buffer_len,
                rec.info.offset,
                1,
                rec.length_tracking,
                rec.fixed_len,
            );
            rec.out_of_bounds = len.is_none();
            unsafe {
                (*(view as *mut BufferHeader)).length = len.unwrap_or(0);
            }
        }
    });
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
    let info = VIEW_REGISTRY.with(|r| r.borrow_mut().remove(&addr).map(|rec| rec.info));
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

/// Allocate a DataView with one cached native data pointer after its
/// `BufferHeader`. Unlike Buffer/Uint8Array views, DataView byte access always
/// enters a runtime helper, so this private payload is never mistaken for
/// indexed storage. The backing remains owned and traced by `VIEW_REGISTRY`.
///
/// Buffer allocations and foreign/shared ArrayBuffer storage are non-moving:
/// `buffer_alloc` uses the old arena because native callers retain byte
/// pointers, and foreign/shared backings have the same stable-address contract.
/// The cached interior pointer therefore stays valid until detach, which zeroes
/// the view length before its backing storage can be released.
pub(crate) fn alloc_data_view(
    backing: *const BufferHeader,
    offset: u32,
    length: u32,
) -> *mut BufferHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_const_ptr(backing);
    let view = buffer_alloc(std::mem::size_of::<usize>() as u32);
    unsafe {
        (*view).length = length;
        owner.with_const_ptr::<BufferHeader, _>(|backing| {
            register(view as usize, backing as usize, offset);
            let data = buffer_data(backing).add(offset as usize);
            data_view_cache_slot(view).write(data as usize);
        });
    }
    view
}

#[inline(always)]
unsafe fn data_view_cache_slot(view: *mut BufferHeader) -> *mut usize {
    (view as *mut u8)
        .add(std::mem::size_of::<BufferHeader>())
        .cast::<usize>()
}

/// Load the stable byte pointer cached by [`alloc_data_view`]. The caller must
/// first bounds-check the DataView and reject a detached backing.
#[inline(always)]
pub(crate) unsafe fn data_view_data_ptr(view: *mut BufferHeader) -> *mut u8 {
    debug_assert!((*view).capacity >= std::mem::size_of::<usize>() as u32);
    data_view_cache_slot(view).read() as *mut u8
}

fn register(view_ptr: usize, backing_ptr: usize, offset: u32) {
    let (backing, offset) = lookup(backing_ptr)
        .map(|parent| (parent.backing, parent.offset + offset))
        .unwrap_or((backing_ptr, offset));
    super::header::u8_inline_cache_invalidate(view_ptr);
    // Every caller sets the view's header length before registering it.
    let fixed_len = unsafe { (*(view_ptr as *const BufferHeader)).length };
    let mut rec = Box::new(ViewRecord {
        info: ViewInfo { backing, offset },
        fixed_len,
        length_tracking: false,
        out_of_bounds: false,
    });
    crate::gc::runtime_write_barrier_external_slot(
        view_ptr,
        &mut rec.info.backing as *mut usize as usize,
        backing as u64,
    );
    VIEW_REGISTRY.with(|r| {
        r.borrow_mut().insert(view_ptr, rec);
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
        if let Some(rec) = r.borrow_mut().get_mut(&addr) {
            visit(&mut rec.info.backing as *mut usize as *mut u64);
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
