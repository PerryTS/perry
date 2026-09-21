//! Resizable `ArrayBuffer` (ES2024): `new ArrayBuffer(len, { maxByteLength })`,
//! `ArrayBuffer.prototype.resize`, and the `resizable` / `maxByteLength`
//! getters (#10873).
//!
//! # Storage model
//!
//! Buffer bytes live INLINE after the `BufferHeader`, and every view aliases
//! its backing by RAW ADDRESS (`view::ViewInfo`, `typedarray_view::ViewMeta`,
//! the DataView's cached data pointer). A resize therefore must never move the
//! payload. So a resizable buffer reserves `maxByteLength` bytes ONCE — its
//! `BufferHeader::capacity` — and `resize(n)` only rewrites
//! `BufferHeader::length`. Nothing is reallocated, and no address a view holds
//! can go stale.
//!
//! Bytes past `length` are never observable: a grow clears what it exposes. It
//! clears only what might be dirty, though — each buffer carries a `dirty_end`
//! boundary past which every byte is known to read as zero (`ResizableInfo`).
//! Construction touches only the initial `length` bytes, so a
//! `new ArrayBuffer(0, { maxByteLength: 64 MiB })` reserves address space, not
//! resident memory. A large shrink hands the dropped pages back to the OS (the
//! same `madvise` detach uses), so RSS follows `byteLength`, not the high-water
//! mark — and on Linux, where `MADV_DONTNEED` zero-fills on the next touch, it
//! also moves `dirty_end` back down, so regrowing costs nothing until the pages
//! are actually written. (macOS gives no such guarantee; there a regrow clears.)
//!
//! # Views
//!
//! A view's observable length is a function of its buffer's current
//! `byteLength` (ES2024 `IsTypedArrayOutOfBounds` / `TypedArrayLength`):
//! a *length-tracking* view (constructed without an explicit length) spans to
//! the end of the buffer, a *fixed-length* view keeps its length while it fits
//! and reads as length 0 / byteOffset 0 while it does not — and comes back when
//! the buffer grows again. Rather than teach every length read about that,
//! `resize` recomputes the header `length` of every registered view EAGERLY,
//! the way `detach` zeroes them. Every fast tier that reads a view's length
//! keeps working unchanged, and a program with no resizable buffer pays nothing:
//! the only probes on shared paths are gated on `header::any_resizable_buffer`.
//!
//! Codegen's inline element tiers are unaffected by construction: a resizable
//! buffer's bytes are only ever reachable through a VIEW, and every inline tier
//! already declines views (`u8_inline_cache` admits non-view buffers only;
//! the typed-array tiers require `PERRY_TA_VIEW_GUARD == 0`).

use super::*;

/// A dropped tail at least this large is handed back to the OS on shrink.
/// Below it the `madvise` syscall costs more than the pages are worth.
const DECOMMIT_MIN_BYTES: usize = 64 * 1024;

/// `ArrayBuffer.prototype.resizable`.
#[inline]
pub fn is_resizable_buffer(addr: usize) -> bool {
    resizable_max_byte_length(addr).is_some()
}

fn throw_type_error(message: &str) -> ! {
    crate::collection_iter::throw_type_error(message)
}

/// A plain V8-style `RangeError` — node attaches no `code` to these.
fn throw_invalid_length(message: &str) -> ! {
    crate::typedarray::throw_range_error(message.as_bytes())
}

/// AllocateArrayBuffer with a `maxByteLength`: reserve `max` bytes, expose
/// `len`. The caller has already established `len <= max`.
pub(crate) fn alloc_resizable_array_buffer(len: i32, max: i32) -> *mut BufferHeader {
    let len = len.max(0) as u32;
    let max = (max.max(0) as u32).max(len);
    let buf = buffer_alloc(max);
    let dirty_end = unsafe {
        (*buf).length = len;
        let data = buffer_data_mut(buf);
        // Only the visible prefix needs zeroing: `resize` clears whatever a
        // later grow exposes, so the reserved tail is never observable.
        if len > 0 {
            std::ptr::write_bytes(data, 0, len as usize);
        }
        // The old arena can hand back a recycled hole, so the reserved tail is
        // not known-zero yet. For a large reservation make it so up front —
        // releasing pages that were never touched is nearly free — so growing
        // into it never has to clear (touch) it.
        let tail = (max - len) as usize;
        if tail >= DECOMMIT_MIN_BYTES
            && super::detach::decommit_payload_pages_zeroed(data.add(len as usize), tail)
        {
            len
        } else {
            max
        }
    };
    mark_as_array_buffer(buf as usize);
    mark_as_resizable_buffer(
        buf as usize,
        ResizableInfo {
            max_byte_length: max,
            dirty_end,
        },
    );
    buf
}

