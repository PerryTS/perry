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
    array_subclass_fast_length, array_subclass_fast_pop, array_subclass_fast_push_one,
    is_array_subclass_class_id, js_packed_arraylike_index_get, js_packed_arraylike_loop_guard,
    js_packed_ecs_u32_loop_guard, raw_receiver_is_heap_object,
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
            (*obj).shape_word(),
            "#8113: ArrayHeader.capacity must alias the ObjectHeader shape word"
        );
        assert!(
            crate::object::shapes::is_layout_word((*obj).shape_word()),
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
        assert_eq!((*hdr).capacity, (*obj).shape_word());
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
        addr_class::FETCH_HANDLE_BAND_END,
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
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
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
    let stable_shape = unsafe { (*obj).shape_word() };
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
    assert_eq!(unsafe { (*obj).shape_word() }, stable_shape);
    assert!(!array_subclass_fast_index_set(receiver, 3, 55.0));
    assert_eq!(js_packed_arraylike_index_get(receiver, 2.0), 33.0);

    crate::object::js_object_delete_dynamic(obj, 1.0);
    assert_ne!(
        stable_dense_key,
        (u64::from(class_id) << 32) | unsafe { u64::from((*obj).shape_word()) },
        "a structural mutation must make the receiver-local proof miss by ShapeId"
    );
    assert_eq!(
        array_subclass_fast_index_get(receiver, 1),
        None,
        "deleting an indexed property must mint a shape whose dense proof side-exits"
    );
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 1.0).to_bits(),
        crate::value::TAG_UNDEFINED,
        "the wrapper must preserve the generic hole result"
    );
}

#[test]
fn dense_array_subclass_layout_follows_the_shape_across_a_prototype_override() {
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let class_id = 0x0074_865A;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    crate::object::js_object_set_index_polymorphic(obj as i64, 0.0, 11.0);

    assert_eq!(array_subclass_fast_index_get(receiver, 0), Some(11.0));
    let before = unsafe { (*obj).shape_word() };
    crate::object::prototype_chain::object_set_user_prototype(obj as usize, crate::value::TAG_NULL);
    // The prototype is a fact of the shape: the receiver moved to another
    // ShapeId, so no layout cached for the old one (owner word or
    // `(class, ShapeId)` key) can answer for it. What the new shape's layout
    // answers is the receiver's OWN element, which no prototype affects.
    assert_ne!(
        unsafe { (*obj).shape_word() },
        before,
        "a prototype change must move the receiver to another ShapeId"
    );
    assert_eq!(
        array_subclass_fast_index_get(receiver, 0),
        Some(11.0),
        "an own dense element reads the same on any prototype"
    );
}

/// A learned sequential numeric append is an exact reversible shape edge.
/// Pin both the direct helpers and the public native push/pop integration: a
/// generic delete would clone the keys array and mint a different predecessor
/// ShapeId, so exact identity makes this test non-vacuous.
#[test]
fn dense_array_subclass_tail_transitions_reuse_exact_shapes_and_slots() {
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8657;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    let mut shapes = vec![unsafe { (*obj).shape_word() }];
    for value in [11.0, 22.0, 33.0] {
        assert_eq!(
            js_array_push_f64(obj as *mut ArrayHeader, value),
            obj as *mut ArrayHeader
        );
        shapes.push(unsafe { (*obj).shape_word() });
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
    assert_eq!(unsafe { (*obj).shape_word() }, shapes[2]);
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
    assert_eq!(unsafe { (*obj).shape_word() }, shapes[3]);
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
    assert_eq!(unsafe { (*obj).shape_word() }, shapes[2]);
    assert_eq!(array_subclass_fast_length(receiver), Some(2.0));
}

/// A numeric loop publishes a proof-bearing sibling ShapeId. Tail transition
/// cache entries are keyed by the ordinary exact shape, so pop and push must
/// retire the proof before looking up their respective historical edges.
#[test]
fn numeric_proof_retires_before_exact_tail_transition_lookup() {
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8695;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    let mut ordinary_shapes = vec![unsafe { (*obj).shape_word() }];
    for value in [11.0, 22.0, 33.0] {
        js_array_push_f64(obj as *mut ArrayHeader, value);
        ordinary_shapes.push(unsafe { (*obj).shape_word() });
    }
    assert!(crate::object::array_tail_transition::lookup_reverse(ordinary_shapes[3]).is_some());

    let mut facts = [0u64; 7];
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver, 3.0, 1, facts.as_mut_ptr()),
        2
    );
    let proven_shape = unsafe { (*obj).shape_word() };
    assert!(
        crate::object::shapes::shape_object_kind_for_word(proven_shape)
            == Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof)
    );
    assert_ne!(proven_shape, ordinary_shapes[3]);
    assert!(crate::object::array_tail_transition::lookup_reverse(proven_shape).is_none());
    assert_eq!(array_subclass_fast_pop(receiver), Some(33.0));
    assert_eq!(unsafe { (*obj).shape_word() }, ordinary_shapes[2]);

    assert_eq!(
        js_packed_arraylike_loop_guard(receiver, 2.0, 1, facts.as_mut_ptr()),
        2
    );
    let proven_shape = unsafe { (*obj).shape_word() };
    assert!(
        crate::object::shapes::shape_object_kind_for_word(proven_shape)
            == Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof)
    );
    assert_ne!(proven_shape, ordinary_shapes[2]);
    assert_eq!(array_subclass_fast_push_one(receiver, 44.0), Some(3.0));
    assert_eq!(unsafe { (*obj).shape_word() }, ordinary_shapes[3]);
    assert_eq!(array_subclass_fast_index_get(receiver, 2), Some(44.0));
}

