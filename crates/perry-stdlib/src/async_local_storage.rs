//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::array::{js_array_length, ArrayHeader};
use perry_runtime::closure::{is_closure_ptr, js_closure_call_array, ClosureHeader};

use crate::common::{get_handle_mut, register_handle, Handle};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const SUBCLASS_BACKING_KEY: &[u8] = b"__perryAsyncLocalStorageBacking";

// Keep the active context in the single perry-runtime provider.  These must be
// extern calls rather than Rust-path calls: in app-only dylib deployments the
// stdlib is a separate image, and linking its own perry-runtime dependency
// would create a second ACTIVE_CONTEXT that promise/timer schedulers cannot
// see (#8037).
extern "C" {
    fn js_async_context_als_run_enter(handle: i64, store: f64);
    fn js_async_context_als_exit_enter(handle: i64);
    fn js_async_context_als_scope_leave();
    fn js_async_context_als_get_store(handle: i64) -> f64;
    fn js_async_context_als_enter_with(handle: i64, store: f64);
    fn js_async_context_als_clear(handle: i64);
}

/// #3092 — `AsyncLocalStorage#run`/`#exit` must reject a non-callable callback
/// with a `TypeError`, matching Node (which throws through its function-apply
/// path). Returns the validated `ClosureHeader` pointer for a callable value,
/// or diverges via `js_throw`. The POINTER_TAG check guards `is_closure_ptr`
/// from the short-string/double bit patterns that can otherwise look
/// pointer-ish enough to segfault.
unsafe fn validate_callback(callback: f64) -> *const ClosureHeader {
    let bits = callback.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as usize;
        if is_closure_ptr(ptr) {
            return ptr as *const ClosureHeader;
        }
    }
    let message = "callback is not a function";
    let msg = perry_runtime::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