/// GetArrayBufferMaxByteLengthOption(options): `None` unless `options` is an
/// object whose `maxByteLength` is not `undefined`. The `Get` and the `ToIndex`
/// both run user code, in that order.
fn max_byte_length_option(options: f64) -> Option<i32> {
    if !crate::value::JSValue::from_bits(options.to_bits()).is_pointer() {
        return None;
    }
    // Interning the key allocates, and `options` is an ordinary (movable)
    // object: hold it in a handle and re-read it after the allocation.
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let key = crate::string::js_string_from_bytes(b"maxByteLength".as_ptr(), 13);
    let obj = crate::value::js_nanbox_get_pointer(options.get_nanbox_f64()) as usize;
    if obj == 0 {
        return None;
    }
    let value =
        crate::object::js_object_get_field_by_name(obj as *const crate::object::ObjectHeader, key);
    if value.is_undefined() {
        return None;
    }
    Some(super::from::array_buffer_to_index(f64::from_bits(
        value.bits(),
    )))
}

/// `new ArrayBuffer(length, options)`. Spec order: `ToIndex(length)`, then the
/// `maxByteLength` option read + `ToIndex`, then the `length > max` RangeError.
#[no_mangle]
pub extern "C" fn js_array_buffer_new_with_options(
    size_value: f64,
    options: f64,
) -> *mut BufferHeader {
    // `ToIndex(length)` can run a user `valueOf`, i.e. collect: root the
    // options bag across it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let len = super::from::array_buffer_to_index(size_value);
    match max_byte_length_option(options.get_nanbox_f64()) {
        None => super::from::js_array_buffer_new(len),
        Some(max) => {
            if len > max {
                throw_invalid_length("Invalid array buffer max length");
            }
            alloc_resizable_array_buffer(len, max)
        }
    }
}

/// `ArrayBuffer.prototype.resize(newLength)`.
pub(crate) fn array_buffer_resize(addr: usize, args: &[f64]) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    // RequireInternalSlot(O, [[ArrayBufferMaxByteLength]]) precedes ToIndex.
    let Some(info) = resizable_info(addr) else {
        throw_type_error("Method ArrayBuffer.prototype.resize called on incompatible receiver");
    };
    let max = info.max_byte_length;
    // ToIndex can run user code that detaches this very buffer; the detached
    // check reads post-coercion state.
    let new_len = super::from::array_buffer_to_index(args.first().copied().unwrap_or(undefined));
    if super::detach::is_detached_buffer(addr) {
        throw_type_error("Cannot perform ArrayBuffer.prototype.resize on a detached ArrayBuffer");
    }
    let new_len = new_len as u32;
    if new_len > max {
        throw_invalid_length("ArrayBuffer.prototype.resize: Invalid length parameter");
    }
    let buf = addr as *mut BufferHeader;
    let old_len = unsafe { (*buf).length };
    if new_len == old_len {
        return undefined;
    }
    // `dirty_end`: every byte at or past it already reads as zero. A grow
    // clears only what lies below it; a large shrink releases the dropped
    // pages and, where the OS then guarantees zero-fill, pulls it back down.
    let mut dirty_end = info.dirty_end.max(old_len);
    unsafe {
        let data = buffer_data_mut(buf);
        if new_len > old_len {
            let clear_to = new_len.min(dirty_end);
            if clear_to > old_len {
                std::ptr::write_bytes(data.add(old_len as usize), 0, (clear_to - old_len) as usize);
            }
            dirty_end = dirty_end.max(new_len);
        }
        (*buf).length = new_len;
        if new_len < old_len && (old_len - new_len) as usize >= DECOMMIT_MIN_BYTES {
            let zeroed = super::detach::decommit_payload_pages_zeroed(
                data.add(new_len as usize),
                (old_len - new_len) as usize,
            );
            // Only when nothing dirty lies beyond the range just released.
            if zeroed && dirty_end <= old_len {
                dirty_end = new_len;
            }
        }
    }
    if dirty_end != info.dirty_end {
        set_resizable_dirty_end(addr, dirty_end);
    }
    super::view::relength_views_of_resized_backing(addr, new_len);
    crate::typedarray_view::relength_views_of_resized_backing(addr, new_len);
    undefined
}

/// The observable length (in elements) of a view over a resizable buffer whose
/// byteLength is now `buffer_len`, or `None` when the view is out of bounds.
///
/// `fixed_len` is the construction-time element count of a fixed-length view
/// and is ignored for a length-tracking one.
#[inline]
pub(crate) fn view_length_after_resize(
    buffer_len: u32,
    byte_offset: u32,
    elem_size: u32,
    length_tracking: bool,
    fixed_len: u32,
) -> Option<u32> {
    let elem_size = elem_size.max(1) as u64;
    let (buffer_len, byte_offset) = (buffer_len as u64, byte_offset as u64);
    if byte_offset > buffer_len {
        return None;
    }
    if length_tracking {
        return Some(((buffer_len - byte_offset) / elem_size) as u32);
    }
    if byte_offset + fixed_len as u64 * elem_size > buffer_len {
        return None;
    }
    Some(fixed_len)
}

/// A DataView whose resizable buffer shrank past it. Its `byteLength` /
/// `byteOffset` getters and every accessor throw a TypeError, where a typed
/// array in the same state merely reads as empty.
#[inline]
pub fn is_out_of_bounds_data_view(addr: usize) -> bool {
    any_resizable_buffer() && is_data_view(addr) && super::view::is_out_of_bounds_view(addr)
}
