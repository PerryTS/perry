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
pub(super) const GC_LAYOUT_SIDE_MASK: u16 = 0x8000;
// Side masks are a win for larger layouts, but a memory tax for the tiny
// mixed objects that dominate JSON churn. Keep small pointer-bearing layouts
// in UNKNOWN state so tracing falls back to the legacy full-slot walk.
pub(super) const GC_LAYOUT_SIDE_MASK_MIN_SLOTS: usize = 16;

#[derive(Clone)]
pub(super) enum LayoutSlotMask {
    Inline(u64),
    Heap(Vec<u64>),
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
        }
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        match self {
            LayoutSlotMask::Inline(bits) => *bits == 0,
            LayoutSlotMask::Heap(words) => words.iter().all(|&w| w == 0),
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
        }
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
    (*header)._reserved =
        ((*header)._reserved & !GC_LAYOUT_STATE_MASK) | (state & GC_LAYOUT_STATE_MASK);
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
    let upper = bits >> 48;
    !(0x7FFC..=0x7FFF).contains(&upper)
}

#[inline]
pub(super) unsafe fn layout_header_for_user(user_ptr: usize) -> Option<*mut GcHeader> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    let obj_type = (*header).obj_type;
    matches!(obj_type, GC_TYPE_ARRAY | GC_TYPE_OBJECT | GC_TYPE_CLOSURE).then_some(header)
}

pub(super) unsafe fn layout_slot_capacity_for_user(
    header: *const GcHeader,
    user_ptr: usize,
) -> usize {
    match (*header).obj_type {
        GC_TYPE_ARRAY => (*(user_ptr as *const crate::array::ArrayHeader)).length as usize,
        GC_TYPE_OBJECT => (*(user_ptr as *const crate::object::ObjectHeader)).field_count as usize,
        GC_TYPE_CLOSURE => crate::closure::real_capture_count(
            (*(user_ptr as *const crate::closure::ClosureHeader)).capture_count,
        ) as usize,
        _ => 0,
    }
}

#[inline]
pub(super) unsafe fn layout_side_mask_worth_tracking(
    header: *const GcHeader,
    user_ptr: usize,
    slot_index: usize,
) -> bool {
    let slot_capacity = layout_slot_capacity_for_user(header, user_ptr);
    slot_index >= GC_LAYOUT_SIDE_MASK_MIN_SLOTS
        || slot_capacity >= GC_LAYOUT_SIDE_MASK_MIN_SLOTS
        || ((*header).obj_type == GC_TYPE_ARRAY && slot_capacity <= 1 && slot_index == 0)
}

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
}

pub(crate) unsafe fn layout_mark_unknown(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    let state = (*header)._reserved & GC_LAYOUT_STATE_MASK;
    if state == GC_LAYOUT_UNKNOWN {
        return;
    }
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    if state == GC_LAYOUT_POINTER_FREE {
        return;
    }
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
}

pub(crate) fn layout_clear_for_ptr(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
}

