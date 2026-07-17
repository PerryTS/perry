//! Issue #1205: shared backing-store semantics for
//! `Buffer.prototype.slice` / `subarray`.
//!
//! Node's `Buffer.slice` / `subarray` return a *view* over the source
//! buffer's memory: mutating the view is visible through the original
//! and vice-versa.  Perry historically returned a freshly-allocated
//! buffer with a copy of the slice bytes.
//!
//! Implementation strategy:
//!
//! 1.  `js_buffer_slice` still allocates a fresh `BufferHeader` and
//!     copies the bytes.  The fresh allocation lets the LLVM codegen's
//!     direct `gep+load/store` against the buffer pointer keep working
//!     unchanged — `view_ptr + 8 + idx` is always valid memory.
//! 2.  The view registry below remembers the alias relationship
//!     between the new view and its ultimate backing buffer plus a
//!     reverse map from the backing buffer to every live view.
//! 3.  The runtime byte mutators (`js_buffer_set`, `js_buffer_write`,
//!     `js_buffer_fill_range`, `js_buffer_copy`) consult the registry
//!     and propagate writes to every aliased buffer.  Together with
//!     the codegen slow-path indexed-write change in
//!     `Uint8ArraySet`/`Uint8ArrayGet` (`crates/perry-codegen/src/
//!     expr/arrays_finds.rs`), this lets `s[0] = 0x5a; buf[1]`-style
//!     round-trips observe the mutation through both sides.
//!
//! Limitations that remain (tracked under follow-up subtasks of
//! #1205):
//! - Codegen `Uint8ArrayGet`/`Uint8ArraySet` *fast paths* (statically
//!   typed `Buffer` locals fed by `Buffer.alloc`) skip the runtime
//!   helper and access memory directly.  Slices of `Buffer.alloc`
//!   buffers therefore lose the back-propagation when the alloc'd
//!   side is the one being mutated by tight-loop code that hits the
//!   fast path.  In practice the gap-suite shapes go through the
//!   slow path (slice receivers and `Buffer.from(...)` initializers
//!   aren't tracked in `buffer_data_slots`).

use super::*;

use std::cell::RefCell;
use std::collections::HashMap;

/// Each live slice/subarray records its ultimate backing buffer plus
/// the [offset, offset + length) range within that backing.  The
/// offset is in bytes; `length` matches the view's own `length`
/// field.  Slices-of-slices flatten on insert — we always resolve
/// to the *ultimate* backing so writes don't have to walk a chain.
#[derive(Copy, Clone)]
pub(crate) struct ViewInfo {
    pub backing: usize,
    pub offset: u32,
    pub length: u32,
}

