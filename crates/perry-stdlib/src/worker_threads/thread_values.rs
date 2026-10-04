//! Per-thread values of the `worker_threads` namespace: `isMainThread`,
//! `threadId`, `parentPort`, `threadName` and `resourceLimits`. The runtime
//! reads them through getters registered at startup, so `getBuiltinModule`
//! and `import * as` see the same per-thread values as named imports.

use super::*;

/// `threadId`: 0 on the main thread, the Worker id inside a worker.
#[no_mangle]
pub extern "C" fn js_worker_threads_thread_id() -> f64 {
    CURRENT_WORKER_ID.with(|id| id.get()) as f64
}

#[no_mangle]
pub extern "C" fn js_worker_threads_is_main_thread() -> f64 {
    js_bool(CURRENT_WORKER_ID.with(|id| id.get()) == 0)
}

/// Get parentPort handle (returns NaN-boxed POINTER_TAG handle)
#[no_mangle]
pub extern "C" fn js_worker_threads_parent_port() -> f64 {
    if CURRENT_WORKER_ID.with(|id| id.get()) != 0 {
        return object_value(parent_port::worker_parent_port_object());
    }
    // On the main thread there is no parent port: Node exposes `parentPort`
    // as `null` (only a spawned Worker has a MessagePort back to its parent).
    // Returning a `{}` object here made `if (parentPort)` truthy on the main
    // thread, diverging from Node.
    js_null()
}

#[no_mangle]
pub extern "C" fn js_worker_threads_thread_name() -> f64 {
    CURRENT_THREAD_NAME.with(|slot| string_value(&slot.borrow()))
}

#[no_mangle]
pub extern "C" fn js_worker_threads_resource_limits() -> f64 {
    if CURRENT_WORKER_ID.with(|id| id.get()) == 0 {
        return object_value(empty_object());
    }
    CURRENT_RESOURCE_LIMITS
        .with(|limits| object_value(worker_resource_limits_object(&limits.get())))
}
