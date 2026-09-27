//! Handle registry for managing opaque pointers across FFI boundaries.
//!
//! Since we can't pass Rust ownership across FFI, we store objects in a
//! registry and return integer handles to JavaScript.
//!
//! Payload-map locks never overlap the native registration-state mutex.

use std::any::Any;

use dashmap::DashMap;
use perry_ffi::{
    NativeLeaseKind, NativeRegistrationError, NativeRegistrationIdentity, NativeRegistrationKind,
    NativeRegistrationLease, NativeRegistrationRegistry, NativeRegistryDomain,
};
use std::sync::LazyLock as Lazy;

/// Handle type - an opaque integer identifier for a managed object
pub type Handle = i64;

/// Invalid handle value (null/undefined)
pub const INVALID_HANDLE: Handle = 0;

/// Global handle registry using DashMap for concurrent access
pub(super) static HANDLES: Lazy<DashMap<Handle, Box<dyn Any + Send + Sync>>> =
    Lazy::new(DashMap::new);

// Band boundary owned by `perry_runtime::value::addr_class`.
#[cfg(test)]
const COMMON_HANDLE_ID_END: Handle =
    perry_runtime::value::addr_class::COMMON_HANDLE_BAND_END as Handle;

/// Common ids come from perry-ffi's SHARED numeric pool, under this registry's
/// own domain — the same arrangement perry-ext-net uses for its sockets.
///
/// This used to be a private `NativeRegistrationRegistry` over the same
/// `[1, COMMON_HANDLE_BAND_END)` band, so it minted `1, 2, 3, …` independently
/// of perry-ffi, which every `perry-ext-*` wrapper allocates from. The first
/// bundled-events `EventEmitter` and the first ext-net socket were therefore
/// BOTH handle 1, and every consumer that asks "is this id in my registry?"
/// answered for the wrong object: `events.once(socket, 'connect')` found the
/// emitter, parked its promise there, and never listened on the socket, so
/// redis@6.1.0's `connect()` hung forever (#11196).
pub(super) static REGISTRATIONS: Lazy<NativeRegistrationRegistry> =
    Lazy::new(perry_ffi::shared_handle_id_pool);
static COMMON_DOMAIN: Lazy<NativeRegistryDomain> =
    Lazy::new(|| NativeRegistryDomain::new().expect("common native registry domains exhausted"));

pub fn common_handle_registry_domain() -> NativeRegistryDomain {
    *COMMON_DOMAIN
}

/// The shared pool also holds other owners' ids; only ours are reported.
pub fn common_handle_registration(handle: Handle) -> Option<NativeRegistrationIdentity> {
    REGISTRATIONS
        .identity(handle)
        .filter(|identity| identity.domain() == *COMMON_DOMAIN)
}

pub fn acquire_common_handle_registration(
    identity: NativeRegistrationIdentity,
    kind: NativeLeaseKind,
) -> Option<NativeRegistrationLease> {
    if identity.domain() != *COMMON_DOMAIN {
        return None;
    }
    REGISTRATIONS.acquire(identity, kind)
}

/// Native preparation API. Common retirements are parked until a full heap
/// trace proves them unreachable (see `handle_lifecycle`), never quarantined
/// on a timer, so this drain never makes a common id reusable.
pub fn drain_quarantined_common_handles() -> usize {
    REGISTRATIONS.drain(std::time::Instant::now())
}

pub fn register_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    let identity = match begin_common_registration() {
        Ok(identity) => identity,
        Err(error) => {
            drop(value);
            throw_ids_exhausted(error)
        }
    };
    publish_payload(value, identity)
}

/// Register a payload whose only owners are JavaScript values (#11453): it is
/// dropped, and its id recycled, once a full heap trace finds no value naming
/// it. Use it only for kinds no native table refers to by id — a digest, a
/// cipher, a string decoder. A payload that later gains id-keyed native state
/// must call [`retain_strongly`] first.
pub fn register_reclaimable_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    let identity = match begin_common_registration() {
        Ok(identity) => identity,
        Err(error) => {
            drop(value);
            throw_ids_exhausted(error)
        }
    };
    let handle = publish_payload(value, identity);
    super::handle_lifecycle::park_reclaimable(identity);
    handle
}

pub use super::handle_lifecycle::{parked_handle_count, retain_strongly};

fn begin_common_registration() -> Result<NativeRegistrationIdentity, NativeRegistrationError> {
    super::handle_lifecycle::note_registration();
    REGISTRATIONS.begin_registration_in_domain(*COMMON_DOMAIN, NativeRegistrationKind::Payload)
}

/// True exhaustion — every id in the shared band is live or still named by a
/// reachable JS value — is a catchable `Error` with code
/// `ERR_PERRY_HANDLE_IDS_EXHAUSTED`, not a process abort. Reclamation of
/// parked ids is requested first, so a later registration can succeed.
fn throw_ids_exhausted(error: NativeRegistrationError) -> ! {
    super::handle_lifecycle::request_full_trace();
    let message = format!(
        "Perry native handle ids exhausted ({error:?}): all {} ids of the shared handle \
         band are live or still referenced by JavaScript values",
        perry_runtime::value::addr_class::COMMON_HANDLE_BAND_END - 1
    );
    perry_runtime::fs::validate::throw_error_with_code(&message, "ERR_PERRY_HANDLE_IDS_EXHAUSTED")
}

