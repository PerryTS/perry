//! Buffer module - provides binary data handling similar to Node.js Buffer

use std::alloc::{alloc, Layout};
use std::ptr;

use crate::array::ArrayHeader;
use crate::string::{
    js_string_alloc_ascii_uninit, js_string_from_ascii_bytes, js_string_from_bytes, StringHeader,
};

mod access;
mod cmp;
mod coding;
mod copy_write;
mod encode;
mod from;
mod header;
mod iter;
mod mutate;
mod numeric;
mod query;
mod transcode;
mod view;

// ---- Re-exports: types & constants ----
pub use header::{BufferHeader, BUFFER_TYPE_ID, SMALL_BUF_THRESHOLD};

// ---- Re-exports: allocation / registry helpers ----
pub use header::{
    buffer_alloc, buffer_data, buffer_data_mut, is_array_buffer, is_registered_buffer,
    is_uint8array_buffer, mark_as_array_buffer, mark_as_uint8array, register_buffer,
};

// ---- Re-exports: Buffer.from / alloc / concat (FFI) ----
pub use from::{
    js_array_buffer_new, js_buffer_alloc, js_buffer_alloc_fill_value, js_buffer_alloc_unsafe,
    js_buffer_concat, js_buffer_fill, js_buffer_fill_range, js_buffer_from_array,
    js_buffer_from_arraybuffer_slice, js_buffer_from_string, js_buffer_from_value,
    js_encoding_tag_from_value, js_uint8array_alloc, js_uint8array_from_array, js_uint8array_new,
};

// ---- Re-exports: predicates / byteLength (FFI) ----
pub use query::{
    js_buffer_byte_length, js_buffer_byte_length_value, js_buffer_is_ascii, js_buffer_is_buffer,
    js_buffer_is_encoding, js_buffer_is_utf8,
};

// ---- Re-exports: toString / print / length / to-array ----
pub use encode::{
    buffer_to_array, js_buffer_length, js_buffer_print, js_buffer_to_string,
    js_buffer_to_string_range, js_value_to_string_with_encoding,
};

// ---- Re-exports: indexed access / slice / Uint8Array.set ----
pub use access::{js_buffer_get, js_buffer_set, js_buffer_set_from, js_buffer_slice};

// ---- Re-exports: copy / write ----
pub use copy_write::{js_buffer_copy, js_buffer_write, js_buffer_write_len};

// ---- Re-exports: compare / search ----
pub use cmp::{
    js_buffer_compare, js_buffer_compare_range, js_buffer_equals, js_buffer_includes,
    js_buffer_includes_enc, js_buffer_index_of, js_buffer_index_of_enc, js_buffer_last_index_of,
    js_buffer_last_index_of_enc, js_buffer_to_json, unbox_buffer_ptr,
};

// ---- Re-exports: random / swap mutators ----
pub use mutate::{js_buffer_fill_random, js_buffer_swap16, js_buffer_swap32, js_buffer_swap64};

// ---- Re-exports: numeric read/write (typed-array view ops) ----
pub use numeric::{
    js_buffer_read_bigint64_be, js_buffer_read_bigint64_le, js_buffer_read_biguint64_be,
    js_buffer_read_biguint64_le, js_buffer_read_double_be, js_buffer_read_double_le,
    js_buffer_read_float_be, js_buffer_read_float_le, js_buffer_read_int16_be,
    js_buffer_read_int16_le, js_buffer_read_int32_be, js_buffer_read_int32_le, js_buffer_read_int8,
    js_buffer_read_int_be, js_buffer_read_int_le, js_buffer_read_uint16_be,
    js_buffer_read_uint16_le, js_buffer_read_uint32_be, js_buffer_read_uint32_le,
    js_buffer_read_uint8, js_buffer_read_uint_be, js_buffer_read_uint_le,
    js_buffer_write_bigint64_be, js_buffer_write_bigint64_le, js_buffer_write_biguint64_be,
    js_buffer_write_biguint64_le, js_buffer_write_double_be, js_buffer_write_double_le,
    js_buffer_write_float_be, js_buffer_write_float_le, js_buffer_write_int16_be,
    js_buffer_write_int16_le, js_buffer_write_int32_be, js_buffer_write_int32_le,
    js_buffer_write_int8, js_buffer_write_int_be, js_buffer_write_int_le,
    js_buffer_write_uint16_be, js_buffer_write_uint16_le, js_buffer_write_uint32_be,
    js_buffer_write_uint32_le, js_buffer_write_uint8, js_buffer_write_uint_be,
    js_buffer_write_uint_le,
};

