//! The real public runtime entry points with controlled owner-thread C replies.
//! Native mock state contains identities/JSON only; imports exercise real JS,
//! moving GC, Promise hooks and origin frames.
use super::*;
use perry_abi::component_host::{
    ComponentHostOps, OriginCallback, OriginResourceCallback, WakeCallback,
};
use serde_json::json;
mod disposal;
mod failures;

mod unused {
    #![allow(dead_code)]
    macro_rules! entries {
        ($($name:ident($($arg:ident:$ty:ty),*) -> $ret:ty;)*) => {
            $(pub unsafe extern "C" fn $name($($arg:$ty),*) -> $ret { let _=($($arg),*); Default::default() })*
            pub const OPS: perry_abi::component_host::ComponentHostOps = perry_abi::component_host::ComponentHostOps {
                abi_version:perry_abi::component_host::ABI_VERSION,
                size:std::mem::size_of::<perry_abi::component_host::ComponentHostOps>(), $($name,)*
            };
        }
    }
    perry_abi::component_host_entry_points!(entries);
}
#[derive(Default)]
struct Engine {
    next: u64,
    requests: BTreeMap<u64, Value>,
    ready: Vec<u64>,
    retained: Vec<u64>,
    imports: Vec<(u64, bool)>,
    events: Vec<(u64, Value)>,
    resolutions: Vec<u64>,
    pending_imports: Vec<u64>,
    polls: usize,
    disposed: usize,
    trap: bool,
    disposal_events: Vec<(u64, Value)>,
    callback: Option<OriginCallback>,
    resource: Option<OriginResourceCallback>,
}
// The production table lives for the process; this test table is scoped.
// Release mock hosts before its guard and the test thread's GC TLS disappear.
struct EngineLease(*mut context::Context, *mut Engine);
impl Drop for EngineLease {
    fn drop(&mut self) {
        unsafe {
            (*self.0).host = std::ptr::null_mut();
            drop(Box::from_raw(self.1));
        }
    }
}
unsafe fn respond(out: *mut Buffer, value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap().into_boxed_slice();
    *out = Buffer {
        len: bytes.len(),
        data: Box::into_raw(bytes).cast(),
    };
}
unsafe extern "C" fn new_driver(
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    callback: Option<OriginCallback>,
    _: Option<perry_abi::component_host::FreeCallback>,
    resource: Option<OriginResourceCallback>,
    _: u64,
    out: *mut Buffer,
) -> *mut c_void {
    assert!(callback.is_some() && resource.is_some());
    *out = Buffer::default();
    Box::into_raw(Box::new(Engine {
        callback,
        resource,
        ..Engine::default()
    }))
    .cast()
}
unsafe extern "C" fn submit(
    handle: *mut c_void,
    bytes: *const u8,
    len: usize,
    out: *mut Buffer,
) -> i32 {
    let engine = &mut *handle.cast::<Engine>();
    let request: Value = serde_json::from_slice(std::slice::from_raw_parts(bytes, len)).unwrap();
    engine.next += 1;
    let id = engine.next;
    if request["function"] == "sync" || request["op"] != "call" {
        engine.ready.push(id);
    }
    engine.requests.insert(id, request);
    respond(out, &json!({"id":id.to_string()}));
    OK
}
unsafe extern "C" fn native_poll(
    handle: *mut c_void,
    primary: u64,
    _: u64,
    _: Option<WakeCallback>,
    _: u64,
    out: *mut Buffer,
) -> i32 {
    let engine = handle.cast::<Engine>();
    (*engine).polls += 1;
    let imports = std::mem::take(&mut (*engine).imports);
    for (origin, asynchronous) in imports {
        if asynchronous {
            (*engine).pending_imports.push(origin);
        }
        let args = if asynchronous {
            json!({"$asyncRequest":origin.to_string(),"args":[]})
        } else {
            json!([])
        };
        let bytes = serde_json::to_vec(&args).unwrap();
        let mut reply = Buffer::default();
        let status = ((*engine).callback.unwrap())(
            primary,
            origin,
            b"".as_ptr(),
            0,
            b"wait".as_ptr(),
            4,
            bytes.as_ptr(),
            bytes.len(),
            &mut reply,
        );
        if status == ERROR {
            (*engine).trap = true;
        } else {
            assert_eq!(
                status,
                if asynchronous {
                    perry_abi::component_host::ASYNC_PENDING
                } else {
                    OK
                }
            );
        }
        callback::free(reply);
    }
    for (origin, event) in std::mem::take(&mut (*engine).events) {
        let bytes = serde_json::to_vec(&event).unwrap();
        let mut reply = Buffer::default();
        assert_eq!(
            ((*engine).resource.unwrap())(primary, origin, bytes.as_ptr(), bytes.len(), &mut reply),
            OK
        );
        callback::free(reply);
    }
    let ready = std::mem::take(&mut (*engine).ready);
    let completed: Vec<_> = ready
        .into_iter()
        .map(|id| {
            assert!((*engine).requests.contains_key(&id));
            if (*engine).trap {
                json!({"id":id.to_string(),"ok":false,"error":"native trap while invoking import"})
            } else {
                json!({"id":id.to_string(),"ok":true,"returned":true,"value":id*10})
            }
        })
        .collect();
    let retained: Vec<_> = (*engine).retained.iter().map(u64::to_string).collect();
    respond(
        out,
        &json!({"completed":completed,"retained":retained,"active":!(*engine).trap && (!retained.is_empty() || !(*engine).pending_imports.is_empty()),"retired":(*engine).trap,"error":if (*engine).trap { Some("native driver trap") } else { None },"failedOrigins":[]}),
    );
    OK
}
unsafe extern "C" fn resolve(
    handle: *mut c_void,
    request: u64,
    _: bool,
    _: *const u8,
    _: usize,
    out: *mut Buffer,
) -> i32 {
    (*handle.cast::<Engine>()).resolutions.push(request);
    (*handle.cast::<Engine>())
        .pending_imports
        .retain(|id| *id != request);
    *out = Buffer::default();
    OK
}
unsafe extern "C" fn native_dispose(handle: *mut c_void, cleanup: u64, out: *mut Buffer) -> i32 {
    let engine = handle.cast::<Engine>();
    (*engine).disposed += 1;
    let mut status = OK;
    for (origin, event) in std::mem::take(&mut (*engine).disposal_events) {
        let bytes = serde_json::to_vec(&event).unwrap();
        let mut result = Buffer::default();
        if ((*engine).resource.unwrap())(cleanup, origin, bytes.as_ptr(), bytes.len(), &mut result)
            != OK
        {
            status = RELEASED_ERROR;
        }
        callback::free(result);
    }
    *out = Buffer::default();
    status
}
unsafe extern "C" fn native_drop(handle: *mut c_void) {
    drop(Box::from_raw(handle.cast::<Engine>()));
}
static OPS: ComponentHostOps = ComponentHostOps {
    perry_component_driver_new: new_driver,
    perry_component_driver_submit: submit,
    perry_component_driver_poll: native_poll,
    perry_component_host_async_import_resolve: resolve,
    perry_component_host_dispose: native_dispose,
    perry_component_host_drop: native_drop,
    perry_component_host_buffer_drop: callback::free,
    ..unused::OPS
};
fn text(value: &str) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        value.as_ptr(),
        value.len() as u32,
    ) as i64)
}
fn array() -> f64 {
    crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64)
}
fn promise(value: f64) -> *mut crate::promise::Promise {
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}
fn call(component: f64, function: &str) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(component);
    let interface = scope.root_nanbox_f64(text(""));
    let function = scope.root_nanbox_f64(text(function));
    let args = scope.root_nanbox_f64(array());
    js_component_call(
        component.get_nanbox_f64(),
        interface.get_nanbox_f64(),
        function.get_nanbox_f64(),
        args.get_nanbox_f64(),
    )
}
extern "C" fn wait(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}
fn load(scope: &RuntimeHandleScope, imported: f64) -> f64 {
    let imported = scope.root_nanbox_f64(imported);
    let imports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(wait, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        closure.get_raw_mut_ptr(),
        0,
        imported.get_nanbox_f64(),
    );
    set_field(
        imports.get_nanbox_f64(),
        b"wait",
        crate::value::js_nanbox_pointer(
            closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let world = json!({"types":{},"imports":[{"name":"","functions":[{"name":"wait","async":true,"params":[],"result":{"kind":"u32"}}]}],
        "exports":[{"name":"","functions":[
        {"name":"run","async":true,"params":[],"result":{"kind":"u32"}},
        {"name":"sync","params":[],"result":{"kind":"u32"}}]}]});
    let contract = scope.root_nanbox_f64(text(&world.to_string()));
    let bytes = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::buffer::js_buffer_alloc(1, 0) as i64,
    ));
    js_component_load(
        bytes.get_nanbox_f64(),
        contract.get_nanbox_f64(),
        imports.get_nanbox_f64(),
        undefined(),
    )
}
fn engine(component: f64) -> *mut Engine {
    unsafe { (*context::get(component)).host.cast() }
}
fn finish_component(component: f64) {
    js_component_dispose(component);
    assert_eq!(poll::poll_components(), 1);
    assert_eq!(poll::has_active(), 0);
}

