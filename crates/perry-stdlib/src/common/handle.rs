//! Handle registry for managing opaque pointers across FFI boundaries.
//!
//! Since we can't pass Rust ownership across FFI, we store objects in a
//! registry and return integer handles to JavaScript.
//!
//! Uses DashMap for lock-free concurrent access, avoiding deadlocks that
//! would occur with Mutex-based approaches.

use std::any::Any;
use std::sync::atomic::{AtomicI64, Ordering};

use dashmap::DashMap;
use once_cell::sync::Lazy;

/// Handle type - an opaque integer identifier for a managed object
pub type Handle = i64;

/// Invalid handle value (null/undefined)
pub const INVALID_HANDLE: Handle = 0;

/// Global handle registry using DashMap for concurrent access
static HANDLES: Lazy<DashMap<Handle, Box<dyn Any + Send + Sync>>> = Lazy::new(DashMap::new);

/// Next handle ID (0 is reserved for invalid/null)
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);

/// Register an object and get a handle to it
pub fn register_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    HANDLES.insert(handle, Box::new(value));
    handle
}

/// Get a reference to a registered object and execute a closure with it.
/// This is the safe way to access handle data without lifetime issues.
pub fn with_handle<T: 'static + Send + Sync, R, F: FnOnce(&T) -> R>(
    handle: Handle,
    f: F,
) -> Option<R> {
    HANDLES
        .get(&handle)
        .and_then(|entry| entry.value().downcast_ref::<T>().map(f))
}

/// Get a reference to a registered object.
/// SAFETY: The returned reference is only valid while the handle exists.
/// The caller must ensure the handle is not removed while the reference is in use.
pub fn get_handle<T: 'static + Send + Sync>(handle: Handle) -> Option<&'static T> {
    // SAFETY: We're returning a 'static reference by keeping the entry in the map.
    // This is safe as long as the handle is not removed while in use.
    // DashMap entries are stable (not moved) as long as they exist.
    HANDLES.get(&handle).and_then(|entry| {
        let ptr = entry.value().downcast_ref::<T>()? as *const T;
        // The reference is valid as long as the entry exists in the map
        Some(unsafe { &*ptr })
    })
}

/// Get a mutable reference to a registered object (use with caution)
pub fn get_handle_mut<T: 'static + Send + Sync>(handle: Handle) -> Option<&'static mut T> {
    HANDLES.get_mut(&handle).and_then(|mut entry| {
        let ptr = entry.value_mut().downcast_mut::<T>()? as *mut T;
        Some(unsafe { &mut *ptr })
    })
}

/// Remove and return a registered object
pub fn take_handle<T: 'static + Send + Sync>(handle: Handle) -> Option<T> {
    HANDLES
        .remove(&handle)
        .and_then(|(_, boxed)| boxed.downcast::<T>().ok())
        .map(|b| *b)
}

/// Remove a handle without returning the value (drop it)
pub fn drop_handle(handle: Handle) -> bool {
    HANDLES.remove(&handle).is_some()
}

/// Check if a handle exists
pub fn handle_exists(handle: Handle) -> bool {
    HANDLES.contains_key(&handle)
}

/// Diagnostic: total number of registered handles.
/// Useful for detecting handle leaks in long-running services.
#[no_mangle]
pub extern "C" fn js_handle_count() -> i64 {
    HANDLES.len() as i64
}

/// Walk every registered handle whose value downcasts to `T`, calling
/// `f(&T)` for each match. Used by stdlib GC root scanners (ws, http,
/// events, fastify) to mark user closures stored in handle-registered
/// structs — without this, a malloc-triggered GC between closure
/// registration and dispatch would sweep them (issue #35 pattern).
pub fn for_each_handle_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(&T),
{
    for entry in HANDLES.iter() {
        if let Some(v) = entry.value().downcast_ref::<T>() {
            f(v);
        }
    }
}

/// Clone a handle's value if it implements Clone
pub fn clone_handle<T: 'static + Send + Sync + Clone>(handle: Handle) -> Option<Handle> {
    HANDLES.get(&handle).and_then(|entry| {
        entry
            .value()
            .downcast_ref::<T>()
            .map(|value| register_handle(value.clone()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get() {
        let value = String::from("test");
        let handle = register_handle(value);

        assert!(handle != INVALID_HANDLE);

        let retrieved: Option<&String> = get_handle(handle);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), "test");
    }

    #[test]
    fn test_take_handle() {
        let value = 42i32;
        let handle = register_handle(value);

        let taken: Option<i32> = take_handle(handle);
        assert_eq!(taken, Some(42));

        // Handle should no longer exist
        let retrieved: Option<&i32> = get_handle(handle);
        assert!(retrieved.is_none());
    }
}
