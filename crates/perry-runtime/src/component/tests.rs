use super::*;
use serde_json::json;

fn object(scope: &RuntimeHandleScope) -> crate::gc::RuntimeHandle<'_> {
    scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ))
}

#[test]
fn structured_values_preserve_bigint_nested_options_and_typed_bytes() {
    let scope = RuntimeHandleScope::new();
    let owner = object(&scope);
    let world = json!({"types":{}});
    let ty = json!({"kind":"record","value":[
        {"name":"bytes","ty":{"kind":"list","value":{"kind":"u8"}}},
        {"name":"integer","ty":{"kind":"u64"}},
        {"name":"option","ty":{"kind":"option","value":{"kind":"option","value":{"kind":"string"}}}},
        {"name":"text","ty":{"kind":"string"}}
    ]});
    let input = json!({"bytes":[0,1,127,255],"integer":{"$bigint":"18446744073709551615"},
        "option":{"tag":"some","value":{"tag":"none"}},"text":"héllo 🌍"});
    let decoded = scope.root_nanbox_f64(value::decode(&input, &ty, &world, owner.get_nanbox_f64()));
    assert_eq!(value::encode(decoded.get_nanbox_f64()), input);
    let bytes = field(decoded.get_nanbox_f64(), b"bytes");
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(
            JSValue::from_bits(bytes.to_bits()).as_pointer::<u8>() as usize
        ),
        Some(crate::typedarray::KIND_UINT8)
    );
    for tag in ["nan", "infinity", "-infinity", "-0"] {
        let input = json!({"$float":tag});
        let decoded = value::decode(
            &input,
            &json!({"kind":"f64"}),
            &world,
            owner.get_nanbox_f64(),
        );
        assert_eq!(value::encode(decoded), input);
    }
}

#[test]
fn inline_short_strings_cross_the_component_boundary_in_nested_options() {
    let scope = RuntimeHandleScope::new();
    let options = object(&scope);
    let wasi = object(&scope);
    let args = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(4));
    for (index, text) in ["", "0", "23", "hello"].iter().enumerate() {
        let value = JSValue::try_short_string(text.as_bytes()).unwrap();
        assert!(value.is_short_string());
        assert_eq!(value::encode(f64::from_bits(value.bits())), json!(text));
        crate::array::js_array_set_f64(
            args.get_raw_mut_ptr(),
            index as u32,
            f64::from_bits(value.bits()),
        );
    }
    set_field(
        wasi.get_nanbox_f64(),
        b"args",
        crate::value::js_nanbox_pointer(args.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64),
    );
    set_field(options.get_nanbox_f64(), b"wasi", wasi.get_nanbox_f64());
    assert_eq!(
        value::encode(options.get_nanbox_f64()),
        json!({"wasi":{"args":["", "0", "23", "hello"]}})
    );
}

extern "C-unwind" fn throwing_getter(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    crate::exception::js_throw(77.0)
}

#[test]
fn a_throwing_record_getter_preserves_the_exception_and_next_encoding_works() {
    let scope = RuntimeHandleScope::new();
    let record = object(&scope);
    set_field(record.get_nanbox_f64(), b"first", 1.0);
    let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(unwind throwing_getter, 0), 0) as i64,
    ));
    let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(b"throw".as_ptr(), 5) as i64,
    ));
    crate::object::js_object_define_getter(
        record.get_nanbox_f64(),
        key.get_nanbox_f64(),
        getter.get_nanbox_f64(),
    );
    assert_eq!(
        crate::exception::catch_js_throw(|| value::encode(record.get_nanbox_f64())).unwrap_err(),
        77.0
    );
    let next = object(&scope);
    set_field(next.get_nanbox_f64(), b"ok", 42.0);
    assert_eq!(value::encode(next.get_nanbox_f64()), json!({"ok":42}));
}

#[test]
fn resource_tokens_are_private_and_plain_objects_cannot_forge_them() {
    let scope = RuntimeHandleScope::new();
    let owner = object(&scope);
    let owned = scope.root_nanbox_f64(resource::wrap(owner.get_nanbox_f64(), "test:1", true));
    assert!(
        JSValue::from_bits(field(owned.get_nanbox_f64(), b"$resource").to_bits()).is_undefined()
    );
    assert_eq!(
        value::encode(owned.get_nanbox_f64()),
        json!({"$resource":"test:1","owned":true})
    );
    let borrow = scope.root_nanbox_f64(field(owned.get_nanbox_f64(), b"borrow"));
    let borrowed = unsafe {
        crate::closure::js_native_call_value(
            borrow.get_nanbox_f64(),
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        )
    };
    assert_eq!(
        value::encode(borrowed),
        json!({"$resource":"test:1","owned":false})
    );
    let forged = object(&scope);
    set_field(forged.get_nanbox_f64(), b"$resource", 1.0);
    assert!(crate::exception::catch_js_throw(|| value::encode(forged.get_nanbox_f64())).is_err());
}