/// The metadata-free tail store is admitted only for constructive numeric
/// entity IDs. Tagged values must retain their exact bits and continue through
/// the ordinary barriered slot-store path.
#[test]
fn dense_array_subclass_numeric_tail_store_preserves_tagged_fallbacks() {
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
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

    // An exotic receiver (here: a typed-array view, which `push` must answer
    // through the observable `Set` — a non-writable `length` throws) can run
    // user code on the complete path. The fused entry is classified
    // allocate-but-never-reenter, so it must decline with null and leave the
    // receiver untouched; the generated caller then performs the complete push.
    let exotic =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 4);
    length = u32::MAX;
    let declined = js_array_push_u31_with_length(exotic as *mut ArrayHeader, 5, &mut length);
    assert!(
        declined.is_null(),
        "an exotic receiver must be declined to the caller's complete push"
    );
    assert_eq!(length, u32::MAX, "a declined push must not report a length");
    assert_eq!(
        unsafe { crate::buffer::store::raw_length(exotic as usize) },
        4,
        "a declined push must not mutate the receiver"
    );
}

/// `cache_carrier` follows live transition-cache occupancy: an inserted edge
/// marks both of its descriptors, and the post-full-trace recompute clears the
/// mark once no live entry names them (eviction / tombstone / test clear).
#[test]
fn transition_cache_carrier_bits_follow_live_occupancy_across_full_trace_recompute() {
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8695;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    let before = unsafe { (*obj).shape_word() };
    assert_eq!(
        js_array_push_f64(obj as *mut ArrayHeader, 11.0),
        obj as *mut ArrayHeader
    );
    // Pop warms the reverse edge; push it back to learn the forward edge.
    assert_eq!(array_subclass_fast_pop(receiver), Some(11.0));
    assert_eq!(
        js_array_push_f64(obj as *mut ArrayHeader, 11.0),
        obj as *mut ArrayHeader
    );
    let after = unsafe { (*obj).shape_word() };
    assert_ne!(before, after);
    let carrier = |id: u32| {
        crate::object::shapes::shape_descriptor_for_word(id)
            .expect("shape exists")
            .cache_carrier
    };
    assert!(
        carrier(before) && carrier(after),
        "descriptors named by a live transition entry must be cache carriers"
    );

    // With no live entry naming them, the recompute releases both.
    crate::object::array_tail_transition::test_clear();
    crate::object::array_tail_transition::recompute_cache_carriers_after_full_trace();
    assert!(
        !carrier(before) && !carrier(after),
        "a full-trace recompute must release descriptors no live entry names"
    );

    // The cleared cache has no reverse edge for the fast pop, so the generic
    // pop runs. Object model step 4: its delete of the last index key goes
    // through the receiver's overflow (no rollback edge, §3.2), so no tail
    // transition is learned again for this receiver; its elements stay right.
    assert_eq!(
        crate::array::js_array_pop_f64(obj as *mut ArrayHeader),
        11.0
    );
    assert!(unsafe { crate::object::overflow::has_overflow(obj) });
    assert_eq!(
        js_array_push_f64(obj as *mut ArrayHeader, 11.0),
        obj as *mut ArrayHeader
    );
    assert_eq!(array_subclass_fast_index_get(receiver, 0), None);
    assert_eq!(
        crate::array::js_array_get_f64(obj as *mut ArrayHeader, 0),
        11.0
    );
    assert_eq!(
        crate::array::js_array_pop_f64(obj as *mut ArrayHeader),
        11.0
    );
    assert_eq!(array_subclass_fast_length(receiver), None);
}

