//! FFI interop functions for calling between native code and JavaScript
//!
//! These functions are called from compiled native code to interact with
//! JavaScript modules loaded in the V8 runtime.

use crate::bridge::{
    capture_export_snapshot_intrinsics, fixup_native_for_v8, get_handle_id, get_js_handle,
    is_js_handle, make_js_handle_value, native_to_v8, release_js_handle, store_js_handle,
    v8_to_native, v8_to_native_export_value, v8_to_native_metadata_target,
    v8_to_native_metadata_value,
};
use crate::{
    ensure_runtime_initialized, get_tokio_runtime, with_runtime, JsRuntimeState, JS_RUNTIME,
};
use deno_core::v8;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::ffi::{c_char, CStr};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Once;
use std::task::{Context as TaskContext, Poll, RawWaker, RawWakerVTable, Waker};

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

struct ForeignPromiseAdapter {
    handle_id: u64,
    native_promise: *mut perry_runtime::promise::Promise,
}

thread_local! {
    static FOREIGN_PROMISE_ADAPTERS: RefCell<Vec<ForeignPromiseAdapter>> = const { RefCell::new(Vec::new()) };
    static PENDING_JSRUNTIME_TICKS: RefCell<Vec<v8::Global<v8::PromiseResolver>>> = const { RefCell::new(Vec::new()) };
}

static JSRUNTIME_PROFILE_ENABLED: AtomicBool = AtomicBool::new(false);
static JSRUNTIME_PROFILE_REG: Once = Once::new();
static JSRUNTIME_PUMP_TICKS: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ADAPTERS_CREATED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ADAPTERS_RESOLVED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ADAPTERS_REJECTED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_MODULE_EVALS_STARTED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_MODULE_EVALS_RESOLVED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_MODULE_EVALS_REJECTED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_BLOCKING_MODULE_EVALS: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_LEGACY_BLOCKING_AWAITS: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_HANDLES_STORED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_HANDLES_RELEASED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_V8_ENTRIES_TOTAL: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_RUNTIME_INIT: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_RUNTIME_SHUTDOWN: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_MODULE_LOAD: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_EXPORT_GET: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_FUNCTION_CALL: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_V8_EXPORT_CALL: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_METHOD_CALL: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_VALUE_CALL: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_ARRAY_GET: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_ARRAY_LENGTH: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_OBJECT_PROPERTY_GET: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_HANDLE_TO_STRING: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_PROPERTY_SET: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_NEW_INSTANCE: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_NEW_FROM_HANDLE: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_CALLBACK_CREATE: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_NATIVE_FUNCTION_REGISTER: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_CALLBACK_INVOKE: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_NATIVE_MODULE_PROPERTY_LOAD: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_TYPEOF_PROBE: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_HANDLE_CONSTRUCTOR: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_SHOULD_USE_RUNTIME: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_NATIVE_PROMISE_RESOLVE: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_NATIVE_PROMISE_REJECT: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_FOREIGN_PROMISE_ADAPTER: AtomicU64 = AtomicU64::new(0);
static JSRUNTIME_ENTRY_LEGACY_BLOCKING_AWAIT: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn js_register_jsruntime_pump(f: extern "C" fn() -> i32);
    fn js_register_jsruntime_has_active(f: extern "C" fn() -> i32);
}

#[derive(Clone, Copy)]
pub(crate) enum V8EntryKind {
    RuntimeInit,
    RuntimeShutdown,
    ModuleLoad,
    ExportGet,
    FunctionCall,
    V8ExportCall,
    MethodCall,
    ValueCall,
    ArrayGet,
    ArrayLength,
    ObjectPropertyGet,
    HandleToString,
    PropertySet,
    NewInstance,
    NewFromHandle,
    CallbackCreate,
    NativeFunctionRegister,
    CallbackInvoke,
    NativeModulePropertyLoad,
    TypeofProbe,
    HandleConstructor,
    ShouldUseRuntime,
    NativePromiseResolve,
    NativePromiseReject,
    ForeignPromiseAdapter,
    LegacyBlockingAwait,
}

fn jsruntime_profile_enabled() -> bool {
    JSRUNTIME_PROFILE_ENABLED.load(Ordering::Relaxed)
}

