//! Independent coverage oracles for a decoded word shared by full marking.
//! Marking and remembering have deliberately different acceptance domains.

use super::super::*;
use super::support::*;
use crate::gc::trace::full_mark_decode_sabotage;
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
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .env("PERRY_GC_VERIFY_CLASSIFIER", "1")
            .args([
                "--exact",
                "gc::tests::full_mark_word::active_classifier_differential_accepts_fold_and_rejects_census_disagreement",
                "--test-threads=1",
                "--nocapture",
            ])
            .output()
            .expect("launch classifier differential control");
        assert!(
            output.status.success(),
            "classifier control failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
            "exact child selector must execute the control",
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
        assert!(trace::classifier_valid_object_start(child));
        assert!(trace::classifier_valid_object_start(
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
        assert!(trace::classifier_verify_enabled());
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
        let classifier_rejected = !trace::classifier_valid_object_start(child);
        // The armed census owns a drop-restoration guard and therefore is
        // not RefUnwindSafe. This query only reads it; the deliberately
        // modified header is restored immediately after the caught panic.
        let red = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| valid.contains(&child)));
        (*child_header).size = size;
        assert!(classifier_rejected);
        let error = red.expect_err("active differential gate must reject census disagreement");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains("classifier rejected censused object"));
        assert!(trace::classifier_verify_enabled());
        assert!(valid.contains(&child));
    });
}

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

unsafe fn fold(header: *mut GcHeader, valid: &ValidPointerSet) -> StickyRememberedSet {
    let mut sticky = StickyRememberedSet::default();
    let mut worklist = Vec::new();
    trace::trace_heap_rewrite_slots_remembering(header, valid, &mut worklist, Some(&mut sticky));
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
        let red = std::panic::catch_unwind(|| assert_eq!(stats.missing_edges, 0, "{stats:?}"));
        assert!(
            red.is_err(),
            "the independent coverage oracle must turn red"
        );
    } else {
        assert_eq!(stats.missing_edges, 0, "{stats:?}");
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
                    assert!(!trace::mark_field_into_worklist(
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

fn external_old_cache(sabotage: bool) {
    isolated(move || unsafe {
        // The actual LazyArray descriptor enumerates bitmap-selected values
        // in a separately allocated old cache whose own header is a leaf.
        let lazy = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::json_tape::LazyArrayHeader>(),
            8,
            GC_TYPE_LAZY_ARRAY,
        )
        .cast::<crate::json_tape::LazyArrayHeader>();
        std::ptr::write_bytes(lazy, 0, 1);
        let cache = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_STRING).cast::<u64>();
        let bitmap = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_STRING).cast::<u64>();
        std::ptr::write_bytes(cache, 0, 4);
        std::ptr::write_bytes(bitmap, 0, 4);
        let child = young_leaf();
        *cache = string_bits(child);
        *bitmap = 1;
        (*lazy).magic = crate::json_tape::LAZY_ARRAY_MAGIC;
        (*lazy).cached_length = 1;
        (*lazy).materialized_elements = cache.cast();
        (*lazy).materialized_bitmap = bitmap;
        js_shadow_slot_set(0, ptr_bits(lazy as usize));
        let parent = header_from_user_ptr(lazy.cast());
        (*parent).gc_flags |= GC_FLAG_MARKED;
        assert!(crate::arena::pointer_in_old_gen(cache as usize));
        assert!(
            !GcMutableSlot::new(cache, None).external(),
            "the slot is old but outside its owner's allocation"
        );
        let expected = (
            parent as usize,
            crate::arena::generation_page_for_addr(cache as usize),
        );
        let valid = valid_set(false);
        let _sabotage = full_mark_decode_sabotage::Guard::new(false, sabotage);
        let sticky = fold(parent, &valid);
        assert_eq!(sticky.external_pages.contains(&expected), !sabotage);
        assert_coverage(&sticky, sabotage);
        if !sabotage {
            let _tenuring = crate::gc::tenuring::set_survivals_for_test(
                crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX,
            );
            let string = child as *const crate::StringHeader;
            let len = (*string).byte_len as usize;
            let expected = std::slice::from_raw_parts(
                (string as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                len,
            )
            .to_vec();
            clear_marks();
            for _ in 0..2 {
                let before = *cache;
                let trace = collect_minor_trace(GcTriggerKind::Direct);
                assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
                assert_ne!(*cache, before, "the external cache edge must actually move");
                assert_string_bytes(
                    (*cache & POINTER_MASK) as *const crate::StringHeader,
                    &expected,
                );
            }
        }
    });
}

#[test]
fn an_external_old_cache_retains_its_owner_descriptor() {
    external_old_cache(false);
}

#[test]
fn generation_only_custody_loses_external_old_cache_coverage() {
    external_old_cache(true);
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
struct ObserverScope;
impl Drop for ObserverScope {
    fn drop(&mut self) {
        crate::gc::full_trace::abort_full_trace();
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(inert_phase, observer);
        OBSERVER_SLOT.with(|slot| slot.set(std::ptr::null_mut()));
        OBSERVER_CHILD.with(|child| child.set(0));
    }
}
fn changing_observer(sabotage: bool) {
    isolated(move || unsafe {
        let child = young_leaf();
        let (parent, slot) = old_parent(ptr_bits(7));
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

#[test]
fn foreign_observer_changes_are_remembered_from_the_post_mark_word() {
    changing_observer(false);
}

#[test]
fn a_stale_snapshot_across_a_foreign_observer_loses_coverage() {
    changing_observer(true);
}