#[test]
fn public_driver_load_schedules_initialization_without_any_export_call() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let imported = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::promise::js_promise_new() as i64,
    ));
    let component = scope.root_nanbox_f64(load(&scope, imported.get_nanbox_f64()));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    assert!(enabled(component.get_nanbox_f64()));
    unsafe {
        (*engine(component.get_nanbox_f64()))
            .imports
            .push((0, true));
    }
    assert_eq!(poll::has_active(), 1);
    poll::poll_components();
    assert_eq!(unsafe { (*engine(component.get_nanbox_f64())).polls }, 1);
    crate::promise::js_promise_resolve(promise(imported.get_nanbox_f64()), 42.0);
    assert_eq!(
        poll::has_active(),
        1,
        "pending initialization imports keep the poll scheduled"
    );
    poll::poll_components();
    assert_eq!(
        unsafe { &(*engine(component.get_nanbox_f64())).resolutions },
        &[0]
    );
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

#[test]
fn public_calls_overlap_finish_individually_and_sync_calls_return_plain_values() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let first = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    let second = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![2];
        (*engine(component.get_nanbox_f64())).retained = vec![1, 2];
    }
    assert_eq!(poll::poll_components(), 0);
    assert_eq!(
        crate::promise::js_promise_state(promise(first.get_nanbox_f64())),
        0
    );
    assert_eq!(
        crate::promise::js_promise_result(promise(second.get_nanbox_f64())),
        20.0
    );
    assert_eq!(call(component.get_nanbox_f64(), "sync"), 30.0);
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![1];
        (*engine(component.get_nanbox_f64())).retained.clear();
    }
    assert_eq!(poll::poll_components(), 1);
    assert_eq!(
        crate::promise::js_promise_result(promise(first.get_nanbox_f64())),
        10.0
    );
    assert!(unsafe { (*state(component.get_nanbox_f64())).origins.is_empty() });
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

