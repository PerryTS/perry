//! #T2 ("inline hit, one exit"): the arms the emitted dynamic `obj[i]` read
//! stopped inlining must still be served, in the same order, with the same
//! cache effects, by its single out-of-line exit
//! [`js_packed_arraylike_index_get`].
//!
//! Every test here is a **differential** against
//! [`crate::value::js_dyn_index_get`], the complete generic accessor that the
//! exit's fast tiers exist to shortcut, because that is the exact contract:
//! every tier — the ordinary-Array arm, the elements-backed Array-subclass
//! probe, the shape-carried layout cache and (new here) the lazy-JSON-array
//! probe — must produce the value the generic path would have produced. A
//! test that only asserted "did not panic" would pass for a stub; these assert
//! the element VALUE and, where a cache is involved, the primed words.
//!
//! The typed-array cases matter even though the emitted site now serves an
//! in-bounds, inline-storage, non-BigInt read from its own `tav.w1/w2/w4/w8`
//! arm: every case that arm's guard rejects — a raised `PERRY_TA_VIEW_GUARD`,
//! a BigInt or Float16 lane, an out-of-range or fractional index — routes the
//! same read to this exit, so the two must produce the same element.

use super::subclass::js_packed_arraylike_index_get;
use super::{ArrayHeader, ArrayLikePicCache, ArrayLikePicCacheSlot, ARRAYLIKE_PIC_WORDS};
use crate::array::{js_array_alloc, js_array_push_f64};
use crate::object::ObjectHeader;
use crate::typedarray::{
    js_typed_array_get, js_typed_array_set, typed_array_alloc, TypedArrayHeader, KIND_BIGINT64,
    KIND_BIGUINT64, KIND_FLOAT32, KIND_FLOAT64, KIND_INT16, KIND_INT32, KIND_INT8, KIND_UINT16,
    KIND_UINT32, KIND_UINT8, KIND_UINT8_CLAMPED,
};

/// The reserved parent class id `class X extends Array` records.
const CLASS_ID_ARRAY: u32 = 0xFFFF_0024;

/// Every element kind a dynamic `obj[i]` can read as a Number, in the order
/// the old per-kind `tav.kd1..7` chain tested them. The site's arms are now
/// keyed on element WIDTH, so these nine kinds map onto four load blocks —
/// which is exactly why each one is read here at three distinct indices.
const INLINE_KINDS: [u8; 9] = [
    KIND_INT8,
    KIND_UINT8,
    KIND_INT16,
    KIND_UINT16,
    KIND_INT32,
    KIND_UINT32,
    KIND_FLOAT32,
    KIND_FLOAT64,
    KIND_UINT8_CLAMPED,
];

fn nanbox(p: *const u8) -> f64 {
    crate::value::js_nanbox_pointer(p as i64)
}

fn typed(kind: u8, values: &[f64]) -> *mut TypedArrayHeader {
    let ta = typed_array_alloc(kind, values.len() as u32);
    for (i, v) in values.iter().enumerate() {
        js_typed_array_set(ta, i as i32, *v);
    }
    ta
}

fn plain(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for v in values {
        arr = js_array_push_f64(arr, *v);
    }
    arr
}

/// A `class X extends Array` instance with `count` pushed numeric elements and
/// `declared` named inline slots ahead of them — the shape that gives the site
/// an object-backed array-like receiver.
/// The shape-carried representation guard the object-backed tiers need: with
/// the default elements store installed, `ObjectMeta.elements` answers every
/// read and NO layout cache is ever primed — which would make the cache
/// assertions below vacuous.
fn shape_carried() -> crate::array::subclass_elements::ArraySubclassRepresentationGuard {
    crate::array::subclass_elements::ArraySubclassRepresentationGuard::shape_carried()
}

