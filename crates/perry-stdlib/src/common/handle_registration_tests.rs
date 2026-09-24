use super::*;

#[test]
fn explicit_registration_rejects_live_and_leased_retired_slots() {
    let _serial = REGISTRATION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let id = register_handle(29_u64);
    let identity = common_handle_registration(id).unwrap();
    let lease = acquire_common_handle_registration(identity, NativeLeaseKind::Wrapper).unwrap();
    assert!(std::panic::catch_unwind(|| register_handle_with_id(31_u64, id)).is_err());
    assert_eq!(with_handle::<u64, _, _>(id, |value| *value), Some(29));
    assert!(drop_handle(id));
    // No global drain in this shared-process adapter suite. The local native
    // core fixtures separately prove that a held lease blocks an eligible drain.
    assert!(std::panic::catch_unwind(|| register_handle_with_id(31_u64, id)).is_err());
    assert!(common_handle_registration(id).is_none());
    drop(lease);
}

#[test]
fn common_and_ffi_payloads_have_independent_domains() {
    let _serial = REGISTRATION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let id = register_handle(47_u64);
    let identity = common_handle_registration(id).unwrap();
    assert_eq!(identity.domain(), common_handle_registry_domain());
    assert_ne!(identity.domain(), perry_ffi::handle_registry_domain());
    let ffi_id = perry_ffi::register_handle(48_u64);
    let ffi_identity = perry_ffi::handle_registration(ffi_id).unwrap();
    assert_eq!(ffi_identity.domain(), perry_ffi::handle_registry_domain());
    assert!(common_handle_registration(ffi_id).is_none());
    assert!(acquire_common_handle_registration(ffi_identity, NativeLeaseKind::Wrapper).is_none());
    assert!(
        !drop_handle(ffi_id),
        "a common drop must not retire an FFI id"
    );
    assert_eq!(perry_ffi::take_handle::<u64>(ffi_id), Some(48));
    assert_eq!(take_handle::<u64>(id), Some(47));
}

// #11196: the common registry and perry-ffi (every `perry-ext-*` wrapper) used
// to mint ids from two independent counters over the same band, so the first
// common handle and the first FFI handle were both `1`. An ext-net socket then
// read as a bundled-events EventEmitter, and `events.once(socket, …)` parked
// its promise on the emitter instead of listening on the socket.
#[test]
fn common_and_ffi_ids_never_alias() {
    let _serial = REGISTRATION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut common = Vec::new();
    let mut ffi = Vec::new();
    for n in 0..16_u64 {
        common.push(register_handle(n));
        ffi.push(perry_ffi::register_handle(n));
        ffi.push(perry_ffi::reserve_handle_id());
    }
    for id in &common {
        assert!(!ffi.contains(id), "common id {id} aliases an FFI id");
        assert!(*id > 0 && *id < COMMON_HANDLE_ID_END);
    }
    for id in common {
        assert!(drop_handle(id));
    }
    for id in ffi {
        if !perry_ffi::drop_handle(id) {
            perry_ffi::free_handle_id(id);
        }
    }
}

#[test]
fn dropped_common_ids_are_not_reused_after_a_shared_pool_drain() {
    let _serial = REGISTRATION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let id = register_handle(7_u64);
    assert!(drop_handle(id));
    // perry-ffi drains the shared pool at every tick boundary.
    perry_ffi::drain_quarantined_handles();
    drain_quarantined_common_handles();
    for n in 0..8_u64 {
        let fresh = register_handle(n);
        assert_ne!(fresh, id, "a stale common id must never name a new payload");
        assert!(drop_handle(fresh));
    }
    assert!(std::panic::catch_unwind(|| register_handle_with_id(9_u64, id)).is_err());
}

#[test]
fn cloning_creates_a_new_registration_and_removal_preserves_type_behavior() {
    let _serial = REGISTRATION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let id = register_handle(String::from("native"));
    let cloned = clone_handle::<String>(id).unwrap();
    let first = common_handle_registration(id).unwrap();
    let second = common_handle_registration(cloned).unwrap();
    assert_ne!(first, second);
    assert!(second.serial() > first.serial());
    assert!(take_handle::<u64>(id).is_none());
    assert!(!handle_exists(id));
    assert_eq!(take_handle::<String>(cloned).as_deref(), Some("native"));
}
