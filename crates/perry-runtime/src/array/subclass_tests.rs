//! #7574 — `class X extends Array` in a `T[]`-annotated binding took the raw
//! `ArrayHeader` fast paths.
//!
//! Every test here is **sabotage-shaped**: it first asserts that the bytes the
//! pre-fix code misread are *still sitting there* — an `ObjectHeader` read as
//! an `ArrayHeader` yields a `(length, capacity)` pair that sails through
//! `clean_arr_ptr`'s length/capacity sanity check — and only then that the
//! entry point refuses or resolves it. A green run therefore proves the brand
//! check FIRED, not that the receiver happened to look invalid for some
//! unrelated reason.
//!
//! #8113 MOVED the overlay. `ObjectHeader::object_type` is gone, so
//! `ArrayHeader.length` now aliases `class_id` and `capacity` aliases the shape
//! word. That makes the class ids used here load-bearing: the pre-fix sanity
//! check is `length <= capacity && length <= 100M`, and `length` is the class
//! id, so every fixture below uses an id under 100,000,000. A larger id would
//! fail that check for an unrelated reason and silently turn these tests
//! vacuous — which is exactly the failure mode the module is written to avoid.

use super::subclass::{
    array_object_receiver, array_subclass_fast_index_get, array_subclass_fast_length,
    is_array_subclass_class_id, js_packed_arraylike_index_get, js_packed_arraylike_loop_guard,
    raw_receiver_is_heap_object,
};
use crate::array::{clean_arr_ptr, js_array_alloc, ArrayHeader};
use crate::object::{js_object_alloc, ObjectHeader};

/// The reserved parent class id `class X extends Array` records.
const CLASS_ID_ARRAY: u32 = 0xFFFF_0024;

fn as_array_header(obj: *mut ObjectHeader) -> *const ArrayHeader {
    obj as *const ArrayHeader
}

/// The overlay that makes this bug possible, pinned. If `ObjectHeader` ever
/// stops starting with `class_id: u32, parent_class_id: u32`, the misread this
/// whole family defends against changes shape and these tests must be revisited.
#[test]
fn object_header_still_overlays_array_header_length_and_capacity() {
    let class_id = 0x0074_0001;
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let hdr = as_array_header(obj);
    unsafe {
        assert_eq!(
            (*hdr).length,
            class_id,
            "#8113: ArrayHeader.length must alias ObjectHeader.class_id"
        );
        assert_eq!(
            (*hdr).capacity,
            (*obj).parent_class_id,
            "#8113: ArrayHeader.capacity must alias the ObjectHeader shape word"
        );
        assert!(
            crate::object::shapes::is_shape_id((*obj).parent_class_id),
            "test premise: a birth-stamped object carries a ShapeId in word 1, \
             which is what keeps the forged capacity above the forged length"
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
    let obj = js_object_alloc(0x0074_0002, 2);
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
    let class_id = 0x0074_0003;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    assert!(
        is_array_subclass_class_id(class_id),
        "the class chain must reach the reserved Array parent id"
    );
    let obj = js_object_alloc(class_id, 2);
    let hdr = as_array_header(obj);
    unsafe {
        // Sabotage precondition: the misread is still available (#8113 overlay).
        assert_eq!((*hdr).length, class_id);
        assert_eq!((*hdr).capacity, (*obj).parent_class_id);
        assert!((*hdr).length <= (*hdr).capacity);
        assert!((*hdr).length <= 100_000_000);
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
    let class_id = 0x0074_0004;
    crate::object::js_register_class_parent(class_id, 0x0074_0005);
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

/// #8655: the object-backed representation still stores dense Array-subclass
/// elements in ordinary property slots. Pin the shape proof and, importantly,
/// its side exit after a structural mutation.
#[test]
fn dense_array_subclass_reads_slots_until_its_shape_changes() {
    let class_id = 0x0074_8655;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    for (index, value) in [11.0, 22.0, 33.0].into_iter().enumerate() {
        crate::object::js_object_set_index_polymorphic(obj as i64, index as f64, value);
    }

    assert_eq!(array_subclass_fast_length(receiver), Some(3.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 1), Some(22.0));
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 2.0, std::ptr::null_mut()),
        33.0
    );

    crate::object::js_object_delete_dynamic(obj, 1.0);
    assert_eq!(
        array_subclass_fast_index_get(receiver, 1),
        None,
        "deleting an indexed property must mint a shape whose dense proof side-exits"
    );
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 1.0, std::ptr::null_mut()).to_bits(),
        crate::value::TAG_UNDEFINED,
        "the wrapper must preserve the generic hole result"
    );
}

/// #8690: pointer-free tagged values skip the GC write barrier. The generic
/// successful-index hook must still retire a numeric-prefix proof, otherwise a
/// later loop clone would reinterpret the SSO bits as an f64 Number.
#[test]
fn packed_numeric_proof_is_retired_by_sso_index_overwrite() {
    let class_id = 0x0074_8690;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(receiver);
    crate::node_stream::js_array_subclass_init(receiver_h.get_nanbox_f64(), 0.0);
    for (index, value) in [11.0, 22.0, 33.0].into_iter().enumerate() {
        let live_raw = receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF;
        crate::object::js_object_set_index_polymorphic(live_raw as i64, index as f64, value);
    }

    let mut facts = [0u64; 7];
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver_h.get_nanbox_f64(), 3.0, 1, facts.as_mut_ptr(),),
        2,
        "the numeric object-backed range should establish a proof"
    );
    let live_raw = (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut u8;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(live_raw as usize) }
        .expect("the rooted receiver is a live GC object");
    assert_ne!(
        header._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF,
        0
    );

    let key_ptr = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
    let key = f64::from_bits(crate::value::js_nanbox_string(key_ptr as i64).to_bits());
    let sso = f64::from_bits(
        crate::value::JSValue::try_short_string(b"9")
            .expect("one byte is an inline SSO")
            .bits(),
    );
    crate::proxy::js_put_value_set(
        receiver_h.get_nanbox_f64(),
        key,
        sso,
        receiver_h.get_nanbox_f64(),
        0,
    );

    let live_raw = (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut u8;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(live_raw as usize) }
        .expect("the rooted receiver is a live GC object");
    assert_eq!(
        header._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF,
        0,
        "a successful SSO overwrite must retire numeric authority without a GC barrier"
    );
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver_h.get_nanbox_f64(), 3.0, 1, facts.as_mut_ptr(),),
        0,
        "the next numeric loop must side-exit after an element-kind transition"
    );
}

#[test]
fn dense_array_subclass_guard_rejects_other_object_brands() {
    let obj = js_object_alloc(0x0074_8656, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, key, 17.0);

    assert_eq!(array_subclass_fast_length(receiver), None);
    assert_eq!(array_subclass_fast_index_get(receiver, 0), None);
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 0.0, std::ptr::null_mut()),
        17.0
    );
}
