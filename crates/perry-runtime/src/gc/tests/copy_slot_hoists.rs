//! The copying minor reads two facts once per traced object that it used to
//! re-derive for every slot of that object: whether the parent is a weak
//! holder, and whether the parent is in old-gen.
//!
//! Both are pinned by a COLLECTION and its observable outcome, not by reading
//! the hoisted value back — and each has a sabotaged twin that forgets the
//! fact, so the hoist is shown to be load-bearing rather than merely present.

use super::super::*;
use super::support::*;
use crate::gc::copying_parent_facts::copy_hoist_sabotage;

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
