//! Handle registry for managing opaque pointers across FFI boundaries.
//!
//! Since we can't pass Rust ownership across FFI, we store objects in a
//! registry and return integer handles to JavaScript.
//!
//! Payload-map locks never overlap the native registration-state mutex.

use std::any::Any;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use perry_ffi::{
    NativeLeaseKind, NativeQuarantine, NativeRegistrationIdentity, NativeRegistrationKind,
    NativeRegistrationLease, NativeRegistrationRegistry, NativeRegistryDomain,
};

/// Handle type - an opaque integer identifier for a managed object
pub type Handle = i64;

/// Invalid handle value (null/undefined)
pub const INVALID_HANDLE: Handle = 0;

/// Global handle registry using DashMap for concurrent access
static HANDLES: Lazy<DashMap<Handle, Box<dyn Any + Send + Sync>>> = Lazy::new(DashMap::new);

// Band boundary owned by `perry_runtime::value::addr_class`.
const COMMON_HANDLE_ID_START: Handle = 1;
const COMMON_HANDLE_ID_END: Handle =
    perry_runtime::value::addr_class::COMMON_HANDLE_BAND_END as Handle;

static REGISTRATIONS: Lazy<NativeRegistrationRegistry> = Lazy::new(|| {
    NativeRegistrationRegistry::new(COMMON_HANDLE_ID_START, COMMON_HANDLE_ID_END, 64 * 1024)
});

pub fn common_handle_registry_domain() -> NativeRegistryDomain {
    REGISTRATIONS.domain()
}

pub fn common_handle_registration(handle: Handle) -> Option<NativeRegistrationIdentity> {
    REGISTRATIONS.identity(handle)
}

pub fn acquire_common_handle_registration(
    identity: NativeRegistrationIdentity,
    kind: NativeLeaseKind,
) -> Option<NativeRegistrationLease> {
    REGISTRATIONS.acquire(identity, kind)
}

/// Native preparation API. Existing publication paths do not drive this drain.
pub fn drain_quarantined_common_handles() -> usize {
    REGISTRATIONS.drain(std::time::Instant::now())
}

pub fn register_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    let identity = REGISTRATIONS
        .begin_registration(NativeRegistrationKind::Payload)
        .expect("common native handle registration exhausted");
    publish_payload(value, identity)
}

/// Explicit insertion rejects any slot that has not completed retirement,
/// quarantine, and lease release. It never replaces an existing payload.
pub fn register_handle_with_id<T: 'static + Send + Sync>(value: T, handle: Handle) -> Handle {
    let identity = REGISTRATIONS
        .begin_registration_with_id(handle, NativeRegistrationKind::Payload)
        .expect("common explicit native handle id is unavailable");
    publish_payload(value, identity)
}

fn publish_payload<T: 'static + Send + Sync>(
    value: T,
    identity: NativeRegistrationIdentity,
) -> Handle {
    let handle = identity.numeric_id();
    let previous = HANDLES.insert(handle, Box::new(value));
    assert!(
        previous.is_none(),
        "pending Common id must have an empty payload slot"
    );
    assert!(REGISTRATIONS.publish(identity));
    if perry_runtime::hot_diag::receiver_repr_on() {
        perry_runtime::hot_diag::receiver_repr_note_constructed(
            perry_runtime::hot_diag::ReceiverReprFamily::Common,
        );
    }
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
    remove_payload(handle)
        .and_then(|boxed| boxed.downcast::<T>().ok())
        .map(|boxed| *boxed)
}

pub fn drop_handle(handle: Handle) -> bool {
    remove_payload(handle).is_some()
}

fn remove_payload(handle: Handle) -> Option<Box<dyn Any + Send + Sync>> {
    let identity = REGISTRATIONS.begin_retirement(handle, NativeRegistrationKind::Payload)?;
    let removed = HANDLES.remove(&handle).map(|(_, boxed)| boxed);
    assert!(REGISTRATIONS.finish_retirement(identity, NativeQuarantine::NextDrain));
    removed
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

/// Like `for_each_handle_of`, but yields handle ids instead of refs.
/// Callers need the id so they can later mutate the entry (`get_handle_mut`)
/// or call `take_handle`/`drop_handle` — neither of which is possible
/// while holding the iterator's read lock on the DashMap. The fastify
/// pump uses this to drain per-server `request_rx` channels without
/// keeping the registry locked across the dispatch (which may itself
/// register/drop handles).
pub fn iter_handle_ids_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(Handle),
{
    for entry in HANDLES.iter() {
        if entry.value().downcast_ref::<T>().is_some() {
            f(*entry.key());
        }
    }
}

/// Walk every registered handle whose value downcasts to `T`, calling
/// `f(&mut T)` for each match. Mutable GC root scanners use this to
/// expose stdlib-owned handle slots so copied minor GC can rewrite moved
/// references in place.
pub fn for_each_handle_mut_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(&mut T),
{
    for mut entry in HANDLES.iter_mut() {
        if let Some(v) = entry.value_mut().downcast_mut::<T>() {
            f(v);
        }
    }
}

/// Clone a handle's value if it implements Clone
pub fn clone_handle<T: 'static + Send + Sync + Clone>(handle: Handle) -> Option<Handle> {
    let cloned = HANDLES
        .get(&handle)
        .and_then(|entry| entry.value().downcast_ref::<T>().cloned());
    cloned.map(register_handle)
}

// Adapter tests share ordering with the original handle tests. They do not
// drive the process-global Common drain: other stdlib modules still use bare ids.
#[cfg(test)]
static REGISTRATION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get() {
        let _serial = REGISTRATION_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let value = String::from("test");
        let handle = register_handle(value);

        assert!(handle != INVALID_HANDLE);
        assert!(handle < COMMON_HANDLE_ID_END);

        let retrieved: Option<&String> = get_handle(handle);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), "test");
    }

    #[test]
    fn test_take_handle() {
        let _serial = REGISTRATION_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let value = 42i32;
        let handle = register_handle(value);

        let taken: Option<i32> = take_handle(handle);
        assert_eq!(taken, Some(42));

        // Handle should no longer exist
        let retrieved: Option<&i32> = get_handle(handle);
        assert!(retrieved.is_none());
    }
}

#[cfg(test)]
#[path = "handle_registration_tests.rs"]
mod registration_tests;
