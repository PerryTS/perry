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
    array_object_receiver, array_subclass_fast_index_get, array_subclass_fast_index_set,
    array_subclass_fast_length, array_subclass_fast_length_with_ic, array_subclass_fast_pop,
    array_subclass_fast_push_one, array_subclass_named_prefix_token_for_slot,
    array_subclass_named_prefix_token_matches_class, is_array_subclass_class_id,
    js_packed_arraylike_index_get, js_packed_arraylike_loop_guard, js_packed_ecs_u32_loop_guard,
    raw_receiver_is_heap_object,
};
use crate::array::{
    clean_arr_ptr, js_array_alloc, js_array_pop_f64, js_array_push_f64,
    js_array_push_u31_with_length, ArrayHeader,
};
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
    let stable_shape = unsafe { (*obj).parent_class_id };
    let stable_dense_key = unsafe { (*(*obj).meta).array_subclass_dense_key };
    assert_eq!(
        stable_dense_key,
        (u64::from(class_id) << 32) | u64::from(stable_shape),
        "the first proved dense read must publish the receiver-local layout"
    );
    assert!(array_subclass_fast_index_set(receiver, 1, 44.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 1), Some(44.0));
    assert_eq!(crate::array::js_array_length(obj as *const ArrayHeader), 3);
    assert_eq!(
        crate::array::js_array_get_f64(obj as *const ArrayHeader, 1),
        44.0
    );
    assert_eq!(
        crate::array::js_array_set_f64_extend(obj as *mut ArrayHeader, 1, 55.0),
        obj as *mut ArrayHeader
    );
    assert_eq!(
        crate::array::js_array_get_f64(obj as *const ArrayHeader, 1),
        55.0
    );
    assert_eq!(
        super::indexing::try_strict_dense_index_set(obj as *mut ArrayHeader, 1, 66.0,),
        Some(obj as *mut ArrayHeader)
    );
    assert_eq!(
        crate::array::js_array_get_f64(obj as *const ArrayHeader, 1),
        66.0
    );
    assert_eq!(unsafe { (*obj).parent_class_id }, stable_shape);
    assert!(!array_subclass_fast_index_set(receiver, 3, 55.0));
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 2.0, std::ptr::null_mut()),
        33.0
    );

    crate::object::js_object_delete_dynamic(obj, 1.0);
    assert_ne!(
        stable_dense_key,
        (u64::from(class_id) << 32) | unsafe { u64::from((*obj).parent_class_id) },
        "a structural mutation must make the receiver-local proof miss by ShapeId"
    );
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

#[test]
fn dense_array_subclass_cache_declines_a_per_instance_prototype_override() {
    let class_id = 0x0074_865A;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    crate::object::js_object_set_index_polymorphic(obj as i64, 0.0, 11.0);

    assert_eq!(array_subclass_fast_index_get(receiver, 0), Some(11.0));
    crate::object::prototype_chain::object_set_static_prototype(
        obj as usize,
        crate::value::TAG_NULL,
    );
    assert_eq!(
        array_subclass_fast_index_get(receiver, 0),
        None,
        "a receiver-local dense-layout record must not survive prototype divergence"
    );
}