extern "C-unwind" fn context_import(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let expected = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let token = crate::closure::js_closure_get_capture_f64(closure, 1) as i64;
    let component = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
    let count = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 3));
    crate::gc::gc_collect_minor();
    assert_eq!(
        crate::async_context::get_store(token).unwrap().to_bits(),
        expected.get_nanbox_f64().to_bits()
    );
    assert_ne!(crate::async_hooks::execution_async_id_u64(), 0);
    assert!(crate::exception::catch_js_throw(|| call(component.get_nanbox_f64(), "sync")).is_err());
    assert_eq!(poll::poll_components(), 0);
    let index = crate::array::js_array_get_f64(array_pointer(count.get_nanbox_f64()).unwrap(), 0);
    crate::array::js_array_set_f64(
        array_pointer(count.get_nanbox_f64()).unwrap(),
        0,
        index + 1.0,
    );
    42.0
}
#[test]
fn returned_invocation_context_survives_public_promise_collection_and_later_callbacks() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let token = crate::async_context::AsyncLocalStoragePayload::default();
    let expected = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let count = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array_pointer(count.get_nanbox_f64()).unwrap(), 0, 0.0);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind context_import,0),
        4,
    ));
    for (index, value) in [
        expected.get_nanbox_f64(),
        token.token() as f64,
        component.get_nanbox_f64(),
        count.get_nanbox_f64(),
    ]
    .into_iter()
    .enumerate()
    {
        crate::closure::js_closure_set_capture_f64(closure.get_raw_mut_ptr(), index as u32, value);
    }
    set_field(
        context::imports(component.get_nanbox_f64()),
        b"wait",
        crate::value::js_nanbox_pointer(
            closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let weak = scope.root_nanbox_f64(undefined());
    {
        let temporary = RuntimeHandleScope::new();
        crate::async_context::set_store(token.token(), expected.get_nanbox_f64());
        let returned = temporary.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(returned.get_nanbox_f64()) as i64,
        ));
        unsafe {
            (*engine(component.get_nanbox_f64())).ready = vec![1];
            (*engine(component.get_nanbox_f64())).retained = vec![1];
        }
        poll::poll_components();
        assert_eq!(
            crate::promise::js_promise_state(promise(returned.get_nanbox_f64())),
            1
        );
    }
    crate::async_context::set_store(token.token(), 17.0);
    crate::gc::gc_collect_minor();
    crate::gc::js_gc_collect();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined()
    );
    assert!(
        unsafe {
            (*state(component.get_nanbox_f64()))
                .origins
                .contains_key(&1)
        },
        "retained origin must preserve its managed callback context"
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).imports = vec![(1, true)];
    }
    poll::poll_components();
    assert_eq!(
        crate::array::js_array_get_f64(array_pointer(count.get_nanbox_f64()).unwrap(), 0),
        1.0
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).retained.clear();
    }
    assert_eq!(poll::poll_components(), 1);
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