pub(super) unsafe fn layout_set_typed_unknown(header: *mut GcHeader, user_ptr: usize) {
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
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
        let pointer = layout_pointer_bearing_bits(value_bits);
        if let Some(typed) = TYPED_LAYOUTS.with(|m| m.borrow().get(&parent_user).cloned()) {
            if slot_index >= typed.slot_count {
                layout_set_typed_unknown(header, parent_user);
                return;
            }
            if typed.raw_f64_mask.contains_slot(slot_index) && !layout_raw_f64_bits(value_bits) {
                layout_set_typed_unknown(header, parent_user);
                return;
            }
            if pointer && !typed.pointer_mask.contains_slot(slot_index) {
                layout_set_typed_unknown(header, parent_user);
                return;
            }
            return;
        }
        let state = (*header)._reserved & GC_LAYOUT_STATE_MASK;
        if state == GC_LAYOUT_SIDE_MASK
            && (*header).obj_type == GC_TYPE_ARRAY
            && layout_slot_capacity_for_user(header, parent_user) < GC_LAYOUT_SIDE_MASK_MIN_SLOTS
            && !layout_side_mask_worth_tracking(header, parent_user, slot_index)
        {
            layout_set_typed_unknown(header, parent_user);
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
                } else if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE
                    && layout_side_mask_worth_tracking(header, parent_user, slot_index)
                {
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

#[no_mangle]
pub extern "C" fn js_gc_init_typed_shape_layout(
    obj: u64,
    slot_count: u32,
    mask_words: *const u64,
    mask_word_count: u32,
) {
    let user_ptr = strip_nanbox_user_ptr(obj);
    let slot_count = slot_count as usize;
    if user_ptr == 0 || slot_count > 16_000_000 {
        return;
    }
    unsafe {
        let Some(header) = layout_header_for_user(user_ptr) else {
            return;
        };
        if (*header).obj_type != GC_TYPE_OBJECT {
            return;
        }
        let obj_header = user_ptr as *const crate::object::ObjectHeader;
        let object_slot_count = (*obj_header).field_count as usize;
        if object_slot_count != slot_count {
            layout_set_typed_unknown(header, user_ptr);
            return;
        }

        let words: &[u64] = if mask_words.is_null() || mask_word_count == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(mask_words, mask_word_count as usize)
        };
        let pointer_mask = LayoutSlotMask::from_words(words);

        if slot_count != 0 {
            let fields = (obj_header as *const u8)
                .add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            for i in 0..slot_count {
                let bits = *fields.add(i);
                if layout_pointer_bearing_bits(bits) && !pointer_mask.contains_slot(i) {
                    layout_set_typed_unknown(header, user_ptr);
                    return;
                }
            }
        }

        let descriptor = TypedLayoutDescriptor {
            slot_count,
            raw_f64_mask: LayoutSlotMask::Inline(0),
            pointer_mask: pointer_mask.clone(),
        };
        TYPED_LAYOUTS.with(|m| {
            m.borrow_mut().insert(user_ptr, descriptor);
        });
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
        if (*header).obj_type != GC_TYPE_OBJECT {
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

        if slot_count != 0 {
            let fields = (obj_header as *const u8)
                .add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            for i in 0..slot_count {
                let bits = *fields.add(i);
                if raw_f64_mask.contains_slot(i) && !layout_raw_f64_bits(bits) {
                    layout_set_typed_unknown(header, user_ptr);
                    return;
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
    exact_small_mixed: bool,
) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    TYPED_LAYOUTS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
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
    } else if !exact_small_mixed && slot_count != 1 && slot_count < GC_LAYOUT_SIDE_MASK_MIN_SLOTS {
        set_layout_state(header, GC_LAYOUT_UNKNOWN);
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
    set_layout_state(new_header, state);
    TYPED_LAYOUTS.with(|m| {
        let mut typed = m.borrow_mut();
        typed.remove(&(new_user as usize));
        if let Some(layout) = typed.remove(&(old_user as usize)) {
            typed.insert(new_user as usize, layout);
        }
    });
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
                let mask = LAYOUT_SLOT_MASKS.with(|m| m.borrow().get(&user_ptr).cloned());
                let Some(mask) = mask else {
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
    PointerFree,
    Masked,
    All(HeapSlotRange),
}

#[derive(Clone)]
pub(super) enum HeapPayloadSlotSelection {
    Empty,
    PointerFree { emitted: bool },
    Masked { mask: LayoutSlotMask, cursor: usize },
    All { cursor: usize },
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
            HeapPayloadSlotSelection::PointerFree { .. } => HeapPayloadSlotScan::PointerFree,
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
            HeapPayloadSlotSelection::PointerFree { emitted } => {
                if *emitted || self.payload.is_empty() {
                    None
                } else {
                    *emitted = true;
                    record_layout_pointer_free_range_skipped(self.payload.slot_count());
                    Some(HeapChildSlot::PointerFreeRange(self.payload))
                }
            }
            HeapPayloadSlotSelection::Masked { mask, cursor } => {
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
    match (*header)._reserved & GC_LAYOUT_STATE_MASK {
        GC_LAYOUT_POINTER_FREE => HeapPayloadSlotSelection::PointerFree { emitted: false },
        GC_LAYOUT_SIDE_MASK => {
            let mask = LAYOUT_SLOT_MASKS.with(|m| m.borrow().get(&user_ptr).cloned());
            match mask {
                Some(mask) => HeapPayloadSlotSelection::Masked { mask, cursor: 0 },
                None => HeapPayloadSlotSelection::All { cursor: 0 },
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
    match (*header).obj_type {
        GC_TYPE_ARRAY => {
            let arr = user_ptr as *mut crate::array::ArrayHeader;
            crate::array::gc_element_slot_range(arr)
                .map(|range| HeapChildSlotIterator::new(header, None, range))
                .unwrap_or_else(HeapChildSlotIterator::empty)
        }
        GC_TYPE_OBJECT => {
            let obj = user_ptr as *mut crate::object::ObjectHeader;
            let Some(range) = crate::object::gc_field_slot_range(obj) else {
                return HeapChildSlotIterator::empty();
            };
            let keys_slot = crate::object::gc_keys_array_slot(obj);
            HeapChildSlotIterator::new(header, keys_slot, range)
        }
        GC_TYPE_CLOSURE => {
            let closure = user_ptr as *mut crate::closure::ClosureHeader;
            crate::closure::gc_capture_slot_range(closure)
                .map(|range| HeapChildSlotIterator::new(header, None, range))
                .unwrap_or_else(HeapChildSlotIterator::empty)
        }
        _ => HeapChildSlotIterator::empty(),
    }
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
