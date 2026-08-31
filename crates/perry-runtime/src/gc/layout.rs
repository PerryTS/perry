//! Per-object pointer-slot layout: the `GcHeader._reserved` layout states,
//! store-time descriptor maintenance (`layout_note_slot`), rebuild/transfer
//! across copying GC, and the child-slot enumeration the collector walks.
//! The slot-mask representation lives in `layout/slot_mask.rs`; the
//! typed-shape descriptor *installation* protocol (`js_gc_init_typed_shape_layout`
//! / `js_gc_declare_typed_shape_layout`) lives in `layout/typed_shape.rs`.

use super::hot_tls::{hot_layout_slot_masks, hot_shape_layouts};
use super::layout_tables::{
    layout_forget_object, mark_per_object_layouts_nonempty, per_object_slot_mask,
    refresh_per_object_layouts_flag, slot_masks_insert, slot_masks_remove,
    transfer_per_object_descriptor, transfer_per_object_slot_mask, typed_layouts_insert,
    typed_layouts_remove, with_per_object_descriptor,
};
use super::*;

// Copied-nursery survival age stored in otherwise-unused low
// GcHeader._reserved bits. Bits 0..2 remain object freeze/seal flags
// and bits 14..15 remain layout state.
pub(super) const GC_COPY_SURVIVAL_AGE_SHIFT: usize = 3;
pub(super) const GC_COPY_SURVIVAL_AGE_MASK: u16 = 0x0038;
pub(super) const GC_COPY_PROMOTION_SURVIVALS: u8 = 4;

// Pointer-slot layout state stored in the high bits of GcHeader._reserved.
// Low bits remain object freeze/seal/preventExtensions flags.
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
/// - "clean at rate 1 + from-space protect" is evidence only once you have
///   shown the misdeclared object EXISTED during a collection;
/// - `PERRY_GC_FROMSPACE_SCAN=1` is the instrument to prefer — its
///   whole-payload word scan consults no layout state, and it reported the
///   stranded children at exactly `dangling=8000 owners=4000`;
/// - `PERRY_GC_VERIFY_EVACUATION` is blind here by construction: it walks the
///   same enumeration the rewrite pass walks, which is to say it asks this
///   state which slots exist.
///
/// The workload-free detectors are the child-slot enumerator and relocation
/// across a copying minor; worked example, sabotage-verified in both
/// directions: `gc/tests/copying/deferred_finalize_7635.rs`.
pub const GC_LAYOUT_POINTER_FREE: u16 = 0x4000;
pub(crate) const GC_LAYOUT_SIDE_MASK: u16 = 0x8000;
// A side-layout payload whose entire live prefix contains pointers. Bit 13 is
// independent from the two high state bits and travels with `_reserved` when
// copying GC moves the object, avoiding a per-array side-table entry.
pub(crate) const GC_LAYOUT_ALL_POINTERS: u16 = 0x2000;

// #5093: per-object "typed shape layout intact" flag, stored in a free bit of
// `GcHeader._reserved` (bit 12; bits 0..11 are object freeze/seal/proto/
// descriptor flags + the copy survival age, bits 14..15 the layout state). Set
// whenever a `TypedLayoutDescriptor` is installed for the object — i.e. its
// canonical raw-f64 / pointer layout is known-valid — and cleared whenever that
// descriptor is removed. Every downgrade routes through `layout_set_typed_unknown`
// or the `layout_*` remove helpers below, all of which clear it, so the invariant
//   intact bit set  ⟹  a canonical typed descriptor exists for this object,
//                      either per-object in `TYPED_LAYOUTS` OR (the #6893 common
//                      case) shared by shape in `SHAPE_LAYOUTS`, keyed by the
//                      object's immutable runtime ShapeId
// holds at all times.
//
// #7834 introduced ONE producer that sets the bit without installing a
// descriptor: the inline `new`'s baked header constant
// (`lower_call/new_alloc.rs`), for a class whose pointer mask is statically
// empty. That is sound at birth — the collector's view of a
// `GC_LAYOUT_POINTER_FREE` payload consults no map — but it made the invariant
// hold only until the first store the descriptor path would have downgraded on.
// #8115 closes that: `layout_note_slot` clears the bit the moment BOTH
// descriptor maps answer `None`, which is the one point where the broken state
// is observable. So the invariant above still holds *for every reader*, with
// the bake as a birth-time exception that self-heals on its first contradicting
// store. (Before #6893 the descriptor was always the per-object
// `TYPED_LAYOUTS` entry; `shape_install_shared` now sets the bit while routing
// same-shape objects through the shared map, so the bit no longer implies a
// per-object entry — only that *some* descriptor is reachable.) The descriptor's
// raw-f64 mask is exactly the compile-time
// canonical mask codegen emits for the class, so combined with a class_id/
// keys_array match the codegen-inlined class-field shape guard can conclude
// "slot K is raw-f64" from this single bit — no cross-crate guard call, no
// thread-local hashmap probe — for any field K the class declares as a raw-f64
// candidate. The bit travels with `_reserved` across copying/evacuating GC (the
// collector copies the whole reserved word), and `layout_transfer` re-syncs it
// defensively after moving the descriptor.
pub const GC_OBJ_TYPED_LAYOUT_INTACT: u16 = 0x1000;

#[inline]
pub(super) unsafe fn header_set_typed_layout_intact(header: *mut GcHeader) {
    (*header)._reserved |= GC_OBJ_TYPED_LAYOUT_INTACT;
}

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

mod slot_mask;
mod typed_shape;

pub(in crate::gc) use slot_mask::LayoutSlotMask;
pub use typed_shape::{
    js_gc_declare_typed_shape_layout, js_gc_init_typed_shape_layout, js_gc_typed_shape_id_for_keys,
};

/// What a single store means for the object's canonical typed descriptor.
/// Computed while the descriptor is still borrowed, acted on after
/// (`layout_set_typed_unknown` borrows the same maps mutably).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotVerdict {
    /// The stored value matches what the descriptor claims for this slot.
    Conforms,
    /// The store contradicts the descriptor — evict it.
    Downgrade,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct TypedLayoutDescriptor {
    pub(super) slot_count: usize,
    pub(super) raw_f64_mask: LayoutSlotMask,
    pub(super) pointer_mask: LayoutSlotMask,
}

// NaN-boxing tag constants (duplicated from value.rs to avoid circular deps)

#[cfg(test)]
thread_local! {
    pub(super) static TRACE_SLOT_READS: Cell<usize> = const { Cell::new(0) };
    static TYPED_SLOT_DESCRIPTOR_PROBES: Cell<usize> = const { Cell::new(0) };
    static TYPED_RAW_F64_DESCRIPTOR_QUERIES: Cell<usize> = const { Cell::new(0) };
}

