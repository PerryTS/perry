//! Buffer module - provides binary data handling similar to Node.js Buffer

use std::alloc::Layout;
use std::ptr;

use crate::array::ArrayHeader;
use crate::string::{
    js_string_alloc_ascii_uninit, js_string_from_ascii_bytes, js_string_from_bytes, StringHeader,
};

mod access;
mod cmp;
mod coding;
mod copy_bytes;
mod copy_write;
mod dataview;
mod encode;
mod from;
mod header;
mod iter;
mod mutate;
mod numeric;
mod query;
mod transcode;
mod u8_codec;
pub mod validate;
pub(crate) mod view;

// ---- Re-exports: types & constants ----
pub use header::{BufferHeader, BUFFER_TYPE_ID, SMALL_BUF_THRESHOLD};

// ---- Re-exports: allocation / registry helpers ----
pub(crate) use header::is_small_buf_slab_addr;
pub use header::{
    asymmetric_key_meta, buffer_ab_alias, buffer_alloc, buffer_backing_array_buffer,
    buffer_byte_offset, buffer_data, buffer_data_mut, crypto_key_meta, ensure_buffer_ab_alias,
    is_any_array_buffer, is_array_buffer, is_data_view, is_registered_buffer, is_secret_key,
    is_shared_array_buffer, is_uint8array_buffer, mark_as_array_buffer, mark_as_asymmetric_key,
    mark_as_crypto_key, mark_as_data_view, mark_as_secret_key, mark_as_shared_array_buffer,
    mark_as_uint8array, register_buffer, resolve_buffer_ab_alias, set_buffer_ab_alias,
};
pub(crate) use header::{
    collect_dead_registered_buffers_post_trace, finalize_collected_dead_buffer,
};

// ---- Re-exports: Buffer.from / alloc / concat (FFI) ----
pub use from::{
    js_array_buffer_new, js_array_buffer_new_value, js_buffer_alloc, js_buffer_alloc_fill_value,
    js_buffer_alloc_unsafe, js_buffer_concat, js_buffer_concat_with_length, js_buffer_fill,
    js_buffer_fill_range, js_buffer_fill_value_range, js_buffer_from_array,
    js_buffer_from_arraybuffer_slice, js_buffer_from_string, js_buffer_from_value,
    js_data_view_new, js_encoding_tag_from_value, js_shared_array_buffer_new,
    js_shared_array_buffer_new_value, js_uint8array_alloc, js_uint8array_from_array,
    js_uint8array_new, js_uint8array_view,
};

// ---- Re-exports: predicates / byteLength (FFI) ----
pub use query::{
    js_buffer_byte_length, js_buffer_byte_length_value, js_buffer_is_ascii, js_buffer_is_buffer,
    js_buffer_is_encoding, js_buffer_is_utf8, js_native_buffer_byte_len, js_native_buffer_data_ptr,
    js_value_buffer_or_typedarray_data,
};

// ---- Re-exports: toString / print / length / to-array ----
pub(crate) use encode::buf_bytes_to_utf8_string;
pub use encode::{
    buffer_to_array, js_buffer_length, js_buffer_print, js_buffer_to_string,
    js_buffer_to_string_range, js_value_to_string_with_encoding,
};

// ---- Re-exports: TC39 Uint8Array base64/hex codecs (#2901) ----
pub use u8_codec::{
    js_u8_from_base64, js_u8_from_hex, js_u8_set_from_base64, js_u8_set_from_hex, js_u8_to_base64,
    js_u8_to_hex,
};

// ---- Re-exports: indexed access / slice / Uint8Array.set ----
pub use access::{
    js_buffer_get, js_buffer_index_get_value, js_buffer_set, js_buffer_set_from,
    js_buffer_set_from_value, js_buffer_slice,
};

// ---- Re-exports: DataView numeric accessors (#2878) ----
pub use dataview::{js_data_view_get, js_data_view_set, DataViewKind};

// ---- Re-exports: copy / write ----
pub use copy_bytes::js_buffer_copy_bytes_from;
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