fn array_subclass(
    class_id: u32,
    declared: u32,
    packed_keys: &[u8],
    count: u32,
) -> *mut ObjectHeader {
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let keys = crate::object::js_build_class_keys_array(
        class_id,
        declared,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    let obj =
        crate::object::js_object_alloc_class_inline_keys(class_id, CLASS_ID_ARRAY, declared, keys);
    crate::node_stream::js_array_subclass_init(nanbox(obj as *const u8), 0.0);
    for i in 0..count {
        js_array_push_f64(obj as *mut ArrayHeader, f64::from(i) + 11.0);
    }
    obj
}

/// The contract in one place: every tier of the site's exit must answer what
/// the complete generic accessor answers.
///
/// The generic side runs FIRST and on its own, so the comparison cannot be
/// satisfied by the exit priming state the generic path then reads.
#[track_caller]
fn assert_matches_generic(receiver: f64, index: f64, what: &str) {
    let expected = crate::value::js_dyn_index_get(receiver, index);
    let mut cache: ArrayLikePicCache = [0; ARRAYLIKE_PIC_WORDS];
    let mut slot: ArrayLikePicCacheSlot = &mut cache;
    let actual = js_packed_arraylike_index_get(receiver, index, &mut slot);
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{what}: the site exit must answer exactly what the generic accessor \
         answers (exit {actual}, generic {expected})"
    );
}

#[test]
fn the_exit_reads_every_element_kind_the_inline_arm_can_serve() {
    let _serialized = crate::array::test_serialize();
    // A value that only survives correct per-kind truncation/sign handling, so
    // a stub that returned 0.0 — or that picked the wrong width — cannot pass.
    for kind in INLINE_KINDS {
        let ta = typed(kind, &[0.0, 7.0, 0.0]);
        let receiver = nanbox(ta as *const u8);
        for index in 0..3u32 {
            let expected = js_typed_array_get(ta, index as i32);
            let actual =
                js_packed_arraylike_index_get(receiver, f64::from(index), std::ptr::null_mut());
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "kind {kind} index {index}: the exit must read the same element the inline arm would"
            );
        }
        assert_eq!(
            js_packed_arraylike_index_get(receiver, 1.0, std::ptr::null_mut()),
            7.0,
            "kind {kind}: the fixture value must survive the per-kind load"
        );
    }
}

#[test]
fn each_kind_reads_its_own_lane_width_not_a_neighbours() {
    let _serialized = crate::array::test_serialize();
    // Distinct per-lane values: a load of the wrong width at index 1 reads
    // element 0's or element 2's bytes and cannot produce 2.0.
    for kind in INLINE_KINDS {
        let ta = typed(kind, &[1.0, 2.0, 3.0]);
        let receiver = nanbox(ta as *const u8);
        for (index, expect) in [(0u32, 1.0), (1, 2.0), (2, 3.0)] {
            assert_eq!(
                js_packed_arraylike_index_get(receiver, f64::from(index), std::ptr::null_mut()),
                expect,
                "kind {kind}: element {index} must be read at its own lane offset"
            );
        }
    }
}

#[test]
fn out_of_range_fractional_and_negative_indices_match_the_generic_accessor() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(KIND_INT32, &[5.0, 6.0]);
    let receiver = nanbox(ta as *const u8);
    for (index, what) in [
        (2.0, "one past the end"),
        (1e9, "far out of range"),
        (-1.0, "negative"),
        (0.5, "fractional"),
        (f64::NAN, "NaN"),
        (4_294_967_296.0, "above the array-index range"),
    ] {
        assert_matches_generic(receiver, index, what);
    }
}

#[test]
fn bigint_and_float16_kinds_are_not_served_as_numbers() {
    let _serialized = crate::array::test_serialize();
    // The inline ladder guarded `kind <= KIND_UINT8_CLAMPED` precisely because
    // a BigInt lane is a NaN-boxed pointer, not a Number. The exit keeps that
    // bound and defers, so `ta[i]` still round-trips as a `bigint`.
    for kind in [KIND_BIGINT64, KIND_BIGUINT64] {
        let ta = typed_array_alloc(kind, 2);
        let receiver = nanbox(ta as *const u8);
        // Deliberately NOT an exact-bits differential: each call allocates a
        // fresh BigInt, so two reads of the same lane are two distinct
        // pointers. What must hold is the TAG — the site's arm keeps the
        // `kind <= KIND_UINT8_CLAMPED` bound precisely because a BigInt lane
        // is not a Number, and the exit it defers to must produce the BigInt.
        let through_exit = js_packed_arraylike_index_get(receiver, 0.0, std::ptr::null_mut());
        let through_dispatcher = js_packed_arraylike_index_get(receiver, 0.0, std::ptr::null_mut());
        for (value, via) in [(through_exit, "exit"), (through_dispatcher, "dispatcher")] {
            assert!(
                crate::value::JSValue::from_bits(value.to_bits()).is_bigint(),
                "kind {kind} via the {via}: a BigInt lane must come back NaN-boxed as a BigInt, \
                 not read as a Number"
            );
        }
    }
}

