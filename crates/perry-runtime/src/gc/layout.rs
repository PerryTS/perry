//! Per-object pointer-slot states, store maintenance and child-slot
//! enumeration. Mask storage is in `layout/slot_mask.rs`; typed descriptor
//! installation is in `layout/typed_shape.rs`; the relocation funnel every
//! moving-GC and growth path calls is in `layout/transfer.rs`.

use super::hot_tls::hot_layout_slot_masks;
use super::layout_tables::{
    layout_forget_object, layout_note_store_mask_insert, mark_per_object_layouts_nonempty,
    per_object_slot_mask, refresh_per_object_layouts_flag, slot_masks_insert_birth,
    slot_masks_insert_rebuild, slot_masks_remove, transfer_per_object_slot_mask,
};
use super::*;
// Copied-nursery survival age in otherwise-unused low `_reserved` bits;
// bits 0..2 remain object flags and bits 14..15 remain layout state.
pub(super) const GC_COPY_SURVIVAL_AGE_SHIFT: usize = 3;
pub(super) const GC_COPY_SURVIVAL_AGE_MASK: u16 = 0x0038;
pub(super) const GC_COPY_PROMOTION_SURVIVALS: u8 = 4;

// Pointer-slot layout state in high `_reserved` bits; low bits remain object flags.
pub const GC_LAYOUT_STATE_MASK: u16 = 0xC000;
pub(super) const GC_LAYOUT_UNKNOWN: u16 = 0x0000;
/// No payload slot holds a pointer, so `heap_payload_slot_selection` skips the
/// WHOLE payload without consulting any mask. This is the one layout state that
/// is not a precision hint: marking, the evacuation rewrite and the
/// remembered-set dirty scan all funnel through that same enumeration, so an
/// object left here while holding a heap pointer loses that child outright — it
/// is neither kept alive nor rewritten when it moves.
///
/// **How to verify a change to this state (#7635).** The end-to-end knobs do
/// catch a misdeclaration, but only if the workload actually holds a misdeclared
/// object across a collection, and it is easy to build one that never does:
/// #7635 forced every JSON-parsed record to `POINTER_FREE` while it held heap
/// strings and got byte-identical correct output under `PERRY_GC_SCHEDULE_RATE=1
/// PERRY_GC_PROTECT_FROMSPACE=1` and `PERRY_GC_FORCE_EVACUATE=1`, because
/// `js_json_parse` is LAZY for 1 KB–16 MB top-level arrays (`json_tape`) and the
/// probe read its records only after the last GC. Under `PERRY_JSON_TAPE=0` the
/// same sabotage SIGSEGVs. So:
///
/// Therefore first prove the object existed during collection; prefer
/// `PERRY_GC_FROMSPACE_SCAN=1`, whose whole-payload scan ignores layout state.
/// `PERRY_GC_VERIFY_EVACUATION` is blind because it trusts this enumeration.
/// Workload-free coverage lives in the child-slot and copying-relocation tests
/// in `gc/tests/copying/deferred_finalize_7635.rs`.
// The complete `GcHeader::_reserved` bit map — BOTH this namespace and
// `gc::types`' `OBJ_FLAG_*` — is on `OBJ_FLAG_RESERVED_BIT_MAP_SEE_DOC` in
// `gc/types.rs`. The constants below own bits 12, 13, 14 and 15; that file's
// flag block used to claim 12..13 were free, and #10842 shipped a mark on
// bit 13 that `set_layout_state` erases. Do not add a flag to either namespace
// without reading that table.
pub const GC_LAYOUT_POINTER_FREE: u16 = 0x4000;
pub(crate) const GC_LAYOUT_SIDE_MASK: u16 = 0x8000;
// A side-layout payload whose entire live prefix contains pointers. Bit 13 is
// independent from the two high state bits and travels with `_reserved` when
// copying GC moves the object, avoiding a per-array side-table entry.
pub(crate) const GC_LAYOUT_ALL_POINTERS: u16 = 0x2000;

// #5093: bit 12 of `GcHeader._reserved`. For an ARRAY it is the typed-literal
// layout claim (`array::header_gc_slots`). Charter step 5: no object carries a
// typed layout descriptor any more (the shape record's `rep` word is the
// authority), and nothing sets this bit on an object; `layout_note_slot` clears
// a stale one.
pub const GC_OBJ_TYPED_LAYOUT_INTACT: u16 = 0x1000;

#[inline]
pub(super) unsafe fn header_clear_typed_layout_intact(header: *mut GcHeader) {
    (*header)._reserved &= !GC_OBJ_TYPED_LAYOUT_INTACT;
}

// Clear the intact bit given only a user pointer (looks the header up). Used by
// the one remove path (`layout_clear_for_ptr`) that doesn't already hold a
// header. No-op for addresses too low to carry a Gc header.
#[inline]
pub(super) fn clear_typed_layout_intact_for_user(user_ptr: usize) {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return;
    }
    unsafe {
        let header = header_from_user_ptr(user_ptr as *const u8);
        (*header)._reserved &= !GC_OBJ_TYPED_LAYOUT_INTACT;
    }
}

