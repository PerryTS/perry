//! #11929: stable bound homes and post-final-remark mutator windows.
use super::*;

#[test]
fn final_remark_visits_two_hundred_bound_shadow_frame_homes() {
    const HOMES: usize = 200;
    let _guard = CopyingNurseryTestGuard::new(HOMES as u32);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Same active bound-address entries as #11960's stable-home lowering.
    // Bind while idle; later writes deliberately bypass the runtime bind API's
    // conservative shading, so only the collector's final scan can retain them.
    let mut homes = vec![0_u64; HOMES];
    for (index, home) in homes.iter_mut().enumerate() {
        js_shadow_slot_bind(index as u32, home as *mut u64);
    }
    let children: Vec<_> = (0..HOMES).map(|_| alloc_tracked_test_closure()).collect();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::ArenaBytes));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(incremental_mark_barrier_active());
    for (home, child) in homes.iter_mut().zip(&children) {
        assert_eq!(
            unsafe { (*header_from_user_ptr(*child)).gc_flags & GC_FLAG_MARKED },
            0,
            "the initial root scan must not have discovered this child"
        );
        *home = ptr_bits(*child as usize);
    }
    // Fresh runtime births during marking also enter the same bound homes.
    // Keep the other half white until the remark so this cannot pass merely
    // because allocate-black retained every object independently of scanning.
    let births: Vec<_> = (0..HOMES / 2)
        .map(|_| alloc_tracked_test_closure())
        .collect();
    for (home, child) in homes.iter_mut().zip(&births) {
        assert_marked_user_ptr(*child as usize, "fresh birth during marking");
        *home = ptr_bits(*child as usize);
    }
    run_cycle_in_single_unit_steps(&mut state);
    std::hint::black_box(&homes);
    for child in children.into_iter().skip(HOMES / 2).chain(births) {
        assert!(
            malloc_user_ptr_tracked(child),
            "FinalRootRemark missed a bound stable shadow-frame home"
        );
    }
}

fn post_remark_unshaded_root_stores_survive(window: &str) {
    const HOLDERS: u32 = 8;
    let _guard = CopyingNurseryTestGuard::new(HOLDERS + 2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    // Keep an old graph in the mark set and enough weak holders to exercise a
    // genuinely sliced post-remark weak-processing window. Full-mark drains
    // now remember old-to-young entries directly; no rebuild window remains.
    let mut old_roots: Vec<u64> = (0..8)
        .map(|_| ptr_bits(unsafe { alloc_old_test_object(0) }.0 as usize))
        .collect();
    for root in &mut old_roots {
        js_gc_register_global_root(root as *mut u64 as i64);
    }
    for slot in 0..HOLDERS {
        let target = crate::object::js_object_alloc(0, 0);
        let holder = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target as usize)));
        js_shadow_slot_set(slot, ptr_bits(holder as usize));
    }
    let marked = alloc_tracked_test_closure();
    let mut source_home = ptr_bits(marked as usize);
    let mut destination_home = 0_u64;
    let mut global_root = 0_u64;
    let mut inline_root = 0_u64;
    js_shadow_slot_bind(HOLDERS, &mut source_home as *mut u64);
    js_shadow_slot_bind(HOLDERS + 1, &mut destination_home as *mut u64);
    js_gc_register_global_root(&mut global_root as *mut u64 as i64);
    js_gc_register_global_root(&mut inline_root as *mut u64 as i64);

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::ArenaBytes));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    for step in 0..100_000 {
        if state.atomic_finalize_subphase_for_tests() == Some(window) {
            break;
        }
        state.step(GcWorkBudget::bounded(1));
        assert!(step < 99_999, "post-remark window {window} never opened");
    }
    assert_eq!(state.phase(), GcCyclePhase::AtomicFinalize);
    assert_eq!(state.atomic_finalize_subphase_for_tests(), Some(window));
    assert!(incremental_mark_barrier_active());
    assert_marked_user_ptr(marked as usize, "pre-existing reachable source");

    // Root-to-root transfer cannot create a white object after the complete
    // remark/drain. A new malloc-path object is black at birth and seeded.
    destination_home = source_home;
    source_home = 0;
    let birth = alloc_tracked_test_closure();
    let born_marked = unsafe { (*header_from_user_ptr(birth)).gc_flags & GC_FLAG_MARKED != 0 };
    global_root = ptr_bits(birth as usize);
    // Model both inline arms after their merge using the SAME live cell and
    // post-initialization seed protocol as the emitted allocation sequence.
    let inline_birth = unsafe {
        let total = GC_HEADER_SIZE + std::mem::size_of::<crate::object::ObjectHeader>() + 16;
        let raw = crate::arena::js_inline_arena_slow_alloc(
            crate::arena::js_inline_arena_state(),
            total,
            8,
        );
        (raw as *mut GcHeader).write(GcHeader {
            obj_type: GC_TYPE_OBJECT,
            gc_flags: GC_FLAG_ARENA | *(*crate::arena::js_inline_arena_state()).birth_flags,
            _reserved: 0,
            size: total as u32,
        });
        let object = raw.add(GC_HEADER_SIZE) as *mut crate::object::ObjectHeader;
        object.write(crate::object::ObjectHeader {
            class_id: 0,
            parent_class_id: 0,
            meta: std::ptr::null_mut(),
        });
        let fields =
            (object as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
        fields.write(crate::value::TAG_UNDEFINED);
        fields.add(1).write(crate::value::TAG_UNDEFINED);
        object
    };
    assert_eq!(
        unsafe { (*header_from_user_ptr(inline_birth as *mut u8)).gc_flags & GC_FLAG_MARKED },
        GC_FLAG_MARKED
    );
    js_gc_note_black_birth(
        unsafe { header_from_user_ptr(inline_birth as *mut u8) },
        unsafe { (*crate::arena::js_inline_arena_state()).birth_seeds },
    );
    let inline_born_marked =
        unsafe { (*header_from_user_ptr(inline_birth as *mut u8)).gc_flags & GC_FLAG_MARKED != 0 };
    inline_root = ptr_bits(inline_birth as usize);
    // An owner's overflow record is swept if that owner is lost; the key is
    // deliberately NOT a root. This observes actual loss, not just mark bits.
    crate::object::test_seed_overflow_fields_root(inline_birth as usize, 42_f64.to_bits());
    // No bind call or root-shading call is allowed at either publication.
    run_cycle_in_single_unit_steps(&mut state);
    std::hint::black_box((
        source_home,
        destination_home,
        global_root,
        inline_root,
        old_roots,
    ));
    let inline_survived = crate::object::debug_overflow_entry_len(inline_birth as usize).is_some();
    crate::object::test_clear_overflow_fields_root();
    assert!(
        inline_survived,
        "post-remark inline birth was lost in {window}"
    );
    assert!(
        inline_born_marked,
        "inline birth operation must mark before publication"
    );
    assert!(
        malloc_user_ptr_tracked(marked),
        "marked root transfer was lost in {window}"
    );
    assert!(
        malloc_user_ptr_tracked(birth),
        "post-remark birth was lost in {window}"
    );
    assert!(
        born_marked,
        "post-remark runtime birth must be black in {window}"
    );
}

#[test]
fn post_remark_root_stores_during_weak_processing_survive() {
    post_remark_unshaded_root_stores_survive("weak_processing");
}