extern "C-unwind" fn dispose_before_settlement(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::gc::gc_collect_minor();
    js_component_dispose(component.get_nanbox_f64());
    undefined()
}
fn hook(
    scope: &RuntimeHandleScope,
    name: &[u8],
    info: *const crate::closure::JsFunctionInfo,
    captures: &[f64],
) -> f64 {
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        info,
        captures.len() as u32,
    ));
    for (index, value) in captures.iter().enumerate() {
        crate::closure::js_closure_set_capture_f64(
            callback.get_raw_mut_ptr(),
            index as u32,
            *value,
        );
    }
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(
        options.get_nanbox_f64(),
        name,
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let hook = crate::async_hooks::js_async_hooks_create_hook(options.get_nanbox_f64());
    crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(hook));
    hook
}
#[test]
fn nested_disposal_preserves_all_native_ready_results_before_trimming_origins() {
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
    let first = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    let second = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![1, 2];
    }
    let hook = scope.root_nanbox_f64(hook(
        &scope,
        b"before",
        crate::fn_info!(unwind dispose_before_settlement,1),
        &[component.get_nanbox_f64()],
    ));
    let result = crate::exception::catch_js_throw(|| poll::poll_components());
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert_eq!(result, Ok(1));
    for (promise_value, expected) in [
        (first.get_nanbox_f64(), 10.0),
        (second.get_nanbox_f64(), 20.0),
    ] {
        assert_eq!(crate::promise::js_promise_state(promise(promise_value)), 1);
        assert_eq!(
            crate::promise::js_promise_result(promise(promise_value)),
            expected
        );
    }
    assert_eq!(unsafe { (*engine(component.get_nanbox_f64())).disposed }, 1);
    assert!(unsafe { (*state(component.get_nanbox_f64())).origins.is_empty() });
    assert_eq!(poll::has_active(), 0);
}