/// #3093 — invoke a validated callback with the forwarded rest arguments.
/// `args_array` is a raw `*const ArrayHeader` (i64) holding the trailing
/// `...args` packed by the codegen `NA_VARARGS` lowering; `0` / empty array
/// means no forwarded args. Mirrors the data/len extraction used by
/// `AsyncResource#runInAsyncScope` in perry-runtime.
unsafe fn call_with_forwarded_args(cb: *const ClosureHeader, args_array: i64) -> f64 {
    let closure_env = cb as i64;
    if args_array == 0 {
        return js_closure_call_array(closure_env, std::ptr::null(), 0);
    }
    let arr = args_array as *const ArrayHeader;
    let len = js_array_length(arr) as i64;
    let data = if arr.is_null() || len == 0 {
        std::ptr::null()
    } else {
        (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
    };
    js_closure_call_array(closure_env, data, len)
}

/// AsyncLocalStorage handle. Store stacks live in perry-runtime's active
/// async context so schedulers can snapshot and restore them across async
/// boundaries.
pub struct AsyncLocalStorageHandle;

impl Default for AsyncLocalStorageHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncLocalStorageHandle {
    pub fn new() -> Self {
        AsyncLocalStorageHandle
    }
}

fn throw_invalid_receiver() -> ! {
    let message = b"Value of \"this\" must be of type AsyncLocalStorage";
    let msg = perry_runtime::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn resolve_async_local_storage_handle(receiver: Handle) -> Option<Handle> {
    if get_handle_mut::<AsyncLocalStorageHandle>(receiver).is_some() {
        return Some(receiver);
    }
    let raw = receiver as usize;
    if !perry_runtime::value::addr_class::is_above_handle_band(raw)
        || !perry_runtime::value::addr_class::is_valid_obj_ptr(raw as *const u8)
    {
        return None;
    }
    let key = perry_runtime::string::js_string_from_bytes(
        SUBCLASS_BACKING_KEY.as_ptr(),
        SUBCLASS_BACKING_KEY.len() as u32,
    );
    let value = perry_runtime::object::js_object_get_field_by_name_f64(
        raw as *const perry_runtime::object::ObjectHeader,
        key,
    );
    if value.to_bits() >> 48 != 0x7FFD {
        return None;
    }
    let backing = (value.to_bits() & POINTER_MASK) as Handle;
    get_handle_mut::<AsyncLocalStorageHandle>(backing).map(|_| backing)
}

/// Stamp an ordinary source-compiled subclass instance with a native ALS
/// backing. Its inherited methods receive the ordinary object as `this` and
/// resolve through the hidden handle above.
#[no_mangle]
pub extern "C" fn js_async_local_storage_subclass_init(this_value: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_handle = scope.root_nanbox_f64(this_value);
    let backing = js_async_local_storage_new();
    let current_this = this_handle.get_nanbox_f64();
    let raw = perry_runtime::value::js_nanbox_get_pointer(current_this)
        as *mut perry_runtime::object::ObjectHeader;
    if !raw.is_null()
        && perry_runtime::value::addr_class::is_above_handle_band(raw as usize)
        && perry_runtime::value::addr_class::is_valid_obj_ptr(raw as *const u8)
    {
        let key = perry_runtime::string::js_string_from_bytes(
            SUBCLASS_BACKING_KEY.as_ptr(),
            SUBCLASS_BACKING_KEY.len() as u32,
        );
        perry_runtime::object::js_object_set_field_by_name(
            raw,
            key,
            perry_runtime::value::js_nanbox_pointer(backing),
        );
        for method in [
            b"run".as_slice(),
            b"getStore".as_slice(),
            b"enterWith".as_slice(),
            b"exit".as_slice(),
            b"disable".as_slice(),
        ] {
            let value = crate::common::dispatch::unbound_async_local_storage_method(method);
            let value_handle = scope.root_nanbox_f64(value);
            let key =
                perry_runtime::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
            let current_raw =
                perry_runtime::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64())
                    as *mut perry_runtime::object::ObjectHeader;
            perry_runtime::object::js_object_set_field_by_name(
                current_raw,
                key,
                value_handle.get_nanbox_f64(),
            );
        }
    }
    this_handle.get_nanbox_f64()
}

/// Create a new AsyncLocalStorage instance
/// Returns a handle (i64)
#[no_mangle]
pub extern "C" fn js_async_local_storage_new() -> Handle {
    register_handle(AsyncLocalStorageHandle::new())
}

/// AsyncLocalStorage.run(store, callback, ...args)
/// Push store onto stack, call callback with the forwarded rest args, pop
/// store, return result. `args_array` carries the `...args` packed by the
/// codegen `NA_VARARGS` lowering (#3093).
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_run(
    receiver: Handle,
    store: f64,
    callback: f64,
    args_array: i64,
) -> f64 {
    // Validate before mutating the async context so an invalid callback throws
    // without leaving a pushed store behind (#3092).
    let cb = validate_callback(callback);
    let handle =
        resolve_async_local_storage_handle(receiver).unwrap_or_else(|| throw_invalid_receiver());

    // A context guard mirrors the pop below: if the callback throws,
    // `js_throw` applies the guard while unwinding so the catch site still
    // observes the pre-`run` store (#788, Node restores via try/finally).
    js_async_context_als_run_enter(handle, store);
    let result = call_with_forwarded_args(cb, args_array);
    js_async_context_als_scope_leave();

    result
}

/// AsyncLocalStorage.getStore()
/// Returns the current store (top of stack) or undefined
#[no_mangle]
pub extern "C" fn js_async_local_storage_get_store(receiver: Handle) -> f64 {
    let handle =
        resolve_async_local_storage_handle(receiver).unwrap_or_else(|| throw_invalid_receiver());
    unsafe { js_async_context_als_get_store(handle) }
}

/// AsyncLocalStorage.enterWith(store)
/// Push store onto stack (caller is responsible for cleanup)
#[no_mangle]
pub extern "C" fn js_async_local_storage_enter_with(receiver: Handle, store: f64) {
    if let Some(handle) = resolve_async_local_storage_handle(receiver) {
        unsafe { js_async_context_als_enter_with(handle, store) };
    }
}

/// AsyncLocalStorage.exit(callback, ...args)
/// Save current stack, clear it, call callback with the forwarded rest args,
/// restore stack. `args_array` carries the `...args` packed by the codegen
/// `NA_VARARGS` lowering (#3093).
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(
    receiver: Handle,
    callback: f64,
    args_array: i64,
) -> f64 {
    // Validate before clearing the context so an invalid callback throws
    // without disturbing the saved store (#3092).
    let cb = validate_callback(callback);
    let handle =
        resolve_async_local_storage_handle(receiver).unwrap_or_else(|| throw_invalid_receiver());
    js_async_context_als_exit_enter(handle);

    let result = call_with_forwarded_args(cb, args_array);

    js_async_context_als_scope_leave();

    result
}

/// AsyncLocalStorage.disable()
/// Clear the store stack
#[no_mangle]
pub extern "C" fn js_async_local_storage_disable(receiver: Handle) {
    if let Some(handle) = resolve_async_local_storage_handle(receiver) {
        unsafe { js_async_context_als_clear(handle) };
    }
}
