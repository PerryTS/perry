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
//   intact bit set  ⟹  TYPED_LAYOUTS holds this object's canonical descriptor
// holds at all times. The descriptor's raw-f64 mask is exactly the compile-time
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

#[derive(Clone)]
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

#[derive(Clone)]
pub(super) struct TypedLayoutDescriptor {
    pub(super) slot_count: usize,
    pub(super) raw_f64_mask: LayoutSlotMask,
    pub(super) pointer_mask: LayoutSlotMask,
}

// NaN-boxing tag constants (duplicated from value.rs to avoid circular deps)

thread_local! {
    pub(super) static LAYOUT_SLOT_MASKS: RefCell<crate::fast_hash::PtrHashMap<usize, LayoutSlotMask>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    pub(super) static TYPED_LAYOUTS: RefCell<crate::fast_hash::PtrHashMap<usize, TypedLayoutDescriptor>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    #[cfg(test)]
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

#[inline]
pub(super) fn strip_nanbox_user_ptr(bits: u64) -> usize {
    if (bits >> 48) >= 0x7FF8 {
        (bits & POINTER_MASK) as usize
    } else {
        bits as usize
    }
}

#[inline]
pub(super) fn layout_pointer_bearing_bits(bits: u64) -> bool {
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
        GcLayoutSlotKind::None => None,
    }
}

#[inline]
pub(crate) unsafe fn layout_init_pointer_free(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
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
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    set_layout_state(header, GC_LAYOUT_SIDE_MASK);
    (*header)._reserved |= GC_LAYOUT_ALL_POINTERS;
}

pub(crate) unsafe fn layout_mark_unknown(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    header_clear_typed_layout_intact(header);
    let state = (*header)._reserved & GC_LAYOUT_STATE_MASK;
    if state == GC_LAYOUT_UNKNOWN {
        TYPED_LAYOUTS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
        return;
    }
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    if state == GC_LAYOUT_POINTER_FREE {
        crate::typed_feedback::invalidate_representation_change(user_ptr as usize);
        return;
    }
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    crate::typed_feedback::invalidate_representation_change(user_ptr as usize);
}

pub(crate) fn layout_clear_for_ptr(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    crate::array::clear_array_numeric_layout_ptr(user_ptr);
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    clear_typed_layout_intact_for_user(user_ptr);
    if user_ptr >= GC_HEADER_SIZE + 0x1000 {
        unsafe {
            (*header_from_user_ptr(user_ptr as *const u8))._reserved &= !GC_LAYOUT_ALL_POINTERS;
        }
    }
}

pub(crate) fn layout_has_typed_descriptor(user_ptr: usize) -> bool {
    if user_ptr == 0 {
        return false;
    }
    TYPED_LAYOUTS.with(|m| m.borrow().contains_key(&user_ptr))
}

