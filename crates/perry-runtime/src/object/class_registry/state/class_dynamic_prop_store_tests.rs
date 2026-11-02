
use super::*;

fn stored(class_id: u32, name: &str) -> Option<f64> {
    class_own_static_field_value(class_id, name)
}

/// The in-place update arm must be observationally identical to the
/// insert arm — same table, same value, same key set. This is the shape
/// `Shape.made = Shape.made + 1` produces once per construction.
#[test]
fn repeated_store_updates_in_place_and_stays_readable() {
    let cid = 0x7c01_0001;
    for i in 0..5u32 {
        class_dynamic_prop_root_store(cid, "made", f64::from(i));
        assert_eq!(stored(cid, "made"), Some(f64::from(i)));
    }
    // A second key on the same class still inserts.
    class_dynamic_prop_root_store(cid, "other", 9.0);
    assert_eq!(stored(cid, "other"), Some(9.0));
    assert_eq!(stored(cid, "made"), Some(4.0));
    let mut keys = class_own_enumerable_field_names(cid);
    keys.sort();
    assert_eq!(keys, vec!["made".to_string(), "other".to_string()]);
}

/// `delete C.k` removes the key from the class function object; a later
/// store defines it again.
#[test]
fn store_after_delete_defines_the_key_again() {
    let cid = 0x7c01_0002;
    class_dynamic_prop_root_store(cid, "k", 1.0);
    class_delete_own_dynamic_prop(cid, "k");
    assert_eq!(stored(cid, "k"), None);

    class_dynamic_prop_root_store(cid, "k", 2.0);
    assert_eq!(stored(cid, "k"), Some(2.0));

    class_dynamic_prop_root_store(cid, "k", 3.0);
    assert_eq!(stored(cid, "k"), Some(3.0));
}