fn bump_jsruntime(counter: &AtomicU64) {
    if jsruntime_profile_enabled() {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn bump_v8_entry(kind: V8EntryKind) {
    jsruntime_profile_register();
    bump_jsruntime(&JSRUNTIME_V8_ENTRIES_TOTAL);
    bump_jsruntime(match kind {
        V8EntryKind::RuntimeInit => &JSRUNTIME_ENTRY_RUNTIME_INIT,
        V8EntryKind::RuntimeShutdown => &JSRUNTIME_ENTRY_RUNTIME_SHUTDOWN,
        V8EntryKind::ModuleLoad => &JSRUNTIME_ENTRY_MODULE_LOAD,
        V8EntryKind::ExportGet => &JSRUNTIME_ENTRY_EXPORT_GET,
        V8EntryKind::FunctionCall => &JSRUNTIME_ENTRY_FUNCTION_CALL,
        V8EntryKind::V8ExportCall => &JSRUNTIME_ENTRY_V8_EXPORT_CALL,
        V8EntryKind::MethodCall => &JSRUNTIME_ENTRY_METHOD_CALL,
        V8EntryKind::ValueCall => &JSRUNTIME_ENTRY_VALUE_CALL,
        V8EntryKind::ArrayGet => &JSRUNTIME_ENTRY_ARRAY_GET,
        V8EntryKind::ArrayLength => &JSRUNTIME_ENTRY_ARRAY_LENGTH,
        V8EntryKind::ObjectPropertyGet => &JSRUNTIME_ENTRY_OBJECT_PROPERTY_GET,
        V8EntryKind::HandleToString => &JSRUNTIME_ENTRY_HANDLE_TO_STRING,
        V8EntryKind::PropertySet => &JSRUNTIME_ENTRY_PROPERTY_SET,
        V8EntryKind::NewInstance => &JSRUNTIME_ENTRY_NEW_INSTANCE,
        V8EntryKind::NewFromHandle => &JSRUNTIME_ENTRY_NEW_FROM_HANDLE,
        V8EntryKind::CallbackCreate => &JSRUNTIME_ENTRY_CALLBACK_CREATE,
        V8EntryKind::NativeFunctionRegister => &JSRUNTIME_ENTRY_NATIVE_FUNCTION_REGISTER,
        V8EntryKind::CallbackInvoke => &JSRUNTIME_ENTRY_CALLBACK_INVOKE,
        V8EntryKind::NativeModulePropertyLoad => &JSRUNTIME_ENTRY_NATIVE_MODULE_PROPERTY_LOAD,
        V8EntryKind::TypeofProbe => &JSRUNTIME_ENTRY_TYPEOF_PROBE,
        V8EntryKind::HandleConstructor => &JSRUNTIME_ENTRY_HANDLE_CONSTRUCTOR,
        V8EntryKind::ShouldUseRuntime => &JSRUNTIME_ENTRY_SHOULD_USE_RUNTIME,
        V8EntryKind::NativePromiseResolve => &JSRUNTIME_ENTRY_NATIVE_PROMISE_RESOLVE,
        V8EntryKind::NativePromiseReject => &JSRUNTIME_ENTRY_NATIVE_PROMISE_REJECT,
        V8EntryKind::ForeignPromiseAdapter => &JSRUNTIME_ENTRY_FOREIGN_PROMISE_ADAPTER,
        V8EntryKind::LegacyBlockingAwait => &JSRUNTIME_ENTRY_LEGACY_BLOCKING_AWAIT,
    });
}

pub(crate) fn bump_js_handle_stored() {
    jsruntime_profile_register();
    bump_jsruntime(&JSRUNTIME_HANDLES_STORED);
}

pub(crate) fn bump_js_handle_released() {
    jsruntime_profile_register();
    bump_jsruntime(&JSRUNTIME_HANDLES_RELEASED);
}

extern "C" fn jsruntime_profile_atexit() {
    if std::env::var_os("PERRY_JSRUNTIME_PROFILE").is_none() {
        return;
    }
    let handles_stored = JSRUNTIME_HANDLES_STORED.load(Ordering::Relaxed);
    let handles_released = JSRUNTIME_HANDLES_RELEASED.load(Ordering::Relaxed);
    let handles_retained = handles_stored.saturating_sub(handles_released);
    let foreign_promise_handles_retained = JSRUNTIME_ADAPTERS_CREATED
        .load(Ordering::Relaxed)
        .saturating_sub(JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED.load(Ordering::Relaxed));
    eprintln!(
        "[jsruntime-profile] pump_ticks={} adapters_created={} adapters_resolved={} adapters_rejected={} module_evals_started={} module_evals_resolved={} module_evals_rejected={} blocking_module_evals={} legacy_blocking_awaits={} handles_stored={} handles_released={} handles_retained={} foreign_promise_handles_released={} foreign_promise_handles_retained={} v8_entries_total={} runtime_inits={} runtime_shutdowns={} module_loads={} export_gets={} function_calls={} v8_export_calls={} method_calls={} value_calls={} array_gets={} array_lengths={} object_property_gets={} handle_to_strings={} property_sets={} new_instances={} new_from_handles={} callback_creates={} native_function_registers={} callback_invokes={} native_module_property_loads={} typeof_probes={} handle_constructors={} should_use_runtime={} native_promise_resolves={} native_promise_rejects={} foreign_promise_adapters={} legacy_blocking_await_entries={}",
        JSRUNTIME_PUMP_TICKS.load(Ordering::Relaxed),
        JSRUNTIME_ADAPTERS_CREATED.load(Ordering::Relaxed),
        JSRUNTIME_ADAPTERS_RESOLVED.load(Ordering::Relaxed),
        JSRUNTIME_ADAPTERS_REJECTED.load(Ordering::Relaxed),
        JSRUNTIME_MODULE_EVALS_STARTED.load(Ordering::Relaxed),
        JSRUNTIME_MODULE_EVALS_RESOLVED.load(Ordering::Relaxed),
        JSRUNTIME_MODULE_EVALS_REJECTED.load(Ordering::Relaxed),
        JSRUNTIME_BLOCKING_MODULE_EVALS.load(Ordering::Relaxed),
        JSRUNTIME_LEGACY_BLOCKING_AWAITS.load(Ordering::Relaxed),
        handles_stored,
        handles_released,
        handles_retained,
        JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED.load(Ordering::Relaxed),
        foreign_promise_handles_retained,
        JSRUNTIME_V8_ENTRIES_TOTAL.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_RUNTIME_INIT.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_RUNTIME_SHUTDOWN.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_MODULE_LOAD.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_EXPORT_GET.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_FUNCTION_CALL.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_V8_EXPORT_CALL.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_METHOD_CALL.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_VALUE_CALL.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_ARRAY_GET.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_ARRAY_LENGTH.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_OBJECT_PROPERTY_GET.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_HANDLE_TO_STRING.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_PROPERTY_SET.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_NEW_INSTANCE.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_NEW_FROM_HANDLE.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_CALLBACK_CREATE.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_NATIVE_FUNCTION_REGISTER.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_CALLBACK_INVOKE.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_NATIVE_MODULE_PROPERTY_LOAD.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_TYPEOF_PROBE.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_HANDLE_CONSTRUCTOR.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_SHOULD_USE_RUNTIME.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_NATIVE_PROMISE_RESOLVE.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_NATIVE_PROMISE_REJECT.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_FOREIGN_PROMISE_ADAPTER.load(Ordering::Relaxed),
        JSRUNTIME_ENTRY_LEGACY_BLOCKING_AWAIT.load(Ordering::Relaxed),
    );
}

fn jsruntime_profile_register() {
    JSRUNTIME_PROFILE_REG.call_once(|| {
        let enabled = std::env::var_os("PERRY_JSRUNTIME_PROFILE").is_some();
        JSRUNTIME_PROFILE_ENABLED.store(enabled, Ordering::Relaxed);
        if enabled {
            unsafe {
                unsafe extern "C" {
                    fn atexit(cb: extern "C" fn()) -> i32;
                }
                atexit(jsruntime_profile_atexit);
            }
        }
    });
}

fn boxed_native_promise(promise: *mut perry_runtime::promise::Promise) -> f64 {
    f64::from_bits(POINTER_TAG | (promise as u64 & POINTER_MASK))
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

unsafe fn notifying_waker_clone(_: *const ()) -> RawWaker {
    notifying_raw_waker()
}

unsafe fn notifying_waker_wake(_: *const ()) {
    perry_runtime::event_pump::js_notify_main_thread();
}

unsafe fn notifying_waker_wake_by_ref(_: *const ()) {
    perry_runtime::event_pump::js_notify_main_thread();
}

unsafe fn notifying_waker_drop(_: *const ()) {}

static NOTIFYING_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    notifying_waker_clone,
    notifying_waker_wake,
    notifying_waker_wake_by_ref,
    notifying_waker_drop,
);

fn notifying_raw_waker() -> RawWaker {
    RawWaker::new(std::ptr::null(), &NOTIFYING_WAKER_VTABLE)
}

fn notifying_waker() -> Waker {
    unsafe { Waker::from_raw(notifying_raw_waker()) }
}

enum AdapterSettlement {
    Resolve(u64, *mut perry_runtime::promise::Promise, f64),
    Reject(u64, *mut perry_runtime::promise::Promise, f64),
}

fn collect_foreign_promise_settlements(state: &mut JsRuntimeState) -> Vec<AdapterSettlement> {
    deno_core::scope!(scope, &mut state.runtime);
    FOREIGN_PROMISE_ADAPTERS.with(|adapters| {
        let mut adapters = adapters.borrow_mut();
        let mut settlements = Vec::new();
        let mut i = 0;
        while i < adapters.len() {
            let adapter = &adapters[i];
            let settlement = match get_js_handle(scope, adapter.handle_id) {
                Some(v8_val) if v8_val.is_promise() => {
                    let promise = v8::Local::<v8::Promise>::try_from(v8_val).unwrap();
                    match promise.state() {
                        v8::PromiseState::Fulfilled => {
                            let result = promise.result(scope);
                            Some(AdapterSettlement::Resolve(
                                adapter.handle_id,
                                adapter.native_promise,
                                v8_to_native(scope, result),
                            ))
                        }
                        v8::PromiseState::Rejected => {
                            let reason = promise.result(scope);
                            Some(AdapterSettlement::Reject(
                                adapter.handle_id,
                                adapter.native_promise,
                                v8_to_native(scope, reason),
                            ))
                        }
                        v8::PromiseState::Pending => None,
                    }
                }
                Some(v8_val) => Some(AdapterSettlement::Resolve(
                    adapter.handle_id,
                    adapter.native_promise,
                    v8_to_native(scope, v8_val),
                )),
                None => Some(AdapterSettlement::Reject(
                    adapter.handle_id,
                    adapter.native_promise,
                    undefined_value(),
                )),
            };

            if let Some(settlement) = settlement {
                settlements.push(settlement);
                adapters.remove(i);
            } else {
                i += 1;
            }
        }
        settlements
    })
}

fn settle_foreign_promise_adapters(state: &mut JsRuntimeState) -> i32 {
    let settlements = collect_foreign_promise_settlements(state);
    let count = settlements.len() as i32;
    for settlement in settlements {
        match settlement {
            AdapterSettlement::Resolve(handle_id, promise, value) => {
                if release_js_handle(handle_id) {
                    bump_jsruntime(&JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED);
                }
                bump_jsruntime(&JSRUNTIME_ADAPTERS_RESOLVED);
                perry_runtime::promise::js_promise_resolve(promise, value);
            }
            AdapterSettlement::Reject(handle_id, promise, reason) => {
                if release_js_handle(handle_id) {
                    bump_jsruntime(&JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED);
                }
                bump_jsruntime(&JSRUNTIME_ADAPTERS_REJECTED);
                perry_runtime::promise::js_promise_reject(promise, reason);
            }
        }
    }
    count
}

fn poll_v8_event_loop_once(state: &mut JsRuntimeState) -> i32 {
    let waker = notifying_waker();
    let mut cx = TaskContext::from_waker(&waker);
    match state.runtime.poll_event_loop(&mut cx, Default::default()) {
        Poll::Ready(Ok(())) => {
            // V8 event loop drained — clear the pending flag so the outer
            // loop can exit (assuming no other source — timers, stdlib,
            // http servers — still keeps it alive).
            state.last_poll_was_pending = false;
            0
        }
        Poll::Ready(Err(e)) => {
            state.last_poll_was_pending = false;
            eprintln!("[jsruntime_pump] event loop error: {}", e);
            1
        }
        Poll::Pending => {
            // Refed async op / dyn import / microtask / promise event
            // outstanding. `jsruntime_has_active_handles` reads this flag
            // to keep the outer event loop alive until the op resolves.
            state.last_poll_was_pending = true;
            0
        }
    }
}

fn resolve_pending_jsruntime_ticks(state: &mut JsRuntimeState) -> i32 {
    let resolvers =
        PENDING_JSRUNTIME_TICKS.with(|ticks| ticks.borrow_mut().drain(..).collect::<Vec<_>>());
    if resolvers.is_empty() {
        return 0;
    }

    let count = resolvers.len() as i32;
    deno_core::scope!(scope, &mut state.runtime);
    let undefined = v8::undefined(scope).into();
    for resolver in resolvers {
        let resolver = v8::Local::new(scope, resolver);
        let _ = resolver.resolve(scope, undefined);
    }
    scope.perform_microtask_checkpoint();
    count
}

fn poll_pending_module_evaluations(state: &mut JsRuntimeState) -> i32 {
    let waker = notifying_waker();
    let mut cx = TaskContext::from_waker(&waker);
    let mut completed = Vec::new();

    for (module_id, pending) in state.pending_module_evaluations.iter_mut() {
        match pending.future.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(())) => {
                completed.push((
                    *module_id,
                    pending.canonical_path.display().to_string(),
                    None,
                ));
            }
            Poll::Ready(Err(e)) => {
                completed.push((
                    *module_id,
                    pending.canonical_path.display().to_string(),
                    Some(e.to_string()),
                ));
            }
            Poll::Pending => {}
        }
    }

    let count = completed.len() as i32;
    for (module_id, path, error) in completed {
        state.pending_module_evaluations.remove(&module_id);
        match error {
            Some(error) => {
                bump_jsruntime(&JSRUNTIME_MODULE_EVALS_REJECTED);
                eprintln!(
                    "[jsruntime_pump] module evaluation error for '{}': {}",
                    path, error
                );
            }
            None => {
                bump_jsruntime(&JSRUNTIME_MODULE_EVALS_RESOLVED);
            }
        }
    }
    count
}

