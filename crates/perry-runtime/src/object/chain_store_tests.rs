use super::*;

#[test]
fn a_non_string_key_has_no_interned_pointer() {
    unsafe {
        assert!(interned_key_for_store(1.5).is_none());
        assert!(interned_key_for_store(f64::from_bits(crate::value::TAG_UNDEFINED)).is_none());
    }
}

#[test]
fn a_non_object_target_has_no_pre_store_shape() {
    unsafe {
        assert_eq!(pre_store_shape(2.0), 0);
        assert_eq!(pre_store_shape(f64::from_bits(crate::value::TAG_NULL)), 0);
    }
}
