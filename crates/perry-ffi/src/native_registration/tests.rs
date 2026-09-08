use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn registry() -> NativeRegistrationRegistry {
    NativeRegistrationRegistry::new(1, 16, 8)
}
fn live(
    registry: &NativeRegistrationRegistry,
    kind: NativeRegistrationKind,
) -> NativeRegistrationIdentity {
    let identity = registry.begin_registration(kind).unwrap();
    assert!(registry.publish(identity));
    identity
}
fn retire(
    registry: &NativeRegistrationRegistry,
    identity: NativeRegistrationIdentity,
    quarantine: NativeQuarantine,
) {
    assert!(registry.begin_retirement_of(identity));
    assert!(registry.finish_retirement(identity, quarantine));
}

#[test]
fn equal_numeric_ids_keep_domains_distinct() {
    let left = registry();
    let right = registry();
    let a = live(&left, NativeRegistrationKind::Payload);
    let b = live(&right, NativeRegistrationKind::Payload);
    assert_eq!(a.numeric_id(), b.numeric_id());
    assert_ne!(
        a.domain(),
        b.domain(),
        "independent registries must have distinct domains"
    );
    assert_ne!(a, b);
    assert!(left.acquire(b, NativeLeaseKind::Wrapper).is_none());
    retire(&left, a, NativeQuarantine::NextDrain);
    assert_eq!(right.identity(b.numeric_id()), Some(b));
}

#[test]
fn reused_numeric_id_receives_a_new_serial() {
    let registry = registry();
    let first = live(&registry, NativeRegistrationKind::Payload);
    retire(&registry, first, NativeQuarantine::NextDrain);
    assert_eq!(registry.drain(Instant::now()), 1);
    let second = live(&registry, NativeRegistrationKind::Payload);
    assert_eq!(first.numeric_id(), second.numeric_id());
    assert!(
        second.serial() > first.serial(),
        "re-registration must advance its serial"
    );
    assert!(registry.acquire(first, NativeLeaseKind::Wrapper).is_none());
    assert!(!registry.begin_retirement_of(first));
    assert_eq!(registry.identity(second.numeric_id()), Some(second));
}

#[test]
fn serial_exhaustion_never_wraps() {
    let counter = AtomicU64::new(u64::MAX - 1);
    assert_eq!(issue_serial(&counter), Ok(u64::MAX - 1));
    assert_eq!(
        issue_serial(&counter),
        Err(NativeRegistrationError::SerialExhausted),
        "serial exhaustion must reject issuance before wrap"
    );
    assert_eq!(
        issue_serial(&counter),
        Err(NativeRegistrationError::SerialExhausted)
    );
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
}

#[test]
fn pending_and_retiring_slots_cannot_be_acquired_or_reused() {
    let registry = registry();
    let identity = registry
        .begin_registration(NativeRegistrationKind::Payload)
        .unwrap();
    assert!(registry.identity(identity.numeric_id()).is_none());
    assert!(registry
        .acquire(identity, NativeLeaseKind::Wrapper)
        .is_none());
    assert_eq!(
        registry.begin_registration_with_id(identity.numeric_id(), NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::Occupied)
    );
    assert!(registry.publish(identity));
    assert!(!registry.publish(identity));
    assert!(registry.begin_retirement_of(identity));
    assert!(registry
        .acquire(identity, NativeLeaseKind::Operation)
        .is_none());
    assert_eq!(registry.drain(Instant::now()), 0);
    assert_eq!(
        registry.begin_registration_with_id(identity.numeric_id(), NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::Occupied)
    );
    assert!(registry.finish_retirement(identity, NativeQuarantine::NextDrain));
}

#[test]
fn explicit_ids_reserve_the_counter_path_and_reject_occupied_slots() {
    let registry = registry();
    let explicit = registry
        .begin_registration_with_id(1, NativeRegistrationKind::Payload)
        .unwrap();
    assert!(registry.publish(explicit));
    assert_eq!(
        registry.begin_registration_with_id(1, NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::Occupied),
        "explicit insertion must reject an occupied registration"
    );
    let ordinary = live(&registry, NativeRegistrationKind::Payload);
    assert_eq!(
        ordinary.numeric_id(),
        2,
        "ordinary allocation must skip an explicit slot"
    );
    assert_eq!(
        registry.begin_registration_with_id(0, NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::InvalidId)
    );
    assert_eq!(
        registry.begin_registration_with_id(16, NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::InvalidId)
    );
}