extern "C" fn jsruntime_process_pending() -> i32 {
    jsruntime_profile_register();
    bump_jsruntime(&JSRUNTIME_PUMP_TICKS);
    // Enter the shared tokio runtime so async ops (e.g. the V8-fallback
    // `op_perry_http_*` listener) that touch `tokio::net` / `tokio::spawn`
    // can run inside a reactor context. Without this guard, polling an
    // async op that does `TcpListener::bind(...)` panics with "there is
    // no reactor running".
    let tokio_rt = crate::get_tokio_runtime();
    let _enter = tokio_rt.enter();
    with_runtime(|state| {
        let mut ran = poll_v8_event_loop_once(state);
        let resolved_ticks = resolve_pending_jsruntime_ticks(state);
        ran += resolved_ticks;
        if resolved_ticks > 0 {
            ran += poll_v8_event_loop_once(state);
        }
        ran += poll_pending_module_evaluations(state);
        ran += settle_foreign_promise_adapters(state);
        ran
    })
}

extern "C" fn jsruntime_has_active_handles() -> i32 {
    let has_foreign_adapters =
        FOREIGN_PROMISE_ADAPTERS.with(|adapters| !adapters.borrow().is_empty());
    let has_pending_ticks = PENDING_JSRUNTIME_TICKS.with(|ticks| !ticks.borrow().is_empty());
    let has_module_evaluations = JS_RUNTIME.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|state| !state.pending_module_evaluations.is_empty())
    });
    // `last_poll_was_pending` is set by `poll_v8_event_loop_once` whenever
    // deno_core returns `Poll::Pending` — i.e. a refed async op / dyn
    // import / microtask / promise event is still in flight. Without this
    // gate, a top-level `await op_perry_http_listen(port)` (or any other
    // async op invoked from module init) returns to the codegen-emitted
    // outer event loop while its body is still suspended on a tokio
    // worker; the header check then sees no other active source and
    // exits before the op resolves and the listening callback can fire.
    // Pairs with the express smoke at #997.
    let has_pending_v8 = JS_RUNTIME.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|state| state.last_poll_was_pending)
    });
    // Keep the program alive while any V8-fallback `http.createServer`
    // is still listening — without this the outer event loop exits
    // immediately after `server.listen(...)` resolves and the accept
    // loop's tokio task is dropped before serving any requests.
    let has_http_servers = crate::ops::perry_http_active_count() > 0;
    if has_foreign_adapters
        || has_pending_ticks
        || has_module_evaluations
        || has_http_servers
        || has_pending_v8
    {
        1
    } else {
        0
    }
}

/// Convert a NaN-boxed f64 to a V8 value, returning None if the conversion fails
/// This is specifically for cases where we need to handle the error explicitly
fn nanbox_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: f64,
) -> Option<v8::Local<'s, v8::Value>> {
    // Check if it's a JS handle first
    if is_js_handle(value) {
        if let Some(handle_id) = get_handle_id(value) {
            return get_js_handle(scope, handle_id);
        }
        return None;
    }
    // Use the standard conversion for other values
    Some(native_to_v8(scope, value))
}

/// Initialize the JavaScript runtime
/// Must be called once before any other jsruntime functions
#[no_mangle]
pub extern "C" fn js_runtime_init() {
    jsruntime_profile_register();
    bump_v8_entry(V8EntryKind::RuntimeInit);
    // Force initialization of the Tokio runtime
    let _ = get_tokio_runtime();
    // Force initialization of the JS runtime on this thread
    ensure_runtime_initialized();

    // Register JS handle functions with perry-runtime so the unified functions can use them
    perry_runtime::js_set_handle_array_get(js_handle_array_get);
    perry_runtime::js_set_handle_array_length(js_handle_array_length);
    perry_runtime::js_set_handle_object_get_property(js_handle_object_get_property);
    perry_runtime::js_set_handle_to_string(js_handle_to_string);
    perry_runtime::js_set_handle_call_method(js_call_method);
    perry_runtime::js_set_native_module_js_loader(native_module_js_property_loader);
    perry_runtime::js_set_new_from_handle_v8(js_new_from_handle_v8_impl);
    perry_runtime::js_set_handle_typeof(js_handle_typeof);
    perry_runtime::promise::js_register_foreign_promise_adapter(js_await_any_promise);
    unsafe {
        js_register_jsruntime_pump(jsruntime_process_pending);
        js_register_jsruntime_has_active(jsruntime_has_active_handles);
    }

    with_runtime(install_reflect_metadata_bridge);
    with_runtime(capture_intrinsics_for_export_snapshots);
}

fn capture_intrinsics_for_export_snapshots(state: &mut JsRuntimeState) {
    deno_core::scope!(scope, &mut state.runtime);
    capture_export_snapshot_intrinsics(scope);
}

fn install_reflect_metadata_bridge(state: &mut JsRuntimeState) {
    deno_core::scope!(scope, &mut state.runtime);
    let global = scope.get_current_context().global(scope);

    macro_rules! define_global_function {
        ($name:literal, $callback:ident) => {
            if let (Some(key), Some(function)) = (
                v8::String::new(scope, $name),
                v8::Function::builder($callback).build(scope),
            ) {
                global.set(scope, key.into(), function.into());
            }
        };
    }

    define_global_function!(
        "__perryReflectDefineMetadata",
        reflect_define_metadata_bridge
    );
    define_global_function!("__perryReflectGetMetadata", reflect_get_metadata_bridge);
    define_global_function!(
        "__perryReflectGetOwnMetadata",
        reflect_get_own_metadata_bridge
    );
    define_global_function!("__perryReflectHasMetadata", reflect_has_metadata_bridge);
    define_global_function!(
        "__perryReflectHasOwnMetadata",
        reflect_has_own_metadata_bridge
    );
    define_global_function!(
        "__perryReflectGetMetadataKeys",
        reflect_get_metadata_keys_bridge
    );
    define_global_function!(
        "__perryReflectGetOwnMetadataKeys",
        reflect_get_own_metadata_keys_bridge
    );
    define_global_function!(
        "__perryReflectDeleteMetadata",
        reflect_delete_metadata_bridge
    );
    define_global_function!("__perryAsyncTick", perry_async_tick_bridge);

    let Some(source) = v8::String::new(
        scope,
        r#"
(function () {
  if (typeof Reflect !== "object" || Reflect === null) return;
  if (
    Reflect.__perryMetadataBridgeInstalled === true &&
    Reflect.defineMetadata &&
    Reflect.defineMetadata.__perryMetadataBridgeWrapper === true
  ) {
    return;
  }
  const markBridgeWrapper = fn => {
    try {
      Object.defineProperty(fn, "__perryMetadataBridgeWrapper", { value: true });
    } catch (_) {}
    return fn;
  };
  const originalDefine = Reflect.defineMetadata;
  const originalGet = Reflect.getMetadata;
  const originalGetOwn = Reflect.getOwnMetadata;
  const originalHas = Reflect.hasMetadata;
  const originalHasOwn = Reflect.hasOwnMetadata;
  if (typeof originalDefine !== "function" || typeof originalGet !== "function") {
    Reflect.defineMetadata = markBridgeWrapper(function (key, value, target, propertyKey) {
      return globalThis.__perryReflectDefineMetadata(key, value, target, propertyKey);
    });
    Reflect.getMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectGetMetadata(key, target, propertyKey);
    });
    Reflect.getOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectGetOwnMetadata(key, target, propertyKey);
    });
    Reflect.hasMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectHasMetadata(key, target, propertyKey);
    });
    Reflect.hasOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectHasOwnMetadata(key, target, propertyKey);
    });
    Reflect.getMetadataKeys = markBridgeWrapper(function (target, propertyKey) {
      return globalThis.__perryReflectGetMetadataKeys(target, propertyKey);
    });
    Reflect.getOwnMetadataKeys = markBridgeWrapper(function (target, propertyKey) {
      return globalThis.__perryReflectGetOwnMetadataKeys(target, propertyKey);
    });
    Reflect.deleteMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectDeleteMetadata(key, target, propertyKey);
    });
    Reflect.metadata = markBridgeWrapper(function (key, value) {
      return function (target, propertyKey) {
        Reflect.defineMetadata(key, value, target, propertyKey);
      };
    });
    Reflect.__perryMetadataBridgeInstalled = true;
    return;
  }

  Reflect.defineMetadata = markBridgeWrapper(function (key, value, target, propertyKey) {
    const result = originalDefine.apply(this, arguments);
    globalThis.__perryReflectDefineMetadata(key, value, target, propertyKey);
    return result;
  });

  Reflect.getMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    const original = originalGet.apply(this, arguments);
    return original === undefined
      ? globalThis.__perryReflectGetMetadata(key, target, propertyKey)
      : original;
  });

  Reflect.getOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    const original = typeof originalGetOwn === "function"
      ? originalGetOwn.apply(this, arguments)
      : undefined;
    return original === undefined
      ? globalThis.__perryReflectGetOwnMetadata(key, target, propertyKey)
      : original;
  });

  Reflect.hasMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    if (typeof originalHas === "function" && originalHas.apply(this, arguments)) return true;
    return globalThis.__perryReflectHasMetadata(key, target, propertyKey);
  });

  Reflect.hasOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    if (typeof originalHasOwn === "function" && originalHasOwn.apply(this, arguments)) return true;
    return globalThis.__perryReflectHasOwnMetadata(key, target, propertyKey);
  });

  Reflect.__perryMetadataBridgeInstalled = true;
})();
"#,
    ) else {
        return;
    };
    let _ = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
}

