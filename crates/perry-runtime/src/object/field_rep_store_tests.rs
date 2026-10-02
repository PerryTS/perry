//! Charter step 5 (P2a): the runtime store check and the generalization of a
//! shape's `F64` lane (`field_rep_store`), and (P2b) the key-add producer.
//! The store-check tests mint their shapes directly through the one intern
//! that takes a rep (`shape_descriptor_ensure_with_rep`) and stamp them on a
//! live object whose lanes hold Numbers.

use super::field_rep::{slot_rep, with_slot_rep, REP_ANY, REP_F64, REP_F64_DEPRECATED};
use super::field_rep_store::{migrate_deprecated_receiver, normalized_shape};
use super::shapes::{
    object_shape_descriptor, object_shape_stamp, publish_shape_result, shape_descriptor_by_id,
    shape_descriptor_ensure_with_rep, stamp_object_shape_id_with_carrier_note,
};
use super::ObjectHeader;

extern "C" fn constfn_body_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    11.0
}

extern "C" fn constfn_body_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    22.0
}

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// A live object `{a: 1, b: 2, c: 3}` and its (all-`Any`) shape.
unsafe fn abc() -> (*mut ObjectHeader, u32) {
    let obj = crate::object::js_object_alloc(0, 4);
    for (i, name) in ["a", "b", "c"].iter().enumerate() {
        crate::object::js_object_set_field_by_name(obj, key(name), (i + 1) as f64);
    }
    // The key-adds earned `F64` lanes (P2b); restamp to the all-`Any`
    // sibling (a valid claim for any object), so each test states its lanes.
    let stamped = object_shape_stamp(obj);
    assert_ne!(stamped, 0, "the fixture object is shaped");
    let id = with_rep(stamped, REP_ANY);
    stamp_object_shape_id_with_carrier_note(obj, id);
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
        None,
    ))
}

fn rep_of(id: u32) -> u64 {
    shape_descriptor_by_id(id).expect("live shape").rep
}

unsafe fn slot_bits(obj: *mut ObjectHeader, index: usize) -> u64 {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    *fields.add(index)
}

#[test]
fn constfn_same_body_keeps_shape_and_different_body_deprecates_on_store() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let _no_move = crate::gc::GcSuppressScope::new();
        let body_a =
            crate::fn_info!(constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
        let first = crate::closure::js_closure_alloc(body_a, 0);
        let second = crate::closure::js_closure_alloc(body_a, 0);
        let other = crate::closure::js_closure_alloc(
            crate::fn_info!(constfn_body_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        );
        let bits = |c: *mut crate::closure::ClosureHeader| {
            crate::value::js_nanbox_pointer(c as i64).to_bits()
        };
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            obj,
            key("constfn_method"),
            f64::from_bits(bits(first)),
        );
        let constfn = object_shape_stamp(obj);
        let d = shape_descriptor_by_id(constfn).expect("keyed shape");
        assert_eq!(d.special_constfn_mask, 1, "runtime key-add minted ConstFn");
        assert!(!super::field_rep_store::shape_slot_is_f64(constfn, 0));
        assert_eq!(d.constfn_infos()[0].info, (*first).info as usize as u64);
        let any = with_rep(constfn, REP_ANY);
        assert_ne!(any, constfn);
        // A cached edge cannot publish SPECIAL until its store is ordered
        // before the shape stamp; it deliberately takes the slow path.
        assert!(!super::field_rep_store::cached_key_add_admits(
            constfn,
            0,
            Some(bits(second))
        ));
        let sibling = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            sibling,
            key("constfn_method"),
            f64::from_bits(bits(second)),
        );
        assert_eq!(
            object_shape_stamp(sibling),
            constfn,
            "fresh closures share body shape"
        );
        assert_eq!(slot_bits(sibling, 0), bits(second));
        let other_obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            other_obj,
            key("constfn_method"),
            f64::from_bits(bits(other)),
        );
        assert_ne!(
            object_shape_stamp(other_obj),
            constfn,
            "different bodies split"
        );
        crate::object::store_object_field_slot(obj, 0, bits(second));
        assert_eq!(object_shape_stamp(obj), constfn);
        assert_eq!(slot_bits(obj, 0), bits(second), "load current closure");

        crate::object::store_object_field_slot(obj, 0, bits(other));
        assert_eq!(object_shape_stamp(obj), any, "different body goes to Any");
        assert_eq!(slot_bits(obj, 0), bits(other));
        assert_eq!(object_shape_stamp(sibling), constfn);
        assert!(migrate_deprecated_receiver(sibling));
        assert_eq!(object_shape_stamp(sibling), any);
        assert_eq!(slot_bits(sibling, 0), bits(second));
    }
}

