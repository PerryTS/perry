use super::*;

fn options_with_value(name: &str, value: f64) -> f64 {
    let options = js_object_alloc(0, 1);
    set_field(options, name, value);
    boxed_ptr(options)
}

fn options_with_bool(name: &str, value: bool) -> f64 {
    options_with_value(
        name,
        f64::from_bits(if value {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        }),
    )
}

fn caught_error_code(f: impl FnOnce() -> f64) -> &'static str {
    let error = catch_js(f).expect_err("operation should throw");
    let error = raw_ptr_from_value(error) as *mut crate::error::ErrorHeader;
    crate::node_submodules::error_code_for_message(crate::error::js_error_get_message(error))
        .expect("error should have a registered Node code")
}

extern "C" fn allocating_true_option(_closure: *const ClosureHeader) -> f64 {
    let _ = js_object_alloc(0, 0);
    f64::from_bits(crate::value::TAG_TRUE)
}

#[test]
fn mock_method_accepts_options_in_the_implementation_slot() {
    let options = options_with_bool("getter", true);
    let (implementation, normalized_options) =
        normalize_mock_method_args(options, undefined_value());

    assert!(is_undefined_value(implementation));
    assert_eq!(normalized_options.to_bits(), options.to_bits());
    let parsed = parse_mock_method_options(normalized_options, false, false);
    assert!(parsed.getter);
    assert!(!parsed.setter);
}

#[test]
fn mock_getter_and_setter_defaults_preserve_the_accessor_kind() {
    let getter = parse_mock_method_options(undefined_value(), true, false);
    assert!(getter.getter);
    assert!(!getter.setter);

    let setter = parse_mock_method_options(undefined_value(), false, true);
    assert!(!setter.getter);
    assert!(setter.setter);
}

#[test]
fn mock_method_options_survive_an_allocating_getter() {
    let options = js_object_alloc(0, 2);
    install_accessor_mock(
        boxed_ptr(options),
        "getter",
        crate::object::AccessorDescriptor {
            get: closure_value(allocating_true_option as *const u8, 0).to_bits(),
            set: 0,
        },
    );
    set_field(options, "setter", f64::from_bits(crate::value::TAG_FALSE));

    let parsed = parse_mock_method_options(boxed_ptr(options), false, false);
    assert!(parsed.getter);
    assert!(!parsed.setter);
}

#[test]
fn mock_accessor_options_reject_invalid_flags() {
    let getter_false = options_with_bool("getter", false);
    assert_eq!(
        caught_error_code(|| {
            let options = parse_mock_method_options(getter_false, true, false);
            validate_mock_accessor_options(options, "getter");
            undefined_value()
        }),
        "ERR_INVALID_ARG_VALUE"
    );

    let setter_false = options_with_bool("setter", false);
    assert_eq!(
        caught_error_code(|| {
            let options = parse_mock_method_options(setter_false, false, true);
            validate_mock_accessor_options(options, "setter");
            undefined_value()
        }),
        "ERR_INVALID_ARG_VALUE"
    );

    let both = js_object_alloc(0, 2);
    set_field(both, "getter", f64::from_bits(crate::value::TAG_TRUE));
    set_field(both, "setter", f64::from_bits(crate::value::TAG_TRUE));
    assert_eq!(
        caught_error_code(|| {
            let options = parse_mock_method_options(boxed_ptr(both), false, false);
            validate_mock_accessor_options(options, "method");
            undefined_value()
        }),
        "ERR_INVALID_ARG_VALUE"
    );

    let non_boolean = options_with_value("getter", 1.0);
    assert_eq!(
        caught_error_code(|| {
            let _ = parse_mock_method_options(non_boolean, false, false);
            undefined_value()
        }),
        "ERR_INVALID_ARG_TYPE"
    );
}
