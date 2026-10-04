//! Read-only numeric words use own-slot proof without granting store rights.

use super::*;
use crate::object::field_rep::{slot_rep, REP_ANY, REP_F64};
use core::sync::atomic::{AtomicU64, Ordering};

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn bits(key: *const crate::StringHeader) -> u64 {
    crate::value::js_nanbox_string(key as i64).to_bits()
}

unsafe fn record(name: &str) -> (*mut crate::ObjectHeader, *mut crate::StringHeader) {
    let obj = crate::object::js_object_alloc(0, 0);
    let key = key(name);
    let boxed = crate::value::js_nanbox_pointer(obj as i64);
    crate::proxy::js_put_value_set(
        boxed,
        f64::from_bits(bits(self::key("rnr_kind"))),
        f64::from_bits(bits(self::key("min"))),
        boxed,
        0,
    );
    crate::proxy::js_put_value_set(boxed, f64::from_bits(bits(key)), 7.0, boxed, 0);
    let d = object_shape_descriptor(obj).unwrap();
    assert_eq!(d.object_kind, ShapeObjectKind::OrdinaryUnmarked);
    assert_eq!(d.semantic_generation, 0);
    assert_eq!(d.hole_count, 0);
    assert_eq!(d.summary, 0);
    assert_eq!(slot_rep(d.rep, 0), REP_ANY);
    assert_eq!(slot_rep(d.rep, 1), REP_F64);
    (obj, key)
}

unsafe fn prime(site: &AtomicU64, obj: *const crate::ObjectHeader, key: u64) -> u64 {
    js_region_loop_prime(
        site,
        object_shape_stamp(obj),
        1,
        key,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        1,
    )
}

#[test]
fn unmarked_numeric_read_publishes_and_reads_the_production_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key) = record("rnr_positive");
        let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
        let word = prime(&site, obj, bits(key));
        assert_ne!(word, REGION_GUARD_WORD_EMPTY);
        assert_eq!(site.load(Ordering::Relaxed), word);
        assert_eq!(word as u32, object_shape_stamp(obj));
        let slot = ((word >> 32) & 63) as usize;
        assert_eq!(
            slot, 1,
            "the string kind precedes the numeric value, as in Zod"
        );
        let raw = (obj as *const u8).add(core::mem::size_of::<crate::ObjectHeader>() + slot * 8)
            as *const f64;
        assert_eq!(*raw, 7.0);
        assert_eq!(
            crate::object::js_object_get_field_by_name(obj, key).bits(),
            (*raw).to_bits()
        );
        assert!(!store_kind::shape_admits_plain_store(word as u32));
    }
}

#[test]
fn unmarked_numeric_read_does_not_admit_stores_or_non_numeric_regions() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key) = record("rnr_store");
        let id = object_shape_stamp(obj);
        for (stored, boxed, r) in [
            (1, 0, 1),
            (1, 1, 1),
            (0, 1, 1),
            (0, 0, 0),
            (2, 0, 1),
            (0, 0, 1 << 31),
            (0, 0, 3),
        ] {
            assert_eq!(
                region_loop_pack(id, 1, [bits(key), 0, 0, 0, 0], stored, boxed, r),
                Err(RegionRefusal::Kind),
                "stored={stored} boxed={boxed} R={r}"
            );
        }
    }
}

#[test]
fn unmarked_numeric_read_retains_generation_holes_rep_and_absence_checks() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key) = record("rnr_facts");
        let d = object_shape_descriptor(obj).unwrap();
        let mint = |generation, holes, rep| {
            publish_shape_result(shape_descriptor_ensure_with_rep(
                d.keys as usize as *const ArrayHeader,
                d.logical_key_count,
                d.live_inline_slot_count,
                generation,
                d.object_kind,
                holes,
                d.proto_id,
                crate::object::shapes::ReceiverFacts::NONE,
                rep,
                None,
            ))
        };
        for (id, expected) in [
            (mint(1, 0, d.rep), RegionRefusal::Kind),
            (mint(0, 1, d.rep), RegionRefusal::Kind),
        ] {
            assert_eq!(
                region_loop_pack(id, 1, [bits(key), 0, 0, 0, 0], 0, 0, 1),
                Err(expected)
            );
        }
        // An Any lane serves the read only through the guard's value test.
        let word = region_loop_pack(mint(0, 0, REP_ANY), 1, [bits(key), 0, 0, 0, 0], 0, 0, 1)
            .expect("an Any lane is served with a value test");
        assert_ne!(word & REGION_LOOP_WORD_VALUE_TEST, 0);
        assert_eq!(
            region_loop_pack(
                object_shape_stamp(obj),
                1,
                [bits(self::key("rnr_absent")), 0, 0, 0, 0],
                0,
                0,
                1
            ),
            Err(RegionRefusal::Absent)
        );
    }
}

#[test]
fn unmarked_numeric_read_refuses_accessors_and_value_tests_deprecated_lanes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key) = record("rnr_accessor");
        let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
        let before = prime(&site, obj, bits(key));
        let boxed = crate::value::js_nanbox_pointer(obj as i64);
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        crate::object::js_object_define_accessor(
            boxed,
            f64::from_bits(bits(key)),
            undefined,
            undefined,
        );
        assert_ne!(object_shape_stamp(obj), before as u32);
        assert_ne!(object_shape_descriptor(obj).unwrap().summary, 0);
        assert_eq!(prime(&site, obj, bits(key)), REGION_GUARD_WORD_EMPTY);

        let (other, other_key) = record("rnr_deprecated");
        shape_record_by_id(object_shape_stamp(other))
            .unwrap()
            .deprecate_rep_slot(1);
        let word = prime(&site, other, bits(other_key));
        assert_ne!(word, REGION_GUARD_WORD_EMPTY);
        assert_ne!(
            word & REGION_LOOP_WORD_VALUE_TEST,
            0,
            "a deprecated lane is served only through the guard's value test"
        );
    }
}

