use super::*;

fn run_until(state: &mut GcCycleState, target: GcCyclePhase) {
    for _ in 0..100_000 {
        if state.phase() == target {
            return;
        }
        state.step(GcWorkBudget::bounded(1));
    }
    panic!("cycle failed to reach {target:?}");
}

#[test]
fn numbers_to_tagged_during_incremental_mark() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (array, slots) = unsafe { alloc_old_test_array(8) };
    unsafe {
        layout_init_pointer_free(array.cast());
    }
    js_shadow_slot_set(0, ptr_bits(array as usize));
    let child = crate::string::js_string_from_bytes(b"incremental-child".as_ptr(), 17) as usize;
    let mut state = GcCycleState::new_full(GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    });
    run_until(&mut state, GcCyclePhase::BlockPersistence);
    assert!(
        incremental_mark_barrier_active(),
        "this must be a live incremental mark"
    );
    unsafe {
        let owner_header = header_from_user_ptr(array.cast());
        assert_ne!(
            (*owner_header).gc_flags & GC_FLAG_MARKED,
            0,
            "NUMBERS parent must already have been marked"
        );
        assert_eq!(
            test_heap_child_slot_count(array.cast()),
            0,
            "its earlier slice skipped the payload"
        );
        // Keep the subject white even if recent-block persistence marked its block.
        (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_MARKED;
    }
    runtime_store_jsvalue_slot(
        array as usize,
        unsafe { slots.add(1) } as usize,
        1,
        string_bits(child),
    );
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(child as *const u8)).gc_flags & GC_FLAG_MARKED,
            0,
            "the child shade must cover a newly installed edge"
        );
        assert_eq!(
            (*header_from_user_ptr(array.cast()))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN
        );
    }
    run_until(&mut state, GcCyclePhase::Complete);
    assert!(
        state.take_outcome().is_some(),
        "the full cycle must complete"
    );
    let before = gc_collection_count();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(gc_collection_count() > before);
    assert!(trace.copying_nursery.copied_objects > 0);
    let bits = crate::array::js_array_get_f64(array, 1).to_bits();
    assert_ne!((bits & POINTER_MASK) as usize, child);
    unsafe {
        assert_string_bytes(
            (bits & POINTER_MASK) as *const crate::StringHeader,
            b"incremental-child",
        );
    }
}

#[test]
fn numbers_to_tagged_through_forwarded_array() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let stub = crate::array::js_array_alloc_with_length(8);
    crate::array::js_array_set_f64(stub, 1, 1.5);
    unsafe {
        assert!(crate::array::rebuild_array_numeric_raw_f64_allow_holes(
            stub
        ));
    }
    let target = crate::array::js_array_grow(stub, 128);
    assert_ne!(target, stub);
    let child = crate::string::js_string_from_bytes(b"forwarded-child".as_ptr(), 15) as usize;
    // The generated helper can still receive a growth stub. Layout maintenance
    // must chase it, independently of the element store's own resolution.
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(target.cast()))._reserved & GC_ARRAY_RAW_F64_HOLES,
            0,
            "the growth target must inherit the numeric holes proof"
        );
    }
    js_gc_note_slot_layout(stub as u64, 1, string_bits(child));
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(target.cast()))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN
        );
        assert_eq!(
            (*header_from_user_ptr(target.cast()))._reserved
                & (GC_ARRAY_RAW_F64_LAYOUT | GC_ARRAY_RAW_F64_HOLES),
            0,
            "the direct generated note must retire numeric sub-flags"
        );
    }
    crate::array::js_array_set_f64(target, 1, f64::from_bits(string_bits(child)));
    js_shadow_slot_set(0, ptr_bits(stub as usize));
    let before = gc_collection_count();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(gc_collection_count() > before);
    assert!(trace.copying_nursery.copied_objects >= 2);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::array::ArrayHeader;
    let bits = crate::array::js_array_get_f64(moved, 1).to_bits();
    assert_ne!((bits & POINTER_MASK) as usize, child);
    unsafe {
        assert_string_bytes(
            (bits & POINTER_MASK) as *const crate::StringHeader,
            b"forwarded-child",
        );
    }
}

#[test]
fn kind_transition_after_rhs_allocation_collection() {
    let _alloc_pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _gc = CopyingNurseryTestGuard::new(2);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let array = crate::array::js_array_alloc_with_length(8);
    crate::array::js_array_set_f64(array, 1, 1.5);
    js_shadow_slot_set(0, ptr_bits(array as usize));
    // Fill the current block exactly. A fixed-size filler can leave enough
    // room for the RHS and silently miss the allocation collection point.
    unsafe {
        let state = crate::arena::js_inline_arena_state();
        let aligned = ((*state).offset + 7) & !7;
        let remaining = (*state).size - aligned;
        if remaining != 0 {
            crate::arena::arena_alloc(remaining, 8);
        }
    }
    triggers.make_arena_trigger_due();
    let before = gc_collection_count();
    // a[1] = {}: evaluating the RHS allocates before the slot address is used.
    let rhs = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(1, ptr_bits(rhs as usize));
    if gc_collection_count() == before {
        complete_budgeted_gc_cycle();
    }
    assert!(
        gc_collection_count() > before,
        "RHS allocation must make a collection due"
    );
    let array_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert_ne!(
        array_after, array,
        "the evaluated LHS owner must move before the store"
    );
    let rhs_after = js_shadow_slot_get(1);
    let before_store = gc_collection_count();
    crate::array::js_array_set_f64(array_after, 1, f64::from_bits(rhs_after));
    assert_eq!(
        gc_collection_count(),
        before_store,
        "layout transition and slot publication must have no safepoint between them"
    );
    js_shadow_slot_set(1, 0);
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(array_after.cast()))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN
        );
    }
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects >= 1);
    let final_array = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::array::ArrayHeader;
    let stored = (crate::array::js_array_get_f64(final_array, 1).to_bits() & POINTER_MASK) as usize;
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(stored as *const u8)).obj_type,
            GC_TYPE_OBJECT
        );
    }
}
