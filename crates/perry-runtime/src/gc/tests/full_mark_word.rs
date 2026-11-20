//! Independent coverage oracles for a decoded word shared by full marking.
//! Marking and remembering have deliberately different acceptance domains.

use super::super::*;
use super::support::*;
use crate::gc::mark::full_mark_decode_sabotage;
use crate::value::{TAG_NULL, TAG_UNDEFINED};
use std::cell::Cell;
use std::ffi::c_void;

fn isolated(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(2);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        assert!(
            !crate::gc::full_trace::handle_trace_active(),
            "observer-free fast-path premise at fixture entry",
        );
        f();
        clear_marks();
        clear_mark_seeds();
    })
    .join()
    .expect("full-mark word fixture must not panic");
}

fn valid_set(classifier: bool) -> ValidPointerSet {
    if classifier {
        let mut valid = ValidPointerSet::new();
        valid.classifier_mode = true;
        valid
    } else {
        build_valid_pointer_set()
    }
}

#[cfg(perry_gc_instruments)]
#[test]
fn active_classifier_differential_accepts_fold_and_rejects_census_disagreement() {
    // The verifier caches its environment switch process-wide. Use a fresh
    // process when the ordinary suite runs with that switch off.
    if !env_flag_enabled("PERRY_GC_VERIFY_CLASSIFIER") {
        let output = crate::test_support::this_test_process()
            .env("PERRY_GC_VERIFY_CLASSIFIER", "1")
            .output()
            .expect("launch classifier differential control");
        assert!(
            crate::test_support::passed(output.status),
            "classifier control failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(CLASSIFIER_CONTROL_DONE),
            "the fresh process must execute the control",
        );
        return;
    }

    isolated(|| unsafe {
        // Fixture reset wipes metadata and suppresses verification. These
        // objects are allocated afterwards and have real, fresh metadata.
        let child = young_leaf();
        let (parent, _) = old_parent(string_bits(child));
        let child_header = header_from_user_ptr(child as *const u8);
        (*child_header).gc_flags |= GC_FLAG_MARKED;
        assert!(mark::classifier_valid_object_start(child));
        assert!(mark::classifier_valid_object_start(
            parent as usize + GC_HEADER_SIZE
        ));

        struct RestoreSuppression(bool);
        impl Drop for RestoreSuppression {
            fn drop(&mut self) {
                CLASSIFIER_VERIFY_SUPPRESSED.with(|flag| flag.set(self.0));
            }
        }
        let _restore =
            RestoreSuppression(CLASSIFIER_VERIFY_SUPPRESSED.with(|flag| flag.replace(false)));
        assert!(mark::classifier_verify_enabled());
        let valid = build_valid_pointer_set();
        assert!(valid.built_by_census);
        assert!(valid.contains(&child));
        assert_coverage(&fold(parent, &valid), false);

        // Keep exact census membership, but make its header unacceptable to
        // both classifier header gates. This independent negative must reach
        // the unchanged differential assertion, rather than a test oracle.
        assert_eq!((*child_header).gc_flags & GC_FLAG_FORWARDED, 0);
        let size = (*child_header).size;
        (*child_header).size = 0;
        let classifier_rejected = !mark::classifier_valid_object_start(child);
        // The armed census owns a drop-restoration guard and therefore is
        // not RefUnwindSafe. This query only reads it; the deliberately
        // modified header is restored immediately after the caught panic.
        let red = crate::test_support::child_unwind(std::panic::AssertUnwindSafe(|| {
            valid.contains(&child)
        }));
        (*child_header).size = size;
        assert!(classifier_rejected);
        let error = red.expect_err("active differential gate must reject census disagreement");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains("classifier rejected censused object"));
        assert!(mark::classifier_verify_enabled());
        assert!(valid.contains(&child));
    });
    println!("{CLASSIFIER_CONTROL_DONE}");
}

#[cfg(perry_gc_instruments)]
const CLASSIFIER_CONTROL_DONE: &str = "classifier differential control completed";

