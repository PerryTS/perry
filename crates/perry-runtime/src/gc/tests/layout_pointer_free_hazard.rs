//! #7635: the `POINTER_FREE` trace-skip hazard was REAL and this test was the
//! probe that faulted — the one the issue asked for.
//!
//! Why every earlier probe was vacuous: `JSON.parse` of a non-tiny blob is
//! LAZY by default (#7499's tape). A probe that parses, churns, and only then
//! reads the records back materializes the whole cohort AFTER the collections
//! ran — there was nothing to strand. Forcing a wrong `POINTER_FREE` on such
//! a run measures nothing, which is exactly what #7633's audit observed. The
//! TS-level shape that discriminates is parse → touch every record →
//! churn → read (the touch defeats the tape); this unit test plants the same
//! hazard directly, with no env knob and no JSON in the loop.
//!
//! Charter step 5 retired that hazard: the collector traces an object BY
//! ITS SHAPE, and a layout state claiming `POINTER_FREE` is no longer read.
//! The plant is kept, both arms of it:
//!
//! - **the former red control** — the lying finalize leaves the object's
//!   layout state claiming `POINTER_FREE`. The shape's lane for the field is
//!   `Any`, so the copying minor visits the field, evacuates the string and
//!   rewrites the slot: the child is NOT stranded.
//! - **the truthful arm** — identical construction, truthful finalize, the
//!   same outcome.
//!
//! Subject-liveness is asserted in both arms (the object must MOVE, the
//! collection must run, the child must be rewritten), so neither arm can pass
//! vacuously.

use super::super::*;
use super::support::*;
use crate::arena::FromSpaceProtection;

const OBJECT_HEADER_SIZE: usize = std::mem::size_of::<crate::ObjectHeader>();

/// Build a 1-field object through the materialiser's exact store path
/// (`store_object_field_slot_layout_deferred`), holding a fresh young string
/// reachable ONLY through that field. Returns `(obj_user, child_bits)`.
unsafe fn plant_object_with_young_string_child(finalize_truthfully: bool) -> (usize, u64) {
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
    // The plant: a caller lying about what it stored leaves the birth
    // POINTER_FREE layout state standing over a pointer-bearing payload.
    layout_finish_deferred_boxed_object(obj as usize, finalize_truthfully);
    (obj as usize, child)
}

unsafe fn field0_bits(obj_user: usize) -> u64 {
    *((obj_user + OBJECT_HEADER_SIZE) as *const u64)
}

/// The shape, not the layout state, decides what is traced: a lying
/// `POINTER_FREE` finalize keeps the field child alive across the move.
#[test]
fn a_lying_pointer_free_finalize_no_longer_strands_the_field_child() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);

    let (obj, child_before) = unsafe { plant_object_with_young_string_child(false) };
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

#[test]
fn a_truthful_finalize_keeps_the_field_child_alive_across_the_move() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);

    let (obj, child_before) = unsafe { plant_object_with_young_string_child(true) };
    js_shadow_slot_set(0, ptr_bits(obj));

    let _ = gc_collect_minor();

    let moved_obj = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved_obj, obj, "premise: the object must have moved");

    // The shape's `Any` lane visits the payload: the string is evacuated
    // and the slot rewritten to the live copy.
    let child_after = unsafe { field0_bits(moved_obj) };
    assert_ne!(
        child_after, child_before,
        "the child slot must have been rewritten to the evacuated copy"
    );
    let child_addr = (child_after & POINTER_MASK) as usize;
    let word = unsafe { *(child_addr as *const u64) };
    assert_ne!(
        word,
        crate::arena::QUARANTINE_POISON_WORD,
        "the evacuated child must be live, not poisoned from-space"
    );
}
