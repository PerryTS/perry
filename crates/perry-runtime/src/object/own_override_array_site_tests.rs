//! The Array-site predicate (`js_array_method_needs_get`), the checked `pop`
//! entry and the dispatcher's array-like gate: a by-name Array method runs
//! the builtin only on a receiver that resolves the name to it.

use super::builtin_proto_entries::js_array_m_pop0;
use super::own_override::{js_array_method_needs_get, js_receiver_may_own_named_method};
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

fn key(name: &[u8]) -> f64 {
    f64::from_bits(crate::JSValue::string_ptr(crate::string::canonical_key(name)).bits())
}

fn boxed<T>(ptr: *mut T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

extern "C" fn own_pop_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    77.0
}

unsafe fn array_like(length: f64) -> *mut crate::object::ObjectHeader {
    let obj = crate::object::js_object_alloc(0, 0);
    let len = crate::string::js_string_from_bytes("length".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(obj, len, length);
    obj
}

/// An ordinary object behind an Array annotation: nothing on the cell says
/// which method `Get` selects, so the Array-site predicate asks for the real
/// `Get`, while the kind predicate (a proven Map, Set or Date site) still
/// answers from the global install flag.
#[test]
fn an_object_receiver_needs_the_get_at_an_array_site() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = array_like(0.0);
        assert_eq!(js_array_method_needs_get(boxed(obj), key(b"indexOf")), 1);
        let arr = pushed(&[1.0, 2.0]);
        assert_eq!(
            js_array_method_needs_get(boxed(arr), key(b"indexOf")),
            js_receiver_may_own_named_method(boxed(arr), key(b"indexOf")),
            "an Array is answered by the kind predicate, unchanged"
        );
        assert_eq!(js_array_method_needs_get(boxed(arr), key(b"indexOf")), 0);
        // A Number has no `indexOf` (node: TypeError): the real `Get`.
        assert_eq!(js_array_method_needs_get(1.5, key(b"indexOf")), 1);
    }
}

/// An unrelated own property leaves the builtin; an own property of the name
/// asks for the `Get`.
#[test]
fn an_array_answers_for_the_name_it_owns() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = set_named(pushed(&[1.0]), "tag", 1.0);
        assert_eq!(js_array_method_needs_get(boxed(arr), key(b"pop")), 0);
        let arr = set_named(arr, "pop", 5.0);
        assert_eq!(js_array_method_needs_get(boxed(arr), key(b"pop")), 1);
    }
}

/// `Expr::ArrayPop` and the inline pop's slow arm call the checked entry: a
/// plain Array pops, an Array that owns `pop` runs it and keeps its elements.
#[test]
fn the_checked_pop_entry_runs_an_own_pop() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = pushed(&[1.0, 2.0]);
        assert_eq!(js_array_m_pop0(boxed(arr)), 2.0);
        assert_eq!(js_array_length(arr), 1);
        let empty = pushed(&[]);
        assert_eq!(
            js_array_m_pop0(boxed(empty)).to_bits(),
            crate::value::TAG_UNDEFINED
        );

        let method = crate::closure::js_closure_alloc(
            crate::fn_info!(own_pop_fixture, 0; with_declared(0)),
            0,
        );
        let owner = set_named(pushed(&[1.0, 2.0]), "pop", boxed(method));
        assert_eq!(js_array_m_pop0(boxed(owner)), 77.0);
        assert_eq!(js_array_length(owner), 2, "the builtin did not run");
    }
}

/// An object with its own `pop` behind the annotation runs it through the
/// checked entry (the admission no longer takes an object for an Array).
#[test]
fn the_checked_pop_entry_runs_an_objects_pop() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = array_like(2.0);
        let method = crate::closure::js_closure_alloc(
            crate::fn_info!(own_pop_fixture, 0; with_declared(0)),
            0,
        );
        let pop = crate::string::js_string_from_bytes("pop".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(obj, pop, boxed(method));
        assert_eq!(js_array_m_pop0(boxed(obj)), 77.0);
    }
}

/// The dispatcher's array-like engine serves a plain object only when the
/// object reaches `Array.prototype`'s method: `{ length: 0 }.push(7)` is left
/// to the ordinary dispatch (TypeError), and nothing is appended.
#[test]
fn an_array_like_without_the_method_is_not_served_the_array_algorithm() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = array_like(0.0);
        let args = [7.0];
        for name in [
            "push", "pop", "shift", "unshift", "reverse", "splice", "sort", "concat",
        ] {
            assert!(
                crate::array::try_object_arraylike_mutator(boxed(obj), name, args.as_ptr(), 1)
                    .is_none(),
                "{name} on an object without it"
            );
        }
        let len = crate::string::js_string_from_bytes("length".as_ptr(), 6);
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(obj, len),
            0.0,
            "nothing was appended"
        );
    }
}

/// A cell of another kind behind an Array annotation (a Map, a closure)
/// reads its methods from its own prototype: the real `Get`.
#[test]
fn a_map_or_closure_receiver_needs_the_get_at_an_array_site() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let map = crate::map::js_map_alloc(0);
        assert_eq!(js_array_method_needs_get(boxed(map), key(b"at")), 1);
        let f = crate::closure::js_closure_alloc(
            crate::fn_info!(own_pop_fixture, 0; with_declared(0)),
            0,
        );
        assert_eq!(js_array_method_needs_get(boxed(f), key(b"at")), 1);
    }
}

/// A String (heap or inline), a boolean or a BigInt behind an Array
/// annotation is no address: the site performs the real `Get` (a String has
/// its own `at`, `indexOf`, `slice`, ...), where the builtin read an inline
/// string's BYTES as an Array header.
#[test]
fn a_string_boolean_or_bigint_receiver_needs_the_get() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let short = f64::from_bits(0x7FF9_0200_0000_6261); // "ab", inline
        let heap = crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
            "abcdefghijklmnopqrstuvwxyz0123456789".as_ptr(),
            36,
        ) as i64);
        let bigint = f64::from_bits(0x7FFA_0000_0000_0000 | 0x1000);
        for recv in [
            short,
            heap,
            bigint,
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_FALSE),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        ] {
            assert_eq!(
                js_array_method_needs_get(recv, key(b"at")),
                1,
                "{:#018x} must take the real Get",
                recv.to_bits()
            );
        }
    }
}
