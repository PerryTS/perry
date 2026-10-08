//! Array/closure pointer-slot states, store maintenance and child-slot
//! enumeration. Object children are selected by ShapeId. Shape selection is in
//! `layout/slot_mask.rs`; the relocation funnel every moving-GC and growth
//! path calls is in `layout/transfer.rs`.

use super::*;
// Copied-nursery survival age in otherwise-unused low `_reserved` bits;
// bits 0..2 remain object flags and bits 14..15 remain layout state.
pub(super) const GC_COPY_SURVIVAL_AGE_SHIFT: usize = 3;
pub(super) const GC_COPY_SURVIVAL_AGE_MASK: u16 = 0x0038;
pub(super) const GC_COPY_PROMOTION_SURVIVALS: u8 = 4;

// Array/closure pointer-slot layout state in high `_reserved` bits.
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
    if (*header).obj_type == GC_TYPE_OBJECT {
        return;
    }
    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
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
/// installed a mixed layout declaration, so for a non-empty literal it was a silent
/// no-op and every later `push` lost #7469's elided store.
///
/// This predicate discharges the claim instead of assuming it. Every slot must
/// be pointer-bearing by [`layout_pointer_bearing_bits`] — the same test the
/// bulk classifier and `GC_LAYOUT_UNKNOWN`'s per-slot re-validation apply — so the
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

/// Settle a FRESH closure/object whose every payload slot is a word the
/// tag-checked scan understands (NaN-boxed values, raw heap pointers, 0) into
/// `GC_LAYOUT_UNKNOWN` — the #7630 state for a payload a pointer mask cannot
/// improve on. The caller has written the slots directly and owns the
/// barrier (`runtime_write_barrier_newborn_slots`). A fresh birth has no
/// representation feedback to retire.
pub(crate) unsafe fn layout_init_unknown_fresh(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    debug_assert_eq!(
        (*header)._reserved & GC_LAYOUT_STATE_MASK,
        GC_LAYOUT_POINTER_FREE,
        "layout_init_unknown_fresh is for a fresh pointer-free birth only"
    );
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
    if (*header).obj_type == GC_TYPE_OBJECT {
        return;
    }
    let state = (*header)._reserved & GC_LAYOUT_STATE_MASK;
    if state == GC_LAYOUT_UNKNOWN {
        return;
    }
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
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
    if user_ptr >= GC_HEADER_SIZE + 0x1000 {
        unsafe {
            (*header_from_user_ptr(user_ptr as *const u8))._reserved &= !GC_LAYOUT_ALL_POINTERS;
        }
    }
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
        }
        if (*header).obj_type == GC_TYPE_OBJECT {
            // Object stores retire the numeric proof in their owner funnel.
            // GC tracing uses the ShapeId rep, never an address-keyed mask.
            return;
        }
        if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_UNKNOWN {
            return;
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
            // Generated layout notes also reach this funnel directly. A
            // pointer invalidates both numeric sub-flags before publication,
            // even when no array store helper has cleared them yet.
            crate::array::clear_array_numeric_layout_ptr(parent_user);
        }
        if !pointer && (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
            return;
        }
        // Mixed payloads use the same tag test as the tracer. A store can
        // only weaken this declaration; explicit bulk scans may strengthen it.
        layout_mark_unknown(parent_user as *mut u8);
    }
}

/// Existing-slot note. Stable scalar or pointer classification needs no kind
/// transition; pointer overwrites still maintain Array element-shape metadata.
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

/// Value-aware ABI used by generated stores until S3 simplifies the funnel.
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

pub(crate) unsafe fn layout_rebuild_from_slots(
    user_ptr: *mut u8,
    slots: *const u64,
    slot_count: usize,
) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    if (*header).obj_type == GC_TYPE_OBJECT {
        return;
    }
    // A bulk scan can strengthen the payload kind without changing numeric facts.
    if slots.is_null() || slot_count == 0 {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        return;
    }

    let any_pointer = (0..slot_count).any(|i| layout_pointer_bearing_bits(*slots.add(i)));
    set_layout_state(
        header,
        if any_pointer {
            GC_LAYOUT_UNKNOWN
        } else {
            GC_LAYOUT_POINTER_FREE
        },
    );
}

/// Layout for a NEWBORN whose slots were just bulk-initialized: the same
/// classification as [`layout_rebuild_from_slots`] (pointer-free / tag scan),
/// with no address-keyed record and no per-slot layout notes (each of
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
    if slots.is_null() || slot_count == 0 {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        return false;
    }
    let any_pointer = (0..slot_count).any(|i| layout_pointer_bearing_bits(*slots.add(i)));
    set_layout_state(
        header,
        if any_pointer {
            GC_LAYOUT_UNKNOWN
        } else {
            GC_LAYOUT_POINTER_FREE
        },
    );
    any_pointer
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
                // Only the header ALL_POINTERS state is precise for arrays
                // and closures; mixed payloads are scanned by tags.
                false
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

/// ABI compatibility until S3 removes the generated declaration.
#[no_mangle]
pub extern "C" fn js_gc_forget_object_layout(_obj: u64) {}
