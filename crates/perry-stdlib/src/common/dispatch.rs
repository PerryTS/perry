//! Handle-based method dispatch for perry-stdlib
//!
//! When native modules (Fastify, ioredis, etc.) use handle-based objects,
//! and those handles are passed to functions as generic parameters,
//! the codegen can't statically determine the type. This module provides
//! runtime dispatch by checking the handle type in the registry.

mod emitter_als;
mod fastify_net_zlib;
mod init;
mod method_dispatch;
mod property_dispatch;
mod sqlite;

// Re-export the no_mangle FFI entry points and helper dispatchers that the
// rest of the crate (and the linker) reach by their original paths. The
// `#[no_mangle]` symbols are already exported objects; the explicit
// re-exports keep `crate::common::dispatch::<name>` resolving for in-crate
// callers and keep the sub-module dispatchers visible to each other.
pub use init::{
    js_handle_own_property_names_dispatch, js_handle_property_set_dispatch,
    js_handle_prototype_dispatch, js_stdlib_init_dispatch,
};
pub use method_dispatch::js_handle_method_dispatch;
pub use property_dispatch::js_handle_property_dispatch;

pub(crate) use emitter_als::{
    dispatch_async_local_storage_method, dispatch_async_local_storage_property,
};
#[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
pub(crate) use emitter_als::{dispatch_event_emitter_method, dispatch_event_emitter_property};
#[cfg(feature = "database-sqlite")]
pub(crate) use sqlite::{dispatch_sqlite_db, dispatch_sqlite_stmt};

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(crate) use fastify_net_zlib::dispatch_external_net_socket;
#[cfg(all(
    feature = "bundled-net",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(crate) use fastify_net_zlib::dispatch_net_socket;
#[cfg(feature = "compression")]
pub(crate) use fastify_net_zlib::dispatch_zlib_stream;

pub(crate) type EventEmitterOn = unsafe extern "C" fn(i64, i64, i64) -> i64;

pub(crate) const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001);
pub(crate) const TAG_UNDEFINED_BITS: i64 = 0x7FFC_0000_0000_0001u64 as i64;
pub(crate) const POINTER_TAG_BITS: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const POINTER_MASK_BITS: u64 = 0x0000_FFFF_FFFF_FFFF;

pub(crate) fn nanbox_handle_value(handle: i64) -> f64 {
    f64::from_bits(POINTER_TAG_BITS | (handle as u64 & POINTER_MASK_BITS))
}

pub(crate) unsafe fn pack_args_array(args: &[f64]) -> *mut perry_runtime::ArrayHeader {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let arr = perry_runtime::js_array_alloc(0);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for arg in &arg_handles {
        let arr =
            perry_runtime::js_array_push_f64(arr_handle.get_raw_mut_ptr(), arg.get_nanbox_f64());
        arr_handle.set_raw_mut_ptr(arr);
    }
    arr_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>()
}

/// Shared `extern "C"` surface of the EventEmitter implementation. Both
/// perry-stdlib (`bundled-events`) and perry-ext-events export these exact
/// symbols, kept byte-identical per #3072. The dispatch arms below call
/// through the linker-resolved symbol instead of `crate::events::*` so that
/// when the well-known flip links perry-ext-events, dynamic dispatch
/// consults the SAME handle registry the constructors used. An in-crate call
/// always hit perry-stdlib's registry and returned `None` for ext-events
/// handles — every dynamic `.on`/`.emit`/`.setMaxListeners` on an emitter
/// silently no-op'd and method-value reads came back `undefined` (#4995).
/// Mirrors the sqlite duplicate-symbol contract noted in
/// `compile/optimized_libs.rs` (#643).
#[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
extern "C" {
    pub(crate) fn js_event_emitter_is_handle(handle: i64) -> bool;
    pub(crate) fn js_event_emitter_on(handle: i64, event_bits: i64, listener_bits: i64) -> i64;
    pub(crate) fn js_event_emitter_once(handle: i64, event_bits: i64, listener_bits: i64) -> i64;
    pub(crate) fn js_event_emitter_prepend_listener(
        handle: i64,
        event_bits: i64,
        listener_bits: i64,
    ) -> i64;
    pub(crate) fn js_event_emitter_prepend_once_listener(
        handle: i64,
        event_bits: i64,
        listener_bits: i64,
    ) -> i64;
    pub(crate) fn js_event_emitter_remove_listener(
        handle: i64,
        event_bits: i64,
        listener_bits: i64,
    ) -> i64;
    pub(crate) fn js_event_emitter_remove_all_listeners(
        handle: i64,
        args_ptr: *const perry_runtime::ArrayHeader,
    ) -> i64;
    pub(crate) fn js_event_emitter_emit(
        handle: i64,
        event_bits: i64,
        args_ptr: *mut perry_runtime::ArrayHeader,
    ) -> f64;
    pub(crate) fn js_event_emitter_listener_count(
        handle: i64,
        event_bits: i64,
        listener_bits: i64,
    ) -> f64;
    pub(crate) fn js_event_emitter_listeners(
        handle: i64,
        event_bits: i64,
    ) -> *mut perry_runtime::ArrayHeader;
    pub(crate) fn js_event_emitter_raw_listeners(
        handle: i64,
        event_bits: i64,
    ) -> *mut perry_runtime::ArrayHeader;
    pub(crate) fn js_event_emitter_event_names(handle: i64) -> *mut perry_runtime::ArrayHeader;
    pub(crate) fn js_event_emitter_set_max_listeners(handle: i64, n: f64) -> i64;
    pub(crate) fn js_event_emitter_get_max_listeners(handle: i64) -> f64;
    pub(crate) fn js_event_emitter_domain_value(handle: i64) -> f64;
    pub(crate) fn js_event_emitter_new_with_options(options: f64) -> i64;
}
