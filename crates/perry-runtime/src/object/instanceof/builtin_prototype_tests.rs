use super::*;

fn builtin(scope: &crate::gc::RuntimeHandleScope, name: &str) -> f64 {
    scope
        .root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
            name.as_ptr(),
            name.len(),
        ))
        .get_nanbox_f64()
}
fn prototype(constructor: f64) -> f64 {
    unsafe {
        crate::value::js_dynamic_object_get_property(constructor, b"prototype".as_ptr().cast(), 9)
    }
}
fn builtin_cases() -> Vec<(&'static str, u32)> {
    let mut cases = vec![
        ("Map", 0xFFFF0022),
        ("Set", 0xFFFF0023),
        ("Date", 0xFFFF0020),
        ("RegExp", 0xFFFF0021),
        ("ArrayBuffer", 0xFFFF0025),
        ("SharedArrayBuffer", 0xFFFF002E),
        ("Buffer", crate::buffer::NODE_BUFFER_CLASS_ID),
        ("Uint8Array", crate::buffer::BUFFER_TYPE_ID),
        ("Error", crate::error::CLASS_ID_ERROR),
        ("TypeError", crate::error::CLASS_ID_TYPE_ERROR),
        ("RangeError", crate::error::CLASS_ID_RANGE_ERROR),
        ("ReferenceError", crate::error::CLASS_ID_REFERENCE_ERROR),
        ("SyntaxError", crate::error::CLASS_ID_SYNTAX_ERROR),
        ("EvalError", crate::error::CLASS_ID_EVAL_ERROR),
        ("URIError", crate::error::CLASS_ID_URI_ERROR),
        ("AggregateError", crate::error::CLASS_ID_AGGREGATE_ERROR),
    ];
    for kind in 0..=crate::typedarray::KIND_FLOAT16 {
        cases.push((
            crate::typedarray::name_for_kind(kind),
            crate::typedarray::class_id_for_kind(kind),
        ));
    }
    cases
}

#[test]
fn ordinary_objects_follow_reserved_builtin_prototype_chains() {
    let mut failures = Vec::new();
    for (name, id) in builtin_cases() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let constructor = scope.root_nanbox_f64(builtin(&scope, name));
        let proto = scope.root_nanbox_f64(prototype(constructor.get_nanbox_f64()));
        assert!(
            crate::value::JSValue::from_bits(proto.get_nanbox_f64().to_bits()).is_pointer(),
            "{name} prototype missing"
        );
        let object = scope.root_nanbox_f64(crate::object::js_object_create(proto.get_nanbox_f64()));
        let grandchild =
            scope.root_nanbox_f64(crate::object::js_object_create(object.get_nanbox_f64()));
        for (label, value) in [("direct", &object), ("indirect", &grandchild)] {
            if js_instanceof(value.get_nanbox_f64(), id).to_bits() != crate::value::TAG_TRUE {
                failures.push(format!("{name}: {label} static"));
            }
            if js_instanceof_dynamic(value.get_nanbox_f64(), constructor.get_nanbox_f64()).to_bits()
                != crate::value::TAG_TRUE
            {
                failures.push(format!("{name}: {label} dynamic"));
            }
        }
        assert_eq!(
            js_instanceof(proto.get_nanbox_f64(), id).to_bits(),
            crate::value::TAG_FALSE,
            "{name}.prototype is not its own instance"
        );
        let unrelated = scope.root_nanbox_f64(crate::object::js_object_create(f64::from_bits(
            crate::value::TAG_NULL,
        )));
        assert_eq!(
            js_instanceof(unrelated.get_nanbox_f64(), id).to_bits(),
            crate::value::TAG_FALSE,
            "unrelated object matched {name}"
        );
        assert_eq!(
            js_instanceof(42.0, id).to_bits(),
            crate::value::TAG_FALSE,
            "number matched {name}"
        );
    }
    assert!(
        failures.is_empty(),
        "prototype-based instanceof failures: {}",
        failures.join(", ")
    );
}