mod by_shape;
mod child_slots;
mod slot_mask;
#[cfg(test)]
mod test_accessors;
mod transfer;

pub(crate) use child_slots::*;
pub(in crate::gc) use slot_mask::LayoutSlotMask;
#[cfg(test)]
pub(crate) use test_accessors::{
    test_gc_rewrite_slot_addresses, test_gc_rewrite_slot_count, test_layout_pointer_slot_count,
};
#[cfg(test)]
pub(in crate::gc) use test_accessors::{test_reset_trace_slot_reads, test_trace_slot_reads};
pub(crate) use transfer::layout_transfer;

// NaN-boxing tag constants (duplicated from value.rs to avoid circular deps)

#[cfg(test)]
thread_local! {
    pub(super) static TRACE_SLOT_READS: Cell<usize> = const { Cell::new(0) };
}

pub(super) unsafe fn header_from_user_ptr(user_ptr: *const u8) -> *mut GcHeader {
    (user_ptr as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader
}

#[inline]
pub(super) unsafe fn set_layout_state(header: *mut GcHeader, state: u16) {
    (*header)._reserved = ((*header)._reserved & !(GC_LAYOUT_STATE_MASK | GC_LAYOUT_ALL_POINTERS))
        | (state & GC_LAYOUT_STATE_MASK);
}

#[inline]
pub(super) fn copied_survival_age(reserved: u16, flags: u8) -> u8 {
    if flags & GC_FLAG_TENURED != 0 {
        return GC_COPY_PROMOTION_SURVIVALS;
    }
    let encoded = ((reserved & GC_COPY_SURVIVAL_AGE_MASK) >> GC_COPY_SURVIVAL_AGE_SHIFT) as u8;
    if encoded != 0 {
        return encoded;
    }
    if flags & GC_FLAG_HAS_SURVIVED != 0 {
        1
    } else {
        0
    }
}

#[inline]
pub(super) fn reserved_with_copied_survival_age(reserved: u16, age: u8) -> u16 {
    let capped = age.min(7) as u16;
    (reserved & !GC_COPY_SURVIVAL_AGE_MASK) | (capped << GC_COPY_SURVIVAL_AGE_SHIFT)
}

/// Stamp a header the way `move_young`'s promoting arm stamps its to-space
/// copy — except that whole-block promotion (#7742) has no copy, so the SAME
/// header is aged in place.
///
/// Both halves matter. `GC_FLAG_TENURED` upholds the `Old ⟹ TENURED`
/// invariant the generated write barrier's fast path is gated on (#7511):
/// without it, a store into a promoted object would skip the remembering call
/// entirely and its young child would be swept alive. Clearing
/// `GC_FLAG_HAS_SURVIVED` and pinning the survival age to
/// `GC_COPY_PROMOTION_SURVIVALS` keeps `copied_survival_age` reading the same
/// value it would have read off an evacuated copy, so nothing downstream can
/// tell a promoted-in-place object from a promoted-by-copy one.
///
/// # Safety
/// `header` must point at a live `GcHeader` inside a block that is being
/// promoted to old-gen this cycle.
#[inline]
pub(crate) unsafe fn stamp_header_promoted_in_place(header: *mut GcHeader) {
    let flags = (*header).gc_flags;
    (*header).gc_flags = (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED;
    (*header)._reserved =
        reserved_with_copied_survival_age((*header)._reserved, GC_COPY_PROMOTION_SURVIVALS);
}

#[inline]
pub(super) fn strip_nanbox_user_ptr(bits: u64) -> usize {
    if (bits >> 48) >= 0x7FF8 {
        (bits & POINTER_MASK) as usize
    } else {
        bits as usize
    }
}

#[inline]
pub(crate) fn layout_pointer_bearing_bits(bits: u64) -> bool {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        return bits & POINTER_MASK != 0;
    }
    if tag >= 0x7FF8_0000_0000_0000 {
        return false;
    }
    (0x1000..=POINTER_MASK).contains(&bits) && (bits & 0x7) == 0
}

#[inline]
pub(super) unsafe fn layout_header_for_user(user_ptr: usize) -> Option<*mut GcHeader> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    match gc_type_layout_slot_kind((*header).obj_type) {
        GcLayoutSlotKind::ArrayElements
        | GcLayoutSlotKind::ObjectFields
        | GcLayoutSlotKind::ClosureCaptures => Some(header),
        // #6812: meta records keep no layout mask — their two child slots
        // (prototype, spill) are enumerated unconditionally.
        GcLayoutSlotKind::None | GcLayoutSlotKind::ObjectMeta | GcLayoutSlotKind::RegExpFields => {
            None
        }
    }
}