// ---- Re-exports: hex / base64 codec helpers ----
pub use coding::{
    base64_decode_into_buffer, base64_encode_into_string, base64url_encode_into_string,
    decode_base64, decode_hex, hex_decode_into_buffer, hex_encode_into_string,
};

// ---- Re-exports: transcode (FFI) ----
pub use transcode::js_buffer_transcode;

// ---- Re-exports: iterator surface (FFI + dispatch hook) ----
pub use iter::{
    dispatch_buffer_iterator_method, js_buffer_entries, js_buffer_keys, js_buffer_values,
    BUFFER_ITERATOR_CLASS_ID,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_buffer_slab_unique_addresses() {
        // Every allocation must occupy a distinct address (no overlap).
        let n = 1000usize;
        let mut ptrs: Vec<*mut BufferHeader> = Vec::new();
        for i in 0..n {
            let cap = (i % (SMALL_BUF_THRESHOLD as usize)) as u32;
            let buf = buffer_alloc(cap);
            assert!(!buf.is_null(), "slab alloc returned null at i={}", i);
            ptrs.push(buf);
        }
        let addrs: std::collections::HashSet<usize> = ptrs.iter().map(|&p| p as usize).collect();
        assert_eq!(
            addrs.len(),
            n,
            "slab allocations must have unique addresses"
        );
    }

    #[test]
    fn test_small_buffer_slab_is_registered() {
        // All slab-allocated buffers must be recognised as buffers.
        for cap in [0u32, 1, 15, 16, 127, 255] {
            let buf = buffer_alloc(cap);
            assert!(
                is_registered_buffer(buf as usize),
                "cap={cap}: slab buffer not recognised by is_registered_buffer"
            );
            assert_eq!(
                unsafe { (*buf).capacity },
                cap,
                "cap={cap}: wrong capacity stored in header"
            );
        }
    }

    #[test]
    fn test_large_buffer_still_registered() {
        // Buffers at or above the threshold still go through the HashSet path.
        let buf = buffer_alloc(SMALL_BUF_THRESHOLD);
        assert!(!buf.is_null());
        assert!(
            is_registered_buffer(buf as usize),
            "large buffer not in BUFFER_REGISTRY"
        );
        assert_eq!(unsafe { (*buf).capacity }, SMALL_BUF_THRESHOLD);
    }

    #[test]
    fn large_object_buffer_alloc_uses_old_gc_header_and_stays_usable() {
        let cap = crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as u32;
        let buf = buffer_alloc(cap);
        assert!(!buf.is_null());
        assert!(is_registered_buffer(buf as usize));
        assert!(crate::arena::pointer_in_old_gen(buf as usize));
        unsafe {
            let header =
                (buf as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            assert_eq!((*header).obj_type, crate::gc::GC_TYPE_BUFFER);
            assert_ne!((*header).gc_flags & crate::gc::GC_FLAG_TENURED, 0);
            (*buf).length = cap;
        }

        js_buffer_set(buf, 0, 0x12);
        js_buffer_set(buf, cap as i32 - 1, 0x34);
        assert_eq!(js_buffer_get(buf, 0), 0x12);
        assert_eq!(js_buffer_get(buf, cap as i32 - 1), 0x34);
    }

    #[test]
    fn test_buffer_alloc() {
        let buf = js_buffer_alloc(10, 0);
        assert_eq!(js_buffer_length(buf), 10);
        for i in 0..10 {
            assert_eq!(js_buffer_get(buf, i), 0);
        }
    }

    #[test]
    fn test_buffer_alloc_with_fill() {
        let buf = js_buffer_alloc(5, 0x42);
        assert_eq!(js_buffer_length(buf), 5);
        for i in 0..5 {
            assert_eq!(js_buffer_get(buf, i), 0x42);
        }
    }

    #[test]
    fn test_buffer_get_set() {
        let buf = js_buffer_alloc(5, 0);
        js_buffer_set(buf, 2, 0x42);
        assert_eq!(js_buffer_get(buf, 2), 0x42);
    }

    #[test]
    fn test_hex_encode_decode() {
        let original = b"Hello";
        let encoded = coding::encode_hex(original);
        let decoded = decode_hex(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_encode_decode() {
        let original = b"Hello, World!";
        let encoded = coding::encode_base64(original);
        let decoded = decode_base64(&encoded);
        assert_eq!(decoded, original);
    }
}
