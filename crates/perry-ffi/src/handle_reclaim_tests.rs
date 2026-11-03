//! JS-owned ffi payloads parked with a `ParkedRecordOwner` (perry-feather#111).
use super::tests::{boxed, collect_unobserved, recycle_serial, reissuable};
use super::*;
use crate::handle_lifecycle::{
    full_scan_edges_for_tests, parked_record_edges_for_tests, trace_phase_for_tests,
};

/// A JS-owned ffi payload for the reclaimable-owner tests: counts its drops
/// and holds one GC edge (a NaN-boxed word) its owner visits.
struct Reclaimed {
    drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    edge: u64,
}
impl Drop for Reclaimed {
    fn drop(&mut self) {
        self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
static VISIT_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct ReclaimedOwner;
impl crate::handle_lifecycle::ParkedRecordOwner for ReclaimedOwner {
    fn visit(&self, payload: &mut (dyn Any + Send + Sync), visitor: &mut GcRootVisitor<'_>) {
        VISIT_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(reclaimed) = payload.downcast_mut::<Reclaimed>() {
            visitor.visit_nanbox_u64_slot(&mut reclaimed.edge);
        }
    }
}
static RECLAIMED_OWNER: ReclaimedOwner = ReclaimedOwner;

/// A distinct object word per payload, so a scan's words say which payloads
/// it visited.
fn edge_word(id: Handle) -> u64 {
    0x7FFD_0000_0100_0000 | ((id as u64) << 4)
}

fn reclaimed_payload() -> (Handle, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let id = register_handle(Reclaimed {
        drops: drops.clone(),
        edge: 0,
    });
    with_handle_mut::<Reclaimed, _, _>(id, |r| r.edge = edge_word(id));
    (id, drops)
}

fn registry_walk_count() -> usize {
    let mut seen = 0;
    iter_handles_of_mut::<Reclaimed, _>(|_| seen += 1);
    seen
}

#[test]
fn a_parked_record_leaves_the_registry_walk_and_stays_readable() {
    let _serial = recycle_serial();
    let before = registry_walk_count();
    let (a, _) = reclaimed_payload();
    let (b, _) = reclaimed_payload();
    assert_eq!(registry_walk_count(), before + 2);
    assert!(park_reclaimable_ffi(a, &RECLAIMED_OWNER));
    assert!(park_reclaimable_ffi(b, &RECLAIMED_OWNER));
    // The registry walk every type scanner does visits 0 parked entries.
    assert_eq!(registry_walk_count(), before);
    // The payload stays readable on its mutator through every lookup.
    assert!(handle_exists(a));
    assert_eq!(
        with_handle::<Reclaimed, _, _>(a, |r| r.edge),
        Some(edge_word(a))
    );
    assert_eq!(
        get_handle::<Reclaimed>(b).map(|r| r.edge),
        Some(edge_word(b))
    );
    assert!(with_handle_mut::<Reclaimed, _, _>(a, |_| ()).is_some());
    assert!(get_handle_mut::<Reclaimed>(b).is_some());
    assert!(native_handle_props(a).is_some());
    // Off its mutator a parked id answers like a missing one.
    std::thread::spawn(move || {
        assert!(!handle_exists(a));
        assert!(with_handle::<Reclaimed, _, _>(a, |_| ()).is_none());
    })
    .join()
    .unwrap();
    // Native code taking it back puts it in the walk again.
    assert!(crate::handle_lifecycle::retain_strongly(b));
    assert_eq!(registry_walk_count(), before + 1);
    assert!(drop_handle(b));
    collect_unobserved();
    assert!(!handle_exists(a) && !handle_exists(b));
}

#[test]
fn a_parked_ffi_payload_is_dropped_and_its_id_reissued_once_unnamed() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
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
    let before = VISIT_CALLS.load(std::sync::atomic::Ordering::SeqCst);
    for _ in 0..3 {
        trace_phase_for_tests(0);
        // Two words naming the id in one trace: its edges are marked once.
        assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
        assert!(crate::handle_lifecycle::observe_for_tests(
            0x7FFD_0000_0000_0000 | id as u64
        ));
        trace_phase_for_tests(1);
    }
    assert_eq!(
        VISIT_CALLS.load(std::sync::atomic::Ordering::SeqCst) - before,
        3
    );
    // What `observe` marks is the owner's visit, collected as words.
    assert_eq!(parked_record_edges_for_tests(id), vec![edge_word(id)]);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(handle_exists(id));
    collect_unobserved();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// The parked-record scanner in a full mark: young and written records are
/// roots, old ones are not, and a rescan of the same trace (the final remark)
/// revisits only records written since.
#[test]
fn a_full_scan_visits_young_or_written_records_and_the_remark_only_written_ones() {
    let _serial = recycle_serial();
    // Age out what other tests left parked.
    collect_unobserved();
    let (id, _drops) = reclaimed_payload();
    let word = edge_word(id);
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    // Young: the trace it was parked before. Its edges are roots.
    trace_phase_for_tests(0);
    assert!(
        full_scan_edges_for_tests().contains(&word),
        "young record is a root"
    );
    assert!(
        !full_scan_edges_for_tests().contains(&word),
        "the remark skips a young record it visited and nobody wrote"
    );
    with_handle_mut::<Reclaimed, _, _>(id, |_| ());
    assert!(
        full_scan_edges_for_tests().contains(&word),
        "a record written after the first scan is revisited"
    );
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    trace_phase_for_tests(1);
    // Old: reached only through a word naming it.
    trace_phase_for_tests(0);
    assert!(
        !full_scan_edges_for_tests().contains(&word),
        "old parked edges go through the id"
    );
    // A mutable borrow during the trace (a possible edge store after
    // `observe` took the edges) roots it for the rest of this trace.
    let _ = get_handle_mut::<Reclaimed>(id);
    assert!(full_scan_edges_for_tests().contains(&word));
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    trace_phase_for_tests(1);
    trace_phase_for_tests(0);
    assert!(
        !full_scan_edges_for_tests().contains(&word),
        "written roots one trace only"
    );
    trace_phase_for_tests(1);
    collect_unobserved();
    assert!(!handle_exists(id));
}

/// An aborted trace gives its epoch back; a record it visited is visited again
/// by the trace that reuses the epoch.
#[test]
fn a_record_visited_by_an_aborted_trace_is_visited_by_the_next() {
    let _serial = recycle_serial();
    let (id, _drops) = reclaimed_payload();
    let word = edge_word(id);
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    trace_phase_for_tests(0);
    assert!(full_scan_edges_for_tests().contains(&word));
    trace_phase_for_tests(2);
    trace_phase_for_tests(0);
    assert!(full_scan_edges_for_tests().contains(&word));
    trace_phase_for_tests(1);
    collect_unobserved();
    assert!(!handle_exists(id));
}

// Setting a bag registers the ffi bag scanner, which needs the runtime.
#[cfg(feature = "runtime-link")]
#[test]
fn a_parked_ffi_payload_bag_is_reported_through_its_record() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    let bag = 0x7FFD_0000_0001_0000u64;
    assert!(set_native_handle_props(id, bag));
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    assert_eq!(native_handle_props(id), Some(bag));
    assert_eq!(parked_record_edges_for_tests(id), vec![bag, edge_word(id)]);
    assert!(set_native_handle_props(id, 0));
    assert_eq!(parked_record_edges_for_tests(id), vec![edge_word(id)]);
    collect_unobserved();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn dropping_a_parked_record_retires_its_id() {
    let _serial = recycle_serial();
    let (id, drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    assert!(drop_handle(id));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(!handle_exists(id));
    assert!(!reissuable(id), "a retired id waits for the proof");
    collect_unobserved();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(reissuable(id));
}