/// True when a traced object provably yields NO child slot to any collector
/// walk, so the walk can be SKIPPED rather than performed and found empty. On a
/// chain-node heap half the traced objects are of this shape.
///
/// This is a claim about four independent edge sources, and every one of them
/// needs its own term. `GC_LAYOUT_POINTER_FREE` alone is NOT enough, because it
/// describes the PAYLOAD and nothing else:
///
/// * **the payload** — `GC_LAYOUT_POINTER_FREE`, which
///   `heap_payload_slot_selection` already trusts to skip the whole payload
///   without consulting a mask;
/// * **the kind's prefix and meta edges** — the reason for the kind term, and
///   the reason it comes first. `gc_child_slots` builds `ArrayElements` as
///   `new(header, None, range)`: no prefix, no meta, no meta2. Every other
///   layout kind carries at least one. `ObjectFields` carries the meta record,
///   which #6812 records as "fatal for the spill buffer, reachable through meta
///   alone"; `RegExpFields` and `ObjectMeta` carry a prefix and two meta edges
///   each. And POINTER_FREE is emphatically not an array-only bit: a closure is
///   ALLOCATED pointer-free (`symbol/properties.rs`, #7154) and only leaves that
///   state when a capture store records a pointer, and a typed object whose
///   shape has an empty pointer mask acquires it (`gc/layout/typed_shape.rs`).
///   Skipping either would drop edges the payload bit says nothing about — the
///   closure's dynamic property values and static `.prototype`, the object's
///   meta record, shape `keys` edge and overflow fields;
/// * **the array's named-property reserve slots** — `GC_ARRAY_NAMED_PROPS`,
///   which live in front of element 0, outside every layout range;
/// * **a residual `Object.setPrototypeOf` entry** — the per-owner header bit
///   from #10611, which is what makes this affordable to ask per object.
///
/// A FORWARDED header is never skippable, whatever its layout: array growth
/// installs PERMANENT forwarding stubs, and walking the stub is what propagates
/// liveness across the hop (#6228). The same guard on the sibling leaf skip in
/// `gc/trace.rs` is there for this reason.
///
/// NOT SUFFICIENT ON ITS OWN FOR THE FULL MARK. `gc/trace.rs` reads every word
/// of a pointer-free payload through `proxy::gc_observe_traced_value` when a
/// proxy trace is active, because a proxy id is a `POINTER_TAG` value in the
/// proxy-id band rather than a heap pointer — which is precisely why the layout
/// mask is entitled to call a payload holding one pointer free. The full mark's
/// call site therefore ANDs in `!proxy_trace_active`; the copying minor and the
/// remembered-set rebuild both ignore `PointerFreeRange` and need no such term.
#[inline]
pub(crate) unsafe fn gc_object_yields_no_child_slots(header: *const GcHeader) -> bool {
    // ORDER IS LOAD-BEARING, and it is a measurement, not a preference. Every
    // object the copying minor moves asks this, and most say no; a first
    // version that asked the type table first cost +0.07% to +0.12% on the
    // three fixtures where almost nothing qualifies. The three header-word
    // terms fold into ONE mask compare on a word `move_young` has already
    // loaded, so a non-candidate is rejected in two instructions.
    let reserved = (*header)._reserved;
    if reserved
        & (GC_LAYOUT_STATE_MASK
            | crate::gc::GC_ARRAY_NAMED_PROPS
            | crate::gc::GC_RESIDUAL_PROTO_OWNER)
        != GC_LAYOUT_POINTER_FREE
    {
        return false;
    }
    if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return false;
    }
    // Keyed on the TYPE rather than the rewrite kind, so the surviving path is
    // a byte compare instead of a table load. This is conservative in the safe
    // direction: a future type that also had no prefix/meta edge and no
    // uncovered sibling would simply not be admitted here, costing a walk it
    // could have skipped. `the_array_type_still_pairs_with_the_prefix_free_
    // layout_kind` pins the two table facts this leans on.
    (*header).obj_type == crate::gc::GC_TYPE_ARRAY
}

#[inline]
pub(crate) unsafe fn layout_init_pointer_free(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
    layout_forget_object(user_ptr as usize);
    header_clear_typed_layout_intact(header);
}

/// Declare that every currently-live slot of a fresh array-like payload holds
/// a pointer. Callers must keep `length` at the initialized prefix while the
/// payload is being filled; the header flag then remains precise across any GC
/// that runs between element allocations.
#[inline]
pub(crate) unsafe fn layout_init_all_pointer_slots(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    header_clear_typed_layout_intact(header);
    layout_forget_object(user_ptr as usize);
    set_layout_state(header, GC_LAYOUT_SIDE_MASK);
    (*header)._reserved |= GC_LAYOUT_ALL_POINTERS;
}

