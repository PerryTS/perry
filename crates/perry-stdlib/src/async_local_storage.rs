//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::array::{js_array_length, ArrayHeader};
use perry_runtime::async_context;
use perry_runtime::closure::{is_closure_ptr, js_closure_call_array, ClosureHeader};
use std::ptr;

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

unsafe fn call_callback_with_args(cb: *const ClosureHeader, args_array: *const ArrayHeader) -> f64 {
    let len = if args_array.is_null() {
        0
    } else {
        js_array_length(args_array) as i64
    };
    let data = if args_array.is_null() {
        ptr::null()
    } else {
        (args_array as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
    };
    js_closure_call_array(cb as i64, data, len)
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

/// AsyncLocalStorage.run(store, callback)
/// Push store onto stack, call callback, pop store, return result
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_run(
    handle: Handle,
    store: f64,
    callback: f64,
    args_array: *const ArrayHeader,
) -> f64 {
    // Validate before mutating the async context so an invalid callback throws
    // without leaving a pushed store behind (#3092).
    let cb = validate_callback(callback);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let cb_handle = scope.root_raw_const_ptr(cb);
    let args_handle = scope.root_raw_const_ptr(args_array);
    let store_handle = scope.root_nanbox_f64(store);

    async_context::push_store(handle, store_handle.get_nanbox_f64());
    let result = call_callback_with_args(
        cb_handle.get_raw_const_ptr::<ClosureHeader>(),
        args_handle.get_raw_const_ptr::<ArrayHeader>(),
    );
    let result_handle = scope.root_nanbox_f64(result);
    async_context::pop_store(handle);

    result_handle.get_nanbox_f64()
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

/// AsyncLocalStorage.exit(callback)
/// Save current stack, clear it, call callback, restore stack
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(
    handle: Handle,
    callback: f64,
    args_array: *const ArrayHeader,
) -> f64 {
    // Validate before clearing the context so an invalid callback throws
    // without disturbing the saved store (#3092).
    let cb = validate_callback(callback);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let cb_handle = scope.root_raw_const_ptr(cb);
    let args_handle = scope.root_raw_const_ptr(args_array);

    let mut saved = if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        Some(async_context::take_store(handle))
    } else {
        None
    };
    let saved_roots = saved
        .as_ref()
        .and_then(|stores| stores.as_ref())
        .map(|stores| {
            stores
                .iter()
                .map(|store| scope.root_nanbox_f64(*store))
                .collect::<Vec<_>>()
        });

    let result = call_callback_with_args(
        cb_handle.get_raw_const_ptr::<ClosureHeader>(),
        args_handle.get_raw_const_ptr::<ArrayHeader>(),
    );
    let result_handle = scope.root_nanbox_f64(result);

    if let (Some(Some(stores)), Some(root_handles)) = (saved.as_mut(), saved_roots.as_ref()) {
        for (store, root) in stores.iter_mut().zip(root_handles.iter()) {
            *store = root.get_nanbox_f64();
        }
    }
    if let Some(saved) = saved {
        async_context::restore_store(handle, saved);
    }

    result_handle.get_nanbox_f64()
}

/// AsyncLocalStorage.disable()
/// Clear the store stack
#[no_mangle]
pub extern "C" fn js_async_local_storage_disable(handle: Handle) {
    if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        async_context::clear_store(handle);
    }
}
