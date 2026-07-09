use super::*;

#[test]
fn test_map_set_foreach_runtime_handles_survive_callback_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let callback = crate::closure::js_closure_alloc(test_foreach_force_minor_gc as *const u8, 0);
    let callback_scope = RuntimeHandleScope::new();
    let callback_handle =
        callback_scope.root_nanbox_f64(f64::from_bits(ptr_bits(callback as usize)));

    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 1.0, test_string_value(b"map-foreach-a"));
    crate::map::js_map_set(map, 2.0, test_string_value(b"map-foreach-b"));
    TEST_FOREACH_FORCE_MINOR_VISITS.with(|visits| visits.set(0));
    let before_map = gc_collection_count();
    crate::map::js_map_foreach(
        map,
        callback_handle.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert!(
        gc_collection_count() > before_map,
        "Map.forEach callback should force copied-minor GC during JS re-entry"
    );
    assert_eq!(
        TEST_FOREACH_FORCE_MINOR_VISITS.with(|visits| visits.get()),
        2
    );

    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, test_string_value(b"set-foreach-a"));
    crate::set::js_set_add(set, test_string_value(b"set-foreach-b"));
    TEST_FOREACH_FORCE_MINOR_VISITS.with(|visits| visits.set(0));
    let before_set = gc_collection_count();
    crate::set::js_set_foreach(
        set,
        callback_handle.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert!(
        gc_collection_count() > before_set,
        "Set.forEach callback should force copied-minor GC during JS re-entry"
    );
    assert_eq!(
        TEST_FOREACH_FORCE_MINOR_VISITS.with(|visits| visits.get()),
        2
    );
}