#[test]
fn async_import_rejection_preserves_the_original_managed_error() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let imported = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::promise::js_promise_new() as i64,
    ));
    let error = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let component = scope.root_nanbox_f64(load(&scope, imported.get_nanbox_f64()));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let result = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    crate::promise::js_promise_mark_internally_handled(promise(result.get_nanbox_f64()));
    unsafe {
        (*engine(component.get_nanbox_f64())).imports = vec![(1, true)];
        (*engine(component.get_nanbox_f64())).retained = vec![1];
    }
    poll::poll_components();
    crate::promise::js_promise_reject(promise(imported.get_nanbox_f64()), error.get_nanbox_f64());
    crate::gc::gc_collect_minor();
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![1];
        (*engine(component.get_nanbox_f64())).retained.clear();
    }
    assert_eq!(poll::poll_components(), 1);
    assert_eq!(
        crate::promise::js_promise_state(promise(result.get_nanbox_f64())),
        2
    );
    assert_eq!(
        crate::promise::js_promise_result(promise(result.get_nanbox_f64())).to_bits(),
        error.get_nanbox_f64().to_bits()
    );
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

#[test]
fn saturated_admission_keeps_collector_identity_until_a_later_poll_accepts_it() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    for _ in 0..1024 {
        call(component.get_nanbox_f64(), "run");
    }
    let request = json!({"kind":"guest","token":"collected:stale"});
    unsafe {
        (*context::get(component.get_nanbox_f64()))
            .pending_collections
            .push(request.clone());
    }
    cleanup::drain(component.get_nanbox_f64());
    assert_eq!(
        unsafe { &(*context::get(component.get_nanbox_f64())).pending_collections },
        &[request]
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = (1..=1024).collect();
    }
    poll::poll_components();
    assert_eq!(
        unsafe {
            (*context::get(component.get_nanbox_f64()))
                .pending_collections
                .len()
        },
        1
    );
    assert_eq!(poll::poll_components(), 1);
    assert!(unsafe {
        (*context::get(component.get_nanbox_f64()))
            .pending_collections
            .is_empty()
    });
    assert_eq!(unsafe { (*engine(component.get_nanbox_f64())).next }, 1025);
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

extern "C" fn take_resource(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::closure::js_closure_set_capture_f64(closure.cast_mut(), 0, undefined());
    value.get_nanbox_f64()
}
#[test]
fn sync_import_results_remain_rooted_until_their_native_ownership_receipt() {
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
        let world = &mut (*context::get(component.get_nanbox_f64())).world;
        world["imports"][0]["functions"][0]["async"] = json!(false);
        world["imports"][0]["functions"][0]["result"] = json!({"kind":"own","value":"resource"});
    }
    let weak = scope.root_nanbox_f64(undefined());
    let original;
    {
        let temporary = RuntimeHandleScope::new();
        let resource = temporary.root_nanbox_f64(resource::wrap(
            component.get_nanbox_f64(),
            "guest:owned-response",
            true,
        ));
        original = resource.get_nanbox_f64().to_bits();
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(resource.get_nanbox_f64()) as i64,
        ));
        let callback = temporary.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(take_resource, 0),
            1,
        ));
        crate::closure::js_closure_set_capture_f64(
            callback.get_raw_mut_ptr(),
            0,
            resource.get_nanbox_f64(),
        );
        set_field(
            context::imports(component.get_nanbox_f64()),
            b"wait",
            crate::value::js_nanbox_pointer(
                callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
    }
    call(component.get_nanbox_f64(), "run");
    unsafe {
        (*engine(component.get_nanbox_f64())).imports = vec![(1, false)];
        (*engine(component.get_nanbox_f64())).retained = vec![1];
    }
    poll::poll_components();
    crate::gc::gc_collect_minor();
    let plans = records::get(
        origin_row(component.get_nanbox_f64(), 1),
        records::IMPORT_RESULTS,
    );
    let plan = records::row(plans, 0);
    assert!(
        !value::retention::is_empty(plan),
        "an import reply is not an ownership commit"
    );
    // Inspect the actual managed plan instead of strengthening the weak edge.
    let list = crate::array::js_array_get_f64(array_pointer(plan).unwrap(), 0);
    let pair = records::row(list, 0);
    assert_ne!(
        records::get(pair, 0).to_bits(),
        original,
        "the retained response wrapper must actually evacuate"
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).events = vec![(
            1,
            json!({"kind":"lower","created":[],"transferred":[],"guestTransferred":["guest:owned-response"],"asyncTransferred":[]}),
        )];
    }
    poll::poll_components();
    crate::gc::js_gc_collect();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "native ownership receipt must release the response wrapper"
    );
    finish_component(component.get_nanbox_f64());
}

