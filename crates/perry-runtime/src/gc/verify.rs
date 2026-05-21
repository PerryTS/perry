use super::*;

pub(super) fn try_rewrite_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let tag = bits & TAG_MASK;
    let (ptr_addr, is_nanbox) = match tag {
        t if t == POINTER_TAG || t == STRING_TAG || t == BIGINT_TAG => {
            ((bits & POINTER_MASK) as usize, true)
        }
        _ => {
            // Reject NaN-tagged non-pointer values (numbers,
            // booleans, undefined, null, SSO, INT32, handles).
            if tag >= 0x7FF8_0000_0000_0000 {
                return None;
            }
            // Raw pointer fallback: lower 48 bits valid range.
            if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
                return None;
            }
            (bits as usize, false)
        }
    };
    let new_user = try_rewrite_raw_addr(ptr_addr, valid_ptrs)?;
    Some(if is_nanbox {
        tag | (new_user as u64 & POINTER_MASK)
    } else {
        new_user as u64
    })
}

pub(super) fn try_rewrite_nanboxed_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
        return None;
    }
    let ptr_addr = (bits & POINTER_MASK) as usize;
    let new_user = try_rewrite_raw_addr(ptr_addr, valid_ptrs)?;
    Some(tag | (new_user as u64 & POINTER_MASK))
}

pub(super) fn try_rewrite_raw_addr(ptr_addr: usize, valid_ptrs: &ValidPointerSet) -> Option<usize> {
    if ptr_addr == 0 {
        return None;
    }
    let mut current = ptr_addr;
    let mut rewrote = false;
    for _ in 0..64 {
        if !valid_ptrs.contains(&current) {
            return rewrote.then_some(current);
        }
        unsafe {
            let header = (current as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            if (*header).gc_flags & GC_FLAG_FORWARDED == 0 {
                return rewrote.then_some(current);
            }
            let next = forwarding_address(header) as usize;
            if next == 0 || next == current {
                return rewrote.then_some(current);
            }
            current = next;
            rewrote = true;
        }
    }
    rewrote.then_some(current)
}

#[cold]
pub(super) fn panic_stale_forwarded_reference(
    surface: &str,
    slot_addr: usize,
    old_bits: u64,
    new_bits: u64,
) -> ! {
    panic!(
        "gc evacuation verification failed: stale forwarded pointer in {surface}: slot=0x{slot_addr:x} old=0x{old_bits:x} forwarded_to=0x{new_bits:x}"
    );
}

/// In-place rewrite helper: read `*slot`, run it through
/// `try_rewrite_value`, write back if a rewrite was produced.
#[inline]
pub(super) unsafe fn rewrite_slot(slot: *mut u64, valid_ptrs: &ValidPointerSet) {
    let bits = *slot;
    if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
        *slot = new_bits;
    }
}

#[inline]
pub(super) unsafe fn verify_slot(slot: *const u64, valid_ptrs: &ValidPointerSet, surface: &str) {
    let bits = *slot;
    if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
        panic_stale_forwarded_reference(surface, slot as usize, bits, new_bits);
    }
}

pub(super) unsafe fn rewrite_gc_child_slots(header: *mut GcHeader, valid_ptrs: &ValidPointerSet) {
    for child_slot in gc_child_slots(header) {
        if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
            record_layout_child_slot_read(layout_kind);
            rewrite_slot(slot, valid_ptrs);
        }
    }
}

pub(super) unsafe fn rewrite_array_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    rewrite_gc_child_slots(header, valid_ptrs);
}

pub(super) unsafe fn rewrite_object_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    rewrite_gc_child_slots(header, valid_ptrs);
}

pub(super) unsafe fn rewrite_map_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 {
        return;
    }
    let entries = (*map).entries as *mut u64;
    if entries.is_null() {
        return;
    }
    for i in 0..(size as usize) {
        rewrite_slot(entries.add(i * 2), valid_ptrs);
        rewrite_slot(entries.add(i * 2 + 1), valid_ptrs);
    }
}

