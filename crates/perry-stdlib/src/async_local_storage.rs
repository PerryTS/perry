//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::array::{js_array_length, ArrayHeader};
use perry_runtime::async_context;
use perry_runtime::closure::{is_closure_ptr, js_closure_call_array, ClosureHeader};

use crate::common::{get_handle_mut, register_handle, Handle};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

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
    handle: Handle,
    store: f64,
    callback: f64,
    args_array: i64,
) -> f64 {
    // Validate before mutating the async context so an invalid callback throws
    // without leaving a pushed store behind (#3092).
    let cb = validate_callback(callback);

    async_context::push_store(handle, store);
    let result = call_with_forwarded_args(cb, args_array);
    async_context::pop_store(handle);

    result
}

/// AsyncLocalStorage.getStore()
/// Returns the current store (top of stack) or undefined
#[no_mangle]
pub extern "C" fn js_async_local_storage_get_store(handle: Handle) -> f64 {
    if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        if let Some(store) = async_context::get_store(handle) {
            return store;
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// AsyncLocalStorage.enterWith(store)
/// Push store onto stack (caller is responsible for cleanup)
#[no_mangle]
pub extern "C" fn js_async_local_storage_enter_with(handle: Handle, store: f64) {
    if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        async_context::enter_with(handle, store);
    }
}

/// AsyncLocalStorage.exit(callback, ...args)
/// Save current stack, clear it, call callback with the forwarded rest args,
/// restore stack. `args_array` carries the `...args` packed by the codegen
/// `NA_VARARGS` lowering (#3093).
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(
    handle: Handle,
    callback: f64,
    args_array: i64,
) -> f64 {
    // Validate before clearing the context so an invalid callback throws
    // without disturbing the saved store (#3092).
    let cb = validate_callback(callback);

    let saved = if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        Some(async_context::take_store(handle))
    } else {
        None
    };

    let result = call_with_forwarded_args(cb, args_array);

    if let Some(saved) = saved {
        async_context::restore_store(handle, saved);
    }

    result
}

/// AsyncLocalStorage.disable()
/// Clear the store stack
#[no_mangle]
pub extern "C" fn js_async_local_storage_disable(handle: Handle) {
    if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        async_context::clear_store(handle);
    }
}
