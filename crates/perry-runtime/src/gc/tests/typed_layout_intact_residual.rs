//! #8115: `GC_OBJ_TYPED_LAYOUT_INTACT` must never outlive the descriptor it
//! claims — not even for #7834's descriptor-less at-allocation bake.
//!
//! ## The state under test
//!
//! #7834 stamps `GC_LAYOUT_POINTER_FREE | GC_OBJ_TYPED_LAYOUT_INTACT` into the
//! inline `new`'s header constant for a class whose pointer mask is statically
//! empty, and installs **no** descriptor. Its published argument is about the
//! collector, and for the collector it holds: a `POINTER_FREE` payload is
//! skipped without consulting any map, and a later pointer store downgrades
//! through `layout_note_slot`'s generic pointer-mask branch, which needs no
//! descriptor.
//!
//! What that argument does not cover is the bit left behind. `set_layout_state`
//! masks `!(GC_LAYOUT_STATE_MASK | GC_LAYOUT_ALL_POINTERS)` = `!0xE000` and the
//! intact bit is `0x1000`, so before #8115 the generic branch published
//! `GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT` **with no descriptor
//! behind it** — a state three codegen consumers read as a licence to skip a
//! map (see [`inline_guard_raw_f64_arm_taken`] and
//! [`conforming_note_elision_taken`]).
//!
//! ## Why the state transition alone is not the test
//!
//! Asserting "the state became `SIDE_MASK`" passes on an unfixed tree — that
//! part always worked. The discriminating conjunction is *reaching that state
//! without a descriptor* **and then taking the inline raw-f64 arm*, so every
//! test below ends by evaluating the codegen predicate over the header the
//! runtime actually produced, and one of them carries the read through to the
//! value the emitted fast path would have handed the program.
//!
//! Two premises are asserted from the maps, never from the bit:
//!
//! * `layout_descriptor_reachable` (not `layout_has_typed_descriptor`, which
//!   *reads the bit under test*) proves the plant is descriptor-less;
//! * `per_object_slot_mask` proves the generic pointer-mask branch really
//!   minted a mask, so a run where `layout_note_slot` returned early cannot
//!   pass as a green one.
//!
//! The legitimate `SIDE_MASK | INTACT` case — a real shared descriptor with a
//! non-empty pointer mask, which `perry-codegen`'s
//! `class_field_store_layout_note_is_conforming` depends on — gets its own
//! test, and it must keep passing both predicates.

use super::super::*;
use super::support::*;
use crate::gc::layout::layout_descriptor_reachable;
use crate::gc::layout_tables::per_object_slot_mask;

const OBJECT_HEADER_SIZE: usize = std::mem::size_of::<crate::ObjectHeader>();

// ---------------------------------------------------------------------------
// Mirrors of the predicates perry-codegen emits over `GcHeader::_reserved`.
// perry-codegen does not depend on perry-runtime, so its constants are textual
// decimals ("4096", "-12288", "-28672"); `codegen_predicate_constants_match`
// below is the anti-drift gate, on `element_shape_guard.rs`'s precedent.
// ---------------------------------------------------------------------------

/// `expr/class_field_inline_guard.rs:303` and `:543`:
/// `and i16 %reserved, 4096` / `icmp ne i16 %intact, 0`. The whole layout half
/// of the raw-f64 licence — there is no layout-STATE test anywhere in that
/// file. `expr/element_shape_guard.rs` folds the same bit into its packed
/// `0x1800_80FF` header compare.
fn inline_guard_raw_f64_arm_taken(reserved: u16) -> bool {
    reserved & 4096 != 0
}

/// `expr/write_barrier.rs:713-720`:
/// `and i16 %reserved, -12288` / `icmp eq i16 %masked, -28672`, i.e.
/// `_reserved & 0xD000 == 0x9000`. When it holds, `layout_note_slot` is not
/// called at all for a pointer store into a class-declared pointer slot.
fn conforming_note_elision_taken(reserved: u16) -> bool {
    (reserved & 0xD000u16) == 0x9000u16
}

