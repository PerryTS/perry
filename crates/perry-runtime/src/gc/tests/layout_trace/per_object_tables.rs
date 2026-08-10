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
use crate::gc::ImmortalLayoutScope;

fn flag() -> bool {
    PER_OBJECT_LAYOUTS_NONEMPTY.with(|h| h.nonempty.get())
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

/// The control half of the immortal-scope pair: WITHOUT the scope, the very
/// same store mints a per-object mask and arms the flag.
///
/// This exists so the scoped test below cannot pass vacuously. If a future
/// change stops routing this shape through `layout_note_slot`'s mask-minting
/// branch at all, this test goes red and says so, rather than leaving its
/// partner green while testing nothing.
#[test]
fn test_pointer_store_outside_an_immortal_scope_still_mints_a_mask() {
    clear_marks();
    clear_mark_seeds();
    assert_flag_sound("before store");
    assert!(test_per_object_tables_are_empty());

    let obj = crate::object::js_object_alloc(0, 2);
    let child = crate::object::js_object_alloc(0, 0);
    crate::gc::layout_note_slot(obj as usize, 1, POINTER_TAG | (child as u64 & POINTER_MASK));

    assert!(
        flag(),
        "a first pointer store into a pointer-free object must mint a mask — \
         if it no longer does, the scoped test below proves nothing"
    );
    assert!(!test_per_object_tables_are_empty());
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    crate::gc::layout_clear_for_ptr(obj as usize);
    assert!(test_per_object_tables_are_empty());

    clear_marks();
    clear_mark_seeds();
}

/// Inside an [`ImmortalLayoutScope`] the identical store must leave both maps
/// empty — and the object must still trace its child, because the fallback is
/// `GC_LAYOUT_UNKNOWN` (the tag-checked scan), not "no pointers here".
#[test]
fn test_immortal_scope_stores_trace_without_taking_a_side_table_entry() {
    clear_marks();
    clear_mark_seeds();
    assert!(test_per_object_tables_are_empty());

    let obj = crate::object::js_object_alloc(0, 2);
    let child = crate::object::js_object_alloc(0, 0);
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    unsafe {
        *(obj as *mut u8).add(8).cast::<u64>().add(1) = POINTER_TAG | (child as u64 & POINTER_MASK);
    }
    {
        let _immortal = ImmortalLayoutScope::new();
        crate::gc::layout_note_slot(obj as usize, 1, POINTER_TAG | (child as u64 & POINTER_MASK));
    }

    assert!(
        test_per_object_tables_are_empty(),
        "an object built inside an ImmortalLayoutScope must not take out a \
         per-object layout record — one permanent entry disables the emptiness \
         fast path for every allocation the process will ever make"
    );
    assert!(!flag());

    // Correctness half: the child is still reached. `GC_LAYOUT_UNKNOWN` scans
    // the payload with a tag check, so precision is lost but nothing is missed.
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
            "the child must still be traced through the tag-checked scan the \
             immortal scope falls back to"
        );
    }

    clear_marks();
    clear_mark_seeds();
}

/// The acceptance gate for the `globalThis` bootstrap itself.
///
/// The bootstrap builds several hundred permanently-rooted objects. Before the
/// `ImmortalLayoutScope` around it, each of their first pointer fields minted a
/// mask that nothing would ever remove, so the first plain-object property miss
/// in ANY program permanently disarmed `PER_OBJECT_LAYOUTS_NONEMPTY`.
///
/// The assertion is paired with a subject-live check: `Array` must resolve to a
/// real closure value, so a bootstrap that silently did nothing cannot pass this
/// by leaving the tables trivially empty.
#[test]
fn test_global_this_bootstrap_leaves_the_per_object_layout_tables_empty() {
    let global = crate::object::js_get_global_this();
    assert_eq!(
        global.to_bits() >> 48,
        0x7FFD,
        "globalThis must be a real heap object for this test to mean anything"
    );
    // Subject-live: the bootstrap actually populated the singleton.
    let array_ctor = crate::object::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
    assert_eq!(
        array_ctor.to_bits() >> 48,
        0x7FFD,
        "globalThis.Array must be populated — otherwise the emptiness assertion \
         below is vacuous"
    );

    let (slot_masks, typed) = crate::gc::per_object_layout_table_sizes();
    assert_eq!(
        (slot_masks, typed),
        (0, 0),
        "the globalThis bootstrap left {slot_masks} slot-mask and {typed} typed \
         per-object layout records behind; every one of them is immortal, so \
         `layout_forget_object` now runs its full two-map probe on every \
         allocation, death and relocation for the rest of the process"
    );
}