pub(super) unsafe fn layout_set_typed_unknown(header: *mut GcHeader, user_ptr: usize) {
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    header_clear_typed_layout_intact(header);
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    crate::typed_feedback::invalidate_representation_change(user_ptr);
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
        if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_UNKNOWN {
            return;
        }
        if let Some(typed) = TYPED_LAYOUTS.with(|m| m.borrow().get(&parent_user).cloned()) {
            if slot_index >= typed.slot_count {
                layout_set_typed_unknown(header, parent_user);
                return;
            }
            if typed.raw_f64_mask.contains_slot(slot_index) {
                if !layout_raw_f64_bits(value_bits) {
                    layout_set_typed_unknown(header, parent_user);
                }
                return;
            }
            let pointer = layout_pointer_bearing_bits(value_bits);
            if pointer && !typed.pointer_mask.contains_slot(slot_index) {
                layout_set_typed_unknown(header, parent_user);
                return;
            }
            return;
        }
        let pointer = layout_pointer_bearing_bits(value_bits);
        // A result array built by a runtime helper can declare that its live
        // prefix is pointer-only once, instead of growing a HashMap-backed
        // bitmap for every inserted element. Runtime construction bypasses
        // this generic write path; any later ordinary array write may create
        // holes or replace an element, so conservatively fall back to the
        // generic scan path regardless of the stored value.
        let all_pointer_layout = (*header)._reserved & GC_LAYOUT_ALL_POINTERS != 0;
        if all_pointer_layout {
            layout_mark_unknown(parent_user as *mut u8);
            return;
        }
        if !pointer && (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
            return;
        }
        LAYOUT_SLOT_MASKS.with(|m| {
            let mut masks = m.borrow_mut();
            if pointer {
                if let Some(mask) = masks.get_mut(&parent_user) {
                    mask.set_slot(slot_index);
                } else if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
                    let mut mask = LayoutSlotMask::Inline(0);
                    mask.set_slot(slot_index);
                    masks.insert(parent_user, mask);
                    set_layout_state(header, GC_LAYOUT_SIDE_MASK);
                } else {
                    set_layout_state(header, GC_LAYOUT_UNKNOWN);
                }
            } else if let Some(mask) = masks.get_mut(&parent_user) {
                mask.clear_slot(slot_index);
                if mask.is_empty() {
                    masks.remove(&parent_user);
                    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
                }
            }
        });
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

unsafe fn init_typed_shape_layout(
    user_ptr: usize,
    slot_count: usize,
    raw_f64_words: &[u64],
    pointer_words: &[u64],
) {
    let Some(header) = layout_header_for_user(user_ptr) else {
        return;
    };
    if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
        return;
    }
    let obj_header = user_ptr as *const crate::object::ObjectHeader;
    let object_slot_count = (*obj_header).field_count as usize;
    if object_slot_count != slot_count {
        layout_set_typed_unknown(header, user_ptr);
        return;
    }

    let raw_f64_mask = LayoutSlotMask::from_words(raw_f64_words);
    let pointer_mask = LayoutSlotMask::from_words(pointer_words);
    if raw_f64_mask.intersects(&pointer_mask, slot_count) {
        layout_set_typed_unknown(header, user_ptr);
        return;
    }

    if slot_count != 0 {
        let fields = (obj_header as *const u8)
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        for i in 0..slot_count {
            let bits = *fields.add(i);
            if raw_f64_mask.contains_slot(i) {
                if !layout_raw_f64_bits(bits) {
                    layout_set_typed_unknown(header, user_ptr);
                    return;
                }
                continue;
            }
            if layout_pointer_bearing_bits(bits) && !pointer_mask.contains_slot(i) {
                layout_set_typed_unknown(header, user_ptr);
                return;
            }
        }
    }

    let descriptor = TypedLayoutDescriptor {
        slot_count,
        raw_f64_mask,
        pointer_mask: pointer_mask.clone(),
    };
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().insert(user_ptr, descriptor);
    });
    header_set_typed_layout_intact(header);
    if pointer_mask.is_empty() {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&user_ptr);
        });
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().insert(user_ptr, pointer_mask);
        });
    }
}

#[no_mangle]
pub extern "C" fn js_gc_init_typed_shape_layout(
    obj: u64,
    slot_count: u32,
    raw_f64_mask_words: *const u64,
    raw_f64_mask_word_count: u32,
    pointer_mask_words: *const u64,
    pointer_mask_word_count: u32,
) {
    let user_ptr = strip_nanbox_user_ptr(obj);
    let slot_count = slot_count as usize;
    if user_ptr == 0 || slot_count > 16_000_000 {
        return;
    }
    unsafe {
        let raw_words: &[u64] = if raw_f64_mask_words.is_null() || raw_f64_mask_word_count == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(raw_f64_mask_words, raw_f64_mask_word_count as usize)
        };
        let pointer_words: &[u64] = if pointer_mask_words.is_null() || pointer_mask_word_count == 0
        {
            &[]
        } else {
            std::slice::from_raw_parts(pointer_mask_words, pointer_mask_word_count as usize)
        };
        init_typed_shape_layout(user_ptr, slot_count, raw_words, pointer_words);
    }
}

