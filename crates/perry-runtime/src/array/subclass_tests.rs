//! #7574 — `class X extends Array` in a `T[]`-annotated binding took the raw
//! `ArrayHeader` fast paths.
//!
//! Every test here is **sabotage-shaped**: it first asserts that the bytes the
//! pre-fix code misread are *still sitting there* — an `ObjectHeader` read as
//! an `ArrayHeader` yields `length == object_type == 1` and
//! `capacity == class_id`, both of which sail through `clean_arr_ptr`'s
//! length/capacity sanity check — and only then that the entry point refuses
//! or resolves it. A green run therefore proves the brand check FIRED, not that
//! the receiver happened to look invalid for some unrelated reason.

use super::subclass::{
    array_object_receiver, is_array_subclass_class_id, raw_receiver_is_heap_object,
};
use crate::array::{clean_arr_ptr, js_array_alloc, ArrayHeader};
use crate::object::{js_object_alloc, ObjectHeader};

/// The reserved parent class id `class X extends Array` records.
const CLASS_ID_ARRAY: u32 = 0xFFFF_0024;

fn as_array_header(obj: *mut ObjectHeader) -> *const ArrayHeader {
    obj as *const ArrayHeader
}

/// The overlay that makes this bug possible, pinned. If `ObjectHeader` ever
/// stops starting with `object_type: u32, class_id: u32`, the misread this
/// whole family defends against changes shape and these tests must be revisited.
#[test]
fn object_header_still_overlays_array_header_length_and_capacity() {
    let class_id = 0x7574_0001;
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let hdr = as_array_header(obj);
    unsafe {
        assert_eq!(
            (*hdr).length,
            1,
            "ArrayHeader.length must still alias ObjectHeader.object_type (= 1)"
        );
        assert_eq!(
            (*hdr).capacity,
            class_id,
            "ArrayHeader.capacity must still alias ObjectHeader.class_id"
        );
        // The sanity check `clean_arr_ptr` applied BEFORE the fix: `length <=
        // capacity && length <= 100M`. Both hold, which is precisely why the
        // forged header was waved through and `push` stored over `keys_array`.
        assert!((*hdr).length <= (*hdr).capacity);
        assert!((*hdr).length <= 100_000_000);
    }
}

#[test]
fn clean_arr_ptr_refuses_a_plain_object_receiver() {
    let obj = js_object_alloc(0x7574_0002, 2);
    let hdr = as_array_header(obj);
    unsafe {
        // Sabotage precondition: the forged (length, capacity) pair is still
        // acceptable to the pre-fix sanity check.
        assert!((*hdr).length <= (*hdr).capacity);
    }
    assert!(
        clean_arr_ptr(hdr).is_null(),
        "an ObjectHeader must not resolve to an ArrayHeader"
    );
}

#[test]
fn a_genuine_array_takes_the_fast_path_and_is_never_redirected() {
    let arr = js_array_alloc(4);
    assert!(!arr.is_null());
    assert_eq!(
        clean_arr_ptr(arr as *const ArrayHeader),
        arr as *const ArrayHeader,
        "a real ArrayHeader must pass clean_arr_ptr unchanged"
    );
    // The #7573 lesson: prove the fast path is not merely agreeing with a
    // redirect that happened to return the same thing.
    assert!(
        !raw_receiver_is_heap_object(arr as *const ArrayHeader),
        "the one-load brand pre-filter must answer false for GC_TYPE_ARRAY"
    );
    assert!(
        array_object_receiver(arr as *const ArrayHeader).is_none(),
        "a real ArrayHeader must never resolve to an array-like OBJECT receiver"
    );
}

#[test]
fn array_object_receiver_admits_an_array_subclass_instance() {
    let class_id = 0x7574_0003;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    assert!(
        is_array_subclass_class_id(class_id),
        "the class chain must reach the reserved Array parent id"
    );
    let obj = js_object_alloc(class_id, 2);
    let hdr = as_array_header(obj);
    unsafe {
        // Sabotage precondition: the misread is still available.
        assert_eq!((*hdr).length, 1);
        assert_eq!((*hdr).capacity, class_id);
    }
    assert!(
        raw_receiver_is_heap_object(hdr),
        "the pre-filter must admit a GC_TYPE_OBJECT allocation"
    );
    let recv = array_object_receiver(hdr).expect("subclass instance must resolve to a receiver");
    assert_eq!(
        (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize,
        obj as usize,
        "the resolved receiver must be the INSTANCE, not a copy"
    );
    // And `clean_arr_ptr` still refuses it, so every entry point that does not
    // resolve explicitly degrades instead of dereferencing the forged header.
    assert!(clean_arr_ptr(hdr).is_null());
}

#[test]
fn array_object_receiver_rejects_an_ordinary_class_instance() {
    let class_id = 0x7574_0004;
    crate::object::js_register_class_parent(class_id, 0x7574_0005);
    assert!(!is_array_subclass_class_id(class_id));
    let obj = js_object_alloc(class_id, 2);
    assert!(
        array_object_receiver(as_array_header(obj)).is_none(),
        "a non-Array class instance must keep its ordinary dispatch"
    );
}

#[test]
fn array_object_receiver_is_safe_for_non_pointers_and_handle_band_ids() {
    // Handle-band registry ids (fetch/zlib/proxy) carry no GcHeader; reading
    // `id - 8` would fault. They must classify as "not an object receiver".
    // Addresses are derived from the `addr_class` band map rather than
    // re-typed as literals (the addr-class ratchet's contract).
    use crate::value::addr_class;
    for id in [
        0usize,
        1,
        addr_class::COMMON_HANDLE_BAND_END,
        addr_class::FETCH_HANDLE_BAND_START,
        addr_class::ZLIB_HANDLE_BAND_START,
        addr_class::PROXY_ID_BAND_START,
        addr_class::HANDLE_BAND_MAX - 1,
    ] {
        let hdr = id as *const ArrayHeader;
        assert!(!raw_receiver_is_heap_object(hdr), "id {id:#x}");
        assert!(array_object_receiver(hdr).is_none(), "id {id:#x}");
    }
}
