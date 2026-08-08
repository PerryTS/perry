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
/// strings and got byte-identical correct output under `PERRY_GC_ZEAL=1
/// PERRY_GC_PROTECT_FROMSPACE=1` and `PERRY_GC_FORCE_EVACUATE=1`, because
/// `js_json_parse` is LAZY for 1 KB–16 MB top-level arrays (`json_tape`) and the
/// probe read its records only after the last GC. Under `PERRY_JSON_TAPE=0` the
/// same sabotage SIGSEGVs. So:
///
/// - "clean under zeal + from-space protect" is evidence only once you have
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
//                      object's `keys_array`
// holds at all times. (Before #6893 the descriptor was always the per-object
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

#[derive(Clone, PartialEq, Eq)]
pub(super) enum LayoutSlotMask {
    Inline(u64),
    Heap(Vec<u64>),
    /// Every currently-live slot is pointer-bearing. This is useful for
    /// runtime-produced arrays such as `String.prototype.split` results: the
    /// array grows its visible length only after each string has been stored,
    /// so the collector can visit `0..length` directly without allocating or
    /// updating a side-table bit for every element.
    AllPointers,
}

impl LayoutSlotMask {
    pub(super) fn from_words(words: &[u64]) -> Self {
        let mut trimmed = words.len();
        while trimmed > 0 && words[trimmed - 1] == 0 {
            trimmed -= 1;
        }
        match trimmed {
            0 => LayoutSlotMask::Inline(0),
            1 => LayoutSlotMask::Inline(words[0]),
            _ => LayoutSlotMask::Heap(words[..trimmed].to_vec()),
        }
    }

