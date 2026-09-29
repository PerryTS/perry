//! Charter step 3: the store facts are shape facts. One test per WRITER of a
//! fact input, each asserting the receiver's shape kind agrees with its
//! per-object record (`store_facts_agree`) after the write — and the
//! invariant's own must-fail test, which manufactures a disagreement.

use super::*;
use crate::object::shapes::{object_shape_id, shape_descriptor_by_id, ShapeObjectKind};

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn kind_of(obj: *const ObjectHeader) -> ShapeObjectKind {
    crate::object::shapes::shape_object_kind_by_id(object_shape_id(obj))
        .expect("test premise: a stamped receiver")
}

unsafe fn with_key(obj: *mut ObjectHeader, name: &str) -> *mut ObjectHeader {
    crate::object::js_object_set_field_by_name(obj, key(name), 1.0);
    obj
}

/// A class id that is neither 0, the native-module id nor `u32::MAX`, and not
/// a registered class (so no vtable prototype identity).
const PLAIN_CLASS: u32 = 0x5F3A;

/// Every birth derives its kind from its record: class-less unmarked births
/// are `OrdinaryUnmarked`, marked (`object_alloc_plain`) and class births are
/// `Ordinary`, a native-module receiver is `OrdinaryUnmarked`.
#[test]
fn births_carry_the_kind_their_record_derives() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let unmarked = with_key(crate::object::js_object_alloc(0, 0), "sk_a");
        assert_eq!(kind_of(unmarked), ShapeObjectKind::OrdinaryUnmarked);
        assert!(store_facts_agree(unmarked));

        let plain = with_key(crate::object::object_alloc_plain(0), "sk_a");
        assert_eq!(kind_of(plain), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(plain));
        assert_eq!(
            shape_descriptor_by_id(object_shape_id(plain)).unwrap().keys,
            shape_descriptor_by_id(object_shape_id(unmarked))
                .unwrap()
                .keys,
            "test premise: the two receivers differ ONLY in the store fact"
        );
        assert_ne!(
            object_shape_id(plain),
            object_shape_id(unmarked),
            "distinct store facts must be distinct ShapeIds"
        );

        let instance = with_key(crate::object::js_object_alloc(PLAIN_CLASS, 0), "sk_a");
        assert_eq!(kind_of(instance), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(instance));

        let native = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 0);
        let native = with_key(native, "sk_a");
        assert_eq!(kind_of(native), ShapeObjectKind::OrdinaryUnmarked);
        assert!(store_facts_agree(native));
        assert!(!shape_admits_plain_store(object_shape_id(native)));
        assert!(shape_admits_plain_store(object_shape_id(instance)));
    }
}

/// Writer: `mark_object_plain_ordinary` on a stamped receiver moves it to its
/// `Ordinary` twin — same keys, another id — and later key-adds stay there.
#[test]
fn marking_moves_a_stamped_receiver_to_its_ordinary_twin() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = with_key(crate::object::js_object_alloc(0, 0), "sk_m");
        let before = object_shape_id(obj);
        assert_eq!(kind_of(obj), ShapeObjectKind::OrdinaryUnmarked);
        crate::object::mark_object_plain_ordinary(obj);
        let after = object_shape_id(obj);
        assert_ne!(before, after, "the mark must transition the shape");
        assert_eq!(kind_of(obj), ShapeObjectKind::Ordinary);
        assert_eq!(
            shape_descriptor_by_id(before).unwrap().keys,
            shape_descriptor_by_id(after).unwrap().keys
        );
        assert!(store_facts_agree(obj));
        // Lineage carries the receiver's kind, re-derived.
        with_key(obj, "sk_m2");
        assert_eq!(kind_of(obj), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(obj));
    }
}

/// Writer: a post-birth `class_id` rewrite (`restamp_object_proto_id`, and
/// `restamp_object_store_kind` for the rewrites that keep the prototype).
#[test]
fn a_class_id_rewrite_moves_the_store_kind() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = with_key(crate::object::js_object_alloc(0, 0), "sk_c");
        assert_eq!(kind_of(obj), ShapeObjectKind::OrdinaryUnmarked);
        (*obj).class_id = PLAIN_CLASS;
        crate::object::shapes::restamp_object_proto_id(obj);
        assert_eq!(kind_of(obj), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(obj));

        let other = with_key(crate::object::js_object_alloc(0, 0), "sk_c");
        (*other).class_id = PLAIN_CLASS;
        restamp_object_store_kind(other);
        assert_eq!(kind_of(other), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(other));

        (*other).class_id = crate::object::NATIVE_MODULE_CLASS_ID;
        restamp_object_store_kind(other);
        assert_eq!(kind_of(other), ShapeObjectKind::OrdinaryUnmarked);
        assert!(store_facts_agree(other));

        // A rewrite that keeps the prototype identity (an anonymous-shape
        // class implies no vtable prototype): the prototype transition mints
        // nothing, so only `restamp_object_proto_id`'s store-kind re-derive
        // can move the shape.
        const ANON: u32 = 0x5F3C;
        crate::object::class_registry::js_register_anon_shape_class_id(ANON);
        let third = with_key(crate::object::js_object_alloc(0, 0), "sk_c");
        let proto_before = crate::object::shapes::shape_proto_id(object_shape_id(third));
        (*third).class_id = ANON;
        crate::object::shapes::restamp_object_proto_id(third);
        assert_eq!(
            crate::object::shapes::shape_proto_id(object_shape_id(third)),
            proto_before,
            "test premise: the rewrite keeps the prototype identity"
        );
        assert_eq!(kind_of(third), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(third));
    }
}

