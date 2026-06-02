//! Buffer surface — `perry-ffi`'s canonical `BufferHeader` plus thin
//! allocator + reader helpers for wrappers that return
//! arbitrary binary bytes (cryptographic digests, image-encoded
//! payloads, BSON documents, …).
//!
//! # Why
//!
//! Some wrappers need to surface raw bytes to user code as a
//! `Buffer` / `Uint8Array`, not as a JS string — UTF-8 validation
//! would either reject the payload outright (binary data) or
//! silently mojibake it (lossy decode). `BufferHeader` is the
//! runtime's representation; the wrappers return a NaN-boxed
//! pointer to one as a `JsValue::from_object_ptr(...)`.
//!
//! Today's surface is intentionally minimal: the runtime type +
//! a single allocator + a slice-reader. Resize / append / clone
//! helpers wait for a real wrapper that demands them.

use crate::BufferHeader;

extern "C" {
    /// Runtime's stable extern allocator entry point. Single
    /// symbol shared across the whole binary — going through
    /// here (vs. `perry_runtime::buffer::buffer_alloc`) means the
    /// allocation lands in the SAME thread-local slab + registry
    /// the dispatch path checks. Without this, wrappers compiled
    /// into separate rlibs (perry-ext-net, etc.) would each get
    /// their own monomorphized `buffer_alloc` copy with private
    /// thread-locals, and the runtime's `is_registered_buffer`
    /// dispatch would miss buffers allocated by external wrappers
    /// (small buffers in particular — they live in a per-thread
    /// slab that the dispatch checks via address-range lookup).
    fn js_buffer_alloc(size: i32, fill: i32) -> *mut BufferHeader;
}

/// Allocate a fresh `BufferHeader` from a byte slice. The
/// runtime arena owns the storage; GC reclaims it when no live
/// reference remains.
///
/// The returned pointer is suitable for handing back to user code
/// as `JsValue::from_object_ptr(buf_ptr)` — the JS-side wrapper
/// sees a `Buffer` / `Uint8Array` it can index directly.
///
/// ```ignore
/// // Typical wrapper exit path:
/// let digest = sha256(input_bytes);
/// let buf = perry_ffi::alloc_buffer(&digest);
/// JsValue::from_object_ptr(buf).bits()  // promise.resolve(...)
/// ```
pub fn alloc_buffer(bytes: &[u8]) -> *mut BufferHeader {
    let len = bytes.len() as i32;
    // SAFETY: `js_buffer_alloc` is the runtime's stable extern
    // allocator. Going through the extern symbol (not the Rust
    // function) means small buffers land in the runtime's
    // per-thread slab that the dispatch path can find via
    // `is_registered_buffer` — fixes the v0.5.572 regression where
    // small Buffer payloads from perry-ext-net's `data` events
    // arrived in user code as `[object Object]` because the
    // dispatch couldn't tell them apart from raw heap pointers.
    unsafe {
        let buf = js_buffer_alloc(len, 0);
        if buf.is_null() {
            return buf;
        }
        // js_buffer_alloc already sets length=size and fills with
        // 0; overwrite payload with the actual bytes.
        let dst = (buf as *mut u8).add(std::mem::size_of::<BufferHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, len as usize);
        buf
    }
}

/// Read the bytes out of a `BufferHeader` as a borrowed `&[u8]`.
/// Returns `None` on a null pointer. The borrow is valid for the
/// duration of the calling FFI invocation (matches
/// `read_string` / `read_bytes`).
pub fn read_buffer_bytes(ptr: *const BufferHeader) -> Option<&'static [u8]> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller's contract — `ptr` is a valid runtime
    // BufferHeader. The bytes immediately follow the header and
    // are bounded by `length`.
    unsafe {
        let header = &*ptr;
        let len = header.length as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<BufferHeader>());
        Some(std::slice::from_raw_parts(data, len))
    }
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;

    #[test]
    fn round_trip_bytes() {
        let input: &[u8] = b"hello, perry-ffi";
        let buf = alloc_buffer(input);
        assert!(!buf.is_null());
        let read = read_buffer_bytes(buf).expect("non-null");
        assert_eq!(read, input);
    }

    #[test]
    fn empty_buffer_round_trips() {
        let buf = alloc_buffer(&[]);
        assert!(!buf.is_null());
        let read = read_buffer_bytes(buf).expect("non-null");
        assert_eq!(read, &[] as &[u8]);
    }

    #[test]
    fn null_returns_none() {
        assert!(read_buffer_bytes(std::ptr::null()).is_none());
    }

    #[test]
    fn binary_bytes_round_trip() {
        // Non-UTF-8 binary data — what BigInt-as-bytes / image
        // payloads actually look like.
        let input: &[u8] = &[0xFF, 0x00, 0x80, 0x7F, 0xFE, 0xC0];
        let buf = alloc_buffer(input);
        let read = read_buffer_bytes(buf).expect("non-null");
        assert_eq!(read, input);
    }
}