pub(super) unsafe fn rewrite_closure_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    rewrite_gc_child_slots(header, valid_ptrs);
    crate::closure::visit_closure_dynamic_prop_value_slots_mut(user_ptr as usize, |slot| {
        rewrite_slot(slot, valid_ptrs);
    });
}

pub(super) unsafe fn rewrite_promise_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let promise = user_ptr as *mut crate::promise::Promise;
    rewrite_slot(&(*promise).value as *const f64 as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).reason as *const f64 as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).on_fulfilled as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).on_rejected as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).next as *const _ as *mut u64, valid_ptrs);
}

pub(super) unsafe fn rewrite_error_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let error = user_ptr as *mut crate::error::ErrorHeader;
    rewrite_slot(&(*error).message as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).name as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).stack as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).cause as *const f64 as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).errors as *const _ as *mut u64, valid_ptrs);
}

pub(super) unsafe fn rewrite_lazy_array_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    rewrite_slot(&(*lazy).blob_str as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*lazy).materialized as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(
        &(*lazy).materialized_elements as *const _ as *mut u64,
        valid_ptrs,
    );
    rewrite_slot(
        &(*lazy).materialized_bitmap as *const _ as *mut u64,
        valid_ptrs,
    );
    // Walk cached materialized JSValues — each holds a NaN-boxed
    // pointer to a backing object that may itself be forwarded.
    let cached_length = (*lazy).cached_length as usize;
    let cache = (*lazy).materialized_elements;
    let bitmap = (*lazy).materialized_bitmap;
    if !cache.is_null() && !bitmap.is_null() && cached_length > 0 {
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
                let slot = cache.add(i) as *mut u64;
                rewrite_slot(slot, valid_ptrs);
            }
        }
    }
}

pub(super) unsafe fn rewrite_heap_object_fields(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match (*header).obj_type {
        GC_TYPE_ARRAY => rewrite_array_fields(user_ptr, valid_ptrs),
        GC_TYPE_OBJECT => rewrite_object_fields(user_ptr, valid_ptrs),
        GC_TYPE_CLOSURE => rewrite_closure_fields(user_ptr, valid_ptrs),
        GC_TYPE_PROMISE => rewrite_promise_fields(user_ptr, valid_ptrs),
        GC_TYPE_ERROR => rewrite_error_fields(user_ptr, valid_ptrs),
        GC_TYPE_MAP => rewrite_map_fields(user_ptr, valid_ptrs),
        GC_TYPE_LAZY_ARRAY => rewrite_lazy_array_fields(user_ptr, valid_ptrs),
        GC_TYPE_STRING | GC_TYPE_BIGINT | GC_TYPE_BUFFER | GC_TYPE_TYPED_ARRAY => {}
        _ => {}
    }
}

// Evacuation copies land in OLD_ARENA after the remembered-set scan
// for this cycle has already run. Rebuild only the pages for copied
// old objects that still hold nursery children so the next minor GC
// sees those old→young edges after the normal collection clear.
#[inline]
pub(super) unsafe fn remember_evacuated_old_to_young_slot(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    slot: *mut u64,
) {
    if slot.is_null() {
        return;
    }
    let child_addr = decode_heap_addr(*slot);
    if child_addr == 0 || !crate::arena::pointer_in_nursery(child_addr) {
        return;
    }
    let external = !matches!(
        crate::arena::classify_heap_generation(slot as usize),
        crate::arena::HeapGeneration::Old
    );
    sticky.remember_slot(parent_header, slot, external);
}

pub(super) unsafe fn remember_evacuated_gc_child_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
) {
    for child_slot in gc_child_slots(header) {
        if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
            record_layout_child_slot_read(layout_kind);
            remember_evacuated_old_to_young_slot(sticky, header, slot);
        }
    }
}

pub(super) unsafe fn remember_evacuated_array_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    _user_ptr: *mut u8,
) {
    remember_evacuated_gc_child_slots(sticky, header);
}

pub(super) unsafe fn remember_evacuated_object_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    _user_ptr: *mut u8,
) {
    remember_evacuated_gc_child_slots(sticky, header);
}

pub(super) unsafe fn remember_evacuated_closure_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    _user_ptr: *mut u8,
) {
    remember_evacuated_gc_child_slots(sticky, header);
}

