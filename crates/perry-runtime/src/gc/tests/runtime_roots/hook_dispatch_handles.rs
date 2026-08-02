use super::*;

#[test]
fn test_async_hook_option_lookup_roots_callbacks_across_copied_minor_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let init = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original = init as usize;
    let options = hook_options(&[(b"init", init)]);

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let _handle = crate::async_hooks::js_async_hooks_create_hook(options);
    drain_scheduled_minor_gc(before, "async hook option key lookup");

    let (callback, _resource_bits) = crate::async_hooks::test_async_hooks_scanner_snapshot();
    assert_eq!(assert_callable_closure(ptr_bits(callback)), original);
}

#[test]
fn test_closure_rest_dispatch_roots_args_during_rest_array_alloc_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    crate::closure::js_register_closure_rest(test_rest_first_value as *const u8, 0);
    let closure = crate::closure::js_closure_alloc(test_rest_first_value as *const u8, 0);
    let value = test_string_value(b"rest-dispatch");
    let args = [value];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe { crate::closure::js_closure_call_array(closure as i64, args.as_ptr(), 1) };
    let result_scope = RuntimeHandleScope::new();
    let result_root = result_scope.root_nanbox_f64(result);
    drain_scheduled_minor_gc(before, "rest-array creation");
    assert_string_value(result_root.get_nanbox_f64(), b"rest-dispatch");
}

#[test]
fn test_bound_timer_dispatch_roots_args_during_async_hook_init_gc() {
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::async_hooks::scan_async_hooks_roots_mut);
    gc_register_mutable_root_scanner(crate::timer::scan_timer_roots_mut);

    let init_hook =
        crate::closure::js_closure_alloc(test_async_hook_init_force_minor_gc as *const u8, 0);
    enable_async_hook(&[(b"init", init_hook)]);

    let timer_callback = crate::closure::js_closure_alloc(test_timer_capture_arg as *const u8, 0);
    let timer_callback_original = timer_callback as usize;
    let arg = test_string_value(b"bound-timer-arg");
    let arg_original = (arg.to_bits() & POINTER_MASK) as usize;

    let module_name = b"timers";
    let namespace =
        crate::object::js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
    let bound = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(bound, 0, namespace);
    let method = b"setTimeout";
    crate::closure::js_closure_set_capture_ptr(bound, 1, method.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(bound, 2, method.len() as i64);

    let timer_value = crate::closure::js_closure_call3(
        bound,
        f64::from_bits(ptr_bits(timer_callback as usize)),
        0.0,
        arg,
    );

    let timer_id = (timer_value.to_bits() & POINTER_MASK) as i64;
    let (callback, arg_bits) = crate::timer::test_callback_timer_snapshot(timer_id)
        .expect("scheduled callback timer should remain queued");
    assert_moved_closure_ptr(ptr_bits(callback), timer_callback_original);
    assert_moved_string_value(f64::from_bits(arg_bits), arg_original, b"bound-timer-arg");
    crate::timer::clearTimeout(timer_id);
}

#[test]
fn test_timer_tick_roots_callback_args_and_previous_context_across_hooks() {
    const ALS_HANDLE: i64 = -8_501;

    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::async_hooks::scan_async_hooks_roots_mut);
    gc_register_mutable_root_scanner(crate::timer::scan_timer_roots_mut);

    let before_hook =
        crate::closure::js_closure_alloc(test_async_hook_event_force_minor_gc as *const u8, 0);
    let after_hook =
        crate::closure::js_closure_alloc(test_async_hook_event_force_minor_gc as *const u8, 0);
    enable_async_hook(&[(b"before", before_hook), (b"after", after_hook)]);

    crate::async_context::clear_store(ALS_HANDLE);
    let callback = crate::closure::js_closure_alloc(test_timer_capture_arg as *const u8, 0);
    let arg = test_string_value(b"timer-tick-arg");
    let timer_args = [arg];
    let timer_id = unsafe {
        crate::timer::js_set_timeout_callback_args(callback as i64, 0.0, timer_args.as_ptr(), 1)
    };

    let previous = test_string_value(b"timer-previous-context");
    let previous_original = (previous.to_bits() & POINTER_MASK) as usize;
    crate::async_context::enter_with(ALS_HANDLE, previous);
    TEST_TIMER_ARG_BITS.with(|slot| slot.set(0));
    TEST_TIMER_CALLED.with(|slot| slot.set(false));

    let before = gc_collection_count();
    assert_eq!(crate::timer::js_callback_timer_tick(), 1);
    drain_scheduled_minor_gc(
        before,
        "timer before/after hooks should trigger copied-minor GC",
    );
    assert!(TEST_TIMER_CALLED.with(|slot| slot.get()));

    let restored = crate::async_context::get_store(ALS_HANDLE)
        .expect("timer tick should restore previous AsyncLocalStorage context");
    assert_moved_string_value(restored, previous_original, b"timer-previous-context");
    crate::async_context::clear_store(ALS_HANDLE);
    crate::timer::clearTimeout(timer_id);
}

