//! The native transport has separate real-socket tests. This controlled engine
//! makes moving GC, pump reentry and simultaneous primary/cleanup errors exact.
mod p3;
use super::*;
use perry_abi::component_host::{ComponentHostOps, ERROR};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

// The controlled host callbacks run on the test's owning thread.
per_test_global! {
    static READY: AtomicBool = AtomicBool::new(false);
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    static FAIL: AtomicBool = AtomicBool::new(false);
}

mod unused {
    #![allow(dead_code)]
    macro_rules! entries {
        ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
            $(pub unsafe extern "C" fn $name($($arg:$ty),*) -> $ret {
                let _ = ($($arg),*);
                Default::default()
            })*
            pub const OPS: perry_abi::component_host::ComponentHostOps = perry_abi::component_host::ComponentHostOps {
                abi_version: perry_abi::component_host::ABI_VERSION,
                size: std::mem::size_of::<perry_abi::component_host::ComponentHostOps>(),
                $($name,)*
            };
        };
    }
    perry_abi::component_host_entry_points!(entries);
}
struct Exchange {
    alive: Arc<AtomicBool>,
    invoked: bool,
}
unsafe fn reply(out: *mut Buffer, bytes: &[u8]) {
    let bytes = bytes.to_vec().into_boxed_slice();
    *out = Buffer {
        len: bytes.len(),
        data: Box::into_raw(bytes).cast(),
    };
}
unsafe extern "C" fn new_server(_: *const u8, _: usize, out: *mut Buffer) -> *mut c_void {
    reply(out, br#""127.0.0.1:1234""#);
    Box::into_raw(Box::new(Arc::new(AtomicBool::new(true)))).cast()
}
unsafe extern "C" fn drop_server(server: *mut c_void) {
    Box::from_raw(server.cast::<Arc<AtomicBool>>()).store(false, Ordering::Release);
}
unsafe extern "C" fn accept(server: *mut c_void, _: *mut Buffer) -> *mut c_void {
    Box::into_raw(Box::new(Exchange {
        alive: (*server.cast::<Arc<AtomicBool>>()).clone(),
        invoked: false,
    }))
    .cast()
}
unsafe extern "C" fn poll(
    pointer: *mut c_void,
    _: Option<perry_abi::component_host::WakeCallback>,
    _: u64,
    out: *mut Buffer,
) -> i32 {
    let exchange = &*pointer.cast::<Exchange>();
    if !exchange.alive.load(Ordering::Acquire) {
        reply(out, b"listener closed");
        ERROR
    } else if exchange.invoked {
        OK
    } else if READY.load(Ordering::Acquire) {
        HTTP_READY
    } else {
        HTTP_PENDING
    }
}
unsafe extern "C" fn handle(
    _: *mut c_void,
    pointer: *mut c_void,
    _: *const u8,
    _: usize,
    primary: u64,
    retirement: u64,
    out: *mut Buffer,
) -> i32 {
    (*pointer.cast::<Exchange>()).invoked = true;
    CALLS.fetch_add(1, Ordering::Relaxed);
    let mut output = Buffer::default();
    let status = callback::invoke(
        primary,
        b"".as_ptr(),
        0,
        b"handler".as_ptr(),
        7,
        b"[]".as_ptr(),
        2,
        &mut output,
    );
    callback::free(output);
    if status != OK {
        (*(retirement as usize as *const callback::Frame<'_>)).capture(99.0);
        reply(out, b"guest trapped; cleanup also failed");
        RELEASED_ERROR
    } else {
        OK
    }
}
unsafe extern "C" fn drop_operation(pointer: *mut c_void) {
    drop(Box::from_raw(pointer.cast::<Exchange>()));
    DROPS.fetch_add(1, Ordering::Relaxed);
}
static OPS: ComponentHostOps = ComponentHostOps {
    perry_component_http_server_new: new_server,
    perry_component_http_server_drop: drop_server,
    perry_component_http_accept: accept,
    perry_component_http_poll: poll,
    perry_component_http_handle: handle,
    perry_component_http_drop: drop_operation,
    perry_component_host_buffer_drop: callback::free,
    ..unused::OPS
};

extern "C-unwind" fn handler(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let expected = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    assert_eq!(pump(), 0, "nested pump must not enter another Store");
    let token = crate::closure::js_closure_get_capture_f64(closure, 1) as i64;
    crate::gc::gc_collect_minor();
    assert_eq!(
        crate::async_context::get_store(token).unwrap().to_bits(),
        expected.get_nanbox_f64().to_bits()
    );
    assert_ne!(crate::async_hooks::execution_async_id_u64(), 0);
    if FAIL.load(Ordering::Acquire) {
        crate::exception::js_throw(expected.get_nanbox_f64());
    }
    undefined()
}

fn reset() {
    assert_eq!(has_active(), 0);
    READY.store(false, Ordering::Release);
    FAIL.store(false, Ordering::Release);
    CALLS.store(0, Ordering::Release);
    DROPS.store(0, Ordering::Release);
}

#[test]
fn pending_http_owns_movable_js_state_and_preserves_submission_context_and_primary_error() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _engine = host::TestOpsGuard::install(&OPS);
    reset();
    let scope = RuntimeHandleScope::new();
    let token = crate::async_context::AsyncLocalStoragePayload::default();
    let expected = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let imports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind handler, 0),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        expected.get_nanbox_f64(),
    );
    crate::closure::js_closure_set_capture_f64(callback.get_raw_mut_ptr(), 1, token.token() as f64);
    set_field(
        imports.get_nanbox_f64(),
        b"handler",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let world = serde_json::json!({"types":{},"imports":[{"name":"","functions":[{"name":"handler","params":[],"result":null}]}]});
    let component = scope.root_nanbox_f64(context::publish(world, imports.get_nanbox_f64()));
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::dangling_mut::<u8>().cast();
    }
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let server = scope.root_nanbox_f64(js_component_http_server_new(options.get_nanbox_f64()));
    assert_eq!(
        string(field(server.get_nanbox_f64(), b"address")),
        "127.0.0.1:1234"
    );
    for fail_request in [false, true] {
        crate::async_context::set_store(token.token(), expected.get_nanbox_f64());
        let promise = scope.root_nanbox_f64(js_component_http_handle(
            server.get_nanbox_f64(),
            component.get_nanbox_f64(),
            crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                b"incoming".as_ptr(),
                8,
            ) as i64),
        ));
        crate::promise::js_promise_mark_internally_handled(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
        );
        crate::async_context::set_store(token.token(), 17.0);
        assert_eq!(pump(), 0);
        let link = PENDING.with(|queue| queue.borrow()[0]);
        let before = unsafe { payload::link_event_owner(link) }
            .unwrap()
            .to_bits();
        crate::gc::gc_collect_minor();
        let after = unsafe { payload::link_event_owner(link) }
            .unwrap()
            .to_bits();
        assert_ne!(before, after, "operation owner must actually evacuate");
        FAIL.store(fail_request, Ordering::Release);
        READY.store(true, Ordering::Release);
        assert_eq!(pump(), 0);
        assert_eq!(crate::async_context::get_store(token.token()), Some(17.0));
        assert_eq!(pump(), 1);
        let pointer = JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut();
        assert_eq!(
            crate::promise::js_promise_state(pointer),
            if fail_request { 2 } else { 1 }
        );
        if fail_request {
            assert_eq!(
                crate::promise::js_promise_result(pointer).to_bits(),
                expected.get_nanbox_f64().to_bits(),
                "cleanup error must not replace primary object identity"
            );
        }
        READY.store(false, Ordering::Release);
    }
    assert_eq!(CALLS.load(Ordering::Acquire), 2);
    assert_eq!(DROPS.load(Ordering::Acquire), 2);
    assert_eq!(has_active(), 0);
    js_component_http_server_close(server.get_nanbox_f64());
    js_component_http_server_close(server.get_nanbox_f64());
    crate::async_context::js_async_context_als_clear(token.token());
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::null_mut();
    }
}