#[test]
fn explicit_reuse_consumes_its_freelist_row() {
    let registry = registry();
    let first = live(&registry, NativeRegistrationKind::Payload);
    retire(&registry, first, NativeQuarantine::NextDrain);
    registry.drain(Instant::now());
    let explicit = registry
        .begin_registration_with_id(first.numeric_id(), NativeRegistrationKind::Payload)
        .unwrap();
    assert!(registry.publish(explicit));
    assert!(
        registry.lock().free.is_empty(),
        "explicit reuse must consume its freelist row"
    );
    assert_ne!(
        live(&registry, NativeRegistrationKind::Payload).numeric_id(),
        explicit.numeric_id()
    );
}

#[test]
fn reserved_slots_retire_once_and_cannot_retire_payload_slots() {
    let registry = registry();
    let reserved = live(&registry, NativeRegistrationKind::Reserved);
    let payload = live(&registry, NativeRegistrationKind::Payload);
    assert!(registry
        .begin_retirement(payload.numeric_id(), NativeRegistrationKind::Reserved)
        .is_none());
    assert!(registry
        .begin_retirement(0, NativeRegistrationKind::Reserved)
        .is_none());
    assert_eq!(
        registry.begin_retirement(reserved.numeric_id(), NativeRegistrationKind::Reserved),
        Some(reserved)
    );
    assert!(
        registry
            .begin_retirement(reserved.numeric_id(), NativeRegistrationKind::Reserved)
            .is_none(),
        "reserved retirement must begin only once"
    );
    assert!(registry.finish_retirement(reserved, NativeQuarantine::NextDrain));
    assert!(
        !registry.finish_retirement(reserved, NativeQuarantine::NextDrain),
        "completed retirement must not queue twice"
    );
    assert_eq!(registry.drain(Instant::now()), 1);
    assert_eq!(registry.drain(Instant::now()), 0);
    assert_eq!(registry.identity(payload.numeric_id()), Some(payload));
}

#[test]
fn wrapper_lease_blocks_ordinary_reuse_until_release_and_drain() {
    let registry = registry();
    let identity = live(&registry, NativeRegistrationKind::Payload);
    let lease = registry
        .acquire(identity, NativeLeaseKind::Wrapper)
        .unwrap();
    retire(&registry, identity, NativeQuarantine::NextDrain);
    assert_eq!(
        registry.drain(Instant::now()),
        0,
        "leased retired id must stay out of freelist"
    );
    assert_eq!(
        registry.begin_registration_with_id(identity.numeric_id(), NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::Occupied)
    );
    drop(lease);
    assert_ne!(
        live(&registry, NativeRegistrationKind::Payload).numeric_id(),
        identity.numeric_id()
    );
    assert_eq!(registry.drain(Instant::now()), 1);
    assert_eq!(
        live(&registry, NativeRegistrationKind::Payload).numeric_id(),
        identity.numeric_id()
    );
}

#[test]
fn operation_lease_outlives_wrapper_lease() {
    let registry = registry();
    let identity = live(&registry, NativeRegistrationKind::Payload);
    let wrapper = registry
        .acquire(identity, NativeLeaseKind::Wrapper)
        .unwrap();
    let operation = registry
        .acquire(identity, NativeLeaseKind::Operation)
        .unwrap();
    assert_eq!(operation.kind(), NativeLeaseKind::Operation);
    assert_eq!(operation.identity(), identity);
    retire(&registry, identity, NativeQuarantine::NextDrain);
    drop(wrapper);
    assert_eq!(
        registry.drain(Instant::now()),
        0,
        "operation lease must block reuse after wrapper release"
    );
    drop(operation);
    assert_eq!(registry.drain(Instant::now()), 1);
}