/// A learned sequential numeric append is an exact reversible shape edge.
/// Pin both the direct helpers and the public native push/pop integration: a
/// generic delete would clone the keys array and mint a different predecessor
/// ShapeId, so exact identity makes this test non-vacuous.
#[test]
fn dense_array_subclass_tail_transitions_reuse_exact_shapes_and_slots() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8657;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    let mut shapes = vec![unsafe { (*obj).parent_class_id }];
    for value in [11.0, 22.0, 33.0] {
        assert_eq!(
            js_array_push_f64(obj as *mut ArrayHeader, value),
            obj as *mut ArrayHeader
        );
        shapes.push(unsafe { (*obj).parent_class_id });
    }
    assert_eq!(array_subclass_fast_length(receiver), Some(3.0));
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_dense_key },
        (u64::from(class_id) << 32) | u64::from(shapes[3]),
        "the warm dense lookup must bind its scalar layout to the current shape"
    );
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_dense_bounds as u32 },
        3
    );
    assert!(crate::object::array_tail_transition::lookup_reverse(shapes[3]).is_some());
    assert_ne!(
        unsafe { (*(*obj).meta).array_tail_object_hot },
        0,
        "learning a spill-backed tail must bind this receiver to its agent-local transition tables"
    );

    assert_eq!(array_subclass_fast_pop(receiver), Some(33.0));
    assert_eq!(unsafe { (*obj).parent_class_id }, shapes[2]);
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_dense_key },
        (u64::from(class_id) << 32) | u64::from(shapes[2]),
        "pop must publish the exact predecessor layout without a cache rebuild"
    );
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_dense_bounds as u32 },
        2
    );
    assert_eq!(array_subclass_fast_length(receiver), Some(2.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 2), None);

    assert_eq!(array_subclass_fast_push_one(receiver, 44.0), Some(3.0));
    assert_eq!(unsafe { (*obj).parent_class_id }, shapes[3]);
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_dense_key },
        (u64::from(class_id) << 32) | u64::from(shapes[3]),
        "push must publish the exact successor layout without a cache rebuild"
    );
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_dense_bounds as u32 },
        3
    );
    assert_eq!(array_subclass_fast_index_get(receiver, 2), Some(44.0));

    assert_eq!(js_array_pop_f64(obj as *mut ArrayHeader), 44.0);
    assert_eq!(unsafe { (*obj).parent_class_id }, shapes[2]);
    assert_eq!(array_subclass_fast_length(receiver), Some(2.0));
}

#[test]
fn array_subclass_length_ic_publishes_only_scalar_exact_or_family_facts() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_867b;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let packed = b"sset\0mask\0";
    let keys =
        crate::object::js_build_class_keys_array(class_id, 2, packed.as_ptr(), packed.len() as u32);
    let obj = crate::object::js_object_alloc_class_inline_keys(class_id, CLASS_ID_ARRAY, 2, keys);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    let mut cache = [0_u64; 3];
    assert_eq!(
        array_subclass_fast_length_with_ic(receiver, cache.as_mut_ptr()),
        Some(0.0)
    );
    if crate::object::object_spill_enabled() {
        assert_ne!(cache[0] & (1_u64 << 63), 0);
        assert_eq!(cache[0] & u64::from(u32::MAX), u64::from(class_id));
        assert_eq!(
            cache[0],
            unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
            "an empty subclass can publish the same pointer-free family proof"
        );
    } else {
        assert_eq!(
            cache[0],
            (u64::from(class_id) << 32) | unsafe { u64::from((*obj).parent_class_id) }
        );
    }
    assert!(
        cache[1] >= cache[2],
        "this wolf-shaped receiver deliberately stores length in ObjectMeta::spill"
    );

    js_array_push_f64(obj as *mut ArrayHeader, 11.0);
    assert_eq!(
        array_subclass_fast_length_with_ic(receiver, cache.as_mut_ptr()),
        Some(1.0)
    );
    if crate::object::object_spill_enabled() {
        assert_ne!(cache[0] & (1_u64 << 63), 0);
        assert_eq!(cache[0] & u64::from(u32::MAX), u64::from(class_id));
        assert_eq!(
            cache[0],
            unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
            "the cache stores the receiver-owned scalar proof, not a heap address"
        );
    }
}