fn perry_async_tick_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let promise = resolver.get_promise(scope);
    PENDING_JSRUNTIME_TICKS.with(|ticks| {
        ticks.borrow_mut().push(v8::Global::new(scope, resolver));
    });
    perry_runtime::event_pump::js_notify_main_thread();
    retval.set(promise.into());
}

fn reflect_define_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let value = v8_to_native_metadata_value(scope, args.get(1));
    let target = v8_to_native_metadata_target(scope, args.get(2));
    let property_key = v8_to_native(scope, args.get(3));
    let result = perry_runtime::proxy::js_reflect_define_metadata(key, value, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_get_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_own_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_get_own_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_has_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_has_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_has_own_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_has_own_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_metadata_keys_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let target = v8_to_native_metadata_target(scope, args.get(0));
    let property_key = v8_to_native(scope, args.get(1));
    let result = perry_runtime::proxy::js_reflect_get_metadata_keys(target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_own_metadata_keys_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let target = v8_to_native_metadata_target(scope, args.get(0));
    let property_key = v8_to_native(scope, args.get(1));
    let result = perry_runtime::proxy::js_reflect_get_own_metadata_keys(target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_delete_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_delete_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

/// Probe a V8 handle's `typeof` discriminator. Returns 1 for callables (functions),
/// 0 for everything else. Wired into `js_value_typeof` so user-visible `typeof gp`
/// returns `"function"` when `gp` is a V8 callable handle. (Issue #258.)
unsafe extern "C" fn js_handle_typeof(value: f64) -> i32 {
    bump_v8_entry(V8EntryKind::TypeofProbe);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        let v = native_to_v8(scope, value);
        if v.is_function() {
            1
        } else {
            0
        }
    })
}

/// V8 new_instance implementation — called via callback from perry-runtime's js_new_from_handle
/// when the constructor is a JS handle (JS_HANDLE_TAG).
unsafe extern "C" fn js_new_from_handle_v8_impl(
    constructor_handle: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::HandleConstructor);
    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        let constructor_val = native_to_v8(scope, constructor_handle);
        if !constructor_val.is_function() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let constructor = v8::Local::<v8::Function>::try_from(constructor_val).unwrap();

        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| {
                let fixed = fixup_native_for_v8(arg);
                native_to_v8(scope, fixed)
            })
            .collect();

        v8::tc_scope!(tc_scope, scope);
        match constructor.new_instance(tc_scope, &v8_args) {
            Some(r) => v8_to_native(tc_scope, r.into()),
            None => {
                if let Some(exception) = tc_scope.exception() {
                    let msg = exception.to_rust_string_lossy(tc_scope);
                    eprintln!("[js_new_from_handle_v8] constructor failed: {}", msg);
                }
                f64::from_bits(0x7FFC_0000_0000_0001)
            }
        }
    })
}

/// V8 fallback for native module property access (e.g., ethers.Contract).
/// Loads the module via V8, finds the property, and returns a JS handle.
unsafe extern "C" fn native_module_js_property_loader(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::NativeModulePropertyLoad);
    let module_name =
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(module_name_ptr, module_name_len));
    let property_name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
        property_name_ptr,
        property_name_len,
    ));

    // Load the module via V8
    let module_handle = js_load_module(module_name.as_ptr() as *const i8, module_name.len());
    if module_handle == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    }

    // Try getting the property as a direct named export (e.g., Contract from ethers)
    let direct = js_get_export(
        module_handle,
        property_name.as_ptr() as *const i8,
        property_name.len(),
    );
    if direct.to_bits() != 0x7FFC_0000_0000_0001 {
        return direct;
    }

    // Try through the namespace export (e.g., ethers.Contract)
    let namespace = js_get_export(
        module_handle,
        module_name.as_ptr() as *const i8,
        module_name.len(),
    );
    if namespace.to_bits() != 0x7FFC_0000_0000_0001 {
        return js_handle_object_get_property(
            namespace,
            property_name.as_ptr() as *const i8,
            property_name.len(),
        );
    }

    f64::from_bits(0x7FFC_0000_0000_0001) // undefined
}

/// Shutdown the JavaScript runtime and release resources
#[no_mangle]
pub extern "C" fn js_runtime_shutdown() {
    bump_v8_entry(V8EntryKind::RuntimeShutdown);
    // The runtime will be cleaned up when the thread exits
    log::debug!("JS runtime shutdown requested");
}

/// Load a JavaScript module and return a handle to it
/// Returns a module handle (u64) that can be used with js_get_export and js_call_function
/// Returns 0 on failure
#[no_mangle]
pub unsafe extern "C" fn js_load_module(path_ptr: *const i8, path_len: usize) -> u64 {
    let path_slice = if path_ptr.is_null() {
        return 0;
    } else if path_len > 0 {
        std::slice::from_raw_parts(path_ptr as *const u8, path_len)
    } else {
        // Null-terminated C string
        CStr::from_ptr(path_ptr as *const c_char).to_bytes()
    };

    let path_str = match std::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    // Use the NodeModuleLoader to resolve bare module specifiers (like "ethers")
    use deno_core::ModuleLoader;
    let loader = crate::modules::NodeModuleLoader::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Try to resolve the module path
    let resolved_path: PathBuf = if path_str.starts_with("./")
        || path_str.starts_with("../")
        || path_str.starts_with('/')
    {
        // Relative or absolute path - resolve directly
        let path = PathBuf::from(path_str);
        std::fs::canonicalize(&path).unwrap_or(path)
    } else {
        // Bare module specifier (like "ethers") - use node_modules resolution
        let referrer = format!("file://{}/index.js", cwd.display());
        match loader.resolve(path_str, &referrer, deno_core::ResolutionKind::Import) {
            Ok(specifier) => {
                // Top-level user imports of bare specifiers that the
                // loader couldn't find produce a `perry-missing:` stub
                // (so nested V8 graph resolution can soft-throw). At
                // the top-level entry, a real failure is correct —
                // surface it as the existing hard error.
                if specifier.scheme() == "perry-missing" {
                    eprintln!(
                            "[js_load_module] FAILED to load '{}': bare module not found in node_modules",
                            path_str
                        );
                    return 0;
                }
                specifier
                    .to_file_path()
                    .unwrap_or_else(|_| PathBuf::from(path_str))
            }
            Err(e) => {
                log::error!("Failed to resolve module '{}': {}", path_str, e);
                return 0;
            }
        }
    };

    let canonical = resolved_path.clone();

    let target_specifier = match deno_core::ModuleSpecifier::from_file_path(&canonical) {
        Ok(s) => s,
        Err(_) => {
            log::error!(
                "Failed to create module specifier from path: {:?}",
                canonical
            );
            return 0;
        }
    };
    let target_specifier_str = target_specifier.to_string();
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    // Materialize the proxy in a per-process temp directory rather than the
    // user's CWD. Deno's recursive loader still resolves the proxy specifier
    // through our NodeModuleLoader, so the file must exist on disk even
    // though the source is also supplied via load_side_es_module_from_code.
    let proxy_dir = std::env::temp_dir().join(format!("perry-js-proxy-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&proxy_dir);
    let proxy_path = proxy_dir.join(format!("__perry_js_proxy_{:016x}.mjs", hasher.finish()));
    let specifier = match deno_core::ModuleSpecifier::from_file_path(&proxy_path) {
        Ok(s) => s,
        Err(_) => {
            log::error!(
                "Failed to create proxy module specifier for {:?}",
                canonical
            );
            return 0;
        }
    };
    let proxy_code = format!(
        r#"import * as __perry_ns from {target:?};
const __perry_default = Object.prototype.hasOwnProperty.call(__perry_ns, "default") ? __perry_ns.default : __perry_ns;
export {{ __perry_default as default }};
export * from {target:?};
"#,
        target = target_specifier_str
    );
    if let Ok(proxy_file_path) = specifier.to_file_path() {
        let _ = std::fs::write(proxy_file_path, &proxy_code);
    }

    let tokio_rt = get_tokio_runtime();

    let result = tokio_rt.block_on(async {
        JS_RUNTIME.with(|cell| {
            let mut opt = cell.borrow_mut();
            let state = match opt.as_mut() {
                Some(s) => s,
                None => {
                    eprintln!("[js_load_module] no JS runtime state!");
                    return Err(());
                }
            };

            // Check if already loaded
            if let Some(&module_id) = state.loaded_modules.get(&canonical) {
                return Ok(module_id as u64);
            }
            bump_v8_entry(V8EntryKind::ModuleLoad);

            // Use a dedicated current-thread Tokio runtime to avoid thread pool starvation deadlock.
            tokio::task::block_in_place(|| {
                let local_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create local Tokio runtime for module loading");
                local_rt.block_on(async {
                    // Load a proxy module rather than the target directly. The target may
                    // already have been evaluated as a dependency of another JS module; a
                    // proxy imports and re-exports it without evaluating that target as a
                    // new side root.
                    let module_id = match state
                        .runtime
                        .load_side_es_module_from_code(&specifier, proxy_code)
                        .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            eprintln!("[js_load_module] FAILED to load '{}': {}", path_str, e);
                            return Err(());
                        }
                    };

                    // Start evaluation, but let Perry's main event loop drive
                    // the returned future via js_run_jsruntime_pump().
                    let eval_future = state.runtime.mod_evaluate(module_id);
                    state.pending_module_evaluations.insert(
                        module_id,
                        crate::PendingModuleEvaluation {
                            canonical_path: canonical.clone(),
                            future: Box::pin(eval_future),
                        },
                    );
                    bump_jsruntime(&JSRUNTIME_MODULE_EVALS_STARTED);

                    // Cache the module immediately so repeated imports reuse
                    // the same module id while evaluation is pump-driven.
                    state.loaded_modules.insert(canonical.clone(), module_id);
                    perry_runtime::event_pump::js_notify_main_thread();
                    let _ = poll_pending_module_evaluations(state);

                    Ok(module_id as u64)
                })
            })
        })
    });

    result.unwrap_or(0)
}