// #6893: SHAPE-keyed canonical typed layout. Replaces the per-OBJECT
// TYPED_LAYOUTS + LAYOUT_SLOT_MASKS storage for the common case where an
// object's live layout matches its shape (header `GC_OBJ_TYPED_LAYOUT_INTACT`).
// Keyed by the immutable runtime ShapeId stamped on every shaped object, so
// this is O(shapes), not O(objects). Measured: object churn stores a per-object
// descriptor for every one of ~2M `{v,w}` objects (all identical) → ~392 MB;
// keying by their single shared shape collapses that to one entry (churn peak
// RSS 830→262 MB, behaviour-identical).
//
// Value `None` = AMBIGUOUS: two live layouts share the same key NAMES but
// different value TYPES (`{v:1,w:2}` vs `{v:"a",w:"b"}`); those objects fall
// back to the per-object maps. ACCELERATOR ONLY: a miss, an ambiguous shape,
// or a field-count mismatch all fall back to the per-object map and then the
// conservative scan — never a wrong descriptor (mirrors the ShapeTable trust
// model). ShapeIds are process-unique and never recycled, so moving a keys
// array cannot stale this index. Nothing to prune on object death (entries are
// per-shape, shared).
thread_local! {
    pub(in crate::gc) static SHAPE_LAYOUTS: RefCell<crate::fast_hash::PtrHashMap<u32, Option<TypedLayoutDescriptor>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

fn shape_layout_keyed_enabled() -> bool {
    use std::sync::OnceLock;
    static E: OnceLock<bool> = OnceLock::new();
    // Default ON; `PERRY_SHAPE_LAYOUT_KEYED=0` restores the per-object maps
    // for dynamically learned layouts (A/B validation). Codegen-registered,
    // immutable layouts remain available because their side-mask headers
    // depend on them for correctness.
    *E.get_or_init(|| super::env_default_on_enabled("PERRY_SHAPE_LAYOUT_KEYED"))
}

/// Borrow the shared canonical descriptor for `user_ptr`'s shape, if
/// shape-keying is on, the object carries a keys_array, and the shape is
/// unambiguous (`Some`). Runs `f` against the descriptor in place — the GC
/// trace path and the store fast path both consult it per object/per store, and
/// a `Heap` mask would allocate a `Vec` on every clone.
#[inline]
unsafe fn with_shape_shared_descriptor<R>(
    user_ptr: usize,
    f: impl Fn(&TypedLayoutDescriptor) -> R,
) -> Option<R> {
    // keys_array / ShapeId only exist on genuine shaped objects
    // (`ObjectFields`). Arrays, closures, RegExps etc. also flow through
    // `layout_note_slot` / `layout_visit_pointer_slots`, and reading those
    // header words off one would interpret unrelated payload bytes as a
    // pointer. Anything else skips the shared shape path.
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
        return None;
    }
    let object = user_ptr as *const crate::object::ObjectHeader;
    // ONE shape-table probe (#8122). This used to be two — one for the keys
    // edge (the retired `object_keys_array_ptr`) and one here for the live
    // bound — on every field store that reaches it and on every traced object.
    let descriptor = crate::object::shapes::object_shape_descriptor(object);
    with_shape_shared_descriptor_from(user_ptr, descriptor, f)
}

/// [`with_shape_shared_descriptor`] against a receiver `ShapeDescriptor` the
/// caller has already resolved (or found absent). The receiver MUST be an
/// ObjectFields object — this skips the kind screen the probing form applies.
///
/// #8122: the collector's per-object path resolves the descriptor once in
/// `gc_child_slots` and hands it down here through
/// [`HeapChildSlotIterator::new_object`], instead of re-probing the shape
/// table for the keys edge and again for the live bound.
#[inline]
unsafe fn with_shape_shared_descriptor_from<R>(
    user_ptr: usize,
    descriptor: Option<crate::object::shapes::ShapeDescriptor>,
    f: impl Fn(&TypedLayoutDescriptor) -> R,
) -> Option<R> {
    let object = user_ptr as *const crate::object::ObjectHeader;
    let shape_id = crate::object::shapes::object_shape_stamp(object);
    if shape_id == 0 {
        return None;
    }
    // Defense-in-depth: both descriptor families must agree on the exact live
    // bound. #8113: an unstamped receiver has no bound anywhere, so 0 — not a
    // second probe (`unwrap_or` is eager).
    let field_count = descriptor
        .map(|descriptor| descriptor.live_inline_slot_count as usize)
        .unwrap_or(0);
    if shape_layout_keyed_enabled() {
        let map = hot_shape_layouts().borrow();
        if let Some(desc) = map.get(&shape_id) {
            let desc = desc.as_ref()?;
            if desc.slot_count != field_count {
                return None;
            }
            return Some(f(desc));
        }
    }

    // #8405: codegen-registered pointer-bearing class layouts live in a
    // process-global immutable registry because the module header image is
    // shared by workers. A dedicated ShapeId makes this lookup unambiguous.
    // Cache the descriptor in the current agent's ordinary hot table on first
    // use, so the mutex is paid once per shape/thread, never per trace/store.
    let desc = typed_shape::registered_typed_shape_layout(shape_id)?;
    if desc.slot_count != field_count {
        return None;
    }
    if shape_layout_keyed_enabled() {
        hot_shape_layouts()
            .borrow_mut()
            .insert(shape_id, Some(desc.clone()));
    }
    Some(f(&desc))
}

/// Answer a *query* about `user_ptr`'s current canonical typed layout, whichever
/// map holds it: the per-object `TYPED_LAYOUTS` entry (objects that diverged
/// from their shape, or carry no keys_array), else — and only while the object
/// is still `GC_OBJ_TYPED_LAYOUT_INTACT` — the shape-shared `SHAPE_LAYOUTS`
/// entry.
///
/// #6957: #6893 moved the descriptor of every *shape-keyed* object (i.e. every
/// class instance — it carries a shared `keys_array`) out of `TYPED_LAYOUTS` and
/// **deleted the per-object entry**. It taught `layout_note_slot`,
/// `layout_visit_pointer_slots` and `heap_payload_slot_selection`'s mask lookup
/// about the new home but not the query helpers below, so every one of them
/// started reporting "no typed descriptor" for real class instances — silently
/// deopting every typed guard that consults them. The existing layout tests all
/// allocate with `js_object_alloc` (class 0, no keys_array), which still takes
/// the per-object path, so nothing caught it.
///
/// The INTACT gate on the shared half is load-bearing.
/// `layout_set_typed_unknown` downgrades exactly ONE object (a store that
/// contradicts the descriptor) by clearing its intact bit and dropping its
/// per-object entry; it cannot drop the `SHAPE_LAYOUTS` entry, which still
/// correctly describes every sibling that has *not* diverged. Reading the shared
/// descriptor without the bit would therefore keep reporting the pre-downgrade
/// layout for the very object that just invalidated it.
///
/// The per-object half stays ungated, so this remains an independent check on a
/// forged/stale intact header bit (see
/// [`layout_typed_accepts_finite_number_slot_for_user`]).
#[inline]
fn with_typed_descriptor_for_query<R>(
    user_ptr: usize,
    f: impl Fn(&TypedLayoutDescriptor) -> R,
) -> Option<R> {
    if let Some(result) = with_per_object_descriptor(user_ptr, &f) {
        return Some(result);
    }
    if !layout_typed_intact_for_user(user_ptr) {
        return None;
    }
    unsafe { with_shape_shared_descriptor(user_ptr, f) }
}

