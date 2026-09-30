//! A newborn object with an Any lane keeps its only child alive across a
//! copying minor without a layout finalizer. The owner and child must move,
//! and the field slot must be rewritten to the live copy.

use super::super::*;
use super::support::*;
use crate::arena::FromSpaceProtection;

const OBJECT_HEADER_SIZE: usize = std::mem::size_of::<crate::ObjectHeader>();

/// Build a 1-field object through the materialiser's exact store path
/// (`store_object_field_slot_layout_deferred`), holding a fresh young string
/// reachable ONLY through that field. Returns `(obj_user, child_bits)`.
unsafe fn plant_object_with_young_string_child() -> (usize, u64) {
    let packed_keys = b"a\0";
    let obj = crate::object::js_object_alloc_with_shape(
        0x7635,
        1,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    let child = string_bits(young_leaf());
    let saw_pointer = crate::object::store_object_field_slot_layout_deferred(obj, 0, child);
    assert!(
        saw_pointer,
        "premise: the stored value must be pointer-bearing"
    );
    (obj as usize, child)
}

unsafe fn field0_bits(obj_user: usize) -> u64 {
    *((obj_user + OBJECT_HEADER_SIZE) as *const u64)
}

/// The shape decides the trace even though no layout finalizer runs.
#[test]
fn any_lane_keeps_child_alive_without_layout_finalize() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);

    let (obj, child_before) = unsafe { plant_object_with_young_string_child() };
    js_shadow_slot_set(0, ptr_bits(obj));

    let _ = gc_collect_minor();

    let moved_obj = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved_obj, obj, "premise: the object must have moved");

    // The shape's lane for the field is `Any`: the payload was visited, the
    // string evacuated and the slot rewritten to the live copy.
    let child_after = unsafe { field0_bits(moved_obj) };
    assert_ne!(
        child_after, child_before,
        "the child slot must have been rewritten: the shape traces the field \
         whatever the layout state claims"
    );
    let child_addr = (child_after & POINTER_MASK) as usize;
    let word = unsafe { *(child_addr as *const u64) };
    assert_ne!(
        word,
        crate::arena::QUARANTINE_POISON_WORD,
        "the evacuated child must be live, not poisoned from-space"
    );
}