#[test]
fn deadline_and_lease_are_independent_conditions() {
    let registry = registry();
    let now = Instant::now();
    let deadline = now + Duration::from_secs(10);
    let identity = live(&registry, NativeRegistrationKind::Reserved);
    let lease = registry
        .acquire(identity, NativeLeaseKind::Wrapper)
        .unwrap();
    retire(&registry, identity, NativeQuarantine::Until(deadline));
    assert_eq!(
        registry.drain(deadline),
        0,
        "elapsed deadline cannot bypass a lease"
    );
    drop(lease);
    assert_eq!(
        registry.drain(now),
        0,
        "zero leases cannot bypass a future deadline"
    );
    assert_eq!(registry.drain(deadline), 1);
    assert_eq!(
        live(&registry, NativeRegistrationKind::Reserved).numeric_id(),
        identity.numeric_id()
    );
}

#[test]
fn ordinary_quarantine_requires_a_drain_boundary() {
    let registry = registry();
    let identity = live(&registry, NativeRegistrationKind::Payload);
    retire(&registry, identity, NativeQuarantine::NextDrain);
    assert_ne!(
        live(&registry, NativeRegistrationKind::Payload).numeric_id(),
        identity.numeric_id()
    );
    assert_eq!(registry.drain(Instant::now()), 1);
    assert_eq!(
        live(&registry, NativeRegistrationKind::Payload).numeric_id(),
        identity.numeric_id()
    );
}

#[test]
fn publication_before_worker_retirement_retains_its_registration() {
    let registry = registry();
    let identity = live(&registry, NativeRegistrationKind::Payload);
    let worker_registry = registry.clone();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        acquired_rx.recv().unwrap();
        retire(&worker_registry, identity, NativeQuarantine::NextDrain);
        assert_eq!(
            worker_registry.drain(Instant::now()),
            0,
            "publication ordered before retirement must retain its lease"
        );
    });
    let lease = registry
        .acquire(identity, NativeLeaseKind::Wrapper)
        .unwrap();
    acquired_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(lease.identity(), identity);
    drop(lease);
    assert_eq!(registry.drain(Instant::now()), 1);
}

#[test]
fn worker_retirement_before_publication_rejects_acquisition() {
    let registry = registry();
    let identity = live(&registry, NativeRegistrationKind::Payload);
    let worker_registry = registry.clone();
    std::thread::spawn(move || retire(&worker_registry, identity, NativeQuarantine::NextDrain))
        .join()
        .unwrap();
    assert!(
        registry
            .acquire(identity, NativeLeaseKind::Wrapper)
            .is_none(),
        "retirement ordered before publication must reject acquisition"
    );
    assert_eq!(registry.drain(Instant::now()), 1);
    let replacement = live(&registry, NativeRegistrationKind::Payload);
    assert_eq!(replacement.numeric_id(), identity.numeric_id());
    assert!(registry
        .acquire(identity, NativeLeaseKind::Operation)
        .is_none());
}

#[test]
fn ordinary_overflow_abandons_reuse_without_retaining_queue_ownership() {
    let registry = NativeRegistrationRegistry::new(1, 8, 1);
    let queued = live(&registry, NativeRegistrationKind::Payload);
    let overflow = live(&registry, NativeRegistrationKind::Payload);
    let lease = registry
        .acquire(overflow, NativeLeaseKind::Operation)
        .unwrap();
    retire(&registry, queued, NativeQuarantine::NextDrain);
    retire(&registry, overflow, NativeQuarantine::NextDrain);
    assert_eq!(
        registry.lock().ordinary,
        vec![queued.numeric_id()],
        "ordinary quarantine capacity must hold"
    );
    assert_eq!(
        registry.lock().slots[&overflow.numeric_id()].phase,
        Phase::Abandoned
    );
    drop(lease);
    assert_eq!(registry.drain(Instant::now()), 1);
    assert_eq!(
        registry.begin_registration_with_id(overflow.numeric_id(), NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::Occupied),
        "abandoned slot must reject explicit reuse"
    );
    let weak = Arc::downgrade(&registry.0);
    drop(registry);
    assert!(
        weak.upgrade().is_none(),
        "discarded queue rows must not retain native registry ownership"
    );
}