#[test]
fn test_next_tick_previous_context_survives_hook_gc() {
    const ALS_HANDLE: i64 = -8_502;

    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::async_hooks::scan_async_hooks_roots_mut);
    gc_register_mutable_root_scanner(crate::builtins::scan_queued_microtask_roots_mut);

    let before_hook =
        crate::closure::js_closure_alloc(test_async_hook_event_force_minor_gc as *const u8, 0);
    enable_async_hook(&[(b"before", before_hook)]);

    crate::async_context::clear_store(ALS_HANDLE);
    let callback = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    crate::builtins::js_queue_next_tick(callback as i64);

    let previous = test_string_value(b"nexttick-previous-context");
    let previous_original = (previous.to_bits() & POINTER_MASK) as usize;
    crate::async_context::enter_with(ALS_HANDLE, previous);

    let before = gc_collection_count();
    crate::builtins::js_drain_queued_microtasks();
    drain_scheduled_minor_gc(
        before,
        "nextTick before hook should trigger copied-minor GC",
    );

    let restored = crate::async_context::get_store(ALS_HANDLE)
        .expect("nextTick should restore previous AsyncLocalStorage context");
    assert_moved_string_value(restored, previous_original, b"nexttick-previous-context");
    crate::async_context::clear_store(ALS_HANDLE);
}

#[test]
fn test_array_map_runtime_handles_survive_callback_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let input = test_string_value(b"array-map-payload");
    let input_ptr = (input.to_bits() & POINTER_MASK) as usize;
    let source = test_array_from_values(&[input]);
    let callback =
        crate::closure::js_closure_alloc(test_array_identity_force_minor_gc as *const u8, 0);

    let before = gc_collection_count();
    let result = crate::array::js_array_map(source, callback);
    drain_scheduled_minor_gc(
        before,
        "array.map callback should force copied-minor GC while result is runtime-held",
    );
    assert_eq!(crate::array::js_array_length(result), 1);

    let stored = crate::array::js_array_get(result, 0).bits();
    assert_eq!(stored & TAG_MASK, STRING_TAG);
    let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
    assert_ne!(
        stored_ptr as usize, input_ptr,
        "mapped heap value should be rewritten to its copied-minor address"
    );
    unsafe {
        assert_string_bytes(stored_ptr, b"array-map-payload");
    }
}

#[test]
fn test_map_materializers_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let key = test_string_value(b"map-key");
    let key_original = (key.to_bits() & POINTER_MASK) as usize;
    let value = test_string_value(b"map-value");
    let value_original = (value.to_bits() & POINTER_MASK) as usize;
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, key, value);
    let map_scope = RuntimeHandleScope::new();
    let map_handle = map_scope.root_raw_mut_ptr(map);

    let before = gc_collection_count();
    crate::map::test_force_next_map_helper_gc();
    let entries = crate::map::js_map_entries(map_handle.get_raw_const_ptr());
    drain_scheduled_minor_gc(
        before,
        "Map.entries should force copied-minor GC while helper handles are live",
    );
    assert_eq!(crate::array::js_array_length(entries), 1);
    let pair_bits = crate::array::js_array_get(entries, 0).bits();
    assert_eq!(pair_bits & TAG_MASK, POINTER_TAG);
    let pair = (pair_bits & POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert_moved_string_value(
        crate::array::js_array_get_f64(pair, 0),
        key_original,
        b"map-key",
    );
    assert_moved_string_value(
        crate::array::js_array_get_f64(pair, 1),
        value_original,
        b"map-value",
    );

    crate::map::test_force_next_map_helper_gc();
    let keys = crate::map::js_map_keys(map_handle.get_raw_const_ptr());
    assert_eq!(crate::array::js_array_length(keys), 1);
    assert_moved_string_value(
        crate::array::js_array_get_f64(keys, 0),
        key_original,
        b"map-key",
    );

    crate::map::test_force_next_map_helper_gc();
    let values = crate::map::js_map_values(map_handle.get_raw_const_ptr());
    assert_eq!(crate::array::js_array_length(values), 1);
    assert_moved_string_value(
        crate::array::js_array_get_f64(values, 0),
        value_original,
        b"map-value",
    );
}

#[test]
fn test_map_from_array_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let key = test_string_value(b"map-from-array-key");
    let key_original = (key.to_bits() & POINTER_MASK) as usize;
    let value = test_string_value(b"map-from-array-value");
    let value_original = (value.to_bits() & POINTER_MASK) as usize;
    let pair = test_pair_array(key, value);
    let input = test_array_from_pair(pair);

    let before = gc_collection_count();
    crate::map::test_force_next_map_helper_gc();
    let map = crate::map::js_map_from_array(input);
    drain_scheduled_minor_gc(
        before,
        "Map-from-array should force copied-minor GC while input and output handles are live",
    );
    assert_eq!(crate::map::js_map_size(map), 1);
    assert_moved_string_value(
        crate::map::js_map_entry_key_at(map, 0),
        key_original,
        b"map-from-array-key",
    );
    assert_moved_string_value(
        crate::map::js_map_entry_value_at(map, 0),
        value_original,
        b"map-from-array-value",
    );
}

