//! #10905: an `Object.create(P)` birth is allocated as wide as the
//! descendants of its birth shape `(P, [])` grow, and that width is a fact of
//! the birth shape's record.

use super::{LEARNED_WIDTH_MAX, TRACKING_BIRTHS, TRACKING_WIDTH};
use crate::object::ObjectHeader;

const FIELDS: [&str; 5] = ["bw_a", "bw_b", "bw_c", "bw_e", "bw_d"];

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
}

/// `Object.create(proto)` through the runtime entry compiled code calls.
fn create(proto: *mut ObjectHeader) -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(crate::object::js_object_create(boxed(proto)))
        as *mut ObjectHeader
}

fn fill(obj: *mut ObjectHeader, fields: &[&str]) {
    for (i, f) in fields.iter().enumerate() {
        crate::object::js_object_set_field_by_name(obj, key(f), i as f64);
    }
}

unsafe fn live(obj: *const ObjectHeader) -> u32 {
    crate::object::object_live_slot_count(obj)
}

/// Does `obj` keep any of its values in overflow storage?
unsafe fn spilled(obj: *const ObjectHeader) -> bool {
    let meta = (*obj).meta;
    !meta.is_null() && (*meta).spill != 0
}

/// A fresh prototype: every test gets its own birth shape.
fn prototype() -> *mut ObjectHeader {
    let proto = crate::object::js_object_alloc(0, 4);
    crate::object::js_object_set_field_by_name(proto, key("bw_inherited"), 6.0);
    proto
}

#[test]
fn object_create_births_are_allocated_as_wide_as_their_descendants_grow() {
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = prototype();
        // While the birth shape is tracking, a birth gets the tracking width,
        // so the first objects a program creates (often its only ones) keep
        // five own fields inline.
        for i in 0..TRACKING_BIRTHS {
            let o = create(proto);
            assert_eq!(
                live(o),
                TRACKING_WIDTH,
                "tracking birth {i} was not served slack"
            );
            fill(o, &FIELDS);
            assert!(
                !spilled(o),
                "tracking birth {i} spilled with {} fields",
                FIELDS.len()
            );
        }
        // Tracking is over: every later birth is exactly as wide as the
        // descendants grew — the five fields, not the tracking width and not
        // the two-slot floor that spilled three of them.
        for i in 0..4 {
            let o = create(proto);
            assert_eq!(
                live(o),
                FIELDS.len() as u32,
                "birth {i} after tracking is not the learned width"
            );
            fill(o, &FIELDS);
            assert!(!spilled(o), "birth {i} after tracking spilled");
            // The width is capacity only: the keys stay authoritative.
            let keys = crate::object::js_object_keys(o);
            assert_eq!(crate::array::js_array_length(keys), FIELDS.len() as u32);
            let v = crate::object::js_object_get_field_by_name_f64(o, key("bw_d"));
            assert_eq!(v, 4.0);
        }
    }
}

#[test]
fn a_birth_shape_whose_descendants_never_grow_births_at_the_floor() {
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = prototype();
        for _ in 0..TRACKING_BIRTHS {
            let o = create(proto);
            fill(o, &FIELDS[..1]);
        }
        let o = create(proto);
        assert_eq!(
            live(o),
            0,
            "nothing grew past the floor, so nothing is reserved"
        );
        // And a program that DOES grow later is still correct: the new key
        // spills exactly as before, and that spill teaches the birth shape.
        fill(o, &FIELDS);
        assert!(
            spilled(o),
            "test premise: a floor birth spills its third key"
        );
        let next = create(proto);
        assert_eq!(
            live(next),
            FIELDS.len() as u32,
            "the growth did not teach the birth shape"
        );
    }
}

#[test]
fn polymorphic_growth_takes_the_widest_and_is_capped() {
    let _gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = prototype();
        let names: Vec<String> = (0..LEARNED_WIDTH_MAX + 8)
            .map(|i| format!("bw_k{i}"))
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        for i in 0..TRACKING_BIRTHS {
            let o = create(proto);
            // One lineage grows to 3 keys, another past the cap.
            let n = if i == 0 { names.len() } else { 3 };
            fill(o, &names[..n]);
        }
        let o = create(proto);
        assert_eq!(live(o), LEARNED_WIDTH_MAX, "the widest descendant, capped");
    }
}
