//! The copying minor reads the parent's weak-holder fact once per traced
//! object instead of re-deriving it for every slot of that object (#10362).
//!
//! Pinned by a COLLECTION and its observable outcome, not by reading the
//! hoisted value back, and paired with a sabotaged twin that forgets the fact,
//! so the hoist is shown to be load-bearing rather than merely present.
//!
//! The parent's remembering fact (`ParentRemembering`) is hoisted too, but its
//! witness is NOT a collection: no sabotage of it could be made to fail that
//! way, because sticky dirty-page coverage carries an old→young edge
//! independently of the remembered-set re-insertion the fact controls
//! (#10388). It is pinned instead against the predicate it replaces,
//! `barrier_parent_needs_remembering`, over every parent kind that predicate
//! distinguishes — with a sabotaged twin that must disagree.

use super::super::*;
use super::support::*;
use crate::gc::copying_parent_facts::{copy_hoist_sabotage, ParentRemembering};

/// A young target reachable ONLY through a rooted `WeakRef`'s weak slot.
/// A copying minor must not evacuate through that slot, so the target dies
/// and the reference reads `undefined`.
fn weak_target_cleared_by_minor(sabotaged: bool) -> bool {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;

        // An OBJECT: a string is not "CanBeHeldWeakly", so `js_weakref_new`
        // would reject it before the collector is ever involved.
        let target = unsafe { alloc_nursery_test_object(0).0 } as usize;
        assert!(
            crate::arena::pointer_in_nursery(target),
            "premise: the weak target must be young, or the minor cannot collect it"
        );
        let holder = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target)));
        let mut root = ptr_bits(holder as usize);
        js_gc_register_global_root(&mut root as *mut u64 as i64);
        assert!(
            unsafe {
                crate::weakref::is_weak_holder_header(
                    header_from_user_ptr(holder as *const u8) as *mut GcHeader
                )
            },
            "premise: a WeakRef is a weak holder"
        );

        {
            let _sabotage = sabotaged.then(copy_hoist_sabotage::WeakGuard::arm);
            let _ = gc_collect_minor();
        }
        crate::weakref::js_weakref_deref(f64::from_bits(root)).to_bits()
            == crate::value::TAG_UNDEFINED
    })
    .join()
    .expect("copy-hoist weak test thread must not panic")
}

#[test]
fn a_copying_minor_skips_a_weak_holders_weak_slot_through_the_per_object_fact() {
    assert!(
        weak_target_cleared_by_minor(false),
        "a target reachable only through the WeakRef's weak slot must not be \
         evacuated through, so it dies in the nursery"
    );
}

#[test]
fn sabotaged_weak_holder_fact_evacuates_through_the_weak_slot() {
    assert!(
        !weak_target_cleared_by_minor(true),
        "with the per-object weak-holder fact forgotten the weak slot is \
         treated as strong and the target survives the minor"
    );
}

/// Every (parent, slot) pair on which the per-object remembering fact and the
/// per-slot predicate it replaces disagree, plus which answers the parents
/// produced — so the caller can require all three arms were exercised.
fn remembering_fact_disagreements(sabotaged: bool) -> (usize, Vec<ParentRemembering>) {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let (old, old_fields) = unsafe { alloc_old_test_object(2) };
        let (young, young_fields) = unsafe { alloc_nursery_test_object(2) };
        let malloc = alloc_tracked_test_symbol() as *mut u8;
        let header = |user: *mut u8| unsafe { header_from_user_ptr(user) as *mut GcHeader };
        let parents = [
            header(old as *mut u8),
            header(young as *mut u8),
            header(malloc),
        ];
        // An inline slot of each generation, and a slot outside every arena
        // block (the malloc object's own payload word): the three answers
        // `external()` can give a slot.
        let slots = [old_fields, young_fields, malloc as *mut u64];
        let _sabotage = sabotaged.then(copy_hoist_sabotage::RememberingGuard::arm);
        let mut disagreements = 0usize;
        let mut answers = Vec::new();
        for &parent in &parents {
            let parent_user = unsafe { (parent as *mut u8).add(GC_HEADER_SIZE) } as usize;
            let fact = unsafe { ParentRemembering::of(parent, false) };
            answers.push(fact);
            let skipped = unsafe { ParentRemembering::of(parent, true) };
            for &slot in &slots {
                let slot = GcMutableSlot::new(slot, None);
                let expected = crate::gc::barrier::barrier_parent_needs_remembering(
                    parent_user,
                    slot.external(),
                );
                disagreements += usize::from(fact.for_slot(slot) != expected);
                // `skip_remembering` is a proof that nothing is remembered.
                disagreements += usize::from(skipped.for_slot(slot));
            }
        }
        (disagreements, answers)
    })
    .join()
    .expect("remembering-fact test thread must not panic")
}

#[test]
fn the_per_object_remembering_fact_agrees_with_the_per_slot_predicate() {
    let (disagreements, answers) = remembering_fact_disagreements(false);
    assert_eq!(
        answers,
        vec![
            ParentRemembering::Always,
            ParentRemembering::Never,
            ParentRemembering::ExternalSlotsOnly,
        ],
        "premise: an old, a young and a malloc parent exercise all three answers"
    );
    assert_eq!(disagreements, 0);
}

#[test]
fn the_per_object_remembering_fact_sabotaged_disagrees_with_the_per_slot_predicate() {
    let (disagreements, _) = remembering_fact_disagreements(true);
    assert!(
        disagreements > 0,
        "a fact that forgets every parent must disagree with the predicate on \
         the old parent's slots"
    );
}
