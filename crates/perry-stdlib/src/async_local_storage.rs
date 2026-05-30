//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::{async_context, js_closure_call0, JSValue};

use crate::common::{get_handle_mut, register_handle, Handle};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

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

fn throw_apply_type_error() -> ! {
    let msg = b"Function.prototype.apply was called on a non-function value";
    let msg_str = perry_runtime::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = perry_runtime::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    perry_runtime::exception::js_throw(f64::from_bits(err_value))
}

fn closure_ptr_from_value(callback: f64) -> i64 {
    let bits = callback.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as usize;
        if perry_runtime::closure::is_closure_ptr(ptr) {
            return ptr as i64;
        }
    }
    throw_apply_type_error()
}

unsafe fn call_callback(callback: i64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_raw_const_ptr(callback as *const perry_runtime::ClosureHeader);

    js_closure_call0(callback_handle.get_raw_const_ptr::<perry_runtime::ClosureHeader>())
}

/// AsyncLocalStorage.run(store, callback)
/// Push store onto stack, call callback, pop store, return result
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_run(
    handle: Handle,
    store: f64,
    callback: f64,
) -> f64 {
    let callback = closure_ptr_from_value(callback);

    async_context::push_store(handle, store);
    let result = call_callback(callback);
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

/// AsyncLocalStorage.exit(callback)
/// Save current stack, clear it, call callback, restore stack
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(handle: Handle, callback: f64) -> f64 {
    let callback = closure_ptr_from_value(callback);

    let saved = if get_handle_mut::<AsyncLocalStorageHandle>(handle).is_some() {
        Some(async_context::take_store(handle))
    } else {
        None
    };

    let result = call_callback(callback);

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
