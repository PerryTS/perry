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
