//! The same allocation sequence as codegen, with an adversarial early-drain
//! control: a seed consumed before payload initialization loses the real child.
use super::*;

unsafe fn raw_birth(kind: u8, shape: u32, child: *mut u8, premature_drain: bool) -> *mut u8 {
    let slots = if kind == GC_TYPE_ARRAY { 2 } else { 8 };
    let prefix = if kind == GC_TYPE_ARRAY {
        8
    } else {
        std::mem::size_of::<crate::object::ObjectHeader>()
    };
    let total = GC_HEADER_SIZE + prefix + slots * 8;
    let state = crate::arena::js_inline_arena_state();
    let raw = crate::arena::js_inline_arena_slow_alloc(state, total, 8);
    (raw as *mut GcHeader).write(GcHeader {
        obj_type: kind,
        gc_flags: GC_FLAG_ARENA | *(*state).birth_flags,
        _reserved: 0,
        size: total as u32,
    });
    let user = raw.add(GC_HEADER_SIZE);
    if kind == GC_TYPE_ARRAY {
        (user as *mut crate::array::ArrayHeader).write(crate::array::ArrayHeader {
            length: slots as u32,
            capacity: slots as u32,
        });
    } else {
        let object = user as *mut crate::object::ObjectHeader;
        object.write(crate::object::ObjectHeader {
            class_id: 0,
            parent_class_id: 0,
            meta: std::ptr::null_mut(),
        });
        crate::object::shapes::store_kind::premark_plain_ordinary(object);
        (*object).parent_class_id = shape;
    }
    let fields = user.add(prefix) as *mut u64;
    for i in 0..slots {
        fields.add(i).write(if premature_drain {
            0xDEADBEEFBAADF0DE
        } else {
            crate::value::TAG_UNDEFINED
        });
    }
    if premature_drain {
        // Previously recycled bytes look like valid old slot values. Force a
        // collector drain IMMEDIATELY after this deliberately premature seed,
        // before the real child stores. MARKED then prevents another visit.
        js_gc_note_black_birth(raw as *mut GcHeader, (*state).birth_seeds);
        let valid = build_valid_pointer_set();
        drain_incremental_mark_barrier_seeds(&valid);
    }
    fields.write(ptr_bits(child as usize));
    if !premature_drain {
        js_gc_note_black_birth(raw as *mut GcHeader, (*state).birth_seeds);
    }
    user
}

fn child_survives(kind: u8, premature_drain: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let shape = if kind == GC_TYPE_OBJECT {
        crate::object::shapes::shape_descriptor_ensure(std::ptr::null(), 0, 8).unwrap()
    } else {
        0
    };
    let child = alloc_tracked_test_closure();
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    assert_eq!(state.phase(), GcCyclePhase::BuildValidPointerSet);
    assert!(
        !incremental_mark_barrier_active(),
        "must exercise barrier-disabled births"
    );
    let parent = unsafe { raw_birth(kind, shape, child, premature_drain) };
    assert_marked_user_ptr(parent as usize, "inline live-cell birth");
    // Parent has no root or insertion barrier. Birth color retains the parent,
    // and ONLY its post-initialization seed can discover the white child.
    run_cycle_in_single_unit_steps(&mut state);
    std::hint::black_box(parent);
    malloc_user_ptr_tracked(child)
}

#[test]
fn inline_array_black_birth_traces_child_in_barrier_disabled_build_window() {
    assert!(
        child_survives(GC_TYPE_ARRAY, false),
        "initialized array seed lost its child"
    );
}

#[test]
fn inline_object_black_birth_traces_child_in_barrier_disabled_build_window() {
    assert!(
        child_survives(GC_TYPE_OBJECT, false),
        "initialized object seed lost its child"
    );
}

#[test]
fn premature_inline_birth_seed_drain_detects_lost_array_child() {
    let result = std::panic::catch_unwind(|| child_survives(GC_TYPE_ARRAY, true));
    assert!(
        result.is_err() || !result.unwrap(),
        "early-drain sabotage must lose the child or fail mark verification"
    );
}

#[test]
fn premature_inline_birth_seed_drain_detects_lost_object_child() {
    let result = std::panic::catch_unwind(|| child_survives(GC_TYPE_OBJECT, true));
    assert!(
        result.is_err() || !result.unwrap(),
        "early-drain sabotage must lose the child or fail mark verification"
    );
}

#[test]
fn inline_birth_flags_address_tracks_cycle_and_sweep_snapshot() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let address = unsafe { (*crate::arena::js_inline_arena_state()).birth_flags };
    let queue = unsafe { (*crate::arena::js_inline_arena_state()).birth_seeds };
    assert_eq!(queue, mark_seed_queue_address());
    assert_eq!(address, gc_birth_flags_address());
    assert_eq!(unsafe { *address }, 0);
    let old = unsafe { alloc_old_test_object(0) }.0;
    js_shadow_slot_set(0, ptr_bits(old as usize));
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    assert_eq!(unsafe { *address }, GC_FLAG_MARKED);
    run_cycle_until_phase(&mut state, GcCyclePhase::Sweep);
    assert_eq!(
        unsafe { *address },
        GC_FLAG_MARKED,
        "finalize->snapshot gap remains black"
    );
    for _ in 0..100_000 {
        if unsafe { *address } == 0 {
            break;
        }
        state.step(GcWorkBudget::bounded(1));
    }
    assert_eq!(
        unsafe { *address },
        0,
        "budgeted sweep births must stay white after snapshot"
    );
    assert_eq!(
        unsafe { (*crate::arena::js_inline_arena_state()).birth_flags },
        address
    );
    run_cycle_in_single_unit_steps(&mut state);
    assert_eq!(
        queue,
        mark_seed_queue_address(),
        "queue address survives drains"
    );
    assert_eq!(
        unsafe { (*crate::arena::js_inline_arena_state()).birth_seeds },
        queue
    );
}
