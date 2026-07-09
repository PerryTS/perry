mod promise_side_tables;
mod survival_and_malloc;
mod weak_semantics;
use super::super::*;
use super::support::*;

fn deactivate_malloc_registry_for_tests() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.set.clear();
        s.registry_state = MallocRegistryState::Inactive;
    });
}

fn malloc_registry_active_for_tests() -> bool {
    MALLOC_STATE.with(|s| s.borrow().malloc_registry_available())
}

#[derive(Default)]
struct TestFfiMutableRootSlots {
    i64_slots: Vec<i64>,
    usize_slots: Vec<usize>,
    raw_ptr_slots: Vec<*mut u8>,
    nanbox_f64_slots: Vec<f64>,
    nanbox_u64_slots: Vec<u64>,
}

thread_local! {
    static TEST_FFI_MUTABLE_ROOTS: RefCell<TestFfiMutableRootSlots> =
        RefCell::new(TestFfiMutableRootSlots::default());
    static TEST_RUST_MUTABLE_ROOTS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn test_ffi_mutable_root_scanner(visit: PerryFfiMutableRootVisitor, ctx: *mut c_void) {
    TEST_FFI_MUTABLE_ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        for slot in roots.i64_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_I64,
                slot as *mut i64 as *mut c_void,
                ctx,
            );
        }
        for slot in roots.usize_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_USIZE,
                slot as *mut usize as *mut c_void,
                ctx,
            );
        }
        for slot in roots.raw_ptr_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_RAW_MUT_PTR,
                slot as *mut *mut u8 as *mut c_void,
                ctx,
            );
        }
        for slot in roots.nanbox_f64_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_NANBOX_F64,
                slot as *mut f64 as *mut c_void,
                ctx,
            );
        }
        for slot in roots.nanbox_u64_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_NANBOX_U64,
                slot as *mut u64 as *mut c_void,
                ctx,
            );
        }
    });
}

struct TemporaryFfiMutableRootScanner {
    previous_len: usize,
    previous_roots: TestFfiMutableRootSlots,
}

impl TemporaryFfiMutableRootScanner {
    fn new(slots: TestFfiMutableRootSlots) -> Self {
        let previous_roots = TEST_FFI_MUTABLE_ROOTS.with(|roots| roots.replace(slots));
        let previous_len = FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_len = scanners.len();
            scanners.push(test_ffi_mutable_root_scanner);
            previous_len
        });
        Self {
            previous_len,
            previous_roots,
        }
    }
}

impl Drop for TemporaryFfiMutableRootScanner {
    fn drop(&mut self) {
        FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_len);
        });
        TEST_FFI_MUTABLE_ROOTS.with(|roots| {
            roots.replace(std::mem::take(&mut self.previous_roots));
        });
    }
}

fn test_rust_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    TEST_RUST_MUTABLE_ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        for slot in roots.iter_mut() {
            visitor.visit_nanbox_u64_slot(slot);
        }
    });
}

struct TemporaryRustMutableRootScanner {
    previous_len: usize,
    previous_roots: Vec<u64>,
}

impl TemporaryRustMutableRootScanner {
    fn new(bits: Vec<u64>) -> Self {
        let previous_roots = TEST_RUST_MUTABLE_ROOTS.with(|roots| roots.replace(bits));
        let previous_len = MUTABLE_ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_len = scanners.len();
            scanners.push(MutableRootScannerEntry {
                scanner: test_rust_mutable_root_scanner,
                source: MutableRootScannerSource::RuntimeMutableScanner,
                budgeted_scanner: None,
                budgeted_state_factory: None,
            });
            previous_len
        });
        Self {
            previous_len,
            previous_roots,
        }
    }
}

impl Drop for TemporaryRustMutableRootScanner {
    fn drop(&mut self) {
        MUTABLE_ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_len);
        });
        TEST_RUST_MUTABLE_ROOTS.with(|roots| {
            roots.replace(std::mem::take(&mut self.previous_roots));
        });
    }
}

#[test]
fn test_old_managed_closure_capture_write_dirties_old_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = young_leaf();
    let payload = std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>();
    let closure = crate::arena::arena_alloc_gc_old(
        payload,
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as *mut crate::closure::ClosureHeader;
    unsafe {
        (*closure).func_ptr = test_captured_singleton_func as *const u8;
        (*closure).capture_count = 1;
        (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
        layout_init_pointer_free(closure as *mut u8);
    }
    let slot = unsafe {
        (closure as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64
    };
    let page = crate::arena::generation_page_for_addr(slot as usize);
    crate::arena::old_page_clear_dirty(page);
    assert!(!old_page_dirty_for(page));

    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));

    assert!(old_page_dirty_for(page));
    assert!(remembered_set_size() > 0);
}