#[test]
fn test_json_reviver_runtime_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);

    let input = br#"{"a":[{"b":"c"}],"d":1}"#;
    let setup_scope = RuntimeHandleScope::new();
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let text_handle = setup_scope.root_string_ptr(text);
    let reviver = crate::closure::js_closure_alloc(test_reviver_force_minor_gc as *const u8, 0);
    let reviver_handle = setup_scope.root_raw_const_ptr(reviver);

    let before = gc_collection_count();
    let parsed = unsafe {
        crate::json::js_json_parse_with_reviver(
            text_handle.get_raw_const_ptr::<crate::StringHeader>(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>() as i64,
        )
    };
    assert!(
        gc_collection_count() > before,
        "reviver callback should force copied-minor GC during traversal"
    );

    let scope = RuntimeHandleScope::new();
    let parsed_handle = scope.root_nanbox_u64(parsed.bits());
    let output = unsafe {
        crate::json::js_json_stringify(f64::from_bits(parsed_handle.get_nanbox_u64()), 0)
    };
    unsafe {
        assert_string_bytes(output, br#"{"a":[{"b":"c"}],"d":1}"#);
    }
}

#[test]
fn test_geisterhand_callback_then_json_reviver_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _callback_guard = RuntimeCallbackRootGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    gc_register_mutable_root_scanner(crate::geisterhand_registry::scan_geisterhand_roots_mut);
    let closure = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original = closure as usize;
    let value = f64::from_bits(ptr_bits(original));
    crate::geisterhand_registry::test_seed_geisterhand_roots(value, value, value);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let roots = crate::geisterhand_registry::test_geisterhand_roots_snapshot();
    for bits in [roots.0, roots.1, roots.2, roots.3] {
        assert_moved_callable_closure(bits, original);
    }

    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let input = br#"{"a":[{"b":"c"}],"d":1}"#;
    let setup_scope = RuntimeHandleScope::new();
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let text_handle = setup_scope.root_string_ptr(text);
    let reviver = crate::closure::js_closure_alloc(test_reviver_force_minor_gc as *const u8, 0);
    let reviver_handle = setup_scope.root_raw_const_ptr(reviver);

    let before = gc_collection_count();
    let parsed = unsafe {
        crate::json::js_json_parse_with_reviver(
            text_handle.get_raw_const_ptr::<crate::StringHeader>(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>() as i64,
        )
    };
    assert!(
        gc_collection_count() > before,
        "reviver callback should force copied-minor GC after Geisterhand scanner activity"
    );

    let scope = RuntimeHandleScope::new();
    let parsed_handle = scope.root_nanbox_u64(parsed.bits());
    let output = unsafe {
        crate::json::js_json_stringify(f64::from_bits(parsed_handle.get_nanbox_u64()), 0)
    };
    unsafe {
        assert_string_bytes(output, br#"{"a":[{"b":"c"}],"d":1}"#);
    }
}

#[test]
fn test_json_reviver_treats_closure_property_as_leaf_after_copied_minor_gc() {
    TEST_REVIVER_CLOSURE_VISITS.with(|visits| visits.set(0));
    let _guard = CopyingNurseryTestGuard::new(0);
    register_runtime_handle_root_scanner_for_tests();

    let scope = RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 1);
    let key = crate::string::js_string_from_bytes(b"fn".as_ptr(), 2);
    let closure = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original_obj = obj as usize;
    let original_closure = closure as usize;
    let obj_handle = scope.root_nanbox_u64(ptr_bits(obj as usize));
    let key_handle = scope.root_string_ptr(key);
    let closure_handle = scope.root_raw_const_ptr(closure);
    crate::object::js_object_set_field_by_name(
        (obj_handle.get_nanbox_u64() & POINTER_MASK) as *mut crate::object::ObjectHeader,
        key_handle.get_raw_const_ptr::<crate::StringHeader>() as *mut crate::StringHeader,
        f64::from_bits(ptr_bits(
            closure_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>() as usize,
        )),
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(
        obj_handle.get_nanbox_u64() & POINTER_MASK,
        original_obj as u64
    );
    assert_ne!(
        closure_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>() as usize,
        original_closure
    );

    let empty_key = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_handle = scope.root_nanbox_f64(f64::from_bits(string_bits(empty_key as usize)));
    let reviver = crate::closure::js_closure_alloc(test_reviver_count_closure_leaf as *const u8, 0);
    let reviver_handle = scope.root_raw_const_ptr(reviver);

    let revived = unsafe {
        crate::json::test_apply_reviver_for_value(
            crate::value::JSValue::from_bits(obj_handle.get_nanbox_u64()),
            empty_key_handle.get_nanbox_f64(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        )
    };
    assert_eq!(
        TEST_REVIVER_CLOSURE_VISITS.with(|visits| visits.get()),
        1,
        "closure-valued property should be passed to reviver exactly once"
    );

    let revived_obj = (revived.bits() & POINTER_MASK) as *const crate::object::ObjectHeader;
    let stored = crate::object::js_object_get_field(revived_obj, 0).bits();
    assert_eq!(stored & TAG_MASK, POINTER_TAG);
    assert_eq!(
        crate::closure::js_closure_call0(
            (stored & POINTER_MASK) as *const crate::closure::ClosureHeader
        ),
        0.0
    );
}

#[test]
fn test_json_tape_eager_materialization_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let object_input = br#"{"a":[{"b":"c"}],"d":1}"#;
    let object_tape = crate::json_tape::build_tape(object_input).unwrap();
    let object_js = {
        let hook = JsonTapeSafepointHookGuard::new(
            crate::json_tape::JsonTapeSafepoint::MaterializeObjectRooted,
        );
        let js = unsafe { crate::json_tape::materialize(&object_tape, object_input) };
        let original_obj = hook.fired_ptr();
        assert_ne!(
            js.bits() & POINTER_MASK,
            original_obj as u64,
            "root object handle should be rewritten after copied-minor GC"
        );
        js
    };
    let scope = RuntimeHandleScope::new();
    let object_handle = scope.root_nanbox_u64(object_js.bits());
    let output = unsafe {
        crate::json::js_json_stringify(f64::from_bits(object_handle.get_nanbox_u64()), 0)
    };
    unsafe {
        assert_string_bytes(output, br#"{"a":[{"b":"c"}],"d":1}"#);
    }

    let array_input = br#"[{"b":"c"},4]"#;
    let array_tape = crate::json_tape::build_tape(array_input).unwrap();
    let array_js = {
        let hook = JsonTapeSafepointHookGuard::new(
            crate::json_tape::JsonTapeSafepoint::MaterializeArrayRooted,
        );
        let js = unsafe { crate::json_tape::materialize(&array_tape, array_input) };
        let original_arr = hook.fired_ptr();
        assert_ne!(
            js.bits() & POINTER_MASK,
            original_arr as u64,
            "root array handle should be rewritten after copied-minor GC"
        );
        js
    };
    let array_handle = scope.root_nanbox_u64(array_js.bits());
    let output =
        unsafe { crate::json::js_json_stringify(f64::from_bits(array_handle.get_nanbox_u64()), 0) };
    unsafe {
        assert_string_bytes(output, br#"[{"b":"c"},4]"#);
    }
}

unsafe fn test_alloc_lazy_json_array(input: &[u8]) -> *mut crate::json_tape::LazyArrayHeader {
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let tape = crate::json_tape::build_tape(input).unwrap();
    let len = crate::json_tape::count_array_length(&tape.entries, 0);
    crate::json_tape::alloc_lazy_array(&tape.entries, 0, len, text)
}

#[test]
fn test_json_tape_lazy_get_header_handle_survives_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let input = br#"["lazy-alpha-long","lazy-beta-long"]"#;
    let hdr = {
        let hook =
            JsonTapeSafepointHookGuard::new(crate::json_tape::JsonTapeSafepoint::LazyArrayRooted);
        let hdr = unsafe { test_alloc_lazy_json_array(input) };
        let original_hdr = hook.fired_ptr();
        assert_ne!(
            hdr as usize, original_hdr,
            "alloc_lazy_array should return the refreshed lazy header after copied-minor GC"
        );
        hdr
    };
    let scope = RuntimeHandleScope::new();
    let hdr_handle = scope.root_raw_mut_ptr(hdr);
    let hook =
        JsonTapeSafepointHookGuard::new(crate::json_tape::JsonTapeSafepoint::LazyGetHeaderRooted);
    let value = unsafe { crate::json_tape::lazy_get(hdr_handle.get_raw_mut_ptr(), 0) };
    let original_hdr = hook.fired_ptr();
    let hdr_after = hdr_handle.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>();
    assert_ne!(
        hdr_after as usize, original_hdr,
        "lazy_get should refresh the rooted lazy header after copied-minor GC"
    );
    unsafe {
        let bitmap = (*hdr_after).materialized_bitmap;
        assert!(!bitmap.is_null());
        assert_ne!(*bitmap & 1, 0, "cold lazy_get should cache element 0");
    }

    assert!(
        value.is_string(),
        "lazy_get should materialize a heap string"
    );
    unsafe {
        assert_string_bytes(value.as_string_ptr(), b"lazy-alpha-long");
    }
}

#[test]
fn test_json_tape_force_materialize_sparse_cache_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let input = br#"[{"id":0},{"id":1},{"id":2},{"id":3}]"#;
    let hdr = unsafe { test_alloc_lazy_json_array(input) };
    let scope = RuntimeHandleScope::new();
    let hdr_handle = scope.root_raw_mut_ptr(hdr);
    let cached = unsafe { crate::json_tape::lazy_get(hdr_handle.get_raw_mut_ptr(), 2) };
    let cached_handle = scope.root_nanbox_u64(cached.bits());
    let before_force_hdr =
        hdr_handle.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>() as usize;

    let hook =
        JsonTapeSafepointHookGuard::new(crate::json_tape::JsonTapeSafepoint::ForceLazyArrayRooted);
    let arr = unsafe { crate::json_tape::force_materialize_lazy(hdr_handle.get_raw_mut_ptr()) };
    let original_arr = hook.fired_ptr();
    let hdr_after = hdr_handle.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>();
    assert_ne!(
        hdr_after as usize, before_force_hdr,
        "force materialization should refresh the rooted lazy header"
    );
    assert_ne!(
        arr as usize, original_arr,
        "force materialization should refresh the rooted array handle"
    );
    unsafe {
        assert_eq!((*hdr_after).materialized, arr);
        assert_eq!(
            crate::array::js_array_get(arr, 2).bits(),
            cached_handle.get_nanbox_u64(),
            "sparse cache hit should preserve element identity after copied-minor GC"
        );
    }

    let arr_handle = scope.root_nanbox_u64(ptr_bits(arr as usize));
    let output =
        unsafe { crate::json::js_json_stringify(f64::from_bits(arr_handle.get_nanbox_u64()), 0) };
    unsafe {
        assert_string_bytes(output, br#"[{"id":0},{"id":1},{"id":2},{"id":3}]"#);
    }
}

#[test]
fn test_promise_then_and_finally_handles_survive_setup_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);

    let scope = RuntimeHandleScope::new();
    let promise = crate::promise::js_promise_new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let callback =
        crate::closure::js_closure_alloc(test_promise_identity_force_minor_gc as *const u8, 0);
    let callback_handle = scope.root_raw_const_ptr(callback);

    let next = crate::promise::js_promise_then(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        std::ptr::null(),
    );
    let next_handle = scope.root_raw_mut_ptr(next);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    unsafe {
        let promise = promise_handle.get_raw_mut_ptr::<crate::promise::Promise>();
        assert_eq!(
            (*promise).on_fulfilled,
            callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>()
        );
        assert_eq!(
            (*promise).next,
            next_handle.get_raw_mut_ptr::<crate::promise::Promise>()
        );
    }

    let promise = crate::promise::js_promise_new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let on_finally =
        crate::closure::js_closure_alloc(test_promise_finally_force_minor_gc as *const u8, 0);
    let on_finally_handle = scope.root_raw_const_ptr(on_finally);

    let finally_next = crate::promise::js_promise_finally(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        on_finally_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    );
    let finally_next_handle = scope.root_raw_mut_ptr(finally_next);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    unsafe {
        let promise = promise_handle.get_raw_mut_ptr::<crate::promise::Promise>();
        assert!(!(*promise).on_fulfilled.is_null());
        assert!(!(*promise).on_rejected.is_null());
        assert!((*promise).next.is_null());
        assert!(!finally_next_handle
            .get_raw_mut_ptr::<crate::promise::Promise>()
            .is_null());
    }
}

#[test]
fn test_promise_combinator_setup_handles_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);

    let scope = RuntimeHandleScope::new();
    let all_value = test_string_value(b"all");
    let all_input = crate::promise::js_promise_resolved(all_value);
    let all_arr = test_array_from_values(&[
        f64::from_bits(ptr_bits(all_input as usize)),
        test_string_value(b"plain"),
    ]);
    let all = crate::promise::js_promise_all(all_arr);
    let all_handle = scope.root_raw_mut_ptr(all);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    drain_promise_microtasks_for_test();
    unsafe {
        let all = all_handle.get_raw_mut_ptr::<crate::promise::Promise>();
        assert_eq!((*all).state, crate::promise::PromiseState::Fulfilled);
        let arr = ((*all).value.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader;
        assert_eq!((*arr).length, 2);
    }

    let race_input = crate::promise::js_promise_resolved(test_string_value(b"race"));
    let race_arr = test_array_from_values(&[f64::from_bits(ptr_bits(race_input as usize))]);
    let race = crate::promise::js_promise_race(race_arr);
    let race_handle = scope.root_raw_mut_ptr(race);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    drain_promise_microtasks_for_test();
    unsafe {
        assert_eq!(
            (*race_handle.get_raw_mut_ptr::<crate::promise::Promise>()).state,
            crate::promise::PromiseState::Fulfilled
        );
    }

    let settled_input = crate::promise::js_promise_resolved(test_string_value(b"settled"));
    let settled_arr = test_array_from_values(&[f64::from_bits(ptr_bits(settled_input as usize))]);
    let settled = crate::promise::js_promise_all_settled(settled_arr);
    let settled_handle = scope.root_raw_mut_ptr(settled);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    drain_promise_microtasks_for_test();
    unsafe {
        let settled = settled_handle.get_raw_mut_ptr::<crate::promise::Promise>();
        assert_eq!((*settled).state, crate::promise::PromiseState::Fulfilled);
        let arr = ((*settled).value.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader;
        assert_eq!((*arr).length, 1);
    }

    let any_input = crate::promise::js_promise_resolved(test_string_value(b"any"));
    let any_arr = test_array_from_values(&[f64::from_bits(ptr_bits(any_input as usize))]);
    let any = crate::promise::js_promise_any(any_arr);
    let any_handle = scope.root_raw_mut_ptr(any);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    drain_promise_microtasks_for_test();
    unsafe {
        assert_eq!(
            (*any_handle.get_raw_mut_ptr::<crate::promise::Promise>()).state,
            crate::promise::PromiseState::Fulfilled
        );
    }
}

#[test]
fn test_promise_contexts_rekey_live_and_drop_dead_from_space_after_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);
    crate::promise::test_clear_promise_scanner_roots();

    let scope = RuntimeHandleScope::new();
    let live = crate::promise::js_promise_new();
    let live_original = live as usize;
    let live_handle = scope.root_raw_mut_ptr(live);
    let dead = crate::promise::js_promise_new();
    let dead_original = dead as usize;
    crate::promise::test_seed_promise_context(live, 0.0);
    crate::promise::test_seed_promise_context(dead, 0.0);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let live_after = live_handle.get_raw_mut_ptr::<crate::promise::Promise>() as usize;
    assert_ne!(live_after, live_original);
    let keys = crate::promise::test_promise_context_keys();
    assert!(keys.contains(&live_after));
    assert!(!keys.contains(&live_original));
    assert!(!keys.contains(&dead_original));
    crate::promise::test_clear_promise_scanner_roots();
}