#[test]
fn unmarked_numeric_read_word_misses_after_mutation_or_exotic_reclassification() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let (obj, key) = record("rnr_mutation");
        let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
        let before = prime(&site, obj, bits(key));
        let boxed = crate::value::js_nanbox_pointer(obj as i64);
        crate::proxy::js_put_value_set(
            boxed,
            f64::from_bits(bits(key)),
            f64::from_bits(crate::value::TAG_TRUE),
            boxed,
            0,
        );
        assert_ne!(object_shape_stamp(obj), before as u32);
        // The generalized lane is served only through the guard's value test,
        // which `true` fails.
        let after = prime(&site, obj, bits(key));
        assert_ne!(after as u32, before as u32);
        assert_ne!(after & REGION_LOOP_WORD_VALUE_TEST, 0);

        let (proto, proto_key) = record("rnr_proto");
        let before = prime(&site, proto, bits(proto_key));
        crate::object::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(proto as i64),
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert_ne!(object_shape_stamp(proto), before as u32);
        assert_eq!(
            object_shape_descriptor(proto).unwrap().proto_id,
            PROTO_ID_NULL
        );

        let (exotic, exotic_key) = record("rnr_exotic");
        let before = prime(&site, exotic, bits(exotic_key));
        crate::object::proto_validity::mark_exotic_read_receiver(exotic as usize);
        assert_ne!(object_shape_stamp(exotic), before as u32);
        assert_eq!(
            object_shape_descriptor(exotic).unwrap().proto_id,
            PROTO_ID_PER_OBJECT
        );
        assert_eq!(
            prime(&site, exotic, bits(exotic_key)),
            REGION_GUARD_WORD_EMPTY
        );
        crate::object::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(exotic as i64),
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert_eq!(object_proto_id(exotic), PROTO_ID_PER_OBJECT);
        assert_eq!(
            object_shape_descriptor(exotic).unwrap().proto_id,
            PROTO_ID_PER_OBJECT
        );
        assert_eq!(
            prime(&site, exotic, bits(exotic_key)),
            REGION_GUARD_WORD_EMPTY,
            "a null prototype cannot erase virtual read semantics"
        );
    }
}

#[test]
fn native_namespace_virtual_value_cannot_publish_a_numeric_own_slot_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        // Manufacture an F64 physical slot before giving the receiver its
        // namespace brand. The virtual override then differs from the slot.
        let obj = crate::object::js_object_alloc(0, 0);
        let module_key = self::key("__module__");
        let module_value = self::key("rnr_test_namespace");
        crate::object::js_object_set_field_by_name(
            obj,
            module_key,
            f64::from_bits(bits(module_value)),
        );
        let key = self::key("rnr_native");
        crate::object::js_object_set_field_by_name(obj, key, 7.0);
        let rep = object_shape_descriptor(obj).unwrap().rep;
        assert_eq!(slot_rep(rep, 1), REP_F64);
        (*obj).class_id = crate::object::NATIVE_MODULE_CLASS_ID;
        restamp_object_proto_id(obj);
        // The prototype transition conservatively loses rep facts. Mint an
        // artificial but compatible native F64 descriptor, so rejection
        // cannot be explained by a missing numeric lane.
        let native = object_shape_descriptor(obj).unwrap();
        let native_f64 = publish_shape_result(shape_descriptor_ensure_with_rep(
            native.keys as usize as *const ArrayHeader,
            native.logical_key_count,
            native.live_inline_slot_count,
            native.semantic_generation,
            native.object_kind,
            native.hole_count,
            native.proto_id,
            crate::object::shapes::ReceiverFacts::of_descriptor(&native, native.summary),
            rep,
            None,
        ));
        stamp_object_shape_id_with_carrier_note(obj, native_f64);
        crate::object::native_module::install_native_module_vtable();
        crate::object::native_module::native_namespace_prop_override_store(
            "rnr_test_namespace",
            "rnr_native",
            17.0,
        );
        assert_eq!(
            crate::object::js_object_get_field_by_name(obj, key).bits(),
            17.0f64.to_bits()
        );
        let d = object_shape_descriptor(obj).unwrap();
        assert_eq!(d.object_kind, ShapeObjectKind::OrdinaryUnmarked);
        assert_eq!(slot_rep(d.rep, 1), REP_F64);
        assert_eq!(d.proto_id, PROTO_ID_PER_OBJECT);
        let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
        assert_eq!(prime(&site, obj, bits(key)), REGION_GUARD_WORD_EMPTY);
        assert_eq!(site.load(Ordering::Relaxed), REGION_GUARD_WORD_EMPTY);

        let born = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 0);
        assert_eq!(
            object_shape_descriptor(born).unwrap().proto_id,
            PROTO_ID_PER_OBJECT,
            "namespace classification is present before any key or cache is installed"
        );
    }
}

#[test]
fn unmarked_numeric_read_refuses_spill_even_when_the_generic_value_is_number() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 0);
        let mut last = core::ptr::null_mut();
        for i in 0..40 {
            last = key(&format!("rnr_spill_{i}"));
            crate::object::js_object_set_field_by_name(obj, last, i as f64);
        }
        let d = object_shape_descriptor(obj).unwrap();
        let location = region_key_location(&d, bits(last)).unwrap();
        assert!(location.0, "premise: requested numeric value is spilled");
        let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
        assert_eq!(
            crate::object::js_object_get_field_by_name(obj, last).bits(),
            39.0f64.to_bits()
        );
        assert_eq!(prime(&site, obj, bits(last)), REGION_GUARD_WORD_EMPTY);
    }
}
