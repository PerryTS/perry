use super::super::*;
use super::support::*;
use std::fmt::Write as _;

unsafe fn alloc_old_test_map(
    capacity: u32,
) -> (*mut crate::map::MapHeader, *mut u64, std::alloc::Layout) {
    let map = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::map::MapHeader>(),
        8,
        GC_TYPE_MAP,
    ) as *mut crate::map::MapHeader;
    let layout = std::alloc::Layout::from_size_align((capacity as usize * 16).max(8), 8)
        .expect("valid map entries layout");
    let entries = std::alloc::alloc_zeroed(layout) as *mut u64;
    assert!(!entries.is_null());
    (*map).size = 0;
    (*map).capacity = capacity;
    (*map).entries = entries as *mut f64;
    (map, entries, layout)
}

unsafe fn retire_old_test_map(
    map: *mut crate::map::MapHeader,
    entries: *mut u64,
    layout: std::alloc::Layout,
) {
    (*map).size = 0;
    (*map).capacity = 0;
    (*map).entries = std::ptr::null_mut();
    std::alloc::dealloc(entries as *mut u8, layout);
}

fn assert_verified_copied_minor(trace: &GcCycleTrace) {
    assert_copied_minor_trace(trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.phase_us.contains_key("evacuation_verify"),
        "PERRY_GC_VERIFY_EVACUATION should run the old-to-young verifier"
    );
}

unsafe fn assert_slot_rewritten_to_nursery(slot: *const u64, before: usize) -> usize {
    let after = (*slot & POINTER_MASK) as usize;
    assert_ne!(after, before);
    assert!(crate::arena::pointer_in_nursery(after));
    after
}

#[test]
fn shared_array_and_object_slot_helpers_preserve_young_children() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let array_child = young_leaf();
    let object_child = young_leaf();
    let (old_arr, array_slot) = unsafe { alloc_old_test_array(1) };
    let (old_obj, object_slot) = unsafe { alloc_old_test_object(1) };
    unsafe {
        layout_init_pointer_free(old_arr as *mut u8);
        layout_init_pointer_free(old_obj as *mut u8);
        crate::array::store_array_slot(old_arr, 0, ptr_bits(array_child));
        (*old_arr).length = 1;
        crate::object::store_object_field_slot(old_obj, 0, ptr_bits(object_child));
    }

    assert!(remembered_set_size() > 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_verified_copied_minor(&trace);
    unsafe {
        assert_slot_rewritten_to_nursery(array_slot, array_child);
        assert_slot_rewritten_to_nursery(object_slot, object_child);
    }
}

