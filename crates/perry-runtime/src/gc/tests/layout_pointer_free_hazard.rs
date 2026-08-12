//! #7635: the `POINTER_FREE` trace-skip hazard is REAL and this test is the
//! probe that faults — the one the issue asked for.
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
//! Two arms, one plant (the `fromspace_protect` convention):
//!
//! - **red control** — the hazard: an object whose only reference to a young
//!   string sits in a field slot, with its layout state left claiming
//!   `POINTER_FREE` (what a `layout_finish_deferred_boxed_object` caller
//!   lying about `saw_pointer` would produce). The copying minor MOVES the
//!   object but — honoring the state — never visits the field: the slot
//!   bits survive verbatim and the child is retired with from-space.
//! - **green arm** — the real contract: identical construction, truthful
//!   finalize. The minor visits the field, evacuates the string, rewrites
//!   the slot.
//!
//! Subject-liveness is asserted in both arms (the object must MOVE, the
//! collection must run), so neither arm can pass vacuously.

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
    // POINTER_FREE standing over a pointer-bearing payload.
    layout_finish_deferred_boxed_object(obj as usize, finalize_truthfully);
    (obj as usize, child)
}

unsafe fn field0_bits(obj_user: usize) -> u64 {
    *((obj_user + OBJECT_HEADER_SIZE) as *const u64)
}

#[test]
fn a_lying_pointer_free_finalize_strands_the_field_child() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);

    let (obj, child_before) = unsafe { plant_object_with_young_string_child(false) };
    js_shadow_slot_set(0, ptr_bits(obj));

    let _ = gc_collect_minor();

    // Subject-liveness: the minor must have MOVED the object.
    let moved_obj = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved_obj, obj, "premise: the object must have moved");

    // The hazard, observed twice over:
    // 1. the slot was never rewritten — the collector skipped the payload;
    let child_after = unsafe { field0_bits(moved_obj) };
    assert_eq!(
        child_after, child_before,
        "a POINTER_FREE payload must not have been visited — a rewritten slot \
         means the skip did not happen and this test is not testing it"
    );
    // 2. the child the slot still names was retired with from-space.
    let child_addr = (child_after & POINTER_MASK) as usize;
    let word = unsafe { *(child_addr as *const u64) };
    assert_eq!(
        word,
        crate::arena::QUARANTINE_POISON_WORD,
        "the stranded child must sit in poisoned from-space — anything else \
         means something rescued it and the hazard is not being exercised"
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

    // The truthful state (UNKNOWN) visits the payload: the string is
    // evacuated and the slot rewritten to the live copy.
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
