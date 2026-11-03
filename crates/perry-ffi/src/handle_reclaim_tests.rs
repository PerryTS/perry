//! JS-owned ffi payloads parked with a `ParkedPayloadOwner` (perry-feather#111).
use super::tests::{boxed, collect_unobserved, recycle_serial, reissuable};
use super::*;

/// A JS-owned ffi payload for the reclaimable-owner tests: counts its
/// drops and the `edges` calls a trace makes through its owner.
struct Reclaimed(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl Drop for Reclaimed {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
static EDGE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct ReclaimedOwner;
impl crate::handle_lifecycle::ParkedPayloadOwner for ReclaimedOwner {
    fn release(&self, identity: crate::NativeRegistrationIdentity) {
        drop(release_reclaimed_payload(identity));
    }
    fn edges(&self, id: Handle, out: &mut Vec<u64>) {
        EDGE_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        out.extend(native_handle_props(id));
    }
}
static RECLAIMED_OWNER: ReclaimedOwner = ReclaimedOwner;

fn reclaimed_payload() -> (Handle, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let id = register_handle(Reclaimed(drops.clone()));
    (id, drops)
}

#[test]
fn a_parked_ffi_payload_is_dropped_and_its_id_reissued_once_unnamed() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    // Parked, not removed: getters keep answering until the proof.
    assert!(with_handle::<Reclaimed, _, _>(id, |_| ()).is_some());
    assert!(!reissuable(id));
    collect_unobserved();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(!handle_exists(id));
    assert!(reissuable(id));
}

#[test]
fn a_named_parked_ffi_payload_keeps_its_payload_and_reports_edges_once_per_trace() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    let before = EDGE_CALLS.load(std::sync::atomic::Ordering::SeqCst);
    for _ in 0..3 {
        crate::handle_lifecycle::trace_phase_for_tests(0);
        // Two words naming the id in one trace: its edges are marked once.
        assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
        assert!(crate::handle_lifecycle::observe_for_tests(
            0x7FFD_0000_0000_0000 | id as u64
        ));
        crate::handle_lifecycle::trace_phase_for_tests(1);
    }
    assert_eq!(
        EDGE_CALLS.load(std::sync::atomic::Ordering::SeqCst) - before,
        3
    );
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(handle_exists(id));
    collect_unobserved();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn parked_edges_are_rooted_while_young_or_written_in_this_trace_only() {
    use crate::handle_lifecycle::{payload_edges_are_rooted, trace_phase_for_tests};
    let _serial = recycle_serial();
    let (id, _drops) = reclaimed_payload();
    // Native-owned: always rooted.
    assert!(payload_edges_are_rooted(id));
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    // Young: the trace it was parked before, and the one after.
    trace_phase_for_tests(0);
    assert!(payload_edges_are_rooted(id));
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    trace_phase_for_tests(1);
    trace_phase_for_tests(0);
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    assert!(
        !payload_edges_are_rooted(id),
        "old parked edges go through the id"
    );
    // A mutable borrow during the trace (a possible edge store after
    // `observe` took the edges) roots the payload for the final remark.
    with_handle_mut::<Reclaimed, _, _>(id, |_| ());
    assert!(payload_edges_are_rooted(id));
    trace_phase_for_tests(1);
    trace_phase_for_tests(0);
    assert!(
        !payload_edges_are_rooted(id),
        "written roots one trace only"
    );
    // Reached in this trace, so the trace that ends next keeps it.
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    // Outside a trace a borrow records nothing.
    trace_phase_for_tests(1);
    let _ = get_handle_mut::<Reclaimed>(id);
    trace_phase_for_tests(0);
    assert!(!payload_edges_are_rooted(id));
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    trace_phase_for_tests(1);
    collect_unobserved();
    assert!(!handle_exists(id));
}

// Setting a bag registers the ffi bag scanner, which needs the runtime.
#[cfg(feature = "runtime-link")]
#[test]
fn a_parked_ffi_payload_bag_is_reported_through_its_owner() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    let bag = 0x7FFD_0000_0001_0000u64;
    assert!(set_native_handle_props(id, bag));
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    let mut edges = Vec::new();
    crate::handle_lifecycle::ParkedPayloadOwner::edges(&RECLAIMED_OWNER, id, &mut edges);
    assert_eq!(edges, vec![bag]);
    assert!(set_native_handle_props(id, 0));
    collect_unobserved();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn release_reclaimed_payload_defers_to_a_concurrent_drop() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    let identity = handle_registration(id).unwrap();
    assert!(drop_handle(id));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(release_reclaimed_payload(identity).is_none());
    collect_unobserved();
    assert!(reissuable(id));
}
