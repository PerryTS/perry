//! ArrayBuffer detach state and `ArrayBuffer.prototype.transfer` /
//! `transferToFixedLength` / `detached` (ES2024).
//!
//! Owned native bytes are freed (or taken by the transfer message). Inline
//! bytes live after the common 16-byte cell in a GC old-arena
//! allocation, so a detached buffer's storage cannot be individually freed
//! while the JS object is alive. Detach therefore (1) zeroes the header —
//! the structuredClone-transfer convention, which makes `byteLength` read 0 —
//! (2) marks the owner detached, which its views check on access, and
//! (3) hands the
//! page-aligned interior of the payload back to the OS with `madvise`, so a
//! large detached buffer stops costing RSS immediately even while the
//! ArrayBuffer object itself is still reachable. The GcHeader `size` field
//! is left untouched: the arena sweep still steps over the full allocation,
//! and the decommitted pages stay mapped (later reads are legal and return
//! zeros), so the only observable effect is RSS dropping.

use super::*;
// Per-buffer header bit; disjoint from byte pins and the storage-layout bits.
pub(crate) const DETACHED: u16 = 1 << 14;

/// Detached state is born and dies with the store owner, never its address.
#[inline]
pub fn is_detached_buffer(addr: usize) -> bool {
    if super::header::byte_cell_type(addr).is_none() {
        return false;
    }
    unsafe { (*super::store::header(super::store::owner(addr)))._reserved & DETACHED != 0 }
}

/// DetachArrayBuffer(buffer): idempotent.
pub fn detach_array_buffer(addr: usize) {
    if is_detached_buffer(addr) {
        return;
    }
    let backing = super::view::backing_of(addr);
    let buf = backing as *mut BufferHeader;
    let capacity = unsafe { (*buf).capacity };
    unsafe {
        super::store::clear_owner_extent(buf as usize);
    }
    #[cfg(test)]
    let mark_detached = !super::bytes::b4_sabotage("detach_mark");
    #[cfg(not(test))]
    let mark_detached = true;
    if mark_detached {
        unsafe {
            (*crate::gc::header_from_trusted_user_ptr(backing as *const u8).cast_mut())
                ._reserved |= DETACHED;
            (*crate::gc::header_from_trusted_user_ptr(addr as *const u8).cast_mut())._reserved |=
                DETACHED;
        }
    }
    // Native-owned bytes are released on detach unless the message took them.
    drop(super::header::take_owned_backing(backing));
    // External ArrayBuffers borrow addon-owned memory. Detaching severs the
    // JavaScript view but must never decommit pages which Perry did not
    // allocate; the registered finalizer still receives the original pointer.
    #[cfg(test)]
    let retain_pinned_inline = !super::bytes::b4_sabotage("inline_detach_decommit");
    #[cfg(not(test))]
    let retain_pinned_inline = true;
    if !super::is_foreign_backed_buffer(backing)
        && (!super::bytes::has_pins(backing) || !retain_pinned_inline)
    {
        super::bytes::no_gc(|_| unsafe {
            decommit_payload_pages(super::store::owner_data(backing), capacity as usize);
        });
    }
}

/// Release the page-aligned interior of a detached payload back to the OS.
/// Rounds INWARD (start up, end down), so only pages lying entirely inside
/// `[data, data + capacity)` are touched — the BufferHeader, the GcHeader in
/// front of it, and any neighbor allocations on the boundary pages are never
/// affected. Failure is harmless (the advice is best-effort), so the return
/// value is ignored.
#[cfg(unix)]
pub(super) fn decommit_payload_pages(data: *mut u8, capacity: usize) {
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
pub(super) fn decommit_payload_pages(_data: *mut u8, _capacity: usize) {}

/// Release `[data, data + len)` AND report whether every byte of it is now
/// guaranteed to read as zero (#10873: what lets a resizable buffer regrow into
/// the range without clearing — i.e. without touching — it).
///
/// Linux only: `MADV_DONTNEED` on private anonymous memory is specified to
/// zero-fill on the next touch, so the whole pages go back to the OS and only
/// the two partial edge pages (< 2 pages) are cleared by hand. macOS's
/// `MADV_FREE_REUSABLE` makes no such promise (a page not yet reclaimed keeps
/// its bytes), so there this only releases and answers `false`.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn decommit_payload_pages_zeroed(data: *mut u8, len: usize) -> bool {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return false;
    }
    let page = page as usize;
    let begin = data as usize;
    let start = begin.wrapping_add(page - 1) & !(page - 1);
    let end = (begin + len) & !(page - 1);
    if end <= start {
        return false;
    }
    unsafe {
        if libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED) != 0 {
            return false;
        }
        std::ptr::write_bytes(data, 0, start - begin);
        std::ptr::write_bytes(end as *mut u8, 0, begin + len - end);
    }
    true
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub(super) fn decommit_payload_pages_zeroed(data: *mut u8, len: usize) -> bool {
    decommit_payload_pages(data, len);
    false
}

fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"TypeError", msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `ArrayBuffer.prototype.transfer(newLength?)` and `transferToFixedLength`
/// (ES2024 ArrayBufferCopyAndDetach): allocate a zero-filled buffer of
/// `newLength` (default: the current byteLength), copy
/// `min(oldLength, newLength)` bytes, detach the source, and return the new
/// buffer. `transfer` preserves resizability — a resizable source yields a
/// resizable result with the same `maxByteLength` (and a `newLength` past it is
/// a RangeError) — while `transferToFixedLength` always yields a fixed-length
/// one. Over a fixed-length source the two are identical.
pub(crate) fn array_buffer_transfer(addr: usize, args: &[f64], preserve_resizability: bool) -> f64 {
    let handles = crate::gc::RuntimeHandleScope::new();
    let source = handles.root_raw_mut_ptr(addr as *mut BufferHeader);
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
    let old_len = unsafe { super::store::length(src as usize) } as i32;
    let new_len = requested_len.unwrap_or(old_len);
    let preserved_max = if preserve_resizability {
        super::resizable_max_byte_length(addr)
    } else {
        None
    };
    let dst = match preserved_max {
        Some(max) => {
            if new_len as i64 > max as i64 {
                crate::typedarray::throw_range_error(b"Invalid array buffer length");
            }
            // Allocates: re-read nothing from `src` across this call other
            // than through its (non-moving, old-arena) address.
            super::resizable::alloc_resizable_array_buffer(new_len, max as i32)
        }
        None => {
            let dst = super::from::zeroed_array_buffer_storage(new_len);
            mark_as_array_buffer(dst as usize);
            dst
        }
    };
    let copy_len = old_len.min(new_len);
    if copy_len > 0 {
        super::bytes::no_gc(|scope| unsafe {
            let src = super::bytes::bytes(
                crate::value::js_nanbox_pointer(source.get_raw_mut_ptr::<BufferHeader>() as i64),
                scope,
            )
            .unwrap();
            let dst = super::bytes::bytes_mut(crate::value::js_nanbox_pointer(dst as i64), scope)
                .unwrap();
            dst[..copy_len as usize].copy_from_slice(&src[..copy_len as usize]);
        });
    }
    detach_array_buffer(addr);
    f64::from_bits(crate::value::JSValue::pointer(dst as *mut u8).bits())
}