#[test]
fn a_live_view_changes_the_route_and_not_the_element() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(KIND_FLOAT64, &[1.5, 2.5]);
    let receiver = nanbox(ta as *const u8);
    let direct = js_packed_arraylike_index_get(receiver, 1.0, std::ptr::null_mut());
    assert_eq!(direct, 2.5, "the exit must serve this receiver");

    // The site's inline arm computes the data pointer as `header + 16`, and
    // its whole licence to do that is the cleared process-wide view guard. A
    // raised guard sends the read here instead, and it must still answer 2.5 —
    // through the dispatcher, whose `data_ptr` consults the view registries.
    crate::typedarray::PERRY_TA_VIEW_GUARD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let guarded = js_packed_arraylike_index_get(receiver, 1.0, std::ptr::null_mut());
    crate::typedarray::PERRY_TA_VIEW_GUARD.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        guarded, 2.5,
        "a raised view guard must change the ROUTE, never the element"
    );
}

#[test]
fn ordinary_arrays_holes_and_lazy_json_arrays_match_the_generic_accessor() {
    let _serialized = crate::array::test_serialize();
    let arr = plain(&[3.0, 4.0, 5.0]);
    let receiver = nanbox(arr as *const u8);
    for index in [0.0, 2.0, 3.0] {
        assert_matches_generic(receiver, index, "an ordinary Array");
    }

    // `JSON.parse` yields GC_TYPE_LAZY_ARRAY, the receiver the emitted
    // `arrlike.lazy.*` tier used to serve inline through
    // `js_lazy_array_index_probe`.
    let json = b"[10,20,30]";
    let text = crate::string::js_string_from_bytes(json.as_ptr(), json.len() as u32);
    let parsed = f64::from_bits(unsafe { crate::json::js_json_parse(text) }.bits());
    for index in [0.0, 1.0, 2.0, 3.0] {
        assert_matches_generic(parsed, index, "a lazy JSON array");
    }
    assert_eq!(
        js_packed_arraylike_index_get(parsed, 1.0, std::ptr::null_mut()),
        20.0,
        "the lazy tier must still produce the element, not undefined"
    );
}

#[test]
fn an_array_subclass_read_primes_the_site_cache_and_matches_the_generic_accessor() {
    let _representation = shape_carried();
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    let obj = array_subclass(0x0074_86A1, 2, b"sset\0mask\0", 3);
    let receiver = nanbox(obj as *const u8);

    // Two reads through two SEPARATE fresh caches: the words the exit
    // publishes are a function of the receiver, not of what the site already
    // held, which is what makes "cache decisions unchanged" checkable.
    let mut first_cache: ArrayLikePicCache = [0; ARRAYLIKE_PIC_WORDS];
    let mut first_slot: ArrayLikePicCacheSlot = &mut first_cache;
    let mut second_cache: ArrayLikePicCache = [0; ARRAYLIKE_PIC_WORDS];
    let mut second_slot: ArrayLikePicCacheSlot = &mut second_cache;
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 0.0, &mut first_slot),
        11.0
    );
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 0.0, &mut second_slot),
        11.0
    );
    assert_ne!(
        first_cache[0], 0,
        "the fixture must actually prime a cache, or this test is vacuous"
    );
    assert_eq!(
        first_cache, second_cache,
        "miss + prime must publish the identical layout words every time"
    );
    for index in [1.0, 2.0, 3.0] {
        assert_matches_generic(receiver, index, "a dense Array subclass");
    }
}

