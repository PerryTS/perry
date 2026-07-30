use super::*;

extern "C" fn original_two_args(_closure: *const ClosureHeader, _first: f64, _second: f64) -> f64 {
    undefined_value()
}

#[test]
fn mock_function_preserves_original_name_length_and_descriptors() {
    let original = make_closure(original_two_args as *const u8, 2, 0);
    crate::object::set_bound_native_closure_name(original, "original");
    let original = boxed_ptr(original);

    let mock = create_mock_function(original, original, MockRestoreTarget::None);
    let mock_ptr = raw_ptr_from_value(mock);

    let name = crate::closure::closure_get_dynamic_prop(mock_ptr, "name");
    assert_eq!(value_to_string(name).as_deref(), Some("original"));
    assert_eq!(crate::object::builtin_closure_length(mock_ptr), Some(2));
    for property in ["name", "length"] {
        let attrs = crate::object::get_property_attrs(mock_ptr, property)
            .unwrap_or_else(|| panic!("{property} descriptor attrs should be installed"));
        assert!(!attrs.writable());
        assert!(!attrs.enumerable());
        assert!(attrs.configurable());
    }
}