#[test]
fn closing_listener_cancels_pending_promise_without_entering_guest() {
    cancel_pending(false);
}

#[test]
fn disposing_component_cancels_pending_promise_without_waiting_for_a_client() {
    cancel_pending(true);
}

fn cancel_pending(dispose: bool) {
    let _engine = host::TestOpsGuard::install(&OPS);
    reset();
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(context::publish(serde_json::json!({}), undefined()));
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::dangling_mut::<u8>().cast();
    }
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let server = scope.root_nanbox_f64(js_component_http_server_new(options.get_nanbox_f64()));
    let interface =
        scope.root_string_ptr(crate::string::js_string_from_bytes(b"incoming".as_ptr(), 8));
    let promise = scope.root_nanbox_f64(js_component_http_handle(
        server.get_nanbox_f64(),
        component.get_nanbox_f64(),
        crate::value::js_nanbox_string(
            interface.get_raw_const_ptr::<crate::string::StringHeader>() as i64,
        ),
    ));
    crate::promise::js_promise_mark_internally_handled(
        JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut(),
    );
    retire(crate::agent::current_agent().wrapping_add(1));
    assert_eq!(
        has_active(),
        1,
        "foreign agent retirement cannot clear this queue"
    );
    if dispose {
        context::finish_disposal(component.get_nanbox_f64(), OK);
    } else {
        js_component_http_server_close(server.get_nanbox_f64());
    }
    assert_eq!(pump(), 1);
    assert_eq!(
        crate::promise::js_promise_state(
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut()
        ),
        2
    );
    assert_eq!(CALLS.load(Ordering::Acquire), 0);
    assert_eq!(DROPS.load(Ordering::Acquire), 1);
    assert_eq!(has_active(), 0);
    js_component_http_server_close(server.get_nanbox_f64());
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::null_mut();
    }
}

