//! Controlled native suspension makes the JS lifetime/context contract exact.
use super::*;
use perry_abi::component_host::{ComponentHostOps, WakeCallback};

mod unused {
    #![allow(dead_code)]
    macro_rules! entries {
        ($($name:ident($($arg:ident:$ty:ty),*) -> $ret:ty;)*) => {
            $(pub unsafe extern "C" fn $name($($arg:$ty),*) -> $ret {
                let _=($($arg),*); Default::default()
            })*
            pub const OPS: perry_abi::component_host::ComponentHostOps = perry_abi::component_host::ComponentHostOps {
                abi_version:perry_abi::component_host::ABI_VERSION,
                size:std::mem::size_of::<perry_abi::component_host::ComponentHostOps>(),
                $($name,)*
            };
        }
    }
    perry_abi::component_host_entry_points!(entries);
}
#[derive(Default)]
struct Engine {
    starts: usize,
    resolved: bool,
    disposed: usize,
    cleanup_error: bool,
}
// Clear stack-only mock engines even if an assertion fails, before GC cells
// outlive the test's temporary ops table.
struct EngineLease(*mut context::Context);
impl Drop for EngineLease {
    fn drop(&mut self) {
        unsafe { (*self.0).host = std::ptr::null_mut() };
    }
}
unsafe fn reply(out: *mut Buffer, bytes: &[u8]) {
    let bytes = bytes.to_vec().into_boxed_slice();
    *out = Buffer {
        len: bytes.len(),
        data: Box::into_raw(bytes).cast(),
    };
}
unsafe extern "C" fn start(
    handle: *mut std::ffi::c_void,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    args: *const u8,
    len: usize,
    primary: u64,
    _: u64,
    _: Option<WakeCallback>,
    _: u64,
    out: *mut Buffer,
) -> i32 {
    assert_eq!(std::slice::from_raw_parts(args, len), b"[]");
    let engine = &mut *handle.cast::<Engine>();
    engine.starts += 1;
    engine.resolved = false;
    let args = br#"{"$asyncRequest":"1","args":[]}"#;
    let mut result = Buffer::default();
    assert_eq!(
        callback::invoke(
            primary,
            b"".as_ptr(),
            0,
            b"wait".as_ptr(),
            4,
            args.as_ptr(),
            args.len(),
            &mut result
        ),
        ASYNC_PENDING
    );
    callback::free(result);
    *out = Buffer::default();
    ASYNC_PENDING
}
unsafe extern "C" fn poll(
    handle: *mut std::ffi::c_void,
    _: u64,
    _: u64,
    _: Option<WakeCallback>,
    _: u64,
    out: *mut Buffer,
) -> i32 {
    let engine = &mut *handle.cast::<Engine>();
    if engine.resolved {
        reply(out, br#"{"returned":true,"value":42}"#);
        OK
    } else {
        *out = Buffer::default();
        ASYNC_PENDING
    }
}
unsafe extern "C" fn resolve(
    handle: *mut std::ffi::c_void,
    request: u64,
    success: bool,
    value: *const u8,
    len: usize,
    out: *mut Buffer,
) -> i32 {
    assert_eq!(request, 1);
    if success {
        assert_eq!(std::slice::from_raw_parts(value, len), b"42");
    }
    (*handle.cast::<Engine>()).resolved = true;
    *out = Buffer::default();
    OK
}
unsafe extern "C" fn dispose(handle: *mut std::ffi::c_void, cleanup: u64, out: *mut Buffer) -> i32 {
    let engine = &mut *handle.cast::<Engine>();
    engine.disposed += 1;
    *out = Buffer::default();
    if engine.cleanup_error {
        (*(cleanup as usize as *const callback::Frame<'_>)).capture(99.0);
        RELEASED_ERROR
    } else {
        OK
    }
}
static OPS: ComponentHostOps = ComponentHostOps {
    perry_component_host_call_async_start: start,
    perry_component_host_call_async_poll: poll,
    perry_component_host_async_import_resolve: resolve,
    perry_component_host_dispose: dispose,
    perry_component_host_buffer_drop: callback::free,
    ..unused::OPS
};
extern "C-unwind" fn handler(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let expected = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let token = crate::closure::js_closure_get_capture_f64(closure, 2) as i64;
    assert_eq!(pump(), 0, "a JS import must not reenter the async pump");
    crate::gc::gc_collect_minor();
    assert_eq!(
        crate::async_context::get_store(token).unwrap().to_bits(),
        expected.get_nanbox_f64().to_bits()
    );
    assert_ne!(crate::async_hooks::execution_async_id_u64(), 0);
    promise.get_nanbox_f64()
}
fn promise_pointer(value: f64) -> *mut crate::promise::Promise {
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}

#[test]
fn pending_component_promises_move_preserve_context_and_rejection_identity() {
    for reject in [false, true] {
        let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::gc_init();
        let _engine = host::TestOpsGuard::install(&OPS);
        assert_eq!(has_active(), 0);
        let mut engine = Engine {
            cleanup_error: reject,
            ..Engine::default()
        };
        let scope = RuntimeHandleScope::new();
        let token = crate::async_context::AsyncLocalStoragePayload::default();
        let expected = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let imported = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let imports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(unwind handler,0),
            3,
        ));
        for (index, value) in [
            imported.get_nanbox_f64(),
            expected.get_nanbox_f64(),
            token.token() as f64,
        ]
        .into_iter()
        .enumerate()
        {
            crate::closure::js_closure_set_capture_f64(
                callback.get_raw_mut_ptr(),
                index as u32,
                value,
            );
        }
        set_field(
            imports.get_nanbox_f64(),
            b"wait",
            crate::value::js_nanbox_pointer(
                callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
        let component=scope.root_nanbox_f64(context::publish(serde_json::json!({"types":{},
            "imports":[{"name":"","functions":[{"name":"wait","async":true,"params":[],"result":{"kind":"u32"}}]}],
            "exports":[{"name":"","functions":[{"name":"run","async":true,"params":[],"result":{"kind":"u32"}}]}]}),imports.get_nanbox_f64()));
        unsafe {
            (*context::get(component.get_nanbox_f64())).host = (&mut engine as *mut Engine).cast();
        }
        let _lease = EngineLease(context::get(component.get_nanbox_f64()));
        let args = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        crate::async_context::set_store(token.token(), expected.get_nanbox_f64());
        let result = scope.root_nanbox_f64(submit(
            component.get_nanbox_f64(),
            "",
            "run",
            args.get_nanbox_f64(),
        ));
        crate::promise::js_promise_mark_internally_handled(promise_pointer(
            result.get_nanbox_f64(),
        ));
        assert!(
            crate::exception::catch_js_throw(|| submit(
                component.get_nanbox_f64(),
                "",
                "run",
                args.get_nanbox_f64()
            ))
            .is_err(),
            "overlapping calls must fail before sharing import replies"
        );
        crate::async_context::set_store(token.token(), 17.0);
        assert_eq!(pump(), 0);
        assert_eq!(engine.starts, 1, "the import must actually suspend");
        let entries = scope.root_nanbox_f64(unsafe {
            crate::object::owned_state_bag::load(
                object_pointer(component.get_nanbox_f64()).unwrap() as usize,
                crate::object::owned_state_bag::COMPONENT_ASYNC_IMPORTS,
            )
        });
        let record = scope.root_nanbox_f64(crate::array::js_array_get_f64(
            array_pointer(entries.get_nanbox_f64()).unwrap(),
            0,
        ));
        let lifetime = scope.root_nanbox_f64(field(record.get_nanbox_f64(), b"lifetime"));
        assert_eq!(
            crate::array::js_array_get_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0),
            1.0
        );
        let link = PENDING.with(|q| q.borrow()[0]);
        let before = unsafe { payload::link_event_owner(link) }
            .unwrap()
            .to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            before,
            unsafe { payload::link_event_owner(link) }
                .unwrap()
                .to_bits(),
            "operation must actually move"
        );
        if reject {
            crate::promise::js_promise_reject(
                promise_pointer(imported.get_nanbox_f64()),
                expected.get_nanbox_f64(),
            );
        } else {
            crate::promise::js_promise_resolve(promise_pointer(imported.get_nanbox_f64()), 42.0);
        }
        assert_eq!(pump(), 1);
        assert_eq!(
            crate::promise::js_promise_state(promise_pointer(result.get_nanbox_f64())),
            if reject { 2 } else { 1 }
        );
        assert_eq!(
            crate::promise::js_promise_result(promise_pointer(result.get_nanbox_f64())).to_bits(),
            if reject {
                expected.get_nanbox_f64().to_bits()
            } else {
                42.0f64.to_bits()
            }
        );
        assert_eq!(
            crate::array::js_array_get_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0),
            0.0,
            "borrow lifetime must expire on settlement"
        );
        assert_eq!(engine.disposed, usize::from(reject));
        assert_eq!(crate::async_context::get_store(token.token()), Some(17.0));
        assert_eq!(has_active(), 0);
        assert!(!unsafe { (*context::get(component.get_nanbox_f64())).async_active });
        if !reject {
            // Repeated imports must reuse cleared slots rather than grow an
            // array for the component's entire lifetime.
            for _ in 0..70 {
                crate::async_context::set_store(token.token(), expected.get_nanbox_f64());
                let next = scope.root_nanbox_f64(submit(
                    component.get_nanbox_f64(),
                    "",
                    "run",
                    args.get_nanbox_f64(),
                ));
                crate::async_context::set_store(token.token(), 17.0);
                assert_eq!(pump(), 0);
                assert_eq!(pump(), 1);
                assert_eq!(
                    crate::promise::js_promise_result(promise_pointer(next.get_nanbox_f64())),
                    42.0
                );
                assert_eq!(
                    unsafe { (*array_pointer(entries.get_nanbox_f64()).unwrap()).length },
                    1
                );
            }
        }
        unsafe {
            (*context::get(component.get_nanbox_f64())).host = std::ptr::null_mut();
        }
        crate::async_context::js_async_context_als_clear(token.token());
    }
}

