//! V8 entry-point profiling counters and `PERRY_JSRUNTIME_PROFILE` plumbing.
//!
//! Tracks per-entry-point call counts plus aggregate handle / adapter
//! statistics so users can run with `PERRY_JSRUNTIME_PROFILE=1` to see
//! exactly which V8 entry points fired during a program's lifetime.

#[allow(unused_imports)]
use super::*; // prelude — kept for consistency with sibling modules

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Once;

pub(crate) static JSRUNTIME_PROFILE_ENABLED: AtomicBool = AtomicBool::new(false);
pub(crate) static JSRUNTIME_PROFILE_REG: Once = Once::new();
pub(crate) static JSRUNTIME_PUMP_TICKS: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ADAPTERS_CREATED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ADAPTERS_RESOLVED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ADAPTERS_REJECTED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_MODULE_EVALS_STARTED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_MODULE_EVALS_RESOLVED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_MODULE_EVALS_REJECTED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_BLOCKING_MODULE_EVALS: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_LEGACY_BLOCKING_AWAITS: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_HANDLES_STORED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_HANDLES_RELEASED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_V8_ENTRIES_TOTAL: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_RUNTIME_INIT: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_RUNTIME_SHUTDOWN: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_MODULE_LOAD: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_EXPORT_GET: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_FUNCTION_CALL: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_V8_EXPORT_CALL: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_METHOD_CALL: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_VALUE_CALL: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_ARRAY_GET: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_ARRAY_LENGTH: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_OBJECT_PROPERTY_GET: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_HANDLE_TO_STRING: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_PROPERTY_SET: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_NEW_INSTANCE: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_NEW_FROM_HANDLE: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_CALLBACK_CREATE: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_NATIVE_FUNCTION_REGISTER: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_CALLBACK_INVOKE: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_NATIVE_MODULE_PROPERTY_LOAD: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_TYPEOF_PROBE: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_HANDLE_CONSTRUCTOR: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_SHOULD_USE_RUNTIME: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_NATIVE_PROMISE_RESOLVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_NATIVE_PROMISE_REJECT: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_FOREIGN_PROMISE_ADAPTER: AtomicU64 = AtomicU64::new(0);
pub(crate) static JSRUNTIME_ENTRY_LEGACY_BLOCKING_AWAIT: AtomicU64 = AtomicU64::new(0);

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

pub(crate) fn jsruntime_profile_enabled() -> bool {
    JSRUNTIME_PROFILE_ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn bump_jsruntime(counter: &AtomicU64) {
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

pub(crate) fn jsruntime_profile_register() {
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