#[test]
fn test_copying_minor_relocates_managed_closure_and_rewrites_capture() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let closure = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));
    js_shadow_slot_set(0, ptr_bits(closure as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let closure_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let capture_after_bits = unsafe {
        let slot = (closure_after as *const u8)
            .add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        *slot
    };
    let capture_after = (capture_after_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(closure_after, closure as usize);
    assert_ne!(capture_after, child);
    assert!(crate::arena::pointer_in_nursery(closure_after));
    assert!(crate::arena::pointer_in_nursery(capture_after));
    assert!(
        trace.copying_nursery.copied_objects >= 2,
        "managed closure and captured child should both move"
    );
}

#[test]
fn test_copying_minor_relocates_managed_map() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let map = crate::map::js_map_alloc(16);
    for i in 0..9 {
        crate::map::js_map_set(map, i as f64, (i * 10) as f64);
    }
    let key = crate::string::js_string_from_bytes(b"managed-map-key".as_ptr(), 15);
    let key_bits = string_bits(key as usize);
    crate::map::js_map_set(map, f64::from_bits(key_bits), 900.0);
    assert!(crate::map::is_registered_map(map as usize));
    assert!(crate::map::test_map_numeric_index_contains(map, 8.0));
    assert!(crate::map::test_map_string_index_contains(
        map,
        f64::from_bits(key_bits)
    ));

    js_shadow_slot_set(0, ptr_bits(map as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let map_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::map::MapHeader;
    let stored_string_key = crate::map::js_map_entry_key_at(map_after, 9);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(map_after as usize, map as usize);
    assert!(!crate::map::is_registered_map(map as usize));
    assert!(crate::map::is_registered_map(map_after as usize));
    assert!(crate::map::test_map_numeric_index_contains(map_after, 8.0));
    assert!(crate::map::test_map_string_index_contains(
        map_after,
        stored_string_key
    ));
    assert_eq!(crate::map::js_map_get(map_after, 8.0), 80.0);
    assert_eq!(crate::map::js_map_get(map_after, stored_string_key), 900.0);
}

#[test]
fn test_copying_minor_relocates_managed_set() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (child_obj, _child_fields) = unsafe { alloc_nursery_test_object(0) };
    let child = child_obj as usize;
    let child_bits = ptr_bits(child);
    let set = crate::set::js_set_alloc(16);
    for i in 0..9 {
        crate::set::js_set_add(set, i as f64);
    }
    crate::set::js_set_add(set, f64::from_bits(child_bits));
    assert!(crate::set::is_registered_set(set as usize));
    assert!(crate::set::test_set_index_contains(set, 8.0));
    assert!(crate::set::test_set_index_contains(
        set,
        f64::from_bits(child_bits)
    ));

    js_shadow_slot_set(0, ptr_bits(set as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let set_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    let rewritten_bits = crate::set::js_set_value_at(set_after, 9).to_bits();
    let rewritten = (rewritten_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(set_after as usize, set as usize);
    assert!(!crate::set::is_registered_set(set as usize));
    assert!(crate::set::is_registered_set(set_after as usize));
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(crate::set::js_set_has(set_after, 8.0), 1);
    assert_eq!(
        crate::set::js_set_has(set_after, f64::from_bits(child_bits)),
        0
    );
    assert_eq!(
        crate::set::js_set_has(set_after, f64::from_bits(rewritten_bits)),
        1
    );
    assert!(crate::set::test_set_index_contains(
        set_after,
        f64::from_bits(rewritten_bits)
    ));
}

#[test]
fn test_copying_minor_finalizes_dead_nursery_map_set_side_allocations() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 1.0, 2.0);
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 3.0);
    assert!(crate::map::is_registered_map(map as usize));
    assert!(crate::set::is_registered_set(set as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(!crate::map::is_registered_map(map as usize));
    assert!(!crate::set::is_registered_set(set as usize));
}

#[test]
fn test_copying_minor_rewrites_exact_object_pointer_slot_only() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = young_leaf();
    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(11.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::from_bits(ptr_bits(child)));
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(33.0));
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), Some(1));
    js_shadow_slot_set(0, ptr_bits(obj as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let fields = unsafe {
        (obj_after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64
    };
    let first = f64::from_bits(unsafe { *fields.add(0) });
    let child_after = unsafe { (*fields.add(1) & POINTER_MASK) as usize };
    let third = f64::from_bits(unsafe { *fields.add(2) });

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(obj_after, obj as usize);
    assert_ne!(child_after, child);
    assert_eq!(first, 11.0);
    assert_eq!(third, 33.0);
    assert!(crate::arena::pointer_in_nursery(obj_after));
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert_eq!(trace.layout_scans.masked_pointer_slots_read, 2);
    assert_eq!(trace.layout_scans.unknown_layout_slots_read, 0);
}

#[test]
fn test_copying_minor_rewrites_exact_closure_pointer_capture_only() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = young_leaf();
    let closure = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, 10.0);
    crate::closure::js_closure_set_capture_f64(closure, 1, f64::from_bits(ptr_bits(child)));
    crate::closure::js_closure_set_capture_f64(closure, 2, 30.0);
    assert_eq!(test_layout_pointer_slot_count(closure as usize, 3), Some(1));
    js_shadow_slot_set(0, ptr_bits(closure as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let closure_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let captures = unsafe {
        (closure_after as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64
    };
    let first = f64::from_bits(unsafe { *captures.add(0) });
    let child_after = unsafe { (*captures.add(1) & POINTER_MASK) as usize };
    let third = f64::from_bits(unsafe { *captures.add(2) });

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(closure_after, closure as usize);
    assert_ne!(child_after, child);
    assert_eq!(first, 10.0);
    assert_eq!(third, 30.0);
    assert!(crate::arena::pointer_in_nursery(closure_after));
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert_eq!(trace.layout_scans.masked_pointer_slots_read, 2);
    assert_eq!(trace.layout_scans.unknown_layout_slots_read, 0);
}

#[test]
fn test_copying_minor_preserves_dynamic_object_values_after_numeric_first_growth() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let id_key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let child_key = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let nested_key = crate::string::js_string_from_bytes(b"nested".as_ptr(), 6);

    let template = crate::object::js_object_alloc(0, 0);
    let template_name = crate::string::js_string_from_bytes(b"template".as_ptr(), 8);
    let template_child = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(template, id_key, 1.0);
    crate::object::js_object_set_field_by_name(
        template,
        name_key,
        f64::from_bits(string_bits(template_name as usize)),
    );
    crate::object::js_object_set_field_by_name(
        template,
        child_key,
        f64::from_bits(ptr_bits(template_child as usize)),
    );

    let obj = crate::object::js_object_alloc(0, 0);
    let name_value = crate::string::js_string_from_bytes(b"roundtrip".as_ptr(), 9);
    let child = crate::object::js_object_alloc(0, 0);
    let nested_value = crate::string::js_string_from_bytes(b"retained".as_ptr(), 8);
    crate::object::js_object_set_field_by_name(
        child,
        nested_key,
        f64::from_bits(string_bits(nested_value as usize)),
    );
    crate::object::js_object_set_field_by_name(obj, id_key, 1.0);
    crate::object::js_object_set_field_by_name(
        obj,
        name_key,
        f64::from_bits(string_bits(name_value as usize)),
    );
    crate::object::js_object_set_field_by_name(
        obj,
        child_key,
        f64::from_bits(ptr_bits(child as usize)),
    );
    js_shadow_slot_set(0, ptr_bits(obj as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::object::ObjectHeader;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(obj_after as usize, obj as usize);
    unsafe {
        let keys = (*obj_after).keys_array;
        assert!(!keys.is_null());
        assert_eq!(crate::array::js_array_length(keys), 3);
        let key0 = crate::array::js_array_get(keys, 0);
        let key1 = crate::array::js_array_get(keys, 1);
        let key2 = crate::array::js_array_get(keys, 2);
        assert!(key0.is_string());
        assert!(key1.is_string());
        assert!(key2.is_string());
        assert_string_bytes(key0.as_string_ptr(), b"id");
        assert_string_bytes(key1.as_string_ptr(), b"name");
        assert_string_bytes(key2.as_string_ptr(), b"child");
    }

    let id_lookup = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
    let name_lookup = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let child_lookup = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let nested_lookup = crate::string::js_string_from_bytes(b"nested".as_ptr(), 6);
    let id_value = crate::object::js_object_get_field_by_name(obj_after, id_lookup);
    let name_value = crate::object::js_object_get_field_by_name(obj_after, name_lookup);
    let child_value = crate::object::js_object_get_field_by_name(obj_after, child_lookup);

    assert_eq!(f64::from_bits(id_value.bits()), 1.0);
    assert!(name_value.is_string());
    unsafe {
        assert_string_bytes(name_value.as_string_ptr(), b"roundtrip");
    }
    assert!(child_value.is_pointer());
    let child_after = (child_value.bits() & POINTER_MASK) as *const crate::object::ObjectHeader;
    assert_ne!(child_after as usize, child as usize);
    let nested_after = crate::object::js_object_get_field_by_name(child_after, nested_lookup);
    assert!(nested_after.is_string());
    unsafe {
        assert_string_bytes(nested_after.as_string_ptr(), b"retained");
    }
}

#[test]
fn test_copying_minor_marks_array_growth_forwarding_target() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let stale_arr = crate::array::js_array_alloc(0);
    let mut current_arr = stale_arr;
    let mut first_closure = 0usize;

    for i in 0..50 {
        let child = young_leaf();
        let closure =
            crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 1);
        crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));
        if i == 0 {
            first_closure = closure as usize;
        }
        current_arr = crate::array::js_array_push_f64(
            current_arr,
            f64::from_bits(ptr_bits(closure as usize)),
        );
    }

    assert_ne!(
        stale_arr, current_arr,
        "test setup should grow the array and leave a forwarding stub"
    );
    js_shadow_slot_set(0, ptr_bits(stale_arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let first_value_bits =
        crate::array::js_array_get_f64(arr_after as *const crate::array::ArrayHeader, 0).to_bits();
    let closure_after = (first_value_bits & POINTER_MASK) as usize;
    let closure_header =
        unsafe { (closure_after as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader };
    let capture_after_bits = unsafe {
        let closure = closure_after as *const crate::closure::ClosureHeader;
        assert_eq!((*closure).type_tag, crate::closure::CLOSURE_MAGIC);
        let slot = (closure as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        *slot
    };
    let capture_after = (capture_after_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(arr_after, stale_arr as usize);
    assert_ne!(arr_after, current_arr as usize);
    assert_ne!(closure_after, first_closure);
    assert_eq!(unsafe { (*closure_header).obj_type }, GC_TYPE_CLOSURE);
    assert!(crate::arena::pointer_in_nursery(arr_after));
    assert!(crate::arena::pointer_in_nursery(closure_after));
    assert!(crate::arena::pointer_in_nursery(capture_after));
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_barriers_inactive() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::BarriersInactive,
        false,
    );
    assert_eq!(trace.copying_nursery.copied_objects, 0);
    assert_eq!(trace.copying_nursery.copied_bytes, 0);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 0);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, 0);
    assert!(!trace.evacuation_policy.considered);
    assert!(!trace.evacuation_policy.enabled);
    assert_eq!(trace.evacuation_policy.reason, "barriers_inactive");
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_conservative_stack_scan() {
    let _isolation = copying_nursery_isolation_lock();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();

    let eligibility = CopiedMinorEligibility::evaluate_with_stack_decision(
        GcTriggerKind::Direct,
        ConservativeStackScanDecision::Scan,
    );

    assert!(!eligibility.eligible);
    assert_eq!(
        eligibility.fallback_reason,
        CopiedMinorFallbackReason::ConservativeStack
    );
    assert_eq!(
        conservative_stack_scan_decision_for(ConservativeStackScanMode::Full, false),
        ConservativeStackScanDecision::Scan
    );
}

#[test]
fn test_copied_minor_eligibility_auto_skips_conservative_stack_scan() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert!(shadow_stack_has_active_frame());

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(trace.conservative_root_count, 0);
    assert_eq!(trace.conservative_pinned, 0);
    assert_eq!(trace.conservative_pinned_bytes, 0);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.pinned_bytes, 0);
    assert_eq!(
        trace.root_sources.native_stack_fallback.decision,
        ConservativeStackScanDecision::SkipDisabled
    );
    assert!(!trace.root_sources.native_stack_fallback.scanned);
    assert_eq!(
        trace
            .root_sources
            .native_stack_fallback
            .compiled_frame_pinned_roots,
        0
    );
}

#[test]
fn root_source_active_shadow_frame_reports_precise_shadow_roots_only() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, child);
    assert_eq!(trace.root_sources.compiled_shadow.slots_scanned, 1);
    assert_eq!(trace.root_sources.compiled_shadow.nonzero_slots, 1);
    assert_eq!(trace.root_sources.compiled_shadow.pointer_roots, 1);
    assert_eq!(trace.root_sources.compiled_shadow.rewritten_slots, 1);
    assert_eq!(
        trace.root_sources.native_stack_fallback.decision,
        ConservativeStackScanDecision::SkipDisabled
    );
    assert!(!trace.root_sources.native_stack_fallback.scanned);
    assert_eq!(
        trace
            .root_sources
            .native_stack_fallback
            .compiled_frame_pinned_bytes,
        0
    );
}