extern "C" fn pending_import(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}
fn instance(scope: &RuntimeHandleScope, engine: &mut Engine, imported: f64) -> f64 {
    let imported = scope.root_nanbox_f64(imported);
    let imports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(pending_import, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        imported.get_nanbox_f64(),
    );
    set_field(
        imports.get_nanbox_f64(),
        b"wait",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let component = scope.root_nanbox_f64(context::publish(serde_json::json!({"types":{},
        "imports":[{"name":"","functions":[{"name":"wait","async":true,"params":[],"result":{"kind":"u32"}}]}],
        "exports":[{"name":"","functions":[{"name":"run","async":true,"params":[],"result":{"kind":"u32"}}]}]}), imports.get_nanbox_f64()));
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = (engine as *mut Engine).cast();
    }
    component.get_nanbox_f64()
}
extern "C-unwind" fn throwing_hook(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    crate::gc::gc_collect_minor();
    crate::exception::js_throw(77.0)
}

#[test]
fn disposal_and_throwing_lifecycle_hooks_cancel_suspended_component_calls() {
    for phase in [None, Some(b"before".as_slice()), Some(b"after".as_slice())] {
        let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::gc_init();
        let _engine = host::TestOpsGuard::install(&OPS);
        assert_eq!(has_active(), 0);
        let scope = RuntimeHandleScope::new();
        let imported = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let mut engine = Engine::default();
        let component =
            scope.root_nanbox_f64(instance(&scope, &mut engine, imported.get_nanbox_f64()));
        let _lease = EngineLease(context::get(component.get_nanbox_f64()));
        let args = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        let promise = scope.root_nanbox_f64(submit(
            component.get_nanbox_f64(),
            "",
            "run",
            args.get_nanbox_f64(),
        ));
        crate::promise::js_promise_mark_internally_handled(promise_pointer(
            promise.get_nanbox_f64(),
        ));
        assert_eq!(pump(), 0);
        assert_eq!(engine.starts, 1);
        let hook = scope.root_nanbox_f64(undefined());
        if let Some(phase) = phase {
            let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let callback =
                crate::closure::js_closure_alloc(crate::fn_info!(unwind throwing_hook, 1), 0);
            set_field(
                options.get_nanbox_f64(),
                phase,
                crate::value::js_nanbox_pointer(callback as i64),
            );
            hook.set_nanbox_f64(crate::async_hooks::js_async_hooks_create_hook(
                options.get_nanbox_f64(),
            ));
            crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(
                hook.get_nanbox_f64(),
            ));
        } else {
            js_component_dispose(component.get_nanbox_f64());
        }
        let outcome = crate::exception::catch_js_throw(|| pump());
        if phase == Some(b"after".as_slice()) {
            assert_eq!(outcome, Err(77.0));
        } else {
            assert_eq!(outcome, Ok(1));
        }
        if phase.is_some() {
            crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
                hook.get_nanbox_f64(),
            ));
            assert_eq!(
                crate::promise::js_promise_result(promise_pointer(promise.get_nanbox_f64())),
                77.0
            );
        }
        assert_eq!(
            crate::promise::js_promise_state(promise_pointer(promise.get_nanbox_f64())),
            2
        );
        assert_eq!(
            engine.disposed, 1,
            "a hook that prevents polling must still cancel the native call"
        );
        assert_eq!(has_active(), 0);
        assert!(!unsafe { (*context::get(component.get_nanbox_f64())).alive });
        unsafe {
            (*context::get(component.get_nanbox_f64())).host = std::ptr::null_mut();
        }
    }
}