#[test]
fn deadline_overflow_and_full_freelist_abandon_reuse() {
    let registry = NativeRegistrationRegistry::new(1, 8, 1);
    let now = Instant::now();
    let ordinary = live(&registry, NativeRegistrationKind::Reserved);
    let deadline = live(&registry, NativeRegistrationKind::Reserved);
    let overflow = live(&registry, NativeRegistrationKind::Reserved);
    retire(&registry, ordinary, NativeQuarantine::NextDrain);
    retire(&registry, deadline, NativeQuarantine::Until(now));
    retire(&registry, overflow, NativeQuarantine::Until(now));
    assert_eq!(
        registry.lock().deadlines,
        vec![deadline.numeric_id()],
        "deadline quarantine capacity must hold"
    );
    assert_eq!(registry.drain(now), 1);
    assert_eq!(
        registry.lock().slots[&deadline.numeric_id()].phase,
        Phase::Abandoned
    );
    assert_eq!(
        registry.lock().slots[&overflow.numeric_id()].phase,
        Phase::Abandoned
    );
}

#[test]
fn lease_owns_native_state_until_its_single_release() {
    let registry = registry();
    let identity = live(&registry, NativeRegistrationKind::Reserved);
    let lease = registry
        .acquire(identity, NativeLeaseKind::Operation)
        .unwrap();
    let weak = Arc::downgrade(&registry.0);
    retire(&registry, identity, NativeQuarantine::NextDrain);
    drop(registry);
    assert!(weak.upgrade().is_some());
    drop(lease);
    assert!(weak.upgrade().is_none());
}

#[test]
fn shared_numeric_pool_preserves_private_registry_domains() {
    let registry = registry();
    let private_domain = NativeRegistryDomain::new().unwrap();
    let private = registry
        .begin_registration_in_domain(private_domain, NativeRegistrationKind::Reserved)
        .unwrap();
    assert!(registry.publish(private));
    let payload = live(&registry, NativeRegistrationKind::Payload);
    assert_ne!(private.numeric_id(), payload.numeric_id());
    assert_eq!(private.domain(), private_domain);
    assert_eq!(payload.domain(), registry.domain());
}

#[test]
fn id_exhaustion_leaves_live_slots_unchanged() {
    let registry = NativeRegistrationRegistry::new(1, 2, 1);
    let identity = live(&registry, NativeRegistrationKind::Payload);
    assert_eq!(
        registry.begin_registration(NativeRegistrationKind::Payload),
        Err(NativeRegistrationError::IdExhausted)
    );
    assert_eq!(registry.identity(1), Some(identity));
}

#[test]
fn freelist_capacity_counts_preexisting_free_rows() {
    let registry = NativeRegistrationRegistry::new(1, 8, 2);
    let first = live(&registry, NativeRegistrationKind::Payload);
    let second = live(&registry, NativeRegistrationKind::Payload);
    let later = live(&registry, NativeRegistrationKind::Payload);
    retire(&registry, first, NativeQuarantine::NextDrain);
    retire(&registry, second, NativeQuarantine::NextDrain);
    assert_eq!(registry.drain(Instant::now()), 2);
    assert_eq!(registry.lock().free.len(), 2);
    // Both earlier rows remain free: no allocation consumes them. The empty
    // ordinary quarantine admits the third retirement, so only the freelist
    // capacity can prevent its promotion on this second drain.
    retire(&registry, later, NativeQuarantine::NextDrain);
    assert_eq!(registry.lock().ordinary, vec![later.numeric_id()]);
    assert_eq!(
        registry.drain(Instant::now()),
        0,
        "full freelist must reject a later eligible retirement"
    );
    assert_eq!(registry.lock().free.len(), 2);
    assert_eq!(
        registry.lock().slots[&later.numeric_id()].phase,
        Phase::Abandoned
    );
}