#[test]
fn test_copied_minor_eligibility_empty_rust_copy_only_scanner_falls_back() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_rust_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_empty_ffi_copy_only_scanner_falls_back() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::ffi_bits(&[]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_rejects_copy_only_without_scanning_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);

    let eligibility = CopiedMinorEligibility::evaluate(GcTriggerKind::Direct);

    assert!(!eligibility.eligible);
    assert_eq!(
        eligibility.fallback_reason,
        CopiedMinorFallbackReason::CopyOnlyRoots
    );
    assert_eq!(eligibility.legacy_root_stats.registered_rust_scanners, 1);
    assert_eq!(eligibility.legacy_root_stats.emitted_roots, 0);
    assert_eq!(eligibility.legacy_root_stats.emitted_young_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_live_young_rust_copy_only_root() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_rust_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_young_roots, 1);
    assert_eq!(
        trace.root_sources.native_stack_fallback.decision,
        ConservativeStackScanDecision::SkipDisabled
    );
    assert_eq!(
        trace
            .root_sources
            .native_stack_fallback
            .compiled_frame_pinned_bytes,
        0
    );
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_live_young_ffi_copy_only_root() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::ffi_bits(&[ptr_bits(child)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_young_roots, 1);
}

#[test]
fn test_ffi_mutable_i64_root_is_copied_without_copy_only_fallback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _mutable_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        i64_slots: vec![child as i64],
        ..TestFfiMutableRootSlots::default()
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = TEST_FFI_MUTABLE_ROOTS.with(|roots| roots.borrow().i64_slots[0] as usize);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, child);
    assert!(crate::arena::pointer_in_nursery(after));
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        0
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_ffi_mutable_active_registry_malloc_root_does_not_report_copy_only_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    activate_malloc_registry_for_tests();
    let _mutable_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        raw_ptr_slots: vec![live_malloc],
        ..TestFfiMutableRootSlots::default()
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = TEST_FFI_MUTABLE_ROOTS.with(|roots| roots.borrow().raw_ptr_slots[0]);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(after, live_malloc);
    assert!(trace.copying_nursery.malloc_validation_lookups > 0);
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        0
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_ffi_mutable_trampoline_visits_all_slot_kinds() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let i64_root = young_leaf();
    let usize_root = young_leaf();
    let raw_root = young_leaf();
    let f64_root = young_leaf();
    let u64_root = young_leaf();
    let _mutable_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        i64_slots: vec![i64_root as i64],
        usize_slots: vec![usize_root],
        raw_ptr_slots: vec![raw_root as *mut u8],
        nanbox_f64_slots: vec![f64::from_bits(ptr_bits(f64_root))],
        nanbox_u64_slots: vec![ptr_bits(u64_root)],
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let (i64_after, usize_after, raw_after, f64_after, u64_after) =
        TEST_FFI_MUTABLE_ROOTS.with(|roots| {
            let roots = roots.borrow();
            (
                roots.i64_slots[0] as usize,
                roots.usize_slots[0],
                roots.raw_ptr_slots[0] as usize,
                (roots.nanbox_f64_slots[0].to_bits() & POINTER_MASK) as usize,
                (roots.nanbox_u64_slots[0] & POINTER_MASK) as usize,
            )
        });

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    for (before, after) in [
        (i64_root, i64_after),
        (usize_root, usize_after),
        (raw_root, raw_after),
        (f64_root, f64_after),
        (u64_root, u64_after),
    ] {
        assert_ne!(after, before);
        assert!(crate::arena::pointer_in_nursery(after));
    }
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn root_source_runtime_handle_rewrite_is_attributed_to_runtime_handles() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        MutableRootScannerSource::RuntimeHandles,
    );
    let child = young_leaf();
    let scope = RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(child as *mut u8);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = handle.get_raw_mut_ptr::<u8>() as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, child);
    assert!(trace.root_sources.runtime_handles.registered_scanners >= 1);
    assert!(trace.root_sources.runtime_handles.slots_scanned > 0);
    assert!(trace.root_sources.runtime_handles.pointer_roots > 0);
    assert!(trace.root_sources.runtime_handles.rewritten_slots > 0);
}