#[no_mangle]
pub extern "C" fn js_gc_init_unboxed_object_layout(
    obj: u64,
    slot_count: u32,
    raw_f64_mask: u64,
    pointer_mask: u64,
) {
    let user_ptr = strip_nanbox_user_ptr(obj);
    let slot_count = slot_count as usize;
    if user_ptr == 0 || slot_count > 64 {
        return;
    }
    unsafe {
        let Some(header) = layout_header_for_user(user_ptr) else {
            return;
        };
        if gc_type_layout_slot_kind((*header).obj_type) != GcLayoutSlotKind::ObjectFields {
            return;
        }
        let obj_header = user_ptr as *const crate::object::ObjectHeader;
        let object_slot_count = (*obj_header).field_count as usize;
        if object_slot_count != slot_count {
            layout_set_typed_unknown(header, user_ptr);
            return;
        }

        let raw_f64_mask = LayoutSlotMask::Inline(raw_f64_mask);
        let pointer_mask = LayoutSlotMask::Inline(pointer_mask);
        if raw_f64_mask.intersects(&pointer_mask, slot_count) {
            layout_set_typed_unknown(header, user_ptr);
            return;
        }

        if slot_count != 0 {
            let fields = (obj_header as *const u8)
                .add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            for i in 0..slot_count {
                let bits = *fields.add(i);
                if raw_f64_mask.contains_slot(i) {
                    if !layout_raw_f64_bits(bits) {
                        layout_set_typed_unknown(header, user_ptr);
                        return;
                    }
                    continue;
                }
                if layout_pointer_bearing_bits(bits) && !pointer_mask.contains_slot(i) {
                    layout_set_typed_unknown(header, user_ptr);
                    return;
                }
            }
        }

        let descriptor = TypedLayoutDescriptor {
            slot_count,
            raw_f64_mask,
            pointer_mask: pointer_mask.clone(),
        };
        TYPED_LAYOUTS.with(|m| {
            m.borrow_mut().insert(user_ptr, descriptor);
        });
        header_set_typed_layout_intact(header);
        if pointer_mask.is_empty() {
            set_layout_state(header, GC_LAYOUT_POINTER_FREE);
            LAYOUT_SLOT_MASKS.with(|m| {
                m.borrow_mut().remove(&user_ptr);
            });
        } else {
            set_layout_state(header, GC_LAYOUT_SIDE_MASK);
            LAYOUT_SLOT_MASKS.with(|m| {
                m.borrow_mut().insert(user_ptr, pointer_mask);
            });
        }
    }
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
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    // The rebuild reconstructs only the pointer mask (no raw-f64 layout), so the
    // object no longer has a canonical typed descriptor: drop the intact bit.
    header_clear_typed_layout_intact(header);
    if slots.is_null() || slot_count == 0 {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
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
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().insert(user_ptr as usize, mask);
        });
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
    } else {
        crate::array::clear_array_numeric_layout_ptr(new_user as usize);
    }
    let new_has_typed = TYPED_LAYOUTS.with(|m| {
        let mut typed = m.borrow_mut();
        typed.remove(&(new_user as usize));
        if let Some(layout) = typed.remove(&(old_user as usize)) {
            typed.insert(new_user as usize, layout);
            true
        } else {
            false
        }
    });
    // Keep the intact bit in lock-step with the moved descriptor. Copying GC
    // normally propagates `_reserved` (so the bit already rode along), but
    // re-sync defensively for callers that allocate the destination fresh
    // (e.g. array growth) so a stale/missing bit can never desync from the map.
    if new_has_typed {
        header_set_typed_layout_intact(new_header);
    } else {
        header_clear_typed_layout_intact(new_header);
    }
    header_clear_typed_layout_intact(old_header);
    LAYOUT_SLOT_MASKS.with(|m| {
        let mut masks = m.borrow_mut();
        masks.remove(&(new_user as usize));
        if let Some(mask) = masks.remove(&(old_user as usize)) {
            masks.insert(new_user as usize, mask);
        }
    });
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
                let mask = LAYOUT_SLOT_MASKS.with(|m| m.borrow().get(&user_ptr).cloned());
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
    TYPED_LAYOUTS.with(|m| {
        m.borrow()
            .get(&user_ptr)
            .map(|layout| {
                slot_index < layout.slot_count && layout.raw_f64_mask.contains_slot(slot_index)
            })
            .unwrap_or(false)
    })
}