thread_local! {
    /// `view_ptr → ViewInfo`.  Lookups during writes are O(1). Address-keyed
    /// `PtrHashMap` (#6386): both maps are probed on EVERY DataView/typed
    /// view write via `propagate_written_range_from_receiver`, and SipHash
    /// dominated those probes.
    static VIEW_REGISTRY: RefCell<crate::fast_hash::PtrHashMap<usize, ViewInfo>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// `backing_ptr → Vec<view_ptr>`.  Backing-side writes walk this
    /// list to mirror bytes into every aliased view.  Vector entries
    /// are tombstoned (set to 0) on view drop rather than removed so
    /// hot-path iteration stays branch-light.
    static BACKING_TO_VIEWS: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<usize>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

#[inline]
pub(crate) fn lookup(view_ptr: usize) -> Option<ViewInfo> {
    VIEW_REGISTRY.with(|r| r.borrow().get(&view_ptr).copied())
}

#[inline]
// #854: buffer-view backing accessor retained for the view subsystem
pub(crate) fn backing_of(buf_ptr: usize) -> usize {
    lookup(buf_ptr).map(|v| v.backing).unwrap_or(buf_ptr)
}

#[inline]
pub(crate) fn byte_offset_of(buf_ptr: usize) -> u32 {
    lookup(buf_ptr).map(|v| v.offset).unwrap_or(0)
}

/// Resolve `buf_ptr` to the canonical data pointer for its bytes.
///
/// A registered view (`Buffer.prototype.slice`/`subarray`,
/// `new Uint8Array(arrayBuffer)`, `new Uint8Array(ab, off, len)`) keeps its
/// own *copy* of the bytes so the codegen fast path can `gep+load` against the
/// view pointer, but the ultimate backing buffer is the source of truth:
/// `read_buffer_byte` reads and `js_buffer_set` writes `backing_data + offset`,
/// and a direct fast-path store to the backing never refreshes the view's
/// local copy. Any code that hands a raw span to a native callee must resolve
/// through here too — otherwise a native write lands in the view's stale local
/// copy while JS reads come from the backing (or vice-versa), a silent
/// corruption with no null and no error (#6515).
///
/// Falls back to the buffer's own inline storage for a plain (non-view) buffer,
/// or for a view whose recorded window no longer fits inside the backing (a
/// backing that was detached or shrunk since the view was registered). The
/// native callee consumes `(*buf_ptr).length` bytes — the byte-length half of
/// the ABI — so the whole `[offset, offset + length)` span must fit in the
/// current backing before we hand back a backing pointer; otherwise it could
/// read/write past the backing's end. The view's own storage is always sized
/// to its own length, so the fallback stays in bounds. The `<=` boundary lets a
/// zero-length view sitting exactly at `backing.length` still resolve to the
/// backing edge rather than falling back.
///
/// SAFETY: `buf_ptr` must be a live `BufferHeader`; a registered view's backing
/// is kept in the registry only while it is live (see
/// `remove_entries_for_dead_buffer`), the same invariant `read_buffer_byte`
/// and `js_buffer_set` already rely on.
pub(crate) unsafe fn resolve_data_ptr(buf_ptr: *const BufferHeader) -> *const u8 {
    if let Some(info) = lookup(buf_ptr as usize) {
        let backing_ptr = info.backing as *const BufferHeader;
        if !backing_ptr.is_null()
            && info.offset.saturating_add((*buf_ptr).length) <= (*backing_ptr).length
        {
            return buffer_data(backing_ptr).add(info.offset as usize);
        }
    }
    buffer_data(buf_ptr)
}

#[inline]
pub(crate) fn for_each_view<F: FnMut(usize, ViewInfo)>(backing_ptr: usize, mut f: F) {
    BACKING_TO_VIEWS.with(|m| {
        if let Some(views) = m.borrow().get(&backing_ptr) {
            for &view in views.iter() {
                if view != 0 {
                    if let Some(info) = lookup(view) {
                        f(view, info);
                    }
                }
            }
        }
    });
}

/// Drop every view-registry entry keyed by (or backed by) a dead buffer's
/// address (2026-07-09 audit, registry death pruning). A recycled address
/// would otherwise inherit stale view/backing metadata and misroute reads
/// and mirrored writes for the next tenant.
pub(crate) fn remove_entries_for_dead_buffer(addr: usize) {
    VIEW_REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.remove(&addr);
        r.retain(|_, info| info.backing != addr);
    });
    BACKING_TO_VIEWS.with(|m| {
        let mut m = m.borrow_mut();
        m.remove(&addr);
        for views in m.values_mut() {
            for view in views.iter_mut() {
                if *view == addr {
                    *view = 0;
                }
            }
        }
    });
}

/// Register a freshly-allocated `view_ptr` as a view over `backing_ptr`
/// at byte range `[offset, offset+length)`.  Resolves slices-of-slices
/// to the ultimate backing so reads/writes never walk a chain.
pub(crate) fn register(
    view_ptr: usize,
    backing_ptr_raw: usize,
    offset_raw: u32,
    length: u32,
) -> ViewInfo {
    // Walk through the chain so `slice.slice()` ends up pointing at
    // the original `Buffer.from(...)` allocation, not the intermediate
    // slice.  This keeps every mutation a single registry hop.
    let (backing, offset) = if let Some(parent) = lookup(backing_ptr_raw) {
        (parent.backing, parent.offset + offset_raw)
    } else {
        (backing_ptr_raw, offset_raw)
    };
    let info = ViewInfo {
        backing,
        offset,
        length,
    };
    VIEW_REGISTRY.with(|r| {
        r.borrow_mut().insert(view_ptr, info);
    });
    BACKING_TO_VIEWS.with(|m| {
        m.borrow_mut().entry(backing).or_default().push(view_ptr);
    });
    info
}

