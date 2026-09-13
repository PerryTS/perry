//! #10182: the full mark reads two facts once per traced object instead of once
//! per slot: whether the proxy registry observes the trace, and whether the
//! object is a weak holder whose weak slots are skipped.
//!
//! The weak-holder fact is the one whose loss would change liveness, so it is
//! pinned with a real collection: a rooted `WeakRef` whose target has no other
//! reference must be cleared by a full. The sabotaged twin makes the per-object
//! fact read false, the weak slot is traced strongly, and the target survives.

use super::super::*;
use super::support::*;
use crate::gc::trace::mark_hoist_sabotage;

fn weak_target_cleared(sabotaged: bool) -> bool {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        let target = unsafe { alloc_old_test_object(0).0 as usize };
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
            let _sabotage = sabotaged.then(mark_hoist_sabotage::Guard::arm);
            let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                GcTriggerKind::OldGenBytes,
            ));
        }
        crate::weakref::js_weakref_deref(f64::from_bits(root)).to_bits()
            == crate::value::TAG_UNDEFINED
    })
    .join()
    .expect("mark-hoist test thread must not panic")
}

#[test]
fn a_full_skips_a_weak_holders_weak_slot_through_the_per_object_fact() {
    assert!(
        weak_target_cleared(false),
        "the target reachable only through the WeakRef's weak slot must be cleared"
    );
}

#[test]
fn sabotaged_weak_holder_fact_keeps_the_weak_target_alive() {
    assert!(
        !weak_target_cleared(true),
        "with the per-object fact forgotten the weak slot is traced strongly"
    );
}
