//! Bulk capture births must refresh raw boxes and tagged pointers when the
//! allocation itself moves them, and retain their shared identities afterwards.
use super::support::*;
use crate::gc::*;

extern "C" fn captured_body(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

static BODY: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    captured_body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
);

#[test]
fn bulk_boxed_birth_preserves_body_shape_flags_and_fresh_identity() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let cell = crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64);
    let captures = [cell as u64, crate::value::TAG_UNDEFINED];
    let count = crate::closure::CAPTURES_THIS_FLAG | crate::closure::NO_THIS_REBIND_FLAG | 2;
    let expected_shape = crate::closure::shape::birth_shape_for_body(&BODY);
    let first = crate::closure::js_closure_alloc_init_boxed(&BODY, count, captures.as_ptr());
    let second = crate::closure::js_closure_alloc_init_boxed(&BODY, count, captures.as_ptr());
    assert_ne!(
        first, second,
        "each evaluation has its own function identity"
    );
    for closure in [first, second] {
        unsafe {
            assert_eq!(
                (*closure).info,
                &BODY as *const crate::closure::JsFunctionInfo
            );
            assert_eq!((*closure).shape_id, expected_shape);
            assert_eq!((*closure).capture_count, count);
            let slots = crate::closure::closure_capture_slots_mut(closure);
            assert_eq!(*slots, cell as u64, "box identity stays shared");
            assert_eq!(*slots.add(1), captures[1]);
        }
    }
}

#[test]
fn large_malloc_bulk_boxed_birth_preserves_young_captures() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let cell = crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64);
    let mut captures = vec![crate::value::TAG_UNDEFINED; LARGE_OBJECT_THRESHOLD_BYTES / 8 + 64];
    captures[0] = cell as u64;
    let closure = crate::closure::js_closure_alloc_init_boxed(
        std::ptr::null(),
        captures.len() as u32,
        captures.as_ptr(),
    );
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(closure as *const u8)).gc_flags & GC_FLAG_ARENA,
            0,
            "this fixture must exercise the malloc fallback"
        );
    }
    assert!(malloc_user_ptr_tracked(closure as *mut u8));
    js_shadow_slot_set(0, ptr_bits(closure as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects > 0);
    let current = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::r#box::Box;
    assert_ne!(cell, current);
    assert_eq!(crate::r#box::js_box_get(current), 17.0);
}

#[test]
fn old_arena_bulk_capture_install_remembers_young_box() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let cell = crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64);
    let closure = crate::arena::arena_alloc_gc_old_born_tenured(
        crate::closure::closure_payload_size(1),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as *mut crate::closure::ClosureHeader;
    unsafe {
        (*closure).capture_count = 1;
        (*closure).shape_id = 0;
        (*closure).info = std::ptr::null();
        // GC_STORE_AUDIT(INIT): fresh test closure, null props edge.
        (*closure).props = std::ptr::null_mut();
        layout_init_pointer_free(closure as *mut u8);
        assert_ne!(
            (*header_from_user_ptr(closure as *const u8)).gc_flags & GC_FLAG_TENURED,
            0
        );
        crate::closure::closure_install_boxed_captures(closure, &[cell as u64]);
    }
    assert!(
        remembered_set_size() > 0,
        "old birth must remember the young box"
    );
    js_shadow_slot_set(0, ptr_bits(closure as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects > 0);
    let current = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::r#box::Box;
    assert_ne!(cell, current);
    assert_eq!(crate::r#box::js_box_get(current), 17.0);
}

#[test]
fn bulk_boxed_birth_shades_captures_during_incremental_marking() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    let cell = crate::r#box::js_box_alloc_bits(17.0f64.to_bits() as i64);
    let valid = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid);
    let captures = [cell as u64];
    let closure =
        crate::closure::js_closure_alloc_init_boxed(std::ptr::null(), 1, captures.as_ptr());
    assert_eq!(
        crate::closure::js_closure_get_capture_ptr(closure, 0),
        cell as i64
    );
    drain_incremental_mark_barrier_seeds(&valid);
    assert_marked_user_ptr(cell as usize, "newborn capture box");
}

