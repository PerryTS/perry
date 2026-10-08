//! Buffer module - provides binary data handling similar to Node.js Buffer

use std::ptr;

use crate::array::ArrayHeader;
use crate::string::{
    js_string_alloc_ascii_uninit, js_string_from_ascii_bytes, js_string_from_bytes, StringHeader,
};

mod access;
mod backing;
#[cfg(test)]
mod byte_cell_admission_tests;
#[cfg(test)]
pub(crate) use backing::LIVE_BACKINGS;
pub mod bytes;
pub use backing::TransferredBacking;
pub(crate) use header::{buffer_adopt_backing, buffer_alloc_owned};
mod cmp;
mod coding;
mod copy_bytes;
mod copy_write;
mod dataview;
mod detach;
mod encode;
mod exotic_view;
/// #8149: `ArrayBuffer` / `SharedArrayBuffer` / `DataView` are registered
/// buffers with no integer-indexed own properties.
#[cfg(test)]
mod exotic_view_tests;
mod from;
pub(crate) mod header;
/// #10694: the brand is the cell's GC type byte.
#[cfg(test)]
mod header_brand_tests;
mod iter;
mod mutate;
mod numeric;
mod own_props;
pub(crate) mod pool;
mod query;
mod resizable;
/// #10873: resizable ArrayBuffer storage model + view relength.
#[cfg(test)]
mod resizable_tests;
pub(crate) mod store;
mod transcode;
mod u8_codec;
pub mod validate;
pub(crate) mod view;
#[cfg(test)]
mod view_tests;

// Canonical view-resolving span accessor. Every path that hands a raw data
// pointer to native code routes through this so a Uint8Array view over an
// ArrayBuffer exposes the same shared bytes to JavaScript and native code.
pub(crate) use view::resolve_data_ptr as resolve_span_data_ptr;

// ---- Re-exports: types & constants ----
pub(crate) use header::buffer_payload_size;
pub use header::{BufferHeader, BUFFER_TYPE_ID, NODE_BUFFER_CLASS_ID, SMALL_BUF_THRESHOLD};

// ---- Re-exports: allocation / registry helpers ----
pub(crate) use header::is_small_buf_slab_addr;
// #9342: primed by `typedarray::js_u8_buffer_read_f64` (codegen slow arm).
pub(crate) use access::{admitted_u8_read, admitted_u8_write, is_admitted_u8_cell};
// #10694: the brand is the cell's GC type byte; see `header`'s module note.
#[cfg(test)]
pub(crate) use header::buffer_alloc_foreign;
pub use header::{
    asymmetric_key_meta, buffer_alloc, buffer_backing_array_buffer, buffer_byte_offset,
    buffer_data, buffer_data_mut, crypto_key_meta, ensure_buffer_ab_alias,
    external_registries_hold_for_test, is_any_array_buffer, is_array_buffer, is_data_view,
    is_registered_buffer, is_secret_key, is_shared_array_buffer, is_uint8array_buffer,
    js_set_crypto_key_death_hook, mark_as_asymmetric_key, register_buffer, set_crypto_key_meta,
    CryptoKeyDeathHookFn,
};
pub(crate) use header::{buffer_family_type_owned, header_is_owned};
pub(crate) use header::{
    drop_owned_backing_at_thread_exit, finalize_collected_dead_buffer, is_foreign_backed_buffer,
};
// Only the wasm host re-points a foreign wrapper (#9611); see the fn's docs.
#[cfg(feature = "wasm-host")]
pub(crate) use header::rebind_foreign_buffer;

// ---- Re-exports: ArrayBuffer detach / transfer (ES2024) ----
// `detach_array_buffer` dereferences the raw address it is given, so it stays
// crate-internal; only the side-effect-free `is_detached_buffer` probe is
// part of the public surface.
pub use detach::is_detached_buffer;
pub(crate) use detach::{array_buffer_transfer, detach_array_buffer};
pub use own_props::{
    buffer_define_own_data_prop, buffer_delete_own_prop, buffer_get_own_prop, buffer_has_own_prop,
    buffer_own_prop_names, buffer_read_own_prop, buffer_set_own_prop,
};
// ---- Re-exports: resizable ArrayBuffer (#10873) ----
pub use header::resizable_max_byte_length;
pub(crate) use header::{resizable_info, set_resizable, ResizableInfo};
pub(crate) use resizable::array_buffer_resize;
pub use resizable::{
    is_out_of_bounds_data_view, is_resizable_buffer, js_array_buffer_new_with_options,
};

// ---- Re-exports: #8149 integer-indexed-exotic discrimination ----
// `ArrayBuffer` / `SharedArrayBuffer` / `DataView` share `BufferHeader` and the
// buffer registry with `Buffer` / `Uint8Array` but have NO integer-indexed own
// properties. See `exotic_view`.
pub use exotic_view::{
    buffer_brand, canonical_index_key, is_byte_indexed_buffer, is_node_buffer,
    is_non_indexed_buffer_view, is_uint8_view_buffer, BufferBrand,
};

