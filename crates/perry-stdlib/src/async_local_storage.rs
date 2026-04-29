//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::js_closure_call0;

use crate::common::{get_handle_mut, register_handle, Handle};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// AsyncLocalStorage handle storing a stack of stores
pub struct AsyncLocalStorageHandle {
    stores: Vec<f64>,
}

impl AsyncLocalStorageHandle {
    pub fn new() -> Self {
        AsyncLocalStorageHandle { stores: Vec::new() }
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
    callback: i64,
) -> f64 {
    if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        als.stores.push(store);
    }

    let result = if callback != 0 {
        js_closure_call0(callback as *const perry_runtime::ClosureHeader)
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };

    if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        als.stores.pop();
    }

    result
}

/// AsyncLocalStorage.getStore()
/// Returns the current store (top of stack) or undefined
#[no_mangle]
pub extern "C" fn js_async_local_storage_get_store(handle: Handle) -> f64 {
    if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        if let Some(&store) = als.stores.last() {
            return store;
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// AsyncLocalStorage.enterWith(store)
/// Push store onto stack (caller is responsible for cleanup)
#[no_mangle]
pub extern "C" fn js_async_local_storage_enter_with(handle: Handle, store: f64) {
    if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        als.stores.push(store);
    }
}

/// AsyncLocalStorage.exit(callback)
/// Save current stack, clear it, call callback, restore stack
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(handle: Handle, callback: i64) -> f64 {
    let saved = if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        let saved = als.stores.clone();
        als.stores.clear();
        saved
    } else {
        Vec::new()
    };

    let result = if callback != 0 {
        js_closure_call0(callback as *const perry_runtime::ClosureHeader)
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };

    if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        als.stores = saved;
    }

    result
}

/// AsyncLocalStorage.disable()
/// Clear the store stack
#[no_mangle]
pub extern "C" fn js_async_local_storage_disable(handle: Handle) {
    if let Some(als) = get_handle_mut::<AsyncLocalStorageHandle>(handle) {
        als.stores.clear();
    }
}
