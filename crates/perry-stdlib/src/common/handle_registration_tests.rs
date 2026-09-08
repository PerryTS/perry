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
    assert!(perry_ffi::acquire_handle_registration(identity, NativeLeaseKind::Wrapper).is_none());
    assert_eq!(take_handle::<u64>(id), Some(47));
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
