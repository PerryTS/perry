//! Storage of key attributes beside the keys (`key_attrs.rs`).

use super::*;

unsafe fn list_with_entries(entries: &[u8]) -> *mut ArrayHeader {
    let keys = alloc_key_list(entries.len() as u32, true, true);
    let attrs = keys_attrs(keys);
    assert!(!attrs.is_null(), "an attribute list carries its array");
    for (i, &e) in entries.iter().enumerate() {
        let name = format!("ka{i}");
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = crate::JSValue::string_ptr(k);
        let elems = crate::array::array_elements_ptr(keys);
        *elems.add(i) = key.bits();
        (*keys).length = i as u32 + 1;
        attrs_write(attrs, i as u32, e, key);
    }
    keys
}

#[test]
fn default_entries_mean_default_attributes() {
    assert_eq!(
        attr_bits_to_entry(0x07),
        0,
        "writable + enumerable + configurable is the default entry"
    );
    assert_eq!(entry_to_attr_bits(0), 0x07);
    assert_eq!(
        entry_to_attr_bits(attr_bits_to_entry(0x02)),
        0x02,
        "round trip"
    );
    assert!(entry_is_plain_writable_data(0));
    assert!(entry_is_plain_writable_data(ENTRY_NON_ENUMERABLE));
    assert!(!entry_is_plain_writable_data(ENTRY_NON_WRITABLE));
    assert!(!entry_is_plain_writable_data(
        ENTRY_ACCESSOR | ENTRY_HAS_GET
    ));
}

#[test]
fn an_accessor_is_not_a_non_writable_data_key() {
    let s = entry_summary(ENTRY_ACCESSOR | ENTRY_NON_WRITABLE);
    assert_eq!(s & SUMMARY_ACCESSOR, SUMMARY_ACCESSOR);
    assert_eq!(s & SUMMARY_NON_WRITABLE, 0);
}

/// The summary of a PREFIX is exact: a shape names a prefix of a shared
/// backing, and a key past its count must not leak into its summary.
#[test]
fn the_summary_of_a_prefix_ignores_later_positions() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let keys = list_with_entries(&[0, ENTRY_NON_ENUMERABLE, 0, ENTRY_ACCESSOR | ENTRY_HAS_GET]);
        assert_eq!(keys_summary(keys, 0), 0);
        assert_eq!(keys_summary(keys, 1), 0, "first key is default");
        assert_eq!(keys_summary(keys, 2), SUMMARY_NON_ENUMERABLE);
        assert_eq!(keys_summary(keys, 3), SUMMARY_NON_ENUMERABLE);
        assert_eq!(
            keys_summary(keys, 4),
            SUMMARY_NON_ENUMERABLE | SUMMARY_ACCESSOR
        );
        assert_eq!(keys_entry(keys, 1), ENTRY_NON_ENUMERABLE);
        assert_eq!(keys_entry(keys, 9), 0, "past the array: default");
        assert!(!keys_have_entries(keys, 1));
        assert!(keys_have_entries(keys, 2));
    }
}

/// An in-place edit of an owned list recomputes every later cumulative word,
/// so a cleared entry stops contributing.
#[test]
fn an_owned_edit_recomputes_the_summary() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let keys = list_with_entries(&[ENTRY_NON_WRITABLE, 0, 0]);
        assert_eq!(keys_summary(keys, 3), SUMMARY_NON_WRITABLE);
        attrs_set_owned(keys, keys_attrs(keys), 0, 0);
        assert_eq!(
            keys_summary(keys, 3),
            0,
            "the cleared entry must not linger"
        );
        attrs_set_owned(keys, keys_attrs(keys), 2, ENTRY_NON_CONFIGURABLE);
        assert_eq!(keys_summary(keys, 2), 0);
        assert_eq!(keys_summary(keys, 3), SUMMARY_NON_CONFIGURABLE);
    }
}

/// A list without attributes answers default everywhere and is laid out
/// exactly as before (no front reserve).
#[test]
fn an_attribute_free_list_is_unchanged() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let keys = alloc_key_list(4, false, false);
        assert!(keys_attrs(keys).is_null());
        assert_eq!(crate::array::array_front_offset(keys), 0);
        let with = alloc_key_list(4, false, true);
        assert_eq!(
            crate::array::array_front_offset(with),
            KEYS_ATTRS_FRONT_SLOTS
        );
    }
}

/// The prefix filters: a key with no entry in the prefix is proved absent,
/// and a key past the prefix does not leak into it. Sabotage: dropping the
/// Bloom update in `Word::after` makes the first assertion fail.
#[test]
fn the_prefix_filters_prove_absence() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let keys = list_with_entries(&[0, ENTRY_NON_ENUMERABLE, 0, ENTRY_ACCESSOR | ENTRY_HAS_GET]);
        assert!(
            keys_may_carry(keys, 4, b"ka1", false),
            "ka1 carries an entry"
        );
        assert!(keys_may_carry(keys, 4, b"ka3", true), "ka3 is an accessor");
        // Absence is only PROVED where the filter has no bit; a collision may
        // answer "maybe", so assert the exact negatives on a single-entry prefix.
        let single = list_with_entries(&[ENTRY_NON_WRITABLE]);
        assert!(!keys_may_carry(single, 1, b"ka0", true), "not an accessor");
        assert!(
            !keys_may_carry(keys, 1, b"ka1", false),
            "ka1 is past the prefix"
        );
    }
}

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// A dictionary receiver edits its PRIVATE list in place, and a compacting
/// delete shifts the attributes with the keys. Sabotage: skipping the shift in
/// `owned_note_remove` leaves `k3`'s attributes at the old position, so `k4`
/// reads non-writable and `k3` writable.
#[test]
fn a_dictionary_delete_shifts_the_attributes_with_the_keys() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 8);
        for i in 0..6 {
            crate::object::js_object_set_field_by_name(obj, key(&format!("dk{i}")), i as f64);
        }
        assert!(
            crate::object::dictionary::latch_object_to_dictionary(obj),
            "premise: the receiver latches to dictionary mode"
        );
        crate::object::set_property_attrs(
            obj as usize,
            "dk3".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert!(
            crate::object::dictionary::is_dictionary(obj),
            "premise: still a dictionary"
        );
        crate::object::js_object_delete_field(obj, key("dk1"));
        let attrs = |k: &str| crate::object::get_property_attrs(obj as usize, k);
        assert!(
            attrs("dk3").is_some_and(|a| !a.writable()),
            "dk3 keeps its attributes across the compacting delete"
        );
        assert!(
            attrs("dk4").is_none(),
            "dk4 must not inherit dk3's old position"
        );
        assert!(attrs("dk2").is_none());
    }
}

/// An attribute installed on a key the object does not have yet CLAIMS the
/// key, with its attributes: an attribute always has a key. Sabotage: skipping
/// the claim in `apply_edits` loses the attribute.
#[test]
fn an_attribute_on_an_absent_key_claims_the_key() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 2);
        crate::object::js_object_set_field_by_name(obj, key("present"), 1.0);
        crate::object::set_property_attrs(
            obj as usize,
            "claimed".to_string(),
            crate::object::PropertyAttrs::new(false, false, true),
        );
        let keys = crate::object::object_keys(obj);
        let pos = crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), b"claimed")
            .expect("the attribute claimed its key");
        assert_eq!(
            keys_entry(keys.arr(), pos),
            ENTRY_NON_WRITABLE | ENTRY_NON_ENUMERABLE
        );
        let attrs = crate::object::get_property_attrs(obj as usize, "claimed").unwrap();
        assert!(!attrs.writable() && !attrs.enumerable() && attrs.configurable());
    }
}