#[test]
fn unloadable_body_stays_any_on_runtime_key_add() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let closure = crate::closure::js_closure_alloc(crate::fn_info!(constfn_body_b, 0), 0);
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            obj,
            key("unloadable_method"),
            crate::value::js_nanbox_pointer(closure as i64),
        );
        let d = shape_descriptor_by_id(object_shape_stamp(obj)).expect("keyed shape");
        assert_eq!(d.special_constfn_mask, 0);
        assert_eq!(slot_rep(d.rep, 0), REP_ANY);
    }
}

#[test]
fn rebindable_this_closure_stays_any_on_runtime_key_add() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let info =
            crate::fn_info!(constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
        let closure = crate::closure::js_closure_alloc(info, crate::closure::CAPTURES_THIS_FLAG);
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(
            obj,
            key("rebindable_method"),
            crate::value::js_nanbox_pointer(closure as i64),
        );
        let d = shape_descriptor_by_id(object_shape_stamp(obj)).expect("keyed shape");
        assert_eq!(d.special_constfn_mask, 0);
        assert_eq!(slot_rep(d.rep, 0), REP_ANY);
    }
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

// ---- P2b: the key-add producer (T2) -------------------------------------

/// A fresh `{}` with one inline allocation, keyed by names no other test
/// uses, so the transition cache holds only this test's edges.
unsafe fn add(obj: *mut ObjectHeader, name: &str, value: f64) -> u32 {
    crate::object::js_object_set_field_by_name(obj, key(name), value);
    object_shape_stamp(obj)
}

#[test]
fn a_number_key_add_publishes_an_f64_lane_and_a_non_number_an_any_lane() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let s = crate::string::js_string_from_bytes(b"txt".as_ptr(), 3);
        let text = f64::from_bits(crate::JSValue::string_ptr(s).bits());
        // Twice: the first object takes the slow path, the second the cached edge.
        let mut ids = Vec::new();
        for _ in 0..2 {
            let obj = crate::object::js_object_alloc(0, 4);
            add(obj, "p2b_n_a", 1.5);
            let id = add(obj, "p2b_n_b", text);
            let rep = rep_of(id);
            assert_eq!(slot_rep(rep, 0), REP_F64, "a Number key-add earns F64");
            assert_eq!(slot_rep(rep, 1), REP_ANY, "a string key-add stays Any");
            ids.push(id);
        }
        assert_eq!(
            ids[0], ids[1],
            "the cached edge reaches the slow path's shape"
        );
    }
}

#[test]
fn a_cached_f64_edge_refuses_a_non_number() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let first = crate::object::js_object_alloc(0, 4);
        let f64_id = add(first, "p2b_r_a", 7.0);
        assert_eq!(slot_rep(rep_of(f64_id), 0), REP_F64);
        let s = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
        let second = crate::object::js_object_alloc(0, 4);
        let id = add(
            second,
            "p2b_r_a",
            f64::from_bits(crate::JSValue::string_ptr(s).bits()),
        );
        assert_ne!(id, f64_id, "a string must not take the F64 edge");
        assert_eq!(slot_rep(rep_of(id), 0), REP_ANY);
    }
}

#[test]
fn an_int32_key_add_is_stored_as_its_double_on_both_paths() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let boxed = f64::from_bits(crate::value::INT32_TAG | 5);
        for _ in 0..2 {
            let obj = crate::object::js_object_alloc(0, 4);
            add(obj, "p2b_i_a", 1.0);
            let id = add(obj, "p2b_i_b", boxed);
            assert_eq!(slot_rep(rep_of(id), 1), REP_F64);
            assert_eq!(
                slot_bits(obj, 1),
                5.0f64.to_bits(),
                "canonical double in an F64 lane"
            );
        }
    }
}

#[test]
fn a_restamp_and_a_bound_change_keep_the_lanes() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        let id = add(obj, "p2b_k_a", 3.0);
        assert_eq!(slot_rep(rep_of(id), 0), REP_F64);
        let d = object_shape_descriptor(obj).expect("shaped");
        let again = super::shapes::stamp_object_shape(
            obj,
            d.keys as usize as *const crate::array::ArrayHeader,
            d.logical_key_count,
            d.live_inline_slot_count,
        );
        assert_eq!(again, id, "a same-edge restamp is the same shape");
        let grown =
            super::shapes::publish_object_live_slot_count(obj, d.live_inline_slot_count + 1);
        assert_eq!(
            slot_rep(rep_of(grown), 0),
            REP_F64,
            "a bound change moves no slot"
        );
    }
}