    #[inline]
    pub(super) fn set_slot(&mut self, slot_index: usize) {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => {
                *bits |= 1u64 << slot_index;
            }
            LayoutSlotMask::Inline(bits) => {
                let mut words = vec![0; slot_index / 64 + 1];
                words[0] = *bits;
                words[slot_index / 64] |= 1u64 << (slot_index % 64);
                *self = LayoutSlotMask::Heap(words);
            }
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                if words.len() <= word {
                    words.resize(word + 1, 0);
                }
                words[word] |= 1u64 << (slot_index % 64);
            }
            LayoutSlotMask::AllPointers => {}
        }
    }

    #[inline]
    pub(super) fn clear_slot(&mut self, slot_index: usize) {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => {
                *bits &= !(1u64 << slot_index);
            }
            LayoutSlotMask::Inline(_) => {}
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                if word < words.len() {
                    words[word] &= !(1u64 << (slot_index % 64));
                    while words.last().copied() == Some(0) {
                        words.pop();
                    }
                    if words.len() == 1 {
                        *self = LayoutSlotMask::Inline(words[0]);
                    }
                }
            }
            // `layout_note_slot` must downgrade an all-pointer layout before
            // clearing a slot, because this variant intentionally stores no
            // per-slot bitmap from which to reconstruct the remaining set.
            LayoutSlotMask::AllPointers => {
                unreachable!("all-pointer layouts must be downgraded before clearing a slot")
            }
        }
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        match self {
            LayoutSlotMask::Inline(bits) => *bits == 0,
            LayoutSlotMask::Heap(words) => words.iter().all(|&w| w == 0),
            LayoutSlotMask::AllPointers => false,
        }
    }

    pub(super) fn visit_slots<F: FnMut(usize)>(&self, slot_count: usize, mut visit: F) {
        match self {
            LayoutSlotMask::Inline(bits) => {
                let limit = slot_count.min(64);
                let mask = if limit == 64 {
                    u64::MAX
                } else if limit == 0 {
                    0
                } else {
                    (1u64 << limit) - 1
                };
                let mut word = *bits & mask;
                while word != 0 {
                    let bit = word.trailing_zeros() as usize;
                    visit(bit);
                    word &= word - 1;
                }
            }
            LayoutSlotMask::Heap(words) => {
                let word_count = slot_count.div_ceil(64);
                for (word_index, &raw_word) in words.iter().take(word_count).enumerate() {
                    let remaining = slot_count.saturating_sub(word_index * 64);
                    let limit = remaining.min(64);
                    let mask = if limit == 64 {
                        u64::MAX
                    } else if limit == 0 {
                        0
                    } else {
                        (1u64 << limit) - 1
                    };
                    let mut word = raw_word & mask;
                    while word != 0 {
                        let bit = word.trailing_zeros() as usize;
                        visit(word_index * 64 + bit);
                        word &= word - 1;
                    }
                }
            }
            LayoutSlotMask::AllPointers => {
                for slot in 0..slot_count {
                    visit(slot);
                }
            }
        }
    }

    pub(super) fn count_slots(&self, slot_count: usize) -> usize {
        let mut count = 0usize;
        self.visit_slots(slot_count, |_| {
            count += 1;
        });
        count
    }

    /// Reference implementation only. The construction path asks this question
    /// of the raw mask words (`shape_install::words_intersect`) rather than of
    /// two built masks — see that module's "mask words" section — and
    /// `shape_install::tests::mask_word_helpers_agree_with_layout_slot_mask`
    /// pins the two together, which is what this now exists for.
    #[cfg(test)]
    pub(super) fn intersects(&self, other: &Self, slot_count: usize) -> bool {
        let mut found = false;
        self.visit_slots(slot_count, |slot| {
            if other.contains_slot(slot) {
                found = true;
            }
        });
        found
    }

    #[inline]
    pub(super) fn contains_slot(&self, slot_index: usize) -> bool {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => (*bits & (1u64 << slot_index)) != 0,
            LayoutSlotMask::Inline(_) => false,
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                word < words.len() && (words[word] & (1u64 << (slot_index % 64))) != 0
            }
            LayoutSlotMask::AllPointers => true,
        }
    }

    pub(super) fn next_slot_at_or_after(&self, cursor: usize, slot_count: usize) -> Option<usize> {
        if cursor >= slot_count {
            return None;
        }
        match self {
            LayoutSlotMask::Inline(bits) => {
                if cursor >= 64 {
                    return None;
                }
                let limit = slot_count.min(64);
                let limit_mask = if limit == 64 {
                    u64::MAX
                } else if limit == 0 {
                    0
                } else {
                    (1u64 << limit) - 1
                };
                let cursor_mask = u64::MAX << cursor;
                let word = *bits & limit_mask & cursor_mask;
                (word != 0).then(|| word.trailing_zeros() as usize)
            }
            LayoutSlotMask::Heap(words) => {
                let mut word_index = cursor / 64;
                let word_count = slot_count.div_ceil(64);
                while word_index < word_count && word_index < words.len() {
                    let word_start = word_index * 64;
                    let remaining = slot_count.saturating_sub(word_start);
                    let limit = remaining.min(64);
                    let limit_mask = if limit == 64 {
                        u64::MAX
                    } else if limit == 0 {
                        0
                    } else {
                        (1u64 << limit) - 1
                    };
                    let cursor_mask = if word_index == cursor / 64 {
                        u64::MAX << (cursor % 64)
                    } else {
                        u64::MAX
                    };
                    let word = words[word_index] & limit_mask & cursor_mask;
                    if word != 0 {
                        return Some(word_start + word.trailing_zeros() as usize);
                    }
                    word_index += 1;
                }
                None
            }
            LayoutSlotMask::AllPointers => (cursor < slot_count).then_some(cursor),
        }
    }
}

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
}

