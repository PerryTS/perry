use super::*;

pub(super) fn assert_whole_heap_kinds() -> usize {
    let mut checked = 0;
    let mut inspect = |header: *mut GcHeader| unsafe {
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        let user = header.cast::<u8>().add(GC_HEADER_SIZE);
        let range = match (*header).obj_type {
            GC_TYPE_ARRAY => crate::array::gc_element_slot_range(user.cast()),
            GC_TYPE_CLOSURE => crate::closure::gc_capture_slot_range(user.cast()),
            _ => return,
        };
        let Some(range) = range else {
            return;
        };
        checked += 1;
        let flags = (*header)._reserved;
        for i in 0..range.slot_count() {
            let bits = *range.slot(i);
            let pointer = layout_pointer_bearing_bits(bits);
            if flags & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
                // RAW_F64's contract gives meaning to untagged bits, including
                // subnormal doubles whose bits happen to be a live address.
                let raw_number = (*header).obj_type == GC_TYPE_ARRAY
                    && flags & (GC_ARRAY_RAW_F64_LAYOUT | GC_ARRAY_RAW_F64_HOLES) != 0
                    && bits >> 48 < 0x7FF8;
                assert!(
                    !pointer || raw_number,
                    "NUMBERS owner {user:p}, slot {i}, bits={bits:#x}"
                );
            }
            if flags & GC_LAYOUT_STATE_MASK == GC_LAYOUT_SIDE_MASK
                && flags & GC_LAYOUT_ALL_POINTERS != 0
            {
                assert!(pointer, "POINTERS owner {user:p}, slot {i}, bits={bits:#x}");
            }
        }
    };
    crate::arena::arena_walk_objects(|header| inspect(header.cast()));
    crate::arena::old_arena_walk_objects(|header| inspect(header.cast()));
    MALLOC_STATE.with(|state| {
        for &header in &state.borrow().objects {
            inspect(header);
        }
    });
    checked
}

#[test]
fn whole_heap_kind_invariant_after_forced_collections() {
    let _gc = CopyingNurseryTestGuard::new(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let mut pointers = crate::array::js_array_alloc(8);
    for _ in 0..4 {
        let text = crate::string::js_string_from_bytes(b"heap-kind-child".as_ptr(), 15);
        pointers =
            crate::array::js_array_push_f64(pointers, crate::value::js_nanbox_string(text as i64));
    }
    let numbers = crate::array::js_array_alloc_with_length(8);
    let mixed = crate::array::js_array_alloc_with_length(8);
    crate::array::js_array_set_f64(mixed, 2, crate::value::js_nanbox_pointer(pointers as i64));
    js_shadow_slot_set(0, ptr_bits(numbers as usize));
    js_shadow_slot_set(1, ptr_bits(pointers as usize));
    js_shadow_slot_set(2, ptr_bits(mixed as usize));
    for collection in 0..3 {
        let before = gc_collection_count();
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(gc_collection_count() > before);
        if collection == 0 {
            assert!(trace.copying_nursery.copied_objects > 0);
        }
        assert!(
            assert_whole_heap_kinds() >= 3,
            "the rooted subjects must be checked"
        );
    }
    let before = gc_collection_count();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    });
    assert!(gc_collection_count() > before);
    assert!(assert_whole_heap_kinds() >= 3);
}