/// The metadata-free tail store is admitted only for constructive numeric
/// entity IDs. Tagged values must retain their exact bits and continue through
/// the ordinary barriered slot-store path.
#[test]
fn dense_array_subclass_numeric_tail_store_preserves_tagged_fallbacks() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_865c;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    // Learn a forward/reverse edge, then revisit it with each value kind.
    js_array_push_f64(obj as *mut ArrayHeader, 11.0);
    assert_eq!(array_subclass_fast_pop(receiver), Some(11.0));

    let tagged_i32 = f64::from_bits(crate::value::JSValue::int32(123).bits());
    assert_eq!(
        array_subclass_fast_push_one(receiver, tagged_i32),
        Some(1.0)
    );
    assert_eq!(
        array_subclass_fast_index_get(receiver, 0),
        Some(123.0),
        "a genuine INT32 Number may be canonicalized to its raw-f64 form"
    );
    assert_eq!(array_subclass_fast_pop(receiver), Some(123.0));

    let sso = f64::from_bits(
        crate::value::JSValue::try_short_string(b"ecs")
            .expect("three bytes fit the inline-string representation")
            .bits(),
    );
    assert_eq!(array_subclass_fast_push_one(receiver, sso), Some(1.0));
    assert_eq!(
        array_subclass_fast_index_get(receiver, 0).map(f64::to_bits),
        Some(sso.to_bits()),
        "an inline string must not be reinterpreted as a Number"
    );
    assert_eq!(
        array_subclass_fast_pop(receiver).map(f64::to_bits),
        Some(sso.to_bits())
    );

    // Class references deliberately share INT32_TAG with small integers. The
    // class registry is the disambiguating guard used by value_bits_to_number.
    unsafe { crate::object::js_register_class_id(class_id) };
    let class_ref = f64::from_bits(crate::value::INT32_TAG | u64::from(class_id));
    assert_eq!(array_subclass_fast_push_one(receiver, class_ref), Some(1.0));
    assert_eq!(
        array_subclass_fast_index_get(receiver, 0).map(f64::to_bits),
        Some(class_ref.to_bits()),
        "a ClassRef must keep its tag for downstream property dispatch"
    );
    assert_eq!(
        array_subclass_fast_pop(receiver).map(f64::to_bits),
        Some(class_ref.to_bits()),
        "the numeric pop specialization must keep ClassRefs on its barriered fallback"
    );
}

#[test]
fn fused_u31_push_reports_length_for_plain_and_subclass_arrays() {
    let mut length = u32::MAX;
    let plain = js_array_alloc(1);
    let plain = js_array_push_u31_with_length(plain, 7, &mut length);
    assert_eq!(length, 1);
    assert_eq!(crate::array::js_array_get_f64(plain, 0), 7.0);
    let plain = js_array_push_u31_with_length(plain, 9, &mut length);
    assert_eq!(
        length, 2,
        "the fused result must follow a reallocating grow"
    );
    assert_eq!(crate::array::js_array_get_f64(plain, 1), 9.0);

    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_865d;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    js_array_push_f64(obj as *mut ArrayHeader, 11.0);
    assert_eq!(array_subclass_fast_pop(receiver), Some(11.0));

    let returned = js_array_push_u31_with_length(obj as *mut ArrayHeader, 42, &mut length);
    assert_eq!(returned, obj as *mut ArrayHeader);
    assert_eq!(length, 1);
    assert_eq!(array_subclass_fast_index_get(receiver, 0), Some(42.0));
}