// #6893: SHAPE-keyed canonical typed layout. Replaces the per-OBJECT
// TYPED_LAYOUTS + LAYOUT_SLOT_MASKS storage for the common case where an
// object's live layout matches its shape (header `GC_OBJ_TYPED_LAYOUT_INTACT`).
// Keyed by the shared `keys_array` pointer — all same-shape objects share ONE
// canonical keys array ("shared keys_array IS a shape"), so this is O(shapes),
// not O(objects). Measured: object churn stores a per-object descriptor for
// every one of ~2M `{v,w}` objects (all identical) → ~392 MB; keying by the
// (single) shared keys_array collapses that to one entry (churn peak RSS
// 830→262 MB, behaviour-identical).
//
// Value `None` = AMBIGUOUS: two live layouts share the same key NAMES but
// different value TYPES (`{v:1,w:2}` vs `{v:"a",w:"b"}`); those objects fall
// back to the per-object maps. ACCELERATOR ONLY: a miss, a stale entry
// (keys_array relocated/recycled by a moving GC), an ambiguous shape, or a
// field-count mismatch all fall back to the per-object map and then the
// conservative scan — never a wrong descriptor (mirrors the ShapeTable trust
// model). Nothing to prune on object death (entries are per-shape, shared).
thread_local! {
    pub(in crate::gc) static SHAPE_LAYOUTS: RefCell<crate::fast_hash::PtrHashMap<usize, Option<TypedLayoutDescriptor>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

fn shape_layout_keyed_enabled() -> bool {
    use std::sync::OnceLock;
    static E: OnceLock<bool> = OnceLock::new();
    // Default ON; `PERRY_SHAPE_LAYOUT_KEYED=0` restores the per-object maps
    // (A/B validation).
    *E.get_or_init(|| {
        std::env::var("PERRY_SHAPE_LAYOUT_KEYED")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

/// keys_array only exists on genuine shaped objects (`ObjectFields`). Arrays,
/// closures, RegExps etc. also flow through `layout_note_slot` /
/// `layout_visit_pointer_slots`, and reading `ObjectHeader::keys_array` off one
/// would interpret unrelated payload bytes as a pointer. Returns 0 for anything
/// that is not an ObjectFields object (⟹ callers skip the shared shape path).
#[inline]
unsafe fn object_keys_array_ptr(user_ptr: usize) -> usize {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return 0;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
        return 0;
    }
    (*(user_ptr as *const crate::object::ObjectHeader)).keys_array as usize
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
    if !shape_layout_keyed_enabled() {
        return None;
    }
    let keys = object_keys_array_ptr(user_ptr);
    if keys == 0 {
        return None;
    }
    // Defense-in-depth: the descriptor's `slot_count` is pinned to the owning
    // object's `field_count` at install (`init_typed_shape_layout` rejects a
    // mismatch). A differing current field_count means this object's shape is
    // not the one the descriptor describes — e.g. a keys_array address reused by
    // a shape with a different field count (moving-GC relocation before the new
    // address is re-installed). Fall back (per-object → conservative).
    let field_count = (*(user_ptr as *const crate::object::ObjectHeader)).field_count as usize;
    let map = hot_shape_layouts().borrow();
    let desc = map.get(&keys)?.as_ref()?;
    if desc.slot_count != field_count {
        return None;
    }
    Some(f(desc))
}

/// Cloning form of [`with_shape_shared_descriptor`], for the callers that need
/// to keep the descriptor past the `SHAPE_LAYOUTS` borrow.
#[inline]
unsafe fn shape_shared_descriptor(user_ptr: usize) -> Option<TypedLayoutDescriptor> {
    with_shape_shared_descriptor(user_ptr, |desc| desc.clone())
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
    shape_shared_descriptor(user_ptr).map(|d| d.pointer_mask)
}

/// Install `descriptor` as the canonical layout for `keys` and set the object's
/// header state (INTACT + POINTER_FREE/SIDE_MASK), WITHOUT any per-object map
/// entry. Returns `true` if the object now rides the shared shape descriptor;
/// `false` if the shape is ambiguous (caller falls back to per-object).
unsafe fn shape_install_shared(
    keys: usize,
    header: *mut GcHeader,
    descriptor: &TypedLayoutDescriptor,
) -> bool {
    let shared_ok = {
        let mut m = hot_shape_layouts().borrow_mut();
        match m.get(&keys) {
            None => {
                m.insert(keys, Some(descriptor.clone()));
                true
            }
            Some(Some(existing)) if existing == descriptor => true,
            Some(Some(_)) => {
                // Same keys, different layout ⟹ ambiguous. Poison the entry so
                // future lookups (and any still-INTACT siblings) fall back.
                m.insert(keys, None);
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
        GcLayoutSlotKind::None | GcLayoutSlotKind::ObjectMeta => None,
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
        // still tolerates a `None` defensively, so a transiently desynced bit
        // can only cost an extra fall-through, never mis-track a slot.
        if (*header)._reserved & GC_OBJ_TYPED_LAYOUT_INTACT != 0 {
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
                    let mut mask = LayoutSlotMask::Inline(0);
                    mask.set_slot(slot_index);
                    masks.insert(parent_user, mask);
                    mark_per_object_layouts_nonempty();
                    set_layout_state(header, GC_LAYOUT_SIDE_MASK);
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

/// Scalar-aware variant of [`js_gc_note_slot_layout`]: `old_bits` is the value
/// previously held in the slot. When **neither** the new value nor the old
/// value is a heap pointer, the slot's pointer-ness is unchanged, so the
/// per-slot GC layout mask needs no update — the `SIDE_MASK`/typed path's
/// thread-local hashmap touch is skipped. The mask invariant ("bit set ⟺ slot
/// holds a pointer") is preserved because the full path still runs whenever a
/// pointer is involved on either side (`new` is a pointer → set; `old` was a
/// pointer → clear), which is exactly when the mask must change. This is the
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
    if !layout_pointer_bearing_bits(value_bits) && !layout_pointer_bearing_bits(old_bits) {
        return;
    }
    let parent_user = strip_nanbox_user_ptr(parent);
    layout_note_slot(parent_user, slot_index as usize, value_bits);
}

/// How `init_typed_shape_layout` establishes the descriptor's truth.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypedShapeProof {
    /// **Observe.** The object's fields already hold their final values, so
    /// check every one against the declared masks and refuse the descriptor if
    /// any disagrees. This is the post-constructor call site.
    ValidateSlots,
    /// **Construct.** The object was allocated moments ago and its slots are
    /// still the allocator's `TAG_UNDEFINED` fill, so there is nothing to
    /// observe yet — validating would reject every raw-f64 slot (`undefined`
    /// carries a `0x7FFC` tag, inside `layout_raw_f64_bits`' reject range) and
    /// downgrade the object it was asked to describe.
    ///
    /// The caller carries the proof instead, and it is a codegen one: #7510
    /// emits this form only for a class whose constructor prologue provably
    /// assigns **every** raw-f64 field from a plain parameter before any other
    /// statement runs (`lower_call::field_init`'s #7486 predicate), so no read
    /// can observe a raw-f64 slot between here and its first write.
    ///
    /// The *collector's* half needs no proof at all: `TAG_UNDEFINED` is a
    /// non-pointer in every slot, which is consistent with both the
    /// `POINTER_FREE` and the `SIDE_MASK` state this installs.
    ///
    /// And the descriptor stays honest afterwards without the loop: a store
    /// that contradicts it is rejected by the guard's `is_plain_number_bits` /
    /// the inline path's finite-exponent test, falls back to the boxed setter,
    /// and downgrades through [`layout_note_slot`] exactly as a post-install
    /// contradiction always has.
    FreshlyAllocated,
}

/// Rebuild a mask slice from the raw `(pointer, word count)` pair the FFI
/// signature carries.
///
/// #7578 keeps the construction path on the raw pair and materialises a slice
/// only where one is actually indexed. `slice::from_raw_parts` requires a
/// non-null aligned pointer, so every call used to open with two
/// null-to-`NonNull::dangling()` `csel` chains — twelve instructions to
/// normalise two arguments that the fast path below then never dereferences.
#[inline(always)]
unsafe fn mask_words<'a>(words: *const u64, word_count: u32) -> &'a [u64] {
    if words.is_null() || word_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(words, word_count as usize)
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn init_typed_shape_layout(
    user_ptr: usize,
    slot_count: usize,
    raw_f64_words: *const u64,
    raw_f64_word_count: u32,
    pointer_words: *const u64,
    pointer_word_count: u32,
    proof: TypedShapeProof,
) {
    // One `gc_type_layout_slot_kind`, not two. `layout_header_for_user`
    // computes the kind and accepts three of them; the line that used to follow
    // it recomputed the same kind — a second load through the 32-byte-strided
    // type table — to narrow those three to one. Requiring `ObjectFields`
    // directly is exactly equivalent, because `ObjectFields` is one of the
    // three `layout_header_for_user` admits (#7578).
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
        return;
    }
    let obj_header = user_ptr as *const crate::object::ObjectHeader;
    let object_slot_count = (*obj_header).field_count as usize;
    if object_slot_count != slot_count {
        layout_set_typed_unknown(header, user_ptr);
        return;
    }

    if slot_count != 0 && proof == TypedShapeProof::ValidateSlots {
        let raw_f64_words = mask_words(raw_f64_words, raw_f64_word_count);
        let pointer_words = mask_words(pointer_words, pointer_word_count);
        let fields = (obj_header as *const u8)
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        for i in 0..slot_count {
            let bits = *fields.add(i);
            if super::shape_install::words_contain_slot(raw_f64_words, i) {
                if !layout_raw_f64_bits(bits) {
                    layout_set_typed_unknown(header, user_ptr);
                    return;
                }
                continue;
            }
            if layout_pointer_bearing_bits(bits)
                && !super::shape_install::words_contain_slot(pointer_words, i)
            {
                layout_set_typed_unknown(header, user_ptr);
                return;
            }
        }
    }

    // #7510 item 1: everything above this line is re-derived per object and
    // stays that way — it is what makes the header declaration below true.
    // What the 20-millionth `{v, w}` literal does NOT need to re-derive is the
    // *map* answer: that its shape's canonical descriptor is already installed
    // and already equal to the one these mask globals describe. That is all
    // `shape_install::hit` asserts, and on a hit the construction reduces to
    // the two header bit-writes `shape_install_shared` would have performed —
    // no `TypedLayoutDescriptor` built, cloned and dropped, no `RefCell`
    // borrow, no hash of `keys`, no field-by-field descriptor comparison.
    //
    // The memo carries no state about the OBJECT. The one bit it carries about
    // the *masks* — whether the pointer mask is empty, which selects
    // `POINTER_FREE` vs `SIDE_MASK` — is a pure function of bytes the entry has
    // already matched by address and length, and those bytes are immutable
    // program constants. See `gc::shape_install` for the full staleness
    // argument.
    //
    // `object_keys_array_ptr`'s two guards are already discharged above (the
    // low addresses were rejected, `GcLayoutSlotKind::ObjectFields` was
    // checked), so read the field directly rather than re-walking the header.
    let keys = (*obj_header).keys_array as usize;
    let memo = if keys == 0 {
        None
    } else {
        super::shape_install::hit(
            keys,
            slot_count,
            raw_f64_words,
            raw_f64_word_count,
            pointer_words,
            pointer_word_count,
        )
    };
    if let Some(pointer_mask_empty) = memo {
        header_set_typed_layout_intact(header);
        if pointer_mask_empty {
            set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        } else {
            set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        }
        layout_forget_object(user_ptr);
        return;
    }

    let raw_f64_slice = mask_words(raw_f64_words, raw_f64_word_count);
    let pointer_slice = mask_words(pointer_words, pointer_word_count);

    // #7578: the mask-disjointness check moved down here, off the hit path.
    //
    // It is a pure function of the two mask globals, and a memo hit proves an
    // install already ran it over the *same* globals and passed — a shape whose
    // masks intersect is downgraded here and never reaches `record`, so no
    // intersecting tuple can be in the table to hit. Running it above the probe
    // charged every construction of every shape for a compile-time property of
    // the class. It stays ahead of every install, which is the only place its
    // answer is used.
    if super::shape_install::words_intersect(raw_f64_slice, pointer_slice, slot_count) {
        layout_set_typed_unknown(header, user_ptr);
        return;
    }

    let pointer_mask = LayoutSlotMask::from_words(pointer_slice);
    let descriptor = TypedLayoutDescriptor {
        slot_count,
        raw_f64_mask: LayoutSlotMask::from_words(raw_f64_slice),
        pointer_mask: pointer_mask.clone(),
    };
    // #6893: try the O(shapes) shared shape descriptor (keyed by the canonical
    // keys_array) before per-object storage. `shape_layout_keyed_enabled()`
    // gates this and therefore also gates the memo above: the memo is only
    // ever populated from a successful install here, so with the knob off the
    // table stays empty and every lookup misses.
    let keys = if shape_layout_keyed_enabled() {
        keys
    } else {
        0
    };
    if keys != 0 && shape_install_shared(keys, header, &descriptor) {
        // The common path for every object literal: the shape already owns a
        // canonical descriptor, so this object needs no per-object record at
        // all. `layout_forget_object` skips the hash entirely when the maps
        // are empty, which on a monomorphic workload they are.
        //
        // `pointer_mask.is_empty()` is what the hit path above will replay from
        // the memo; `words_are_empty` is pinned equal to it by
        // `shape_install::tests::mask_word_helpers_agree_with_layout_slot_mask`.
        super::shape_install::record(
            keys,
            slot_count,
            raw_f64_words,
            raw_f64_word_count,
            pointer_words,
            pointer_word_count,
            pointer_mask.is_empty(),
        );
        layout_forget_object(user_ptr);
        return;
    }
    typed_layouts_insert(user_ptr, descriptor);
    header_set_typed_layout_intact(header);
    if pointer_mask.is_empty() {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        slot_masks_remove(user_ptr);
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        slot_masks_insert(user_ptr, pointer_mask);
    }
}

#[inline]
fn typed_shape_layout_entry(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
    proof: TypedShapeProof,
) {
    let user_ptr = strip_nanbox_user_ptr(obj);
    let slot_count = slot_count as usize;
    if user_ptr == 0 || slot_count > 16_000_000 {
        return;
    }
    unsafe {
        init_typed_shape_layout(
            user_ptr,
            slot_count,
            raw_f64_mask_words,
            raw_f64_mask_word_count,
            pointer_mask_words,
            pointer_mask_word_count,
            proof,
        );
    }
}

/// Register a constructed instance's canonical layout **after** its fields hold
/// their final values. Validates every slot before promoting.
#[no_mangle]
pub extern "C" fn js_gc_init_typed_shape_layout(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
) {
    typed_shape_layout_entry(
        obj,
        slot_count,
        raw_f64_mask_words,
        raw_f64_mask_word_count,
        pointer_mask_words,
        pointer_mask_word_count,
        TypedShapeProof::ValidateSlots,
    );
}

/// #7510: register a **freshly allocated** instance's canonical layout, before
/// its constructor runs, so the constructor's own field stores can pass the
/// `GC_OBJ_TYPED_LAYOUT_INTACT` guard.
///
/// `js_gc_init_typed_shape_layout` cannot be moved earlier: it validates that
/// each raw-f64 slot already holds a plain double, and a fresh slot holds
/// `TAG_UNDEFINED`, so an early call downgrades every instance it touches. That
/// is why a declared-`number` class field was *slower* than the equivalent
/// object literal (#7512) — the descriptor arrived after the only stores that
/// wanted it, so every one fell back to `js_put_value_set`.
///
/// This form carries the proof on the codegen side instead; see
/// [`TypedShapeProof::FreshlyAllocated`] for what it rests on and why the
/// collector's half is unconditional. **Callers must invoke it only on an
/// instance whose slots are still the allocator's fill** — the whole contract
/// is "nothing has been written yet", and it is not checkable from here.
#[no_mangle]
pub extern "C" fn js_gc_declare_typed_shape_layout(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
) {
    typed_shape_layout_entry(
        obj,
        slot_count,
        raw_f64_mask_words,
        raw_f64_mask_word_count,
        pointer_mask_words,
        pointer_mask_word_count,
        TypedShapeProof::FreshlyAllocated,
    );
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
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        slot_masks_insert(user_ptr as usize, mask);
    }
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
    // `SHAPE_LAYOUTS` entry (#6893) is keyed by the shared `keys_array`, which
    // the relocated copy carries verbatim — it needs no move, but it only
    // describes THIS object while the object is still INTACT.
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
    pub(super) payload: HeapSlotRange,
    pub(super) selection: HeapPayloadSlotSelection,
}

impl HeapChildSlotIterator {
    pub(super) fn empty() -> Self {
        Self {
            prefix_slot: None,
            meta_slot: None,
            payload: HeapSlotRange::new(std::ptr::null_mut(), 0),
            selection: HeapPayloadSlotSelection::Empty,
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
            payload,
            selection,
        }
    }

    pub(super) fn with_meta_slot(mut self, slot: Option<*mut u64>) -> Self {
        self.meta_slot = slot;
        self
    }

    pub(super) fn take_meta_child_slot(&mut self) -> Option<*mut u64> {
        self.meta_slot.take()
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
    if header.is_null() || payload.is_empty() {
        return HeapPayloadSlotSelection::Empty;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    let raw_numeric_object_slots = if (*header).obj_type == GC_TYPE_OBJECT {
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
            let mask = per_object_slot_mask(user_ptr)
                .or_else(|| shape_shared_pointer_mask(user_ptr, header));
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
            // Wall 18 follow-up: a `RegExpHeader` is allocated as
            // `GC_TYPE_OBJECT` but is a NATIVE struct, NOT a shaped JS object.
            // The generic ObjectHeader read takes `field_count` from offset 12,
            // which for a `RegExpHeader` overlaps the high 32 bits of
            // `pattern_ptr` (~900 on macOS's 0x3xx_… heap) → a bogus ~900-slot
            // range that scans/rewrites ADJACENT heap during evacuation (heap
            // corruption; `PERRY_GC_VERIFY_EVACUATION` reports it as a stale
            // forwarded pointer "inside" the regex at an offset far past its
            // size). This is a latent pre-existing bug — exposed deterministically
            // once Wall 18 grew the header. Detect the regex via its
            // self-identifying magic and scan EXACTLY its GC-visible slots —
            // `pattern_ptr`/`flags_ptr` (a 2-slot contiguous payload range) and
            // `last_index` (the prefix slot). The off-heap `regex_ptr`/`fancy_ptr`,
            // the bool flags, the `magic` sentinel, and any tail padding are never
            // inspected, so evacuation can never touch raw native data.
            if crate::regex::regex_header_has_magic(user_ptr as *const crate::regex::RegExpHeader) {
                let (pattern_slot, slot_count, last_index_slot) =
                    crate::regex::regex_gc_slot_ptrs(user_ptr as *mut crate::regex::RegExpHeader);
                let range = HeapSlotRange::new(pattern_slot, slot_count);
                return HeapChildSlotIterator::new(header, Some(last_index_slot), range);
            }
            let obj = user_ptr as *mut crate::object::ObjectHeader;
            let Some(range) = crate::object::gc_field_slot_range(obj) else {
                return HeapChildSlotIterator::empty();
            };
            let keys_slot = crate::object::gc_keys_array_slot(obj);
            // #6812: the meta record is a raw-pointer child edge; before the
            // spill buffer it was enumerated only on the rewrite path, which
            // left it invisible to MARKING (latent for custom prototypes,
            // which are usually rooted elsewhere; fatal for the spill
            // buffer, reachable through meta alone). A second prefix slot
            // keeps payload slot indices aligned with the layout masks.
            HeapChildSlotIterator::new(header, keys_slot, range)
                .with_meta_slot(crate::object::gc_object_meta_slot(user_ptr as usize))
        }
        GcLayoutSlotKind::ObjectMeta => {
            // #6812: prototype (NaN-boxed / raw / sentinel) as the prefix
            // slot, the raw spill-buffer pointer as a 1-slot range. Mirrors
            // the rewrite descriptor arm — marking must see the same edges.
            let meta = user_ptr as *mut crate::object::ObjectMeta;
            let proto_slot = Some(&mut (*meta).prototype as *mut u64);
            let range = HeapSlotRange::new(&mut (*meta).spill as *mut u64, 1);
            HeapChildSlotIterator::new(header, proto_slot, range)
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
    PointerFreeRange,
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
            GcMutableSlotDescriptor::PointerFreeRange => {}
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