/// Get an export from a loaded module
/// Returns the value as a NaN-boxed f64
#[no_mangle]
pub unsafe extern "C" fn js_get_export(
    module_handle: u64,
    export_name_ptr: *const i8,
    export_name_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::ExportGet);
    let name_slice = if export_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if export_name_len > 0 {
        std::slice::from_raw_parts(export_name_ptr as *const u8, export_name_len)
    } else {
        CStr::from_ptr(export_name_ptr as *const c_char).to_bytes()
    };

    let export_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                eprintln!("[js_get_export] failed to get namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        deno_core::scope!(scope, &mut state.runtime);
        let namespace = v8::Local::new(scope, namespace);

        // For namespace imports (export_name == "*"), return the entire module namespace object
        if export_name == "*" {
            let result = v8_to_native(scope, namespace.into());
            return result;
        }

        let key = match v8::String::new(scope, export_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let value = match namespace.get(scope, key.into()) {
            Some(v) => v,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        v8_to_native_export_value(scope, value)
    })
}

/// Call a JavaScript function with arguments
/// Returns the result as a NaN-boxed f64
#[no_mangle]
pub unsafe extern "C" fn js_call_function(
    module_handle: u64,
    func_name_ptr: *const i8,
    func_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::FunctionCall);
    let name_slice = if func_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if func_name_len > 0 {
        std::slice::from_raw_parts(func_name_ptr as *const u8, func_name_len)
    } else {
        CStr::from_ptr(func_name_ptr as *const c_char).to_bytes()
    };

    let func_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                log::error!("Failed to get module namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        call_function_impl(state, namespace, func_name, &args)
    })
}

/// Issue #678: invoke a named export of a V8-fallback module by specifier.
///
/// Bundles `js_load_module` + `js_call_function` into a single FFI entry the
/// codegen can drop in wherever an import resolves to a `ModuleKind::Interpreted`
/// module. Without this, the codegen would emit `perry_fn_<src>__<name>` for
/// imports out of a V8-routed module — but no such native symbol exists, so
/// the linker fails with `Undefined symbols: _perry_fn_..._<name>`.
///
/// `specifier_ptr` / `specifier_len` and `export_name_ptr` / `export_name_len`
/// follow the same ptr+len convention as `js_load_module` / `js_call_function`
/// (zero len = null-terminated C string). `args_ptr` / `args_len` carry the
/// already-NaN-boxed Perry argument doubles; result is also NaN-boxed.
#[no_mangle]
pub unsafe extern "C" fn js_call_v8_export(
    specifier_ptr: *const i8,
    specifier_len: usize,
    export_name_ptr: *const i8,
    export_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::V8ExportCall);
    let module_handle = js_load_module(specifier_ptr, specifier_len);
    if module_handle == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    js_call_function(
        module_handle,
        export_name_ptr,
        export_name_len,
        args_ptr,
        args_len,
    )
}