unsafe fn old_parent(bits: u64) -> (*mut GcHeader, GcMutableSlot) {
    let (parent, fields) = alloc_old_test_object(1);
    *fields = bits;
    layout_note_slot(parent as usize, 0, bits);
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    let header = header_from_user_ptr(parent.cast());
    (*header).gc_flags |= GC_FLAG_MARKED;
    assert!(barrier_parent_needs_remembering(parent as usize));
    (header, GcMutableSlot::new(fields, None))
}

unsafe fn malloc_parent(bits: u64) -> (*mut GcHeader, GcMutableSlot) {
    let shape_id = crate::object::shapes::shape_descriptor_ensure(std::ptr::null(), 0, 1)
        .expect("fixture shape id");
    let payload =
        std::mem::size_of::<crate::object::ObjectHeader>() + crate::object::INLINE_SLOT_FLOOR * 8;
    let parent = gc_malloc(payload, GC_TYPE_OBJECT).cast::<crate::object::ObjectHeader>();
    std::ptr::write_bytes(parent.cast::<u8>(), 0, payload);
    crate::object::shapes::store_kind::premark_plain_ordinary(parent);
    (*parent).parent_class_id = shape_id.0;
    let fields = parent
        .cast::<u8>()
        .add(std::mem::size_of::<crate::object::ObjectHeader>())
        .cast::<u64>();
    *fields = bits;
    layout_note_slot(parent as usize, 0, bits);
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    let header = header_from_user_ptr(parent.cast());
    (*header).gc_flags |= GC_FLAG_MARKED;
    assert!(matches!(
        crate::gc::barrier::barrier_remembering_parent(parent as usize),
        Some(crate::gc::barrier::RememberingParent::Malloc)
    ));
    (header, GcMutableSlot::new(fields, None))
}