/// Does the all-pointer claim actually hold for a payload that **already holds
/// initialized slots** — the non-empty array literal `const a: C[] = [x, y]`?
///
/// [`layout_init_all_pointer_slots`]' array caller
/// (`js_array_declare_all_pointer_elements`) used to refuse every non-empty
/// array outright, because the claim covers `0..length` and only an empty array
/// makes it vacuously true. #8102 is what that cost: the declaration is emitted
/// from the `Stmt::Let` tail, i.e. *after* a literal's element stores have
/// installed a per-slot side mask, so for a non-empty literal it was a silent
/// no-op and every later `push` lost #7469's elided store.
///
/// This predicate discharges the claim instead of assuming it. Every slot must
/// be pointer-bearing by [`layout_pointer_bearing_bits`] — the same test the
/// mask builder and `GC_LAYOUT_UNKNOWN`'s per-slot re-validation apply — so the
/// declaration never has to trust a caller's static proof. `slot_count == 0` is
/// the empty case and holds vacuously, which keeps the pre-#8102 path
/// bit-identical.
#[inline]
pub(crate) unsafe fn layout_all_pointer_slots_would_hold(
    slots: *const u64,
    slot_count: usize,
) -> bool {
    if slot_count == 0 {
        return true;
    }
    if slots.is_null() {
        return false;
    }
    (0..slot_count).all(|i| layout_pointer_bearing_bits(*slots.add(i)))
}

/// #7630: settle a materialiser-built object's layout state ONCE, after its
/// construction loop elided the per-slot notes
/// (`runtime_store_jsvalue_slot_layout_deferred`). Two exact outcomes:
///
/// - **No pointer was stored**: the `layout_init_pointer_free` birth state is
///   still the truth, and it is the valuable one — the tracer skips the whole
///   payload. Nothing to do.
/// - **Any pointer was stored**: `GC_LAYOUT_UNKNOWN`, the tag-checked
///   scan-all-slots state. For a cohort whose every slot is a NaN-boxed
///   `JSValue`, a pointer mask can never skip anything a tag check would not
///   reject anyway — the mask machinery (per-object side-table entry, hashmap
///   round-trip per store, `layout_transfer` per promotion,
///   `layout_forget_object` per death) buys nothing here. Routed through
///   `layout_mark_unknown`, not a bare state store, so a mask that a
///   slow-path by-name store DID create mid-construction (shape-overflow
///   records) is removed with the state change rather than stranded.
pub(crate) unsafe fn layout_finish_deferred_boxed_object(user_ptr: usize, saw_pointer: bool) {
    if !saw_pointer {
        return;
    }
    layout_mark_unknown(user_ptr as *mut u8);
}

/// Settle a FRESH closure/object whose every payload slot is a word the
/// tag-checked scan understands (NaN-boxed values, raw heap pointers, 0) into
/// `GC_LAYOUT_UNKNOWN` — the #7630 state for a payload a pointer mask cannot
/// improve on. The caller has written the slots directly and owns the
/// barrier (`runtime_write_barrier_newborn_slots`). Unlike
/// [`layout_mark_unknown`] this touches no side table: a fresh birth whose
/// state is still `GC_LAYOUT_POINTER_FREE` has no mask, no typed descriptor
/// and no representation feedback to retire.
pub(crate) unsafe fn layout_init_unknown_fresh(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    debug_assert_eq!(
        (*header)._reserved & GC_LAYOUT_STATE_MASK,
        GC_LAYOUT_POINTER_FREE,
        "layout_init_unknown_fresh is for a fresh pointer-free birth only"
    );
    header_clear_typed_layout_intact(header);
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
}

/// A header-level child edge was just installed on `user_ptr` (a closure's
/// own-property bag): a `GC_LAYOUT_POINTER_FREE` payload state must not let
/// any walk treat the cell as edge-free. Other states already visit it.
pub(crate) unsafe fn layout_note_closure_edge_installed(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
        layout_mark_unknown(user_ptr);
    }
}

pub(crate) unsafe fn layout_mark_unknown(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    header_clear_typed_layout_intact(header);
    let state = (*header)._reserved & GC_LAYOUT_STATE_MASK;
    if state == GC_LAYOUT_UNKNOWN {
        layout_forget_object(user_ptr as usize);
        return;
    }
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    if state == GC_LAYOUT_POINTER_FREE {
        crate::typed_feedback::invalidate_representation_change(user_ptr as usize);
        return;
    }
    slot_masks_remove(user_ptr as usize);
    crate::typed_feedback::invalidate_representation_change(user_ptr as usize);
}

pub(crate) fn layout_clear_for_ptr(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    crate::array::clear_array_numeric_layout_ptr(user_ptr);
    // #7480: this runs on object death / address recycle, so drop the record
    // outright rather than only clearing the bit.
    crate::array::forget_element_shape(user_ptr);
    layout_forget_object(user_ptr);
    clear_typed_layout_intact_for_user(user_ptr);
    if user_ptr >= GC_HEADER_SIZE + 0x1000 {
        unsafe {
            (*header_from_user_ptr(user_ptr as *const u8))._reserved &= !GC_LAYOUT_ALL_POINTERS;
        }
    }
}

/// Reads the `GC_OBJ_TYPED_LAYOUT_INTACT` header bit. Charter step 5: nothing
/// sets it on an object any more, so for an object this answers `false`; the
/// object-side callers go with the object layout-state bits.
pub(crate) fn layout_has_typed_descriptor(user_ptr: usize) -> bool {
    layout_typed_intact_for_user(user_ptr)
}