extern "C" fn dispose_during_init(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) == "PROMISE" {
        js_component_dispose(crate::closure::js_closure_get_capture_f64(closure, 0));
        crate::gc::gc_collect_minor();
    }
    undefined()
}
#[test]
fn async_call_rechecks_component_after_promise_init_hook_disposes_it() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _engine = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let imported = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::promise::js_promise_new() as i64,
    ));
    let mut engine = Engine::default();
    let component = scope.root_nanbox_f64(instance(&scope, &mut engine, imported.get_nanbox_f64()));
    let _lease = EngineLease(context::get(component.get_nanbox_f64()));
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(dispose_during_init, 4),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        component.get_nanbox_f64(),
    );
    set_field(
        options.get_nanbox_f64(),
        b"init",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let hook = scope.root_nanbox_f64(crate::async_hooks::js_async_hooks_create_hook(
        options.get_nanbox_f64(),
    ));
    crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    let args = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(0) as i64,
    ));
    assert!(crate::exception::catch_js_throw(|| submit(
        component.get_nanbox_f64(),
        "",
        "run",
        args.get_nanbox_f64()
    ))
    .is_err());
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert_eq!(engine.starts, 0);
    assert_eq!(engine.disposed, 1);
    assert_eq!(has_active(), 0);
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::null_mut();
    }
}

