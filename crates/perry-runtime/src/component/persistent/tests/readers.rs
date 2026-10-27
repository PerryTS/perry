use super::*;
pub(super) fn invoke(value: f64, name: &[u8], args: &[f64]) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    unsafe {
        crate::closure::js_native_call_value(
            field(value.get_nanbox_f64(), name),
            crate::closure::plain_call_receiver(),
            args.as_ptr(),
            args.len(),
        )
    }
}
fn readable(component: f64, stream: bool, payload: &Value) -> f64 {
    super::super::super::async_value::decode(
        component,
        &if stream {
            json!({"$stream":"test:reader"})
        } else {
            json!({"$future":"test:reader"})
        },
        payload,
        stream,
    )
}
#[test]
fn canonical_readers_decode_payloads_bytes_and_eof_without_becoming_thenable() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let future = scope.root_nanbox_f64(readable(
        component.get_nanbox_f64(),
        false,
        &json!({"kind":"u64"}),
    ));
    assert!(JSValue::from_bits(field(future.get_nanbox_f64(), b"then").to_bits()).is_undefined());
    unsafe {
        (*engine(component.get_nanbox_f64()))
            .replies
            .insert(1, json!({"$bigint":"18446744073709551615"}));
    }
    let result = scope.root_nanbox_f64(invoke(future.get_nanbox_f64(), b"read", &[]));
    poll::poll_components();
    assert_eq!(
        value::encode(crate::promise::js_promise_result(promise(
            result.get_nanbox_f64()
        ))),
        json!({"$bigint":"18446744073709551615"})
    );
    assert!(
        crate::exception::catch_js_throw(|| invoke(future.get_nanbox_f64(), b"read", &[])).is_err()
    );
    let stream = scope.root_nanbox_f64(readable(
        component.get_nanbox_f64(),
        true,
        &json!({"kind":"u8"}),
    ));
    unsafe {
        (*engine(component.get_nanbox_f64()))
            .replies
            .insert(2, json!({"done":false,"value":[0,255,17]}));
    }
    let chunk = scope.root_nanbox_f64(invoke(stream.get_nanbox_f64(), b"read", &[3.0]));
    poll::poll_components();
    let chunk = scope.root_nanbox_f64(crate::promise::js_promise_result(promise(
        chunk.get_nanbox_f64(),
    )));
    let bytes = field(chunk.get_nanbox_f64(), b"value");
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(
            JSValue::from_bits(bytes.to_bits()).as_pointer::<u8>() as usize
        ),
        Some(crate::typedarray::KIND_UINT8)
    );
    assert_eq!(
        value::encode(chunk.get_nanbox_f64()),
        json!({"done":false,"value":[0,255,17]})
    );
    unsafe {
        (*engine(component.get_nanbox_f64()))
            .replies
            .insert(3, json!({"done":true}));
    }
    let end = scope.root_nanbox_f64(invoke(stream.get_nanbox_f64(), b"read", &[]));
    poll::poll_components();
    assert_eq!(
        value::encode(crate::promise::js_promise_result(promise(
            end.get_nanbox_f64()
        ))),
        json!({"done":true})
    );
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}
extern "C" fn unused_handler(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    undefined()
}
#[test]
fn observed_temporary_readers_survive_moving_gc_and_release_their_owner_after_settlement() {
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
        (*engine(component.get_nanbox_f64())).defer_reads = true;
    }
    let weak = scope.root_nanbox_f64(undefined());
    {
        let temporary = RuntimeHandleScope::new();
        let future = temporary.root_nanbox_f64(readable(
            component.get_nanbox_f64(),
            false,
            &json!({"kind":"u32"}),
        ));
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(future.get_nanbox_f64()) as i64,
        ));
        let result = temporary.root_nanbox_f64(invoke(future.get_nanbox_f64(), b"read", &[]));
        let handler = temporary.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(unused_handler, 1),
            0,
        ));
        // Await uses the same attach-handlers entry. No user Promise or reader
        // root survives this scope; the registered reaction is the consumer.
        crate::promise::then::js_promise_attach_handlers(
            promise(result.get_nanbox_f64()),
            handler.get_raw_const_ptr(),
            std::ptr::null(),
        );
    }
    crate::gc::gc_collect_minor();
    assert!(
        !JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "observed operation must retain its temporary owned handle"
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![1];
        (*engine(component.get_nanbox_f64()))
            .replies
            .insert(1, json!(42));
    }
    poll::poll_components();
    crate::promise::js_promise_run_microtasks();
    crate::gc::js_gc_collect();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "settlement must release the operation's owner link"
    );
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}
#[test]
fn abandoned_unobserved_reads_are_weak_and_queue_soft_collection() {
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
        (*engine(component.get_nanbox_f64())).defer_reads = true;
    }
    let weak = scope.root_nanbox_f64(undefined());
    {
        let temporary = RuntimeHandleScope::new();
        let future = temporary.root_nanbox_f64(readable(
            component.get_nanbox_f64(),
            false,
            &json!({"kind":"u32"}),
        ));
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(future.get_nanbox_f64()) as i64,
        ));
        invoke(future.get_nanbox_f64(), b"read", &[]);
    }
    crate::gc::gc_collect_minor();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "producer must not root an unobserved public Promise/reader cycle"
    );
    assert!(crate::weakref::pending_finalization_jobs_count() >= 2);
    // Explicit full GC delivers recorded jobs to nextTick; inspect custody
    // before that delivery, then run the actual queued callbacks.
    crate::gc::js_gc_collect();
    crate::weakref::drain_pending_finalization_jobs();
    crate::promise::js_promise_run_microtasks();
    let queued = unsafe { &(*engine(component.get_nanbox_f64())).requests };
    assert!(
        queued.values().any(|value| value["op"] == "collect"),
        "GC must submit soft collection rather than dropping a started read observer"
    );
    finish_component(component.get_nanbox_f64());
}