/// True when `slot_index` is the **append position** of an array whose live
/// prefix is currently declared all-pointer.
///
/// Every array append protocol in the tree — `js_array_push_f64`,
/// `js_array_push_f64_grow`, and the codegen-inlined push — writes the element
/// slot and notes it BEFORE bumping `length`, so an append records
/// `slot_index == length`. Writing a pointer there keeps
/// "every slot in `0..length + 1` holds a pointer" exactly true, which is the
/// whole content of [`GC_LAYOUT_ALL_POINTERS`]; a replace (`slot < length`) or
/// a hole-creating jump (`slot > length`) does not, and downgrades.
///
/// Restricted to `GC_TYPE_ARRAY` on purpose: object fields and closure
/// captures have a FIXED live prefix (`field_count` / `capture_count`), so
/// they have no append position at all and nothing to preserve. `length <
/// capacity` keeps the claim inside the allocation.
#[inline]
unsafe fn layout_all_pointer_array_append(
    header: *const GcHeader,
    parent_user: usize,
    slot_index: usize,
) -> bool {
    if (*header).obj_type != GC_TYPE_ARRAY {
        return false;
    }
    let arr = parent_user as *const crate::array::ArrayHeader;
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    slot_index == length && length < capacity
}

pub(crate) fn layout_note_slot(parent_user: usize, slot_index: usize, value_bits: u64) {
    #[cfg(test)]
    crate::object::TEST_LAYOUT_NOTE_SLOT_CALLS.with(|calls| calls.set(calls.get() + 1));
    if slot_index > 16_000_000 {
        return;
    }
    unsafe {
        let Some(header) = layout_header_for_user(parent_user) else {
            return;
        };
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            let new_user = forwarding_address(header) as usize;
            if new_user != 0 && new_user != parent_user {
                layout_note_slot(new_user, slot_index, value_bits);
            }
            return;
        }
        // #7480: maintain the per-array homogeneous element-shape invariant.
        // This is the one funnel BOTH the runtime's element-store helpers
        // (`note_array_slot` and siblings) and codegen's inline element
        // stores already pass through — `array_store_needs_layout_note`
        // elides the note only for an array statically proven numeric and
        // pointer-free, which an element-shape array can never be. It sits
        // ahead of the `GC_LAYOUT_UNKNOWN` early return below because an
        // all-pointer array is marked unknown on its first generic write,
        // and costs one `obj_type` compare on the header word the next line
        // reads anyway.
        if (*header).obj_type == GC_TYPE_ARRAY {
            crate::array::note_element_store(
                parent_user as *mut crate::array::ArrayHeader,
                slot_index,
                value_bits,
            );
        } else if (*header).obj_type == GC_TYPE_OBJECT
            && (*header)._reserved & OBJ_FLAG_PACKED_NUMERIC_PROOF != 0
        {
            // The proof is a shape fact. Retire it on an owner write before
            // leaving the object arm; object-owned spill writes use their
            // matching owner hook because their physical note names an Array.
            crate::array::clear_packed_subclass_numeric_proof(
                parent_user as *mut crate::object::ObjectHeader,
            );
        }
        if (*header).obj_type == GC_TYPE_OBJECT {
            // The collector selects object fields from their shape's rep
            // word, so an object store never needs a per-address pointer
            // mask or a layout-state transition. Keep this entry until the
            // proof retirement is moved into the object store funnel.
            header_clear_typed_layout_intact(header);
            return;
        }
        if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_UNKNOWN {
            return;
        }
        // Charter step 5: no object carries a typed layout descriptor (the
        // shape record's `rep` word is the authority), so a set
        // `GC_OBJ_TYPED_LAYOUT_INTACT` here has nothing behind it: clear it
        // before the pointer-mask path below (`set_layout_state` masks
        // `!0xE000` and does not touch this `0x1000` bit).
        let claimed_intact = (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT != 0;
        if claimed_intact {
            header_clear_typed_layout_intact(header);
        }
        let pointer = layout_pointer_bearing_bits(value_bits);
        // A result array built by a runtime helper can declare that its live
        // prefix is pointer-only once, instead of growing a HashMap-backed
        // bitmap for every inserted element. Runtime construction bypasses
        // this generic write path; any later ordinary array write may create
        // holes or replace an element, so conservatively fall back to the
        // generic scan path regardless of the stored value.
        //
        // ONE exception (#7469): an APPEND of a pointer at the array's current
        // append position keeps the declaration exact rather than violating it,
        // so it is preserved instead of downgraded. Without this a codegen
        // `[]` + push-loop array (declared all-pointer at allocation) would be
        // demoted by the very first growth — `js_array_push_f64_grow` routes
        // through this function — and every later push would fall off the
        // declared fast path for the rest of the array's life.
        let all_pointer_layout = (*header)._reserved & GC_LAYOUT_ALL_POINTERS != 0;
        if all_pointer_layout {
            if pointer && layout_all_pointer_array_append(header, parent_user, slot_index) {
                return;
            }
            layout_mark_unknown(parent_user as *mut u8);
            return;
        }
        // An empty array's first pointer append is also a complete proof of
        // the all-pointer invariant: before the caller bumps `length` the
        // live prefix is empty, and immediately afterwards its sole element
        // is the pointer we just classified. Publish that stronger state
        // instead of minting a one-bit side mask (or falling back to UNKNOWN),
        // so later pointer appends can consume the same O(1) header proof as
        // arrays declared all-pointer by codegen.
        //
        // This is deliberately restricted to `length == 0`. A POINTER_FREE
        // array with an existing numeric prefix may also receive a pointer at
        // its append position, but that prefix does not satisfy the claim.
        // Clear both raw-f64 flags before publishing ALL_POINTERS: the two
        // representations are mutually exclusive, and generated append code
        // uses their absence as part of its admission test.
        if pointer
            && (*header).obj_type == GC_TYPE_ARRAY
            && (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE
        {
            let arr = parent_user as *const crate::array::ArrayHeader;
            if (*arr).length == 0
                && layout_all_pointer_array_append(header, parent_user, slot_index)
            {
                crate::array::clear_array_numeric_layout_ptr(parent_user);
                layout_init_all_pointer_slots(parent_user as *mut u8);
                return;
            }
        }
        if !pointer && (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
            return;
        }
        // The insert branch below breaks the emptiness the flag asserts, so it
        // arms the flag inline; the removal branch re-tests it afterwards,
        // outside the borrow `refresh_per_object_layouts_flag` would re-enter.
        let mut emptied = false;
        {
            let mut masks = hot_layout_slot_masks().borrow_mut();
            if pointer {
                if let Some(mask) = masks.get_mut(&parent_user) {
                    mask.set_slot(slot_index);
                    // A non-empty pointer mask MUST be reflected by SIDE_MASK
                    // state: `heap_payload_slot_selection` treats POINTER_FREE
                    // as "no pointers" and skips the WHOLE payload without ever
                    // consulting the mask. If a stale POINTER_FREE lingers here
                    // (an array truncated to a numeric/empty prefix flips to
                    // POINTER_FREE while its element mask is retained), recording
                    // a pointer would leave every masked element untraced — the
                    // evacuating minor then reclaims/relocates the child out from
                    // under the live slot, later read+called as a garbage pointer
                    // ("value is not a function"). Recording a pointer proves the
                    // object is not pointer-free, so restore SIDE_MASK.
                    if (*header)._reserved & GC_LAYOUT_STATE_MASK != GC_LAYOUT_SIDE_MASK {
                        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
                    }
                } else if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
                    if super::layout_tables::immortal_layout_scope_active()
                        || super::layout_tables::layout_prefers_scan_over_mask(
                            header,
                            parent_user,
                            slot_index,
                        )
                    {
                        // Two reasons to decline the mask, one fallback. An
                        // object built inside an `ImmortalLayoutScope` is
                        // rooted for the life of the process, so the entry it
                        // would mint here is never removed — and one such
                        // entry disables `PER_OBJECT_LAYOUTS_NONEMPTY` for
                        // every allocation the program will ever make (see
                        // `ImmortalLayoutScope`). And a payload too small for
                        // the mask to earn its side-table entry
                        // (`layout_prefers_scan_over_mask`) skips nothing the
                        // tag-checked scan would not check anyway. Both take
                        // the same `GC_LAYOUT_UNKNOWN` fallback the `else`
                        // arm below uses for this exact situation.
                        set_layout_state(header, GC_LAYOUT_UNKNOWN);
                    } else {
                        let mut mask = LayoutSlotMask::Inline(0);
                        mask.set_slot(slot_index);
                        // The one insert site that holds its own `borrow_mut`,
                        // so it maintains the address filter, the young log
                        // and the young-record count inline too. The log lives
                        // in the hint, not in this map, so arming it here
                        // takes no second borrow — and it goes BEFORE the
                        // insert (`gc/young_log.rs` rule 1). Before #9841 this
                        // site published a young record without counting it;
                        // on cc it is the DOMINANT insert path (`TYPED_LAYOUTS`
                        // is empty there), so it is where a missing arm would
                        // do the most damage.
                        let young = super::layout_tables::arm_young_layout_key(parent_user);
                        masks.insert(parent_user, mask);
                        layout_note_store_mask_insert();
                        mark_per_object_layouts_nonempty();
                        super::layout_tables::layout_addr_filter_note(parent_user);
                        if young {
                            super::layout_tables::count_new_young_layout_record();
                        }
                        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
                    }
                } else {
                    set_layout_state(header, GC_LAYOUT_UNKNOWN);
                }
            } else if let Some(mask) = masks.get_mut(&parent_user) {
                mask.clear_slot(slot_index);
                if mask.is_empty() {
                    masks.remove(&parent_user);
                    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
                    emptied = true;
                }
            }
        }
        refresh_per_object_layouts_flag(emptied);
    }
}