extern "C-unwind" fn throwing_hook(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
) -> f64 {
    crate::gc::gc_collect_minor();
    crate::exception::js_throw(99.0)
}

#[test]
fn promise_producer_scope_restores_context_when_lifecycle_hooks_collect_and_throw() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let token = crate::async_context::AsyncLocalStoragePayload::default();
    let outside_id = crate::async_hooks::execution_async_id_u64();
    for phase in [b"before".as_slice(), b"after".as_slice()] {
        crate::async_context::set_store(token.token(), 11.0);
        let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::set_promise_callback_context(promise.get_raw_mut_ptr());
        crate::async_context::set_store(token.token(), 22.0);
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
        let hook = scope.root_nanbox_f64(crate::async_hooks::js_async_hooks_create_hook(
            options.get_nanbox_f64(),
        ));
        crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(
            hook.get_nanbox_f64(),
        ));
        let ran = Cell::new(false);
        let result = unsafe {
            crate::async_hooks::try_run_promise_scope(promise.get_raw_mut_ptr(), || {
                ran.set(true);
                assert_eq!(crate::async_context::get_store(token.token()), Some(11.0));
                crate::exception::js_throw(41.0)
            })
        };
        crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
            hook.get_nanbox_f64(),
        ));
        assert_eq!(ran.get(), phase == b"after");
        assert_eq!(result, Err(if phase == b"before" { 99.0 } else { 41.0 }));
        assert_eq!(crate::async_context::get_store(token.token()), Some(22.0));
        assert_eq!(crate::async_hooks::execution_async_id_u64(), outside_id);
    }
    crate::async_context::js_async_context_als_clear(token.token());
}

extern "C" fn close_during_init(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    kind: f64,
    _: f64,
    _: f64,
) -> f64 {
    if string(kind) == "PROMISE" {
        js_component_http_server_close(crate::closure::js_closure_get_capture_f64(closure, 0));
        crate::gc::gc_collect_minor();
    }
    undefined()
}

#[test]
fn promise_init_hook_may_collect_and_close_listener_before_accept_is_created() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let _engine = host::TestOpsGuard::install(&OPS);
    reset();
    let scope = RuntimeHandleScope::new();
    let component = scope.root_nanbox_f64(context::publish(serde_json::json!({}), undefined()));
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::dangling_mut::<u8>().cast();
    }
    let options = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let server = scope.root_nanbox_f64(js_component_http_server_new(options.get_nanbox_f64()));
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(close_during_init, 4),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        server.get_nanbox_f64(),
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
    let interface =
        scope.root_string_ptr(crate::string::js_string_from_bytes(b"incoming".as_ptr(), 8));
    let result = crate::exception::catch_js_throw(|| {
        js_component_http_handle(
            server.get_nanbox_f64(),
            component.get_nanbox_f64(),
            crate::value::js_nanbox_string(
                interface.get_raw_const_ptr::<crate::string::StringHeader>() as i64,
            ),
        )
    });
    let error =
        scope.root_nanbox_f64(result.expect_err("closed listener must reject before accept"));
    crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
        hook.get_nanbox_f64(),
    ));
    assert_eq!(
        string(crate::value::js_nanbox_string(
            crate::error::js_error_get_message(
                JSValue::from_bits(error.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::error::ErrorHeader>()
                    .cast_mut(),
            ) as i64
        )),
        "Expected an open ComponentHttpServer"
    );
    assert_eq!(CALLS.load(Ordering::Acquire), 0);
    assert_eq!(
        DROPS.load(Ordering::Acquire),
        0,
        "no native accept was allocated"
    );
    assert_eq!(has_active(), 0);
    unsafe {
        (*context::get(component.get_nanbox_f64())).host = std::ptr::null_mut();
    }
}