/// Writer: `OBJ_FLAG_TYPED_ARRAY_PROTO` on a marked class-less receiver
/// withdraws F-A (`global_this/populate.rs`).
#[test]
fn the_typed_array_prototype_flag_withdraws_admission() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = with_key(crate::object::object_alloc_plain(0), "sk_t");
        assert_eq!(kind_of(obj), ShapeObjectKind::Ordinary);
        (*gc_header(obj))._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
        restamp_object_store_kind(obj);
        assert_eq!(kind_of(obj), ShapeObjectKind::OrdinaryUnmarked);
        assert!(store_facts_agree(obj));
    }
}

/// Writer: the Array-subclass numeric proof. Publishing moves the receiver to
/// its proof twin, retiring moves it back to the SAME unproven id, and any
/// other stamp (a key-add) retires the proof itself.
#[test]
fn the_numeric_proof_is_a_shape_transition() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = with_key(crate::object::js_object_alloc(PLAIN_CLASS, 0), "sk_p");
        let unproven = object_shape_id(obj);
        assert_eq!(kind_of(obj), ShapeObjectKind::Ordinary);

        assert_eq!(stamp_numeric_proof_twin(obj), Some(unproven));
        let proven = object_shape_id(obj);
        assert_ne!(proven, unproven, "publishing the proof must transition");
        assert_eq!(kind_of(obj), ShapeObjectKind::OrdinaryNumericProof);
        assert!(receiver_carries_numeric_proof(obj));
        assert!(store_facts_agree(obj));
        assert!(
            !shape_admits_plain_store(proven),
            "a proof-carrying shape is never store-admitted"
        );

        assert!(crate::array::clear_packed_subclass_numeric_proof(obj));
        assert_eq!(
            object_shape_id(obj),
            unproven,
            "retire returns to the unproven twin"
        );
        assert!(!receiver_carries_numeric_proof(obj));
        assert!(store_facts_agree(obj));

        // Re-publishing finds the same proof twin (no new mint).
        assert_eq!(stamp_numeric_proof_twin(obj), Some(unproven));
        assert_eq!(object_shape_id(obj), proven);
        // A structural change retires the proof in the stamp funnel.
        with_key(obj, "sk_p2");
        assert!(!receiver_carries_numeric_proof(obj));
        assert_eq!(kind_of(obj), ShapeObjectKind::Ordinary);
        assert!(store_facts_agree(obj));

        // An unmarked receiver cannot carry the proof.
        let unmarked = with_key(crate::object::js_object_alloc(0, 0), "sk_p");
        assert_eq!(stamp_numeric_proof_twin(unmarked), None);
        assert!(!receiver_carries_numeric_proof(unmarked));
    }
}

/// The invariant must be able to fail: each per-object write WITHOUT its
/// transition is a disagreement `store_facts_agree` reports — the unit form of
/// sabotaging each writer.
#[test]
fn the_invariant_reports_every_unmoved_write() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        // Mark without transition.
        let a = with_key(crate::object::js_object_alloc(0, 0), "sk_i");
        (*gc_header(a))._reserved |= crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
        assert!(!store_facts_agree(a), "an unmoved mark must disagree");
        restamp_object_store_kind(a);
        assert!(store_facts_agree(a));

        // Class-id rewrite without transition.
        let b = with_key(crate::object::js_object_alloc(0, 0), "sk_i");
        (*b).class_id = PLAIN_CLASS;
        assert!(
            !store_facts_agree(b),
            "an unmoved class-id rewrite must disagree"
        );

        // Typed-array-prototype flag without transition.
        let c = with_key(crate::object::object_alloc_plain(0), "sk_i");
        (*gc_header(c))._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
        assert!(
            !store_facts_agree(c),
            "an unmoved typed-array-prototype flag must disagree"
        );

        // Proof bit without transition, and proof shape without the bit.
        let d = with_key(crate::object::js_object_alloc(PLAIN_CLASS, 0), "sk_i");
        (*gc_header(d))._reserved |= crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
        assert!(
            !store_facts_agree(d),
            "a proof bit on an Ordinary shape must disagree"
        );
        (*gc_header(d))._reserved &= !crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
        assert!(stamp_numeric_proof_twin(d).is_some());
        (*gc_header(d))._reserved &= !crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
        assert!(
            !store_facts_agree(d),
            "a proof shape without the bit must disagree"
        );
    }
}

/// R3: a caller-supplied id whose kind is not the receiver's is declined —
/// a class-less UNMARKED newborn offered the keys-only (`Ordinary`) id mints
/// its own `OrdinaryUnmarked` twin instead of carrying a store-admitted id.
#[test]
fn an_explicit_id_of_the_wrong_kind_is_declined() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let keys = crate::array::js_array_alloc_with_length(1);
        let name = key("sk_x");
        crate::array::js_array_set(keys, 0, crate::JSValue::string_ptr(name));
        let ordinary = crate::object::shapes::shape_id_for_keys_ensure(keys, 1);
        assert_eq!(
            crate::object::shapes::shape_object_kind_by_id(ordinary),
            Some(ShapeObjectKind::Ordinary)
        );
        let unmarked =
            crate::object::js_object_alloc_class_inline_keys_stamped(0, 0, 1, keys, ordinary);
        assert_ne!(
            object_shape_id(unmarked),
            ordinary,
            "the Ordinary id must be declined"
        );
        assert_eq!(kind_of(unmarked), ShapeObjectKind::OrdinaryUnmarked);
        assert!(store_facts_agree(unmarked));

        let plain =
            crate::object::alloc_plain::alloc_plain_record_inline_keys_stamped(1, keys, ordinary);
        assert_eq!(
            object_shape_id(plain),
            ordinary,
            "a marked newborn takes it"
        );
        assert!(store_facts_agree(plain));
    }
}