pub(super) unsafe fn remember_evacuated_promise_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let promise = user_ptr as *mut crate::promise::Promise;
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).value as *const f64 as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).reason as *const f64 as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).on_fulfilled as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).on_rejected as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(sticky, header, &(*promise).next as *const _ as *mut u64);
}

pub(super) unsafe fn remember_evacuated_error_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let error = user_ptr as *mut crate::error::ErrorHeader;
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).message as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).name as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).stack as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).cause as *const f64 as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).errors as *const _ as *mut u64);
}

pub(super) unsafe fn remember_evacuated_map_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 || (*map).entries.is_null() {
        return;
    }
    let entries = (*map).entries as *mut u64;
    for i in 0..(size as usize) {
        remember_evacuated_old_to_young_slot(sticky, header, entries.add(i * 2));
        remember_evacuated_old_to_young_slot(sticky, header, entries.add(i * 2 + 1));
    }
}

pub(super) unsafe fn remember_evacuated_lazy_array_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    remember_evacuated_old_to_young_slot(sticky, header, &(*lazy).blob_str as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*lazy).materialized as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*lazy).materialized_elements as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*lazy).materialized_bitmap as *const _ as *mut u64,
    );

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
            remember_evacuated_old_to_young_slot(sticky, header, cache.add(i) as *mut u64);
        }
    }
}

pub(super) unsafe fn remember_evacuated_old_copy_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
) {
    if header.is_null() {
        return;
    }
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 || flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    if !crate::arena::pointer_in_old_gen(user_ptr as usize) {
        return;
    }
    match (*header).obj_type {
        GC_TYPE_ARRAY => remember_evacuated_array_young_slots(sticky, header, user_ptr),
        GC_TYPE_OBJECT => remember_evacuated_object_young_slots(sticky, header, user_ptr),
        GC_TYPE_CLOSURE => remember_evacuated_closure_young_slots(sticky, header, user_ptr),
        GC_TYPE_PROMISE => remember_evacuated_promise_young_slots(sticky, header, user_ptr),
        GC_TYPE_ERROR => remember_evacuated_error_young_slots(sticky, header, user_ptr),
        GC_TYPE_MAP => remember_evacuated_map_young_slots(sticky, header, user_ptr),
        GC_TYPE_LAZY_ARRAY => remember_evacuated_lazy_array_young_slots(sticky, header, user_ptr),
        GC_TYPE_STRING | GC_TYPE_BIGINT | GC_TYPE_BUFFER | GC_TYPE_TYPED_ARRAY => {}
        _ => {}
    }
}

pub(super) fn rebuild_evacuated_old_to_young_remembered_set(
    evacuated_headers: &[*mut GcHeader],
) -> StickyRememberedSet {
    let mut sticky = StickyRememberedSet::default();
    for &header in evacuated_headers {
        unsafe {
            remember_evacuated_old_copy_young_slots(&mut sticky, header);
        }
    }
    sticky
}

pub(super) unsafe fn verify_gc_child_slots(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    for child_slot in gc_child_slots(header) {
        if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
            record_layout_child_slot_read(layout_kind);
            verify_slot(slot, valid_ptrs, surface);
        }
    }
}

pub(super) unsafe fn verify_array_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    verify_gc_child_slots(header, valid_ptrs, surface);
}

pub(super) unsafe fn verify_object_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    verify_gc_child_slots(header, valid_ptrs, surface);
}

pub(super) unsafe fn verify_map_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 || (*map).entries.is_null() {
        return;
    }
    let entries = (*map).entries as *const u64;
    for i in 0..(size as usize) {
        verify_slot(entries.add(i * 2), valid_ptrs, surface);
        verify_slot(entries.add(i * 2 + 1), valid_ptrs, surface);
    }
}

pub(super) unsafe fn verify_closure_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    verify_gc_child_slots(header, valid_ptrs, surface);
    crate::closure::visit_closure_dynamic_prop_value_slots_mut(user_ptr as usize, |slot| {
        verify_slot(slot as *const u64, valid_ptrs, surface);
    });
}