/// Issue #818 (Effect.succeed pattern): invoke a method on a NAMED member of a
/// V8-fallback module — `Effect.succeed(42)` where `Effect` is imported by name
/// (`import { Effect } from 'effect'`) and the export is itself a sub-namespace
/// object that holds the actual `succeed` function.
///
/// Without this entry, `StaticMethodCall { class_name: "Effect", method_name:
/// "succeed" }` fell through to `double_literal(0.0)` because:
///   - `methods.get(("Effect","succeed"))` misses (Effect isn't a perry class)
///   - `namespace_imports.contains("Effect")` is false (it's a Named, not a
///     `import * as Effect`)
///   - The existing `js_call_v8_export` would call `effect.succeed(...)` at
///     the top level of the module, but the actual function lives at
///     `effect.Effect.succeed`.
///
/// Bundles `js_load_module` + namespace-property-get + method-call so the
/// codegen can drop in a single FFI call wherever a named V8 import is invoked
/// as a static method. Argument and return marshalling follows the same
/// conventions as `js_call_v8_export` — args already NaN-boxed, result
/// NaN-boxed (objects come back as JS handles so subsequent `.value` /
/// `.pipe()` accesses route through the existing HANDLE_PROPERTY / METHOD
/// dispatch and reach V8 again with the prototype intact).
#[no_mangle]
pub unsafe extern "C" fn js_call_v8_member_method(
    specifier_ptr: *const i8,
    specifier_len: usize,
    member_name_ptr: *const i8,
    member_name_len: usize,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::V8ExportCall);
    let module_handle = js_load_module(specifier_ptr, specifier_len);
    if module_handle == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    let member_name = match c_str_to_utf8(member_name_ptr, member_name_len) {
        Some(s) => s,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let method_name = match c_str_to_utf8(method_name_ptr, method_name_len) {
        Some(s) => s,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                log::error!("Failed to get module namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        deno_core::scope!(scope, &mut state.runtime);
        let namespace = v8::Local::new(scope, namespace);
        v8::tc_scope!(tc_scope, scope);

        // Walk the member chain (single hop here — caller passes `Effect`
        // for `Effect.succeed(args)`). Result must be a callable host.
        let member_key = match v8::String::new(tc_scope, &member_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };
        let member_val = match namespace.get(tc_scope, member_key.into()) {
            Some(v) => v,
            None => {
                eprintln!(
                    "[JS-INTEROP] V8 member '{}' not found on module namespace",
                    member_name
                );
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };
        if !member_val.is_object() {
            eprintln!(
                "[JS-INTEROP] V8 member '{}' is not an object (got typeof {})",
                member_name,
                if member_val.is_function() {
                    "function"
                } else {
                    "primitive"
                }
            );
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let member_obj = match member_val.to_object(tc_scope) {
            Some(o) => o,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let method_key = match v8::String::new(tc_scope, &method_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };
        let method_val = match member_obj.get(tc_scope, method_key.into()) {
            Some(v) => v,
            None => {
                eprintln!(
                    "[JS-INTEROP] V8 method '{}.{}' not found",
                    member_name, method_name
                );
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };
        if !method_val.is_function() {
            eprintln!(
                "[JS-INTEROP] V8 '{}.{}' is not a function",
                member_name, method_name
            );
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let method = v8::Local::<v8::Function>::try_from(method_val).unwrap();

        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&a| native_to_v8(tc_scope, fixup_native_for_v8(a)))
            .collect();

        // Bind `this` to the member object so methods that use `this`
        // (most class-style static methods) see the right receiver.
        let result = match method.call(tc_scope, member_obj.into(), &v8_args) {
            Some(r) => r,
            None => {
                if tc_scope.has_caught() {
                    if let Some(exception) = tc_scope.exception() {
                        let msg = exception.to_rust_string_lossy(tc_scope);
                        eprintln!(
                            "[JS-INTEROP] '{}.{}' threw: {}",
                            member_name, method_name, msg
                        );
                    }
                }
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(tc_scope, result)
    })
}

fn c_str_to_utf8(ptr: *const i8, len: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe {
        if len > 0 {
            std::slice::from_raw_parts(ptr as *const u8, len)
        } else {
            CStr::from_ptr(ptr as *const c_char).to_bytes()
        }
    };
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn call_function_impl(
    state: &mut JsRuntimeState,
    namespace: v8::Global<v8::Object>,
    func_name: &str,
    args: &[f64],
) -> f64 {
    deno_core::scope!(scope, &mut state.runtime);
    let namespace = v8::Local::new(scope, namespace);

    // Use TryCatch to properly handle V8 exceptions
    v8::tc_scope!(tc_scope, scope);

    // Get the function from the namespace
    let key = match v8::String::new(tc_scope, func_name) {
        Some(k) => k,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let func_val = match namespace.get(tc_scope, key.into()) {
        Some(v) => v,
        None => {
            log::error!("Function '{}' not found in module", func_name);
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
    };

    if !func_val.is_function() {
        log::error!("'{}' is not a function", func_name);
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let func = v8::Local::<v8::Function>::try_from(func_val).unwrap();

    // Convert arguments from native to V8
    let v8_args: Vec<v8::Local<v8::Value>> = args
        .iter()
        .map(|&arg| native_to_v8(tc_scope, fixup_native_for_v8(arg)))
        .collect();

    // Call the function
    let undefined = v8::undefined(tc_scope);
    let result = match func.call(tc_scope, undefined.into(), &v8_args) {
        Some(r) => r,
        None => {
            // Get and log the exception, then clear it so subsequent calls work
            if tc_scope.has_caught() {
                if let Some(exception) = tc_scope.exception() {
                    // Try to get detailed message
                    if let Some(msg_obj) = tc_scope.message() {
                        let msg_str = msg_obj.get(tc_scope).to_rust_string_lossy(tc_scope);
                        let line = msg_obj.get_line_number(tc_scope).unwrap_or(0);
                        let script = msg_obj
                            .get_script_resource_name(tc_scope)
                            .map(|s| s.to_rust_string_lossy(tc_scope))
                            .unwrap_or_default();
                        eprintln!(
                            "[JS-INTEROP] Function '{}' threw: {} ({}:{})",
                            func_name, msg_str, script, line
                        );
                    } else {
                        let msg = exception.to_rust_string_lossy(tc_scope);
                        eprintln!("[JS-INTEROP] Function '{}' threw: {}", func_name, msg);
                    }

                    // Log args for debugging
                    for (i, &arg) in args.iter().enumerate() {
                        let bits = arg.to_bits();
                        let tag = bits >> 48;
                        eprintln!(
                            "[JS-INTEROP]   arg[{}]: bits=0x{:016x} tag=0x{:04x}",
                            i, bits, tag
                        );
                    }
                }
                // Exception is automatically cleared when TryCatch scope drops
            } else {
                eprintln!(
                    "[JS-INTEROP] Function '{}' call returned None (no exception)",
                    func_name
                );
            }
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
    };

    // Handle promises - for now just return the promise object
    // Proper async support would require more complex handling
    v8_to_native(tc_scope, result)
}

/// Call a method on a JavaScript object
#[no_mangle]
pub unsafe extern "C" fn js_call_method(
    object_ptr: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::MethodCall);
    let name_slice = if method_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    } else if method_name_len > 0 {
        std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len)
    } else {
        CStr::from_ptr(method_name_ptr as *const c_char).to_bytes()
    };

    let method_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the object pointer to a V8 object
        let obj_val = native_to_v8(scope, object_ptr);
        if !obj_val.is_object() {
            log::error!("Value is not an object");
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let obj = obj_val.to_object(scope).unwrap();

        // Get the method
        let key = match v8::String::new(scope, method_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let method_val = match obj.get(scope, key.into()) {
            Some(v) => v,
            None => {
                log::error!("Method '{}' not found on object", method_name);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        if !method_val.is_function() {
            log::error!("'{}' is not a function", method_name);
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let method = v8::Local::<v8::Function>::try_from(method_val).unwrap();

        // Convert arguments
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| native_to_v8(scope, fixup_native_for_v8(arg)))
            .collect();

        // Call with 'this' bound to the object
        let result = match method.call(scope, obj.into(), &v8_args) {
            Some(r) => r,
            None => {
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(scope, result)
    })
}

/// Call a JavaScript function value directly (for callback parameters)
/// func_value: NaN-boxed f64 containing a V8 function handle
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as a NaN-boxed f64
#[no_mangle]
pub unsafe extern "C" fn js_call_value(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::ValueCall);
    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        v8::tc_scope!(tc_scope, scope);

        // Extract the function from the NaN-boxed value
        let func_local = match nanbox_to_v8(tc_scope, func_value) {
            Some(v) => v,
            None => {
                log::error!("Failed to convert function value from NaN-boxed");
                return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
            }
        };

        if !func_local.is_function() {
            log::error!("Value is not a function");
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let func = v8::Local::<v8::Function>::try_from(func_local).unwrap();

        // Convert arguments
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| native_to_v8(tc_scope, fixup_native_for_v8(arg)))
            .collect();

        // Call with undefined as 'this'
        let undefined = v8::undefined(tc_scope);
        let result = match func.call(tc_scope, undefined.into(), &v8_args) {
            Some(r) => r,
            None => {
                if tc_scope.has_caught() {
                    if let Some(msg_obj) = tc_scope.message() {
                        let msg_str = msg_obj.get(tc_scope).to_rust_string_lossy(tc_scope);
                        let line = msg_obj.get_line_number(tc_scope).unwrap_or(0);
                        let script = msg_obj
                            .get_script_resource_name(tc_scope)
                            .map(|s| s.to_rust_string_lossy(tc_scope))
                            .unwrap_or_default();
                        log::error!(
                            "[JS-INTEROP] Function value threw: {} ({}:{})",
                            msg_str,
                            script,
                            line
                        );
                    } else if let Some(exception) = tc_scope.exception() {
                        log::error!(
                            "[JS-INTEROP] Function value threw: {}",
                            exception.to_rust_string_lossy(tc_scope)
                        );
                    }
                } else {
                    log::error!("Function call failed");
                }
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(tc_scope, result)
    })
}

/// Register a native function that can be called from JavaScript
#[no_mangle]
pub unsafe extern "C" fn js_register_native_function(
    name_ptr: *const i8,
    name_len: usize,
    func_ptr: *const u8,
    param_count: usize,
) {
    bump_v8_entry(V8EntryKind::NativeFunctionRegister);
    let name_slice = if name_ptr.is_null() {
        return;
    } else if name_len > 0 {
        std::slice::from_raw_parts(name_ptr as *const u8, name_len)
    } else {
        CStr::from_ptr(name_ptr as *const c_char).to_bytes()
    };

    let _func_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };

    // Store the function pointer and param count for later use
    log::debug!(
        "Registered native function at {:?} with {} params",
        func_ptr,
        param_count
    );

    // TODO: Implement proper native function registration
}

/// Get an element from a JavaScript array by index
/// array_handle: NaN-boxed value containing a JS handle to an array
/// index: The array index
/// Returns the element value as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_handle_array_get(array_handle: f64, index: i32) -> f64 {
    bump_v8_entry(V8EntryKind::ArrayGet);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the handle to a V8 value
        let arr_val = native_to_v8(scope, array_handle);

        // Use Object::get_index which works for both arrays and array-like objects
        // (e.g., ethers.js Result extends Array but V8 is_array() returns false)
        if arr_val.is_object() {
            let obj = v8::Local::<v8::Object>::try_from(arr_val).unwrap();
            let elem = match obj.get_index(scope, index as u32) {
                Some(v) => v,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            return v8_to_native(scope, elem);
        }

        // Fallback for non-objects
        f64::from_bits(0x7FFC_0000_0000_0001) // undefined
    })
}

/// Get the length of a JavaScript array
/// array_handle: NaN-boxed value containing a JS handle to an array
/// Returns the length as i32
#[no_mangle]
pub extern "C" fn js_handle_array_length(array_handle: f64) -> i32 {
    bump_v8_entry(V8EntryKind::ArrayLength);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the handle to a V8 value
        let arr_val = native_to_v8(scope, array_handle);

        // For actual arrays, use Array::length()
        if arr_val.is_array() {
            let arr = v8::Local::<v8::Array>::try_from(arr_val).unwrap();
            return arr.length() as i32;
        }

        // For array-like objects (e.g., ethers.js Result), get the "length" property
        if arr_val.is_object() {
            let obj = v8::Local::<v8::Object>::try_from(arr_val).unwrap();
            let key = v8::String::new(scope, "length").unwrap();
            if let Some(length_val) = obj.get(scope, key.into()) {
                if length_val.is_number() {
                    return length_val.number_value(scope).unwrap_or(0.0) as i32;
                }
            }
        }

        0
    })
}

/// Get a property from a JavaScript object (for JS handle objects)
/// This is called by js_dynamic_object_get_property in perry-runtime when a JS handle is detected
/// object_ptr: NaN-boxed value containing a JS handle
/// Returns the property value as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_handle_object_get_property(
    object_ptr: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::ObjectPropertyGet);
    let name_slice = if property_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if property_name_len > 0 {
        unsafe { std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len) }
    } else {
        unsafe { CStr::from_ptr(property_name_ptr as *const c_char).to_bytes() }
    };

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    // Issue #255: when called from inside a V8 callback trampoline,
    // reuse the trampoline's scope rather than creating a new one via
    // `state.runtime.handle_scope()`. The latter clashes with V8's
    // scope-stack tracking under deno_core (panics with "active scope
    // can't be dropped" when the inner scope drops). The trampoline
    // stashes its scope ptr in REENTRY_SCOPE_PTR; this branch picks
    // it up. Outside a callback, fall through to the normal path.
    if let Some(scope) = unsafe { crate::try_trampoline_scope() } {
        return get_property_with_scope(scope, object_ptr, property_name);
    }

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        get_property_with_scope(scope, object_ptr, property_name)
    })
}

/// Shared body of `js_handle_object_get_property` parameterized over the
/// V8 scope to use — extracted so both the normal path (creates a scope
/// from the runtime) and the trampoline-reuse path (issue #255) share
/// the same logic.
fn get_property_with_scope(
    scope: &mut v8::PinScope<'_, '_>,
    object_ptr: f64,
    property_name: &str,
) -> f64 {
    let obj_val = native_to_v8(scope, object_ptr);
    if !obj_val.is_object() {
        eprintln!("[js_handle_object_get_property] value is not an object!");
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let obj = obj_val.to_object(scope).unwrap();

    let key = match v8::String::new(scope, property_name) {
        Some(k) => k,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let prop_val = match obj.get(scope, key.into()) {
        Some(v) => v,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    v8_to_native(scope, prop_val)
}

/// Convert a JavaScript handle value to a native string
/// handle: NaN-boxed value containing a JS handle
/// Returns a pointer to a native StringHeader
#[no_mangle]
pub extern "C" fn js_handle_to_string(handle: f64) -> *mut perry_runtime::string::StringHeader {
    bump_v8_entry(V8EntryKind::HandleToString);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the handle to a V8 value
        let v8_val = native_to_v8(scope, handle);

        // Convert to string
        let str_val = match v8_val.to_string(scope) {
            Some(s) => s,
            None => {
                // Return empty string on failure
                return perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
            }
        };

        // Get the UTF-8 bytes
        let len = str_val.utf8_length(scope);
        let mut buffer = vec![0u8; len];
        str_val.write_utf8_v2(scope, &mut buffer, v8::WriteFlags::empty(), None);

        // Create a native string
        perry_runtime::string::js_string_from_bytes(buffer.as_ptr(), buffer.len() as u32)
    })
}

/// Set a property on a JavaScript object
/// object_ptr: NaN-boxed value containing a JS handle
/// value: NaN-boxed value to set
#[no_mangle]
pub unsafe extern "C" fn js_set_property(
    object_ptr: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
    value: f64,
) {
    bump_v8_entry(V8EntryKind::PropertySet);
    let name_slice = if property_name_ptr.is_null() {
        return;
    } else if property_name_len > 0 {
        std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
    } else {
        CStr::from_ptr(property_name_ptr as *const c_char).to_bytes()
    };

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return,
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the object pointer to a V8 object
        let obj_val = native_to_v8(scope, object_ptr);
        if !obj_val.is_object() {
            log::error!("Value is not an object");
            return;
        }

        let obj = obj_val.to_object(scope).unwrap();

        // Set the property
        let key = match v8::String::new(scope, property_name) {
            Some(k) => k,
            None => return,
        };

        let v8_value = native_to_v8(scope, value);
        obj.set(scope, key.into(), v8_value);
    })
}