/// Write a single byte into every live view of `backing_ptr` whose
/// range covers `back_offset`.  Skips the originating `skip_view`
/// pointer so a view-originated write isn't double-applied to itself.
///
/// SAFETY: callers must guarantee that every recorded view pointer in
/// `BACKING_TO_VIEWS` still references a live `BufferHeader` allocation.
/// Slab/large buffers in Perry today live for the thread's lifetime
/// (see `buffer_alloc_small` and the malloc path), so that invariant
/// holds.
pub(crate) unsafe fn propagate_byte_to_views(
    backing_ptr: usize,
    back_offset: u32,
    value: u8,
    skip_view: usize,
) {
    for_each_view(backing_ptr, |view_ptr, info| {
        if view_ptr == skip_view {
            return;
        }
        if back_offset < info.offset {
            return;
        }
        let local = back_offset - info.offset;
        if local >= info.length {
            return;
        }
        let view_data = buffer_data_mut(view_ptr as *mut BufferHeader);
        *view_data.add(local as usize) = value;
    });
}

/// Write a range of bytes from `src` into every live view of
/// `backing_ptr` whose window overlaps `[back_offset, back_offset+len)`.
/// Used by `js_buffer_write`, `js_buffer_fill_range`, and the copy
/// helper so per-byte loops in user code don't have to call into the
/// registry for every store.
pub(crate) unsafe fn propagate_range_to_views(
    backing_ptr: usize,
    back_offset: u32,
    src: *const u8,
    len: u32,
    skip_view: usize,
) {
    if len == 0 || src.is_null() {
        return;
    }
    for_each_view(backing_ptr, |view_ptr, info| {
        if view_ptr == skip_view {
            return;
        }
        let view_start = info.offset;
        let view_end = info.offset + info.length;
        let back_start = back_offset;
        let back_end = back_offset + len;
        let lo = view_start.max(back_start);
        let hi = view_end.min(back_end);
        if lo >= hi {
            return;
        }
        let view_data = buffer_data_mut(view_ptr as *mut BufferHeader);
        let src_off = (lo - back_start) as usize;
        let view_off = (lo - view_start) as usize;
        let bytes = (hi - lo) as usize;
        std::ptr::copy_nonoverlapping(src.add(src_off), view_data.add(view_off), bytes);
    });
}

/// Mirror a just-written byte range from `receiver_ptr` into the ultimate
/// backing buffer and every other registered view. `local_offset` is relative
/// to the receiver's own visible window.
pub(crate) unsafe fn propagate_written_range_from_receiver(
    receiver_ptr: usize,
    local_offset: u32,
    src: *const u8,
    len: u32,
) {
    if len == 0 || src.is_null() {
        return;
    }
    if let Some(info) = lookup(receiver_ptr) {
        let back_offset = info.offset + local_offset;
        let backing_ptr = info.backing as *mut BufferHeader;
        if backing_ptr.is_null() || back_offset >= (*backing_ptr).length {
            return;
        }
        let bounded_len = len.min((*backing_ptr).length - back_offset);
        let backing_data = buffer_data_mut(backing_ptr).add(back_offset as usize);
        std::ptr::copy_nonoverlapping(src, backing_data, bounded_len as usize);
        propagate_range_to_views(info.backing, back_offset, src, bounded_len, receiver_ptr);
    } else {
        propagate_range_to_views(receiver_ptr, local_offset, src, len, receiver_ptr);
    }
}
