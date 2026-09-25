//! #10724: [`class_vtable_fast_guard`]'s own-key shadowing scan reads the
//! receiver's keys through the raw dense slots, never through the JS-facing
//! element accessor.
//!
//! That scan runs on every dynamic method call on a class instance, once per
//! own key. On a natively compiled `tsc --noEmit` it was 26.4 M of the 33.7 M
//! `js_array_get_f64` calls, which is what made that accessor the top runtime
//! symbol in #10724's profile. Counted, not timed: `test_element_accessor_calls`
//! is the entry counter on `js_array_get_f64` itself, so reverting the scan to
//! `js_array_get` turns every "no accessor call" assertion below red.
//!
//! The other half is that the answer is unchanged: an own key equal to the
//! method name still declines the fast path wherever it sits in the keys
//! array, for short and long names, below and above the shape-index
//! threshold. The byte comparison itself (`js_string_key_matches_bytes`) is the
//! one the scan always used.

use super::*;

fn class_instance(class_id: u32, keys: &[&str]) -> f64 {
    let packed = keys.join("\0");
    let obj = crate::object::js_object_alloc_class_with_keys(
        class_id,
        0,
        keys.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits())
}

fn guard(receiver: f64, method: &str) -> Option<(usize, u32)> {
    unsafe { class_vtable_fast_guard(receiver, method.as_bytes()) }
}

#[test]
fn own_key_scan_reads_raw_slots_not_the_element_accessor() {
    let _lock = crate::gc::global_side_table_test_lock();
    let keys = [
        "kind",
        "pos",
        "end",
        "flags",
        "parent",
        "transformFlagsWideName",
    ];
    let receiver = class_instance(0x7A24, &keys);

    let before = crate::array::test_element_accessor_calls();
    let hit = guard(receiver, "getSourceFile");
    assert_eq!(
        crate::array::test_element_accessor_calls(),
        before,
        "the shadowing scan must not reach `js_array_get_f64` once per own key"
    );
    let (_, class_id) =
        hit.expect("no own key is named `getSourceFile`, so the guard must admit the receiver");
    assert_eq!(class_id, 0x7A24);

    // Shadowing still declines, from the first slot to the last.
    for own in keys {
        let before = crate::array::test_element_accessor_calls();
        assert!(
            guard(receiver, own).is_none(),
            "an own key `{own}` shadows a vtable method of the same name"
        );
        assert_eq!(crate::array::test_element_accessor_calls(), before);
    }
    // A prefix / extension of an own key is not a match.
    assert!(guard(receiver, "kin").is_some());
    assert!(guard(receiver, "kinds").is_some());
}

#[test]
fn wide_receiver_scan_is_accessor_free_and_still_sees_every_key() {
    let _lock = crate::gc::global_side_table_test_lock();
    // Past `KEYS_INDEX_THRESHOLD` (32), so a wide class instance is covered too.
    let names: Vec<String> = (0..48).map(|i| format!("field_{i}")).collect();
    let keys: Vec<&str> = names.iter().map(String::as_str).collect();
    let receiver = class_instance(0x7A25, &keys);

    let before = crate::array::test_element_accessor_calls();
    assert!(guard(receiver, "method").is_some());
    assert!(guard(receiver, "field_0").is_none());
    assert!(guard(receiver, "field_31").is_none());
    assert!(guard(receiver, "field_47").is_none());
    assert!(guard(receiver, "field_48").is_some());
    assert_eq!(crate::array::test_element_accessor_calls(), before);
}
