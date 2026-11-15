//! `o[i]` on a GC-headed receiver that is no Array, ordinary object, closure,
//! collection, buffer or typed array (a Date, an Error, a Promise): the
//! ordinary [[Get]] / [[Set]] of the key, never a word of the receiver's cell
//! or of the memory behind it.

use super::{js_dyn_index_get, js_dyn_index_set};

const UNDEFINED: u64 = crate::value::TAG_UNDEFINED;

fn receivers() -> Vec<(&'static str, f64)> {
    vec![
        ("Date", crate::date::js_date_new_from_value(0.0)),
        (
            "Error",
            crate::value::js_nanbox_pointer(crate::error::js_error_new() as i64),
        ),
        (
            "Promise",
            crate::value::js_nanbox_pointer(crate::promise::js_promise_resolved(1.0) as i64),
        ),
    ]
}

#[test]
fn a_fresh_receiver_has_no_indexed_property_at_any_index() {
    for (name, receiver) in receivers() {
        // 0..7 lie inside the cell, 100_000 and i32::MAX far behind it: the
        // old tail loaded `receiver + 8 + 8 * index` for each of them.
        for index in [0.0, 1.0, 2.0, 5.0, 7.0, 100_000.0, f64::from(i32::MAX)] {
            assert_eq!(
                js_dyn_index_get(receiver, index).to_bits(),
                UNDEFINED,
                "{name}[{index}]"
            );
        }
    }
}

#[test]
fn the_polymorphic_reader_answers_undefined_not_nan() {
    for (name, receiver) in receivers() {
        let raw = (receiver.to_bits() & crate::value::POINTER_MASK) as i64;
        for index in [0.0, 1.0, 5.0, -1.0, 1.5, 4_294_967_295.0] {
            assert_eq!(
                crate::object::js_object_get_index_polymorphic(raw, index).to_bits(),
                UNDEFINED,
                "{name}[{index}]"
            );
        }
    }
}

#[test]
fn an_index_store_creates_an_own_property_both_readers_find() {
    for (name, receiver) in receivers() {
        let raw = (receiver.to_bits() & crate::value::POINTER_MASK) as i64;
        js_dyn_index_set(receiver, 1.0, 42.0);
        assert_eq!(js_dyn_index_get(receiver, 1.0), 42.0, "{name}[1]");
        assert_eq!(
            crate::object::js_object_get_index_polymorphic(raw, 1.0),
            42.0,
            "{name}[1] (polymorphic)"
        );
        assert_eq!(
            js_dyn_index_get(receiver, 0.0).to_bits(),
            UNDEFINED,
            "{name}[0]"
        );
        assert_eq!(
            js_dyn_index_get(receiver, 2.0).to_bits(),
            UNDEFINED,
            "{name}[2]"
        );
        // The polymorphic store used to hand the receiver to the array setter,
        // which refused it and dropped the value.
        crate::object::js_object_set_index_polymorphic(raw, 3.0, 7.5);
        assert_eq!(js_dyn_index_get(receiver, 3.0), 7.5, "{name}[3]");
    }
}

#[test]
fn an_array_keeps_its_element_read() {
    let mut array = crate::array::js_array_alloc(2);
    array = crate::array::js_array_push_f64(array, 10.0);
    array = crate::array::js_array_push_f64(array, 20.0);
    let receiver = crate::value::js_nanbox_pointer(array as i64);
    assert_eq!(js_dyn_index_get(receiver, 0.0), 10.0);
    assert_eq!(js_dyn_index_get(receiver, 1.0), 20.0);
    assert_eq!(js_dyn_index_get(receiver, 2.0).to_bits(), UNDEFINED);
    assert_eq!(js_dyn_index_get(receiver, 100_000.0).to_bits(), UNDEFINED);
}
