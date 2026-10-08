//! Compatibility pointer surface over the byte API. The cell is opaque.
use crate::BufferHeader;

/// Copy a Rust byte slice into an opaque runtime-owned Buffer.
pub fn alloc_buffer(input: &[u8]) -> *mut BufferHeader {
    crate::bytes::from_slice(crate::bytes::Brand::Buffer, input).as_pointer()
}
/// Borrow a live byte window, ending before allocation, JS, or a safepoint.
pub fn read_buffer_bytes<'s>(
    ptr: *const BufferHeader,
    scope: &'s crate::bytes::NoGc<'s>,
) -> Option<&'s [u8]> {
    crate::bytes::borrow(
        crate::JsValue::from_object_ptr(ptr as *mut BufferHeader),
        scope,
    )
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;

    #[test]
    fn round_trip_bytes() {
        let input: &[u8] = b"hello, perry-ffi";
        let buf = alloc_buffer(input);
        assert!(!buf.is_null());
        let read =
            crate::bytes::no_gc(|scope| read_buffer_bytes(buf, scope).expect("non-null").to_vec());
        assert_eq!(read, input);
    }

    #[test]
    fn empty_buffer_round_trips() {
        let buf = alloc_buffer(&[]);
        assert!(!buf.is_null());
        let read =
            crate::bytes::no_gc(|scope| read_buffer_bytes(buf, scope).expect("non-null").to_vec());
        assert_eq!(read, &[] as &[u8]);
    }

    #[test]
    fn null_returns_none() {
        crate::bytes::no_gc(|scope| assert!(read_buffer_bytes(std::ptr::null(), scope).is_none()));
    }

    #[test]
    fn binary_bytes_round_trip() {
        // Non-UTF-8 binary data — what BigInt-as-bytes / image
        // payloads actually look like.
        let input: &[u8] = &[0xFF, 0x00, 0x80, 0x7F, 0xFE, 0xC0];
        let buf = alloc_buffer(input);
        let read =
            crate::bytes::no_gc(|scope| read_buffer_bytes(buf, scope).expect("non-null").to_vec());
        assert_eq!(read, input);
    }

    #[test]
    fn subarray_reads_live_backing_window() {
        let source = crate::bytes::from_slice(crate::bytes::Brand::Uint8Array, &[10, 20, 30, 40])
            .as_pointer::<BufferHeader>();
        let view = perry_runtime::buffer::js_buffer_slice(source.cast(), 1, 3);
        crate::bytes::no_gc(|scope| {
            assert_eq!(read_buffer_bytes(view.cast(), scope).unwrap(), &[20, 30])
        });
        perry_runtime::buffer::js_buffer_set(source.cast(), 1, 99);
        crate::bytes::no_gc(|scope| {
            assert_eq!(read_buffer_bytes(view.cast(), scope).unwrap(), &[99, 30])
        });
        crate::bytes::no_gc(|scope| {
            assert_eq!(read_buffer_bytes(view.cast(), scope).unwrap(), &[99, 30])
        });
        let empty = perry_runtime::buffer::js_buffer_slice(view, 2, 2);
        crate::bytes::no_gc(|scope| {
            assert_eq!(
                read_buffer_bytes(empty.cast(), scope).unwrap(),
                &[] as &[u8]
            )
        });
    }
}
