use super::readers::invoke;
use super::*;

#[test]
fn intentional_writer_retirement_is_quiet_without_suppressing_real_cleanup_errors() {
    let _ops = host::TestOpsGuard::install(&OPS);
    for (marker, error, quiet) in [
        (Some(1), None, true),
        (None, None, false),
        (Some(999), None, false),
        (Some(1), Some(77.0), false),
    ] {
        let scope = RuntimeHandleScope::new();
        let component = scope.root_nanbox_f64(load(&scope, 42.0));
        let _lease = EngineLease(
            context::get(component.get_nanbox_f64()),
            engine(component.get_nanbox_f64()),
        );
        unsafe {
            (*context::get(component.get_nanbox_f64()))
                .pending_collections
                .push(json!({"kind":"async-writer","writer":{"$writer":"forgotten:future"}}));
        }
        cleanup::drain(component.get_nanbox_f64());
        unsafe {
            (*engine(component.get_nanbox_f64())).trap = true;
            (*engine(component.get_nanbox_f64())).collected_writer_retirement = marker;
        }
        if let Some(error) = error {
            remember(component.get_nanbox_f64(), 1, error);
        }
        let result = crate::exception::catch_js_throw(|| poll::poll_components());
        assert_eq!(result.is_ok(), quiet);
        if let Some(error) = error {
            assert_eq!(result.unwrap_err(), error);
        }
        assert!(!unsafe { (*context::get(component.get_nanbox_f64())).alive });
        assert_eq!(poll::has_active(), 0);
    }
}

#[test]
fn returned_background_contexts_do_not_consume_native_pending_call_permits() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let limit = perry_abi::component_host::MAX_PENDING_CALLS as u64;
    for _ in 0..limit {
        call(component.get_nanbox_f64(), "run");
    }
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = (1..=limit).collect();
        (*engine(component.get_nanbox_f64())).retained = (1..=limit).collect();
    }
    poll::poll_components();
    for _ in 0..6 {
        call(component.get_nanbox_f64(), "run");
    }
    assert_eq!(
        unsafe { (*state(component.get_nanbox_f64())).origins.len() },
        limit as usize + 6
    );
    assert_eq!(
        unsafe { (*engine(component.get_nanbox_f64())).next },
        limit + 6
    );
    finish_component(component.get_nanbox_f64());
}
#[test]
fn factory_metadata_mutation_cannot_change_its_private_type_and_writes_decode_results() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    unsafe {
        let world = &mut (*context::get(component.get_nanbox_f64())).world;
        world["types"]["payload"] = json!({"ty":{"kind":"u32"}});
        (*engine(component.get_nanbox_f64())).types = json!([{"index":7,"schema":{"kind":"future","value":{"kind":"ref","value":"payload"}}}]);
        (*engine(component.get_nanbox_f64())).replies.insert(
            1,
            json!({"value":{"$future":"factory:r"},"writer":{"$writer":"factory:w"}}),
        );
    }
    let types = scope.root_nanbox_f64(invoke(component.get_nanbox_f64(), b"asyncTypes", &[]));
    let factory = scope.root_nanbox_f64(crate::array::js_array_get_f64(
        array_pointer(types.get_nanbox_f64()).unwrap(),
        0,
    ));
    let fake = scope.root_nanbox_f64(text("tampered"));
    assert_eq!(
        serde_json::from_str::<Value>(&string(field(factory.get_nanbox_f64(), b"schema"))).unwrap(),
        json!({"kind":"future","value":{"kind":"u32"}})
    );
    set_field(factory.get_nanbox_f64(), b"schema", fake.get_nanbox_f64());
    set_field(factory.get_nanbox_f64(), b"kind", fake.get_nanbox_f64());
    let created = scope.root_nanbox_f64(invoke(factory.get_nanbox_f64(), b"create", &[]));
    poll::poll_components();
    assert_eq!(
        unsafe { &(*engine(component.get_nanbox_f64())).requests }[&1]["index"],
        7
    );
    let pair = scope.root_nanbox_f64(crate::promise::js_promise_result(promise(
        created.get_nanbox_f64(),
    )));
    assert_eq!(string(field(pair.get_nanbox_f64(), b"kind")), "future");
    let writer = scope.root_nanbox_f64(field(pair.get_nanbox_f64(), b"writer"));
    assert!(JSValue::from_bits(field(writer.get_nanbox_f64(), b"then").to_bits()).is_undefined());
    unsafe {
        (*engine(component.get_nanbox_f64())).replies.insert(
            2,
            json!({"written":1,"remaining":[],"cancelled":false,"closed":true}),
        );
    }
    let written = scope.root_nanbox_f64(invoke(writer.get_nanbox_f64(), b"write", &[41.0]));
    poll::poll_components();
    assert_eq!(
        unsafe { &(*engine(component.get_nanbox_f64())).requests }[&2]["values"],
        json!([41])
    );
    assert_eq!(
        value::encode(crate::promise::js_promise_result(promise(
            written.get_nanbox_f64()
        ))),
        json!({"written":1,"remaining":[],"cancelled":false,"closed":true})
    );
    assert!(crate::exception::catch_js_throw(|| invoke(
        writer.get_nanbox_f64(),
        b"write",
        &[42.0]
    ))
    .is_err());
    assert_eq!(
        poll::has_active(),
        0,
        "settled factory/write operations release their driver pin"
    );
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

#[test]
fn pending_write_payload_roots_release_only_after_native_ownership_receipts() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).defer_writes = true;
    }
    let writer = scope.root_nanbox_f64(super::super::super::writer::decode(
        component.get_nanbox_f64(),
        &json!({"$writer":"writer:owned"}),
        &json!({"kind":"own","value":"resource"}),
        false,
    ));
    let weak = scope.root_nanbox_f64(undefined());
    let written = scope.root_nanbox_f64(undefined());
    {
        let temporary = RuntimeHandleScope::new();
        let owner = temporary.root_nanbox_f64(resource::wrap(
            component.get_nanbox_f64(),
            "guest:write-payload",
            true,
        ));
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(owner.get_nanbox_f64()) as i64,
        ));
        written.set_nanbox_f64(invoke(
            writer.get_nanbox_f64(),
            b"write",
            &[owner.get_nanbox_f64()],
        ));
    }
    crate::gc::gc_collect_minor();
    assert!(
        !JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined()
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).events = vec![(
            1,
            json!({"kind":"lower","created":[],"transferred":[],"guestTransferred":["guest:write-payload"],"asyncTransferred":[]}),
        )];
    }
    poll::poll_components();
    assert_eq!(
        crate::promise::js_promise_state(promise(written.get_nanbox_f64())),
        0
    );
    crate::gc::js_gc_collect();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "payload retention ends at the lower receipt, before write completion"
    );
    crate::promise::js_promise_mark_internally_handled(promise(written.get_nanbox_f64()));
    finish_component(component.get_nanbox_f64());
}

#[test]
fn payload_free_chunks_materialize_undefined_units_instead_of_null_payloads() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let chunk = scope.root_nanbox_f64(super::super::super::async_value::chunk(
        component.get_nanbox_f64(),
        &json!([null, null, null]),
        &Value::Null,
        &Value::Null,
        true,
    ));
    assert_eq!(
        crate::array::js_array_length(array_pointer(chunk.get_nanbox_f64()).unwrap()),
        3
    );
    for i in 0..3 {
        assert!(JSValue::from_bits(
            crate::array::js_array_get_f64(array_pointer(chunk.get_nanbox_f64()).unwrap(), i)
                .to_bits()
        )
        .is_undefined());
    }
    finish_component(component.get_nanbox_f64());
}
