use super::*;

#[test]
fn payload_retirement_preserves_leased_identity_and_kind() {
    let _serial = super::tests::recycle_serial();
    let id = register_handle(41_u64);
    let identity = handle_registration(id).unwrap();
    assert_eq!(identity.domain(), handle_registry_domain());
    let wrapper = acquire_handle_registration(identity, NativeLeaseKind::Wrapper).unwrap();
    let operation = acquire_handle_registration(identity, NativeLeaseKind::Operation).unwrap();
    free_handle_id(id);
    assert_eq!(with_handle::<u64, _, _>(id, |value| *value), Some(41));
    assert_eq!(take_handle::<u64>(id), Some(41));
    assert!(!handle_exists(id));
    assert!(handle_registration(id).is_none());
    super::tests::collect_unobserved();
    assert_eq!(
        REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Payload),
        Err(crate::NativeRegistrationError::Occupied)
    );
    drop(wrapper);
    super::tests::collect_unobserved();
    assert_eq!(
        REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Payload),
        Err(crate::NativeRegistrationError::Occupied)
    );
    drop(operation);
}

#[test]
fn worker_retirement_uses_native_state_and_type_mismatch_still_removes() {
    let _serial = super::tests::recycle_serial();
    let id = register_handle(17_u64);
    let identity = handle_registration(id).unwrap();
    let lease = acquire_handle_registration(identity, NativeLeaseKind::Operation).unwrap();
    std::thread::spawn(move || {
        assert!(take_handle::<String>(id).is_none());
        assert!(!handle_exists(id));
    })
    .join()
    .unwrap();
    assert!(
        acquire_handle_registration(identity, NativeLeaseKind::Wrapper).is_none(),
        "worker retirement must end native availability"
    );
    assert_eq!(
        REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Reserved),
        Err(crate::NativeRegistrationError::Occupied)
    );
    drop(lease);
    // The worker has no heap to decide the id: it is orphaned, adopted by the
    // next trace here, and released only by two later traces that name nothing.
    crate::handle_lifecycle::trace_phase_for_tests(0);
    crate::handle_lifecycle::trace_phase_for_tests(1);
    assert_eq!(
        REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Reserved),
        Err(crate::NativeRegistrationError::Occupied),
        "the adopting trace decides nothing"
    );
    super::tests::collect_unobserved();
    let replacement = REGISTRATIONS
        .begin_registration_with_id(id, NativeRegistrationKind::Reserved)
        .expect("worker retirement must complete quarantine before reuse");
    assert_ne!(replacement.serial(), identity.serial());
    assert!(REGISTRATIONS.publish(replacement));
    free_handle_id(replacement.numeric_id());
}

#[test]
fn payload_destructor_can_reenter_registration() {
    let _serial = super::tests::recycle_serial();
    struct Reenter(std::sync::mpsc::Sender<()>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            let other = register_handle(5_u64);
            assert!(drop_handle(other));
            self.0.send(()).unwrap();
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let id = register_handle(Reenter(tx));
    let worker = std::thread::spawn(move || assert!(drop_handle(id)));
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .expect("payload destructor must run outside registry locks");
    worker.join().unwrap();
}

#[test]
fn duplicate_reserved_free_preserves_one_retirement() {
    let _serial = super::tests::recycle_serial();
    let domain = NativeRegistryDomain::new().unwrap();
    let id = reserve_handle_id_in_domain(domain);
    let identity = handle_registration(id).expect("reservation must create a native registration");
    assert_eq!(identity.domain(), domain);
    let lease = acquire_handle_registration(identity, NativeLeaseKind::Operation).unwrap();
    free_handle_id(id);
    free_handle_id_until(id, Instant::now());
    assert!(handle_registration(id).is_none());
    super::tests::collect_unobserved();
    assert_eq!(
        REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Reserved),
        Err(crate::NativeRegistrationError::Occupied)
    );
    drop(lease);
}
