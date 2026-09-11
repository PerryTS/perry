//! Regression coverage for #10059's 65,536-own-key cutoff.

use super::*;

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn assert_membership(obj: *mut ObjectHeader, name: &str, expected: bool) {
    let probe = key(name);
    assert_eq!(
        own_key_present(obj, probe),
        expected,
        "own_key_present({name:?})"
    );
    assert_eq!(
        own_key_present_via_index(obj, probe),
        Some(expected),
        "indexed membership({name:?})"
    );
}

/// Before #10059, both membership helpers returned false as soon as the
/// object's key count became 65,537. Build through the exact boundary once and
/// assert the last key at each size, a present-undefined key, short and Unicode
/// keys, an absent key, and index invalidation after delete/reinsert.
#[test]
fn own_key_membership_crosses_65536_without_a_cutoff() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(js_object_alloc(0, 0));

        for (name, value) in [("s", 1.0), ("κey", 2.0)] {
            let property = key(name);
            js_object_set_field_by_name(object.get_raw_mut_ptr(), property, value);
        }

        for i in 0..65_535u32 {
            let name = format!("field_{i}");
            let property = key(&name);
            let value = if i == 42 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from(i)
            };
            js_object_set_field_by_name(object.get_raw_mut_ptr(), property, value);

            let count = i + 3; // two leading keys plus this insertion
            if matches!(count, 65_535 | 65_536 | 65_537) {
                assert_membership(object.get_raw_mut_ptr(), &name, true);
            }
        }

        let obj = object.get_raw_mut_ptr();
        for present in ["s", "κey", "field_0", "field_42", "field_65534"] {
            assert_membership(obj, present, true);
        }
        assert_membership(obj, "missing_field", false);

        let victim = key("field_32768");
        assert_eq!(js_object_delete_field(obj, victim), 1);
        assert_membership(object.get_raw_mut_ptr(), "field_32768", false);
        assert_membership(object.get_raw_mut_ptr(), "field_32769", true);

        let victim = key("field_32768");
        js_object_set_field_by_name(object.get_raw_mut_ptr(), victim, 32768.0);
        assert_membership(object.get_raw_mut_ptr(), "field_32768", true);
        assert_membership(object.get_raw_mut_ptr(), "missing_field", false);
    }
}