#[test]
fn buffer_prototype_inherits_from_uint8_array() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer = scope.root_nanbox_f64(builtin(&scope, "Buffer"));
    let proto = scope.root_nanbox_f64(prototype(buffer.get_nanbox_f64()));
    assert_eq!(
        js_instanceof(proto.get_nanbox_f64(), crate::buffer::BUFFER_TYPE_ID).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_instanceof(
            proto.get_nanbox_f64(),
            crate::typedarray::CLASS_ID_UINT8_ARRAY
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn callable_values_follow_the_live_function_prototype_for_object_instanceof() {
    let _lock = crate::gc::global_side_table_test_lock();
    extern "C" fn noop(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let object_constructor = scope.root_nanbox_f64(builtin(&scope, "Object"));
    let function_constructor = scope.root_nanbox_f64(builtin(&scope, "Function"));
    let function_prototype =
        scope.root_nanbox_f64(prototype(function_constructor.get_nanbox_f64()));
    let original_parent = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
        function_prototype.get_nanbox_f64(),
    ));
    let ordinary = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(noop, 0), 0) as i64,
    ));
    let arrow_info = crate::fn_info!(noop, 0; with_flags(crate::closure::FN_ARROW));
    let arrow = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(arrow_info, 0) as i64,
    ));
    assert!(!crate::closure::closure_is_arrow(
        crate::value::js_nanbox_get_pointer(ordinary.get_nanbox_f64())
            as *const crate::closure::ClosureHeader
    ));
    assert!(crate::closure::closure_is_arrow(
        crate::value::js_nanbox_get_pointer(arrow.get_nanbox_f64())
            as *const crate::closure::ClosureHeader
    ));

    for (kind, callable) in [("ordinary", &ordinary), ("arrow", &arrow)] {
        assert_eq!(
            js_instanceof(callable.get_nanbox_f64(), 0xFFFF0050).to_bits(),
            crate::value::TAG_TRUE,
            "{kind} callable should initially inherit from Object.prototype"
        );
    }

    crate::object::js_object_set_prototype_of(
        function_prototype.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_NULL),
    );
    let null_link_results = [
        js_instanceof(ordinary.get_nanbox_f64(), 0xFFFF0050).to_bits(),
        js_instanceof(arrow.get_nanbox_f64(), 0xFFFF0050).to_bits(),
    ];

    crate::object::js_object_set_prototype_of(
        function_prototype.get_nanbox_f64(),
        original_parent.get_nanbox_f64(),
    );
    let restored_results = [
        js_instanceof(ordinary.get_nanbox_f64(), 0xFFFF0050).to_bits(),
        js_instanceof(arrow.get_nanbox_f64(), 0xFFFF0050).to_bits(),
    ];
    assert_eq!(
        null_link_results,
        [crate::value::TAG_FALSE; 2],
        "callables must observe the live null Function.prototype link"
    );
    assert_eq!(
        restored_results,
        [crate::value::TAG_TRUE; 2],
        "restoring Function.prototype should restore Object membership"
    );
    // Keep the constructor rooted through the operation and make the intended
    // target explicit: the generic static id represents Object.
    assert!(
        crate::object::js_instanceof_dynamic(
            ordinary.get_nanbox_f64(),
            object_constructor.get_nanbox_f64(),
        )
        .to_bits()
            == crate::value::TAG_TRUE
    );

    // The dynamic path must use the constructor value it was given, even if
    // its `.prototype` differs from the intrinsic Object.prototype.
    let custom_constructor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(noop, 0), 0) as i64,
    ));
    let custom_prototype = scope.root_nanbox_f64(crate::object::js_object_create(f64::from_bits(
        crate::value::TAG_NULL,
    )));
    let prototype_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    crate::object::js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(custom_constructor.get_nanbox_f64())
            as *mut ObjectHeader,
        prototype_key,
        custom_prototype.get_nanbox_f64(),
    );
    assert_eq!(
        super::static_dispatch::instanceof_with_constructor(
            ordinary.get_nanbox_f64(),
            0xFFFF0050,
            Some(custom_constructor.get_nanbox_f64()),
        )
        .to_bits(),
        crate::value::TAG_FALSE,
        "a supplied constructor with a distinct prototype should not use global Object"
    );
    assert_eq!(
        js_instanceof(ordinary.get_nanbox_f64(), 0xFFFF0050).to_bits(),
        crate::value::TAG_TRUE,
        "the static Object path should target intrinsic Object.prototype"
    );
    crate::object::js_object_set_prototype_of(
        ordinary.get_nanbox_f64(),
        custom_prototype.get_nanbox_f64(),
    );
    assert_eq!(
        super::static_dispatch::instanceof_with_constructor(
            ordinary.get_nanbox_f64(),
            0xFFFF0050,
            Some(custom_constructor.get_nanbox_f64()),
        )
        .to_bits(),
        crate::value::TAG_TRUE,
        "the actual RHS constructor prototype should be used"
    );
    assert_eq!(
        js_instanceof(ordinary.get_nanbox_f64(), 0xFFFF0050).to_bits(),
        crate::value::TAG_FALSE,
        "the static Object path should still target intrinsic Object.prototype"
    );
}