#[test]
fn root_source_runtime_and_ffi_mutable_scanners_are_attributed_separately() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let rust_root = young_leaf();
    let ffi_root = young_leaf();
    let _rust_root_guard = TemporaryRustMutableRootScanner::new(vec![ptr_bits(rust_root)]);
    let _ffi_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        nanbox_u64_slots: vec![ptr_bits(ffi_root)],
        ..TestFfiMutableRootSlots::default()
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let rust_after =
        TEST_RUST_MUTABLE_ROOTS.with(|roots| (roots.borrow()[0] & POINTER_MASK) as usize);
    let ffi_after = TEST_FFI_MUTABLE_ROOTS
        .with(|roots| (roots.borrow().nanbox_u64_slots[0] & POINTER_MASK) as usize);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(rust_after, rust_root);
    assert_ne!(ffi_after, ffi_root);
    assert!(
        trace
            .root_sources
            .runtime_mutable_scanners
            .registered_scanners
            >= 1
    );
    assert!(trace.root_sources.runtime_mutable_scanners.pointer_roots > 0);
    assert!(trace.root_sources.runtime_mutable_scanners.rewritten_slots > 0);
    assert!(trace.root_sources.ffi_mutable_scanners.registered_scanners >= 1);
    assert!(trace.root_sources.ffi_mutable_scanners.pointer_roots > 0);
    assert!(trace.root_sources.ffi_mutable_scanners.rewritten_slots > 0);
}

#[test]
fn test_copied_minor_rewrites_old_promise_fixed_value_slot() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let promise = unsafe { alloc_old_test_promise() };

    crate::promise::js_promise_resolve(promise, f64::from_bits(ptr_bits(child)));
    assert!(remembered_set_size() > 0);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    unsafe {
        let after = ((*promise).value.to_bits() & POINTER_MASK) as usize;
        assert_ne!(after, child);
        assert!(crate::arena::pointer_in_nursery(after));
        assert_eq!(
            (*header_from_user_ptr(after as *const u8)).obj_type,
            GC_TYPE_OBJECT
        );
    }
}

#[test]
fn test_copied_minor_rewrites_old_error_cause_and_errors_slots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let cause_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let errors = crate::array::js_array_alloc(0);
    let original_errors = errors as usize;
    let error = unsafe { alloc_old_test_error() };

    unsafe {
        crate::error::error_set_cause(error, f64::from_bits(ptr_bits(cause_child)));
        crate::error::error_set_errors(error, errors);
    }
    assert!(remembered_set_size() > 0);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    unsafe {
        let cause_after = ((*error).cause.to_bits() & POINTER_MASK) as usize;
        let errors_after = (*error).errors as usize;
        assert_ne!(cause_after, cause_child);
        assert_ne!(errors_after, original_errors);
        assert!(crate::arena::pointer_in_nursery(cause_after));
        assert!(crate::arena::pointer_in_nursery(errors_after));
        assert_eq!(
            (*header_from_user_ptr(cause_after as *const u8)).obj_type,
            GC_TYPE_OBJECT
        );
        assert_eq!(
            (*header_from_user_ptr(errors_after as *const u8)).obj_type,
            GC_TYPE_ARRAY
        );
    }
}

