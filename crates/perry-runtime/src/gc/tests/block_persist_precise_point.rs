//! Block persistence at a precise collection point on a target whose
//! conservative scan does nothing (wasm32).
//!
//! The window substitutes for the conservative scan exactly where one was
//! requested and cannot run (`block_persist_scan_window.rs`). A collection that
//! requests no scan is at a precise point, where the root set is complete, so
//! the window must not run there even though a generated frame is live: on
//! wasm the event loop runs inside the generated main's frame, so "a shadow
//! frame is live" is true for every pump-boundary and safepoint cycle of a
//! server. The wasm answer is modelled on the host through
//! `ConservativeScanEffectiveTestGuard`.

use super::super::*;
use super::support::*;
use crate::gc::roots::ConservativeScanEffectiveTestGuard;

fn reset_old_reclaim_pressure() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

/// A budgeted, non-manual cycle driven the way the safepoint poll drives it,
/// with a main-shaped shadow frame live and the wasm scan answer: no object of
/// the recent window is force-marked, and the rooted survivor is intact.
///
/// Fails before the rule: the live shadow frame alone ran the pass.
#[test]
fn a_precise_point_cycle_under_a_live_frame_does_not_force_mark_its_window() {
    std::thread::spawn(|| {
        let _guard = CopyingNurseryTestGuard::new(2);
        let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _effective = ConservativeScanEffectiveTestGuard::set(false);
        reset_old_reclaim_pressure();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        let keep_bytes = b"wtc_block_persist_precise_survivor";
        let keep =
            crate::string::js_string_from_bytes(keep_bytes.as_ptr(), keep_bytes.len() as u32);
        js_shadow_slot_set(0, string_bits(keep as usize));
        for _ in 0..20_000 {
            std::hint::black_box(young_leaf());
        }
        assert!(roots::shadow_stack_has_active_frame());
        assert!(!matches!(
            roots::conservative_stack_scan_decision(),
            roots::ConservativeStackScanDecision::Scan
        ));
        trigger_guard.make_arena_trigger_due();

        let before = crate::gc::block_persist_force_mark_count();
        let mut result = JsGcStepResult::default();
        assert_eq!(
            js_gc_step_work_units(1, &mut result),
            JS_GC_STEP_STATUS_ACTIVE,
            "the fixture must start a budgeted cycle"
        );
        assert_ne!(result.trigger_kind, GcTriggerKind::Manual.ffi_code());
        let completed = complete_budgeted_gc_cycle();
        assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
        let after = crate::gc::block_persist_force_mark_count();

        assert_eq!(
            after,
            before,
            "no conservative scan was requested, so the root set is precise and \
             the window must not run; it force-marked {} objects",
            after - before
        );
        unsafe {
            assert_string_bytes(
                (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader,
                keep_bytes,
            );
        }
    })
    .join()
    .expect("precise-point window test thread must not panic");
}

/// The same precise collection as a synchronous full (the shape a safepoint
/// drain or a host-pressure collection takes).
#[test]
fn a_precise_full_under_a_live_frame_does_not_force_mark_its_window() {
    std::thread::spawn(|| {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _effective = ConservativeScanEffectiveTestGuard::set(false);
        reset_old_reclaim_pressure();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        let keep_bytes = b"wtc_block_persist_precise_full";
        let keep =
            crate::string::js_string_from_bytes(keep_bytes.as_ptr(), keep_bytes.len() as u32);
        js_shadow_slot_set(0, string_bits(keep as usize));
        for _ in 0..20_000 {
            std::hint::black_box(young_leaf());
        }
        assert!(roots::shadow_stack_has_active_frame());

        let before = crate::gc::block_persist_force_mark_count();
        let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::Direct,
        ));
        let after = crate::gc::block_persist_force_mark_count();

        assert_eq!(
            after,
            before,
            "a precise full must not force-mark its window; it force-marked {} objects",
            after - before
        );
        unsafe {
            assert_string_bytes(
                (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader,
                keep_bytes,
            );
        }
    })
    .join()
    .expect("precise-full window test thread must not panic");
}