#[test]
fn array_subclass_named_prefix_token_survives_only_exact_numeric_tail_transitions() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_865b;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let packed = b"sset\0mask\0";
    let keys =
        crate::object::js_build_class_keys_array(class_id, 2, packed.as_ptr(), packed.len() as u32);
    let obj = crate::object::js_object_alloc_class_inline_keys(class_id, CLASS_ID_ARRAY, 2, keys);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    crate::object::descriptor_state::set_property_attrs(
        obj as usize,
        "fill".to_string(),
        crate::object::descriptor_state::PropertyAttrs::new(true, false, true),
    );

    // Learn both edges and force numeric storage beyond the two declared
    // inline slots, which gives this object the ObjectMeta that carries the
    // move-stable family proof.
    js_array_push_f64(obj as *mut ArrayHeader, 11.0);
    js_array_push_f64(obj as *mut ArrayHeader, 22.0);
    let mask_key = crate::string::js_string_from_bytes(b"mask".as_ptr(), 4);
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    crate::object::js_object_get_field_ic_miss(obj, mask_key, &mut cache);
    let token = cache[2] as u64;
    assert_ne!(token, 0);
    assert!(unsafe { array_subclass_named_prefix_token_matches_class(obj, class_id) });
    assert!(
        !unsafe { array_subclass_named_prefix_token_matches_class(obj, class_id.wrapping_add(1)) },
        "the token must pin the exact class rather than merely the Array-subclass family"
    );
    assert_eq!(
        unsafe { array_subclass_named_prefix_token_for_slot(obj, 1) },
        token,
        "the IC miss must publish the same owner-side token it caches"
    );

    let mut index_ic = [0u64; 5];
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 0.0, index_ic.as_mut_ptr()),
        11.0
    );
    assert_eq!(
        index_ic[0], token,
        "a dense numeric read must publish the tail-family token instead of an exact ShapeId"
    );
    assert!(
        index_ic[1] >= index_ic[4],
        "this fixture keeps Array-subclass length in ObjectMeta::spill"
    );
    assert_eq!(
        index_ic[3], 2,
        "the cached prefix is a safe high-water mark"
    );

    let zero_key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    let mut element_cache = [0i64; crate::object::PIC_CACHE_WORDS];
    assert_eq!(
        crate::object::js_object_get_field_ic_miss(obj, zero_key, &mut element_cache),
        11.0
    );
    assert_eq!(
        element_cache[2], 0,
        "a numeric-element site must never borrow the stable named-prefix identity"
    );
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
        token,
        "declining a numeric site must not retire the independently valid named-prefix proof"
    );

    assert_eq!(array_subclass_fast_pop(receiver), Some(22.0));
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
        token,
        "the exact reverse numeric-tail edge must preserve the named-prefix proof"
    );
    assert_eq!(array_subclass_fast_push_one(receiver, 33.0), Some(2.0));
    assert_eq!(
        unsafe { array_subclass_named_prefix_token_for_slot(obj, 1) },
        token,
        "the exact forward edge must retain the same class-wide token"
    );

    let extra = crate::string::js_string_from_bytes(b"extra".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(obj, extra, 7.0);
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
        0,
        "generic named structural mutation must retire the family proof before publication"
    );
    assert!(
        !unsafe { array_subclass_named_prefix_token_matches_class(obj, class_id) },
        "a retired token must not remain consumable as an ordinary-object proof"
    );
    assert_eq!(
        unsafe { array_subclass_named_prefix_token_for_slot(obj, 1) },
        0,
        "an instance-specific named suffix must not borrow the class token again"
    );
}

/// Wolf's `_ent` / `_updateTo` arrays store `Archetype` instances while each
/// Archetype's dense numeric tail changes on every entity migration. The
/// enclosing plain array's element-class proof must be able to consume the
/// subclass's stable named-prefix token; consulting the new ShapeId on every
/// overwrite defeats the transition cache that made the tail mutation cheap.
#[test]
fn plain_array_element_shape_consumes_array_subclass_prefix_proof() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8667;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let packed = b"sset\0mask\0change\0";
    let keys =
        crate::object::js_build_class_keys_array(class_id, 3, packed.as_ptr(), packed.len() as u32);
    let obj = crate::object::js_object_alloc_class_inline_keys(class_id, CLASS_ID_ARRAY, 3, keys);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    crate::object::descriptor_state::set_property_attrs(
        obj as usize,
        "fill".to_string(),
        crate::object::descriptor_state::PropertyAttrs::new(true, false, true),
    );

    // Learn the 1 -> 2 edge before publishing the token. The first generic
    // edge is allowed to rebuild shape metadata; the warm exact edge below is
    // the one whose preservation contract the ECS path relies on.
    js_array_push_f64(obj as *mut ArrayHeader, 11.0);
    js_array_push_f64(obj as *mut ArrayHeader, 22.0);
    assert_eq!(array_subclass_fast_pop(receiver), Some(22.0));
    assert_ne!(
        unsafe { array_subclass_named_prefix_token_for_slot(obj, 1) },
        0,
        "precondition: the subclass must carry the ordinary-prefix proof"
    );

    let mut owners = js_array_alloc(1);
    owners = js_array_push_f64(owners, receiver);
    // Proofs are demand-driven: a consumer requests the enclosing array's
    // class proof, then the keep/clear paths maintain it across stores.
    assert_eq!(
        crate::array::js_array_ensure_element_shape(owners),
        class_id as i32,
        "the enclosing plain array should prove a class invariant on request"
    );

    let original_shape = unsafe { (*obj).parent_class_id };
    assert_eq!(array_subclass_fast_push_one(receiver, 33.0), Some(2.0));
    assert_ne!(
        unsafe { (*obj).parent_class_id },
        original_shape,
        "the numeric tail transition must actually change the exact ShapeId"
    );
    let hits_before = crate::array::element_shape::test_array_subclass_prefix_store_hits();
    assert_eq!(
        crate::array::indexing::try_strict_dense_index_set(owners, 0, receiver),
        Some(owners)
    );
    assert!(
        crate::array::element_shape::test_array_subclass_prefix_store_hits() > hits_before,
        "the overwrite should use the stable subclass proof rather than classify the changing ShapeId"
    );
    assert_eq!(
        crate::array::js_array_element_shape_class(owners),
        class_id as i32,
        "consuming the subclass proof must retain the enclosing class invariant"
    );
}