#[test]
fn test_native_async_completion_token_roots_survive_copied_minor_gc() {
    let _native_async_guard = crate::promise::native_async::test_native_async_lock();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    crate::promise::native_async::test_reset_native_async_registry();
    gc_register_mutable_root_scanner(crate::promise::scan_native_async_completion_roots_mut);

    let token = crate::promise::js_native_async_completion_new(0);
    let promise = crate::promise::js_native_async_completion_promise(token);
    let original_promise = promise as usize;
    let result = crate::string::js_string_from_bytes(b"native-async-result".as_ptr(), 19);
    let handle = crate::string::js_string_from_bytes(b"native-async-handle".as_ptr(), 19);
    let result_bits = string_bits(result as usize);
    let handle_bits = string_bits(handle as usize);

    assert_eq!(
        crate::promise::js_native_async_completion_attach_handle(
            token,
            handle_bits,
            crate::promise::PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT
        ),
        crate::promise::PERRY_NATIVE_ASYNC_OK
    );
    assert_eq!(
        crate::promise::js_native_async_completion_resolve_bits(token, result_bits),
        crate::promise::PERRY_NATIVE_ASYNC_OK
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let (moved_promise, moved_payload, moved_handles) =
        crate::promise::native_async::test_native_async_slot_snapshot(token);
    assert_ne!(
        moved_promise, original_promise,
        "token promise slot should be rewritten after copied-minor GC"
    );
    assert_ne!(
        moved_payload.unwrap() & POINTER_MASK,
        result as u64,
        "token payload slot should be rewritten after copied-minor GC"
    );
    assert_ne!(
        moved_handles[0] & POINTER_MASK,
        handle as u64,
        "token attached handle slot should be rewritten after copied-minor GC"
    );

    assert_eq!(crate::promise::js_native_async_process_pending(), 1);
    let promise = moved_promise as *mut crate::promise::Promise;
    unsafe {
        assert_eq!((*promise).state, crate::promise::PromiseState::Fulfilled);
        let value_bits = (*promise).value.to_bits();
        assert_eq!(value_bits & TAG_MASK, STRING_TAG);
        assert_string_bytes(
            (value_bits & POINTER_MASK) as *const crate::StringHeader,
            b"native-async-result",
        );
    }
}

#[test]
fn test_microtask_dispatch_handles_survive_callback_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);

    let scope = RuntimeHandleScope::new();
    let value = test_string_value(b"microtask");
    let source = crate::promise::js_promise_resolved(value);
    let source_handle = scope.root_raw_mut_ptr(source);
    let callback =
        crate::closure::js_closure_alloc(test_promise_identity_force_minor_gc as *const u8, 0);
    let callback_handle = scope.root_raw_const_ptr(callback);
    let next = crate::promise::js_promise_then(
        source_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        std::ptr::null(),
    );
    let next_handle = scope.root_raw_mut_ptr(next);

    let before = gc_collection_count();
    drain_promise_microtasks_for_test();
    assert!(
        gc_collection_count() > before,
        "promise callback should force copied-minor GC after task pop"
    );

    unsafe {
        let next = next_handle.get_raw_mut_ptr::<crate::promise::Promise>();
        assert_eq!((*next).state, crate::promise::PromiseState::Fulfilled);
        let value_bits = (*next).value.to_bits();
        assert_eq!(value_bits & TAG_MASK, STRING_TAG);
        let value_ptr = (value_bits & POINTER_MASK) as *const crate::StringHeader;
        assert_string_bytes(value_ptr, b"microtask");
    }
}