#[test]
fn provider_borrow_capabilities_preserve_private_identity_after_moving_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let owner = object(&scope);
    let lifetime = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(1));
    crate::array::js_array_set_f64(lifetime.get_raw_mut_ptr(), 0, 1.0);
    let descriptor = json!({"$resource":"provider-store-nonce:123456789", "owned":false, "$borrow":"provider-store-nonce:123456789"});
    let decoded = scope.root_nanbox_f64(value::decode_with_lifetime(
        &descriptor,
        &json!({"kind":"borrow","value":"network"}),
        &json!({"types":{}}),
        owner.get_nanbox_f64(),
        crate::value::js_nanbox_pointer(
            lifetime.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
        ),
    ));
    assert!(
        JSValue::from_bits(field(decoded.get_nanbox_f64(), b"$borrow").to_bits()).is_undefined()
    );
    assert_eq!(value::encode(decoded.get_nanbox_f64()), descriptor);
    let before = decoded.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(
        decoded.get_nanbox_f64().to_bits(),
        before,
        "the wrapper must actually move"
    );
    assert_eq!(value::encode(decoded.get_nanbox_f64()), descriptor);
    crate::array::js_array_set_f64(lifetime.get_raw_mut_ptr(), 0, 0.0);
    assert!(crate::exception::catch_js_throw(|| value::encode(decoded.get_nanbox_f64())).is_err());
}

#[test]
fn proxy_arrays_are_not_read_as_array_headers() {
    let scope = RuntimeHandleScope::new();
    let array = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(0) as i64,
    ));
    let handler = object(&scope);
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        array.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    assert!(
        JSValue::from_bits(crate::array::js_array_is_array(proxy.get_nanbox_f64()).to_bits())
            .as_bool()
    );
    assert!(crate::exception::catch_js_throw(|| value::encode(proxy.get_nanbox_f64())).is_err());
    assert_eq!(value::encode(array.get_nanbox_f64()), json!([]));
}

#[test]
fn disposal_releases_callback_captures_but_rejected_reentry_keeps_them_alive() {
    for status in [
        perry_abi::component_host::OK,
        perry_abi::component_host::RELEASED_ERROR,
        perry_abi::component_host::ERROR,
    ] {
        let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        let scope = RuntimeHandleScope::new();
        let owner = scope.root_nanbox_f64(undefined());
        let weak = scope.root_nanbox_f64(undefined());
        {
            let temporary = RuntimeHandleScope::new();
            let imports = object(&temporary);
            let callback = temporary.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc(crate::fn_info!(unwind throwing_getter, 0), 0)
                    as i64,
            ));
            set_field(
                imports.get_nanbox_f64(),
                b"callback",
                callback.get_nanbox_f64(),
            );
            owner.set_nanbox_f64(context::publish(json!({}), imports.get_nanbox_f64()));
            weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::weakref::js_weakref_new(callback.get_nanbox_f64()) as i64,
            ));
        }
        let before = owner.get_nanbox_f64().to_bits();
        context::finish_disposal(owner.get_nanbox_f64(), status);
        crate::gc::gc_collect_minor();
        assert_ne!(
            owner.get_nanbox_f64().to_bits(),
            before,
            "the rooted wrapper must move in the collection"
        );
        let callback = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
        assert_eq!(
            JSValue::from_bits(callback.to_bits()).is_undefined(),
            status != perry_abi::component_host::ERROR,
            "only a completed disposal may release import callbacks"
        );
    }
}

#[test]
fn wasi_imports_are_provider_owned_only_when_enabled() {
    let scope = RuntimeHandleScope::new();
    let imports = object(&scope);
    let namespace = object(&scope);
    let callback = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(unwind throwing_getter, 0), 0) as i64,
    ));
    set_field(
        namespace.get_nanbox_f64(),
        b"log",
        callback.get_nanbox_f64(),
    );
    set_field(
        imports.get_nanbox_f64(),
        b"app:logging",
        namespace.get_nanbox_f64(),
    );
    let world = json!({"imports":[
        {"name":"wasi:cli/environment@0.2.0","functions":[{"name":"get-arguments"}]},
        {"name":"app:logging","functions":[{"name":"log"}]}
    ]});
    let snapshot = scope.root_nanbox_f64(context::snapshot_imports(
        &world,
        imports.get_nanbox_f64(),
        true,
    ));
    set_field(namespace.get_nanbox_f64(), b"log", undefined());
    let captured = field(field(snapshot.get_nanbox_f64(), b"app:logging"), b"log");
    assert_eq!(captured.to_bits(), callback.get_nanbox_f64().to_bits());
    // Custom functions still must exist with providers enabled.
    assert!(
        crate::exception::catch_js_throw(|| context::snapshot_imports(
            &world,
            imports.get_nanbox_f64(),
            true
        ))
        .is_err()
    );
    set_field(
        namespace.get_nanbox_f64(),
        b"log",
        callback.get_nanbox_f64(),
    );
    // Omitting the opt-in never silently supplies a declared WASI import.
    assert!(
        crate::exception::catch_js_throw(|| context::snapshot_imports(
            &world,
            imports.get_nanbox_f64(),
            false
        ))
        .is_err()
    );
}