/// Entity migration removes the last id from its source Archetype before
/// `_archChange` reads `arch.change` and `arch.mask`. That receiver is an empty,
/// descriptor-bearing Array subclass: the ordinary exact-shape PIC is closed,
/// so the class-prefix token must be available before a `"0"` key exists.
#[test]
fn empty_array_subclass_named_prefix_token_survives_warm_tail_cycle() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8659;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let packed = b"sset\0mask\0change\0";
    let keys =
        crate::object::js_build_class_keys_array(class_id, 3, packed.as_ptr(), packed.len() as u32);
    let obj = crate::object::js_object_alloc_class_inline_keys(class_id, CLASS_ID_ARRAY, 3, keys);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    crate::object::descriptor_state::set_property_attrs(
        obj as usize,
        "fill".to_string(),
        crate::object::descriptor_state::PropertyAttrs::new(true, false, true),
    );

    // Learn the 0 -> 1 edge once, then return to the exact empty predecessor.
    // Subsequent cycles use the allocation-free transition cache.
    js_array_push_f64(obj as *mut ArrayHeader, 11.0);
    assert_eq!(array_subclass_fast_pop(receiver), Some(11.0));

    let change_key = crate::string::js_string_from_bytes(b"change".as_ptr(), 6);
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    let via_ic = crate::object::js_object_get_field_ic_miss(obj, change_key, &mut cache);
    let via_ladder = crate::object::js_object_get_field_by_name_f64(obj, change_key);
    assert_eq!(via_ic.to_bits(), via_ladder.to_bits());
    let token = cache[2] as u64;
    assert_ne!(
        token, 0,
        "an empty subclass must arm the declared-prefix PIC"
    );
    assert_eq!(
        unsafe { array_subclass_named_prefix_token_for_slot(obj, 2) },
        token
    );

    assert_eq!(array_subclass_fast_push_one(receiver, 22.0), Some(1.0));
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
        token,
        "the warm forward edge must retain the empty-prefix proof"
    );
    assert_eq!(array_subclass_fast_pop(receiver), Some(22.0));
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
        token,
        "the warm reverse edge must retain the empty-prefix proof"
    );

    crate::object::set_accessor_descriptor(
        obj as usize,
        "change".to_string(),
        crate::object::AccessorDescriptor { get: 1, set: 0 },
    );
    assert_eq!(
        unsafe { (*(*obj).meta).array_subclass_named_prefix_token },
        0,
        "an accessor mutation must retire the direct-load proof"
    );
    assert_eq!(
        unsafe { array_subclass_named_prefix_token_for_slot(obj, 2) },
        0,
        "an accessor-backed declared key must not re-arm the family token"
    );
}