#[cfg(feature = "full")]
#[test]
fn test_plugin_callback_registry_rewrites_young_closure_after_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _callback_guard = RuntimeCallbackRootGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::plugin::scan_plugin_roots_mut);

    let closure = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original = closure as usize;
    crate::plugin::test_seed_plugin_roots(ptr_bits(original));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let roots = crate::plugin::test_plugin_roots_snapshot();
    for bits in [
        roots.0, roots.1, roots.2, roots.3, roots.4, roots.5, roots.6,
    ] {
        assert_moved_callable_closure(bits, original);
    }
}

#[test]
fn test_geisterhand_callback_registry_rewrites_young_closure_after_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _callback_guard = RuntimeCallbackRootGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::geisterhand_registry::scan_geisterhand_roots_mut);

    let closure = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original = closure as usize;
    let value = f64::from_bits(ptr_bits(original));
    crate::geisterhand_registry::test_seed_geisterhand_roots(value, value, value);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let roots = crate::geisterhand_registry::test_geisterhand_roots_snapshot();
    for bits in [roots.0, roots.1, roots.2, roots.3] {
        assert_moved_callable_closure(bits, original);
    }
}

#[test]
fn test_ui_text_foreach_registry_rewrites_young_render_closure_after_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _callback_guard = RuntimeCallbackRootGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::ui_text_registry::scan_ui_text_registry_roots_mut);

    let closure = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    let original = closure as usize;
    let value = f64::from_bits(ptr_bits(original));
    crate::ui_text_registry::test_seed_ui_text_registry_roots(value, value);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let (state_bits, render_bits) = crate::ui_text_registry::test_ui_text_registry_roots_snapshot();
    assert_moved_callable_closure(state_bits, original);
    assert_moved_callable_closure(render_bits, original);
}

#[test]
fn test_promise_iter_result_mutable_scanner_rewrites_slot() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let initial = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    crate::promise::js_iter_result_set(initial, 0);

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    crate::promise::scan_iter_result_root_mut(&mut visitor);

    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
    crate::promise::js_iter_result_set(0.0, 0);
}

#[test]
fn test_promise_iter_result_raw_f64_slot_is_not_scanned_as_root() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let raw_pointer_like = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    crate::promise::js_iter_result_set_f64(raw_pointer_like, 0);

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    crate::promise::scan_iter_result_root_mut(&mut visitor);

    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        raw_pointer_like.to_bits(),
        "raw f64 iter-result slots must not be rewritten as GC roots"
    );
    assert_eq!(
        crate::promise::js_iter_result_get_value_f64().to_bits(),
        raw_pointer_like.to_bits()
    );
    crate::promise::js_iter_result_set(0.0, 0);
}

#[test]
fn test_promise_iter_result_raw_i32_slot_is_not_scanned_as_root() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let stale_pointer_like = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    crate::promise::js_iter_result_set(stale_pointer_like, 0);
    crate::promise::js_iter_result_set_i32(-17, 0);

    let mut marked = Vec::new();
    crate::promise::scan_iter_result_root(&mut |value| marked.push(value.to_bits()));

    assert!(
        marked.is_empty(),
        "raw i32 iter-result slots must not scan stale pointer-shaped JSValue bits"
    );
    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        crate::value::JSValue::int32(-17).bits()
    );
    assert_eq!(crate::promise::js_iter_result_get_value_i32(), -17);
    assert_eq!(crate::promise::js_iter_result_get_value_f64(), -17.0);
    crate::promise::js_iter_result_set(0.0, 0);
}

#[test]
fn test_promise_iter_result_raw_i1_slot_is_not_scanned_as_root() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let stale_pointer_like = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    crate::promise::js_iter_result_set(stale_pointer_like, 0);
    crate::promise::js_iter_result_set_i1(1, 0);

    let mut marked = Vec::new();
    crate::promise::scan_iter_result_root(&mut |value| marked.push(value.to_bits()));

    assert!(
        marked.is_empty(),
        "raw i1 iter-result slots must not scan stale pointer-shaped JSValue bits"
    );
    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(crate::promise::js_iter_result_get_value_i1(), 1);
    crate::promise::js_iter_result_set(0.0, 0);
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_root_slot() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    reset_global_roots();
    let fixture = ForwardedRootFixture::new();
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, fixture.nursery_bits);

    assert_panics_with("shadow stack roots", || {
        verify_mutable_root_slots(&fixture.valid_ptrs);
    });

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_runtime_scanner_slot() {
    let fixture = ForwardedRootFixture::new();
    crate::promise::test_seed_promise_scanner_roots(
        fixture.nursery_user as *mut crate::promise::Promise,
        fixture.nursery_value(),
        fixture.nursery_value(),
        fixture.nursery_user as *const crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::promise::Promise,
    );

    assert_panics_with("runtime mutable root scanner", || {
        let mut visitor =
            RuntimeRootVisitor::for_verify(&fixture.valid_ptrs, "runtime mutable root scanner");
        promise_mutable_root_scanner(&mut visitor);
    });

    crate::promise::test_clear_promise_scanner_roots();
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_dirty_range_slot() {
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let child_bits = POINTER_TAG | (child as u64 & POINTER_MASK);
    unsafe {
        *fields = child_bits;
    }
    js_write_barrier_slot(POINTER_TAG | old_obj as u64, fields as u64, child_bits);
    let valid_ptrs = build_valid_pointer_set();
    let old_child = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(header_from_user_ptr(child), old_child);
    }

    assert_panics_with("remembered dirty ranges", || {
        verify_remembered_dirty_ranges(&valid_ptrs);
    });

    remembered_set_clear();
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_heap_field() {
    clear_marks();
    let fixture = ForwardedRootFixture::new();
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = fixture.nursery_bits;
        let header = header_from_user_ptr(old_obj as *const u8);
        (*header).gc_flags |= GC_FLAG_MARKED;
        assert_panics_with("heap fields", || {
            verify_heap_object_fields(header, &fixture.valid_ptrs, "heap fields");
        });
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
}

#[test]
fn test_evacuation_verify_copy_only_pinned_root_allows_non_forwarded_target() {
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    unsafe {
        (*header_from_user_ptr(user)).gc_flags |= GC_FLAG_PINNED;
    }
    verify_copy_only_scanner_bits(
        POINTER_TAG | (user as u64 & POINTER_MASK),
        &valid_ptrs,
        "copy-only root scanner",
    );
    unsafe {
        (*header_from_user_ptr(user)).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_evacuation_verify_copy_only_root_rejects_forwarded_target() {
    let fixture = ForwardedRootFixture::new();
    assert_panics_with("copy-only root scanner", || {
        verify_copy_only_scanner_bits(
            fixture.nursery_bits,
            &fixture.valid_ptrs,
            "copy-only root scanner",
        );
    });
}

struct ForwardedRootFixture {
    valid_ptrs: ValidPointerSet,
    nursery_user: *mut u8,
    old_user: *mut u8,
    nursery_bits: u64,
    old_bits: u64,
}

impl ForwardedRootFixture {
    fn new() -> Self {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(nursery_user) as *mut GcHeader,
                old_user,
            );
        }
        Self {
            valid_ptrs,
            nursery_user,
            old_user,
            nursery_bits: POINTER_TAG | (nursery_user as u64 & POINTER_MASK),
            old_bits: POINTER_TAG | (old_user as u64 & POINTER_MASK),
        }
    }

    fn nursery_value(&self) -> f64 {
        f64::from_bits(self.nursery_bits)
    }

    fn old_addr(&self) -> usize {
        self.old_user as usize
    }

    fn nursery_addr(&self) -> usize {
        self.nursery_user as usize
    }

    fn nursery_i64(&self) -> i64 {
        self.nursery_user as i64
    }
}

fn clear_runtime_callback_roots_for_test() {
    #[cfg(feature = "full")]
    crate::plugin::test_clear_plugin_roots();
    crate::geisterhand_registry::test_clear_geisterhand_roots();
    crate::ui_text_registry::test_clear_ui_text_registry_roots();
}

static RUNTIME_CALLBACK_ROOT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct RuntimeCallbackRootGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _plugin_lock: std::sync::MutexGuard<'static, ()>,
}

