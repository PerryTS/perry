//! A Set's scalar cursor history follows the Set, without a GC table.
use super::super::*;
use super::support::*;

#[test]
fn set_compaction_history_survives_owner_evacuation() {
    check_objtables_set_cursor(false);
}

#[test]
#[ignore = "negative control: intentionally erase the owner invariant"]
fn negative_control_objtables_set_cursor() {
    check_objtables_set_cursor(true);
}

fn check_objtables_set_cursor(erase_invariant: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let set = crate::set::js_set_alloc(16);
    for i in 0..8 {
        crate::set::js_set_add(set, i as f64);
    }
    let epoch = crate::set::set_compaction_epoch(set);
    crate::set::js_set_clear(set);
    crate::set::js_set_add(set, 9.0);
    crate::set::js_set_add(set, 10.0);
    js_shadow_slot_set(0, crate::value::JSValue::pointer(set.cast()).bits());
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::set::SetHeader;
    assert_ne!(moved, set);
    if erase_invariant {
        unsafe {
            crate::set::test_remove_compaction_history(moved);
        }
    }
    let boxed = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        crate::set::js_set_cursor_next(boxed, 5.0, epoch as f64),
        0.0
    );
    // Negative control: lose the invariant and the resumed cursor skips all values.
    unsafe {
        crate::set::test_remove_compaction_history(moved);
    }
    assert_eq!(
        crate::set::js_set_cursor_next(boxed, 5.0, epoch as f64),
        -1.0
    );
    js_shadow_slot_set(0, 0);
}
