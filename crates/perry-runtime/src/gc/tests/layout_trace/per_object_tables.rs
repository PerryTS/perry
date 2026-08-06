//! #7510: the emptiness flag that guards the per-object layout side tables.
//!
//! The flag's whole value is that `false` lets the allocation, store, death
//! and trace paths skip both maps. That is only sound while
//!
//!   flag == false  ⟹  both maps are empty
//!
//! holds, so these tests drive an object through every transition that can
//! populate or drain the maps and assert the implication after each one. A
//! stale `true` is merely slow and is deliberately not asserted against.

use super::*;
use crate::gc::layout_tables::{test_per_object_tables_are_empty, PER_OBJECT_LAYOUTS_NONEMPTY};

fn flag() -> bool {
    PER_OBJECT_LAYOUTS_NONEMPTY.with(|c| c.get())
}

/// The invariant itself: never `false` while either map holds an entry.
fn assert_flag_sound(context: &str) {
    if !flag() {
        assert!(
            test_per_object_tables_are_empty(),
            "{context}: flag claims both per-object layout tables are empty, but one is not — \
             every probe skipped on that claim would miss a live record"
        );
    }
}

/// A pointer-masked object with no `keys_array` cannot ride the shape-shared
/// descriptor, so it lands in the per-object maps — the one regime the flag
/// has to notice.
#[test]
fn test_per_object_tables_flag_arms_on_install_and_clears_on_death() {
    clear_marks();
    clear_mark_seeds();
    assert_flag_sound("before install");

    let obj = crate::object::js_object_alloc(0, 2);
    let pointer_mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        std::ptr::null(),
        0,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );

    assert!(
        flag(),
        "installing a per-object descriptor must arm the flag, or the next \
         `layout_forget_object` skips a live record"
    );
    assert!(!test_per_object_tables_are_empty());
    // The descriptor is still reachable through the guarded accessors.
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    crate::gc::layout_clear_for_ptr(obj as usize);

    assert_flag_sound("after death");
    assert!(
        test_per_object_tables_are_empty(),
        "object death must drain both per-object tables"
    );
    assert!(
        !flag(),
        "draining the last per-object record must re-arm the empty fast path"
    );

    clear_marks();
    clear_mark_seeds();
}

/// Two live records, removed one at a time: the flag may only go `false` once
/// the *second* one is gone.
#[test]
fn test_per_object_tables_flag_survives_a_partial_drain() {
    clear_marks();
    clear_mark_seeds();

    let first = crate::object::js_object_alloc(0, 2);
    let second = crate::object::js_object_alloc(0, 2);
    let pointer_mask = [0b10u64];
    for obj in [first, second] {
        js_gc_init_typed_shape_layout(
            obj as u64,
            2,
            std::ptr::null(),
            0,
            pointer_mask.as_ptr(),
            pointer_mask.len() as u32,
        );
    }
    assert!(flag(), "two per-object descriptors must arm the flag");

    crate::gc::layout_clear_for_ptr(first as usize);
    assert_flag_sound("after draining the first of two");
    assert!(
        flag(),
        "the second object's records are still live — clearing the flag here \
         would let every later probe skip them"
    );
    assert_eq!(test_layout_pointer_slot_count(second as usize, 2), Some(1));

    crate::gc::layout_clear_for_ptr(second as usize);
    assert_flag_sound("after draining the second");
    assert!(
        !flag(),
        "the last record is gone; the fast path must come back"
    );

    clear_marks();
    clear_mark_seeds();
}

/// A store that contradicts the descriptor evicts it
/// (`layout_set_typed_unknown`) — the removal path that does *not* go through
/// object death.
#[test]
fn test_per_object_tables_flag_tracks_a_typed_downgrade() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    // The install validates the live field bits against the mask it is handed,
    // so the raw-f64 slot has to already hold a number or the descriptor is
    // rejected before it is ever stored.
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.5));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.5));
    let raw_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(flag(), "a per-object raw-f64 descriptor must arm the flag");

    // Slot 0 is declared raw-f64; storing a string contradicts that and
    // evicts the whole descriptor.
    let child = crate::string::js_string_from_bytes(b"downgrade".as_ptr(), 9);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert_flag_sound("after a typed downgrade");
    assert!(
        test_per_object_tables_are_empty(),
        "a downgrade drops the per-object descriptor"
    );
    assert!(!flag());

    clear_marks();
    clear_mark_seeds();
}

/// The mutator path that grows a mask in place: a pointer written into a
/// pointer-free object installs a fresh `LAYOUT_SLOT_MASKS` entry from inside
/// `layout_note_slot`'s own `borrow_mut`, which is the one insert site that
/// cannot go through the wrappers.
#[test]
fn test_per_object_tables_flag_arms_on_a_pointer_store_into_a_pointer_free_object() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.0));
    crate::gc::layout_clear_for_ptr(obj as usize);
    unsafe {
        crate::gc::layout_init_pointer_free(obj as *mut u8);
    }
    assert!(!flag(), "a pointer-free object keeps no per-object record");

    let child = crate::string::js_string_from_bytes(b"late-pointer".as_ptr(), 12);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));

    assert_flag_sound("after a late pointer store");
    assert!(
        flag(),
        "the mask grown in place by `layout_note_slot` must arm the flag too"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    clear_marks();
    clear_mark_seeds();
}

/// The canonical keys array of a shape is anchored by the shape cache for the
/// program's lifetime (#179), so a per-element pointer mask on it never drains
/// — and ~every program builds at least one shape. That single entry is what
/// used to keep the emptiness fast path dead (`churn_alloc`: one hit in 40
/// million calls), so the header declaration that replaces it is load-bearing
/// for #7510, not a tidy-up.
#[test]
fn test_class_keys_array_declares_all_pointer_slots_instead_of_a_mask() {
    clear_marks();
    clear_mark_seeds();

    let packed: &[u8] = b"alpha\0beta\0gamma\0";
    let keys =
        crate::object::js_build_class_keys_array(0x7510, 3, packed.as_ptr(), packed.len() as u32);
    assert!(!keys.is_null());

    assert!(
        !flag(),
        "building a shape's keys array must not leave a permanent per-object \
         mask behind — that entry alone disables the fast path process-wide"
    );
    assert!(test_per_object_tables_are_empty());

    // The declaration has to be as precise as the mask it replaced: all three
    // key strings are still enumerated as children, and each one still traces.
    let key_headers: Vec<_> = (0..3)
        .map(|i| unsafe {
            let bits = *(keys as *const u8).add(8).cast::<u64>().add(i);
            header_from_user_ptr((bits & POINTER_MASK) as *const u8)
        })
        .collect();
    assert_eq!(test_layout_pointer_slot_count(keys as usize, 3), Some(3));
    assert_eq!(test_heap_child_slot_count(keys as *mut u8), 3);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (keys as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);
    for (i, header) in key_headers.iter().enumerate() {
        unsafe {
            assert_ne!(
                (**header).gc_flags & GC_FLAG_MARKED,
                0,
                "key string {i} must still be traced through the declared \
                 all-pointer layout"
            );
        }
    }

    clear_marks();
    clear_mark_seeds();
}