/// Existing-slot layout note with the value that was overwritten.
/// The GC slot mask records whether a slot can carry a heap edge. Replacing one
/// pointer-bearing value with another leaves that bit unchanged. Arrays also
/// maintain a homogeneous element-shape record, so the pointer-over-pointer path
/// runs that hook after validating/chasing the owner header and then stops
/// before the typed-layout and per-slot-mask machinery. Object-backed packed
/// numeric proofs are retired for the same reason as in [`layout_note_slot`].
/// Scalar-over-scalar keeps the historical fast return. A change in either
/// direction uses the complete note so pointer masks and typed descriptors are
/// updated exactly as before.
#[inline]
pub(crate) fn layout_note_slot_aware(
    parent_user: usize,
    slot_index: usize,
    value_bits: u64,
    old_bits: u64,
) {
    let value_is_pointer = layout_pointer_bearing_bits(value_bits);
    let old_is_pointer = layout_pointer_bearing_bits(old_bits);
    if !value_is_pointer && !old_is_pointer {
        return;
    }
    if value_is_pointer && old_is_pointer {
        if slot_index > 16_000_000 {
            return;
        }
        unsafe {
            let Some(header) = layout_header_for_user(parent_user) else {
                return;
            };
            if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
                let new_user = forwarding_address(header) as usize;
                if new_user != 0 && new_user != parent_user {
                    layout_note_slot_aware(new_user, slot_index, value_bits, old_bits);
                }
                return;
            }
            if (*header).obj_type == GC_TYPE_ARRAY {
                crate::array::note_element_store(
                    parent_user as *mut crate::array::ArrayHeader,
                    slot_index,
                    value_bits,
                );
            } else if (*header).obj_type == GC_TYPE_OBJECT
                && (*header)._reserved & OBJ_FLAG_PACKED_NUMERIC_PROOF != 0
            {
                crate::array::clear_packed_subclass_numeric_proof(
                    parent_user as *mut crate::object::ObjectHeader,
                );
            }
        }
        return;
    }
    layout_note_slot(parent_user, slot_index, value_bits);
}