fn layout_typed_raw_f64_slot_count_for_user(user_ptr: usize, slot_count: usize) -> usize {
    TYPED_LAYOUTS.with(|m| {
        m.borrow()
            .get(&user_ptr)
            .map(|layout| {
                let bounded_count = slot_count.min(layout.slot_count);
                layout.raw_f64_mask.count_slots(bounded_count)
            })
            .unwrap_or(0)
    })
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
    pub(super) payload: HeapSlotRange,
    pub(super) selection: HeapPayloadSlotSelection,
}

impl HeapChildSlotIterator {
    pub(super) fn empty() -> Self {
        Self {
            prefix_slot: None,
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
            payload,
            selection,
        }
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
            let mask = LAYOUT_SLOT_MASKS.with(|m| m.borrow().get(&user_ptr).cloned());
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
            HeapChildSlotIterator::new(header, keys_slot, range)
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
fn fixed_slot(slot: *mut u64) -> GcMutableSlotDescriptor {
    GcMutableSlotDescriptor::Slot(GcMutableSlot::new(slot, None))
}

pub(super) unsafe fn visit_gc_layout_slot_descriptors(
    header: *mut GcHeader,
    visit: &mut dyn FnMut(GcMutableSlotDescriptor),
) {
    let mut child_slots = gc_child_slots(header);
    if let Some(slot) = child_slots.take_prefix_child_slot() {
        visit(fixed_slot(slot).with_layout(HeapChildSlotReadKind::Prefix));
    }

    match child_slots.payload_scan() {
        HeapPayloadSlotScan::Empty => {}
        HeapPayloadSlotScan::PointerFree {
            raw_numeric_array,
            raw_numeric_object_slots,
        } => {
            let range = child_slots.payload;
            record_layout_pointer_free_range_skipped(range.slot_count());
            if raw_numeric_array {
                record_layout_raw_numeric_array_range_skipped(range.slot_count());
            }
            if raw_numeric_object_slots != 0 {
                record_layout_raw_numeric_object_field_range_skipped(raw_numeric_object_slots);
            }
            visit(GcMutableSlotDescriptor::PointerFreeRange);
        }
        HeapPayloadSlotScan::Masked => {
            for child_slot in child_slots {
                if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
                    visit(GcMutableSlotDescriptor::Slot(GcMutableSlot::new(
                        slot,
                        Some(layout_kind),
                    )));
                }
            }
        }
        HeapPayloadSlotScan::All(range) => visit(GcMutableSlotDescriptor::Range {
            range,
            layout_kind: Some(HeapChildSlotReadKind::Unknown),
        }),
    }
}

impl GcMutableSlotDescriptor {
    #[inline]
    fn with_layout(self, layout_kind: HeapChildSlotReadKind) -> Self {
        match self {
            GcMutableSlotDescriptor::Slot(mut slot) => {
                slot.layout_kind = Some(layout_kind);
                GcMutableSlotDescriptor::Slot(slot)
            }
            other => other,
        }
    }
}

pub(super) unsafe fn visit_gc_rewrite_slot_descriptors(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlotDescriptor),
) {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match gc_type_rewrite_descriptor_kind((*header).obj_type) {
        GcRewriteDescriptorKind::Array => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
        }
        GcRewriteDescriptorKind::Object => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
            crate::object::visit_overflow_field_slots_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
            // #2820: the recorded `Object.setPrototypeOf` value is a live
            // reference; rewrite it if the prototype object moved.
            crate::object::prototype_chain::visit_object_static_prototype_slot_mut(
                user_ptr as usize,
                |slot| {
                    visit(fixed_slot(slot));
                },
            );
        }
        GcRewriteDescriptorKind::Closure => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
            crate::closure::visit_closure_dynamic_prop_value_slots_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
            crate::closure::visit_closure_static_prototype_slot_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
        }
        GcRewriteDescriptorKind::Promise => {
            let promise = user_ptr as *mut crate::promise::Promise;
            visit(fixed_slot(&mut (*promise).value as *mut f64 as *mut u64));
            visit(fixed_slot(&mut (*promise).reason as *mut f64 as *mut u64));
            visit(fixed_slot(
                &mut (*promise).on_fulfilled as *mut _ as *mut u64,
            ));
            visit(fixed_slot(
                &mut (*promise).on_rejected as *mut _ as *mut u64,
            ));
            visit(fixed_slot(&mut (*promise).next as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Error => {
            let error = user_ptr as *mut crate::error::ErrorHeader;
            visit(fixed_slot(&mut (*error).message as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).name as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).stack as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).cause as *mut f64 as *mut u64));
            visit(fixed_slot(&mut (*error).errors as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Map => {
            let map = user_ptr as *mut crate::map::MapHeader;
            let size = (*map).size;
            let capacity = (*map).capacity;
            // Corruption guard only: mirror Set's 16M bound (set.rs
            // gc_element_slot_range). Every GC walk (mark, copy, rewrite,
            // dirty-scan, verify) funnels through this descriptor, so a
            // lower cap makes larger maps invisible to the collector —
            // entries reachable only through a >cap map would be swept
            // while live and never rewritten after a move.
            if size > capacity || size > 16_000_000 || (*map).entries.is_null() {
                return;
            }
            visit(GcMutableSlotDescriptor::Range {
                range: HeapSlotRange::new((*map).entries as *mut u64, size as usize * 2),
                layout_kind: None,
            });
        }
        GcRewriteDescriptorKind::Set => {
            let set = user_ptr as *mut crate::set::SetHeader;
            if let Some(range) = crate::set::gc_element_slot_range(set) {
                visit(GcMutableSlotDescriptor::Range {
                    range,
                    layout_kind: None,
                });
            }
        }
        GcRewriteDescriptorKind::LazyArray => {
            let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
            if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
                return;
            }
            visit(fixed_slot(&mut (*lazy).blob_str as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*lazy).materialized as *mut _ as *mut u64));
            visit(fixed_slot(
                &mut (*lazy).materialized_elements as *mut _ as *mut u64,
            ));
            visit(fixed_slot(
                &mut (*lazy).materialized_bitmap as *mut _ as *mut u64,
            ));

            let cached_length = (*lazy).cached_length as usize;
            let cache = (*lazy).materialized_elements;
            let bitmap = (*lazy).materialized_bitmap;
            if cache.is_null() || bitmap.is_null() || cached_length == 0 {
                return;
            }
            let bitmap_words = cached_length.div_ceil(64);
            for w in 0..bitmap_words {
                let word = *bitmap.add(w);
                if word == 0 {
                    continue;
                }
                let base_idx = w * 64;
                for b in 0..64usize {
                    if word & (1u64 << b) == 0 {
                        continue;
                    }
                    let i = base_idx + b;
                    if i >= cached_length {
                        break;
                    }
                    visit(fixed_slot(cache.add(i) as *mut u64));
                }
            }
        }
        GcRewriteDescriptorKind::NativeTypedView => {
            let view = user_ptr as *mut crate::native_arena::NativeTypedViewHeader;
            visit(fixed_slot(&mut (*view).owner as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::NativePodView => {
            let view = user_ptr as *mut crate::native_arena::NativePodViewHeader;
            visit(fixed_slot(&mut (*view).owner as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Leaf => {}
    }
}

pub(super) unsafe fn visit_gc_rewrite_slots(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlot),
) {
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        descriptor.visit_slots(&mut visit);
    });
}

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
