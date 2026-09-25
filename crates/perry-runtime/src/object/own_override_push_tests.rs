//! #11021: `js_array_push_f64_spec_or_own`, the push every `Expr::ArrayPush`
//! slow arm calls — its absence proof, its precise probe, and both exits.

use super::own_override::{array_owning_push_for_test, js_array_push_f64_spec_or_own};
use crate::array::{js_array_alloc, js_array_length, js_array_push_f64, ArrayHeader};

fn pushed(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for &v in values {
        arr = js_array_push_f64(arr, v);
    }
    arr
}

unsafe fn set_named(arr: *mut ArrayHeader, name: &str, value: f64) -> *mut ArrayHeader {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::array::js_array_set_string_key(arr, key, value)
}

fn reserved(arr: *const ArrayHeader) -> u16 {
    unsafe { crate::array::array_object_flags_resolved(arr) }
}

/// The builtin exit: `(new head, own = 0)`.
unsafe fn push_or_own(arr: *mut ArrayHeader, value: f64) -> (u64, u32) {
    let mut own = u32::MAX;
    let bits = js_array_push_f64_spec_or_own(arr, value, &mut own);
    (bits, own)
}

extern "C" fn own_push_fixture(_closure: *const crate::closure::ClosureHeader, x: f64) -> f64 {
    x * 10.0
}

#[test]
fn a_plain_array_takes_the_builtin_exit() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = pushed(&[1.0]);
        assert_eq!(reserved(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS, 0);
        assert!(array_owning_push_for_test(arr).is_none());
        let (bits, own) = push_or_own(arr, 2.0);
        assert_eq!(own, 0, "a plain array must take the builtin exit");
        assert_eq!(js_array_length(bits as *const ArrayHeader), 2);
    }
}

/// The flag is "some named property", not "an own push": an unrelated one
/// arms it and must still push.
#[test]
fn an_unrelated_named_property_arms_the_flag_and_still_pushes() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = set_named(pushed(&[1.0]), "foo", 1.0);
        assert_ne!(
            reserved(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
            0,
            "the absence proof: installing a named property must arm the admission bit"
        );
        assert!(array_owning_push_for_test(arr).is_none());
        let (bits, own) = push_or_own(arr, 2.0);
        assert_eq!(own, 0);
        assert_eq!(js_array_length(bits as *const ArrayHeader), 2);
    }
}

/// Both storages of an array's own named properties arm the bit the inline
/// admission mask tests, and both are found by the probe: a FULL literal
/// (the address-keyed fallback table) and one with room (the pairs reserve).
#[test]
fn an_own_push_is_seen_in_both_named_property_storages() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let full = pushed(&[1.0]);
        let full = set_named(full, "push", 5.0);
        let mut roomy = js_array_alloc(8);
        roomy = js_array_push_f64(roomy, 1.0);
        let roomy = set_named(roomy, "push", 5.0);
        for (label, arr) in [("full", full), ("roomy", roomy)] {
            assert_ne!(
                reserved(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
                0,
                "{label}: an own `push` must arm the admission bit"
            );
            assert!(
                array_owning_push_for_test(arr).is_some(),
                "{label}: the probe must find the own `push`"
            );
        }
    }
}

/// A pre-growth alias is a forwarding stub whose own header predates the
/// install: the probe must answer from the live head.
#[test]
fn a_forwarded_alias_answers_from_the_live_head() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let alias = pushed(&[1.0]);
        let mut live = alias;
        while live == alias {
            live = js_array_push_f64(live, 0.0);
        }
        set_named(live, "push", 5.0);
        assert!(
            array_owning_push_for_test(alias).is_some(),
            "an own `push` installed after growth must be seen through the old head"
        );
    }
}

/// The own exit: the METHOD's return, `own = 1`, nothing appended.
#[test]
fn an_own_user_method_takes_the_own_exit_with_its_return_value() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let method = crate::closure::js_closure_alloc(own_push_fixture as *const u8, 0);
        crate::closure::js_register_closure_arity(own_push_fixture as *const u8, 1);
        let arr = set_named(
            pushed(&[1.0]),
            "push",
            crate::value::js_nanbox_pointer(method as i64),
        );
        let (bits, own) = push_or_own(arr, 7.0);
        assert_eq!(own, 1, "an own user `push` must take the own exit");
        assert_eq!(
            f64::from_bits(bits),
            70.0,
            "the value is the method's return"
        );
        assert_eq!(
            js_array_length(arr),
            1,
            "the own method replaced the append"
        );
    }
}

/// `Get` then `Call`: an own value that is not callable throws before
/// anything is appended.
#[test]
fn a_non_callable_own_push_throws_before_appending() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = set_named(pushed(&[1.0]), "push", 5.0);
        let threw = crate::exception::catch_js_throw(|| {
            let _ = push_or_own(arr, 2.0);
        })
        .is_err();
        assert!(threw, "a non-callable own `push` must throw TypeError");
        assert_eq!(js_array_length(arr), 1);
    }
}