// ---- Re-exports: Buffer.from / alloc / concat (FFI) ----
pub(crate) use from::buffer_string_bytes_for_encoding;
pub use from::{
    js_array_buffer_new, js_array_buffer_new_value, js_buffer_alloc, js_buffer_alloc_fill_value,
    js_buffer_alloc_unsafe, js_buffer_alloc_unsafe_slow, js_buffer_concat,
    js_buffer_concat_with_length, js_buffer_fill, js_buffer_fill_range, js_buffer_fill_value_range,
    js_buffer_from_array, js_buffer_from_arraybuffer_slice, js_buffer_from_string,
    js_buffer_from_value, js_data_view_new, js_encoding_tag_from_value, js_shared_array_buffer_new,
    js_shared_array_buffer_new_value, js_uint8array_alloc, js_uint8array_from_array,
    js_uint8array_new, js_uint8array_view,
};

// ---- Re-exports: predicates / byteLength (FFI) ----
pub use query::{
    js_buffer_byte_length, js_buffer_byte_length_value, js_buffer_is_ascii, js_buffer_is_buffer,
    js_buffer_is_encoding, js_buffer_is_node_buffer, js_buffer_is_utf8, js_native_buffer_byte_len,
    js_native_buffer_data_ptr, js_value_buffer_or_typedarray_data,
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
pub(crate) use access::buffer_slice_copy;
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
pub(crate) use iter::{dispatch_buffer_iterator_method_builtin, dispatch_buffer_iterator_step};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_array_factories_keep_their_final_brand() {
        let array = std::ptr::null();
        let bytes = js_uint8array_from_array(array);
        assert!(is_uint8array_buffer(bytes as usize));
        assert!(!is_node_buffer(bytes as usize));
        assert_eq!(js_buffer_length(bytes), 0);
        let buffer = js_buffer_from_array(array);
        assert!(is_node_buffer(buffer as usize));
        assert_eq!(js_buffer_length(buffer), 0);
    }

    /// The GC buffer sweep must drop the CryptoKey/secret-key side tables
    /// along with the buffer identity ones. They are plain `addr -> metadata`
    /// maps that never rooted the `BufferHeader`, so leaving them behind both
    /// leaked an entry per key and let a recycled address inherit CryptoKey
    /// identity (`crypto_key_meta`/`is_secret_key` gate `instanceof CryptoKey`,
    /// `util.types.isCryptoKey`, `KeyObject.from`, `.export()` …) — the #6080
    /// ABA class this finalizer exists to prevent.
    #[test]
    fn test_dead_buffer_finalize_prunes_crypto_key_side_tables() {
        let buf = store::alloc_test(crate::gc::GC_TYPE_BUFFER_CRYPTO_KEY, 32);
        assert!(!buf.is_null());
        let addr = buf as usize;

        // Shape a WebCrypto secret CryptoKey: HMAC / SHA-256 / secret. Its
        // brand is the cell's type byte (#10694); the metadata is the
        // attribute table this finalizer must drop.
        set_crypto_key_meta(addr, 1, 2, 1);

        assert!(crypto_key_meta(addr).is_some(), "meta registered");
        assert!(
            is_uint8array_buffer(addr),
            "a CryptoKey is Uint8Array-backed"
        );
        assert!(is_registered_buffer(addr), "buffer brand");

        // Exactly what the sweep's BufferSideTables finalize hook runs for a
        // dead cell.
        finalize_collected_dead_buffer(addr);

        // The cell itself is still allocated here (the test called the
        // finalizer directly), so its brand still reads CryptoKey; what must
        // be gone is the attribute entry a recycled address would inherit.
        assert!(
            !header::test_crypto_key_meta_registered(addr),
            "dead buffer must not keep CryptoKey metadata — a recycled address \
             would answer to instanceof CryptoKey / KeyObject.from()"
        );
    }

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
                unsafe { crate::buffer::store::capacity(buf as usize) },
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
            super::store::set_length(buf as usize, 3);
            bytes::no_gc(|scope| {
                bytes::bytes_mut(crate::value::js_nanbox_pointer(buf as i64), scope)
                    .unwrap()
                    .copy_from_slice(&[7, 8, 9])
            });
        }
        let buf_value = f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits());
        let iter_sym = crate::symbol::well_known_symbol("iterator");
        assert!(!iter_sym.is_null());
        let iter_sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());

        let method =
            unsafe { crate::symbol::js_object_get_symbol_property(buf_value, iter_sym_value) };
        assert_ne!(method.to_bits(), crate::value::TAG_UNDEFINED);

        let iterator = unsafe {
            crate::closure::js_native_call_value(
                method,
                crate::closure::plain_call_receiver(),
                std::ptr::null(),
                0,
            )
        };
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
            super::store::set_length(buf as usize, 1);
            js_buffer_set(buf, 0, 7);
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
            super::store::set_length(buf as usize, 4);
            bytes::no_gc(|scope| {
                bytes::bytes_mut(crate::value::js_nanbox_pointer(buf as i64), scope)
                    .unwrap()
                    .copy_from_slice(&[1, 2, 3, 4])
            });
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
        assert_eq!(
            unsafe { crate::buffer::store::capacity(buf as usize) },
            SMALL_BUF_THRESHOLD
        );
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
            super::store::set_length(buf as usize, cap);
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

    /// Bytes currently in an `ArrayBuffer`'s own storage — the shared truth a
    /// DataView and a typed array over it must both agree with.
    fn backing_bytes(ab: *const BufferHeader, len: usize) -> Vec<u8> {
        bytes::no_gc(|scope| {
            bytes::bytes(crate::value::js_nanbox_pointer(ab as i64), scope).unwrap()[..len].to_vec()
        })
    }

    /// A `DataView` and a MULTI-BYTE typed array over the same `ArrayBuffer`
    /// must observe each other's writes in both directions.
    ///
    /// A DataView owns a `BufferHeader` seeded from the backing at construction
    /// (`js_data_view_new`), and only writes routed through the view registry
    /// refresh that snapshot. A `Uint16Array`/`Uint32Array`/`Float64Array`
    /// element store goes straight into the backing store
    /// (`typedarray::data_ptr_mut`), so nothing refreshed the snapshot and every
    /// `get*` returned pre-write bytes — silently, with no throw. `Uint8Array`
    /// masked the bug: its element writes go through `js_buffer_set`, which
    /// mirrors into every registered view.
    #[test]
    fn data_view_reads_multi_byte_typed_array_writes() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        for (kind, elem_size) in [
            (crate::typedarray::KIND_UINT16, 2usize),
            (crate::typedarray::KIND_UINT32, 4),
            (crate::typedarray::KIND_FLOAT64, 8),
        ] {
            let ab = js_array_buffer_new(elem_size as i32);
            let ab_value = f64::from_bits(crate::value::JSValue::pointer(ab as *const u8).bits());
            let ta =
                crate::typedarray_view::js_typed_array_view(kind as i32, ab_value, undef, undef);
            assert!(!ta.is_null(), "kind={kind}: typed-array view");
            // Constructed BEFORE the write, so its snapshot is all zeroes and a
            // stale read is unambiguous.
            let dv = js_data_view_new(ab_value, undef, undef);

            crate::typedarray::js_typed_array_set(ta, 0, 258.0);

            // Subject-liveness: without a store that actually reaches the
            // ArrayBuffer, every byte comparison below would pass vacuously
            // (0 == 0).
            let backing = backing_bytes(ab, elem_size);
            assert_ne!(
                backing,
                vec![0u8; elem_size],
                "kind={kind}: typed-array store never reached the ArrayBuffer, \
                 so this test proves nothing"
            );

            for i in 0..elem_size {
                assert_eq!(
                    js_data_view_get(dv, i as f64, DataViewKind::Uint8, false),
                    backing[i] as f64,
                    "kind={kind}: DataView byte {i} lags the typed-array write"
                );
            }
        }
    }

    /// The other direction, and the windowed (`new DataView(ab, 4, 8)`) shape:
    /// a DataView write must land in the backing at `byteOffset + offset` where
    /// the typed array reads it.
    #[test]
    fn multi_byte_typed_array_reads_windowed_data_view_writes() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let ab = js_array_buffer_new(16);
        let ab_value = f64::from_bits(crate::value::JSValue::pointer(ab as *const u8).bits());
        let ta = crate::typedarray_view::js_typed_array_view(
            crate::typedarray::KIND_UINT32 as i32,
            ab_value,
            undef,
            undef,
        );
        assert!(!ta.is_null());
        // Window covering elements 1 and 2 of the Uint32Array.
        let dv = js_data_view_new(ab_value, 4.0, 8.0);

        // DataView -> typed array. Big-endian (the DataView default) so the
        // byte order is fixed regardless of host endianness.
        js_data_view_set(dv, 0.0, 0x0102_0304u32 as f64, DataViewKind::Uint32, false);
        assert_eq!(
            backing_bytes(ab, 16)[4..8],
            [0x01, 0x02, 0x03, 0x04],
            "windowed DataView write must land at byteOffset 4 of the backing"
        );
        assert_eq!(
            crate::typedarray::js_typed_array_get(ta, 1),
            u32::from_ne_bytes([0x01, 0x02, 0x03, 0x04]) as f64,
            "typed array must read the DataView's bytes"
        );

        // typed array -> windowed DataView, at the window's far end.
        crate::typedarray::js_typed_array_set(ta, 2, 0xDEAD_BEEFu32 as f64);
        let backing = backing_bytes(ab, 16);
        for i in 0..8usize {
            assert_eq!(
                js_data_view_get(dv, i as f64, DataViewKind::Uint8, false),
                backing[4 + i] as f64,
                "windowed DataView byte {i} must mirror backing byte {}",
                4 + i
            );
        }
        // Bytes outside the window stay untouched and unreachable.
        assert_eq!(&backing[0..4], &[0u8; 4], "element 0 must be untouched");
        assert_eq!(&backing[12..16], &[0u8; 4], "element 3 must be untouched");
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

#[cfg(feature = "node-api-host")]
pub(crate) use header::{enqueue_all_foreign_finalizers, set_foreign_finalizer};

#[cfg(test)]
mod adopted_backing_tests;
