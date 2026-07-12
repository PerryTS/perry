//! ArrayBuffer detach state and `ArrayBuffer.prototype.transfer` /
//! `transferToFixedLength` / `detached` (ES2024).
//!
//! Buffer bytes live INLINE after the `BufferHeader` in a GC old-arena
//! allocation, so a detached buffer's storage cannot be individually freed
//! while the JS object is alive. Detach therefore (1) zeroes the header —
//! the pre-existing structuredClone-transfer convention, which makes
//! `byteLength` read 0 — (2) zeroes every registered view's length so views
//! over the detached buffer report length 0 like Node, and (3) hands the
//! page-aligned interior of the payload back to the OS with `madvise`, so a
//! large detached buffer stops costing RSS immediately even while the
//! ArrayBuffer object itself is still reachable. The GcHeader `size` field
//! is left untouched: the arena sweep still steps over the full allocation,
//! and the decommitted pages stay mapped (later reads are legal and return
//! zeros), so the only observable effect is RSS dropping.

use super::*;
use crate::fast_hash::{new_ptr_hash_set, PtrHashSet};
use std::cell::RefCell;

thread_local! {
    /// Buffers detached via `transfer`/`transferToFixedLength`/structuredClone
    /// transfer. A detached buffer also has `length == capacity == 0`, but that
    /// alone cannot be the probe: `new ArrayBuffer(0)` is empty yet NOT
    /// detached.
    static DETACHED_BUFFER_REGISTRY: RefCell<PtrHashSet<usize>> =
        RefCell::new(new_ptr_hash_set());
}

/// `ArrayBuffer.prototype.detached` — true after a successful transfer.
pub fn is_detached_buffer(addr: usize) -> bool {
    DETACHED_BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr))
}

/// Drop the detached mark when the buffer dies — a recycled address would
/// otherwise inherit detached-ness (the #6080 ABA class).
pub(crate) fn remove_detached_entry_for_dead_buffer(addr: usize) {
    DETACHED_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
}

/// DetachArrayBuffer(buffer): idempotent.
pub fn detach_array_buffer(addr: usize) {
    if is_detached_buffer(addr) {
        return;
    }
    let buf = addr as *mut BufferHeader;
    let capacity = unsafe { (*buf).capacity };
    unsafe {
        (*buf).length = 0;
        (*buf).capacity = 0;
    }
    DETACHED_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr);
    });
    // Buffer-shaped views (`new Uint8Array(ab)`, DataView slices): zero their
    // own header lengths so `.length`/`.byteLength` report 0 and every indexed
    // access is out-of-bounds, matching Node's view-over-detached semantics.
    // `ArrayBuffer.prototype.slice` results also land in the view table (the
    // Buffer.slice aliasing mechanism registers them), but they are
    // independent COPIES per spec and must survive the source's detach —
    // they're the only view-table entries marked as ArrayBuffers, so skip
    // those.
    super::view::for_each_view(addr, |view_ptr, _info| {
        if is_array_buffer(view_ptr) {
            return;
        }
        unsafe {
            (*(view_ptr as *mut BufferHeader)).length = 0;
        }
    });
    // Typed-array views (`new Float32Array(ab, ...)`) record their backing in
    // a separate side table; zero those lengths too.
    crate::typedarray_view::zero_views_of_detached_backing(addr);
    decommit_payload_pages(buffer_data_mut(buf), capacity as usize);
}

/// Release the page-aligned interior of a detached payload back to the OS.
/// Rounds INWARD (start up, end down), so only pages lying entirely inside
/// `[data, data + capacity)` are touched — the BufferHeader, the GcHeader in
/// front of it, and any neighbor allocations on the boundary pages are never
/// affected. Failure is harmless (the advice is best-effort), so the return
/// value is ignored.
#[cfg(unix)]
fn decommit_payload_pages(data: *mut u8, capacity: usize) {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return;
    }
    let page = page as usize;
    let start = (data as usize).wrapping_add(page - 1) & !(page - 1);
    let end = (data as usize + capacity) & !(page - 1);
    if end <= start {
        return;
    }
    unsafe {
        // macOS: MADV_FREE_REUSABLE drops the pages from the process
        // footprint immediately (plain MADV_FREE only reclaims under
        // memory pressure, so RSS wouldn't visibly shrink). It can fail
        // on some region types — fall back to MADV_FREE then.
        #[cfg(target_os = "macos")]
        {
            let len = end - start;
            if libc::madvise(start as *mut libc::c_void, len, libc::MADV_FREE_REUSABLE) != 0 {
                libc::madvise(start as *mut libc::c_void, len, libc::MADV_FREE);
            }
        }
        // Linux (and other unix): MADV_DONTNEED drops the pages (and RSS)
        // immediately; later reads legally return zeros.
        #[cfg(not(target_os = "macos"))]
        {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
}

#[cfg(not(unix))]
fn decommit_payload_pages(_data: *mut u8, _capacity: usize) {}

fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"TypeError", msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `ArrayBuffer.prototype.transfer(newLength?)` and `transferToFixedLength`.
/// Perry has no resizable ArrayBuffers, so both produce a fixed-length result
/// and are identical: allocate a zero-filled buffer of `newLength` (default:
/// the current byteLength), copy `min(oldLength, newLength)` bytes, detach the
/// source, and return the new buffer.
pub(crate) fn array_buffer_transfer(addr: usize, args: &[f64]) -> f64 {
    // ES2024 ArrayBufferCopyAndDetach ordering: ToIndex(newLength) runs FIRST
    // — it can execute user code (`valueOf`) that detaches this very buffer —
    // and IsDetachedBuffer is checked after, so a mid-coercion detach is
    // caught before any stale header read.
    let requested_len = match args.first().copied() {
        Some(v) if !crate::value::JSValue::from_bits(v.to_bits()).is_undefined() => {
            Some(super::from::array_buffer_to_index(v))
        }
        _ => None,
    };
    if is_detached_buffer(addr) {
        throw_type_error("Cannot perform ArrayBuffer.prototype.transfer on a detached ArrayBuffer");
    }
    let src = addr as *mut BufferHeader;
    let old_len = unsafe { (*src).length } as i32;
    let new_len = requested_len.unwrap_or(old_len);
    let dst = super::from::zeroed_array_buffer_storage(new_len);
    mark_as_array_buffer(dst as usize);
    let copy_len = old_len.min(new_len);
    if copy_len > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                buffer_data(src),
                buffer_data_mut(dst),
                copy_len as usize,
            );
        }
    }
    detach_array_buffer(addr);
    f64::from_bits(crate::value::JSValue::pointer(dst as *mut u8).bits())
}