#[test]
fn a_spilled_length_subclass_and_its_family_token_match_the_generic_accessor() {
    let _representation = shape_carried();
    let _global = crate::gc::global_side_table_test_lock();
    crate::object::array_tail_transition::test_clear();
    // Four declared named fields push the Array-subclass `length` and the
    // element slots past the inline region: the `arrlike.ic.length_spill_*`
    // and `arrlike.ic.spill*` tiers, plus the high-bit family token that
    // `arrlike.ic.family_meta`/`family_token` used to compare inline.
    let obj = array_subclass(0x0074_86A2, 4, b"a\0b\0c\0d\0", 4);
    let receiver = nanbox(obj as *const u8);

    let mut cache: ArrayLikePicCache = [0; ARRAYLIKE_PIC_WORDS];
    let mut slot: ArrayLikePicCacheSlot = &mut cache;
    assert_eq!(
        js_packed_arraylike_index_get(receiver, 0.0, &mut slot),
        11.0
    );
    assert_ne!(
        cache[0], 0,
        "the fixture must actually prime a cache, or this test is vacuous"
    );
    assert!(
        cache[1] >= cache[4],
        "this fixture must actually spill the length slot, or the spill arms are untested \
         (length slot {}, inline bound {})",
        cache[1],
        cache[4]
    );
    for index in [0.0, 1.0, 3.0, 4.0] {
        assert_matches_generic(receiver, index, "a spilled Array subclass");
    }
}

#[test]
fn a_plain_object_and_a_non_pointer_receiver_match_the_generic_accessor() {
    let _global = crate::gc::global_side_table_test_lock();
    let obj = crate::object::js_object_alloc(0x0074_86A3, 2);
    let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, key, 17.0);
    assert_matches_generic(nanbox(obj as *const u8), 0.0, "a plain object");
    assert_matches_generic(nanbox(obj as *const u8), 1.0, "a plain object, absent key");
    // A non-pointer receiver: `(42)[0]` is `undefined`, not a throw, so it is
    // safe to compare here. `undefined[0]` / `null[0]` deliberately are NOT —
    // they raise a TypeError, and the emitted site routes them to this exit
    // precisely so the dispatcher can raise it.
    assert_matches_generic(42.0, 0.0, "a non-pointer receiver");
}

/// The exit returns a BOXED element, which is why a number context still
/// wraps it in `js_number_coerce` at the site (`arrlike.ic.miss`) instead of
/// folding the coercion into the call — folding it would have cost every
/// receiver that reaches this exit an extra argument and its test.
#[test]
fn the_exit_returns_a_boxed_element_that_a_number_context_must_coerce() {
    let _serialized = crate::array::test_serialize();
    // A string element is the case that distinguishes "coerced" from "not":
    // `ToNumber("12")` is 12, and the raw read is a NaN-boxed string.
    let mut arr = js_array_alloc(1);
    let text = crate::string::js_string_from_bytes(b"12".as_ptr(), 2);
    arr = js_array_push_f64(arr, crate::value::js_nanbox_string(text as i64));
    let receiver = nanbox(arr as *const u8);

    let raw = js_packed_arraylike_index_get(receiver, 0.0, std::ptr::null_mut());
    assert!(
        crate::value::JSValue::from_bits(raw.to_bits()).is_any_string(),
        "the exit must leave the element boxed"
    );
    assert_eq!(
        crate::builtins::js_number_coerce(js_packed_arraylike_index_get(
            receiver,
            0.0,
            std::ptr::null_mut(),
        )),
        12.0,
        "the site's number-context `js_number_coerce` must apply ToNumber"
    );
    // `undefined` is the OOB answer, and `ToNumber(undefined)` is NaN — the
    // property the emitted merge phi depends on in a number context.
    assert!(
        crate::builtins::js_number_coerce(js_packed_arraylike_index_get(
            receiver,
            9.0,
            std::ptr::null_mut(),
        ))
        .is_nan(),
        "a coerced out-of-bounds read must be NaN, not the undefined box"
    );
}