/// A direct-mapped transition cache can appear correct on short arrays yet
/// lose one historical edge to a hash collision. One miss changes the shape
/// lineage and makes every older edge unusable, which is catastrophic for ECS
/// archetypes that drain and refill a thousand entities per tick.
#[test]
fn dense_array_subclass_tail_cache_preserves_a_1024_shape_lattice() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_865a;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    let mut shapes = Vec::with_capacity(1025);
    shapes.push(unsafe { (*obj).parent_class_id });
    for value in 0u32..1024 {
        js_array_push_f64(obj as *mut ArrayHeader, f64::from(value));
        shapes.push(unsafe { (*obj).parent_class_id });
    }

    for length in (1u32..=1024).rev() {
        assert_eq!(
            array_subclass_fast_pop(receiver),
            Some(f64::from(length - 1)),
            "reverse edge at length {length} must survive unrelated hash collisions"
        );
        assert_eq!(
            unsafe { (*obj).parent_class_id },
            shapes[length as usize - 1]
        );
    }
    for value in 0u32..1024 {
        assert_eq!(
            array_subclass_fast_push_one(receiver, f64::from(value)),
            Some(f64::from(value + 1)),
            "forward edge at length {value} must remain reusable"
        );
        assert_eq!(
            unsafe { (*obj).parent_class_id },
            shapes[value as usize + 1]
        );
    }
}

#[test]
fn dense_array_subclass_tail_fast_path_declines_restricted_receivers() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8658;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    js_array_push_f64(obj as *mut ArrayHeader, 11.0);

    let header = unsafe {
        (obj as *mut u8)
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>()
    };
    unsafe { (*header)._reserved |= crate::gc::OBJ_FLAG_SEALED };
    assert_eq!(array_subclass_fast_pop(receiver), None);
    assert_eq!(array_subclass_fast_push_one(receiver, 22.0), None);
    assert_eq!(array_subclass_fast_length(receiver), Some(1.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 0), Some(11.0));
    unsafe { (*header)._reserved &= !crate::gc::OBJ_FLAG_SEALED };

    let shape_before_descriptor = unsafe { (*obj).parent_class_id };
    crate::object::descriptor_state::set_property_attrs(
        obj as usize,
        "0".to_string(),
        crate::object::descriptor_state::PropertyAttrs::new(true, true, false),
    );
    assert_ne!(unsafe { (*obj).parent_class_id }, shape_before_descriptor);
    assert_eq!(
        array_subclass_fast_pop(receiver),
        None,
        "descriptor mutation must mint a ShapeId that cannot reuse the learned plain edge"
    );
}

#[test]
fn dense_array_subclass_tail_transition_edges_survive_moving_gc() {
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    crate::object::array_tail_transition::test_clear();

    let class_id = 0x0074_8659;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    crate::node_stream::js_array_subclass_init(receiver_h.get_nanbox_f64(), 0.0);
    for value in [11.0, 22.0, 33.0] {
        let live =
            (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
        js_array_push_f64(live, value);
    }
    let pre_gc_shape = unsafe {
        let live =
            (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
        (*live).parent_class_id
    };
    assert!(crate::object::array_tail_transition::lookup_reverse(pre_gc_shape).is_some());
    let before = crate::gc::copying_minor_cycles();
    let _ = crate::gc::gc_collect_minor();
    assert!(crate::gc::copying_minor_cycles() > before);

    let live_receiver = receiver_h.get_nanbox_f64();
    let live_obj = (live_receiver.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    assert_eq!(array_subclass_fast_length(live_receiver), Some(3.0));
    assert_eq!(unsafe { (*live_obj).parent_class_id }, pre_gc_shape);
    assert_eq!(
        unsafe { (*(*live_obj).meta).array_subclass_dense_key },
        (u64::from(class_id) << 32) | u64::from(pre_gc_shape),
        "the receiver-local scalar layout must survive owner/meta evacuation"
    );
    assert!(
        crate::object::array_tail_transition::lookup_reverse(pre_gc_shape).is_some(),
        "moving GC must repair both rooted key-array edges in the reverse cache"
    );
    assert_eq!(array_subclass_fast_pop(live_receiver), Some(33.0));
    assert_eq!(array_subclass_fast_push_one(live_receiver, 44.0), Some(3.0));
    assert_eq!(array_subclass_fast_index_get(live_receiver, 2), Some(44.0));
    assert!(crate::object::shapes::is_shape_id(unsafe {
        (*live_obj).parent_class_id
    }));
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
fn packed_numeric_proof_survives_pointer_free_index_swap() {
    let class_id = 0x0074_8692;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    for (index, value) in [11.0, 22.0, 33.0].into_iter().enumerate() {
        crate::object::js_object_set_index_polymorphic(obj as i64, index as f64, value);
    }

    let mut facts = [0u64; 7];
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver, 3.0, 1, facts.as_mut_ptr()),
        2
    );
    let header = unsafe { crate::value::addr_class::try_read_gc_header(obj as usize) }
        .expect("the subclass receiver is live");
    assert_ne!(
        header._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF,
        0
    );

    assert!(array_subclass_fast_index_set(receiver, 1, 33.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 1), Some(33.0));
    let header = unsafe { crate::value::addr_class::try_read_gc_header(obj as usize) }
        .expect("the subclass receiver remains live");
    assert_ne!(
        header._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF,
        0,
        "a numeric-for-numeric overwrite must preserve the exact packed-u32 proof"
    );
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver, 3.0, 1, facts.as_mut_ptr()),
        2,
        "the next packed loop must consume the still-valid proof"
    );
}

