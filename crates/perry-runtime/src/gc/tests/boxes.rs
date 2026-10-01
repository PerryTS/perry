use super::support::*;
use crate::gc::*;

#[test]
fn gc_box_closure_and_payload_move_together() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let cell = crate::r#box::js_box_alloc_bits(string_bits(child) as i64);
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
    js_shadow_slot_set(0, ptr_bits(closure as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let moved_closure =
        (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader;
    assert_ne!(moved_closure, closure);
    let moved_cell =
        crate::closure::js_closure_get_capture_ptr(moved_closure, 0) as *mut crate::r#box::Box;
    assert_ne!(
        moved_cell, cell,
        "capture must be rewritten to a moved cell"
    );
    let moved_child = crate::r#box::js_box_get_bits(moved_cell) as u64 & POINTER_MASK;
    assert_ne!(moved_child as usize, child, "cell payload must also move");
    assert!(build_valid_pointer_set().contains(&(moved_child as usize)));
    crate::r#box::js_box_set(moved_cell, 42.0);
    assert_eq!(crate::r#box::js_box_get(moved_cell), 42.0);
}

#[test]
fn gc_box_primitive_cells_move_without_tracing_payload_bits() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let state = crate::r#box::js_i32_box_alloc(-37);
    let done = crate::r#box::js_bool_box_alloc(1);
    js_shadow_slot_set(0, ptr_bits(state as usize));
    js_shadow_slot_set(1, ptr_bits(done as usize));
    gc_collect_minor();
    let moved_state = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::r#box::I32Box;
    let moved_done = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::r#box::BoolBox;
    assert_ne!(state, moved_state);
    assert_ne!(done, moved_done);
    assert_eq!(crate::r#box::js_i32_box_get(moved_state), -37);
    assert_eq!(crate::r#box::js_bool_box_get(moved_done), 1);
    assert!(test_gc_rewrite_slot_addresses(moved_state as usize)
        .unwrap()
        .is_empty());
    assert!(test_gc_rewrite_slot_addresses(moved_done as usize)
        .unwrap()
        .is_empty());
}

#[test]
fn gc_box_old_to_young_store_is_remembered_and_rewritten() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let cell = crate::arena::arena_alloc_gc_old(8, 8, GC_TYPE_BOX) as *mut crate::r#box::Box;
    unsafe {
        (*cell).value = crate::value::TAG_UNDEFINED;
    }
    js_shadow_slot_set(0, ptr_bits(cell as usize));
    let child = young_leaf();
    crate::r#box::js_box_set_bits(cell, string_bits(child) as i64);
    assert!(
        remembered_set_size() > 0,
        "setter must remember the old box"
    );
    gc_collect_minor();
    let moved = crate::r#box::js_box_get_bits(cell) as u64 & POINTER_MASK;
    assert_ne!(moved as usize, child);
    assert!(build_valid_pointer_set().contains(&(moved as usize)));
}

#[test]
fn gc_box_unreachable_closure_cycle_is_reclaimed_without_release() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let cell = crate::r#box::js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
    crate::r#box::js_box_set_bits(cell, ptr_bits(closure as usize) as i64);
    js_shadow_slot_set(0, ptr_bits(closure as usize));
    gc_collect_minor();
    let live_closure = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let live_cell =
        crate::closure::js_closure_get_capture_ptr(live_closure as *const _, 0) as usize;
    assert!(build_valid_pointer_set().contains(&live_cell));
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    gc_collect_inner();
    let live = build_valid_pointer_set();
    assert!(
        !live.contains(&live_cell),
        "no registry may keep the cell alive"
    );
    assert!(
        !live.contains(&live_closure),
        "cell/closure cycles must die"
    );
}