/// Explicit insertion rejects any slot that has not completed retirement,
/// quarantine, and lease release. It never replaces an existing payload.
pub fn register_handle_with_id<T: 'static + Send + Sync>(value: T, handle: Handle) -> Handle {
    let identity = REGISTRATIONS
        .begin_registration_with_id_in_domain(
            *COMMON_DOMAIN,
            handle,
            NativeRegistrationKind::Payload,
        )
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

/// #11471: `HANDLES` is process-global, but some payload types store JS values
/// (closure pointers, NaN-boxed values, promise pointers) of the thread that
/// created or mutated them. When that thread exits its arena is freed and a
/// surviving thread may reuse the addresses, while every thread's GC scanner
/// still walks those payloads through `for_each_handle_mut_of`.
///
/// The registry is type-erased, so it cannot see inside a payload itself. A
/// payload type that holds GC values registers a releaser here (from a `Once`
/// on its own insert path, before its first GC value is stored); at thread exit
/// [`release_handle_payloads_in_freed_ranges`] hands every payload of that type
/// to it. The releaser neutralizes the slots that name freed memory and returns
/// `true` if the whole payload should instead be dropped (retired exactly like
/// `drop_handle`). A releaser runs inside the exiting thread's TLS destructor
/// with a `HANDLES` shard write-locked: it must not touch thread-locals,
/// allocate on the GC heap, call JS, or call back into this registry.
type ErasedPayloadReleaser = std::sync::Arc<
    dyn Fn(&mut (dyn Any + Send + Sync), &perry_runtime::arena::thread_exit::FreedRanges) -> bool
        + Send
        + Sync,
>;

static PAYLOAD_RELEASERS: std::sync::Mutex<Vec<(std::any::TypeId, ErasedPayloadReleaser)>> =
    std::sync::Mutex::new(Vec::new());

/// Register `release` for every `HANDLES` payload of type `T` (see
/// [`ErasedPayloadReleaser`]). Idempotent per type: the first registration wins.
pub fn register_handle_payload_releaser<T: 'static + Send + Sync>(
    release: fn(&mut T, &perry_runtime::arena::thread_exit::FreedRanges) -> bool,
) {
    static REGISTER_HOOK: std::sync::Once = std::sync::Once::new();
    REGISTER_HOOK.call_once(|| {
        perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
            release_handle_payloads_in_freed_ranges,
        )
    });
    let type_id = std::any::TypeId::of::<T>();
    let mut releasers = PAYLOAD_RELEASERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if releasers.iter().any(|(existing, _)| *existing == type_id) {
        return;
    }
    releasers.push((
        type_id,
        std::sync::Arc::new(move |payload, freed| {
            payload
                .downcast_mut::<T>()
                .is_some_and(|payload| release(payload, freed))
        }),
    ));
}

/// Thread-exit hook (#11471): run each registered payload releaser over the
/// payloads of its type, then retire the payloads a releaser asked to drop.
/// Payload types with no releaser hold no GC value and are left alone.
fn release_handle_payloads_in_freed_ranges(freed: &perry_runtime::arena::thread_exit::FreedRanges) {
    let releasers: Vec<(std::any::TypeId, ErasedPayloadReleaser)> = PAYLOAD_RELEASERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if releasers.is_empty() {
        return;
    }
    let mut to_drop = Vec::new();
    for mut entry in HANDLES.iter_mut() {
        let handle = *entry.key();
        let payload: &mut (dyn Any + Send + Sync) = entry.value_mut().as_mut();
        let type_id = (*payload).type_id();
        if let Some((_, release)) = releasers.iter().find(|(id, _)| *id == type_id) {
            if release(payload, freed) {
                to_drop.push(handle);
            }
        }
    }
    // Retired outside the iteration: `remove_payload` write-locks the shard.
    for handle in to_drop {
        drop(remove_payload(handle));
    }
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

/// Retirement parks the id until a full heap trace proves no JS value still
/// holds it (`handle_lifecycle`), then recycles it. A timer quarantine would
/// recycle ids that JS still holds as bare numbers, with no lease to stop it;
/// before #11453 the id was tombstoned forever instead, which bounded a
/// process to one band's worth of common handles.
fn remove_payload(handle: Handle) -> Option<Box<dyn Any + Send + Sync>> {
    let identity = common_handle_registration(handle)?;
    if !REGISTRATIONS.begin_retirement_of(identity) {
        return None;
    }
    let removed = HANDLES.remove(&handle).map(|(_, boxed)| boxed);
    super::handle_lifecycle::park_retired(identity);
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
pub(super) static REGISTRATION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