pub(super) unsafe fn verify_promise_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let promise = user_ptr as *const crate::promise::Promise;
    verify_slot(
        &(*promise).value as *const f64 as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).reason as *const f64 as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).on_fulfilled as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).on_rejected as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).next as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
}

pub(super) unsafe fn verify_error_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let error = user_ptr as *const crate::error::ErrorHeader;
    verify_slot(
        &(*error).message as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).name as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).stack as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).cause as *const f64 as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).errors as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
}

pub(super) unsafe fn verify_lazy_array_fields(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    surface: &str,
) {
    let lazy = user_ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    verify_slot(
        &(*lazy).blob_str as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*lazy).materialized as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*lazy).materialized_elements as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*lazy).materialized_bitmap as *const _ as *const u64,
        valid_ptrs,
        surface,
    );

    let cached_length = (*lazy).cached_length as usize;
    let cache = (*lazy).materialized_elements;
    let bitmap = (*lazy).materialized_bitmap;
    if !cache.is_null() && !bitmap.is_null() && cached_length > 0 {
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
                verify_slot(cache.add(i) as *const u64, valid_ptrs, surface);
            }
        }
    }
}

pub(super) unsafe fn verify_heap_object_fields(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    surface: &'static str,
) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match (*header).obj_type {
        GC_TYPE_ARRAY => verify_array_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_OBJECT => verify_object_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_CLOSURE => verify_closure_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_PROMISE => verify_promise_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_ERROR => verify_error_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_MAP => verify_map_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_LAZY_ARRAY => verify_lazy_array_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_STRING | GC_TYPE_BIGINT | GC_TYPE_BUFFER | GC_TYPE_TYPED_ARRAY => {}
        _ => {}
    }
}

/// Walk every live (MARKED, non-FORWARDED) object on the heap and
/// rewrite any forwarded references in its fields. Includes new
/// evac copies (marked at evac time) and surviving non-evacuated
/// objects.
pub(super) fn rewrite_heap_objects(valid_ptrs: &ValidPointerSet) {
    let rewrite_one = |header: *mut GcHeader| {
        unsafe {
            let flags = (*header).gc_flags;
            // FORWARDED originals are stale — first 8 bytes of
            // payload now holds the forwarding address, not real
            // field data. Skip them entirely.
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Skip dead objects — sweep is about to free them.
            if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                return;
            }
            rewrite_heap_object_fields(header, valid_ptrs);
        }
    };
    crate::arena::arena_walk_objects(|hp| rewrite_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            rewrite_one(h);
        }
    });
}

pub(super) fn rewrite_remembered_dirty_ranges(valid_ptrs: &ValidPointerSet) {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats::default();
    let mut rewrite_dirty_slot = |slot: *mut u64, _stats: &mut RememberedSetTraceStats| unsafe {
        rewrite_slot(slot, valid_ptrs);
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut rewrite_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            rewrite_heap_object_fields(header_addr as *mut GcHeader, valid_ptrs);
        }
    }
}

/// Walk every mutable root slot and rewrite forwarded pointers.
/// Shadow slots are NaN-boxed JSValues; globals can be NaN-boxed or
/// raw object-start pointers. `try_rewrite_value` handles both forms.
pub(super) fn rewrite_mutable_root_slots(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
            slot.write(new_bits);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                if let Some(stats) = shadow_stats.as_mut() {
                    stats.record_rewrite();
                }
            }
        }
    });
}

pub(super) fn rewrite_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_rewrite(valid_ptrs);
    for scanner in scanners {
        scanner(&mut visitor);
    }
    visit_ffi_mutable_registered_roots(&mut visitor);
}

pub(super) fn verify_mutable_root_slots(valid_ptrs: &ValidPointerSet) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
            let surface = match slot.kind {
                MutableRootSlotKind::ShadowStack => "shadow stack roots",
                MutableRootSlotKind::GlobalRoot => "global roots",
            };
            panic_stale_forwarded_reference(surface, slot.ptr as usize, bits, new_bits);
        }
    });
}

