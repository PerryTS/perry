//! External Events owns a domain independently of FFI's numeric allocator.
//! Registry-state and payload-vector locks are never held together.

use super::{
    EventEmitterHandle, Handle, EVENT_EMITTER_HANDLE_ID_END, EVENT_EMITTER_HANDLE_ID_START,
};
#[cfg(test)]
use perry_ffi::NativeQuarantine;
use perry_ffi::{
    NativeLeaseKind, NativeRegistrationIdentity, NativeRegistrationKind, NativeRegistrationLease,
    NativeRegistrationRegistry, NativeRegistryDomain,
};
use std::sync::{LazyLock, Mutex, MutexGuard};

type EventEmitterRegistry = Vec<Option<Box<EventEmitterHandle>>>;
static EVENT_EMITTERS: Mutex<EventEmitterRegistry> = Mutex::new(Vec::new());
static REGISTRATIONS: LazyLock<NativeRegistrationRegistry> = LazyLock::new(|| {
    NativeRegistrationRegistry::new(
        EVENT_EMITTER_HANDLE_ID_START,
        EVENT_EMITTER_HANDLE_ID_END,
        32 * 1024,
    )
});

pub fn event_emitter_registry_domain() -> NativeRegistryDomain {
    REGISTRATIONS.domain()
}
pub fn event_emitter_registration(id: Handle) -> Option<NativeRegistrationIdentity> {
    REGISTRATIONS.identity(id)
}
pub fn acquire_event_emitter_registration(
    identity: NativeRegistrationIdentity,
    kind: NativeLeaseKind,
) -> Option<NativeRegistrationLease> {
    REGISTRATIONS.acquire(identity, kind)
}

/// Native preparation API; production has no payload retirement path yet.
pub fn drain_quarantined_event_emitter_handles() -> usize {
    REGISTRATIONS.drain(std::time::Instant::now())
}

pub(super) fn lock_event_emitters() -> MutexGuard<'static, EventEmitterRegistry> {
    EVENT_EMITTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn handle_index(handle: Handle) -> Option<usize> {
    if !(EVENT_EMITTER_HANDLE_ID_START..EVENT_EMITTER_HANDLE_ID_END).contains(&handle) {
        return None;
    }
    Some((handle - EVENT_EMITTER_HANDLE_ID_START) as usize)
}

pub(super) fn register_event_emitter_handle(value: EventEmitterHandle) -> Handle {
    let identity = REGISTRATIONS
        .begin_registration(NativeRegistrationKind::Payload)
        .expect("perry-ext-events handle registration exhausted");
    let handle = identity.numeric_id();
    let idx = handle_index(handle).expect("allocated EventEmitter id must be in range");
    {
        let mut registry = lock_event_emitters();
        if idx >= registry.len() {
            registry.resize_with(idx + 1, || None);
        }
        assert!(
            registry[idx].is_none(),
            "pending EventEmitter id must have an empty slot"
        );
        registry[idx] = Some(Box::new(value));
    }
    assert!(REGISTRATIONS.publish(identity));
    handle
}

pub(super) fn get_event_emitter_mut(handle: Handle) -> Option<&'static mut EventEmitterHandle> {
    let idx = handle_index(handle)?;
    let ptr = {
        let mut registry = lock_event_emitters();
        &mut **registry.get_mut(idx)?.as_mut()? as *mut EventEmitterHandle
    };
    // The existing provider contract orders payload removal after these borrows.
    Some(unsafe { &mut *ptr })
}

pub(super) fn is_local_event_emitter_handle(handle: Handle) -> bool {
    let Some(idx) = handle_index(handle) else {
        return false;
    };
    lock_event_emitters()
        .get(idx)
        .is_some_and(|slot| slot.is_some())
}

#[cfg(test)]
pub(super) fn drop_event_emitter_handle(handle: Handle) -> bool {
    let Some(identity) = REGISTRATIONS.begin_retirement(handle, NativeRegistrationKind::Payload)
    else {
        return false;
    };
    let removed = {
        let mut registry = lock_event_emitters();
        registry
            .get_mut(handle_index(handle).unwrap())
            .and_then(Option::take)
    };
    assert!(REGISTRATIONS.finish_retirement(identity, NativeQuarantine::NextDrain));
    removed.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_event_emitter_registration_blocks_empty_slot_reuse() {
        let id = register_event_emitter_handle(EventEmitterHandle::new());
        let identity = event_emitter_registration(id).unwrap();
        let lease = acquire_event_emitter_registration(identity, NativeLeaseKind::Wrapper).unwrap();
        assert!(drop_event_emitter_handle(id));
        drain_quarantined_event_emitter_handles();
        assert_eq!(
            REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Payload),
            Err(perry_ffi::NativeRegistrationError::Occupied)
        );
        let other = register_event_emitter_handle(EventEmitterHandle::new());
        assert_ne!(
            other, id,
            "retained Events registration must block empty-slot selection"
        );
        assert_ne!(
            event_emitter_registry_domain(),
            perry_ffi::handle_registry_domain()
        );
        drop(lease);
        assert!(drop_event_emitter_handle(other));
    }
}