/// The spec push entry (the typed field-push lowering's complete fallback)
/// and the generic entry both append to an object-backed Array subclass
/// through the dense fast arm, off the header tag, without the tracked
/// resolver: the receiver pointer is returned unchanged and the element is
/// readable through the dense read.
#[test]
fn spec_and_generic_push_entries_append_to_an_object_backed_subclass_densely() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8696;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    assert!(!obj.is_null());
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    // Learn the tail edge once through the generic route.
    assert_eq!(
        js_array_push_f64(obj as *mut ArrayHeader, 1.0),
        obj as *mut ArrayHeader
    );
    assert_eq!(array_subclass_fast_pop(receiver), Some(1.0));
    // Reference: the fused u31 entry's dense arm. Whatever tracked-resolver
    // probes the arm itself needs, the spec and generic entries must need the
    // same number — none of their own before reaching it.
    let probes = crate::value::addr_class::tracked_header_probe_count_for_tests;
    let mut length = u32::MAX;
    let before = probes();
    assert_eq!(
        js_array_push_u31_with_length(obj as *mut ArrayHeader, 5, &mut length),
        obj as *mut ArrayHeader
    );
    let u31_probes = probes() - before;
    assert_eq!(array_subclass_fast_pop(receiver), Some(5.0));
    let before = probes();
    assert_eq!(
        crate::array::js_array_push_f64_spec(obj as *mut ArrayHeader, 7.0),
        obj as *mut ArrayHeader
    );
    let spec_probes = probes() - before;
    // Back to the learned edge (length 0 -> 1) before the generic entry.
    assert_eq!(array_subclass_fast_pop(receiver), Some(7.0));
    let before = probes();
    assert_eq!(
        js_array_push_f64(obj as *mut ArrayHeader, 9.0),
        obj as *mut ArrayHeader
    );
    let generic_probes = probes() - before;
    assert_eq!(
        (spec_probes, generic_probes),
        (u31_probes, u31_probes),
        "the spec and generic entries must reach the dense arm without tracked probes of their own"
    );
    assert_eq!(array_subclass_fast_length(receiver), Some(1.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 0), Some(9.0));
}

/// A direct-mapped transition cache can appear correct on short arrays yet
/// lose one historical edge to a hash collision. One miss changes the shape
/// lineage and makes every older edge unusable, which is catastrophic for ECS
/// archetypes that drain and refill a thousand entities per tick.
#[test]
fn dense_array_subclass_tail_cache_preserves_a_1024_shape_lattice() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let class_id = 0x0074_8693;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);

    let mut shapes = Vec::with_capacity(1025);
    shapes.push(unsafe { (*obj).shape_word() });
    for value in 0u32..1024 {
        js_array_push_f64(obj as *mut ArrayHeader, f64::from(value));
        shapes.push(unsafe { (*obj).shape_word() });
    }

    for length in (1u32..=1024).rev() {
        assert_eq!(
            array_subclass_fast_pop(receiver),
            Some(f64::from(length - 1)),
            "reverse edge at length {length} must survive unrelated hash collisions"
        );
        assert_eq!(unsafe { (*obj).shape_word() }, shapes[length as usize - 1]);
    }
    for value in 0u32..1024 {
        assert_eq!(
            array_subclass_fast_push_one(receiver, f64::from(value)),
            Some(f64::from(value + 1)),
            "forward edge at length {value} must remain reusable"
        );
        assert_eq!(unsafe { (*obj).shape_word() }, shapes[value as usize + 1]);
    }
}

#[test]
fn dense_array_subclass_tail_fast_path_declines_restricted_receivers() {
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
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

    let shape_before_descriptor = unsafe { (*obj).shape_word() };
    crate::object::descriptor_state::set_property_attrs(
        obj as usize,
        "0".to_string(),
        crate::object::descriptor_state::PropertyAttrs::new(true, true, false),
    );
    assert_ne!(unsafe { (*obj).shape_word() }, shape_before_descriptor);
    assert_eq!(
        array_subclass_fast_pop(receiver),
        None,
        "descriptor mutation must mint a ShapeId that cannot reuse the learned plain edge"
    );
}

#[test]
fn dense_array_subclass_tail_transition_edges_survive_moving_gc() {
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    crate::object::array_tail_transition::test_clear();

    let class_id = 0x0074_8694;
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
        (*live).shape_word()
    };
    assert!(crate::object::array_tail_transition::lookup_reverse(pre_gc_shape).is_some());
    let before = crate::gc::copying_minor_cycles();
    let _ = crate::gc::gc_collect_minor();
    assert!(crate::gc::copying_minor_cycles() > before);

    let live_receiver = receiver_h.get_nanbox_f64();
    let live_obj = (live_receiver.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    assert_eq!(array_subclass_fast_length(live_receiver), Some(3.0));
    assert_eq!(unsafe { (*live_obj).shape_word() }, pre_gc_shape);
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
    assert!(crate::object::shapes::is_layout_word(unsafe {
        (*live_obj).shape_word()
    }));
}