#[no_mangle]
pub extern "C" fn js_gc_note_slot_layout(parent: u64, slot_index: u32, value_bits: u64) {
    let parent_user = strip_nanbox_user_ptr(parent);
    layout_note_slot(parent_user, slot_index as usize, value_bits);
}

/// Value-aware variant of [`js_gc_note_slot_layout`]: `old_bits` is the value
/// previously held in the slot. When old and new have the same heap-pointer
/// classification, the per-slot GC layout mask needs no update. The
/// pointer-over-pointer path still maintains Array element-shape metadata;
/// classification changes retain the full typed-layout and mask pipeline.
/// The mask invariant ("bit set ⟺ slot holds a pointer") is therefore
/// preserved while avoiding the thread-local hashmap on stable overwrites. This is the
/// dominant per-write cost on heterogeneous `any[]` numeric write loops
/// (stubbing `layout_note_slot` makes `bench_numeric_array_downgrade` 11×
/// faster). `layout_pointer_bearing_bits` is the same predicate the layout
/// machinery uses internally, so raw-pointer array slots are classified
/// correctly (not just NaN-boxed tags).
#[no_mangle]
pub extern "C" fn js_gc_note_slot_layout_aware(
    parent: u64,
    slot_index: u32,
    value_bits: u64,
    old_bits: u64,
) {
    let parent_user = strip_nanbox_user_ptr(parent);
    layout_note_slot_aware(parent_user, slot_index as usize, value_bits, old_bits);
}

pub(super) unsafe fn layout_rebuild_from_slots_with_policy(
    user_ptr: *mut u8,
    slots: *const u64,
    slot_count: usize,
    _exact_small_mixed: bool,
) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    // The rebuild reconstructs only the pointer mask (no raw-f64 layout), so the
    // object no longer has a canonical typed descriptor: drop the intact bit.
    header_clear_typed_layout_intact(header);
    if slots.is_null() || slot_count == 0 {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        slot_masks_remove(user_ptr as usize);
        return;
    }

    let mut mask = if slot_count <= 64 {
        LayoutSlotMask::Inline(0)
    } else {
        LayoutSlotMask::Heap(vec![0; slot_count.div_ceil(64)])
    };
    for i in 0..slot_count {
        if layout_pointer_bearing_bits(*slots.add(i)) {
            mask.set_slot(i);
        }
    }

    if mask.is_empty() {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        slot_masks_remove(user_ptr as usize);
    } else if super::layout_tables::immortal_layout_scope_active()
        || slot_count < super::layout_tables::layout_mask_min_slots()
    {
        // Same two reasons as the `layout_note_slot` branch, same fallback. An
        // object built inside an `ImmortalLayoutScope` never dies, so the mask
        // it would install here is a permanent tenant of a side table whose
        // emptiness is a process-wide fast path; and too few slots means the
        // mask cannot earn its side-table entry — the tag-checked scan is
        // exact and costs the program nothing globally. Falling back is sound
        // *for this rebuild specifically* because the mask above is itself
        // derived from `layout_pointer_bearing_bits` — exactly the test
        // `GC_LAYOUT_UNKNOWN` re-runs per slot. (This is why the scope may not
        // be applied to a TYPED descriptor, whose raw-f64 slots the tag test
        // would misread; see `ImmortalLayoutScope`.)
        set_layout_state(header, GC_LAYOUT_UNKNOWN);
        slot_masks_remove(user_ptr as usize);
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        slot_masks_insert_rebuild(user_ptr as usize, mask);
    }
}

