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

    fn bytes(&self, payload: &(dyn Any + Send + Sync)) -> usize {
        if payload.is::<Reclaimed>() {
            RECLAIMED_BYTES
        } else {
            0
        }
    }
}
/// The native bytes one `Reclaimed` payload reports (odd, so a sum of whole
/// payloads cannot match by accident).
const RECLAIMED_BYTES: usize = 4099;
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

// The collector counter is perry-ffi's own test stub here.
#[cfg(not(feature = "runtime-link"))]
#[test]
fn a_parked_record_counts_its_bytes_until_it_leaves() {
    use crate::handle_lifecycle::parked_bytes_for_tests as counted;
    let _serial = recycle_serial();
    let base = counted();
    let (a, _) = reclaimed_payload();
    let (b, _) = reclaimed_payload();
    let (c, _) = reclaimed_payload();
    assert_eq!(counted(), base, "a registered payload is native-owned");
    for id in [a, b, c] {
        assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    }
    assert_eq!(
        counted() - base,
        3 * RECLAIMED_BYTES,
        "parking counts each record"
    );
    // Native code taking one back, or dropping one, removes its bytes.
    assert!(crate::handle_lifecycle::retain_strongly(b));
    assert_eq!(counted() - base, 2 * RECLAIMED_BYTES);
    assert!(drop_handle(c));
    assert_eq!(counted() - base, RECLAIMED_BYTES);
    // A trace that keeps the young record keeps its bytes; the trace that
    // proves it unnamed frees them inside the pause, before the mutator drops
    // the payload.
    drain_quarantined_handles();
    trace_phase_for_tests(0);
    trace_phase_for_tests(1);
    assert!(handle_exists(a));
    assert_eq!(
        counted() - base,
        RECLAIMED_BYTES,
        "a kept record stays counted"
    );
    trace_phase_for_tests(0);
    crate::handle_lifecycle::trace_finish_unreleased_for_tests();
    // `a` and the retired id of `c` wait for the mutator.
    assert_eq!(crate::handle_lifecycle::proven_dead_pending_for_tests(), 2);
    assert_eq!(counted(), base, "the proving full freed the parked bytes");
    crate::handle_lifecycle::release_all_proven();
    assert!(!handle_exists(a));
    assert!(drop_handle(b));
    assert_eq!(counted(), base);
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

/// A minor-scoped walk visits only the parked records the young log names: a
/// record is logged when parked holding a word a minor can act on and on every
/// mutable borrow, and stays logged while it holds such a word.
#[cfg(not(feature = "runtime-link"))]
#[test]
fn a_minor_visits_only_parked_records_that_may_hold_a_young_word() {
    use crate::handle::MINOR_RELEVANT_WORDS_FOR_TESTS as YOUNG;
    use crate::handle_lifecycle::{young_record_walk_for_tests, young_scan_edges_for_tests};
    let _serial = recycle_serial();
    collect_unobserved();
    let (id, _drops) = reclaimed_payload();
    let word = edge_word(id);
    let _ = young_scan_edges_for_tests();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    // Parked holding only an old word: nothing a minor could act on, so the
    // log does not name it.
    assert!(
        !young_scan_edges_for_tests().contains(&word),
        "an old record nobody wrote is skipped by every minor"
    );
    assert_eq!(young_record_walk_for_tests().logged, 0);
    // A mutable borrow may store a young word.
    with_handle_mut::<Reclaimed, _, _>(id, |_| ());
    assert!(
        young_scan_edges_for_tests().contains(&word),
        "a written record is visited"
    );
    assert!(!young_scan_edges_for_tests().contains(&word));
    let _ = get_handle_mut::<Reclaimed>(id);
    assert!(
        young_scan_edges_for_tests().contains(&word),
        "get_handle_mut notes too"
    );
    // A young word keeps the record logged until it is old.
    YOUNG.with(|young| young.borrow_mut().push(word));
    with_handle_mut::<Reclaimed, _, _>(id, |_| ());
    for _ in 0..3 {
        assert!(
            young_scan_edges_for_tests().contains(&word),
            "a young word stays logged"
        );
        assert_eq!(young_record_walk_for_tests().kept, 1);
    }
    YOUNG.with(|young| young.borrow_mut().clear());
    assert!(
        young_scan_edges_for_tests().contains(&word),
        "promoted: one last visit"
    );
    assert!(
        !young_scan_edges_for_tests().contains(&word),
        "then skipped"
    );
    // A full-scope walk never drops a logged record.
    with_handle_mut::<Reclaimed, _, _>(id, |_| ());
    let _ = full_scan_edges_for_tests();
    assert!(young_scan_edges_for_tests().contains(&word));
    collect_unobserved();
    assert!(!handle_exists(id));
}

/// A record parked holding a word a minor can act on is named by the log before
/// it is findable, and the walk keeps it while the word stays young.
#[cfg(not(feature = "runtime-link"))]
#[test]
fn a_record_parked_with_a_young_word_is_logged_at_park() {
    use crate::handle::MINOR_RELEVANT_WORDS_FOR_TESTS as YOUNG;
    use crate::handle_lifecycle::{young_record_walk_for_tests, young_scan_edges_for_tests};
    let _serial = recycle_serial();
    collect_unobserved();
    let _ = young_scan_edges_for_tests();
    let (id, _drops) = reclaimed_payload();
    let word = edge_word(id);
    YOUNG.with(|young| young.borrow_mut().push(word));
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    assert!(
        young_scan_edges_for_tests().contains(&word),
        "a record parked with a young word is logged at park"
    );
    let walk = young_record_walk_for_tests();
    assert_eq!((walk.logged, walk.kept), (1, 1));
    YOUNG.with(|young| young.borrow_mut().clear());
    assert!(young_scan_edges_for_tests().contains(&word));
    assert!(!young_scan_edges_for_tests().contains(&word));
    collect_unobserved();
    assert!(!handle_exists(id));
}

/// The young log names a parked record once between minor-scoped walks, however
/// often native code borrows it: the record's mark says it is named, so a note
/// while marked adds nothing, and a walk visits the record once.
#[cfg(not(feature = "runtime-link"))]
#[test]
fn a_parked_record_borrowed_many_times_is_logged_once() {
    use crate::handle_lifecycle::{young_record_walk_for_tests, young_scan_edges_for_tests};
    let _serial = recycle_serial();
    collect_unobserved();
    let _ = young_scan_edges_for_tests();
    let (id, _drops) = reclaimed_payload();
    let word = edge_word(id);
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    for _ in 0..5 {
        with_handle_mut::<Reclaimed, _, _>(id, |_| ());
        let _ = get_handle_mut::<Reclaimed>(id);
    }
    assert!(young_scan_edges_for_tests().contains(&word));
    let walk = young_record_walk_for_tests();
    assert_eq!(
        (walk.logged, walk.visited),
        (1, 1),
        "fifteen notes, one log entry, one visit"
    );
    // The walk took the mark: the next borrow names the record again.
    with_handle_mut::<Reclaimed, _, _>(id, |_| ());
    assert!(young_scan_edges_for_tests().contains(&word));
    assert_eq!(young_record_walk_for_tests().logged, 1);
    collect_unobserved();
    assert!(!handle_exists(id));
}

/// Rule 2 of the young log: a parked record holding a young word the log does
/// not name (a writer that skipped its note) turns the minor walk red.
#[cfg(all(debug_assertions, not(feature = "runtime-link")))]
#[test]
fn a_young_word_stored_without_a_note_is_caught_by_the_minor_walk() {
    use crate::handle::MINOR_RELEVANT_WORDS_FOR_TESTS as YOUNG;
    use crate::handle_lifecycle::young_scan_edges_for_tests;
    let _serial = recycle_serial();
    collect_unobserved();
    let (id, _drops) = reclaimed_payload();
    let word = edge_word(id);
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    let _ = young_scan_edges_for_tests();
    // The record now holds a young word that no borrow path noted.
    YOUNG.with(|young| young.borrow_mut().push(word));
    let caught = std::panic::catch_unwind(young_scan_edges_for_tests);
    YOUNG.with(|young| young.borrow_mut().clear());
    assert!(caught.is_err(), "an unlogged young record is a red walk");
    collect_unobserved();
}

/// A finished trace's verdict is final, but its payloads are dropped and ids
/// reissued by the mutator after the pause, not inside it.
#[test]
fn a_proven_dead_record_is_released_after_the_pause() {
    use crate::handle_lifecycle::{
        proven_dead_pending_for_tests, release_all_proven, trace_finish_unreleased_for_tests,
    };
    let _serial = recycle_serial();
    collect_unobserved();
    let (id, drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    trace_phase_for_tests(0);
    trace_phase_for_tests(1);
    trace_phase_for_tests(0);
    trace_finish_unreleased_for_tests();
    assert!(!handle_exists(id), "proven dead: no longer parked");
    assert_eq!(
        drops.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "not dropped inside the pause"
    );
    assert!(proven_dead_pending_for_tests() >= 1);
    assert!(!reissuable(id));
    release_all_proven();
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(proven_dead_pending_for_tests(), 0);
    assert!(reissuable(id));
}

/// The young rule by event-loop turn: a trace keeps an id parked in the turn
/// it began in, and decides one parked in an earlier turn (no native frame of
/// that turn still holds it). The runtime-link build reads the runtime's own
/// turn counter, which only the generated main loop advances.
#[cfg(not(feature = "runtime-link"))]
#[test]
fn an_id_parked_in_an_earlier_turn_is_decided_by_the_next_trace() {
    use std::sync::atomic::Ordering::SeqCst;
    let _serial = recycle_serial();
    // Age out what other tests left parked.
    collect_unobserved();
    let (kept, kept_drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(kept, &RECLAIMED_OWNER));
    trace_phase_for_tests(0);
    assert!(
        full_scan_edges_for_tests().contains(&edge_word(kept)),
        "parked in the turn the trace began in: young, a root"
    );
    trace_phase_for_tests(1);
    assert_eq!(kept_drops.load(SeqCst), 0, "a young id is kept");
    let (id, drops) = reclaimed_payload();
    assert!(park_reclaimable_ffi(id, &RECLAIMED_OWNER));
    crate::handle_lifecycle::begin_event_loop_turn_for_tests();
    trace_phase_for_tests(0);
    assert!(
        !full_scan_edges_for_tests().contains(&edge_word(id)),
        "parked in an earlier turn: not a root"
    );
    trace_phase_for_tests(1);
    assert_eq!(
        drops.load(SeqCst),
        1,
        "unnamed and parked in an earlier turn: released by the first trace after it"
    );
    assert!(!handle_exists(id));
    assert_eq!(
        kept_drops.load(SeqCst),
        1,
        "the second trace decides the first id"
    );
}
