//! #10182: the full mark does not queue a pointer-free object that is not a
//! forwarding stub; tracing it would do nothing.
//!
//! A rooted old array holds strings (leaves) and one string header forwarded to
//! another string, the shape old-page evacuation leaves behind. After a full,
//! every held string survives and so does the forwarding target, reachable only
//! through the stub's hop. The sabotaged twin skips queueing the forwarded leaf
//! too, and the target is swept.

use super::super::*;
use super::support::*;
use crate::gc::trace::leaf_mark_sabotage;

/// Returns `(held strings surviving, held strings, target survived)`.
fn collect(sabotaged: bool) -> (usize, usize, bool) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        unsafe {
            let (holder, elements) = alloc_old_test_array(64);
            let mut root = ptr_bits(holder as usize);
            js_gc_register_global_root(&mut root as *mut u64 as i64);
            let mut held = Vec::new();
            for i in 0..63usize {
                let user = crate::arena::arena_alloc_gc_old(24 + i, 8, GC_TYPE_STRING) as usize;
                *elements.add(i) = string_bits(user);
                held.push(user);
                // Dead neighbours, so a missed mark would really be reclaimed.
                crate::arena::arena_alloc_gc_old(24, 8, GC_TYPE_STRING);
            }
            let stub = crate::arena::arena_alloc_gc_old(24, 8, GC_TYPE_STRING) as usize;
            let target = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_STRING) as usize;
            crate::gc::set_forwarding_address(
                header_from_user_ptr(stub as *const u8) as *mut GcHeader,
                target as *mut u8,
            );
            *elements.add(63) = string_bits(stub);
            {
                let _sabotage = sabotaged.then(leaf_mark_sabotage::Guard::arm);
                let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                    GcTriggerKind::OldGenBytes,
                ));
            }
            let survived = held
                .iter()
                .filter(|&&u| (*header_from_user_ptr(u as *const u8)).obj_type == GC_TYPE_STRING)
                .count();
            let target_alive =
                (*header_from_user_ptr(target as *const u8)).obj_type == GC_TYPE_STRING;
            (survived, held.len(), target_alive)
        }
    })
    .join()
    .expect("leaf-mark test thread must not panic")
}

#[test]
fn unqueued_leaf_marks_keep_leaves_and_a_forwarded_leaf_still_hops() {
    let (survived, held, target_alive) = collect(false);
    assert_eq!(survived, held, "every held string must survive");
    assert!(
        target_alive,
        "the forwarding target must be reached through the stub"
    );
}

#[test]
fn sabotaged_forwarded_leaf_loses_its_target() {
    let (survived, held, target_alive) = collect(true);
    assert_eq!(
        survived, held,
        "plain leaves are unaffected by the sabotage"
    );
    assert!(
        !target_alive,
        "a forwarded leaf that is not queued never hops, and its target is swept"
    );
}
