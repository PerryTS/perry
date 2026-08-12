//! #7510 item 1: `js_gc_declare_typed_shape_layout` — installing a canonical
//! typed layout on a FRESHLY ALLOCATED instance, before its constructor runs.
//!
//! The runtime half of the change is one thing: this entry point does not
//! validate slot contents. `js_gc_init_typed_shape_layout` does, which is
//! exactly why it cannot be moved earlier — a fresh slot holds `TAG_UNDEFINED`
//! (`0x7FFC`, inside `layout_raw_f64_bits`' reject range), so an early call
//! would downgrade every instance it touched. These tests pin both halves of
//! that claim: the validating form still refuses a fresh object, the declaring
//! form accepts it, and the descriptor the declaring form installs is
//! afterwards maintained — and downgraded — by exactly the same store paths.

use super::*;

/// The reason a second entry point had to exist. Left as executable
/// documentation: if `layout_raw_f64_bits` ever starts accepting `undefined`,
/// this test fails and the whole `TypedShapeProof` split becomes removable.
#[test]
fn test_validating_install_refuses_a_freshly_allocated_instance() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let raw_mask = [0b11u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    assert!(
        !crate::gc::layout_typed_intact_for_user(obj as usize),
        "a fresh instance's slots hold `undefined`, which is not raw-f64 bits — \
         the validating install must refuse it, which is why the constructor's \
         own stores could never see a descriptor (#7512)"
    );

    clear_marks();
    clear_mark_seeds();
}

/// The declaring form accepts the same object, and the descriptor it installs
/// is real: the slots read back as raw-f64 through the query the codegen guard
/// consults.
#[test]
fn test_declaring_install_accepts_a_freshly_allocated_instance() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let raw_mask = [0b11u64];
    js_gc_declare_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    assert!(
        crate::gc::layout_typed_intact_for_user(obj as usize),
        "the declaring install must set the intact bit the class-field store \
         guard tests"
    );
    assert!(crate::gc::layout_typed_raw_f64_slot_for_user(
        obj as usize,
        0
    ));
    assert!(crate::gc::layout_typed_raw_f64_slot_for_user(
        obj as usize,
        1
    ));
    // An empty pointer mask means POINTER_FREE — byte-identical to what
    // `layout_init_pointer_free` already set at birth, so the collector's view
    // is unchanged by the declaration.
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));

    clear_marks();
    clear_mark_seeds();
}

/// The load-bearing follow-through: skipping validation does NOT mean the
/// descriptor is trusted forever. A store that contradicts it must still evict
/// it — otherwise a string written into a slot the collector believes is
/// raw-f64 would never be traced.
#[test]
fn test_a_contradicting_store_downgrades_a_declared_layout() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let raw_mask = [0b11u64];
    js_gc_declare_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(crate::gc::layout_typed_intact_for_user(obj as usize));

    let child = crate::string::js_string_from_bytes(b"contradiction".as_ptr(), 13);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert!(
        !crate::gc::layout_typed_intact_for_user(obj as usize),
        "a string stored into a slot declared raw-f64 must evict the descriptor"
    );

    // And the proof that the eviction is what keeps the collector honest: the
    // string is reachable only through that slot, and it must be traced.
    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "the downgraded object must be scanned conservatively — a missed \
             trace here is a use-after-free, not a slow path"
        );
    }

    clear_marks();
    clear_mark_seeds();
}

/// A declaration whose slot count disagrees with the object is still rejected:
/// the mismatch check runs ahead of the proof split, so a mis-derived mask
/// cannot ride in on the declaring path either.
#[test]
fn test_declaring_install_still_rejects_a_slot_count_mismatch() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let raw_mask = [0b111u64];
    js_gc_declare_typed_shape_layout(
        obj as u64,
        3, // the object has 2 fields
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    assert!(
        !crate::gc::layout_typed_intact_for_user(obj as usize),
        "a slot-count mismatch must land in the conservative state on both \
         entry points"
    );

    clear_marks();
    clear_mark_seeds();
}

/// Raw-f64 and pointer masks that overlap are contradictory on their face; the
/// declaring path must reject them without needing to look at any slot.
#[test]
fn test_declaring_install_rejects_overlapping_masks() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let raw_mask = [0b11u64];
    let pointer_mask = [0b10u64];
    js_gc_declare_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );

    assert!(
        !crate::gc::layout_typed_intact_for_user(obj as usize),
        "a slot cannot be both raw-f64 and pointer-bearing"
    );

    clear_marks();
    clear_mark_seeds();
}