#[test]
fn a_key_add_after_generalization_converges_on_the_normalized_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        // A second key, so the add takes the append arm: the first-key arm's
        // trailing same-edge restamp would normalize on its own.
        let first = crate::object::js_object_alloc(0, 4);
        add(first, "p2b_g_0", 0.5);
        let f64_id = add(first, "p2b_g_a", 1.0);
        let s = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
        let general = add(
            first,
            "p2b_g_a",
            f64::from_bits(crate::JSValue::string_ptr(s).bits()),
        );
        assert_ne!(general, f64_id, "the store generalized");
        assert_eq!(slot_rep(rep_of(general), 1), REP_ANY);
        // A new object adding the same key with a Number: the cached edge's
        // target learned a deprecated lane, so it must not serve.
        let next = crate::object::js_object_alloc(0, 4);
        add(next, "p2b_g_0", 0.5);
        assert_eq!(
            add(next, "p2b_g_a", 2.0),
            general,
            "new objects never get S again"
        );
    }
}

/// P2d key-add convergence (DESIGN §1.5 step 4) with BOTH siblings born on
/// the runtime path, where no site memo re-primes onto one of them: a
/// Number key-add makes the `F64` successor, a string key-add of the same
/// key from the same predecessor makes the `Any` one. The second must
/// deprecate the first's lane, so an object still on it converges on the
/// `Any` shape at its next miss and a later Number key-add is born there:
/// one shape for the lineage, not two for good.
#[test]
fn a_non_number_key_add_deprecates_its_f64_sibling() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let first = crate::object::js_object_alloc(0, 4);
        add(first, "p2d_c_0", 0.5);
        let f64_id = add(first, "p2d_c_a", 1.0);
        assert_eq!(slot_rep(rep_of(f64_id), 1), REP_F64);
        let second = crate::object::js_object_alloc(0, 4);
        add(second, "p2d_c_0", 0.5);
        let any_id = add(second, "p2d_c_a", boxed("s"));
        assert_ne!(any_id, f64_id, "the string took the Any sibling");
        assert_eq!(slot_rep(rep_of(any_id), 1), REP_ANY);
        assert!(
            super::field_rep::has_deprecated(rep_of(f64_id)),
            "the string key-add deprecated the F64 sibling's lane"
        );
        assert!(migrate_deprecated_receiver(first));
        assert_eq!(
            object_shape_stamp(first),
            any_id,
            "one shape for the lineage"
        );
        let third = crate::object::js_object_alloc(0, 4);
        add(third, "p2d_c_0", 0.5);
        assert_eq!(
            add(third, "p2d_c_a", 2.0),
            any_id,
            "a Number key-add is born into the Any shape"
        );
    }
}

fn boxed(name: &str) -> f64 {
    f64::from_bits(crate::value::js_nanbox_string(key(name) as i64).to_bits())
}

/// P2c: the invariant check must be able to fail. A non-Number written
/// raw under an `F64` lane trips it.
#[test]
#[should_panic(expected = "field-rep invariant")]
fn the_invariant_check_fires_on_a_non_number_under_an_f64_lane() {
    unsafe {
        let (obj, id) = abc();
        let f64_a = with_rep(id, with_slot_rep(REP_ANY, 0, REP_F64));
        stamp_object_shape_id_with_carrier_note(obj, f64_a);
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        super::field_rep_store::assert_field_rep_lanes(
            obj,
            super::shapes::object_shape_record(obj),
            3,
        );
        // GC_STORE_AUDIT(INIT): the deliberate unchecked store this test
        // exists to catch; nothing collects before the check below.
        *fields = boxed("not a number").to_bits();
        super::field_rep_store::assert_field_rep_lanes(
            obj,
            super::shapes::object_shape_record(obj),
            3,
        );
    }
}