/// The address filter is an *accelerator*, never an authority: `false` must be
/// a proof of absence and nothing else may rest on it. This drives enough
/// inserts to force at least one filter rebuild and then checks, for every
/// live record, that the guarded accessors still find it and that
/// `layout_forget_object` still removes it.
#[test]
fn test_addr_filter_never_hides_a_live_record_across_a_rebuild() {
    clear_marks();
    clear_mark_seeds();

    // Comfortably more inserts than the rebuild threshold (half the bits), so
    // the rebuild path is exercised rather than merely reachable.
    let mut objs = Vec::new();
    for _ in 0..6000 {
        let obj = crate::object::js_object_alloc(0, 2);
        let child = crate::object::js_object_alloc(0, 0);
        crate::gc::layout_note_slot(obj as usize, 1, POINTER_TAG | (child as u64 & POINTER_MASK));
        objs.push(obj);
    }
    assert!(flag(), "6000 masks must arm the flag");

    for (i, obj) in objs.iter().enumerate() {
        assert_eq!(
            test_layout_pointer_slot_count(*obj as usize, 2),
            Some(1),
            "record {i} became invisible — the filter proved absence for an \
             address that has a live entry"
        );
    }
    for obj in &objs {
        crate::gc::layout_clear_for_ptr(*obj as usize);
    }
    assert!(
        test_per_object_tables_are_empty(),
        "every record must still be removable after a filter rebuild"
    );
    assert!(!flag());

    clear_marks();
    clear_mark_seeds();
}

/// Subject-live check for the accelerator itself: with a record present (so
/// `PER_OBJECT_LAYOUTS_NONEMPTY` is armed and the old global test would force
/// the full two-map probe), an unrelated address must still be *proved absent*.
///
/// This is the whole point of the address filter. The global flag alone cannot
/// distinguish "some object somewhere has a record" from "this address has a
/// record", so one immortal entry — which the `globalThis` bootstrap used to
/// leave 1113 of, and which ordinary runtime init still leaves one or two of —
/// put every allocation, death and relocation in the process back on the slow
/// path. If this assertion ever fails, the accelerator has silently stopped
/// accelerating even though nothing throws.
#[test]
fn test_addr_filter_proves_absence_while_the_global_flag_is_armed() {
    clear_marks();
    clear_mark_seeds();

    let live = crate::object::js_object_alloc(0, 2);
    let child = crate::object::js_object_alloc(0, 0);
    crate::gc::layout_note_slot(
        live as usize,
        1,
        POINTER_TAG | (child as u64 & POINTER_MASK),
    );
    assert!(
        flag(),
        "the global flag must be armed for this test to mean anything"
    );
    assert!(crate::gc::layout_tables::layout_addr_filter_may_hold(
        live as usize
    ));

    // A large sample of unrelated addresses: with one live record in 8192 bits
    // the filter must prove nearly all of them absent. Anything close to 100%
    // "maybe" means the filter is saturated or mis-hashed and the fast path is
    // gone even though every test still passes.
    let probes = 4096usize;
    let mut maybe = 0usize;
    for i in 0..probes {
        let addr = 0x2000_0000_0000usize + i * 64;
        if addr == live as usize {
            continue;
        }
        if crate::gc::layout_tables::layout_addr_filter_may_hold(addr) {
            maybe += 1;
        }
    }
    assert!(
        maybe * 100 < probes,
        "{maybe}/{probes} unrelated addresses were not proved absent — the \
         address filter is not accelerating anything"
    );

    crate::gc::layout_clear_for_ptr(live as usize);
    clear_marks();
    clear_mark_seeds();
}