extern "C-unwind" fn admit_before_settlement(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let holder = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    if JSValue::from_bits(records::get(holder.get_nanbox_f64(), 0).to_bits()).is_undefined() {
        let result = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
        records::set(holder.get_nanbox_f64(), 0, result.get_nanbox_f64());
    }
    undefined()
}
#[test]
fn calls_admitted_by_settlement_hooks_survive_the_older_retention_snapshot() {
    let _ops = host::TestOpsGuard::install(&OPS);
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(load(&scope, 42.0));
    let _lease = EngineLease(
        context::get(component.get_nanbox_f64()),
        engine(component.get_nanbox_f64()),
    );
    let first = scope.root_nanbox_f64(call(component.get_nanbox_f64(), "run"));
    let holder = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    records::set(holder.get_nanbox_f64(), 0, undefined());
    let hook = scope.root_nanbox_f64(hook(
        &scope,
        b"before",
        crate::fn_info!(unwind admit_before_settlement,1),
        &[component.get_nanbox_f64(), holder.get_nanbox_f64()],
    ));
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![1];
    }
    let outcome = crate::exception::catch_js_throw(|| poll::poll_components());
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert_eq!(outcome, Ok(0));
    assert_eq!(
        crate::promise::js_promise_state(promise(first.get_nanbox_f64())),
        1
    );
    assert!(unsafe {
        (*state(component.get_nanbox_f64()))
            .origins
            .contains_key(&2)
    });
    let second = scope.root_nanbox_f64(records::get(holder.get_nanbox_f64(), 0));
    assert_eq!(
        crate::promise::js_promise_state(promise(second.get_nanbox_f64())),
        0
    );
    unsafe {
        (*engine(component.get_nanbox_f64())).ready = vec![2];
    }
    assert_eq!(poll::poll_components(), 1);
    assert_eq!(
        crate::promise::js_promise_result(promise(second.get_nanbox_f64())),
        20.0
    );
    poll::schedule(component.get_nanbox_f64());
    finish_component(component.get_nanbox_f64());
}

extern "C-unwind" fn dispose_on_init(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) == "PROMISE" {
        let scope = RuntimeHandleScope::new();
        let component =
            scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
        crate::gc::gc_collect_minor();
        js_component_dispose(component.get_nanbox_f64());
    }
    undefined()
}
#[test]
fn public_submission_rechecks_disposal_after_the_promise_init_hook() {
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
    let hook = scope.root_nanbox_f64(hook(
        &scope,
        b"init",
        crate::fn_info!(unwind dispose_on_init,4),
        &[component.get_nanbox_f64()],
    ));
    let result = crate::exception::catch_js_throw(|| call(component.get_nanbox_f64(), "run"));
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert!(result.is_err());
    assert_eq!(
        unsafe { (*engine(component.get_nanbox_f64())).next },
        0,
        "disposal in init must prevent native admission"
    );
    assert!(unsafe { (*state(component.get_nanbox_f64())).origins.is_empty() });
    assert_eq!(poll::poll_components(), 1);
    assert_eq!(poll::has_active(), 0);
}