#[test]
fn queued_calls_retain_resource_wrappers_after_the_argument_array_is_mutated() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _engine = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let imported = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::promise::js_promise_new() as i64,
    ));
    let mut engine = Engine::default();
    let component = scope.root_nanbox_f64(instance(&scope, &mut engine, imported.get_nanbox_f64()));
    let _lease = EngineLease(context::get(component.get_nanbox_f64()));
    unsafe {
        (&mut (*context::get(component.get_nanbox_f64())).world)["imports"][0]["resources"] =
            serde_json::json!({"item":"r0"});
        (&mut (*context::get(component.get_nanbox_f64())).world)["exports"][0]["functions"][0]
            ["params"] = serde_json::json!([{"name":"item", "ty":{"kind":"own", "value":"r0"}}]);
    }
    let args = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    let weak = scope.root_nanbox_f64(undefined());
    let before;
    {
        let temporary = RuntimeHandleScope::new();
        let ty = temporary.root_nanbox_f64(crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(b"r0".as_ptr(), 2) as i64,
        ));
        let item =
            temporary.root_nanbox_f64(host_resource::payload::js_component_host_resource_create(
                component.get_nanbox_f64(),
                ty.get_nanbox_f64(),
                42.0,
                undefined(),
            ));
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(item.get_nanbox_f64()) as i64,
        ));
        before = item.get_nanbox_f64().to_bits();
        crate::array::js_array_set_f64(
            array_pointer(args.get_nanbox_f64()).unwrap(),
            0,
            item.get_nanbox_f64(),
        );
    }
    let promise = scope.root_nanbox_f64(submit(
        component.get_nanbox_f64(),
        "",
        "run",
        args.get_nanbox_f64(),
    ));
    crate::promise::js_promise_mark_internally_handled(promise_pointer(promise.get_nanbox_f64()));
    crate::array::js_array_set_f64(
        array_pointer(args.get_nanbox_f64()).unwrap(),
        0,
        undefined(),
    );
    crate::gc::gc_collect_minor();
    let item = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
    assert!(
        !JSValue::from_bits(item.to_bits()).is_undefined(),
        "encoded resource must stay alive before native lowering commits"
    );
    assert_ne!(
        item.to_bits(),
        before,
        "retained resource must actually move"
    );
    js_component_dispose(component.get_nanbox_f64());
    assert_eq!(pump(), 1);
    assert_eq!(
        engine.starts, 0,
        "pre-poll cancellation must not enter a disposed guest"
    );
    assert_eq!(has_active(), 0);
}