extern "C-unwind" fn refuse_read(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::gc::gc_collect_minor();
    crate::exception::js_throw(error.get_nanbox_f64())
}

#[test]
fn refused_ready_read_rejects_with_the_hook_error_and_collects_its_owned_output() {
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
    let future = scope.root_nanbox_f64(readable(
        component.get_nanbox_f64(),
        false,
        &json!({"kind":"u32"}),
    ));
    let result = scope.root_nanbox_f64(invoke(future.get_nanbox_f64(), b"read", &[]));
    crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    unsafe {
        (*engine(component.get_nanbox_f64()))
            .replies
            .insert(1, json!({"$resource":"unpublished:owned","owned":true}));
    }
    let hook = scope.root_nanbox_f64(hook(
        &scope,
        b"before",
        crate::fn_info!(unwind refuse_read,1),
        &[error.get_nanbox_f64()],
    ));
    poll::poll_components();
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert_eq!(
        crate::promise::js_promise_state(promise(result.get_nanbox_f64())),
        2
    );
    assert_eq!(
        crate::promise::js_promise_result(promise(result.get_nanbox_f64())).to_bits(),
        error.get_nanbox_f64().to_bits()
    );
    assert!(
        unsafe { &(*engine(component.get_nanbox_f64())).requests }
            .values()
            .any(|request| request["op"] == "resource-collect"
                && request["request"]["token"] == "unpublished:owned"),
        "rejecting the public Promise must not strand the successful native result"
    );
    finish_component(component.get_nanbox_f64());
}

#[test]
fn read_completion_after_observer_collection_retires_owned_output_without_decoding_it() {
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
        (*engine(component.get_nanbox_f64())).defer_reads = true;
    }
    let weak = scope.root_nanbox_f64(undefined());
    {
        let temporary = RuntimeHandleScope::new();
        let future = temporary.root_nanbox_f64(readable(
            component.get_nanbox_f64(),
            false,
            &json!({"kind":"u32"}),
        ));
        let result = temporary.root_nanbox_f64(invoke(future.get_nanbox_f64(), b"read", &[]));
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(result.get_nanbox_f64()) as i64,
        ));
    }
    crate::gc::gc_collect_minor();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined()
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![1];
        (*engine(component.get_nanbox_f64()))
            .replies
            .insert(1, json!({"$resource":"unobserved:owned","owned":true}));
    }
    poll::poll_components();
    assert!(unsafe { &(*engine(component.get_nanbox_f64())).requests }
        .values()
        .any(|request| request["op"] == "resource-collect"
            && request["request"]["token"] == "unobserved:owned"));
    finish_component(component.get_nanbox_f64());
}

#[test]
fn ownership_receipts_close_every_alias_of_a_transferred_reader() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let future = scope.root_nanbox_f64(readable(
        component.get_nanbox_f64(),
        false,
        &json!({"kind":"u32"}),
    ));
    let plan = scope.root_nanbox_f64(value::retention::new_plan());
    for _ in 0..2 {
        value::encode_for_retained(
            future.get_nanbox_f64(),
            component.get_nanbox_f64(),
            plan.get_nanbox_f64(),
        );
    }
    let slots = value::retention::transferred(
        plan.get_nanbox_f64(),
        &json!({"kind":"lower","asyncTransferred":["test:reader"]}),
    );
    assert_eq!(slots.len(), 2);
    for slot in slots {
        value::retention::release(plan.get_nanbox_f64(), slot);
    }
    assert!(
        crate::exception::catch_js_throw(|| invoke(future.get_nanbox_f64(), b"read", &[])).is_err()
    );
    invoke(future.get_nanbox_f64(), b"dispose", &[]);
    assert!(
        unsafe { &(*engine(component.get_nanbox_f64())).requests }.is_empty(),
        "transferred readers must not later collect guest-owned values"
    );
    finish_component(component.get_nanbox_f64());
}
