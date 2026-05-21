//! FFI interop functions for calling between native code and JavaScript
//!
//! These functions are called from compiled native code to interact with
//! JavaScript modules loaded in the V8 runtime.
//!
//! Split into topical sub-modules in v0.5.1019:
//!
//! - [`profile`] — `PERRY_JSRUNTIME_PROFILE` counters + atexit dump
//! - [`pump`] — event-loop pump + foreign-promise adapter machinery
//! - [`runtime`] — `js_runtime_init` / `_shutdown` + Reflect-metadata bridge
//! - [`modules_ffi`] — module load / export get / native-module fallback
//! - [`calls`] — function / method / value-call FFIs
//! - [`handles`] — array / object / `to_string` / `typeof` handle FFIs
//! - [`instances`] — `new` instance FFIs
//! - [`callbacks`] — native-callback registration + V8 trampoline
//! - [`promises`] — `await` adapters for JS and native promises

// Sub-module prelude — shared imports each sibling pulls in via `use super::*`.
pub(crate) use crate::bridge::{
    capture_export_snapshot_intrinsics, fixup_native_for_v8, get_handle_id, get_js_handle,
    is_js_handle, make_js_handle_value, native_to_v8, release_js_handle, store_js_handle,
    v8_to_native, v8_to_native_export_value, v8_to_native_metadata_target,
    v8_to_native_metadata_value,
};
pub(crate) use crate::{
    ensure_runtime_initialized, get_tokio_runtime, with_runtime, JsRuntimeState, JS_RUNTIME,
};

mod callbacks;
mod calls;
mod handles;
mod instances;
mod modules_ffi;
mod profile;
mod promises;
mod pump;
mod runtime;

// Re-export the public FFI surface using explicit names (no globs).
// Most of these are `#[no_mangle]` extern fns reached via the linker
// rather than by Rust callers, so `#[allow(unused_imports)]` silences
// the dead-re-export warning while keeping the API surface intact.
#[allow(unused_imports)]
pub use callbacks::js_create_callback;
#[allow(unused_imports)]
pub use calls::{
    js_call_function, js_call_method, js_call_v8_export, js_call_v8_member_method, js_call_value,
    js_register_native_function,
};
#[allow(unused_imports)]
pub use handles::{
    js_handle_array_get, js_handle_array_length, js_handle_object_get_property,
    js_handle_to_string, js_set_property,
};
#[allow(unused_imports)]
pub use instances::{js_new_from_handle, js_new_instance};
#[allow(unused_imports)]
pub use modules_ffi::{js_get_export, js_load_module, js_should_use_runtime};
#[allow(unused_imports)]
pub use promises::{js_await_any_promise, js_await_js_promise};
#[allow(unused_imports)]
pub use runtime::{js_runtime_init, js_runtime_shutdown};

// Sibling-private items promoted to `pub(crate)` so siblings (and the
// `bridge` module) can reach them.
#[allow(unused_imports)]
pub(crate) use callbacks::native_callback_trampoline;
pub(crate) use handles::js_handle_typeof;
pub(crate) use instances::js_new_from_handle_v8_impl;
pub(crate) use modules_ffi::{c_str_to_utf8, native_module_js_property_loader};
pub(crate) use profile::{
    bump_js_handle_released, bump_js_handle_stored, bump_jsruntime, bump_v8_entry,
    jsruntime_profile_register, V8EntryKind, JSRUNTIME_ADAPTERS_CREATED,
    JSRUNTIME_ADAPTERS_REJECTED, JSRUNTIME_ADAPTERS_RESOLVED,
    JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED, JSRUNTIME_LEGACY_BLOCKING_AWAITS,
    JSRUNTIME_MODULE_EVALS_REJECTED, JSRUNTIME_MODULE_EVALS_RESOLVED,
    JSRUNTIME_MODULE_EVALS_STARTED, JSRUNTIME_PUMP_TICKS,
};
pub(crate) use pump::{
    boxed_native_promise, js_register_jsruntime_has_active, js_register_jsruntime_pump,
    jsruntime_has_active_handles, jsruntime_process_pending, poll_pending_module_evaluations,
    ForeignPromiseAdapter, FOREIGN_PROMISE_ADAPTERS, PENDING_JSRUNTIME_TICKS,
};
pub(crate) use runtime::install_reflect_metadata_bridge;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_init() {
        js_runtime_init();
        // Should not panic
    }
}
