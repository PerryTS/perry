//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::array::{js_array_length, ArrayHeader};
use perry_runtime::async_context;
use perry_runtime::closure::js_closure_call_array;

use crate::common::{get_handle_mut, register_handle, Handle};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Invoke `callback` with the forwarded call arguments packed by the codegen
/// `NA_VARARGS` ABI into `args_array` (a NaN-boxed `*const ArrayHeader` raw
/// pointer, or 0 when no trailing args were supplied). Returns the callback's
/// return value, or `undefined` when there is no callback.
///
/// Mirrors the array-unpacking done by `js_async_resource_run_in_async_scope`:
/// the element data lives immediately after the `ArrayHeader` and each slot is
/// already a NaN-boxed `f64`, so we hand `(data, len)` straight to
/// `js_closure_call_array`.
unsafe fn call_forwarding_args(callback: i64, args_array: i64) -> f64 {
    if callback == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if args_array == 0 {
        return js_closure_call_array(callback, std::ptr::null(), 0);
    }
    let arr = args_array as *const ArrayHeader;
    let len = js_array_length(arr) as i64;
    let data = if arr.is_null() {
        std::ptr::null()
    } else {
        (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
    };
    js_closure_call_array(callback, data, len)
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
/// Push store onto stack, call `callback(...args)`, pop store, return result.
/// `args_array` carries the forwarded trailing call arguments (codegen
/// `NA_VARARGS` ABI); they are passed through to the callback unchanged.
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_run(
    handle: Handle,
    store: f64,
    callback: i64,
    args_array: i64,
) -> f64 {
    async_context::push_store(handle, store);

    let result = call_forwarding_args(callback, args_array);

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
/// Save current stack, clear it, call `callback(...args)`, restore stack.
/// `args_array` carries the forwarded trailing call arguments (codegen
/// `NA_VARARGS` ABI); they are passed through to the callback unchanged.
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(
    handle: Handle,
    callback: i64,
    args_array: i64,
) -> f64 {
    let saved = if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        Some(async_context::take_store(handle))
    } else {
        None
    };

    let result = call_forwarding_args(callback, args_array);

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
