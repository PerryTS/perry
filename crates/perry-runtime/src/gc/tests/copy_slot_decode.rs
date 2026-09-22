//! The copying minor decodes each visited word ONCE (`visit_value_bits_child`):
//! a raw word's validating classification is the one the mark uses, and the
//! remembering arm reuses the child the visit decoded instead of re-decoding
//! the slot. Both halves are pinned by collections and what they leave in the
//! heap, and each has a sabotaged twin that must fail.

use super::super::*;
use super::support::*;
use crate::gc::copying_parent_facts::copy_decode_sabotage::{Guard, CHILD, RAW_MARK};

fn string_bytes(addr: usize) -> Vec<u8> {
    unsafe {
        let s = addr as *const crate::StringHeader;
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

/// A young string reachable ONLY through a RAW (untagged) word in a rooted
/// young object. Returns whether the minor evacuated it through that word.
fn raw_child_evacuated(sabotaged: bool) -> bool {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _roots = ShadowAndGlobalRootResetGuard;
        let (parent, fields) = unsafe { alloc_nursery_test_object(1) };
        let child = young_leaf();
        let expected = string_bytes(child);
        unsafe { *fields = child as u64 };
        js_shadow_slot_set(0, ptr_bits(parent as usize));
        assert!(
            crate::arena::pointer_in_nursery(child),
            "premise: the child must be young, or there is nothing to evacuate"
        );
        {
            let _sabotage = sabotaged.then(|| Guard::arm(RAW_MARK));
            let _ = gc_collect_minor();
        }
        let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(
            parent_after, parent as usize,
            "premise: the rooted parent moved"
        );
        let word = unsafe {
            *((parent_after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64)
        };
        // Checked before any read through `word`: a stale word names from-space.
        word != child as u64 && string_bytes(word as usize) == expected
    })
    .join()
    .expect("raw-word decode test thread must not panic")
}

#[test]
fn a_raw_word_is_marked_through_its_validating_classification() {
    assert!(
        raw_child_evacuated(false),
        "the young child behind a raw word must be evacuated and the word rewritten"
    );
}

#[test]
fn sabotaged_raw_mark_leaves_the_raw_word_stale() {
    assert!(
        !raw_child_evacuated(true),
        "with the validated raw word dropped instead of marked, the child is not evacuated"
    );
}

/// An OLD parent whose NaN-boxed slot holds a young child, handed to the minor
/// through the write barrier, then two minors: the second finds the edge only
/// if the first re-remembered it from the child its visit decoded. `Err` is
/// the collection thread's panic message.
fn old_edge_across_two_minors(sabotaged: bool) -> Result<bool, String> {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _tenuring = crate::gc::tenuring::set_survivals_for_test(
            crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX,
        );
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _roots = ShadowAndGlobalRootResetGuard;
        let (parent, fields) = unsafe { alloc_old_test_object(1) };
        let child = young_leaf();
        let expected = string_bytes(child);
        unsafe { *fields = ptr_bits(child) };
        js_write_barrier_slot(ptr_bits(parent as usize), fields as u64, ptr_bits(child));
        assert!(
            crate::arena::pointer_in_old_gen(parent as usize)
                && crate::arena::pointer_in_nursery(child),
            "premise: an old parent and a young child"
        );
        let read = || unsafe { (*fields & POINTER_MASK) as usize };
        {
            let _sabotage = sabotaged.then(|| Guard::arm(CHILD));
            let _ = gc_collect_minor();
        }
        let first = read();
        assert!(
            first != child && crate::arena::pointer_in_nursery(first),
            "premise: the first minor copied the child within the nursery"
        );
        let _ = gc_collect_minor();
        let second = read();
        second != first && string_bytes(second) == expected
    })
    .join()
    .map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

#[test]
fn an_old_parents_edge_is_remembered_from_the_child_the_visit_decoded() {
    assert_eq!(
        old_edge_across_two_minors(false),
        Ok(true),
        "the second minor must find and move the child through the remembered edge"
    );
}

/// In a production release build `restore_surviving_dirty_coverage` would
/// re-add the page the arm failed to remember, which is why a forgotten
/// remembered-set entry is invisible to a survival check alone. Unit-test
/// builds retain the same cross-check as debug builds, so this sabotage twin
/// proves the dirty scan's per-slot re-remembering remains load-bearing under
/// `cargo test --release` as well.
#[test]
fn sabotaged_remembering_arm_is_refused_by_the_coverage_cross_check() {
    let outcome = old_edge_across_two_minors(true);
    assert!(
        matches!(&outcome, Err(message) if message.contains("restore_surviving_dirty_coverage")),
        "with the decoded child forgotten, the coverage walk must report the \
         unremembered page; got {outcome:?}"
    );
}