impl RuntimeCallbackRootGuard {
    fn new() -> Self {
        let lock = RUNTIME_CALLBACK_ROOT_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let plugin_lock = crate::plugin::PLUGIN_REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_runtime_callback_roots_for_test();
        Self {
            _lock: lock,
            _plugin_lock: plugin_lock,
        }
    }
}

impl Drop for RuntimeCallbackRootGuard {
    fn drop(&mut self) {
        clear_runtime_callback_roots_for_test();
    }
}

fn assert_moved_callable_closure(bits: u64, original: usize) {
    let rewritten = assert_moved_closure_ptr(bits, original);
    assert_eq!(
        crate::closure::js_closure_call0(rewritten as *const crate::closure::ClosureHeader),
        0.0
    );
}

fn assert_moved_closure_ptr(bits: u64, original: usize) -> usize {
    assert_eq!(bits & TAG_MASK, POINTER_TAG);
    let rewritten = (bits & POINTER_MASK) as usize;
    assert_ne!(
        rewritten, original,
        "runtime callback root should be rewritten after copied-minor GC"
    );
    assert!(crate::arena::pointer_in_nursery(rewritten));
    rewritten
}

#[test]
fn test_gc_init_mutable_scanner_families_rewrite_runtime_slots() {
    let _async_hook_guard = AsyncHookRuntimeTestGuard::new();
    let _guard = CopyingNurseryTestGuard::new(0);
    crate::set::test_clear_set_roots();
    let _callback_guard = RuntimeCallbackRootGuard::new();
    let fixture = ForwardedRootFixture::new();
    let active_context_handle = -724_331;
    let shape_id = 0x51A9_E001;
    let box_ptr = crate::r#box::js_box_alloc(fixture.nursery_value());

    crate::promise::test_seed_promise_scanner_roots(
        fixture.nursery_user as *mut crate::promise::Promise,
        fixture.nursery_value(),
        fixture.nursery_value(),
        fixture.nursery_user as *const crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::promise::Promise,
    );
    crate::timer::test_seed_timer_scanner_roots(
        fixture.nursery_user as *mut crate::promise::Promise,
        fixture.nursery_value(),
        fixture.nursery_i64(),
        fixture.nursery_value(),
        fixture.nursery_value(),
    );
    crate::exception::test_set_exception(fixture.nursery_value());
    crate::async_context::clear_store(active_context_handle);
    crate::async_context::enter_with(active_context_handle, fixture.nursery_value());
    crate::builtins::test_seed_queued_microtask(fixture.nursery_i64(), fixture.nursery_value());
    crate::builtins::test_seed_queued_microtask_previous_context(fixture.nursery_value());
    crate::async_hooks::test_seed_async_hooks_scanner_roots(
        fixture.nursery_user as *const crate::closure::ClosureHeader,
        fixture.nursery_value(),
    );
    crate::object::test_seed_shape_cache_root(
        shape_id,
        fixture.nursery_user as *mut crate::array::ArrayHeader,
    );
    crate::regex::test_set_last_exec_groups(
        fixture.nursery_user as *mut crate::object::ObjectHeader,
    );
    crate::array::test_seed_template_raw_roots(
        fixture.nursery_user as *mut crate::array::ArrayHeader,
        fixture.nursery_user as *mut crate::array::ArrayHeader,
    );
    crate::object::test_seed_transition_cache_root(fixture.nursery_addr());
    crate::object::test_seed_object_cache_roots([fixture.nursery_bits; 7], fixture.nursery_i64());
    crate::object::test_seed_class_dynamic_prop_root(0x5501, "dyn", fixture.nursery_bits);
    crate::object::test_seed_class_prototype_method_root(0x5501, "proto", fixture.nursery_bits);
    crate::object::test_seed_class_prototype_method_value_root(
        0x5501,
        "bound",
        fixture.nursery_bits,
    );
    crate::object::test_seed_function_class_id_key(fixture.nursery_bits, 0x8200_5501);
    crate::json::test_seed_parse_roots(
        fixture.nursery_value(),
        fixture.nursery_user as *const crate::string::StringHeader,
    );
    crate::string::test_seed_intern_table_root(fixture.nursery_addr());
    crate::string::test_seed_small_int_cache_root(7, fixture.nursery_addr());
    crate::builtins::test_set_console_log_singleton(fixture.nursery_i64());
    crate::node_submodules::test_seed_node_submodule_roots(
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::object::ObjectHeader,
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::os::test_seed_process_event_listener_root(
        fixture.nursery_user as *const crate::closure::ClosureHeader,
    );
    crate::promise::js_iter_result_set(fixture.nursery_value(), 0);
    crate::promise::test_seed_async_step_thunk_cache(
        fixture.nursery_addr(),
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::closure::test_clear_singleton_closure_caches();
    crate::closure::test_seed_singleton_closure_cache(
        test_no_capture_singleton_func as *const u8,
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::closure::test_seed_captured_singleton_closure_cache(
        test_captured_singleton_func as *const u8,
        vec![fixture.nursery_bits],
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::tui::hooks::test_seed_hook_slot_roots(fixture.nursery_bits);
    crate::tui::state::test_reset_state_slots();
    let tui_state = crate::tui::state::js_perry_tui_state_alloc(fixture.nursery_value());
    #[cfg(feature = "full")]
    crate::plugin::test_seed_plugin_roots(fixture.nursery_bits);
    crate::geisterhand_registry::test_seed_geisterhand_roots(
        fixture.nursery_value(),
        fixture.nursery_value(),
        fixture.nursery_value(),
    );
    crate::ui_text_registry::test_seed_ui_text_registry_roots(
        fixture.nursery_value(),
        fixture.nursery_value(),
    );

    let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
    promise_mutable_root_scanner(&mut visitor);
    timer_mutable_root_scanner(&mut visitor);
    exception_mutable_root_scanner(&mut visitor);
    async_context_mutable_root_scanner(&mut visitor);
    async_hooks_mutable_root_scanner(&mut visitor);
    shape_cache_mutable_root_scanner(&mut visitor);
    crate::regex::scan_last_exec_groups_root_mut(&mut visitor);
    crate::array::scan_template_raw_roots_mut(&mut visitor);
    transition_cache_mutable_root_scanner(&mut visitor);
    crate::object::scan_object_cache_roots_mut(&mut visitor);
    crate::object::scan_class_side_table_roots_mut(&mut visitor);
    json_parse_mutable_root_scanner(&mut visitor);
    intern_table_mutable_root_scanner(&mut visitor);
    small_int_cache_mutable_root_scanner(&mut visitor);
    crate::builtins::scan_console_log_singleton_roots_mut(&mut visitor);
    crate::node_submodules::scan_node_submodule_singleton_roots_mut(&mut visitor);
    crate::os::scan_process_event_listener_roots_mut(&mut visitor);
    crate::r#box::scan_box_roots_mut(&mut visitor);
    crate::promise::scan_iter_result_root_mut(&mut visitor);
    crate::promise::scan_async_step_thunk_cache_mut(&mut visitor);
    crate::closure::scan_singleton_closure_roots_mut(&mut visitor);
    crate::tui::hooks::scan_hook_slot_roots_mut(&mut visitor);
    crate::tui::state::scan_state_slot_roots_mut(&mut visitor);
    #[cfg(feature = "full")]
    crate::plugin::scan_plugin_roots_mut(&mut visitor);
    crate::geisterhand_registry::scan_geisterhand_roots_mut(&mut visitor);
    crate::ui_text_registry::scan_ui_text_registry_roots_mut(&mut visitor);

    let promise = crate::promise::test_promise_scanner_snapshot();
    assert_eq!(promise.task_promise_ptr, fixture.old_addr());
    assert_eq!(promise.task_value_bits, fixture.old_bits);
    assert_eq!(promise.task_context_store_bits, fixture.old_bits);
    assert_eq!(promise.current_microtask_promise_ptr, fixture.old_addr());
    assert_eq!(promise.current_microtask_callback_ptr, fixture.old_addr());
    assert_eq!(promise.current_microtask_value_bits, fixture.old_bits);
    assert_eq!(promise.current_microtask_next_ptr, fixture.old_addr());
    assert_eq!(promise.inline_trap_next_ptr, fixture.old_addr());
    assert_eq!(promise.inline_trap_step_ptr, fixture.old_addr());
    assert_eq!(promise.async_step_guard_last_closure, fixture.old_addr());
    assert_eq!(promise.inline_callback_ptr, fixture.old_addr());
    assert_eq!(promise.inline_next_ptr, fixture.old_addr());
    assert_eq!(promise.inline_value_bits, fixture.old_bits);
    assert_eq!(promise.async_step_callback_ptr, fixture.old_addr());
    assert_eq!(promise.async_step_next_ptr, fixture.old_addr());
    assert_eq!(promise.async_step_value_bits, fixture.old_bits);
    assert_eq!(promise.promise_context_key, fixture.old_addr());
    assert_eq!(promise.promise_context_store_bits, fixture.old_bits);
    assert_eq!(
        promise.previous_microtask_context_store_bits,
        fixture.old_bits
    );
    assert_eq!(promise.scheduled_promise_ptr, fixture.old_addr());
    assert_eq!(promise.scheduled_value_bits, fixture.old_bits);

    let timer = crate::timer::test_timer_scanner_snapshot();
    assert_eq!(timer.timeout_promise_ptr, fixture.old_addr());
    assert_eq!(timer.timeout_value_bits, fixture.old_bits);
    assert_eq!(timer.callback_ptr, fixture.old_addr());
    assert_eq!(timer.callback_arg_bits, fixture.old_bits);
    assert_eq!(timer.callback_context_store_bits, fixture.old_bits);
    assert_eq!(timer.interval_callback_ptr, fixture.old_addr());
    assert_eq!(timer.interval_context_store_bits, fixture.old_bits);

    assert_eq!(
        crate::exception::js_get_exception().to_bits(),
        fixture.old_bits
    );
    assert_eq!(
        crate::async_context::get_store(active_context_handle)
            .map(f64::to_bits)
            .unwrap_or(0),
        fixture.old_bits
    );
    assert_eq!(
        crate::builtins::test_queued_microtask_snapshot(),
        (fixture.old_addr(), fixture.old_bits, fixture.old_bits)
    );
    assert_eq!(
        crate::async_hooks::test_async_hooks_scanner_snapshot(),
        (fixture.old_addr(), fixture.old_bits)
    );
    assert_eq!(
        crate::object::test_shape_cache_root(shape_id),
        (fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(crate::regex::test_last_exec_groups(), fixture.old_addr());
    assert_eq!(
        crate::array::test_template_raw_roots(),
        (fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(
        crate::object::test_transition_cache_root(),
        fixture.old_addr()
    );
    assert_eq!(
        crate::object::test_object_cache_roots(),
        ([fixture.old_bits; 7], fixture.old_addr() as i64)
    );
    assert_eq!(
        crate::object::test_class_dynamic_prop_root_bits(0x5501, "dyn"),
        fixture.old_bits
    );
    assert_eq!(
        crate::object::test_class_prototype_method_root_bits(0x5501, "proto"),
        fixture.old_bits
    );
    assert_eq!(
        crate::object::test_class_prototype_method_value_root_bits(0x5501, "bound"),
        fixture.old_bits
    );
    assert_eq!(
        crate::object::test_function_class_id_key_for_class(0x8200_5501),
        fixture.old_bits
    );
    assert_eq!(
        crate::json::test_parse_roots_snapshot(),
        (fixture.old_bits, fixture.old_addr())
    );
    assert_eq!(crate::string::test_intern_table_root(), fixture.old_addr());
    assert_eq!(
        crate::string::test_small_int_cache_root(7),
        fixture.old_addr()
    );
    assert_eq!(
        crate::builtins::test_console_log_singleton() as usize,
        fixture.old_addr()
    );
    assert_eq!(
        crate::node_submodules::test_node_submodule_roots(),
        (fixture.old_addr(), fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(
        crate::os::test_process_event_listener_root_snapshot(),
        fixture.old_addr()
    );
    assert_eq!(
        crate::r#box::js_box_get(box_ptr).to_bits(),
        fixture.old_bits
    );
    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        fixture.old_bits
    );
    assert_eq!(
        crate::promise::test_async_step_thunk_cache(),
        (fixture.old_addr(), fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(
        crate::closure::test_singleton_closure_cache_entry(
            test_no_capture_singleton_func as *const u8
        )
        .map(|ptr| ptr as usize),
        Some(fixture.old_addr())
    );
    assert_eq!(
        crate::closure::test_captured_singleton_closure_cache_entries(
            test_captured_singleton_func as *const u8
        ),
        vec![(
            vec![fixture.old_bits],
            fixture.old_user as *mut crate::closure::ClosureHeader
        )]
    );
    assert_eq!(
        crate::tui::hooks::test_hook_slot_roots(),
        (fixture.old_bits, fixture.old_bits, fixture.old_bits)
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(tui_state).to_bits(),
        fixture.old_bits
    );
    #[cfg(feature = "full")]
    assert_eq!(
        crate::plugin::test_plugin_roots_snapshot(),
        (
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits
        )
    );
    assert_eq!(
        crate::geisterhand_registry::test_geisterhand_roots_snapshot(),
        (
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits,
            fixture.old_bits
        )
    );
    assert_eq!(
        crate::ui_text_registry::test_ui_text_registry_roots_snapshot(),
        (fixture.old_bits, fixture.old_bits)
    );

    crate::promise::test_clear_promise_scanner_roots();
    crate::timer::test_clear_timer_scanner_roots(fixture.nursery_addr(), fixture.old_addr());
    crate::exception::js_clear_exception();
    crate::async_context::clear_store(active_context_handle);
    crate::object::test_clear_transition_cache_root();
    crate::object::test_clear_object_cache_roots();
    crate::set::test_clear_set_roots();
    crate::string::test_clear_intern_table_root();
    crate::string::test_clear_small_int_cache_root(7);
    crate::builtins::test_set_console_log_singleton(0);
    crate::async_hooks::reset_for_tests();
    crate::os::test_clear_process_event_listeners();
    crate::promise::js_iter_result_set(0.0, 0);
    crate::closure::test_clear_singleton_closure_caches();
    crate::tui::state::test_reset_state_slots();
}

#[cfg(feature = "ohos-napi")]
#[test]
fn test_arkts_callbacks_mutable_scanner_rewrites_callback_slots() {
    let fixture = ForwardedRootFixture::new();
    let callback_idx = 3;
    crate::arkts_callbacks::test_clear_arkts_callback_roots();
    crate::arkts_callbacks::test_seed_arkts_callback_root(callback_idx, fixture.nursery_value());

    let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
    crate::arkts_callbacks::arkts_callbacks_root_scanner_mut(&mut visitor);

    assert_eq!(
        crate::arkts_callbacks::test_arkts_callback_root(callback_idx),
        fixture.old_bits
    );
    crate::arkts_callbacks::test_clear_arkts_callback_roots();
}

#[cfg(feature = "ohos-napi")]
#[test]
fn test_lazy_media_mutable_scanner_rewrites_callback_slots() {
    let fixture = ForwardedRootFixture::new();
    let handle = i64::MIN + 377;
    crate::media_playback::test_seed_media_callback_roots(
        handle,
        fixture.nursery_value(),
        fixture.nursery_value(),
    );

    let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
    crate::media_playback::media_callbacks_root_scanner_mut(&mut visitor);

    assert_eq!(
        crate::media_playback::test_media_callback_roots(handle),
        (fixture.old_bits, fixture.old_bits)
    );
}

// ---------------------------------------------------------------------------
// 2026-07-09 GC audit, wave 1: raw-pointer windows across user callbacks.
// Each test forces a COPIED (moving) minor GC from inside the user callback —
// with the pre-fix code the helper kept reading through pre-move raw
// pointers / unrooted Rust buffers, publishing from-space addresses.
// ---------------------------------------------------------------------------

fn string_value_content(value: f64) -> String {
    let bits = value.to_bits();
    assert_eq!(
        bits & TAG_MASK,
        STRING_TAG,
        "expected a live string value, got bits {bits:#x}"
    );
    crate::string::string_as_str((bits & POINTER_MASK) as *const crate::StringHeader).to_string()
}

extern "C" fn test_sort_comparator_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let a_handle = scope.root_nanbox_f64(a);
    let b_handle = scope.root_nanbox_f64(b);
    let _ = crate::gc::gc_collect_minor();
    let a_str = string_value_content(a_handle.get_nanbox_f64());
    let b_str = string_value_content(b_handle.get_nanbox_f64());
    match a_str.cmp(&b_str) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

/// `Array.prototype.sort(comparator)` where every comparator call forces a
/// copied minor GC: the receiver, the #6076 temp copy, the merge scratch, and
/// the comparator closure itself must all be re-derived from rooted handles.
/// 40 elements exercises the run + bottom-up-merge engine (threshold 32); the
/// 8-element pass covers the pure insertion-sort path.
#[test]
fn test_array_sort_comparator_rooted_buffers_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    for count in [8usize, 40usize] {
        let scope = RuntimeHandleScope::new();
        let comparator =
            crate::closure::js_closure_alloc(test_sort_comparator_force_minor_gc as *const u8, 0);
        let comparator_handle = scope.root_raw_const_ptr(comparator);
        let arr = crate::array::js_array_alloc(count as u32);
        let arr_handle = scope.root_raw_mut_ptr(arr);
        for i in 0..count {
            // Descending heap strings ("s39", "s38", …) so the sort has real
            // work and stale pre-move addresses are observable as garbage.
            let text = format!("s{:02}", count - 1 - i);
            let sp = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let sp_handle = scope.root_string_ptr(sp);
            let arr = crate::array::js_array_push(
                arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
                crate::JSValue::from_bits(string_bits(
                    sp_handle.get_raw_const_ptr::<crate::StringHeader>() as usize,
                )),
            );
            arr_handle.set_raw_mut_ptr(arr);
        }

        let before = gc_collection_count();
        let sorted = crate::array::js_array_sort_with_comparator(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            comparator_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        );
        assert!(
            gc_collection_count() > before,
            "comparator should force copied-minor GCs during the sort"
        );
        unsafe {
            assert_eq!((*sorted).length as usize, count);
            let elems = (sorted as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *const f64;
            for i in 0..count {
                let got = string_value_content(*elems.add(i));
                assert_eq!(
                    got,
                    format!("s{i:02}"),
                    "element {i} of the {count}-element sort should hold the \
                     post-move string, not a stale pre-move address"
                );
            }
        }
    }
}

extern "C" fn test_tojson_force_minor_gc(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    test_string_value(b"tojson-out")
}

fn json_test_object_with_gc_forcing_tojson(scope: &RuntimeHandleScope) -> f64 {
    let to_json = crate::closure::js_closure_alloc(test_tojson_force_minor_gc as *const u8, 0);
    let to_json_handle = scope.root_raw_mut_ptr(to_json);
    let nested = crate::object::js_object_alloc(0, 1);
    let nested_handle = scope.root_raw_mut_ptr(nested);
    let key = crate::string::js_string_from_bytes(b"toJSON".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(
        nested_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key,
        f64::from_bits(ptr_bits(to_json_handle.get_raw_mut_ptr::<u8>() as usize)),
    );
    f64::from_bits(ptr_bits(nested_handle.get_raw_mut_ptr::<u8>() as usize))
}

fn set_string_field(
    scope: &RuntimeHandleScope,
    obj_handle: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    value: &[u8],
) {
    let value_handle = scope.root_nanbox_f64(test_string_value(value));
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key_ptr,
        value_handle.get_nanbox_f64(),
    );
}

/// `JSON.stringify` of an object whose MIDDLE field is a nested object with a
/// GC-forcing `toJSON`: after the callback moves the parent, the remaining
/// fields must be read through the re-derived keys/field buffers (the pre-fix
/// code hoisted `keys_elements`/`fields_ptr` once, before any user code).
#[test]
fn test_json_stringify_object_rederives_fields_after_tojson_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let scope = RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 3);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    set_string_field(&scope, &obj_handle, b"a", b"first");
    let nested_value = json_test_object_with_gc_forcing_tojson(&scope);
    let nested_handle = scope.root_nanbox_f64(nested_value);
    {
        let key_ptr = crate::string::js_string_from_bytes(b"b".as_ptr(), 1);
        crate::object::js_object_set_field_by_name(
            obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            key_ptr,
            nested_handle.get_nanbox_f64(),
        );
    }
    set_string_field(&scope, &obj_handle, b"c", b"last");

    let before = gc_collection_count();
    let out = unsafe {
        crate::json::js_json_stringify(
            f64::from_bits(ptr_bits(obj_handle.get_raw_mut_ptr::<u8>() as usize)),
            0,
        )
    };
    assert!(
        gc_collection_count() > before,
        "nested toJSON should force a copied-minor GC mid-stringify"
    );
    unsafe {
        assert_string_bytes(out, br#"{"a":"first","b":"tojson-out","c":"last"}"#);
    }
}

/// `JSON.stringify` of an array whose MIDDLE element serializes through a
/// GC-forcing `toJSON`: the elements after it must be read through the
/// re-derived element base (the pre-fix code hoisted `elements` once).
#[test]
fn test_json_stringify_array_rederives_elements_after_tojson_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let scope = RuntimeHandleScope::new();
    let arr = crate::array::js_array_alloc(3);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let first = scope.root_nanbox_f64(test_string_value(b"head"));
    let nested = scope.root_nanbox_f64(json_test_object_with_gc_forcing_tojson(&scope));
    let last = scope.root_nanbox_f64(test_string_value(b"tail"));
    for value in [&first, &nested, &last] {
        let arr = crate::array::js_array_push(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            crate::JSValue::from_bits(value.get_nanbox_f64().to_bits()),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }

    let before = gc_collection_count();
    let out = unsafe {
        crate::json::js_json_stringify(
            f64::from_bits(ptr_bits(arr_handle.get_raw_mut_ptr::<u8>() as usize)),
            0,
        )
    };
    assert!(
        gc_collection_count() > before,
        "element toJSON should force a copied-minor GC mid-stringify"
    );
    unsafe {
        assert_string_bytes(out, br#"["head","tojson-out","tail"]"#);
    }
}

extern "C" fn test_replacer_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    matched: f64,
    _offset: f64,
    _whole: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let matched_handle = scope.root_nanbox_f64(matched);
    let _ = crate::gc::gc_collect_minor();
    // The matched argument must still be a live string after the collection.
    let _ = string_value_content(matched_handle.get_nanbox_f64());
    test_string_value(b"<R>")
}

/// `String.prototype.replaceAll(pattern, fn)` where every replacer call
/// forces a copied minor GC: the subject's `&str` view must be re-derived
/// from the rooted handle for the between-match slices (the pre-fix code
/// cached `str_data` once and kept a live `match_indices` borrow across the
/// callbacks).
#[test]
fn test_string_replace_all_fn_rederives_subject_after_callback_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let scope = RuntimeHandleScope::new();
    let callback = crate::closure::js_closure_alloc(test_replacer_force_minor_gc as *const u8, 0);
    let callback_handle = scope.root_raw_mut_ptr(callback);
    let s = crate::string::js_string_from_bytes(b"one--two--three".as_ptr(), 15);
    let s_handle = scope.root_string_ptr(s);
    let pattern = crate::string::js_string_from_bytes(b"--".as_ptr(), 2);
    let pattern_handle = scope.root_string_ptr(pattern);

    let before = gc_collection_count();
    let out = crate::regex::js_string_replace_all_string_fn(
        s_handle.get_raw_const_ptr::<crate::StringHeader>(),
        pattern_handle.get_raw_const_ptr::<crate::StringHeader>(),
        f64::from_bits(ptr_bits(callback_handle.get_raw_mut_ptr::<u8>() as usize)),
    );
    assert!(
        gc_collection_count() > before,
        "replacer should force copied-minor GCs during replaceAll"
    );
    unsafe {
        assert_string_bytes(out, b"one<R>two<R>three");
    }
}

extern "C" fn test_bigint_comparator_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let a_handle = scope.root_nanbox_f64(a);
    let b_handle = scope.root_nanbox_f64(b);
    let _ = crate::gc::gc_collect_minor();
    let read = |value: f64| -> i64 {
        let bits = value.to_bits();
        let ptr = (bits & POINTER_MASK) as *const crate::bigint::BigIntHeader;
        let cleaned = crate::bigint::clean_bigint_ptr(ptr);
        assert!(!cleaned.is_null(), "comparator arg should be a live BigInt");
        unsafe { (*cleaned).limbs[0] as i64 }
    };
    let av = read(a_handle.get_nanbox_f64());
    let bv = read(b_handle.get_nanbox_f64());
    match av.cmp(&bv) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

/// `BigInt64Array.prototype.sort(comparator)` where every comparator call
/// forces a copied minor GC: the raw lanes are sorted through an owned Rust
/// buffer with two per-compare boxed (and rooted) BigInts — the pre-fix code
/// parked a `Vec<f64>` full of unrooted BigInt boxes across every call.
#[test]
fn test_bigint64_sort_comparator_boxes_survive_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let scope = RuntimeHandleScope::new();
    let comparator =
        crate::closure::js_closure_alloc(test_bigint_comparator_force_minor_gc as *const u8, 0);
    let comparator_handle = scope.root_raw_const_ptr(comparator);

    let lanes: [i64; 6] = [42, -7, 1_000_000_000_007, 0, -900_000_000_001, 13];
    let ta = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_BIGINT64, 6);
    unsafe {
        let base = crate::typedarray::data_ptr_mut(ta) as *mut i64;
        for (i, v) in lanes.iter().enumerate() {
            *base.add(i) = *v;
        }
    }

    let before = gc_collection_count();
    let sorted = crate::typedarray::js_typed_array_sort_with_comparator(
        ta,
        comparator_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
    );
    assert!(
        gc_collection_count() > before,
        "comparator should force copied-minor GCs during the BigInt64 sort"
    );
    unsafe {
        let base = crate::typedarray::data_ptr_mut(sorted) as *const i64;
        let mut expected = lanes;
        expected.sort_unstable();
        for (i, v) in expected.iter().enumerate() {
            assert_eq!(
                *base.add(i),
                *v,
                "lane {i} should hold the comparator-sorted value"
            );
        }
    }
}