// ---- Re-exports: Node argument validation (FFI, #2013) ----
pub use validate::{js_buffer_validate_concat_list, js_buffer_validate_size};

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

    // #5226 successor (2026-07-09 audit): every buffer — including the
    // formerly slab-allocated small tier — now carries a REAL GcHeader with
    // `GC_TYPE_BUFFER`, so the runtime's `*(ptr - GC_HEADER_SIZE)` type
    // probes read a genuine header (matching no other GC_TYPE) instead of
    // the old zeroed off-heap sentinel. The classification property the
    // sentinel protected must keep holding.
    #[test]
    fn small_buffer_reserves_zeroed_header_sentinel() {
        for cap in [0u32, 1, 3, 16, 255] {
            let buf = buffer_alloc(cap);
            assert!(is_registered_buffer(buf as usize), "cap={cap}");
            unsafe {
                let header =
                    (buf as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                assert_eq!(
                    (*header).obj_type,
                    crate::gc::GC_TYPE_BUFFER,
                    "cap={cap}: buffers must carry a real GC_TYPE_BUFFER header"
                );
                assert_ne!(
                    (*header).gc_flags & crate::gc::GC_FLAG_TENURED,
                    0,
                    "cap={cap}: buffers are born tenured in the old arena"
                );
            }
            // The header-probing classifiers must answer "not my type"
            // without faulting.
            let v = crate::value::js_nanbox_pointer(buf as i64);
            assert_eq!(crate::promise::js_value_is_promise(v), 0, "cap={cap}");
            assert!(!crate::date::is_date_cell_addr(buf as usize), "cap={cap}");
        }
    }

    #[test]
    fn test_buffer_symbol_iterator_uses_values_iterator() {
        let buf = buffer_alloc(3);
        unsafe {
            (*buf).length = 3;
            std::ptr::copy_nonoverlapping([7u8, 8, 9].as_ptr(), buffer_data_mut(buf), 3);
        }
        let buf_value = f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits());
        let iter_sym = crate::symbol::well_known_symbol("iterator");
        assert!(!iter_sym.is_null());
        let iter_sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());

        let method =
            unsafe { crate::symbol::js_object_get_symbol_property(buf_value, iter_sym_value) };
        assert_ne!(method.to_bits(), crate::value::TAG_UNDEFINED);

        let iterator = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
        let result = unsafe {
            crate::object::js_native_call_method(
                iterator,
                b"next".as_ptr() as *const i8,
                b"next".len(),
                std::ptr::null(),
                0,
            )
        };
        let result_obj =
            crate::value::js_nanbox_get_pointer(result) as *const crate::object::ObjectHeader;
        assert!(!result_obj.is_null());
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(result_obj, value_key),
            7.0
        );
    }

    #[test]
    fn test_buffer_symbol_iterator_respects_own_symbol_property() {
        let buf = buffer_alloc(1);
        unsafe {
            (*buf).length = 1;
            *buffer_data_mut(buf) = 7;
        }
        let buf_value = f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits());
        let iter_sym = crate::symbol::well_known_symbol("iterator");
        assert!(!iter_sym.is_null());
        let iter_sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());

        unsafe {
            crate::symbol::js_object_set_symbol_property(buf_value, iter_sym_value, 123.0);
        }

        let method =
            unsafe { crate::symbol::js_object_get_symbol_property(buf_value, iter_sym_value) };
        assert_eq!(method, 123.0);
    }

    #[test]
    fn test_array_from_small_buffer_materializes_bytes() {
        let buf = buffer_alloc(4);
        unsafe {
            (*buf).length = 4;
            std::ptr::copy_nonoverlapping([1u8, 2, 3, 4].as_ptr(), buffer_data_mut(buf), 4);
        }

        let arr = crate::array::js_array_clone(buf as *const crate::array::ArrayHeader);
        assert_eq!(crate::array::js_array_length(arr), 4);
        for (i, expected) in [1.0, 2.0, 3.0, 4.0].iter().copied().enumerate() {
            assert_eq!(crate::array::js_array_get_f64(arr, i as u32), expected);
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

    /// #6088: the JS-value accessor reads `undefined` for an out-of-range
    /// canonical index (IntegerIndexedExotic `[[Get]]`), unlike the native
    /// `js_buffer_get` which returns the `0` byte-sentinel. In-range reads
    /// still return the byte as a plain (non-NaN) f64 number.
    #[test]
    fn test_buffer_index_get_value_oob_is_undefined() {
        let buf = js_buffer_alloc(3, 0);
        js_buffer_set(buf, 0, 5);
        js_buffer_set(buf, 1, 6);
        js_buffer_set(buf, 2, 7);
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);

        // In-range: the byte value as a number (not undefined).
        assert_eq!(js_buffer_index_get_value(buf, 0), 5.0);
        assert_eq!(js_buffer_index_get_value(buf, 2), 7.0);

        // Out-of-range and negative: undefined, NOT the 0 sentinel.
        assert_eq!(js_buffer_index_get_value(buf, 3).to_bits(), undef.to_bits());
        assert_eq!(js_buffer_index_get_value(buf, 9).to_bits(), undef.to_bits());
        assert_eq!(
            js_buffer_index_get_value(buf, -1).to_bits(),
            undef.to_bits()
        );
        // The native accessor keeps its 0-for-OOB contract for its callers.
        assert_eq!(js_buffer_get(buf, 9), 0);

        // Null receiver: undefined.
        assert_eq!(
            js_buffer_index_get_value(std::ptr::null(), 0).to_bits(),
            undef.to_bits()
        );
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

    /// #1767: `Buffer.from(shortString)` must handle inline SSO strings.
    /// A string of length 0..=5 lives in the NaN-box payload (tag 0x7FF9),
    /// not behind a heap `StringHeader`. `js_buffer_from_value` only checked
    /// the strict `is_string()` (STRING_TAG 0x7FFF) predicate, so an SSO
    /// value fell through to the pointer/array path and its inline bytes
    /// (e.g. the ASCII of a 5-char `apiKey` like "mango") were dereferenced
    /// as an `ArrayHeader*` — SIGSEGV. Reached from `@perryts/mysql`'s
    /// prepared-statement param encoder (`Buffer.from(v, 'utf8')`).
    #[test]
    fn buffer_from_value_decodes_sso_short_string_utf8() {
        for s in ["", "a", "id", "p1", "mango"] {
            let v = crate::JSValue::try_short_string(s.as_bytes())
                .expect("len<=5 encodes as inline SSO");
            assert!(v.is_short_string(), "{s:?} should be an inline SSO value");
            let buf = js_buffer_from_value(v.bits() as i64, 0 /* utf8 */);
            assert!(!buf.is_null(), "null buffer for {s:?}");
            assert_eq!(
                js_buffer_length(buf) as usize,
                s.len(),
                "length mismatch for {s:?}"
            );
            for (i, &b) in s.as_bytes().iter().enumerate() {
                assert_eq!(
                    js_buffer_get(buf, i as i32) as u8,
                    b,
                    "byte {i} mismatch for {s:?}"
                );
            }
        }
    }

    /// Same SSO value, but decoded under the `hex` encoding tag (1): the
    /// short string holds hex digits and must produce the decoded bytes,
    /// proving the SSO branch routes through the shared encoding helper
    /// rather than a utf8-only fast path.
    #[test]
    fn buffer_from_value_decodes_sso_short_string_hex() {
        // "ff00" is 4 bytes (<= 5) → SSO; hex-decodes to [0xff, 0x00].
        let v = crate::JSValue::try_short_string(b"ff00").expect("SSO");
        let buf = js_buffer_from_value(v.bits() as i64, 1 /* hex */);
        assert!(!buf.is_null());
        assert_eq!(js_buffer_length(buf), 2);
        assert_eq!(js_buffer_get(buf, 0) as u8, 0xff);
        assert_eq!(js_buffer_get(buf, 1) as u8, 0x00);
    }
}
