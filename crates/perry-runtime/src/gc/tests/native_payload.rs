//! #11919 P0: an ordinary object that owns a native payload.
//!
//! The payload is a `GC_TYPE_NATIVE_HANDLE` cell named by the object's
//! `ObjectMeta.native_state` word. These pin the contract the pattern rests on:
//! the word is a traced edge (the cell lives while the object does, through a
//! moving minor), the drop runs exactly once whether `close` or the collector
//! gets there first, and the payload's native bytes feed GC pacing until then.

use super::super::*;
use super::support::*;
use crate::native_payload::{self, NativePayloadFamily, PayloadMiss};
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Probe {
    value: u64,
}

impl Drop for Probe {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

fn install(_proto: &mut native_payload::PayloadPrototype) {}

static FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::CRYPTO_HASH,
    name: "Probe",
    constructor_export: None,
    constructor_length: 0,
    links_owner: false,
    install_prototype: install,
};

fn register_scanners() {
    gc_register_mutable_root_scanner(native_payload::scan_payload_prototype_roots_mut);
}

struct PrototypeReset;
impl Drop for PrototypeReset {
    fn drop(&mut self) {
        native_payload::reset_payload_prototypes_for_tests();
    }
}

fn full_collection() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

/// `close` drops the payload once, releases its bytes, and the collector that
/// later finds the dead object does not drop it again.
#[test]
fn native_payload_close_then_collection_drops_exactly_once() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset;
    register_scanners();
    DROPS.store(0, Ordering::SeqCst);
    let live_before = policy::external_side_live_bytes();
    let value = native_payload::alloc(&FAMILY, Probe { value: 7 }, 4096, &[]);
    assert_eq!(policy::external_side_live_bytes(), live_before + 4096);
    assert_eq!(
        unsafe { native_payload::payload_mut::<Probe>(value, &FAMILY) }
            .map(|p| p.value)
            .ok(),
        Some(7)
    );
    assert_eq!(
        native_payload::close(value, &FAMILY),
        native_payload::CloseOutcome::Closed
    );
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(policy::external_side_live_bytes(), live_before);
    assert_eq!(
        native_payload::close(value, &FAMILY),
        native_payload::CloseOutcome::AlreadyClosed
    );
    assert_eq!(
        unsafe { native_payload::payload_mut::<Probe>(value, &FAMILY) }.err(),
        Some(PayloadMiss::Closed)
    );
    assert!(native_payload::is_instance(value, &FAMILY));
    let _ = value;
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    full_collection();
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        1,
        "the sweep must not drop it again"
    );
}

/// With no close at all, the collection that finds the object dead drops the
/// payload exactly once and gives its bytes back.
#[test]
fn native_payload_dead_object_is_dropped_once_by_the_collector() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset;
    register_scanners();
    DROPS.store(0, Ordering::SeqCst);
    let live_before = policy::external_side_live_bytes();
    for i in 0..64 {
        let _ = native_payload::alloc(&FAMILY, Probe { value: i }, 1000, &[]);
    }
    assert_eq!(policy::external_side_live_bytes(), live_before + 64_000);
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        64,
        "every dead payload is dropped"
    );
    assert_eq!(policy::external_side_live_bytes(), live_before);
    full_collection();
    assert_eq!(DROPS.load(Ordering::SeqCst), 64, "and none twice");
}

/// A rooted object keeps its payload through a full collection.
#[test]
fn native_payload_live_object_keeps_its_payload_through_a_full_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = PrototypeReset;
    register_scanners();
    DROPS.store(0, Ordering::SeqCst);
    let value = native_payload::alloc(&FAMILY, Probe { value: 42 }, 0, &[]);
    js_shadow_slot_set(0, value.to_bits());
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    let value = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        0,
        "a reachable payload must not be dropped"
    );
    assert_eq!(
        unsafe { native_payload::payload_mut::<Probe>(value, &FAMILY) }
            .map(|p| p.value)
            .ok(),
        Some(42)
    );
}

/// The object is movable and its payload cell is not: a copying minor moves
/// the object, the meta record moves with it, and the traced `native_state`
/// word still names the same live cell. Removing the `native_state` visit in
/// `gc/layout_slot_visit.rs` leaves the cell unmarked, so the minor's malloc
/// sweep drops it and the assertions below go red.
#[test]
fn native_payload_cell_survives_a_moving_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = PrototypeReset;
    register_scanners();
    DROPS.store(0, Ordering::SeqCst);
    let value = native_payload::alloc(&FAMILY, Probe { value: 9 }, 0, &[]);
    js_shadow_slot_set(0, value.to_bits());
    let before = value.to_bits() & POINTER_MASK;
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(
        trace.copying_nursery.eligible,
        "test premise: a COPYING minor must have run"
    );
    let moved = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(
        moved.to_bits() & POINTER_MASK,
        before,
        "test premise: the object moved"
    );
    let _no_conservative = ConservativeScanDisabledGuard::new();
    // Enough malloc-sweeping minors for the owner to tenure: an OLD meta
    // record naming a malloc cell is an old->malloc edge the minors must
    // still honour once the owner stops being traced as young.
    for _ in 0..6 {
        let _ = collect_minor_trace(GcTriggerKind::MallocCount);
    }
    let moved = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        0,
        "the cell must survive with its owner"
    );
    assert_eq!(
        unsafe { native_payload::payload_mut::<Probe>(moved, &FAMILY) }
            .map(|p| p.value)
            .ok(),
        Some(9)
    );
}

/// Distinct allocations are distinct objects, and a foreign object is not an
/// instance.
#[test]
fn native_payload_instances_are_distinct_objects() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset;
    register_scanners();
    let a = native_payload::alloc(&FAMILY, Probe { value: 1 }, 0, &[]);
    let b = native_payload::alloc(&FAMILY, Probe { value: 2 }, 0, &[]);
    assert_ne!(a.to_bits(), b.to_bits());
    let plain = crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
    assert!(!native_payload::is_instance(plain, &FAMILY));
    assert_eq!(
        unsafe { native_payload::payload_mut::<Probe>(plain, &FAMILY) }.err(),
        Some(PayloadMiss::Foreign)
    );
    native_payload::close(a, &FAMILY);
    native_payload::close(b, &FAMILY);
}