/// Create a new instance of a JavaScript class
/// module_handle: Handle to the loaded module
/// class_name: Name of the class to instantiate
/// args: Array of NaN-boxed f64 arguments
/// Returns a JS handle to the new instance
#[no_mangle]
pub unsafe extern "C" fn js_new_instance(
    module_handle: u64,
    class_name_ptr: *const i8,
    class_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::NewInstance);
    let name_slice = if class_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if class_name_len > 0 {
        std::slice::from_raw_parts(class_name_ptr as *const u8, class_name_len)
    } else {
        CStr::from_ptr(class_name_ptr as *const c_char).to_bytes()
    };

    let class_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        let module_id = module_handle as deno_core::ModuleId;
        let namespace = match state.runtime.get_module_namespace(module_id) {
            Ok(ns) => ns,
            Err(e) => {
                log::error!("Failed to get module namespace: {}", e);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        deno_core::scope!(scope, &mut state.runtime);
        let namespace = v8::Local::new(scope, namespace);

        // Get the class constructor from the namespace
        let key = match v8::String::new(scope, class_name) {
            Some(k) => k,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };

        let constructor_val = match namespace.get(scope, key.into()) {
            Some(v) => v,
            None => {
                log::error!("Class '{}' not found in module", class_name);
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        if !constructor_val.is_function() {
            log::error!("'{}' is not a constructor", class_name);
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let constructor = v8::Local::<v8::Function>::try_from(constructor_val).unwrap();

        // Convert arguments from native to V8
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| native_to_v8(scope, fixup_native_for_v8(arg)))
            .collect();

        // Call the constructor with 'new'
        let result = match constructor.new_instance(scope, &v8_args) {
            Some(r) => r,
            None => {
                log::error!("Constructor call failed");
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
        };

        v8_to_native(scope, result.into())
    })
}

/// Create a new instance using a JS handle to a constructor function
/// constructor_handle: NaN-boxed value containing a JS handle to a constructor
/// args: Array of NaN-boxed f64 arguments
/// Returns a JS handle to the new instance
#[no_mangle]
pub unsafe extern "C" fn js_new_from_handle(
    constructor_handle: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::NewFromHandle);
    let ctor_bits = constructor_handle.to_bits();
    let tag = ctor_bits >> 48;

    // Only process JS handles — for non-handle constructors, return undefined
    if tag != 0x7FFB {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let args = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Get the constructor from the handle
        let constructor_val = native_to_v8(scope, constructor_handle);
        if !constructor_val.is_function() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        let constructor = v8::Local::<v8::Function>::try_from(constructor_val).unwrap();

        // Convert arguments from native to V8
        let v8_args: Vec<v8::Local<v8::Value>> = args
            .iter()
            .map(|&arg| {
                let fixed = fixup_native_for_v8(arg);
                native_to_v8(scope, fixed)
            })
            .collect();

        // Call the constructor with 'new'
        v8::tc_scope!(tc_scope, scope);
        match constructor.new_instance(tc_scope, &v8_args) {
            Some(r) => v8_to_native(tc_scope, r.into()),
            None => {
                if let Some(exception) = tc_scope.exception() {
                    let msg = exception.to_rust_string_lossy(tc_scope);
                    eprintln!("[js_new_from_handle] constructor failed: {}", msg);
                }
                f64::from_bits(0x7FFC_0000_0000_0001)
            }
        }
    })
}

// Storage for native callback function pointers and their closure environments
thread_local! {
    static NATIVE_CALLBACKS: std::cell::RefCell<std::collections::HashMap<u64, (i64, i64)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static NEXT_CALLBACK_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Create a V8 function that wraps a native callback
/// func_ptr: Pointer to the native function to call
/// closure_env: Pointer to the closure environment (or 0 for no environment)
/// param_count: Number of parameters the callback expects
/// Returns a JS handle to the V8 function
#[no_mangle]
pub unsafe extern "C" fn js_create_callback(
    func_ptr: i64,
    closure_env: i64,
    param_count: i64,
) -> f64 {
    bump_v8_entry(V8EntryKind::CallbackCreate);
    // Store the callback info
    let callback_id = NEXT_CALLBACK_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });

    NATIVE_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow_mut()
            .insert(callback_id, (func_ptr, closure_env));
    });

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Create external data to store the callback ID and param count
        let data_array = v8::Array::new(scope, 2);
        let id_val = v8::Number::new(scope, callback_id as f64);
        let count_val = v8::Number::new(scope, param_count as f64);
        data_array.set_index(scope, 0, id_val.into());
        data_array.set_index(scope, 1, count_val.into());

        // Create the callback function
        let callback_fn = v8::Function::builder(native_callback_trampoline)
            .data(data_array.into())
            .build(scope);

        match callback_fn {
            Some(func) => {
                let handle_id = store_js_handle(scope, func.into());
                make_js_handle_value(handle_id)
            }
            None => {
                log::error!("Failed to create callback function");
                f64::from_bits(0x7FFC_0000_0000_0001)
            }
        }
    })
}