#[test]
fn bulk_boxed_birth_survives_collection_inside_allocation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = ConservativeScanDisabledGuard::new();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let child = young_leaf();
    let cell = crate::r#box::js_box_alloc_bits(string_bits(child) as i64);
    let flags = crate::closure::CAPTURES_THIS_FLAG | crate::closure::NO_THIS_REBIND_FLAG;
    let captures = [cell as u64, string_bits(child), 42.0f64.to_bits()];
    super::runtime_roots::force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();

    let closure = crate::closure::js_closure_alloc_init_boxed(
        std::ptr::null(),
        flags | captures.len() as u32,
        captures.as_ptr(),
    );
    // Inspect the installed words before a getter or another collection can
    // follow forwarding pointers and conceal a missing post-allocation reload.
    let installed = unsafe {
        std::slice::from_raw_parts(crate::closure::closure_capture_slots_mut(closure), 3)
    };
    assert_ne!(installed[0], captures[0], "raw box root must be refreshed");
    assert_ne!(installed[1], captures[1], "tagged root must be refreshed");
    assert_eq!(installed[2], captures[2], "numeric bits must stay exact");
    assert_eq!(
        captures[0], cell as u64,
        "input buffer is not a mutable root"
    );
    assert_eq!(captures[1], string_bits(child));
    js_shadow_slot_set(0, ptr_bits(closure as usize));
    let moved_cell =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::r#box::Box;
    let moved_child = crate::closure::js_closure_get_capture_bits(closure, 1) & POINTER_MASK;
    assert_ne!(
        moved_cell, cell,
        "collection inside the helper must move the box"
    );
    assert_ne!(moved_child as usize, child, "tagged capture must move too");
    assert_eq!(
        crate::r#box::js_box_get_bits(moved_cell) as u64,
        string_bits(moved_child as usize)
    );
    unsafe {
        assert_eq!((*closure).capture_count, flags | 3);
        assert_eq!(
            (*header_from_user_ptr(closure as *const u8))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN,
            "boxed birth retains its capture policy in the header"
        );
    }

    let second_captures = [moved_cell as u64];
    let second =
        crate::closure::js_closure_alloc_init_boxed(std::ptr::null(), 1, second_captures.as_ptr());
    assert_ne!(
        (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader,
        second,
        "closure evaluations retain fresh identity"
    );
    let scope = RuntimeHandleScope::new();
    let second_root = scope.root_raw_mut_ptr(second);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects > 0);
    let first = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::closure::ClosureHeader;
    let second = second_root.get_raw_mut_ptr::<crate::closure::ClosureHeader>();
    let first_cell = crate::closure::js_closure_get_capture_ptr(first, 0) as *mut crate::r#box::Box;
    assert_eq!(
        first_cell as i64,
        crate::closure::js_closure_get_capture_ptr(second, 0)
    );
    crate::r#box::js_box_set(first_cell, 99.0);
    assert_eq!(crate::r#box::js_box_get(first_cell), 99.0);
}

#[test]
fn plain_bulk_birth_refreshes_tagged_captures_inside_allocation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = ConservativeScanDisabledGuard::new();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let child = young_leaf();
    let captures = [string_bits(child), crate::value::TAG_UNDEFINED];
    super::runtime_roots::force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();
    let closure = crate::closure::js_closure_alloc_init(std::ptr::null(), 2, captures.as_ptr());
    let installed = unsafe { *crate::closure::closure_capture_slots_mut(closure) };
    assert_ne!(installed, captures[0], "tagged root must be refreshed");
    let current = crate::closure::js_closure_get_capture_bits(closure, 0) & POINTER_MASK;
    assert_ne!(current as usize, child, "collection must move the capture");
    assert!(build_valid_pointer_set().contains(&(current as usize)));
    assert_eq!(
        crate::closure::js_closure_get_capture_bits(closure, 1),
        captures[1]
    );
}