/// Trace-path helper: pointer mask for a SIDE_MASK object with no per-object
/// mask entry. Returns the shape's canonical pointer mask iff the object is
/// still INTACT (⟹ it was registered against the shared shape descriptor, not
/// a diverged per-object mask).
#[inline]
unsafe fn shape_shared_pointer_mask(
    user_ptr: usize,
    header: *const GcHeader,
) -> Option<LayoutSlotMask> {
    if (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT == 0 {
        return None;
    }
    // Clone the ONE mask we return, not the whole descriptor. `LayoutSlotMask`
    // is `Heap(Vec<u64>)` above 64 slots, so `shape_shared_descriptor`'s
    // `desc.clone()` allocated and freed a second vector — the `raw_f64_mask`
    // we immediately drop — once per traced wide object per GC walk.
    with_shape_shared_descriptor(user_ptr, |d| d.pointer_mask.clone())
}

/// [`shape_shared_pointer_mask`] for an ObjectFields receiver whose
/// `ShapeDescriptor` the caller already resolved (#8122, see
/// [`with_shape_shared_descriptor_from`]).
#[inline]
unsafe fn shape_shared_pointer_mask_from(
    user_ptr: usize,
    header: *const GcHeader,
    descriptor: Option<crate::object::shapes::ShapeDescriptor>,
) -> Option<LayoutSlotMask> {
    if (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT == 0 {
        return None;
    }
    with_shape_shared_descriptor_from(user_ptr, descriptor, |d| d.pointer_mask.clone())
}

/// Install `descriptor` as the canonical layout for `shape_id` and set the
/// object's header state (INTACT + POINTER_FREE/SIDE_MASK), WITHOUT any
/// per-object map entry. Returns `true` if the object now rides the shared
/// shape descriptor; `false` if the shape is ambiguous (caller falls back to
/// per-object).
unsafe fn shape_install_shared(
    shape_id: u32,
    header: *mut GcHeader,
    descriptor: &TypedLayoutDescriptor,
) -> bool {
    let shared_ok = {
        let mut m = hot_shape_layouts().borrow_mut();
        match m.get(&shape_id) {
            None => {
                m.insert(shape_id, Some(descriptor.clone()));
                true
            }
            Some(Some(existing)) if existing == descriptor => true,
            Some(Some(_)) => {
                // Same keys, different layout ⟹ ambiguous. Poison the entry so
                // future lookups (and any still-INTACT siblings) fall back.
                m.insert(shape_id, None);
                // #7510: `Some(D)` → `None` is the ONE transition that can
                // falsify a construction memo (see `gc::shape_install`). It is
                // also the only transition this map has, since entries are
                // never removed and never overwritten with a different `Some`.
                super::shape_install::invalidate();
                false
            }
            // Already ambiguous.
            Some(None) => false,
        }
    };
    if shared_ok {
        header_set_typed_layout_intact(header);
        if descriptor.pointer_mask.is_empty() {
            set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        } else {
            set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        }
    }
    shared_ok
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
pub(in crate::gc) fn layout_pointer_bearing_bits(bits: u64) -> bool {
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
pub(super) fn layout_raw_f64_bits(bits: u64) -> bool {
    let tag = bits & crate::value::TAG_MASK;
    !(crate::value::SHORT_STRING_TAG..=crate::value::STRING_TAG).contains(&tag)
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
    typed_layouts_remove(user_ptr as usize);
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

/// True when `user_ptr`'s object currently has a canonical `TypedLayoutDescriptor`
/// — per-object in `TYPED_LAYOUTS` or (the #6893 common case) shared by shape in
/// `SHAPE_LAYOUTS`. Reads the O(1) `GC_OBJ_TYPED_LAYOUT_INTACT` header bit
/// instead of probing either map: the bit is maintained in lock-step with
/// descriptor install/removal (intact set ⟹ *some* descriptor is reachable —
/// see the invariant on `GC_OBJ_TYPED_LAYOUT_INTACT`), so it answers the same
/// question without a per-call TLS hashmap touch. This is on the dynamic
/// object-store hot path via `mark_object_dynamic_shape_unknown` (#5094).
pub(crate) fn layout_has_typed_descriptor(user_ptr: usize) -> bool {
    layout_typed_intact_for_user(user_ptr)
}

/// #8115 test probe: would [`layout_note_slot`]'s descriptor probe find a
/// `TypedLayoutDescriptor` for `user_ptr` right now — asked of the two maps,
/// never of the header bit?
///
/// [`layout_has_typed_descriptor`] above answers a similar question by reading
/// `GC_OBJ_TYPED_LAYOUT_INTACT`, which is the very claim #8115 is about. A test
/// that used it could not tell "the bit is honest" from "the bit lies", so the
/// premise of every intact-bit test has to come from here instead.
///
/// Note this is the *shape*'s answer, not the object's licence: the shared
/// `SHAPE_LAYOUTS` entry outlives one object's divergence on purpose (see
/// `with_shape_shared_descriptor`), so after `layout_set_typed_unknown` this
/// still reports `true` while the diverged object no longer claims it.
#[cfg(test)]
pub(in crate::gc) fn layout_descriptor_reachable(user_ptr: usize) -> bool {
    if with_per_object_descriptor(user_ptr, |_| ()).is_some() {
        return true;
    }
    unsafe { with_shape_shared_descriptor(user_ptr, |_| ()).is_some() }
}

pub(super) unsafe fn layout_set_typed_unknown(header: *mut GcHeader, user_ptr: usize) {
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    header_clear_typed_layout_intact(header);
    layout_forget_object(user_ptr);
    crate::typed_feedback::invalidate_representation_change(user_ptr);
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
            // #8690: the proof payload lives with ObjectMeta, but this
            // GcHeader bit is its cheap authority. Retire it before an inline
            // owner store; object-owned spill stores use the matching owner
            // hook because their physical layout note names the spill Array.
            crate::array::clear_packed_subclass_numeric_proof(
                parent_user as *mut crate::object::ObjectHeader,
            );
        }
        if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_UNKNOWN {
            return;
        }
        // The canonical typed-shape descriptor probe below is a thread-local
        // hashmap lookup, paid on every field/element store. Gate it on the
        // O(1) `GC_OBJ_TYPED_LAYOUT_INTACT` header bit: that bit is set and
        // cleared in lock-step with descriptor install/removal (per-object in
        // `TYPED_LAYOUTS` or, since #6893, shared by shape in `SHAPE_LAYOUTS` —
        // see the invariant on `GC_OBJ_TYPED_LAYOUT_INTACT`), so a clear bit
        // proves neither map has a descriptor for this object — the probe would
        // return `None` and fall through to the pointer-mask path below.
        // Skipping it removes the per-write TLS touch on the common dynamic-shape
        // / pointer-free object and array store path (#5094). The inner `if let`
        // still tolerates a `None` defensively — see the #8115 clear below for
        // what a `None` costs and why it is no longer merely a fall-through.
        let claimed_intact = (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT != 0;
        if claimed_intact {
            // #5094: a plain, non-pointer-bearing double is representation-
            // compatible with every in-bounds typed object slot. A raw-f64
            // slot consumes the bits directly; a boxed slot consumes the same
            // bits as an ordinary NaN-boxed number; and neither case changes
            // the pointer mask. The descriptor's slot_count was pinned equal
            // to ObjectHeader::field_count when INTACT was installed (also a
            // requirement of the descriptor-free pointer-free allocation
            // bake), so this header + bounds check proves the same `Conforms`
            // verdict without resolving either thread-local descriptor map.
            //
            // Keep the raw-pointer classifier in the predicate. Some aligned
            // low words are both valid f64 bit patterns and conservatively
            // pointer-bearing; a boxed slot outside the pointer mask must
            // still take the descriptor path and downgrade on such a store.
            if layout_raw_f64_bits(value_bits)
                && !layout_pointer_bearing_bits(value_bits)
                // ObjectFields currently has exactly one concrete type. Use
                // the byte already in the hot header instead of walking the
                // type-info table a second time after layout_header_for_user.
                && (*header).obj_type == GC_TYPE_OBJECT
            {
                let object = parent_user as *const crate::object::ObjectHeader;
                // #8113: 0, not a second (eager) descriptor probe. This is
                // `layout_note_slot`, i.e. every object field store.
                let live_slots = crate::object::shapes::object_shape_descriptor(object)
                    .map(|descriptor| descriptor.live_inline_slot_count as usize)
                    .unwrap_or(0);
                if slot_index < live_slots {
                    return;
                }
            }

            // #6893: per-object descriptor (diverged/ambiguous objects) OR the
            // shared shape descriptor (the common INTACT case). Exactly one is
            // present for an INTACT object.
            //
            // #7510: decide *inside* the borrow and act after it, rather than
            // cloning the descriptor out. The clone existed only so that
            // `layout_set_typed_unknown` — which takes both maps mutably —
            // could not re-enter a live `RefCell` borrow; a `SlotVerdict` is
            // two bits and carries no `Vec`, so a `Heap` mask no longer
            // allocates on every store. The per-object probe is also skipped
            // outright while [`PER_OBJECT_LAYOUTS_NONEMPTY`] proves that map
            // empty, which on a monomorphic workload is always.
            #[cfg(test)]
            TYPED_SLOT_DESCRIPTOR_PROBES.with(|c| c.set(c.get() + 1));
            let verdict = {
                let classify = |typed: &TypedLayoutDescriptor| {
                    if slot_index >= typed.slot_count {
                        return SlotVerdict::Downgrade;
                    }
                    if typed.raw_f64_mask.contains_slot(slot_index) {
                        return if layout_raw_f64_bits(value_bits) {
                            SlotVerdict::Conforms
                        } else {
                            SlotVerdict::Downgrade
                        };
                    }
                    if layout_pointer_bearing_bits(value_bits)
                        && !typed.pointer_mask.contains_slot(slot_index)
                    {
                        return SlotVerdict::Downgrade;
                    }
                    SlotVerdict::Conforms
                };
                match with_per_object_descriptor(parent_user, classify) {
                    Some(verdict) => Some(verdict),
                    None => with_shape_shared_descriptor(parent_user, classify),
                }
            };
            if let Some(verdict) = verdict {
                if verdict == SlotVerdict::Downgrade {
                    layout_set_typed_unknown(header, parent_user);
                }
                return;
            }
        }
        // #8115: reaching here with the bit still set means BOTH descriptor maps
        // answered `None` — the probe above is exhaustive — so the object is
        // INTACT and descriptor-less, and the invariant documented on
        // [`GC_OBJ_TYPED_LAYOUT_INTACT`] ("intact ⟹ a canonical descriptor is
        // reachable") is false for it. Restore the invariant here, at the one
        // place that observes it broken.
        //
        // The state stores in the generic pointer-mask branch below CANNOT do
        // it: `set_layout_state` masks `!(GC_LAYOUT_STATE_MASK |
        // GC_LAYOUT_ALL_POINTERS)` = `!0xE000`, and this bit is `0x1000`. So
        // before this clear the branch could publish `SIDE_MASK | INTACT`
        // WITHOUT a descriptor — a state three separate consumers read as a
        // proof they may skip a map:
        //
        // * `class_field_inline_guard` (codegen) tests this bit ALONE before
        //   reading/writing a slot as a bare `double`;
        // * `element_shape_guard`'s packed `0x1800_80FF` header test folds it
        //   in for the same license;
        // * `class_field_store_layout_note_is_conforming` (codegen
        //   `expr/helpers.rs`) elides the layout note outright on
        //   `_reserved & 0xD000 == 0x9000`, whose proof is "a descriptor built
        //   from this class's mask globals is reachable".
        //
        // #7834's at-allocation bake is what made that reachable: it stamps
        // `POINTER_FREE | INTACT` into the inline `new`'s header constant with
        // no descriptor behind it, deliberately, on the argument that the
        // generic branch below downgrades correctly. It does — for the
        // collector. The bit it leaves behind is the half that was missing:
        // `docs/engine-plan.md`'s construction-cost section, item 2, named this
        // mechanism exactly — it used to forbid the bake outright, and now
        // records the residual and this repair.
        //
        // Cost: one 16-bit store, only on the fall-through, which for a baked
        // object is only ever a store the descriptor path would have called
        // `layout_set_typed_unknown` for. Pre-#7834 that is precisely what it
        // did — every pointer-free class carried a real descriptor, and any
        // non-conforming store evicted it, bit included.
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
                        masks.insert(parent_user, mask);
                        mark_per_object_layouts_nonempty();
                        // The one insert site that holds its own `borrow_mut`,
                        // so it maintains the address filter inline too.
                        super::layout_tables::layout_addr_filter_note(parent_user);
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
///
/// The GC slot mask records one bit of information: whether a slot can carry a
/// heap edge. Replacing one pointer-bearing value with another cannot change
/// that bit. Arrays still have a second, independent invariant to maintain —
/// their homogeneous element-shape record — so the pointer-over-pointer path
/// runs that hook after validating/chasing the owner header and then stops
/// before the typed-layout and per-slot-mask machinery. Object-backed packed
/// numeric proofs are retired for the same reason as in [`layout_note_slot`].
///
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

/// True when `slot_index` of `parent_user` is a **raw-f64-masked slot of an
/// intact typed-shape descriptor** — i.e. exactly the case where
/// [`layout_note_slot`] would call `layout_set_typed_unknown` (permanently
/// evicting the descriptor) for a stored value whose bits are not raw f64.
///
/// Mirrors `layout_note_slot`'s own prologue — forwarding resolution, the
/// `GC_LAYOUT_UNKNOWN` short-circuit, and the O(1) `GC_OBJ_TYPED_LAYOUT_INTACT`
/// gate before the thread-local probe — so the two agree on every object.
pub(crate) fn layout_slot_is_raw_f64_typed(parent_user: usize, slot_index: usize) -> bool {
    if slot_index > 16_000_000 {
        return false;
    }
    unsafe {
        let Some(header) = layout_header_for_user(parent_user) else {
            return false;
        };
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            let new_user = forwarding_address(header) as usize;
            if new_user != 0 && new_user != parent_user {
                return layout_slot_is_raw_f64_typed(new_user, slot_index);
            }
            return false;
        }
        if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_UNKNOWN {
            return false;
        }
        if (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT == 0 {
            return false;
        }
        // #6893/#6957: per-object descriptor (diverged objects, and objects with
        // no keys_array) OR the shared shape descriptor — exactly as
        // `layout_note_slot` resolves it, which is the agreement this helper
        // documents.
        with_per_object_descriptor(parent_user, |typed| {
            slot_index < typed.slot_count && typed.raw_f64_mask.contains_slot(slot_index)
        })
        .or_else(|| {
            with_shape_shared_descriptor(parent_user, |typed| {
                slot_index < typed.slot_count && typed.raw_f64_mask.contains_slot(slot_index)
            })
        })
        .unwrap_or(false)
    }
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
    typed_layouts_remove(user_ptr as usize);
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
        slot_masks_insert(user_ptr as usize, mask);
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
            slot_masks_insert(user_ptr as usize, LayoutSlotMask::Inline(bits));
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
        slot_masks_insert(user_ptr as usize, mask);
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

pub(crate) unsafe fn layout_transfer(old_user: *mut u8, new_user: *mut u8) {
    if old_user.is_null() || new_user.is_null() || old_user == new_user {
        return;
    }
    let Some(old_header) = layout_header_for_user(old_user as usize) else {
        return;
    };
    let Some(new_header) = layout_header_for_user(new_user as usize) else {
        return;
    };
    let state = (*old_header)._reserved & GC_LAYOUT_STATE_MASK;
    let all_pointers = (*old_header)._reserved & GC_LAYOUT_ALL_POINTERS != 0;
    set_layout_state(new_header, state);
    if all_pointers {
        (*new_header)._reserved |= GC_LAYOUT_ALL_POINTERS;
    }
    if (*old_header).obj_type == GC_TYPE_ARRAY && (*new_header).obj_type == GC_TYPE_ARRAY {
        crate::array::transfer_array_numeric_layout(old_user as usize, new_user as usize);
        // #7480: the element-shape bit rides `_reserved` for free, but its
        // record is address-keyed and has to follow the move — same split,
        // and same call site, as `TYPED_LAYOUTS` below.
        crate::array::transfer_element_shape(old_user as usize, new_user as usize);
        // #9304: real arrays keep explicit [[Prototype]] values in the
        // residual address-keyed registry. Array growth and moving GC both
        // replace the owner allocation through this transfer hook.
        crate::object::prototype_chain::object_static_prototype_owner_moved(
            old_user as usize,
            new_user as usize,
        );
    } else {
        crate::array::clear_array_numeric_layout_ptr(new_user as usize);
        crate::array::clear_element_shape_ptr(new_user as usize);
    }
    // Read the source object's intact bit BEFORE the transfer clears it — it is
    // the per-object half of the shape-keyed resolution below. `_reserved` is
    // untouched by `set_forwarding_address` (which writes gc_flags and the first
    // payload word), so it is still authoritative here even though the
    // evacuation callers forward the original before calling us.
    let old_intact = (*old_header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT != 0;
    // #7510: with both per-object maps provably empty there is nothing to
    // move, and every relocated object would otherwise pay two `RefCell`
    // round-trips plus two hashes during evacuation. The shape-keyed half
    // below is unaffected — it needs no move at all.
    let new_has_typed = transfer_per_object_descriptor(old_user as usize, new_user as usize);
    // #6964: the canonical descriptor may live in EITHER map, exactly as the
    // query helpers resolve it (#6957/#6963). The per-object `TYPED_LAYOUTS`
    // entry is keyed by ADDRESS, so it has to be moved (above). The shape-keyed
    // `SHAPE_LAYOUTS` entry (#6893/#8289) is keyed by immutable runtime
    // ShapeId, which the relocated copy carries verbatim — it needs no move,
    // but it only describes THIS object while the object is still INTACT.
    //
    // Probing only `TYPED_LAYOUTS` missed for every object #6893 actually moved
    // (i.e. every class instance: it carries a keys_array and therefore has NO
    // per-object entry), so `new_has_typed` was false and the relocated copy had
    // a still-valid intact bit CLEARED — permanently deopting its typed guards.
    // Latent until an evacuating minor became reachable (#6950); the fourth
    // caller, array growth in `array/push_pop.rs`, is `GC_TYPE_ARRAY`, which is
    // not `GcLayoutSlotKind::ObjectFields` and so never had a shape-keyed
    // descriptor to lose.
    //
    // Read the shape through `new_user`: the evacuation callers install the
    // forwarding pointer over the ORIGINAL's first payload word, which for an
    // ObjectFields object overlaps the header fields this lookup reads.
    //
    // Mirrors #6963's split: the per-object half stays ungated (so a forged or
    // stale intact bit cannot manufacture a descriptor), the shared half is
    // gated on the source object's intact bit (so an object that diverged from
    // its shape does not silently re-adopt the shape's stale descriptor by
    // moving).
    let new_has_shape_typed = !new_has_typed
        && old_intact
        && with_shape_shared_descriptor(new_user as usize, |_| ()).is_some();
    // Keep the intact bit in lock-step with the moved descriptor. Copying GC
    // normally propagates `_reserved` (so the bit already rode along), but
    // re-sync defensively for callers that allocate the destination fresh
    // (e.g. array growth) so a stale/missing bit can never desync from the map.
    if new_has_typed || new_has_shape_typed {
        header_set_typed_layout_intact(new_header);
    } else {
        header_clear_typed_layout_intact(new_header);
    }
    header_clear_typed_layout_intact(old_header);
    transfer_per_object_slot_mask(old_user as usize, new_user as usize);
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
                let mask = per_object_slot_mask(user_ptr)
                    .or_else(|| shape_shared_pointer_mask(user_ptr, header));
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
/// bit the codegen-inlined class-field shape guard tests; exposed for the
/// `PERRY_VERIFY_TYPED_INTACT=1` self-check in the typed-feedback fast contract,
/// which asserts the bit never claims a raw-f64 layout the side table disagrees
/// with.
pub(crate) fn layout_typed_intact_for_user(user_ptr: usize) -> bool {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(user_ptr as *const u8);
        (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT != 0
    }
}

pub(crate) fn layout_typed_raw_f64_slot_for_user(user_ptr: usize, slot_index: usize) -> bool {
    #[cfg(test)]
    TYPED_RAW_F64_DESCRIPTOR_QUERIES.with(|c| c.set(c.get() + 1));
    with_typed_descriptor_for_query(user_ptr, |layout| {
        slot_index < layout.slot_count && layout.raw_f64_mask.contains_slot(slot_index)
    })
    .unwrap_or(false)
}

/// Validate that an intact typed descriptor contains `slot_index`.
///
/// A finite numeric value is representation-compatible with either kind of
/// typed object slot: raw-f64 slots consume the double directly, while every
/// other slot consumes the same bits as an ordinary NaN-boxed JS number.
/// Callers must separately prove the stored value finite; this helper also
/// keeps a forged/stale intact header bit from standing in for the descriptor
/// side-table invariant.
pub(crate) fn layout_typed_accepts_finite_number_slot_for_user(
    user_ptr: usize,
    slot_index: usize,
) -> bool {
    with_typed_descriptor_for_query(user_ptr, |layout| slot_index < layout.slot_count)
        .unwrap_or(false)
}

fn layout_typed_raw_f64_slot_count_for_user(user_ptr: usize, slot_count: usize) -> usize {
    with_typed_descriptor_for_query(user_ptr, |layout| {
        let bounded_count = slot_count.min(layout.slot_count);
        layout.raw_f64_mask.count_slots(bounded_count)
    })
    .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapSlotRange {
    pub(super) slots: *mut u64,
    pub(super) slot_count: usize,
}

impl HeapSlotRange {
    #[inline]
    pub(crate) fn new(slots: *mut u64, slot_count: usize) -> Self {
        Self { slots, slot_count }
    }

    #[inline]
    pub(super) fn is_empty(self) -> bool {
        self.slots.is_null() || self.slot_count == 0
    }

    #[inline]
    pub(super) fn slots(self) -> *mut u64 {
        self.slots
    }

    #[inline]
    pub(super) fn slot_count(self) -> usize {
        self.slot_count
    }

    #[inline]
    pub(super) unsafe fn slot(self, index: usize) -> *mut u64 {
        debug_assert!(index < self.slot_count);
        self.slots.add(index)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapChildSlot {
    Child(*mut u64, HeapChildSlotReadKind),
    PointerFreeRange(HeapSlotRange),
}

pub(super) enum HeapPayloadSlotScan {
    Empty,
    PointerFree {
        raw_numeric_array: bool,
        raw_numeric_object_slots: usize,
    },
    /// [`LayoutSlotMask::AllPointers`]: the mask selects EVERY live payload
    /// slot, so the slot set is a contiguous range and the descriptor visitor
    /// emits one `Range` rather than `slot_count` individual `Slot`s.
    ///
    /// This is not a micro-optimisation. `scan_dirty_object_slots`'s `Slot` arm
    /// answers "is this slot on a dirty page?" with a hash-set probe **per
    /// slot**, so a 3M-element array of pointers cost 3M probes on every minor
    /// — O(live array) rather than O(dirty pages) — even though the remembered
    /// set knew only a few hundred of its pages were dirty. Its `Range` arm
    /// intersects the range with the dirty-page set directly
    /// (`dirty_slot_ranges_for`), which is what `dirty_slot_ranges_scanned == 0`
    /// in every `retain.ts` GC trace was recording: the cheap arm was never
    /// reached, because an all-pointer array is `Masked`, not `All` (#7787).
    AllPointers {
        raw_numeric_object_slots: usize,
    },
    Masked,
    All(HeapSlotRange),
}

#[derive(Clone)]
pub(super) enum HeapPayloadSlotSelection {
    Empty,
    PointerFree {
        emitted: bool,
        raw_numeric_array: bool,
        raw_numeric_object_slots: usize,
    },
    Masked {
        mask: LayoutSlotMask,
        cursor: usize,
        raw_numeric_object_slots: usize,
        raw_numeric_recorded: bool,
    },
    All {
        cursor: usize,
    },
}

pub(crate) struct HeapChildSlotIterator {
    pub(super) prefix_slot: Option<*mut u64>,
    /// #6812: second prefix — the object's `meta` header edge. Kept
    /// separate from `prefix_slot` so payload indices stay mask-aligned.
    pub(super) meta_slot: Option<*mut u64>,
    /// A second explicit meta edge — the Array-subclass `elements` store at
    /// the end of the `ObjectMeta` record — read exactly like `meta_slot`.
    pub(super) meta_slot2: Option<*mut u64>,
    pub(super) payload: HeapSlotRange,
    pub(super) selection: HeapPayloadSlotSelection,
    /// #8122: the receiver's `ShapeDescriptor`, resolved ONCE by
    /// [`gc_child_slots`] for an ObjectFields object and carried here so
    /// `visit_gc_layout_slot_descriptors` reads the same facts instead of
    /// probing the shape table again. `None` for every other kind, and for
    /// an unstamped object.
    pub(super) object_shape: Option<crate::object::shapes::ShapeDescriptor>,
}

impl HeapChildSlotIterator {
    pub(super) fn empty() -> Self {
        Self {
            prefix_slot: None,
            meta_slot: None,
            meta_slot2: None,
            payload: HeapSlotRange::new(std::ptr::null_mut(), 0),
            selection: HeapPayloadSlotSelection::Empty,
            object_shape: None,
        }
    }

    pub(super) fn new(
        header: *mut GcHeader,
        prefix_slot: Option<*mut u64>,
        payload: HeapSlotRange,
    ) -> Self {
        let selection = unsafe { heap_payload_slot_selection(header, payload) };
        Self {
            prefix_slot,
            meta_slot: None,
            meta_slot2: None,
            payload,
            selection,
            object_shape: None,
        }
    }

    /// [`Self::new`] for an ObjectFields receiver whose `ShapeDescriptor` the
    /// caller already resolved (#8122). The payload-mask selection reuses it
    /// instead of probing the shape table, and it is retained on the iterator
    /// for the slot visitor.
    pub(super) fn new_object(
        header: *mut GcHeader,
        prefix_slot: Option<*mut u64>,
        payload: HeapSlotRange,
        object_shape: Option<crate::object::shapes::ShapeDescriptor>,
    ) -> Self {
        let selection = unsafe { heap_payload_slot_selection_from(header, payload, object_shape) };
        Self {
            prefix_slot,
            meta_slot: None,
            meta_slot2: None,
            payload,
            selection,
            object_shape,
        }
    }

    pub(super) fn with_meta_slot(mut self, slot: Option<*mut u64>) -> Self {
        self.meta_slot = slot;
        self
    }

    pub(super) fn with_meta_slot2(mut self, slot: Option<*mut u64>) -> Self {
        self.meta_slot2 = slot;
        self
    }

    pub(super) fn take_meta_child_slot(&mut self) -> Option<*mut u64> {
        self.meta_slot.take()
    }

    pub(super) fn take_meta_child_slot2(&mut self) -> Option<*mut u64> {
        self.meta_slot2.take()
    }

    pub(super) fn take_prefix_child_slot(&mut self) -> Option<*mut u64> {
        self.prefix_slot.take()
    }

    pub(super) fn payload_scan(&self) -> HeapPayloadSlotScan {
        match self.selection {
            HeapPayloadSlotSelection::Empty => HeapPayloadSlotScan::Empty,
            HeapPayloadSlotSelection::PointerFree {
                raw_numeric_array,
                raw_numeric_object_slots,
                ..
            } => HeapPayloadSlotScan::PointerFree {
                raw_numeric_array,
                raw_numeric_object_slots,
            },
            HeapPayloadSlotSelection::Masked {
                mask: LayoutSlotMask::AllPointers,
                raw_numeric_object_slots,
                raw_numeric_recorded,
                ..
            } => HeapPayloadSlotScan::AllPointers {
                // Mirror the iterator's one-shot accounting: `next` records the
                // raw-numeric skip on its first call and never again, so a
                // descriptor visit that replaces the whole iteration records it
                // exactly once too.
                raw_numeric_object_slots: if raw_numeric_recorded {
                    0
                } else {
                    raw_numeric_object_slots
                },
            },
            HeapPayloadSlotSelection::Masked { .. } => HeapPayloadSlotScan::Masked,
            HeapPayloadSlotSelection::All { .. } => HeapPayloadSlotScan::All(self.payload),
        }
    }
}

impl Iterator for HeapChildSlotIterator {
    type Item = HeapChildSlot;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(slot) = self.prefix_slot.take() {
            return Some(HeapChildSlot::Child(slot, HeapChildSlotReadKind::Prefix));
        }
        if let Some(slot) = self.meta_slot.take() {
            return Some(HeapChildSlot::Child(slot, HeapChildSlotReadKind::Prefix));
        }
        if let Some(slot) = self.meta_slot2.take() {
            return Some(HeapChildSlot::Child(slot, HeapChildSlotReadKind::Prefix));
        }
        match &mut self.selection {
            HeapPayloadSlotSelection::Empty => None,
            HeapPayloadSlotSelection::PointerFree {
                emitted,
                raw_numeric_array,
                raw_numeric_object_slots,
            } => {
                if *emitted || self.payload.is_empty() {
                    None
                } else {
                    *emitted = true;
                    record_layout_pointer_free_range_skipped(self.payload.slot_count());
                    if *raw_numeric_array {
                        record_layout_raw_numeric_array_range_skipped(self.payload.slot_count());
                    }
                    if *raw_numeric_object_slots != 0 {
                        record_layout_raw_numeric_object_field_range_skipped(
                            *raw_numeric_object_slots,
                        );
                    }
                    Some(HeapChildSlot::PointerFreeRange(self.payload))
                }
            }
            HeapPayloadSlotSelection::Masked {
                mask,
                cursor,
                raw_numeric_object_slots,
                raw_numeric_recorded,
            } => {
                if !*raw_numeric_recorded {
                    *raw_numeric_recorded = true;
                    if *raw_numeric_object_slots != 0 {
                        record_layout_raw_numeric_object_field_range_skipped(
                            *raw_numeric_object_slots,
                        );
                    }
                }
                let index = mask.next_slot_at_or_after(*cursor, self.payload.slot_count())?;
                *cursor = index + 1;
                Some(HeapChildSlot::Child(
                    unsafe { self.payload.slot(index) },
                    HeapChildSlotReadKind::Masked,
                ))
            }
            HeapPayloadSlotSelection::All { cursor } => {
                if *cursor >= self.payload.slot_count() {
                    return None;
                }
                let index = *cursor;
                *cursor += 1;
                Some(HeapChildSlot::Child(
                    unsafe { self.payload.slot(index) },
                    HeapChildSlotReadKind::Unknown,
                ))
            }
        }
    }
}

pub(super) unsafe fn heap_payload_slot_selection(
    header: *mut GcHeader,
    payload: HeapSlotRange,
) -> HeapPayloadSlotSelection {
    heap_payload_slot_selection_impl(header, payload, |user_ptr, header| {
        shape_shared_pointer_mask(user_ptr, header)
    })
}

/// [`heap_payload_slot_selection`] for an ObjectFields receiver whose
/// `ShapeDescriptor` the caller already resolved (#8122): the shared-shape
/// pointer-mask lookup reuses it instead of probing the shape table twice.
pub(super) unsafe fn heap_payload_slot_selection_from(
    header: *mut GcHeader,
    payload: HeapSlotRange,
    descriptor: Option<crate::object::shapes::ShapeDescriptor>,
) -> HeapPayloadSlotSelection {
    heap_payload_slot_selection_impl(header, payload, |user_ptr, header| {
        shape_shared_pointer_mask_from(user_ptr, header, descriptor)
    })
}

#[inline]
unsafe fn heap_payload_slot_selection_impl(
    header: *mut GcHeader,
    payload: HeapSlotRange,
    shared_mask: impl FnOnce(usize, *const GcHeader) -> Option<LayoutSlotMask>,
) -> HeapPayloadSlotSelection {
    if header.is_null() || payload.is_empty() {
        return HeapPayloadSlotSelection::Empty;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    // `raw_numeric_object_slots` feeds exactly one consumer:
    // `record_layout_raw_numeric_object_field_range_skipped`, a counter that
    // returns on its first line unless `PERRY_GC_LAYOUT_SCAN_TRACE` armed
    // `layout_scan_trace_active()`. Computing it costs
    // `with_typed_descriptor_for_query` — a per-object map probe and, for every
    // class instance, a `SHAPE_LAYOUTS` hash lookup behind a TLS `RefCell`
    // borrow — and this function runs once per traced object per GC walk
    // (mark, rewrite, verify). So the shipped collector paid a hash lookup per
    // object to produce a number nothing read. Same shape as #7702: a facility
    // already disabled at runtime, whose *argument* was still being evaluated.
    let raw_numeric_object_slots =
        if (*header).obj_type == GC_TYPE_OBJECT && layout_scan_trace_active() {
            layout_typed_raw_f64_slot_count_for_user(user_ptr, payload.slot_count())
        } else {
            0
        };
    match (*header)._reserved & GC_LAYOUT_STATE_MASK {
        GC_LAYOUT_POINTER_FREE => HeapPayloadSlotSelection::PointerFree {
            emitted: false,
            raw_numeric_array: (*header).obj_type == GC_TYPE_ARRAY
                && (*header)._reserved & GC_ARRAY_RAW_F64_LAYOUT != 0,
            raw_numeric_object_slots,
        },
        GC_LAYOUT_SIDE_MASK => {
            if (*header)._reserved & GC_LAYOUT_ALL_POINTERS != 0 {
                return HeapPayloadSlotSelection::Masked {
                    mask: LayoutSlotMask::AllPointers,
                    cursor: 0,
                    raw_numeric_object_slots,
                    raw_numeric_recorded: false,
                };
            }
            let mask = per_object_slot_mask(user_ptr).or_else(|| shared_mask(user_ptr, header));
            match mask {
                Some(mask) => HeapPayloadSlotSelection::Masked {
                    mask,
                    cursor: 0,
                    raw_numeric_object_slots,
                    raw_numeric_recorded: false,
                },
                None => {
                    set_layout_state(header, GC_LAYOUT_UNKNOWN);
                    HeapPayloadSlotSelection::All { cursor: 0 }
                }
            }
        }
        _ => HeapPayloadSlotSelection::All { cursor: 0 },
    }
}

pub(super) unsafe fn gc_child_slots(header: *mut GcHeader) -> HeapChildSlotIterator {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return HeapChildSlotIterator::empty();
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match gc_type_layout_slot_kind((*header).obj_type) {
        GcLayoutSlotKind::ArrayElements => {
            let arr = user_ptr as *mut crate::array::ArrayHeader;
            crate::array::gc_element_slot_range(arr)
                .map(|range| HeapChildSlotIterator::new(header, None, range))
                .unwrap_or_else(HeapChildSlotIterator::empty)
        }
        GcLayoutSlotKind::ObjectFields => {
            let obj = user_ptr as *mut crate::object::ObjectHeader;
            // #8122: resolve the receiver's ShapeDescriptor ONCE and thread it
            // through every step that needs a shape fact — the field range,
            // the keys edge, the shared pointer mask (`new_object`) and the
            // slot visitor (`object_shape` on the iterator). These used to be
            // five independent `shape_descriptor_by_id` probes per traced
            // object, the top leaf of a traced in-place-promotion cycle.
            let descriptor = crate::object::shapes::object_shape_descriptor(obj);
            let Some(range) = crate::object::gc_field_slot_range(obj, descriptor) else {
                return HeapChildSlotIterator::empty();
            };
            // #6812: the meta record is a raw-pointer child edge; before the
            // spill buffer it was enumerated only on the rewrite path, which
            // left it invisible to MARKING (latent for custom prototypes,
            // which are usually rooted elsewhere; fatal for the spill
            // buffer, reachable through meta alone). A second prefix slot
            // keeps payload slot indices aligned with the layout masks.
            HeapChildSlotIterator::new_object(header, None, range, descriptor)
                .with_meta_slot(crate::object::gc_object_meta_slot(user_ptr as usize))
        }
        GcLayoutSlotKind::RegExpFields => {
            let (pattern_slot, slot_count, last_index_slot) =
                crate::regex::regex_gc_slot_ptrs(user_ptr as *mut crate::regex::RegExpHeader);
            HeapChildSlotIterator::new(
                header,
                Some(last_index_slot),
                HeapSlotRange::new(pattern_slot, slot_count),
            )
            // #6759 phase 1: the metadata edge. RegExp reaches marking through
            // THIS iterator (its rewrite arm delegates here), so the edge has
            // to be enumerated at this point, not in the rewrite match.
            .with_meta_slot(crate::object::cell_meta_slot(user_ptr as usize).map(|s| s as *mut u64))
        }
        GcLayoutSlotKind::ObjectMeta => {
            // Prototype and the private-evaluation brand are explicit prefix
            // edges. Keep the brand out of the payload selection: its class
            // object can be reachable only through this metadata record, so
            // treating it as ordinary payload lets a stale/partial layout
            // mask silently collect the class evaluation identity.
            let meta = user_ptr as *mut crate::object::ObjectMeta;
            let proto_slot = Some(&mut (*meta).prototype as *mut u64);
            let brand_slot = Some(&mut (*meta).private_evaluation_brand as *mut u64);
            let range = HeapSlotRange::new(&mut (*meta).spill as *mut u64, 1);
            // The Array-subclass elements store is a raw-pointer child edge
            // (0 = none) exactly like `spill`; it sits at the end of the
            // record, so it is enumerated as a second explicit meta edge.
            let elements_slot = Some(&mut (*meta).elements as *mut u64);
            HeapChildSlotIterator::new(header, proto_slot, range)
                .with_meta_slot(brand_slot)
                .with_meta_slot2(elements_slot)
        }
        GcLayoutSlotKind::ClosureCaptures => {
            let closure = user_ptr as *mut crate::closure::ClosureHeader;
            crate::closure::gc_capture_slot_range(closure)
                .map(|range| HeapChildSlotIterator::new(header, None, range))
                .unwrap_or_else(HeapChildSlotIterator::empty)
        }
        GcLayoutSlotKind::None => HeapChildSlotIterator::empty(),
    }
}

#[derive(Clone, Copy)]
pub(super) struct GcMutableSlot {
    pub(super) slot: *mut u64,
    pub(super) layout_kind: Option<HeapChildSlotReadKind>,
    pub(super) external: bool,
}

impl GcMutableSlot {
    #[inline]
    pub(super) fn new(slot: *mut u64, layout_kind: Option<HeapChildSlotReadKind>) -> Self {
        let external = !matches!(
            crate::arena::classify_heap_generation(slot as usize),
            crate::arena::HeapGeneration::Old
        );
        Self {
            slot,
            layout_kind,
            external,
        }
    }

    #[inline]
    pub(super) fn record_layout_read(self) {
        if let Some(kind) = self.layout_kind {
            record_layout_child_slot_read(kind);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum GcMutableSlotDescriptor {
    Slot(GcMutableSlot),
    Range {
        range: HeapSlotRange,
        layout_kind: Option<HeapChildSlotReadKind>,
    },
    PointerFreeRange(HeapSlotRange),
}

impl GcMutableSlotDescriptor {
    pub(super) unsafe fn visit_slots(self, visit: &mut dyn FnMut(GcMutableSlot)) {
        match self {
            GcMutableSlotDescriptor::Slot(slot) => visit(slot),
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                for i in 0..range.slot_count() {
                    visit(GcMutableSlot::new(range.slot(i), layout_kind));
                }
            }
            GcMutableSlotDescriptor::PointerFreeRange(_) => {}
        }
    }
}

#[inline]
#[cfg(test)]
pub(crate) fn test_layout_pointer_slot_count(user_ptr: usize, slot_count: usize) -> Option<usize> {
    let mut count = 0usize;
    if layout_visit_pointer_slots(user_ptr, slot_count, |_| count += 1) {
        Some(count)
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) fn test_gc_rewrite_slot_count(user_ptr: usize) -> Option<usize> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = unsafe { header_from_user_ptr(user_ptr as *const u8) };
    let mut count = 0usize;
    unsafe {
        visit_gc_rewrite_slot_descriptors(header, |descriptor| {
            let mut visit_slot = |_| {
                count += 1;
            };
            descriptor.visit_slots(&mut visit_slot);
        });
    }
    Some(count)
}

#[cfg(test)]
pub(crate) fn test_gc_rewrite_slot_addresses(user_ptr: usize) -> Option<Vec<usize>> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = unsafe { header_from_user_ptr(user_ptr as *const u8) };
    let mut slots = Vec::new();
    unsafe {
        visit_gc_rewrite_slot_descriptors(header, |descriptor| {
            descriptor.visit_slots(&mut |slot| slots.push(slot.slot as usize));
        });
    }
    Some(slots)
}

#[inline(always)]
pub(super) fn record_trace_slot_read() {
    #[cfg(test)]
    TRACE_SLOT_READS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
pub(super) fn test_reset_trace_slot_reads() {
    TRACE_SLOT_READS.with(|c| c.set(0));
}

#[cfg(test)]
pub(super) fn test_trace_slot_reads() -> usize {
    TRACE_SLOT_READS.with(|c| c.get())
}

#[cfg(test)]
pub(super) fn test_reset_typed_slot_descriptor_probes() {
    TYPED_SLOT_DESCRIPTOR_PROBES.with(|c| c.set(0));
}

#[cfg(test)]
pub(super) fn test_typed_slot_descriptor_probes() -> usize {
    TYPED_SLOT_DESCRIPTOR_PROBES.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn test_reset_typed_raw_f64_descriptor_queries() {
    TYPED_RAW_F64_DESCRIPTOR_QUERIES.with(|c| c.set(0));
}

#[cfg(test)]
pub(crate) fn test_typed_raw_f64_descriptor_queries() -> usize {
    TYPED_RAW_F64_DESCRIPTOR_QUERIES.with(Cell::get)
}