#[test]
fn codegen_predicate_constants_match_the_runtime() {
    assert_eq!(4096u16, GC_OBJ_TYPED_LAYOUT_INTACT, "intact bit drifted");
    assert_eq!(
        0xD000u16,
        GC_LAYOUT_STATE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT,
        "conforming-elision mask drifted"
    );
    assert_eq!(
        0x9000u16,
        GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT,
        "conforming-elision expectation drifted"
    );
    // The textual i16 forms perry-codegen emits.
    assert_eq!(-12288i16, 0xD000u16 as i16);
    assert_eq!(-28672i16, 0x9000u16 as i16);
}

// ---------------------------------------------------------------------------
// Plants
// ---------------------------------------------------------------------------

unsafe fn reserved_of(obj: *mut crate::ObjectHeader) -> u16 {
    (*header_from_user_ptr(obj as *const u8))._reserved
}

unsafe fn slot_bits(obj: *mut crate::ObjectHeader, slot: usize) -> u64 {
    *((obj as usize + OBJECT_HEADER_SIZE + slot * 8) as *const u64)
}

/// Reproduce #7834's bake **exactly**: a shape-keyed instance whose header
/// claims `GC_LAYOUT_POINTER_FREE | GC_OBJ_TYPED_LAYOUT_INTACT` and for which
/// `js_gc_declare_typed_shape_layout` was never called. Not calling it IS the
/// plant — that call is the thing #7834 removes.
///
/// Three payload slots, because a payload below `layout_mask_min_slots()` makes
/// `layout_note_slot` decline the mask and take `GC_LAYOUT_UNKNOWN` instead;
/// this test wants the `SIDE_MASK` arm.
unsafe fn plant_baked_instance(shape_id: u32, packed_keys: &[u8]) -> *mut crate::ObjectHeader {
    let obj = crate::object::js_object_alloc_with_shape(
        shape_id,
        3,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    let header = header_from_user_ptr(obj as *const u8);
    (*header)._reserved = ((*header)._reserved & !GC_LAYOUT_STATE_MASK)
        | GC_LAYOUT_POINTER_FREE
        | GC_OBJ_TYPED_LAYOUT_INTACT;
    assert!(
        !layout_descriptor_reachable(obj as usize),
        "premise: the bake installs NO descriptor — that is what makes it free"
    );
    assert!(
        inline_guard_raw_f64_arm_taken(reserved_of(obj)),
        "premise: at birth the bake does claim the raw-f64 licence"
    );
    obj
}

/// The legitimate `SIDE_MASK | INTACT`: a real shared descriptor whose pointer
/// mask is non-empty, installed the way the runtime declare installs one.
unsafe fn plant_descriptor_backed_instance(
    shape_id: u32,
    packed_keys: &[u8],
) -> *mut crate::ObjectHeader {
    let obj = crate::object::js_object_alloc_with_shape(
        shape_id,
        3,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    // slot 0 pointer, slots 1..2 raw f64 — the shape of `class C { s: string;
    // x: number; y: number }`.
    let raw_f64_words: [u64; 1] = [0b110];
    let pointer_words: [u64; 1] = [0b001];
    crate::gc::js_gc_declare_typed_shape_layout(
        ptr_bits(obj as usize),
        3,
        raw_f64_words.as_ptr(),
        1,
        pointer_words.as_ptr(),
        1,
    );
    assert!(
        layout_descriptor_reachable(obj as usize),
        "premise: the declare must install a reachable descriptor"
    );
    obj
}

// ---------------------------------------------------------------------------
// The residual
// ---------------------------------------------------------------------------

/// The fix. A pointer stored into a statically-all-`number` slot is a runtime
/// type violation, which Perry permits by design (CLAUDE.md, "No runtime type
/// *validation*") — it is the one way to drive a baked instance off its
/// `POINTER_FREE` birth state. Doing so must retire the raw-f64 licence with it.
#[test]
fn a_descriptorless_bake_drops_its_intact_claim_on_the_generic_downgrade() {
    unsafe {
        let obj = plant_baked_instance(0x8115_0001, b"x\0y\0z\0");
        let child = string_bits(young_leaf());

        crate::object::store_object_field_slot(obj, 0, child);

        // Subject-liveness: the generic pointer-mask branch must actually have
        // run and minted a mask. Without this, a `layout_note_slot` that
        // returned early would pass every assertion below vacuously.
        let reserved = reserved_of(obj);
        assert_eq!(
            reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_SIDE_MASK,
            "premise: the generic branch must have published SIDE_MASK"
        );
        let mask = per_object_slot_mask(obj as usize)
            .expect("premise: the generic branch must have minted a per-object mask");
        assert!(
            mask.contains_slot(0),
            "premise: the mask must record slot 0 as pointer-bearing"
        );
        assert!(
            !layout_descriptor_reachable(obj as usize),
            "premise: nothing installed a descriptor — the state is SIDE_MASK \
             with no descriptor behind it, which is the #8115 state"
        );

        // The residual itself, in both consumers' terms.
        assert!(
            !inline_guard_raw_f64_arm_taken(reserved),
            "#8115: a descriptor-less object must not license codegen's raw-f64 \
             arm — `_reserved` = {reserved:#06x}"
        );
        assert!(
            !conforming_note_elision_taken(reserved),
            "#8115: a descriptor-less object must not license the conforming \
             layout-note elision — `_reserved` = {reserved:#06x}"
        );
    }
}

/// The conjunction the issue names, carried through to the value the program
/// sees: reach `SIDE_MASK` without a descriptor, then take the inline arm.
///
/// The emitted fast path is a bare `load double` of the slot. Slot 0 holds a
/// NaN-boxed string, so taking that arm hands the program
/// `f64::from_bits(STRING_TAG | addr)` — a NaN — where the fallback hands it the
/// string. Sabotage check: restore the stale bit (the `red_control` arm) and
/// the same code reads the NaN, which is what an unfixed tree produces.
#[test]
fn the_inline_raw_f64_arm_must_not_read_a_pointer_slot_as_a_double() {
    unsafe {
        let obj = plant_baked_instance(0x8115_0002, b"a\0b\0c\0");
        let child = string_bits(young_leaf());
        crate::object::store_object_field_slot(obj, 0, child);
        assert_eq!(
            slot_bits(obj, 0),
            child,
            "premise: the slot holds the string"
        );

        // What the emitted guard would do, evaluated against the header the
        // runtime actually produced.
        let green = read_field_the_way_codegen_would(obj, 0);
        assert_eq!(
            green,
            FieldRead::Boxed(child),
            "#8115: the guard must fall back to the boxed read; taking the raw \
             arm here yields {:?}",
            FieldRead::RawDouble(f64::from_bits(child))
        );

        // Red control: the pre-#8115 header, reconstructed by putting the stale
        // bit back. This is the sabotage, in-test — it proves the assertion
        // above can fail, and that what it forbids is a real wrong answer.
        let header = header_from_user_ptr(obj as *const u8);
        (*header)._reserved |= GC_OBJ_TYPED_LAYOUT_INTACT;
        let red = read_field_the_way_codegen_would(obj, 0);
        match red {
            FieldRead::RawDouble(v) => assert!(
                v.is_nan(),
                "red control: the raw arm must read the NaN-box as a double"
            ),
            FieldRead::Boxed(_) => panic!(
                "red control did not reproduce the residual — the raw-f64 arm \
                 was not taken with the stale intact bit restored"
            ),
        }
        (*header)._reserved &= !GC_OBJ_TYPED_LAYOUT_INTACT;
    }
}

#[derive(Debug, PartialEq)]
enum FieldRead {
    /// `class_field_inline_guard`'s fast arm: `load double, ptr %slot`.
    RawDouble(f64),
    /// The guard call / by-name fallback: the NaN-boxed slot word, unchanged.
    Boxed(u64),
}

/// Model of the emitted `this.f` read: take the raw-f64 arm iff the guard's
/// layout predicate holds. The guard's other conjuncts (class id, ShapeId,
/// `obj_type`, not-forwarded, no per-object descriptors) are all trivially true
/// for this fixture's single unmodified object, so the intact bit is the only
/// one that can decide the branch.
unsafe fn read_field_the_way_codegen_would(
    obj: *mut crate::ObjectHeader,
    slot: usize,
) -> FieldRead {
    let bits = slot_bits(obj, slot);
    if inline_guard_raw_f64_arm_taken(reserved_of(obj)) {
        FieldRead::RawDouble(f64::from_bits(bits))
    } else {
        FieldRead::Boxed(bits)
    }
}

// ---------------------------------------------------------------------------
// The legitimate case must survive
// ---------------------------------------------------------------------------

/// `perry-codegen`'s `class_field_store_layout_note_is_conforming` elides the
/// layout note on exactly `SIDE_MASK | INTACT`, and its proof is "a descriptor
/// built from this class's mask globals is reachable". #8115 must not cost that
/// case its bit: the clear fires only where BOTH descriptor maps answered
/// `None`, and a descriptor-backed object returns from the `Some(verdict)` arm
/// long before it.
#[test]
fn a_descriptor_backed_side_mask_object_keeps_its_intact_claim() {
    unsafe {
        let obj = plant_descriptor_backed_instance(0x8115_0003, b"s\0x\0y\0");

        let birth = reserved_of(obj);
        assert_eq!(
            birth & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_SIDE_MASK,
            "premise: a non-empty pointer mask installs SIDE_MASK"
        );
        assert!(
            conforming_note_elision_taken(birth),
            "premise: this is the state expr/helpers.rs documents as legitimate"
        );

        // A conforming pointer store into the declared pointer slot: the very
        // store the elision is for.
        crate::object::store_object_field_slot(obj, 0, string_bits(young_leaf()));
        // And a conforming raw-f64 store into a declared raw-f64 slot.
        crate::object::store_object_field_slot(obj, 1, 1.5f64.to_bits());

        let after = reserved_of(obj);
        assert!(
            layout_descriptor_reachable(obj as usize),
            "the descriptor must survive two conforming stores"
        );
        assert!(
            inline_guard_raw_f64_arm_taken(after),
            "#8115 must not clear the bit on a descriptor-backed object — \
             `_reserved` = {after:#06x}"
        );
        assert!(
            conforming_note_elision_taken(after),
            "#8115 must not break the legitimate SIDE_MASK | INTACT case — \
             `_reserved` = {after:#06x}"
        );
    }
}

/// The other half of the legitimate case: a store that *contradicts* the
/// descriptor still downgrades through `layout_set_typed_unknown`, exactly as
/// before. #8115 adds a second clear; it must not displace the first.
#[test]
fn a_contradicting_store_still_downgrades_through_the_descriptor_path() {
    unsafe {
        let obj = plant_descriptor_backed_instance(0x8115_0004, b"p\0q\0r\0");

        // Slot 1 is declared raw-f64; a NaN-boxed pointer there contradicts it.
        crate::object::store_object_field_slot(obj, 1, string_bits(young_leaf()));

        let after = reserved_of(obj);
        assert_eq!(
            after & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN,
            "a contradicting store must reach GC_LAYOUT_UNKNOWN"
        );
        assert!(
            !inline_guard_raw_f64_arm_taken(after),
            "the intact bit must be gone with the descriptor it claimed"
        );
        // The SHARED `SHAPE_LAYOUTS` entry deliberately survives — it still
        // describes every sibling that has not diverged, and `layout.rs`'s
        // `with_shape_shared_descriptor` doc calls the INTACT gate on that half
        // load-bearing for exactly this reason. So `layout_descriptor_reachable`
        // still answers `true` here; what changed is that THIS object no longer
        // claims it.
        assert!(
            layout_descriptor_reachable(obj as usize),
            "premise: the shared entry survives one object's divergence"
        );
    }
}

/// The bake's own fast path must stay free: a plain double into an in-range
/// slot of a baked instance returns at `layout_note_slot`'s raw-f64 arm, above
/// the #8115 clear, so the licence survives. Without this, "clear the bit"
/// could be implemented as "clear it on every store" and no other test here
/// would notice.
#[test]
fn a_conforming_raw_f64_store_leaves_the_bake_intact() {
    unsafe {
        let obj = plant_baked_instance(0x8115_0005, b"u\0v\0w\0");

        crate::object::store_object_field_slot(obj, 0, 3.5f64.to_bits());
        crate::object::store_object_field_slot(obj, 1, (-7.25f64).to_bits());

        let after = reserved_of(obj);
        assert_eq!(
            after & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_POINTER_FREE,
            "a pointer-free payload must stay POINTER_FREE"
        );
        assert!(
            inline_guard_raw_f64_arm_taken(after),
            "#7834's bake must keep its licence while nothing contradicts it — \
             `_reserved` = {after:#06x}"
        );
        assert_eq!(
            read_field_the_way_codegen_would(obj, 0),
            FieldRead::RawDouble(3.5),
            "the fast arm must still be taken, and read the right double"
        );
    }
}

/// Why the residual never surfaced as a wrong answer from TypeScript, and why
/// that was not a defence worth keeping.
///
/// Every by-name store — which is where a pointer into a statically-`number`
/// slot actually arrives, because the inline store arm refuses a non-finite
/// value for a raw-f64 field — calls `mark_object_dynamic_shape_unknown` first.
/// Its guard is
///
/// ```text
/// state != GC_LAYOUT_SIDE_MASK && !layout_has_typed_descriptor(obj)  ->  return
/// ```
///
/// and #8115's issue read the second conjunct as true for a baked instance
/// ("no typed descriptor"). It is **false**: `layout_has_typed_descriptor`
/// answers by reading `GC_OBJ_TYPED_LAYOUT_INTACT` — the bit the bake set — so
/// the guard does not fire and `layout_mark_unknown` heals the object. The
/// stale claim was its own antidote.
///
/// This test pins that coupling so it cannot be dissolved silently: making
/// `layout_has_typed_descriptor` honest (probing the maps) would turn the
/// early return back on. After #8115 nothing depends on it —
/// `a_descriptorless_bake_drops_its_intact_claim_on_the_generic_downgrade`
/// above heals through `layout_note_slot` alone, with
/// `mark_object_dynamic_shape_unknown` never called.
#[test]
fn the_bake_healed_itself_only_because_the_descriptor_probe_reads_the_same_bit() {
    unsafe {
        let obj = plant_baked_instance(0x8115_0006, b"m\0n\0o\0");

        // The two questions disagree. That disagreement IS the #8115 state.
        assert!(
            crate::gc::layout_has_typed_descriptor(obj as usize),
            "premise: the O(1) probe answers from the bit, so the bake makes it \
             say yes"
        );
        assert!(
            !layout_descriptor_reachable(obj as usize),
            "premise: no descriptor is actually reachable"
        );

        crate::object::mark_object_dynamic_shape_unknown(obj);

        let after = reserved_of(obj);
        assert_eq!(
            after & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN,
            "the guard must NOT have early-returned — if it did, a baked \
             instance would keep a licence no store had earned"
        );
        assert!(
            !inline_guard_raw_f64_arm_taken(after),
            "`layout_mark_unknown` clears the bit on the way through"
        );
    }
}