#[test]
fn test_copied_minor_eligibility_old_only_copy_only_root_falls_back() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let old = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_OBJECT) as usize;
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(old)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_old_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_young_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_malformed_copy_only_root_falls_back() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[0x7FFD_0000_0000_1000]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.malformed_roots, 1);
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_malloc_copy_only_root() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    let _copy_only_root_guard =
        TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(live_malloc as usize)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(
        trace.legacy_copy_only_scanner_pinned.emitted_malloc_roots,
        1
    );
}

#[test]
fn test_copying_minor_rewrites_shadow_and_global_roots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let shadow_child = young_leaf();
    let global_child = young_leaf();
    let mut global_slot = global_child as u64;
    js_shadow_slot_set(0, ptr_bits(shadow_child));
    js_gc_register_global_root(&mut global_slot as *mut u64 as i64);

    let _ = gc_collect_minor();
    let shadow_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let global_after = global_slot as usize;

    assert_ne!(shadow_after, shadow_child);
    assert_ne!(global_after, global_child);
    assert!(crate::arena::pointer_in_nursery(shadow_after));
    assert!(crate::arena::pointer_in_nursery(global_after));
    assert_eq!(
        crate::arena::classify_heap_space(shadow_after),
        crate::arena::active_survivor_space()
    );
}

#[test]
fn test_copying_minor_rewrites_bound_compiled_local_slot() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let child = young_leaf();
    let mut compiled_local_slot = ptr_bits(child);
    js_shadow_slot_bind(0, &mut compiled_local_slot as *mut u64);

    let _ = gc_collect_minor();
    let local_after = (compiled_local_slot & POINTER_MASK) as usize;

    assert_ne!(local_after, child);
    assert_eq!(js_shadow_slot_get(0), compiled_local_slot);
    assert_eq!(
        crate::arena::classify_heap_space(local_after),
        crate::arena::active_survivor_space()
    );
}

#[test]
fn test_copying_minor_ignores_cleared_dead_shadow_slot_but_preserves_live_slot() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let dead = young_leaf();
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(dead));
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, ptr_bits(live));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let live_after = (js_shadow_slot_get(1) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(js_shadow_slot_get(0), 0);
    assert_ne!(live_after, live);
    assert!(crate::arena::pointer_in_nursery(live_after));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.shadow_roots.slots_scanned, 1);
    assert_eq!(trace.shadow_roots.nonzero_slots, 1);
    assert_eq!(trace.shadow_roots.pointer_roots, 1);
    assert_eq!(trace.shadow_roots.rewritten_slots, 1);
    assert_eq!(trace.root_sources.compiled_shadow.slots_scanned, 1);
    assert_eq!(trace.root_sources.compiled_shadow.nonzero_slots, 1);
    assert_eq!(trace.root_sources.compiled_shadow.pointer_roots, 1);
    assert_eq!(trace.root_sources.compiled_shadow.rewritten_slots, 1);
}

#[test]
fn large_object_copying_minor_excludes_rooted_old_object_from_copy_counts() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let large =
        crate::arena::arena_alloc_gc(LARGE_OBJECT_THRESHOLD_BYTES, 8, GC_TYPE_STRING) as usize;
    let header = unsafe { header_from_user_ptr(large as *const u8) };
    let total = unsafe { (*header).size as usize };

    assert!(is_large_object_total_size(total));
    assert!(crate::arena::pointer_in_old_gen(large));
    js_shadow_slot_set(0, ptr_bits(large));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(after, large);
    assert_eq!(trace.copying_nursery.copied_objects, 0);
    assert_eq!(trace.copying_nursery.copied_bytes, 0);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 1);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, total);

    let event = trace.into_json(GcStepSnapshot::current());
    assert_eq!(
        event["copying_nursery"]["large_excluded_objects"].as_u64(),
        Some(1)
    );
    assert_eq!(
        event["copying_nursery"]["large_excluded_bytes"].as_u64(),
        Some(total as u64)
    );
}

#[test]
fn large_object_old_born_array_slot_write_keeps_young_child_alive() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let arr = crate::array::js_array_alloc(4096);

    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    crate::array::js_array_set_f64_extend(arr, 0, f64::from_bits(ptr_bits(child)));
    assert!(
        remembered_set_size() > 0,
        "large old-born array write should dirty old-page metadata"
    );

    let elements = unsafe {
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor edge must remain remembered after copied minor"
    );
}

#[test]
fn large_object_array_literal_direct_store_keeps_young_child_alive_and_excludes_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let arr = crate::array::js_array_alloc_literal(4096);
    let parent_total = unsafe { (*header_from_user_ptr(arr as *const u8)).size as usize };

    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    assert!(is_large_object_total_size(parent_total));
    let elements = unsafe {
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    unsafe {
        *elements = ptr_bits(child);
    }
    layout_note_slot(arr as usize, 0, unsafe { *elements });
    runtime_write_barrier_slot(arr as usize, elements as usize, unsafe { *elements });
    assert!(
        remembered_set_size() > 0,
        "direct large literal store should dirty old-page metadata"
    );
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(arr_after, arr as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 1);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, parent_total);
}

#[test]
fn large_object_inline_push_store_keeps_young_child_alive_and_excludes_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let arr = crate::array::js_array_alloc(4096);
    let parent_total = unsafe { (*header_from_user_ptr(arr as *const u8)).size as usize };

    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    assert!(is_large_object_total_size(parent_total));

    let elements = unsafe {
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    let slot = unsafe {
        let length = (*arr).length as usize;
        assert!(length < (*arr).capacity as usize);
        let slot = elements.add(length);
        *slot = ptr_bits(child);
        (*arr).length = length as u32 + 1;
        layout_note_slot(arr as usize, length, *slot);
        runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        slot
    };
    assert!(
        remembered_set_size() > 0,
        "optimized direct push store should dirty old-page metadata"
    );
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let rewritten = unsafe { (*slot & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(arr_after, arr as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 1);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, parent_total);
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor edge must remain remembered after copied minor"
    );
}