/// #8690: pointer-free tagged values skip the GC write barrier. The generic
/// successful-index hook must still retire a numeric-prefix proof, otherwise a
/// later loop clone would reinterpret the SSO bits as an f64 Number.
#[test]
fn packed_numeric_proof_is_retired_by_sso_index_overwrite() {
    let _global = crate::gc::global_side_table_test_lock();
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
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

    let initial_shape = unsafe {
        crate::object::shapes::object_shape_stamp(
            (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader,
        )
    };
    let mut facts = [0u64; 7];
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver_h.get_nanbox_f64(), 3.0, 1, facts.as_mut_ptr(),),
        2,
        "the numeric object-backed range should establish a proof"
    );
    let live_raw = (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut u8;
    let proof_shape =
        unsafe { crate::object::shapes::object_shape_stamp(live_raw as *const ObjectHeader) };
    assert_ne!(proof_shape, initial_shape);
    assert!(
        crate::object::shapes::shape_object_kind_for_word(proof_shape)
            == Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof)
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
    let retired_shape =
        unsafe { crate::object::shapes::object_shape_stamp(live_raw as *const ObjectHeader) };
    assert!(
        crate::object::shapes::shape_object_kind_for_word(retired_shape)
            != Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof)
    );
    assert_eq!(
        js_packed_arraylike_loop_guard(receiver_h.get_nanbox_f64(), 3.0, 1, facts.as_mut_ptr(),),
        0,
        "the next numeric loop must side-exit after an element-kind transition"
    );
}

#[test]
fn packed_numeric_proof_survives_pointer_free_index_swap() {
    let _global = crate::gc::global_side_table_test_lock();
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
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
    assert_eq!(
        crate::object::shapes::shape_object_kind_for_word(unsafe {
            crate::object::shapes::object_shape_stamp(obj)
        },),
        Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof)
    );

    assert!(array_subclass_fast_index_set(receiver, 1, 33.0));
    assert_eq!(array_subclass_fast_index_get(receiver, 1), Some(33.0));
    assert_eq!(
        crate::object::shapes::shape_object_kind_for_word(unsafe {
            crate::object::shapes::object_shape_stamp(obj)
        },),
        Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof),
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
    // Pins the shape-carried representation: the elements store is the
    // default, and this test is about the property-shape machinery.
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
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
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 65536);
    let right =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 65536);
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
    // Columns shorter than the admitted bound: the fused loop reads every
    // column up to that bound, so admission must be declined even though the
    // columns agree with each other.
    let tiny_a =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 2);
    let tiny_b =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT32 as i32, 2);
    assert_eq!(
        js_packed_ecs_u32_loop_guard(
            receiver_h.get_nanbox_f64(),
            3.0,
            crate::value::js_nanbox_pointer(tiny_a as i64),
            crate::value::js_nanbox_pointer(tiny_b as i64),
            0.0,
            0.0,
            2,
            facts.as_mut_ptr(),
        ),
        0,
        "a column shorter than the admitted bound must decline the fused loop"
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
    assert_eq!(js_packed_arraylike_index_get(receiver, 0.0), 17.0);
}

/// Retiring a packed-numeric proof is idempotent, and only the call that
/// ACTUALLY retires one reports `true`.
///
/// `array_subclass_fast_pop_validated` flushes the process-global store-plan
/// cache when this returns `true`. It used to flush unconditionally, so a pop
/// loop discarded every cached store plan in the program on every iteration
/// while retiring a proof only on the first.
#[test]
fn retiring_a_packed_numeric_proof_reports_only_the_call_that_did_it() {
    let _global = crate::gc::global_side_table_test_lock();
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let class_id = 0x0074_8694;
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
        js_packed_arraylike_loop_guard(receiver_h.get_nanbox_f64(), 3.0, 1, facts.as_mut_ptr()),
        2,
        "test premise: the numeric range establishes a proof to retire"
    );

    let live =
        || (receiver_h.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    let epoch_before = crate::object::prop_plan::prop_plan_semantic_epoch();
    assert!(
        unsafe { super::subclass::clear_packed_subclass_numeric_proof(live()) },
        "the first retire must report that it retired the proof"
    );
    let epoch_after_retire = crate::object::prop_plan::prop_plan_semantic_epoch();

    for _ in 0..4 {
        assert!(
            !unsafe { super::subclass::clear_packed_subclass_numeric_proof(live()) },
            "a receiver with no proof left must report that it retired nothing"
        );
    }
    assert_eq!(
        crate::object::prop_plan::prop_plan_semantic_epoch(),
        epoch_after_retire,
        "retiring nothing must not move the epoch — the whole point of the \
         conditional flush in array_subclass_fast_pop_validated"
    );
    let _ = epoch_before;
}
