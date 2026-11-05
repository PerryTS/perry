//! Block persistence under a `Scan` decision.
//!
//! `block_persistence_is_redundant` used to treat a `Scan` decision as proof that
//! every register-held object was already pinned. On wasm32-wasip1 the scan
//! returns without walking anything, so an allocation-point valve or emergency
//! reclaim (both run under a pinned `ManualGcScanGuard::force_full_scan`) dropped
//! the only protection an unrooted runtime-helper local had. The wasm answer is
//! modelled on the host through `ConservativeScanEffectiveTestGuard`.

use super::super::*;
use super::support::*;
use crate::gc::roots::ConservativeScanEffectiveTestGuard;

/// A rooted tenant (this also opens a shadow frame) and an unrooted owner in
/// the same arena block; the owner's side-table entry survives only while the
/// window force-marks it.
fn plant_unrooted_owner_beside_a_rooted_tenant() -> usize {
    crate::arena::arena_reset_all_blocks_to_zero();

    let (live, _) = unsafe { alloc_nursery_test_object(0) };
    js_shadow_slot_set(0, ptr_bits(live as usize));

    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        8,
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(ptr) };
    let owner = ptr as usize;
    crate::closure::closure_set_dynamic_prop(owner, "memo", 42.0);
    assert!(crate::closure::closure_get_own_dynamic_prop(owner, "memo").is_some());
    // Dead neighbours no native stack word points at, so the count below does
    // not depend on whether a real scan happened to pin `owner`.
    for _ in 0..256 {
        std::hint::black_box(young_leaf());
    }
    owner
}

/// A valve-style full: the scan is forced on, as the allocation-point sites do.
/// The test guards pin the scan mode, which makes `force_full_scan` a no-op, so
/// unpin it for the duration.
fn scan_pinned_full() {
    let previous = roots::set_conservative_stack_scan_override(None);
    let _restore = RestoreScanOverride(previous);
    let _scan =
        roots::ManualGcScanGuard::force_full_scan(ConservativeScanSite::NurseryChurnSlackValve);
    assert!(matches!(
        roots::conservative_stack_scan_decision(),
        roots::ConservativeStackScanDecision::Scan
    ));
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

struct RestoreScanOverride(Option<roots::ConservativeStackScanMode>);

impl Drop for RestoreScanOverride {
    fn drop(&mut self) {
        roots::set_conservative_stack_scan_override(self.0);
    }
}

/// Where the scan does nothing (wasm), a `Scan` decision must not skip the
/// window: the unrooted neighbour of the rooted tenant is force-marked.
#[test]
fn an_ineffective_scan_keeps_the_window() {
    std::thread::spawn(|| {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _effective = ConservativeScanEffectiveTestGuard::set(false);
        let owner = plant_unrooted_owner_beside_a_rooted_tenant();
        let _root_reset = ShadowAndGlobalRootResetGuard;
        assert!(roots::shadow_stack_has_active_frame());

        let before = crate::gc::block_persist_force_mark_count();
        scan_pinned_full();

        assert!(
            crate::gc::block_persist_force_mark_count() > before,
            "a Scan decision that scans nothing must not replace the window"
        );
        assert!(
            crate::closure::closure_get_own_dynamic_prop(owner, "memo").is_some(),
            "a force-marked owner is not dead, so its side-table entry survives"
        );
    })
    .join()
    .expect("window test thread must not panic");
}

/// Native control: where the scan is effective it pins the register-held
/// objects itself, so the same full still skips the window.
#[test]
fn an_effective_scan_still_replaces_the_window() {
    std::thread::spawn(|| {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _effective = ConservativeScanEffectiveTestGuard::set(true);
        let owner = plant_unrooted_owner_beside_a_rooted_tenant();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        let before = crate::gc::block_persist_force_mark_count();
        scan_pinned_full();

        assert_eq!(
            crate::gc::block_persist_force_mark_count(),
            before,
            "an effective scan makes the window redundant"
        );
        let _ = owner;
    })
    .join()
    .expect("window control thread must not panic");
}
