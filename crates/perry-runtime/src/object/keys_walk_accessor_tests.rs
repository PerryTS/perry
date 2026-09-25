//! #10724: runtime walks over an object's keys array read its raw dense slots,
//! never the JS-facing element accessor.
//!
//! `js_array_get_f64` is the accessor compiled code reaches for an `arr[i]` it
//! cannot prove is a plain array: a lazy-array strip, Map / Set / typed-array /
//! subclass receiver arms, `clean_arr_ptr`, the descriptor gate and the hole →
//! prototype-chain fallback, on every call. A keys array needs none of it — it
//! is an internal, dense, string-only array — yet two walks here paid it per
//! key: `transition_edge_places_key` (3.6 M calls on a natively compiled
//! `tsc --noEmit`) and `class_object_own_field_bytes` (2.8 M). Together with the
//! vtable guard's scan (`native_call_method/vtable_guard_scan_tests.rs`) they
//! were ~96% of that accessor's calls in #10724's workload.
//!
//! Counted, not timed: `test_element_accessor_calls` is the entry counter on
//! `js_array_get_f64` itself, so putting either walk back on `js_array_get`
//! turns its "no accessor call" assertion red. Each test also pins the answer.

use super::*;

fn class_instance(class_id: u32, keys: &[&str]) -> *mut ObjectHeader {
    let packed = keys.join("\0");
    js_object_alloc_class_with_keys(
        class_id,
        0,
        keys.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    )
}

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

#[test]
fn transition_edge_check_reads_one_raw_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let obj = class_instance(0x7A26, &["alpha", "beta", "gammaDeltaEpsilon"]);
    let keys = unsafe { object_keys(obj).arr() } as usize;
    assert!(
        keys != 0,
        "test premise: the class instance publishes a keys array"
    );

    let before = crate::array::test_element_accessor_calls();
    assert!(transition_edge_places_key(keys, 0, key("alpha")));
    assert!(transition_edge_places_key(keys, 1, key("beta")));
    assert!(transition_edge_places_key(
        keys,
        2,
        key("gammaDeltaEpsilon")
    ));
    // Wrong key for the slot, and a slot past the list: both reject.
    assert!(!transition_edge_places_key(keys, 1, key("alpha")));
    assert!(!transition_edge_places_key(keys, 3, key("alpha")));
    assert_eq!(
        crate::array::test_element_accessor_calls(),
        before,
        "a transition-edge check is one slot compare, not a `js_array_get_f64` call"
    );
}

#[test]
fn class_object_own_field_scan_reads_raw_slots() {
    let _lock = crate::gc::global_side_table_test_lock();
    let names = ["a", "bb", "__perry_parent_class", "tailFieldWithALongName"];
    let obj = class_instance(0x7A27, &names);
    for (i, v) in [10.0f64, 20.0, 30.0, 40.0].iter().enumerate() {
        js_object_set_field(obj, i as u32, JSValue::from_bits(v.to_bits()));
    }

    let own = |want: &str| class_registry::class_object_own_field_bytes(obj, want.as_bytes());
    let before = crate::array::test_element_accessor_calls();
    assert_eq!(own("a"), Some(10.0));
    assert_eq!(own("bb"), Some(20.0));
    assert_eq!(own("__perry_parent_class"), Some(30.0));
    assert_eq!(own("tailFieldWithALongName"), Some(40.0));
    assert_eq!(own("b"), None, "a prefix of an own key is not that key");
    assert_eq!(own("missing"), None);
    assert_eq!(
        crate::array::test_element_accessor_calls(),
        before,
        "the own-field scan must not reach `js_array_get_f64` once per key"
    );

    // An own slot holding `undefined` still reads as absent.
    js_object_set_field(obj, 1, JSValue::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(own("bb"), None);
}
