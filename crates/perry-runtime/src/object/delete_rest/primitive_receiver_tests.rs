//! `delete` on a primitive receiver: ToObject's wrapper answers. A Symbol owns
//! nothing (it used to reach the object delete path as an `ObjectHeader` and
//! abort in `overflow::ensure`); a String owns `length` and its index keys,
//! all non-configurable.

use super::*;
use crate::value::JSValue;

fn heap_str(s: &str) -> f64 {
    let h = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::js_nanbox_string(h as i64).to_bits())
}

fn sso(s: &str) -> f64 {
    f64::from_bits(
        JSValue::try_short_string(s.as_bytes())
            .expect("short")
            .bits(),
    )
}

#[test]
fn a_symbol_receiver_owns_no_property() {
    let sym = unsafe { crate::symbol::js_symbol_new_empty() };
    let other = unsafe { crate::symbol::js_symbol_new_empty() };
    let name = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
    assert_eq!(js_object_delete_field_value(sym, name), 1);
    for key in [
        heap_str("x"),
        heap_str("0"),
        heap_str("length"),
        0.0,
        1.5,
        other,
        sym,
    ] {
        assert_eq!(js_object_delete_dynamic_value(sym, key), 1);
    }
}

#[test]
fn a_string_receiver_owns_length_and_its_indices() {
    for recv in [sso("abc"), heap_str("abc")] {
        // Own and non-configurable: `false` (TypeError in strict code).
        for key in [
            heap_str("length"),
            sso("1"),
            sso("0"),
            heap_str("2"),
            1.0,
            -0.0,
        ] {
            assert_eq!(
                js_object_delete_dynamic_value(recv, key),
                0,
                "{:#x}",
                key.to_bits()
            );
        }
        // Not own: `true`.
        for key in [sso("3"), sso("01"), sso("x"), sso("1.5"), 3.0, 1.5, -1.0] {
            assert_eq!(
                js_object_delete_dynamic_value(recv, key),
                1,
                "{:#x}",
                key.to_bits()
            );
        }
        let length = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let one = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
        let far = crate::string::js_string_from_bytes(b"9".as_ptr(), 1);
        assert_eq!(js_object_delete_field_value(recv, length), 0);
        assert_eq!(js_object_delete_field_value(recv, one), 0);
        assert_eq!(js_object_delete_field_value(recv, far), 1);
    }
    // The empty string owns only `length`.
    assert_eq!(js_object_delete_dynamic_value(sso(""), 0.0), 1);
    assert_eq!(
        js_object_delete_dynamic_value(sso(""), heap_str("length")),
        0
    );
}

#[test]
fn other_primitives_own_nothing() {
    let key = heap_str("length");
    for recv in [5.0, f64::from_bits(crate::value::TAG_TRUE)] {
        assert_eq!(js_object_delete_dynamic_value(recv, key), 1);
        assert_eq!(js_object_delete_dynamic_value(recv, 0.0), 1);
    }
}