/// Delete's raw moves (the hole write, the shift that writes `b`'s string
/// into slot 0, the squeeze) carry no store check. They need none: every
/// delete of a receiver whose lanes are live republishes its shape with all
/// lanes `Any` before it moves a slot. A shared key list is forked or
/// compacted, and both publish through `set_object_keys` /
/// `publish_object_shape_from` (`REP_ANY`); an owned list only exists after
/// such a fork, and its in-place re-adds keep the fork's `Any` record.
#[test]
fn delete_publishes_any_lanes_before_its_raw_moves() {
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(obj, key("a"), 1.5);
        let s = boxed("s");
        crate::object::js_object_set_field_by_name(obj, key("b"), s);
        crate::object::js_object_set_field_by_name(obj, key("c"), 2.5);
        let before = super::field_rep_store::shape_rep(object_shape_stamp(obj));
        assert_eq!(
            slot_rep(before, 0),
            REP_F64,
            "the fixture has an F64 lane at a"
        );
        assert_eq!(slot_rep(before, 2), REP_F64, "and at c");
        assert_eq!(crate::object::js_object_delete_field(obj, key("a")), 1);
        assert_eq!(
            super::field_rep_store::shape_rep(object_shape_stamp(obj)),
            REP_ANY,
            "the delete successor carries no lane"
        );
        super::field_rep_store::assert_field_rep_lanes(
            obj,
            super::shapes::object_shape_record(obj),
            3,
        );
        let b = crate::object::js_object_get_field_by_name_f64(obj, key("b"));
        assert_eq!(b.to_bits(), s.to_bits(), "b moved down intact");
    }
}

/// T2 grants a class instance's inline key-add the `F64` lane like any
/// other receiver's. The class-keyed writers stay sound because they compare
/// the ShapeId against the class's birth shape, whose lanes are `Any`: an
/// instance carrying a lane is on a different shape and misses them.
#[test]
fn a_class_instance_key_add_earns_the_lane_too() {
    unsafe {
        let plain = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(plain, key("n"), 1.5);
        let plain_rep = super::field_rep_store::shape_rep(object_shape_stamp(plain));
        assert_eq!(
            slot_rep(plain_rep, 0),
            REP_F64,
            "a plain object earns the lane"
        );
        let inst = crate::object::js_object_alloc(0x00C0_FFEE, 4);
        assert!(!crate::object::is_anon_shape_class_id((*inst).class_id));
        crate::object::js_object_set_field_by_name(inst, key("n"), 1.5);
        let inst_rep = super::field_rep_store::shape_rep(object_shape_stamp(inst));
        assert_ne!(object_shape_stamp(inst), 0, "the instance is shaped");
        assert_eq!(slot_rep(inst_rep, 0), REP_F64);
    }
}

/// The diagnostic validates old carriers too: deprecation grants no permission
/// for an unchecked store to replace their recorded body.
#[test]
fn constfn_verifier_valid_stale_deprecated_and_lifecycle() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let a = crate::closure::js_closure_alloc(
            crate::fn_info!(constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        );
        let b = crate::closure::js_closure_alloc(
            crate::fn_info!(constfn_body_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        );
        let obj = crate::object::js_object_alloc(0, 4);
        let name = key("constfn_verifier_body");
        crate::object::js_object_set_field_by_name(
            obj,
            name,
            f64::from_bits(crate::JSValue::object_ptr(a.cast()).bits()),
        );
        let record = super::shapes::object_shape_record(obj).unwrap();
        assert_eq!(
            record.special_constfn_mask(),
            1,
            "fixture must carry SPECIAL"
        );
        let verify = || super::field_rep_store::assert_field_rep_lanes(obj, Some(record), 1);
        verify();
        assert!(record.deprecate_special_to_any(0));
        verify(); // An unchanged old carrier still satisfies its old body.
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        for bits in [
            crate::JSValue::object_ptr(b.cast()).bits(),
            crate::value::TAG_UNDEFINED,
        ] {
            // GC_STORE_AUDIT(INIT): deliberate sabotage, no collection before verification.
            *fields = bits;
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(verify));
            let message = failure.expect_err("stale SPECIAL must fail");
            let text = message
                .downcast_ref::<String>()
                .map(String::as_str)
                .unwrap_or("");
            assert!(
                text.contains("field-rep invariant: SPECIAL ConstFn"),
                "{text}"
            );
        }
        // Restore before the checked store; its Any successor relinquishes the
        // image-body fact, so revocation requires no metadata dereference.
        *fields = crate::JSValue::object_ptr(a.cast()).bits();
        crate::object::js_object_set_field_by_name(
            obj,
            name,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(
            super::shapes::object_shape_record(obj)
                .unwrap()
                .special_constfn_mask(),
            0
        );
        (*a).info = std::ptr::null();
        super::field_rep_store::assert_field_rep_lanes(
            obj,
            super::shapes::object_shape_record(obj),
            1,
        );
    }
}