/// Trampoline function that V8 calls when a native callback is invoked
fn native_callback_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    bump_v8_entry(V8EntryKind::CallbackInvoke);
    // Get the callback ID and param count from the data
    let data = args.data();
    if !data.is_array() {
        retval.set(v8::undefined(scope).into());
        return;
    }

    let data_array = v8::Local::<v8::Array>::try_from(data).unwrap();
    let callback_id = data_array
        .get_index(scope, 0)
        .and_then(|v| v.number_value(scope))
        .unwrap_or(0.0) as u64;
    let _param_count = data_array
        .get_index(scope, 1)
        .and_then(|v| v.number_value(scope))
        .unwrap_or(0.0) as i64;

    // Get the function pointer and closure environment
    let (func_ptr, closure_env) = NATIVE_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow()
            .get(&callback_id)
            .copied()
            .unwrap_or((0, 0))
    });

    if func_ptr == 0 {
        log::error!("Native callback not found: {}", callback_id);
        retval.set(v8::undefined(scope).into());
        return;
    }

    // Convert arguments to native format
    let arg_count = args.length();
    let mut native_args: Vec<f64> = Vec::with_capacity(arg_count as usize);
    for i in 0..arg_count {
        let arg = args.get(i);
        native_args.push(v8_to_native(scope, arg));
    }

    // Issue #255: stash this scope so re-entrant FFIs (e.g. js_get_property
    // called from inside the Perry callback to read `ctx.deltaTime`) can
    // reuse it instead of calling state.runtime.handle_scope() — which
    // V8's scope tracking rejects with "active scope can't be dropped"
    // because we'd be creating a new scope above the one V8 itself has
    // active for this trampoline call. Guard auto-restores any prior
    // stashed scope on Drop, so nested trampoline invocations work.
    let _scope_guard = crate::stash_trampoline_scope(scope);

    // Call the native function
    // Function signature: fn(closure_env: i64, args_ptr: *const f64, args_len: i64) -> f64
    type CallbackFn = extern "C" fn(i64, *const f64, i64) -> f64;
    let callback: CallbackFn = unsafe { std::mem::transmute(func_ptr as *const ()) };
    let result = callback(closure_env, native_args.as_ptr(), native_args.len() as i64);

    // Convert result back to V8
    let v8_result = native_to_v8(scope, result);
    retval.set(v8_result);
}

/// Check if a module path should be loaded via the JS runtime
/// Returns 1 if it should use JS runtime, 0 if it should be compiled natively
#[no_mangle]
pub unsafe extern "C" fn js_should_use_runtime(path_ptr: *const i8, path_len: usize) -> i32 {
    bump_v8_entry(V8EntryKind::ShouldUseRuntime);
    let path_slice = if path_ptr.is_null() {
        return 0;
    } else if path_len > 0 {
        std::slice::from_raw_parts(path_ptr as *const u8, path_len)
    } else {
        CStr::from_ptr(path_ptr as *const c_char).to_bytes()
    };

    let path_str = match std::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    // Check if this is a .js file (not .ts/.tsx)
    if path_str.ends_with(".js") || path_str.ends_with(".mjs") || path_str.ends_with(".cjs") {
        return 1;
    }

    // Check if this is in node_modules and not TypeScript
    if path_str.contains("node_modules") {
        let path = PathBuf::from(path_str);

        // If it's a directory reference, check for TypeScript files
        if path.is_dir() {
            let has_ts = path.join("index.ts").exists()
                || path.join("index.tsx").exists()
                || path.join("src/index.ts").exists();

            if !has_ts {
                return 1;
            }
        }
    }

    0
}

/// Await a V8 JavaScript Promise that was returned as a JS handle.
/// Takes a NaN-boxed f64 containing a JS handle to a V8 Promise.
/// Legacy/debug path: when explicitly enabled, runs the V8 event loop until
/// the Promise settles, then returns the resolved value.
/// If the value is not a Promise, returns it as-is.
/// Returns the resolved value as NaN-boxed f64.
#[no_mangle]
pub extern "C" fn js_await_js_promise(value: f64) -> f64 {
    jsruntime_profile_register();
    bump_v8_entry(V8EntryKind::LegacyBlockingAwait);
    if std::env::var_os("PERRY_JSRUNTIME_ENABLE_LEGACY_BLOCKING_AWAIT").is_none() {
        return js_await_any_promise(value);
    }
    bump_jsruntime(&JSRUNTIME_LEGACY_BLOCKING_AWAITS);
    let handle_id = match get_handle_id(value) {
        Some(id) => id,
        None => {
            return value;
        }
    };

    let tokio_rt = get_tokio_runtime();
    tokio_rt.block_on(async {
        JS_RUNTIME.with(|cell| {
            let mut opt = cell.borrow_mut();
            let state = match opt.as_mut() {
                Some(s) => s,
                None => {
                    return f64::from_bits(0x7FFC_0000_0000_0001);
                }
            };

            // Check if the value is a Promise and if it's already settled
            {
                deno_core::scope!(scope, &mut state.runtime);
                let v8_val = match get_js_handle(scope, handle_id) {
                    Some(v) => v,
                    None => {
                        return f64::from_bits(0x7FFC_0000_0000_0001);
                    }
                };

                if !v8_val.is_promise() {
                    return v8_to_native(scope, v8_val);
                }

                let promise = v8::Local::<v8::Promise>::try_from(v8_val).unwrap();
                let state_val = promise.state();
                if state_val != v8::PromiseState::Pending {
                    let result = promise.result(scope);
                    return v8_to_native(scope, result);
                }
            }

            // Promise is pending - run the event loop to settle it
            tokio::task::block_in_place(|| {
                let local_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create local Tokio runtime for V8 event loop");
                local_rt.block_on(async {
                    let _ = state.runtime.run_event_loop(Default::default()).await;
                })
            });

            // Now get the resolved value
            deno_core::scope!(scope, &mut state.runtime);
            let v8_val = match get_js_handle(scope, handle_id) {
                Some(v) => v,
                None => {
                    return f64::from_bits(0x7FFC_0000_0000_0001);
                }
            };

            if v8_val.is_promise() {
                let promise = v8::Local::<v8::Promise>::try_from(v8_val).unwrap();
                match promise.state() {
                    v8::PromiseState::Fulfilled => {
                        let result = promise.result(scope);
                        v8_to_native(scope, result)
                    }
                    v8::PromiseState::Rejected => {
                        f64::from_bits(0x7FFC_0000_0000_0001) // undefined
                    }
                    v8::PromiseState::Pending => {
                        f64::from_bits(0x7FFC_0000_0000_0001) // undefined
                    }
                }
            } else {
                v8_to_native(scope, v8_val)
            }
        })
    })
}

/// Await any promise — handles both JS handle promises (JS_HANDLE_TAG) and
/// native POINTER_TAG promises. If the value is neither, returns it as-is.
///
/// This is the unified await for F64 values where the type isn't known at compile time
/// (e.g., generic method dispatch returning either JS or native promises).
#[no_mangle]
pub extern "C" fn js_await_any_promise(value: f64) -> f64 {
    jsruntime_profile_register();
    let bits = value.to_bits();
    let tag = bits >> 48;

    if tag == 0x7FFB {
        // JS_HANDLE_TAG: if the handle is a V8 Promise, create a native
        // pending Promise and let the jsruntime pump settle it. This keeps
        // V8 promise progress inside Perry's existing event pump instead of
        // blocking inside await lowering.
        let handle_id = match get_handle_id(value) {
            Some(id) => id,
            None => return value,
        };

        let adapter_handle_id = with_runtime(|state| {
            deno_core::scope!(scope, &mut state.runtime);
            get_js_handle(scope, handle_id).and_then(|v| {
                if !v.is_promise() {
                    return None;
                }
                let promise = v8::Local::<v8::Promise>::try_from(v).unwrap();
                promise.mark_as_handled();
                Some(store_js_handle(scope, v))
            })
        });

        let Some(adapter_handle_id) = adapter_handle_id else {
            return value;
        };

        let native_promise = perry_runtime::promise::js_promise_new();
        FOREIGN_PROMISE_ADAPTERS.with(|adapters| {
            adapters.borrow_mut().push(ForeignPromiseAdapter {
                handle_id: adapter_handle_id,
                native_promise,
            });
        });
        bump_v8_entry(V8EntryKind::ForeignPromiseAdapter);
        bump_jsruntime(&JSRUNTIME_ADAPTERS_CREATED);
        perry_runtime::event_pump::js_notify_main_thread();
        return boxed_native_promise(native_promise);
    }

    // For POINTER_TAG (native promises) and all other values, return as-is.
    // The codegen-emitted busy-wait loop handles native promise polling correctly
    // using the same thread's microtask queue.
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_init() {
        js_runtime_init();
        // Should not panic
    }
}