#[test]
fn test_structured_clone_map_runtime_handles_survive_nested_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let outer_key = test_string_value(b"clone-map-outer-key");
    let outer_key_original = (outer_key.to_bits() & POINTER_MASK) as usize;
    let inner_key = test_string_value(b"clone-map-inner-key");
    let inner_key_original = (inner_key.to_bits() & POINTER_MASK) as usize;
    let inner_value = test_string_value(b"clone-map-inner-value");
    let inner_value_original = (inner_value.to_bits() & POINTER_MASK) as usize;

    let inner_map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(inner_map, inner_key, inner_value);
    let outer_map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(
        outer_map,
        outer_key,
        f64::from_bits(ptr_bits(inner_map as usize)),
    );

    let before = gc_collection_count();
    crate::map::test_force_next_map_helper_gc();
    crate::map::test_force_next_map_helper_gc();
    let cloned = crate::builtins::js_structured_clone(f64::from_bits(ptr_bits(outer_map as usize)));
    assert!(
        gc_collection_count() >= before + 2,
        "structuredClone(Map) should force copied-minor GC in outer and nested Map.entries"
    );

    let cloned_bits = cloned.to_bits();
    assert_eq!(cloned_bits & TAG_MASK, POINTER_TAG);
    let cloned_map = (cloned_bits & POINTER_MASK) as *mut crate::map::MapHeader;
    assert!(crate::map::is_registered_map(cloned_map as usize));
    assert_eq!(crate::map::js_map_size(cloned_map), 1);
    assert_moved_string_value(
        crate::map::js_map_entry_key_at(cloned_map, 0),
        outer_key_original,
        b"clone-map-outer-key",
    );

    let inner_clone_value = crate::map::js_map_entry_value_at(cloned_map, 0);
    let inner_clone_bits = inner_clone_value.to_bits();
    assert_eq!(inner_clone_bits & TAG_MASK, POINTER_TAG);
    let inner_clone = (inner_clone_bits & POINTER_MASK) as *mut crate::map::MapHeader;
    assert!(crate::map::is_registered_map(inner_clone as usize));
    assert_ne!(inner_clone as usize, inner_map as usize);
    assert_eq!(crate::map::js_map_size(inner_clone), 1);
    assert_moved_string_value(
        crate::map::js_map_entry_key_at(inner_clone, 0),
        inner_key_original,
        b"clone-map-inner-key",
    );
    assert_moved_string_value(
        crate::map::js_map_entry_value_at(inner_clone, 0),
        inner_value_original,
        b"clone-map-inner-value",
    );
}

#[test]
fn test_set_materializers_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let value = test_string_value(b"set-value");
    let value_original = (value.to_bits() & POINTER_MASK) as usize;
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, value);

    let before = gc_collection_count();
    crate::set::test_force_next_set_helper_gc();
    let arr = crate::set::js_set_to_array(set);
    drain_scheduled_minor_gc(
        before,
        "Set-to-array should force copied-minor GC while helper handles are live",
    );
    assert_eq!(crate::array::js_array_length(arr), 1);
    assert_moved_string_value(
        crate::array::js_array_get_f64(arr, 0),
        value_original,
        b"set-value",
    );

    let from_array_value = test_string_value(b"set-from-array");
    let from_array_original = (from_array_value.to_bits() & POINTER_MASK) as usize;
    let input = test_array_from_values(&[from_array_value]);
    crate::set::test_force_next_set_helper_gc();
    let from_array = crate::set::js_set_from_array(input);
    assert_eq!(crate::set::js_set_size(from_array), 1);
    assert_moved_string_value(
        crate::set::js_set_value_at(from_array, 0),
        from_array_original,
        b"set-from-array",
    );

    let iterable = test_string_value(b"ab");
    crate::set::test_force_next_set_helper_gc();
    let from_iterable = crate::set::js_set_from_iterable(iterable);
    assert_eq!(crate::set::js_set_size(from_iterable), 2);
    unsafe {
        let first = crate::set::js_set_value_at(from_iterable, 0);
        let second = crate::set::js_set_value_at(from_iterable, 1);
        assert_string_bytes(
            (first.to_bits() & POINTER_MASK) as *const crate::StringHeader,
            b"a",
        );
        assert_string_bytes(
            (second.to_bits() & POINTER_MASK) as *const crate::StringHeader,
            b"b",
        );
    }
}