#[test]
fn map_and_set_external_helper_stores_preserve_young_children() {
    struct SetRootGuard;

    impl Drop for SetRootGuard {
        fn drop(&mut self) {
            crate::set::test_clear_set_roots();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(0);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _set_guard = SetRootGuard;
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::set::test_clear_set_roots();

    let map_child = young_leaf();
    let set_child = young_leaf();
    let (map, entries, layout) = unsafe { alloc_old_test_map(1) };
    unsafe {
        (*map).size = 1;
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            entries as usize,
            ptr_bits(map_child),
        );
    }

    let (set, set_elements, set_layout) = unsafe { alloc_old_test_set(1) };
    unsafe {
        (*set).size = 1;
        crate::gc::runtime_store_external_jsvalue_slot(
            set as usize,
            set_elements as usize,
            ptr_bits(set_child),
        );
    }

    assert!(remembered_set_size() > 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_verified_copied_minor(&trace);
    unsafe {
        assert_slot_rewritten_to_nursery(entries, map_child);
        retire_old_test_map(map, entries, layout);
        assert_slot_rewritten_to_nursery(set_elements, set_child);
        retire_old_test_set(set, set_elements, set_layout);
    }
}

#[test]
fn json_large_object_materialization_preserves_young_string_fields() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let mut json = String::from("{");
    for i in 0..4096 {
        if i != 0 {
            json.push(',');
        }
        write!(&mut json, "\"k{i}\":\"value_{i}\"").unwrap();
    }
    json.push('}');

    let text = crate::string::js_string_from_bytes(json.as_ptr(), json.len() as u32);
    let parsed = unsafe { crate::json::test_json_parse_direct(text) };
    let obj = (parsed.bits() & POINTER_MASK) as *mut crate::object::ObjectHeader;
    js_shadow_slot_set(0, parsed.bits());

    assert!(crate::arena::pointer_in_old_gen(obj as usize));
    let fields = unsafe {
        (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64
    };
    let first_child = unsafe { (*fields & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(first_child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let fields_after = unsafe {
        (obj_after as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64
    };

    assert_verified_copied_minor(&trace);
    unsafe {
        assert_slot_rewritten_to_nursery(fields_after, first_child);
    }
}

#[test]
fn regex_global_result_array_preserves_young_match_strings() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let source_bytes = "x".repeat(4096);
    let source =
        crate::string::js_string_from_bytes(source_bytes.as_ptr(), source_bytes.len() as u32);
    let pattern = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
    let flags = crate::string::js_string_from_bytes(b"g".as_ptr(), 1);
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_handle = scope.root_string_ptr(source);
    let pattern_handle = scope.root_string_ptr(pattern);
    let flags_handle = scope.root_string_ptr(flags);
    let regex = crate::regex::js_regexp_new(
        pattern_handle.get_raw_const_ptr::<crate::StringHeader>(),
        flags_handle.get_raw_const_ptr::<crate::StringHeader>(),
    );
    let regex_handle = scope.root_raw_const_ptr(regex);

    let result = crate::regex::js_string_match(
        source_handle.get_raw_const_ptr::<crate::StringHeader>(),
        regex_handle.get_raw_const_ptr::<crate::regex::RegExpHeader>(),
    );
    assert!(crate::arena::pointer_in_old_gen(result as usize));
    js_shadow_slot_set(0, ptr_bits(result as usize));
    let elements = unsafe {
        (result as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    let first_match = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(first_match));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let result_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::array::ArrayHeader;
    let elements_after = unsafe {
        (result_after as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };

    assert_verified_copied_minor(&trace);
    unsafe {
        assert_slot_rewritten_to_nursery(elements_after, first_match);
    }
}

#[test]
fn plugin_and_promise_field_population_helpers_preserve_young_children() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _plugin_registry_guard = crate::plugin::PLUGIN_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let plugin_name = young_leaf();
    let tool_name = young_leaf();
    let promise_child = young_leaf();
    let (plugin_obj, plugin_fields) = unsafe { alloc_old_test_object(4) };
    let (tool_obj, tool_fields) = unsafe { alloc_old_test_object(3) };
    let (promise_obj, promise_fields) = unsafe { alloc_old_test_object(3) };

    unsafe {
        layout_init_pointer_free(plugin_obj as *mut u8);
        layout_init_pointer_free(tool_obj as *mut u8);
        layout_init_pointer_free(promise_obj as *mut u8);
        crate::plugin::test_store_plugin_metadata_fields(
            plugin_obj,
            1.0,
            f64::from_bits(string_bits(plugin_name)),
            2.0,
            3.0,
        );
        crate::plugin::test_store_tool_metadata_fields(
            tool_obj,
            f64::from_bits(string_bits(tool_name)),
            4.0,
            5.0,
        );
        crate::promise::test_store_with_resolvers_result_fields(
            promise_obj,
            f64::from_bits(ptr_bits(promise_child)),
            6.0,
            7.0,
        );
    }

    assert!(remembered_set_size() > 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_verified_copied_minor(&trace);
    unsafe {
        assert_slot_rewritten_to_nursery(plugin_fields.add(1), plugin_name);
        assert_slot_rewritten_to_nursery(tool_fields, tool_name);
        assert_slot_rewritten_to_nursery(promise_fields, promise_child);
    }
}

#[test]
fn thread_materialized_array_and_object_helpers_preserve_young_children() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let array_child = young_leaf();
    let object_child = young_leaf();
    let (old_arr, array_slot) = unsafe { alloc_old_test_array(1) };
    let (old_obj, object_slot) = unsafe { alloc_old_test_object(1) };
    unsafe {
        layout_init_pointer_free(old_arr as *mut u8);
        layout_init_pointer_free(old_obj as *mut u8);
        crate::thread::test_store_thread_array_slot(old_arr, 0, ptr_bits(array_child));
        crate::thread::test_store_thread_object_field(old_obj, 0, ptr_bits(object_child));
    }

    assert!(remembered_set_size() > 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_verified_copied_minor(&trace);
    unsafe {
        assert_slot_rewritten_to_nursery(array_slot, array_child);
        assert_slot_rewritten_to_nursery(object_slot, object_child);
    }
}