fn malloc_coverage(drop_entry: bool) {
    for classifier in [false, true] {
        isolated(move || unsafe {
            let child = young_leaf();
            let (parent, _) = malloc_parent(string_bits(child));
            let valid = valid_set(classifier);
            let _drop = drop_entry.then(mark::remembered_mark_sabotage::Guard::arm);
            let sticky = fold(parent, &valid);
            assert_ne!(
                (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
                0
            );
            assert!(
                sticky.old_pages.is_empty(),
                "malloc custody must be owner-qualified"
            );
            assert_coverage(&sticky, drop_entry);
        });
    }
}

#[test]
fn malloc_full_mark_records_owner_qualified_post_mark_edges() {
    malloc_coverage(false);
}

#[test]
fn dropping_malloc_full_mark_coverage_is_detected_independently_of_liveness() {
    malloc_coverage(true);
}

#[test]
fn malloc_ephemeron_words_are_not_unconditional_remembered_edges() {
    isolated(|| unsafe {
        let child = young_leaf();
        let payload = std::mem::size_of::<crate::weakref::storage::WeakStorage>()
            + std::mem::size_of::<crate::weakref::storage::Entry>()
            + 3 * 4;
        let storage =
            gc_malloc(payload, GC_TYPE_WEAK_STORAGE).cast::<crate::weakref::storage::WeakStorage>();
        std::ptr::write_bytes(storage.cast::<u8>(), 0, payload);
        (*storage).capacity = 1;
        (*storage).len = 1;
        (*(*storage).entries()).key = string_bits(child);
        (*(*storage).entries()).value = string_bits(child);
        let header = header_from_user_ptr(storage.cast());
        (*header).gc_flags |= GC_FLAG_MARKED;
        let sticky = fold(header, &valid_set(false));
        assert_eq!(
            (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert!(sticky.old_pages.is_empty() && sticky.external_pages.is_empty());
    });
}

unsafe fn fold(header: *mut GcHeader, valid: &ValidPointerSet) -> StickyRememberedSet {
    let mut sticky = StickyRememberedSet::default();
    let mut worklist = Vec::new();
    mark::trace_heap_rewrite_slots_remembering::<{ cfg!(test) }>(
        header,
        valid,
        &mut worklist,
        Some(&mut sticky),
    );
    sticky
}

fn assert_coverage(sticky: &StickyRememberedSet, expect_missing: bool) {
    sticky.restore();
    let stats = verify_old_to_young_edges_collect();
    assert!(
        stats.checked_old_to_young_edges > 0,
        "live coverage premise: {stats:?}"
    );
    if expect_missing {
        let red =
            crate::test_support::child_unwind(|| assert_eq!(stats.missing_edges, 0, "{stats:?}"));
        assert!(
            red.is_err(),
            "the independent coverage oracle must turn red"
        );
    } else {
        assert_eq!(stats.missing_edges, 0, "{stats:?}");
    }
}

#[test]
fn remembered_inline_ranges_still_mark_children_and_cover_later_pages() {
    for classifier in [false, true] {
        for capacity in [8, 1200] {
            for precovered in [false, true] {
                isolated(move || unsafe {
                    let array = crate::array::js_array_alloc_canonical_keys(capacity, true, false);
                    let first = young_leaf();
                    let last = young_leaf();
                    crate::array::note_array_slot(array, 0, string_bits(first));
                    crate::array::note_array_slot(array, capacity as usize - 1, string_bits(last));
                    (*array).length = capacity;
                    let header = header_from_user_ptr(array.cast());
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    let slots = crate::array::array_elements_ptr(array) as *mut u64;
                    let first_page = crate::arena::generation_page_for_addr(slots as usize);
                    let last_page = crate::arena::generation_page_for_addr(
                        slots.add(capacity as usize - 1) as usize,
                    );
                    assert_eq!(first_page == last_page, capacity == 8);
                    let mut sticky = StickyRememberedSet::default();
                    if precovered {
                        sticky.old_pages.insert(first_page);
                    }
                    let valid = valid_set(classifier);
                    let mut worklist = Vec::new();
                    mark::trace_heap_rewrite_slots_remembering::<{ cfg!(test) }>(
                        header,
                        &valid,
                        &mut worklist,
                        Some(&mut sticky),
                    );
                    // Existing coverage must never bypass child marking. A range
                    // spanning pages must also remember the independently stored
                    // child on its last page.
                    for child in [first, last] {
                        assert_ne!(
                            (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
                            0
                        );
                    }
                    assert!(sticky.old_pages.contains(&last_page));
                    assert_coverage(&sticky, false);
                });
            }
        }
    }
}

#[test]
fn already_marked_children_are_remembered_in_census_and_classifier_modes() {
    for classifier in [false, true] {
        for malloc in [false, true] {
            for tag in [POINTER_TAG, STRING_TAG, BIGINT_TAG] {
                isolated(move || unsafe {
                    let child = if malloc {
                        alloc_tracked_test_symbol() as usize
                    } else {
                        young_leaf()
                    };
                    let (parent, _) = old_parent(tag | child as u64);
                    let child_header = header_from_user_ptr(child as *const u8);
                    (*child_header).gc_flags |= GC_FLAG_MARKED;
                    let valid = valid_set(classifier);
                    assert!(valid.contains(&child));
                    assert!(!mark::mark_field_into_worklist(
                        tag | child as u64,
                        &valid,
                        &mut Vec::new(),
                        false
                    ));
                    assert_coverage(&fold(parent, &valid), false);
                });
            }
        }
    }
}

#[test]
fn conditioning_remembering_on_new_marking_loses_already_marked_coverage() {
    isolated(|| unsafe {
        let child = young_leaf();
        let (parent, _) = old_parent(string_bits(child));
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_MARKED;
        let valid = valid_set(false);
        assert!(valid.contains(&child));
        let _sabotage = full_mark_decode_sabotage::Guard::new(true, false);
        assert_coverage(&fold(parent, &valid), true);
    });
}

#[test]
fn a_pinned_child_keeps_coverage_with_and_without_an_existing_mark() {
    for marked in [false, true] {
        isolated(move || unsafe {
            let child = young_leaf();
            let header = header_from_user_ptr(child as *const u8);
            pin_object(header);
            if marked {
                (*header).gc_flags |= GC_FLAG_MARKED;
            }
            let (parent, _) = old_parent(string_bits(child));
            let valid = valid_set(false);
            assert_ne!((*header).gc_flags & GC_FLAG_PINNED, 0);
            assert!(
                !crate::gc::pin::pinned_counts_as_marked((*header).gc_flags),
                "a pin alone is not a mark"
            );
            assert_coverage(&fold(parent, &valid), false);
            unpin_object(header);
        });
    }
}

fn rejected_raw(classifier: bool, sabotage: bool) {
    isolated(move || unsafe {
        let (young, fields) = alloc_nursery_test_object(2);
        *fields = 0;
        *fields.add(1) = 0;
        let raw = fields.add(1) as u64;
        js_shadow_slot_set(1, ptr_bits(young as usize));
        let (parent, slot) = old_parent(raw);
        let valid = valid_set(classifier);
        assert!(
            !valid.contains(&(raw as usize)),
            "the raw word is not an object start"
        );
        assert_eq!(decode_heap_addr(raw), raw as usize);
        assert!(barrier::remembered_child_needs_tracking(raw as usize));
        let _sabotage = full_mark_decode_sabotage::Guard::new(sabotage, false);
        assert_coverage(&fold(parent, &valid), sabotage);
        assert_eq!(
            slot.read(),
            raw,
            "full marking never rewrites the rejected word"
        );
    });
}

#[test]
fn rejected_raw_nursery_words_retain_coverage_in_both_membership_modes() {
    rejected_raw(false, false);
    rejected_raw(true, false);
}

#[test]
fn conditioning_remembering_on_new_marking_loses_rejected_raw_coverage() {
    rejected_raw(false, true);
    rejected_raw(true, true);
}

#[test]
fn a_non_arena_malloc_word_marks_raw_but_is_remembered_only_when_tagged() {
    for raw in [false, true] {
        isolated(move || unsafe {
            let child = alloc_tracked_test_symbol() as usize;
            assert!(matches!(
                crate::arena::classify_heap_generation(child),
                crate::arena::HeapGeneration::Unknown
            ));
            let bits = if raw { child as u64 } else { ptr_bits(child) };
            let (parent, _) = old_parent(bits);
            let valid = valid_set(false);
            assert!(valid.contains(&child));
            assert_eq!(decode_heap_addr(bits) != 0, !raw);
            let sticky = fold(parent, &valid);
            assert_ne!(
                (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
                0
            );
            assert_eq!(!sticky.old_pages.is_empty(), !raw);
            assert!(sticky.external_pages.is_empty());
        });
    }
}

#[test]
fn a_folded_already_marked_edge_keeps_its_payload_across_two_copying_minors() {
    isolated(|| unsafe {
        let _tenuring = crate::gc::tenuring::set_survivals_for_test(
            crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX,
        );
        let text = b"full-mark-to-next-minor";
        let child = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32) as usize;
        assert!(crate::arena::pointer_in_nursery(child));
        let (parent, slot) = old_parent(string_bits(child));
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_MARKED;
        let sticky = fold(parent, &valid_set(false));
        assert_coverage(&sticky, false);
        clear_marks();
        for _ in 0..2 {
            let before = slot.read();
            let trace = collect_minor_trace(GcTriggerKind::Direct);
            assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
            assert_ne!(
                slot.read(),
                before,
                "the sole child edge must actually move"
            );
            assert_string_bytes(
                (slot.read() & POINTER_MASK) as *const crate::StringHeader,
                text,
            );
        }
    });
}

#[test]
fn scalar_null_old_and_longlived_words_do_not_fabricate_tracking() {
    isolated(|| unsafe {
        let old = alloc_old_test_object(0).0 as usize;
        let longlived = crate::arena::arena_alloc_gc_longlived(40, 8, GC_TYPE_OBJECT) as usize;
        for bits in [
            0,
            TAG_NULL,
            TAG_UNDEFINED,
            100.5f64.to_bits(),
            ptr_bits(old),
            old as u64,
            ptr_bits(longlived),
            longlived as u64,
            0x10001,
        ] {
            let (parent, _) = old_parent(bits);
            let sticky = fold(parent, &valid_set(false));
            assert!(sticky.old_pages.is_empty(), "word {bits:#x}");
            assert!(sticky.external_pages.is_empty(), "word {bits:#x}");
        }
    });
}

thread_local! {
    static OBSERVER_SLOT: Cell<*mut u64> = const { Cell::new(std::ptr::null_mut()) };
    static OBSERVER_CHILD: Cell<u64> = const { Cell::new(0) };
    static OBSERVER_HITS: Cell<usize> = const { Cell::new(0) };
}
extern "C" fn observer_phase(_: u32) -> bool {
    true
}
extern "C" fn inert_phase(_: u32) -> bool {
    false
}
extern "C" fn observer(bits: u64, mark: extern "C" fn(u64, *mut c_void), ctx: *mut c_void) -> bool {
    assert_eq!(bits, ptr_bits(7));
    OBSERVER_HITS.with(|hits| hits.set(hits.get() + 1));
    let child = OBSERVER_CHILD.with(Cell::get);
    mark(child, ctx);
    OBSERVER_SLOT.with(|slot| unsafe {
        *slot.get() = child;
    });
    true
}
extern "C" fn non_mutating_observer(
    bits: u64,
    mark: extern "C" fn(u64, *mut c_void),
    ctx: *mut c_void,
) -> bool {
    assert_eq!(bits, ptr_bits(7));
    OBSERVER_HITS.with(|hits| hits.set(hits.get() + 1));
    mark(OBSERVER_CHILD.with(Cell::get), ctx);
    true
}
struct ObserverScope;
impl Drop for ObserverScope {
    fn drop(&mut self) {
        crate::gc::full_trace::abort_full_trace();
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(inert_phase, observer);
        OBSERVER_SLOT.with(|slot| slot.set(std::ptr::null_mut()));
        OBSERVER_CHILD.with(|child| child.set(0));
    }
}
fn changing_observer_parent(sabotage: bool, malloc: bool) {
    isolated(move || unsafe {
        let child = young_leaf();
        let (parent, slot) = if malloc {
            malloc_parent(ptr_bits(7))
        } else {
            old_parent(ptr_bits(7))
        };
        OBSERVER_SLOT.with(|value| value.set(slot.slot));
        OBSERVER_CHILD.with(|value| value.set(string_bits(child)));
        OBSERVER_HITS.with(|value| value.set(0));
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(observer_phase, observer);
        let _scope = ObserverScope;
        crate::gc::full_trace::begin_full_trace();
        assert!(crate::gc::full_trace::handle_trace_active());
        let valid = valid_set(false);
        let _sabotage = sabotage.then(full_mark_decode_sabotage::ObserverGuard::arm);
        let sticky = fold(parent, &valid);
        assert_eq!(
            OBSERVER_HITS.with(Cell::get),
            1,
            "observer subject must execute in both arms"
        );
        assert_eq!(slot.read(), string_bits(child));
        assert_ne!(
            (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_coverage(&sticky, sabotage);
    });
}

fn changing_observer(sabotage: bool) {
    changing_observer_parent(sabotage, false);
}

#[test]
fn malloc_full_mark_records_foreign_observer_rewrites() {
    changing_observer_parent(false, true);
}

#[test]
fn foreign_observer_changes_are_remembered_from_the_post_mark_word() {
    changing_observer(false);
}

#[test]
fn an_active_foreign_observer_cannot_observe_string_words() {
    for classifier in [false, true] {
        isolated(move || unsafe {
            let array = crate::array::js_array_alloc_canonical_keys(8, true, false);
            let first = young_leaf();
            let last = young_leaf();
            crate::array::note_array_slot(array, 0, string_bits(first));
            crate::array::note_array_slot(array, 7, string_bits(last));
            (*array).length = 8;
            let header = header_from_user_ptr(array.cast());
            (*header).gc_flags |= GC_FLAG_MARKED;
            OBSERVER_HITS.with(|value| value.set(0));
            crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(
                observer_phase,
                observer,
            );
            let _scope = ObserverScope;
            crate::gc::full_trace::begin_full_trace();
            assert!(crate::gc::full_trace::handle_trace_active());
            let valid = valid_set(classifier);
            assert!(!crate::gc::full_trace::observe_handle(
                string_bits(first),
                &valid
            ));
            let sticky = fold(header, &valid);
            assert_eq!(OBSERVER_HITS.with(Cell::get), 0);
            for child in [first, last] {
                assert_ne!(
                    (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
                    0
                );
            }
            assert_coverage(&sticky, false);
        });
    }
}

#[test]
fn a_stale_snapshot_across_a_foreign_observer_loses_coverage() {
    changing_observer(true);
}

#[test]
fn string_decode_keeps_the_child_descriptor_observer_gate() {
    for negative in [false, true] {
        isolated(move || unsafe {
            // Word tags are not an object-class proof. Even a string-tagged edge
            // must queue a pointer-free payload when a handle observer is active.
            let (array, _) = alloc_old_test_array(1);
            layout_init_pointer_free(array.cast());
            let child_header = header_from_user_ptr(array.cast());
            assert!(gc_object_yields_no_child_slots(child_header));
            let (parent, _) = old_parent(string_bits(array as usize));
            crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(
                observer_phase,
                observer,
            );
            let _scope = ObserverScope;
            crate::gc::full_trace::begin_full_trace();
            let _control = negative.then(mark::zero_slot_skip_sabotage::Guard::arm);
            let mut sticky = StickyRememberedSet::default();
            let mut worklist = Vec::new();
            mark::trace_heap_rewrite_slots_remembering::<{ cfg!(test) }>(
                parent,
                &valid_set(false),
                &mut worklist,
                Some(&mut sticky),
            );
            // The sabotaged control must drop the child (a panic, observed
            // from a child process); the shipped walk must keep it.
            if negative {
                let verdict = crate::test_support::child_unwind(|| {
                    assert!(worklist.contains(&child_header));
                });
                assert!(verdict.is_err());
            } else {
                assert!(worklist.contains(&child_header));
            }
        });
    }
}

#[test]
fn shared_scalar_storage_skips_handle_words_but_ordinary_arrays_keep_observers() {
    for shared in [false, true] {
        isolated(move || unsafe {
            let (array, slots) = if shared {
                let array = crate::array::js_array_alloc_canonical_keys(8, false, false);
                (array, crate::array::array_elements_ptr(array))
            } else {
                let (array, slots) = alloc_old_test_array(8);
                layout_init_pointer_free(array.cast());
                (array, slots)
            };
            *slots = if shared {
                crate::value::INTERNAL_WORD_TAG | 1
            } else {
                ptr_bits(7)
            };
            (*array).length = 1;
            let child = young_leaf();
            OBSERVER_CHILD.with(|value| value.set(string_bits(child)));
            OBSERVER_HITS.with(|value| value.set(0));
            crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(
                observer_phase,
                non_mutating_observer,
            );
            let _scope = ObserverScope;
            crate::gc::full_trace::begin_full_trace();
            let (parent, _) = old_parent(ptr_bits(array as usize));
            let valid = valid_set(false);
            let mut sticky = StickyRememberedSet::default();
            let mut worklist = Vec::new();
            mark::trace_heap_rewrite_slots_remembering::<{ cfg!(test) }>(
                parent,
                &valid,
                &mut worklist,
                Some(&mut sticky),
            );
            let header = header_from_user_ptr(array.cast());
            assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
            assert_eq!(worklist.contains(&header), !shared);
            if !shared {
                mark::trace_heap_rewrite_slots_remembering::<{ cfg!(test) }>(
                    header,
                    &valid,
                    &mut Vec::new(),
                    Some(&mut sticky),
                );
                assert_ne!(
                    (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
                    0
                );
            }
            assert_eq!(OBSERVER_HITS.with(Cell::get), usize::from(!shared));
        });
    }
}

#[test]
fn a_non_array_shared_bit_does_not_suppress_its_foreign_observer() {
    isolated(|| unsafe {
        let child = young_leaf();
        let (parent, slot) = old_parent(ptr_bits(7));
        (*parent).gc_flags |= GC_FLAG_SHAPE_SHARED;
        assert!(crate::gc::full_trace::object_words_may_name_handles(parent));
        OBSERVER_SLOT.with(|value| value.set(slot.slot));
        OBSERVER_CHILD.with(|value| value.set(string_bits(child)));
        OBSERVER_HITS.with(|value| value.set(0));
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(observer_phase, observer);
        let _scope = ObserverScope;
        crate::gc::full_trace::begin_full_trace();
        let sticky = fold(parent, &valid_set(false));
        assert_eq!(OBSERVER_HITS.with(Cell::get), 1);
        assert_coverage(&sticky, false);
    });
}

#[test]
fn a_foreign_handle_cannot_be_declared_shared_key_metadata() {
    isolated(|| unsafe {
        let array = crate::array::js_array_alloc_canonical_keys(1, false, false);
        *crate::array::array_elements_ptr(array) = ptr_bits(7);
        (*array).length = 1;
        let header = header_from_user_ptr(array.cast());
        let red = crate::test_support::child_unwind(std::panic::AssertUnwindSafe(|| {
            crate::gc::full_trace::object_words_may_name_handles(header);
        }));
        assert!(
            red.is_err(),
            "the shared-storage declaration must reject a handle word"
        );
    });
}

extern "C" fn passive_observer(
    _bits: u64,
    _mark: extern "C" fn(u64, *mut c_void),
    _ctx: *mut c_void,
) -> bool {
    false
}

/// A young large container is promoted in place by the sweep that follows the
/// full mark, so the mark records its young children as an old parent's: as
/// bare old pages. An active foreign observer moves the visit onto the
/// observer path, which must keep that custody. Recorded as owner-qualified
/// external entries instead, the edges are invisible to array growth, which
/// inherits only old-page coverage, and the grown copy's young children go
/// unremembered.
#[test]
fn an_active_observer_keeps_a_young_large_containers_edges_as_old_pages() {
    isolated(|| unsafe {
        let container = crate::array::js_array_alloc(20_000);
        assert!(
            crate::arena::is_young_large_owner(container as usize),
            "fixture: the container must be a young large owner"
        );
        let child = crate::array::js_array_alloc(2);
        crate::array::js_array_set_f64_extend(
            container,
            3,
            f64::from_bits(ptr_bits(child as usize)),
        );
        let header = header_from_user_ptr(container.cast());
        assert!(crate::gc::full_trace::object_words_may_name_handles(header));
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(
            observer_phase,
            passive_observer,
        );
        let _scope = ObserverScope;
        crate::gc::full_trace::begin_full_trace();
        assert!(crate::gc::full_trace::handle_trace_active());
        let sticky = fold(header, &valid_set(false));
        let slot = crate::array::array_elements_ptr(container) as usize + 3 * 8;
        let page = crate::arena::generation_page_for_addr(slot);
        assert!(
            sticky.external_pages.is_empty(),
            "a young large container's edges are not owner-qualified: {:?}",
            sticky.external_pages
        );
        assert!(
            sticky.old_pages.contains(&page),
            "the young child's slot page must be recorded as an old page"
        );
    });
}