/// Layout for a NEWBORN whose slots were just bulk-initialized: the same
/// classification as [`layout_rebuild_from_slots`] (pointer-free / unknown /
/// side mask), but with one `layout_forget_object` up front — a recycled
/// address may carry stale entries — instead of per-table removes interleaved
/// with the rebuild, and no per-slot `layout_note_slot` round trips (each of
/// which re-resolved the header, re-checked forwarding and re-dispatched on the
/// object kind). Callers must treat the object as fully initialized after
/// this returns.
///
/// Returns `true` when at least one slot holds a pointer-bearing value, so
/// the caller can skip the write barrier entirely for a pointer-free birth
/// (the barrier's own child check would reject every slot anyway, after a
/// call and a page classification per slot).
pub(crate) unsafe fn layout_init_from_slots(
    user_ptr: *mut u8,
    slots: *const u64,
    slot_count: usize,
) -> bool {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return true;
    };
    if super::layout_tables::per_object_layouts_maybe_nonempty() {
        layout_forget_object(user_ptr as usize);
    }
    header_clear_typed_layout_intact(header);
    if slots.is_null() || slot_count == 0 {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        return false;
    }
    // Small births (the common case: a handful of captures) classify with a
    // register-resident mask and no heap `Vec`; the min-slots threshold is
    // read once here, not per slot.
    let mut any_pointer = false;
    if slot_count <= 64 {
        let mut bits: u64 = 0;
        for i in 0..slot_count {
            if layout_pointer_bearing_bits(*slots.add(i)) {
                bits |= 1u64 << i;
            }
        }
        if bits == 0 {
            set_layout_state(header, GC_LAYOUT_POINTER_FREE);
            return false;
        }
        any_pointer = true;
        if super::layout_tables::immortal_layout_scope_active()
            || slot_count < super::layout_tables::layout_mask_min_slots()
        {
            set_layout_state(header, GC_LAYOUT_UNKNOWN);
        } else {
            set_layout_state(header, GC_LAYOUT_SIDE_MASK);
            slot_masks_insert_birth(user_ptr as usize, LayoutSlotMask::Inline(bits));
        }
        return any_pointer;
    }
    let mut mask = LayoutSlotMask::Heap(vec![0; slot_count.div_ceil(64)]);
    for i in 0..slot_count {
        if layout_pointer_bearing_bits(*slots.add(i)) {
            mask.set_slot(i);
            any_pointer = true;
        }
    }
    if !any_pointer {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
    } else if super::layout_tables::immortal_layout_scope_active()
        || slot_count < super::layout_tables::layout_mask_min_slots()
    {
        set_layout_state(header, GC_LAYOUT_UNKNOWN);
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        slot_masks_insert_birth(user_ptr as usize, mask);
    }
    any_pointer
}

pub(crate) unsafe fn layout_rebuild_from_slots(
    user_ptr: *mut u8,
    slots: *const u64,
    slot_count: usize,
) {
    layout_rebuild_from_slots_with_policy(user_ptr, slots, slot_count, false);
}

pub(crate) unsafe fn layout_rebuild_exact_from_slots(
    user_ptr: *mut u8,
    slots: *const u64,
    slot_count: usize,
) {
    layout_rebuild_from_slots_with_policy(user_ptr, slots, slot_count, true);
}

pub(super) fn layout_visit_pointer_slots<F: FnMut(usize)>(
    user_ptr: usize,
    slot_count: usize,
    mut visit: F,
) -> bool {
    unsafe {
        let Some(header) = layout_header_for_user(user_ptr) else {
            return false;
        };
        match (*header)._reserved & GC_LAYOUT_STATE_MASK {
            GC_LAYOUT_POINTER_FREE => true,
            GC_LAYOUT_SIDE_MASK => {
                if (*header)._reserved & GC_LAYOUT_ALL_POINTERS != 0 {
                    for slot in 0..slot_count {
                        visit(slot);
                    }
                    return true;
                }
                let mask = per_object_slot_mask(user_ptr);
                let Some(mask) = mask else {
                    set_layout_state(header, GC_LAYOUT_UNKNOWN);
                    return false;
                };
                mask.visit_slots(slot_count, &mut visit);
                true
            }
            _ => false,
        }
    }
}

pub(crate) fn layout_visit_pointer_slots_for_user<F: FnMut(usize)>(
    user_ptr: usize,
    slot_count: usize,
    visit: F,
) -> bool {
    layout_visit_pointer_slots(user_ptr, slot_count, visit)
}

/// #5093: read the per-object "typed shape layout intact" bit. This is the same
/// bit the old typed-layout protocol tests read.
pub(crate) fn layout_typed_intact_for_user(user_ptr: usize) -> bool {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(user_ptr as *const u8);
        (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT != 0
    }
}