#[test]
fn large_object_clone_direct_copy_keeps_young_child_alive_and_excludes_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let src = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(src, 0, crate::value::JSValue::from_bits(ptr_bits(child)));

    let clone = unsafe {
        crate::object::js_object_clone_with_extra(
            f64::from_bits(ptr_bits(src as usize)),
            4096,
            std::ptr::null(),
            0,
        )
    };
    let parent_total = unsafe { (*header_from_user_ptr(clone as *const u8)).size as usize };
    let fields = unsafe {
        (clone as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64
    };

    assert!(crate::arena::pointer_in_old_gen(clone as usize));
    assert!(is_large_object_total_size(parent_total));
    assert!(
        remembered_set_size() > 0,
        "old-born clone field copy should dirty old-page metadata"
    );
    js_shadow_slot_set(0, ptr_bits(clone as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let clone_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let rewritten = unsafe { (*fields & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(clone_after, clone as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert!(trace.copying_nursery.large_excluded_objects >= 1);
    assert!(trace.copying_nursery.large_excluded_bytes >= parent_total);
}

#[test]
fn malloc_backed_large_closure_capture_in_old_container_survives_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let max_managed_captures = (LARGE_OBJECT_THRESHOLD_BYTES
        - GC_HEADER_SIZE
        - std::mem::size_of::<crate::closure::ClosureHeader>())
        / std::mem::size_of::<u64>();
    let closure = crate::closure::js_closure_alloc(
        test_captured_singleton_func as *const u8,
        (max_managed_captures + 1) as u32,
    );
    let closure_header = unsafe { header_from_user_ptr(closure as *const u8) };
    unsafe {
        assert_eq!((*closure_header).obj_type, GC_TYPE_CLOSURE);
        assert_eq!((*closure_header).gc_flags & GC_FLAG_ARENA, 0);
        assert!(is_large_object_total_size((*closure_header).size as usize));
    }
    assert!(malloc_user_ptr_tracked(closure as *mut u8));

    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));
    let capture_slot = unsafe {
        (closure as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64
    };
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(closure as usize);
        layout_note_slot(old_arr as usize, 0, *elements);
        runtime_write_barrier_slot(old_arr as usize, elements as usize, *elements);
    }
    js_shadow_slot_set(0, ptr_bits(old_arr as usize));
    assert!(
        remembered_set_size() > 0,
        "malloc-backed closure capture write should dirty external slot metadata"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let captured_after = unsafe { (*capture_slot & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(captured_after, child);
    assert!(crate::arena::pointer_in_nursery(captured_after));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    // Two remembered entries since the old→malloc fix: the external
    // capture-page entry (closure→young child) AND the old array's element
    // page (old→malloc edge). The latter is what keeps this malloc-backed
    // closure alive across minor malloc sweeps when the old container is
    // its only referrer.
    assert_eq!(trace.remembered_set.dirty_pages_before, 2);
    assert_eq!(trace.remembered_set.dirty_pages_scanned, 2);
    assert_eq!(trace.remembered_set.dirty_objects_scanned, 2);
    assert!(
        (1..=1024).contains(&trace.remembered_set.dirty_slots_scanned),
        "two dirty pages (capture page + old element page) should bound copied-minor scanning"
    );
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor closure capture edge should remain remembered"
    );

    for _ in 0..3 {
        let _ = gc_collect_minor();
    }
    let promoted = unsafe { (*capture_slot & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_old_gen(promoted));
    // The capture's external entry clears once the capture points old, but
    // the old array's element page stays remembered: its old→malloc edge
    // must keep protecting the closure from minor malloc sweeps.
    assert!(
        malloc_user_ptr_tracked(closure as *mut u8),
        "malloc-backed closure must remain tracked while its old container lives"
    );
    assert_eq!(
        remembered_set_size(),
        1,
        "only the old→malloc element page should remain remembered once the \
         capture no longer points to young gen"
    );
}

#[test]
fn copied_minor_rewrites_dirty_set_external_element_and_reindexes() {
    struct SetRootGuard;

    impl Drop for SetRootGuard {
        fn drop(&mut self) {
            crate::set::test_clear_set_roots();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _set_guard = SetRootGuard;
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::set::test_clear_set_roots();

    let (child_obj, _child_fields) = unsafe { alloc_nursery_test_object(0) };
    let child = child_obj as usize;
    let child_bits = ptr_bits(child);
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, f64::from_bits(child_bits));
    assert_eq!(
        remembered_set_size(),
        0,
        "young managed Set stores should not dirty old-to-young metadata"
    );

    js_shadow_slot_set(0, ptr_bits(set as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let set_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    let rewritten_bits = crate::set::js_set_value_at(set_after, 0).to_bits();
    let rewritten = (rewritten_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(set_after as usize, set as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(
        crate::set::js_set_has(set_after, f64::from_bits(child_bits)),
        0
    );
    assert_eq!(
        crate::set::js_set_has(set_after, f64::from_bits(rewritten_bits)),
        1,
        "Set lookup index should be rebuilt after copied-minor rewrites"
    );
}

#[test]
fn test_copied_minor_verify_evacuation_env_remains_eligible() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.phase_us.contains_key("evacuation_verify"),
        "forced copied-minor verification should run before from-space reset"
    );
    assert_ne!(after, child);
    assert!(crate::arena::pointer_in_nursery(after));
}

#[test]
fn test_copied_minor_verify_evacuation_copy_only_roots_reject_before_copying() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(trace.copying_nursery.copied_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_copying_minor_rewrites_dirty_old_slot_and_keeps_sticky_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = young_leaf();
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor edge must stay dirty for the next minor"
    );
}

#[test]
fn test_copying_minor_copies_transitive_young_graph() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(1);
    let child = young_leaf();
    unsafe {
        (*arr).length = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = ptr_bits(child);
        layout_note_slot(arr as usize, 0, *elements);
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let _ = gc_collect_minor();
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after = unsafe {
        let elements = (arr_after as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *mut u64;
        (*elements & POINTER_MASK) as usize
    };

    assert_ne!(arr_after, arr as usize);
    assert_ne!(child_after, child);
    assert!(crate::arena::pointer_in_nursery(arr_after));
    assert!(crate::arena::pointer_in_nursery(child_after));
}

#[test]
fn test_copying_minor_moves_layout_masked_transitive_object() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(1);
    let (child, _child_fields) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*arr).length = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = ptr_bits(child as usize);
        layout_note_slot(arr as usize, 0, *elements);
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after = unsafe {
        let elements = (arr_after as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *mut u64;
        (*elements & POINTER_MASK) as usize
    };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(arr_after, arr as usize);
    assert_ne!(child_after, child as usize);
    assert!(crate::arena::pointer_in_nursery(arr_after));
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert!(
        trace.copying_nursery.copied_objects >= 2,
        "root array and transitive object should both move"
    );
}

#[test]
fn test_copying_minor_rewrites_singleton_closure_caches() {
    struct SingletonClosureCacheGuard;

    impl Drop for SingletonClosureCacheGuard {
        fn drop(&mut self) {
            crate::closure::test_clear_singleton_closure_caches();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _cache_guard = SingletonClosureCacheGuard;
    crate::closure::test_clear_singleton_closure_caches();
    gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);

    let no_capture_func = test_no_capture_singleton_func as *const u8;
    let no_capture = crate::closure::js_closure_alloc_singleton(no_capture_func);
    assert_eq!(
        crate::closure::test_singleton_closure_cache_entry(no_capture_func),
        Some(no_capture)
    );

    let captured_value = young_leaf();
    let capture_bits = ptr_bits(captured_value);
    js_shadow_slot_set(0, capture_bits);

    let captured_func = test_captured_singleton_func as *const u8;
    let captures = [capture_bits];
    let captured = crate::closure::js_closure_alloc_with_captures_singleton(
        captured_func,
        1,
        captures.as_ptr(),
    );
    assert_eq!(
        crate::closure::js_closure_alloc_with_captures_singleton(
            captured_func,
            1,
            captures.as_ptr(),
        ),
        captured,
        "captured singleton cache should hit before GC"
    );

    let before_entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func);
    assert_eq!(before_entries.len(), 1);
    assert_eq!(before_entries[0].0, vec![capture_bits]);
    assert_eq!(before_entries[0].1, captured);

    let capture_slot = unsafe {
        (captured as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64
    };
    assert_eq!(unsafe { *capture_slot }, capture_bits);

    activate_malloc_registry_for_tests();
    js_shadow_slot_set(0, 0);
    let _ = gc_collect_minor();

    let no_capture_after = crate::closure::test_singleton_closure_cache_entry(no_capture_func)
        .expect("no-capture singleton cache should remain populated");
    assert_ne!(
        no_capture_after, no_capture,
        "managed no-capture singleton should be rewritten after copied-minor"
    );
    assert_eq!(
        crate::closure::js_closure_alloc_singleton(no_capture_func),
        no_capture_after,
        "no-capture singleton should remain a cache hit across copied-minor"
    );

    let after_entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func);
    assert_eq!(after_entries.len(), 1);
    let captured_after = after_entries[0].1;
    assert_eq!(
        crate::arena::classify_heap_space(captured_after as usize),
        crate::arena::active_survivor_space()
    );
    assert_ne!(
        captured_after, captured,
        "captured singleton closure should be rewritten after copied-minor"
    );

    let capture_after_slot = unsafe {
        (captured_after as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *mut u64
    };
    let capture_after_bits = unsafe { *capture_after_slot };
    let capture_after = (capture_after_bits & POINTER_MASK) as usize;
    assert_ne!(
        capture_after, captured_value,
        "captured young value should move out of eden"
    );
    assert_eq!(
        crate::arena::classify_heap_space(capture_after),
        crate::arena::active_survivor_space()
    );

    assert_eq!(after_entries[0].1, captured_after);
    assert_eq!(
        after_entries[0].0,
        vec![capture_after_bits],
        "captured-cache key should be rewritten to the moved capture"
    );

    let rewritten_captures = [capture_after_bits];
    assert_eq!(
        crate::closure::js_closure_alloc_with_captures_singleton(
            captured_func,
            1,
            rewritten_captures.as_ptr(),
        ),
        captured_after,
        "future cache lookups should hit with the rewritten capture key"
    );
}

#[test]
fn test_copying_minor_rewrites_overflow_owner_metadata_key() {
    struct OverflowFieldsRootGuard;

    impl Drop for OverflowFieldsRootGuard {
        fn drop(&mut self) {
            crate::object::test_clear_overflow_fields_root();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _overflow_guard = OverflowFieldsRootGuard;
    crate::object::test_clear_overflow_fields_root();

    let owner = crate::object::js_object_alloc(0, 0) as usize;
    let overflow_value = young_leaf();
    crate::object::test_seed_overflow_fields_root(owner, ptr_bits(overflow_value));
    js_shadow_slot_set(0, ptr_bits(owner));

    let _ = gc_collect_minor();
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let (mapped_owner, mapped_value_bits) = crate::object::test_overflow_fields_root();
    let mapped_value = (mapped_value_bits & POINTER_MASK) as usize;

    assert_ne!(owner_after, owner);
    assert_eq!(mapped_owner, owner_after);
    assert_ne!(mapped_value, overflow_value);
    assert!(crate::arena::pointer_in_nursery(owner_after));
    assert!(crate::arena::pointer_in_nursery(mapped_value));
}

#[test]
fn test_copying_minor_rewrites_class_side_table_values_and_function_keys() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::object::test_clear_class_side_table_roots();
    gc_register_mutable_root_scanner(crate::object::scan_class_side_table_roots_mut);

    let value = young_leaf();
    let prototype_object = crate::object::js_object_alloc(0, 0) as usize;
    let parent_closure = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as usize;
    let key = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as usize;
    unsafe {
        init_test_closure(parent_closure as *mut u8);
        init_test_closure(key as *mut u8);
    }
    js_shadow_slot_set(0, ptr_bits(key));

    crate::object::test_seed_class_dynamic_prop_root(0x5401, "dyn", string_bits(value));
    crate::object::test_seed_class_prototype_method_root(0x5401, "proto", string_bits(value));
    crate::object::test_seed_class_prototype_method_value_root(0x5401, "bound", string_bits(value));
    crate::object::test_seed_class_prototype_object_root(0x5401, prototype_object);
    crate::object::test_seed_class_parent_closure_root(0x5401, parent_closure);
    crate::object::test_seed_function_class_id_key(ptr_bits(key), 0x8200_5401);

    let _ = gc_collect_minor();

    let dynamic_bits = crate::object::test_class_dynamic_prop_root_bits(0x5401, "dyn");
    let prototype_bits = crate::object::test_class_prototype_method_root_bits(0x5401, "proto");
    let cached_bits = crate::object::test_class_prototype_method_value_root_bits(0x5401, "bound");
    let prototype_object_after = crate::object::test_class_prototype_object_root_addr(0x5401);
    let parent_closure_after = crate::object::test_class_parent_closure_root_addr(0x5401);
    let value_after = (dynamic_bits & POINTER_MASK) as usize;
    let key_after_bits = js_shadow_slot_get(0);

    assert_eq!(dynamic_bits & TAG_MASK, STRING_TAG);
    assert_eq!(prototype_bits, dynamic_bits);
    assert_eq!(cached_bits, dynamic_bits);
    assert_ne!(value_after, value);
    assert!(crate::arena::pointer_in_nursery(value_after));
    assert_ne!(prototype_object_after, prototype_object);
    assert!(crate::arena::pointer_in_nursery(prototype_object_after));
    assert_ne!(parent_closure_after, parent_closure);
    assert!(crate::arena::pointer_in_nursery(parent_closure_after));
    assert_ne!(key_after_bits, ptr_bits(key));
    assert_eq!(
        crate::object::test_function_class_id_key_for_class(0x8200_5401),
        key_after_bits
    );
    assert_eq!(
        crate::object::function_class_id(f64::from_bits(key_after_bits)),
        0x8200_5401
    );
}

#[test]
fn test_copying_minor_rewrites_symbol_side_table_roots_and_lookups() {
    let _guard = CopyingNurseryTestGuard::new(1);
    crate::symbol::test_clear_symbol_side_table_roots();
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);

    let owner = crate::object::js_object_alloc(0, 0) as usize;
    let sym_key = unsafe { alloc_nursery_test_symbol() };
    let value = young_leaf();
    let static_sym_key = unsafe { alloc_nursery_test_symbol() };
    let static_value = young_leaf();
    js_shadow_slot_set(0, ptr_bits(owner));

    crate::symbol::test_seed_symbol_pointer_root(sym_key);
    crate::symbol::test_seed_symbol_pointer_root(static_sym_key);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            f64::from_bits(ptr_bits(owner)),
            f64::from_bits(ptr_bits(sym_key)),
            f64::from_bits(string_bits(value)),
        );
        crate::symbol::js_class_register_static_symbol(
            0x5402,
            f64::from_bits(ptr_bits(static_sym_key)),
            f64::from_bits(string_bits(static_value)),
        );
    }

    let _ = gc_collect_minor();

    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let entries = crate::symbol::test_symbol_property_roots(owner_after);
    assert_eq!(entries.len(), 1);
    let (sym_key_after, value_bits_after) = entries[0];
    let value_after = (value_bits_after & POINTER_MASK) as usize;
    let static_entries = crate::symbol::test_class_static_symbol_roots_for_class(0x5402);
    assert_eq!(static_entries.len(), 1);
    let (static_sym_key_after, static_value_bits_after) = static_entries[0];
    let static_value_after = (static_value_bits_after & POINTER_MASK) as usize;

    assert_ne!(owner_after, owner);
    assert_ne!(sym_key_after, sym_key);
    assert_ne!(value_after, value);
    assert_ne!(static_sym_key_after, static_sym_key);
    assert_ne!(static_value_after, static_value);
    assert!(crate::arena::pointer_in_nursery(owner_after));
    assert!(crate::arena::pointer_in_nursery(sym_key_after));
    assert!(crate::arena::pointer_in_nursery(value_after));
    assert!(crate::arena::pointer_in_nursery(static_sym_key_after));
    assert!(crate::arena::pointer_in_nursery(static_value_after));
    assert!(
        !crate::symbol::test_symbol_property_owner_exists(owner),
        "symbol side table should not keep the stale owner key after moving"
    );
    assert!(crate::symbol::test_symbol_pointer_root_contains(
        sym_key_after
    ));
    assert!(crate::symbol::test_symbol_pointer_root_contains(
        static_sym_key_after
    ));
    assert!(!crate::symbol::test_symbol_pointer_root_contains(sym_key));
    assert!(!crate::symbol::test_symbol_pointer_root_contains(
        static_sym_key
    ));

    let moved_owner = f64::from_bits(ptr_bits(owner_after));
    let moved_sym = f64::from_bits(ptr_bits(sym_key_after));
    let moved_static_sym = f64::from_bits(ptr_bits(static_sym_key_after));
    let class_ref = f64::from_bits(crate::value::INT32_TAG | 0x5402);
    unsafe {
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(moved_owner, moved_sym).to_bits(),
            value_bits_after
        );
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(class_ref, moved_static_sym).to_bits(),
            static_value_bits_after
        );
    }
}

#[test]
fn test_copying_minor_rewrites_old_overflow_object_child_without_reentrant_borrow() {
    struct OverflowFieldsRootGuard;

    impl Drop for OverflowFieldsRootGuard {
        fn drop(&mut self) {
            crate::object::test_clear_overflow_fields_root();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _overflow_guard = OverflowFieldsRootGuard;
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::object::test_clear_overflow_fields_root();

    let (owner, _) = unsafe { alloc_old_test_object(8) };
    let owner_addr = owner as usize;
    assert!(crate::arena::pointer_in_old_gen(owner_addr));
    js_shadow_slot_set(0, ptr_bits(owner_addr));

    for i in 0..8 {
        let name = format!("k{i}");
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(owner, key, i as f64);
    }

    let child = crate::object::js_object_alloc(0, 0) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    unsafe {
        assert_eq!((*child_header).obj_type, GC_TYPE_OBJECT);
    }
    assert!(crate::arena::pointer_in_nursery(child));

    let overflow_key = crate::string::js_string_from_bytes(b"k8".as_ptr(), 2);
    crate::object::js_object_set_field_by_name(
        owner,
        overflow_key,
        f64::from_bits(ptr_bits(child)),
    );
    assert_eq!(
        crate::object::test_overflow_field_bits(owner_addr, 8) & POINTER_MASK,
        child as u64
    );
    assert!(
        remembered_set_size() > 0,
        "old overflow slot write must enter remembered metadata"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after =
        (crate::object::test_overflow_field_bits(owner_addr, 8) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(owner_after, owner_addr);
    assert_ne!(child_after, child);
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert!(trace.copying_nursery.copied_objects >= 1);
    assert_eq!(trace.remembered_set.dirty_objects_scanned, 1);
    assert!(
        trace.remembered_set.dirty_pages_scanned <= 2,
        "old owner page plus overflow Vec page should bound copied-minor scanning"
    );
    assert!(
        trace.remembered_set.dirty_slots_scanned <= 32,
        "overflow regression should scan only the dirty owner slots"
    );

    for _ in 0..3 {
        let _ = gc_collect_minor();
    }
    let promoted = (crate::object::test_overflow_field_bits(owner_addr, 8) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_old_gen(promoted));
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(
        stats.checked_old_to_young_edges, 0,
        "old overflow edge should stop being old-to-young once the child promotes"
    );
    assert_eq!(stats.missing_edges, 0);
}