#[test]
fn fused_ecs_guard_requires_distinct_owning_u32_columns_and_exact_entity_ids() {
    let class_id = 0x0074_8691;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(receiver);
    crate::node_stream::js_array_subclass_init(receiver_h.get_nanbox_f64(), 0.0);
    for (index, value) in [3.0, 12.0, 7.0].into_iter().enumerate() {
        let live_raw = receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF;
        crate::object::js_object_set_index_polymorphic(live_raw as i64, index as f64, value);
    }

    let left =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 16);
    let right =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 16);
    let wrong_kind =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_INT32 as i32, 16);
    let short =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 8);
    let left_value = crate::value::js_nanbox_pointer(left as i64);
    let right_value = crate::value::js_nanbox_pointer(right as i64);
    let wrong_value = crate::value::js_nanbox_pointer(wrong_kind as i64);
    let short_value = crate::value::js_nanbox_pointer(short as i64);
    let mut facts = [0u64; 11];

    assert_ne!(
        js_packed_ecs_u32_loop_guard(
            receiver_h.get_nanbox_f64(),
            3.0,
            left_value,
            right_value,
            0.0,
            0.0,
            2,
            facts.as_mut_ptr(),
        ),
        0
    );
    assert_eq!(facts[7], left as u64);
    assert_eq!(facts[8], right as u64);
    assert_eq!(
        js_packed_ecs_u32_loop_guard(
            receiver_h.get_nanbox_f64(),
            3.0,
            left_value,
            left_value,
            0.0,
            0.0,
            2,
            facts.as_mut_ptr(),
        ),
        0,
        "aliased component columns must retain generic assignment semantics"
    );
    assert_eq!(
        js_packed_ecs_u32_loop_guard(
            receiver_h.get_nanbox_f64(),
            3.0,
            left_value,
            wrong_value,
            0.0,
            0.0,
            2,
            facts.as_mut_ptr(),
        ),
        0,
        "non-Uint32 component columns must not borrow the direct clone"
    );
    assert_eq!(
        js_packed_ecs_u32_loop_guard(
            receiver_h.get_nanbox_f64(),
            3.0,
            left_value,
            short_value,
            0.0,
            0.0,
            2,
            facts.as_mut_ptr(),
        ),
        0,
        "unequal column lengths need per-column out-of-bounds semantics"
    );

    let live_raw = receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF;
    crate::object::js_object_set_index_polymorphic(live_raw as i64, 1.0, 12.5);
    assert_eq!(
        js_packed_ecs_u32_loop_guard(
            receiver_h.get_nanbox_f64(),
            3.0,
            left_value,
            right_value,
            0.0,
            0.0,
            2,
            facts.as_mut_ptr(),
        ),
        0,
        "a fractional entity id must revoke the exact-u32 source proof"
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