pub(super) fn verify_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_verify(valid_ptrs, "runtime mutable root scanner");
    for scanner in scanners {
        scanner(&mut visitor);
    }
    visit_ffi_mutable_registered_roots(&mut visitor);
}

pub(super) fn verify_copy_only_scanner_bits(
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    surface: &'static str,
) {
    if let Some(new_bits) = try_rewrite_nanboxed_value(bits, valid_ptrs) {
        panic_stale_forwarded_reference(surface, 0, bits, new_bits);
    }
}

pub(super) struct RegisteredRootVerifyContext {
    pub(super) valid_ptrs: *const ValidPointerSet,
}

pub(super) extern "C" fn perry_ffi_verify_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const RegisteredRootVerifyContext) };
    if ctx.valid_ptrs.is_null() {
        return;
    }
    let valid_ptrs = unsafe { &*ctx.valid_ptrs };
    verify_copy_only_scanner_bits(value.to_bits(), valid_ptrs, "ffi copy-only root scanner");
}

pub(super) fn verify_copy_only_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
    for scanner in scanners {
        scanner(&mut |value: f64| {
            verify_copy_only_scanner_bits(value.to_bits(), valid_ptrs, "copy-only root scanner");
        });
    }

    let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut ctx = RegisteredRootVerifyContext {
        valid_ptrs: valid_ptrs as *const ValidPointerSet,
    };
    let ctx = &mut ctx as *mut RegisteredRootVerifyContext as *mut c_void;
    for scanner in ffi_scanners {
        scanner(perry_ffi_verify_root, ctx);
    }
}

pub(super) fn verify_remembered_dirty_ranges(valid_ptrs: &ValidPointerSet) {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats::default();
    let mut verify_dirty_slot = |slot: *mut u64, _stats: &mut RememberedSetTraceStats| unsafe {
        verify_slot(slot as *const u64, valid_ptrs, "remembered dirty ranges");
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut verify_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            verify_heap_object_fields(
                header_addr as *mut GcHeader,
                valid_ptrs,
                "remembered fallback headers",
            );
        }
    }
}

pub(super) fn verify_heap_objects(valid_ptrs: &ValidPointerSet) {
    let verify_one = |header: *mut GcHeader| unsafe {
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            return;
        }
        verify_heap_object_fields(header, valid_ptrs, "heap fields");
    };
    crate::arena::arena_walk_objects(|hp| verify_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            verify_one(h);
        }
    });
}

pub(super) fn verify_evacuated_no_stale_forwarded_refs(valid_ptrs: &ValidPointerSet) {
    verify_mutable_root_slots(valid_ptrs);
    verify_mutable_registered_roots(valid_ptrs);
    verify_copy_only_registered_roots(valid_ptrs);
    verify_remembered_dirty_ranges(valid_ptrs);
    verify_heap_objects(valid_ptrs);
}

/// Top-level Phase C4b-γ-2 entry: rewrite every reference site we
/// own. Skipped: conservatively-discovered C-stack words (we can't
/// safely overwrite arbitrary stack memory; pinning of conservative-
/// root targets in `gc_collect_minor` keeps those references valid
/// without rewriting). Legacy copy-only scanners still pin their own
/// discoveries directly during root marking.
pub(super) fn rewrite_forwarded_references(
    valid_ptrs: &ValidPointerSet,
    shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    rewrite_mutable_root_slots(valid_ptrs, shadow_stats);
    rewrite_mutable_registered_roots(valid_ptrs);
    rewrite_remembered_dirty_ranges(valid_ptrs);
    rewrite_heap_objects(valid_ptrs);
}

/// Gen-GC Phase C4b: is `header` pinned this cycle (cannot be
/// evacuated)? Tested by the evacuation candidate filter in
/// `gc_collect_minor` after the age-bump pass.
#[inline]
pub fn is_conservatively_pinned(header: *const GcHeader) -> bool {
    CONS_PINNED.with(|s| s.borrow().contains(&(header as usize)))
}

/// Test-only diagnostic: number of objects pinned this cycle.
pub fn cons_pinned_count() -> usize {
    CONS_PINNED.with(|s| s.borrow().len())
}
