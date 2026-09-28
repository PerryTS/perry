//! Charter step 5 (P2a): the runtime store check and the generalization of a
//! shape's `F64` lane (`field_rep_store`). Nothing in the runtime produces an
//! `F64` shape yet, so these tests mint one directly through the one intern
//! that takes a rep (`shape_descriptor_ensure_with_rep`) and stamp it on a
//! live object whose lanes hold Numbers.

use super::field_rep::{slot_rep, with_slot_rep, REP_ANY, REP_F64, REP_F64_DEPRECATED};
use super::field_rep_store::{migrate_deprecated_receiver, normalized_shape};
use super::shapes::{
    object_shape_descriptor, object_shape_stamp, publish_shape_result, shape_descriptor_by_id,
    shape_descriptor_ensure_with_rep, stamp_object_shape_id_with_carrier_note,
};
use super::ObjectHeader;

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// A live object `{a: 1, b: 2, c: 3}` and its (all-`Any`) shape.
unsafe fn abc() -> (*mut ObjectHeader, u32) {
    let obj = crate::object::js_object_alloc(0, 4);
    for (i, name) in ["a", "b", "c"].iter().enumerate() {
        crate::object::js_object_set_field_by_name(obj, key(name), (i + 1) as f64);
    }
    let id = object_shape_stamp(obj);
    assert_ne!(id, 0, "the fixture object is shaped");
    (obj, id)
}

/// The shape with `id`'s facts and field representation `rep`.
fn with_rep(id: u32, rep: u64) -> u32 {
    let d = shape_descriptor_by_id(id).expect("live shape");
    publish_shape_result(shape_descriptor_ensure_with_rep(
        d.keys as usize as *const crate::array::ArrayHeader,
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        d.object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        rep,
    ))
}

fn rep_of(id: u32) -> u64 {
    shape_descriptor_by_id(id).expect("live shape").rep
}

unsafe fn slot_bits(obj: *mut ObjectHeader, index: usize) -> u64 {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    *fields.add(index)
}

/// T3: a Number keeps an `F64` lane `F64`, and the funnel stores its
/// canonical double (an INT32 box as its double, any NaN as the canonical
/// NaN). Sabotage: skipping the check in `slot_store` stores the INT32 box
/// verbatim and the first bit assertion fails.
#[test]
fn a_number_keeps_an_f64_lane_and_is_stored_as_its_canonical_double() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let (obj, any) = abc();
        let typed = with_rep(any, with_slot_rep(0, 1, REP_F64));
        assert_ne!(typed, any);
        stamp_object_shape_id_with_carrier_note(obj, typed);

        crate::object::store_object_field_slot(obj, 1, crate::value::INT32_TAG | 7);
        assert_eq!(
            slot_bits(obj, 1),
            7.0f64.to_bits(),
            "an INT32 box is stored as its double"
        );
        assert_eq!(object_shape_stamp(obj), typed, "a Number never transitions");

        crate::object::store_object_field_slot(obj, 1, 0xFFF8_0000_0000_0001);
        assert_eq!(
            slot_bits(obj, 1),
            0x7FF8_0000_0000_0000,
            "any NaN is the canonical NaN"
        );
        crate::object::store_object_field_slot(obj, 1, f64::NEG_INFINITY.to_bits());
        assert_eq!(
            slot_bits(obj, 1),
            f64::NEG_INFINITY.to_bits(),
            "Infinity is a Number"
        );
        assert_eq!(object_shape_stamp(obj), typed);

        // An `Any` lane of the same shape is today's store, bit for bit.
        crate::object::store_object_field_slot(obj, 0, crate::value::INT32_TAG | 7);
        assert_eq!(slot_bits(obj, 0), crate::value::INT32_TAG | 7);
    }
}

/// T4: a non-Number into an `F64` lane generalizes it lineage-wide. The
/// carried shape learns the lane deprecated (not identity: minting its facts
/// again finds it), the receiver moves to the normalized shape (here the
/// all-`Any` shape it had before, the same facts) BEFORE the value lands, and
/// a sibling still carrying the old shape converges on the next miss.
/// Sabotage: dropping the restamp leaves an `undefined` under an `F64` lane
/// and the stamp assertion fails.
#[test]
fn a_non_number_generalizes_the_lane_for_the_whole_lineage() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let (obj, any) = abc();
        let (sibling, sibling_any) = abc();
        assert_eq!(sibling_any, any, "one lineage");
        let f64_at_1 = with_slot_rep(0, 1, REP_F64);
        let typed = with_rep(any, f64_at_1);
        stamp_object_shape_id_with_carrier_note(obj, typed);
        stamp_object_shape_id_with_carrier_note(sibling, typed);

        crate::object::store_object_field_slot(obj, 1, crate::value::TAG_UNDEFINED);
        assert_eq!(
            object_shape_stamp(obj),
            any,
            "moved to the normalized shape"
        );
        assert_eq!(slot_bits(obj, 1), crate::value::TAG_UNDEFINED);
        assert_eq!(
            slot_rep(rep_of(typed), 1),
            REP_F64_DEPRECATED,
            "the carried shape learned the lane"
        );
        assert_eq!(
            with_rep(any, f64_at_1),
            typed,
            "deprecation is not identity"
        );

        // The sibling still holds a Number at the lane: its shape is valid,
        // and a Number store keeps it; the next miss migrates it.
        crate::object::store_object_field_slot(sibling, 1, 5.0f64.to_bits());
        assert_eq!(object_shape_stamp(sibling), typed);
        assert!(migrate_deprecated_receiver(sibling));
        assert_eq!(object_shape_stamp(sibling), any);
        assert_eq!(
            slot_bits(sibling, 1),
            5.0f64.to_bits(),
            "migration moves no data"
        );
        assert!(!migrate_deprecated_receiver(sibling), "migrates once");
    }
}

/// The normalized successor keeps every lane that is still `F64` and follows
/// a record that has itself learned a deprecated lane since, to a fixed point.
#[test]
fn normalization_keeps_live_lanes_and_reaches_a_fixed_point() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let (obj, any) = abc();
        let (other, _) = abc();
        let both = with_slot_rep(with_slot_rep(0, 0, REP_F64), 2, REP_F64);
        let s = with_rep(any, both);
        stamp_object_shape_id_with_carrier_note(obj, s);
        stamp_object_shape_id_with_carrier_note(other, s);

        crate::object::store_object_field_slot(obj, 0, crate::value::TAG_NULL);
        let t = object_shape_stamp(obj);
        assert_eq!(rep_of(t), with_slot_rep(0, 2, REP_F64), "lane 2 stays F64");

        // T's lane 2 generalizes too; S (lane 0 deprecated) now normalizes
        // through T to the all-`Any` shape.
        crate::object::store_object_field_slot(obj, 2, crate::value::TAG_NULL);
        assert_eq!(object_shape_stamp(obj), any);
        assert_eq!(normalized_shape(s), any);
        assert_eq!(
            slot_rep(rep_of(s), 2),
            REP_F64,
            "S itself never saw lane 2 fail"
        );
        assert!(migrate_deprecated_receiver(other));
        assert_eq!(object_shape_stamp(other), any);
        assert_eq!(rep_of(any), REP_ANY);
        assert!(object_shape_descriptor(other).is_some());
    }
}
