//! Resizable `ArrayBuffer` (ES2024): `new ArrayBuffer(len, { maxByteLength })`,
//! `ArrayBuffer.prototype.resize`, and the `resizable` / `maxByteLength`
//! getters (#10873).
//!
//! The owner reserves its maximum byte capacity at birth. Resize changes
//! only the owner length and zeroes each newly exposed range. Views compute
//! their current window from their owner's header; no reverse index or dirty
//! boundary metadata is retained.

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
    let buf = super::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER,
        max,
        super::store::Init::Uninit,
    );
    unsafe {
        super::store::set_length(buf as usize, len);
    }
    super::bytes::no_gc(|_| unsafe {
        let data = super::store::owner_data(buf as usize);
        if len > 0 {
            std::ptr::write_bytes(data, 0, len as usize);
        }
        let tail = (max - len) as usize;
        if tail >= DECOMMIT_MIN_BYTES {
            super::detach::decommit_payload_pages_zeroed(data.add(len as usize), tail);
        }
    });
    set_resizable(
        buf as usize,
        ResizableInfo {
            max_byte_length: max,
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
    let handles = crate::gc::RuntimeHandleScope::new();
    let receiver = handles.root_nanbox_f64(crate::value::js_nanbox_pointer(addr as i64));
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
    let buf = unsafe {
        super::store::owner(crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize)
    } as *mut BufferHeader;
    let old_len = unsafe { super::store::length(buf as usize) as u32 };
    if new_len == old_len {
        return undefined;
    }
    super::bytes::no_gc(|_| unsafe {
        let data = super::store::owner_data(buf as usize);
        if new_len > old_len {
            std::ptr::write_bytes(data.add(old_len as usize), 0, (new_len - old_len) as usize);
        }
        super::store::set_length(buf as usize, new_len);
        if new_len < old_len && (old_len - new_len) as usize >= DECOMMIT_MIN_BYTES {
            super::detach::decommit_payload_pages_zeroed(
                data.add(new_len as usize),
                (old_len - new_len) as usize,
            );
        }
    });
    undefined
}

/// The observable length (in elements) of a view over a resizable buffer whose
/// byteLength is now `buffer_len`, or `None` when the view is out of bounds.
///
/// `fixed_len` is the construction-time element count of a fixed-length view
/// and is ignored for a length-tracking one.
#[inline]
#[cfg(test)]
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
    is_data_view(addr) && super::view::is_out_of_bounds_view(addr)
}
