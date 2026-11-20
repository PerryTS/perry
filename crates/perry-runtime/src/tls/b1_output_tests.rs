use super::*;
use perry_base64::Engine;

#[test]
fn secure_context_factory_and_constructor_share_registered_prototype() {
    crate::gc::gc_init();
    crate::object::static_shapes::js_shape_run_static_seed();
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        "tls",
        "SecureContext",
    ));
    let property = |object, name: &str| unsafe {
        crate::value::js_get_property(object, name.as_ptr() as i64, name.len() as i64)
    };
    let prototype = scope.root_nanbox_f64(property(constructor.get_nanbox_f64(), "prototype"));
    assert_eq!(
        property(prototype.get_nanbox_f64(), "constructor").to_bits(),
        constructor.get_nanbox_f64().to_bits()
    );
    for create in [
        js_tls_create_secure_context as extern "C" fn(f64) -> f64,
        js_tls_secure_context_new,
    ] {
        let context = scope.root_nanbox_f64(create(f64::from_bits(TAG_UNDEFINED)));
        assert!(is_secure_context_instance(context.get_nanbox_f64()));
        assert_eq!(
            crate::object::js_object_get_prototype_of(context.get_nanbox_f64()).to_bits(),
            prototype.get_nanbox_f64().to_bits(),
            "both creation entries must bind the registered instance prototype"
        );
        assert_eq!(
            crate::object::js_instanceof_dynamic(
                context.get_nanbox_f64(),
                constructor.get_nanbox_f64()
            )
            .to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            property(context.get_nanbox_f64(), "constructor").to_bits(),
            constructor.get_nanbox_f64().to_bits()
        );
        let inner = scope.root_nanbox_f64(property(context.get_nanbox_f64(), "context"));
        assert!(object_ptr(inner.get_nanbox_f64()).is_some());
        let keys = scope.root_raw_mut_ptr(crate::object::js_object_keys_value(
            context.get_nanbox_f64(),
        ));
        assert_eq!(
            keys.with_mut_ptr(|keys| crate::array::js_array_length(keys)),
            1
        );
        let first = keys.with_mut_ptr(|keys| crate::array::js_array_get_f64(keys, 0));
        assert_eq!(value_to_string(first).as_deref(), Some("context"));
        crate::gc::gc_collect_minor();
        assert_eq!(
            crate::object::js_object_get_prototype_of(context.get_nanbox_f64()).to_bits(),
            prototype.get_nanbox_f64().to_bits(),
            "the instance-prototype edge must survive collection"
        );
    }
}

#[test]
fn certificate_raw_output_preserves_der() {
    crate::gc::gc_init();
    let encoded: String = roots::bundled_certificates()[0]
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = perry_base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let cert = unsafe { tls_legacy_certificate_object(&der, false) };
    let raw = get_field(
        JSValue::from_bits(cert.to_bits())
            .as_pointer::<ObjectHeader>()
            .cast_mut(),
        "raw",
    );
    let ptr = JSValue::from_bits(raw.to_bits()).as_pointer::<crate::buffer::BufferHeader>();
    let actual =
        crate::buffer::bytes::ReadLease::new(crate::value::js_nanbox_pointer(ptr as i64)).unwrap();
    assert_eq!(&actual[..], der);
}
