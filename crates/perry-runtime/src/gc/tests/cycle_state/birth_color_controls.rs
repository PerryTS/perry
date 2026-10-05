//! Post-remark header-color sabotage with tracked malloc storage, so loss is
//! observed without dereferencing a swept arena address. Payload/header writes
//! model the two emitted inline births; a seed alone does NOT mark its owner.
use super::*;

fn post_remark_birth_survives(kind: u8, include_live_flags: bool) -> std::thread::Result<bool> {
    let _guard = CopyingNurseryTestGuard::new(9);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    // Full-mark drains now accumulate remembered entries directly; the former
    // sliced rebuild no longer opens a mutator window. Weak holders ensure the
    // remaining post-remark weak-processing window actually returns to us.
    for slot in 0..8 {
        let target = crate::object::js_object_alloc(0, 0);
        let holder = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target as usize)));
        js_shadow_slot_set(slot, ptr_bits(holder as usize));
    }
    let mut local = 0_u64;
    js_shadow_slot_bind(8, &mut local);
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    for step in 0..100_000 {
        if state.atomic_finalize_subphase_for_tests() == Some("weak_processing") {
            break;
        }
        state.step(GcWorkBudget::bounded(1));
        assert!(step < 99_999, "post-remark window must be exercised");
    }
    assert_eq!(state.phase(), GcCyclePhase::AtomicFinalize);
    assert!(incremental_mark_barrier_active());
    let (prefix, slots) = if kind == GC_TYPE_ARRAY {
        (8, 2)
    } else {
        (std::mem::size_of::<crate::object::ObjectHeader>(), 8)
    };
    let user = gc_malloc(prefix + slots * 8, kind);
    unsafe {
        let live = *(*crate::arena::js_inline_arena_state()).birth_flags;
        assert_eq!(live, GC_FLAG_MARKED);
        // This is the sabotage: omit the live flags from the newborn header.
        // Malloc storage has no ARENA bit; otherwise the color/seed protocol is
        // identical to the inline header store. gc_malloc's queued seed remains
        // in BOTH arms, proving that a seed cannot repair a white owner.
        (*header_from_user_ptr(user)).gc_flags = if include_live_flags { live } else { 0 };
        if kind == GC_TYPE_ARRAY {
            (user as *mut crate::array::ArrayHeader).write(crate::array::ArrayHeader {
                length: slots as u32,
                capacity: slots as u32,
            });
        } else {
            (user as *mut crate::object::ObjectHeader).write(crate::object::ObjectHeader {
                class_id: 0,
                parent_class_id: 0,
                meta: std::ptr::null_mut(),
            });
        }
        for slot in 0..slots {
            (user.add(prefix) as *mut u64)
                .add(slot)
                .write(crate::value::TAG_UNDEFINED);
        }
        js_gc_note_black_birth(
            header_from_user_ptr(user),
            (*crate::arena::js_inline_arena_state()).birth_seeds,
        );
    }
    local = ptr_bits(user as usize);
    // Setup/color assertions above deliberately remain OUTSIDE the catch: a
    // missing window must fail the sabotage rather than count as lost-object
    // detection. Only the verifier/collection of the deliberately white owner
    // may panic in the negative arm.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_cycle_in_single_unit_steps(&mut state);
        std::hint::black_box(local);
        malloc_user_ptr_tracked(user)
    }))
}

#[test]
fn post_remark_array_birth_color_preserves_local_only_owner() {
    assert!(post_remark_birth_survives(GC_TYPE_ARRAY, true).expect("valid array birth"));
}

#[test]
fn post_remark_object_birth_color_preserves_local_only_owner() {
    assert!(post_remark_birth_survives(GC_TYPE_OBJECT, true).expect("valid object birth"));
}

#[test]
fn missing_array_birth_flags_sabotage_detects_swept_local_only_owner() {
    let result = post_remark_birth_survives(GC_TYPE_ARRAY, false);
    assert!(result.is_err() || !result.unwrap());
}

#[test]
fn missing_object_birth_flags_sabotage_detects_swept_local_only_owner() {
    let result = post_remark_birth_survives(GC_TYPE_OBJECT, false);
    assert!(result.is_err() || !result.unwrap());
}
