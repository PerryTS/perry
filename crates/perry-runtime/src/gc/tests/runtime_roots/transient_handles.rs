use super::*;

#[test]
fn test_transient_runtime_handle_slots_mark_and_rewrite() {
    clear_marks();
    clear_mark_seeds();

    let nanbox_f64_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let nanbox_u64_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let raw_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let raw_string_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING);
    let heap_word_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();

    let old_nanbox_f64 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_nanbox_u64 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_raw = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_raw_string = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING);
    let old_heap_word = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nanbox_f64_user) as *mut GcHeader,
            old_nanbox_f64,
        );
        set_forwarding_address(
            header_from_user_ptr(nanbox_u64_user) as *mut GcHeader,
            old_nanbox_u64,
        );
        set_forwarding_address(header_from_user_ptr(raw_user) as *mut GcHeader, old_raw);
        set_forwarding_address(
            header_from_user_ptr(raw_string_user) as *mut GcHeader,
            old_raw_string,
        );
        set_forwarding_address(
            header_from_user_ptr(heap_word_user) as *mut GcHeader,
            old_heap_word,
        );
    }

    let scope = RuntimeHandleScope::new();
    let nanbox_f64 = scope.root_nanbox_f64(f64::from_bits(ptr_bits(nanbox_f64_user as usize)));
    let nanbox_u64 = scope.root_nanbox_u64(string_bits(nanbox_u64_user as usize));
    let raw = scope.root_raw_mut_ptr(raw_user);
    let raw_string = scope.root_string_ptr(raw_string_user as *const crate::StringHeader);
    let heap_word = scope.root_heap_word_u64(heap_word_user as u64);

    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut marker);
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(nanbox_f64_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(nanbox_u64_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(raw_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(raw_string_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(heap_word_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
    }

    let mut rewriter = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut rewriter);

    assert_eq!(
        nanbox_f64.get_nanbox_f64().to_bits(),
        ptr_bits(old_nanbox_f64 as usize)
    );
    assert_eq!(
        nanbox_u64.get_nanbox_u64(),
        string_bits(old_nanbox_u64 as usize)
    );
    assert_eq!(raw.get_raw_mut_ptr::<u8>(), old_raw);
    assert_eq!(
        raw_string.get_raw_const_ptr::<crate::StringHeader>() as *mut u8,
        old_raw_string
    );
    assert_eq!(heap_word.get_heap_word_u64(), old_heap_word as u64);
}

#[test]
fn test_transient_runtime_handle_scope_drop_removes_roots() {
    clear_marks();
    clear_mark_seeds();

    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    let valid_ptrs = build_valid_pointer_set();

    {
        let scope = RuntimeHandleScope::new();
        let _handle = scope.root_nanbox_u64(ptr_bits(user as usize));
        assert!(RuntimeHandleScope::active_len_for_tests() > 0);
    }
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), 0);

    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut marker);
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "dropped handle scopes must not retain transient roots"
        );
    }
}

#[test]
fn test_set_gc_field_rewrite_reindexes_elements() {
    clear_marks();
    clear_mark_seeds();
    crate::set::test_clear_set_roots();

    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let nursery_bits = ptr_bits(nursery_user as usize);
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, f64::from_bits(nursery_bits));

    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_bits = ptr_bits(old_user as usize);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let valid_ptrs = build_valid_pointer_set();
    unsafe {
        rewrite_heap_object_fields(header_from_user_ptr(set as *const u8), &valid_ptrs);
    }

    assert_eq!(crate::set::js_set_value_at(set, 0).to_bits(), old_bits);
    assert_eq!(crate::set::js_set_has(set, f64::from_bits(old_bits)), 1);
    assert_eq!(
        crate::set::js_set_has(set, f64::from_bits(nursery_bits)),
        0,
        "set lookup index should be rebuilt after element rewrites"
    );

    clear_marks();
    clear_mark_seeds();
    crate::set::test_clear_set_roots();
}

#[test]
fn test_transient_runtime_handle_string_concat_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let left_bytes = vec![b'a'; 600_000];
    let right_bytes = vec![b'b'; 600_000];
    let left = crate::string::js_string_from_bytes(left_bytes.as_ptr(), left_bytes.len() as u32);
    let right = crate::string::js_string_from_bytes(right_bytes.as_ptr(), right_bytes.len() as u32);

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = crate::string::js_string_concat(left, right);

    let result_scope = RuntimeHandleScope::new();
    let result_root = result_scope.root_string_ptr(result);
    drain_scheduled_minor_gc(before, "concat allocation");
    let result = result_root.get_raw_const_ptr::<crate::StringHeader>();
    unsafe {
        assert_eq!((*result).byte_len, 1_200_000);
        let data = (result as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        assert_eq!(*data, b'a');
        assert_eq!(*data.add(599_999), b'a');
        assert_eq!(*data.add(600_000), b'b');
        assert_eq!(*data.add(1_199_999), b'b');
    }
}

#[test]
fn test_dynamic_string_add_roots_left_string_across_rhs_coercion_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let left = crate::string::js_string_from_bytes(b"dyn-left-".as_ptr(), 9);
    let left_value = f64::from_bits(string_bits(left as usize));
    let left_scope = RuntimeHandleScope::new();
    let left_root = left_scope.root_string_ptr(left);

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::value::js_dynamic_string_or_number_add(
            left_value,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        )
    };

    let result_root = left_scope.root_nanbox_f64(result);
    drain_scheduled_minor_gc(before, "rhs ToString allocation");
    unsafe {
        assert_string_bytes(
            left_root.get_raw_const_ptr::<crate::StringHeader>(),
            b"dyn-left-",
        );
    }

    let result_value = crate::value::JSValue::from_bits(result_root.get_nanbox_u64());
    assert!(result_value.is_string());
    unsafe {
        assert_string_bytes(result_value.as_string_ptr(), b"dyn-left-undefined");
    }
}

#[test]
fn test_dynamic_bigint_add_roots_both_bigint_across_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    // #2908: a BigInt operator now requires BOTH operands to be BigInt
    // (`1n + 1` throws TypeError instead of coercing). This test exercises
    // the same GC-rooting guarantee — the left BigInt must survive a minor
    // GC scheduled by the right operand's allocation — but with a second
    // BigInt operand so the dynamic add takes the (now only legal) BigInt
    // path instead of the removed mixed-coercion path.
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let cases: [(&str, unsafe extern "C" fn(f64, f64) -> f64); 2] = [
        ("dynamic add", crate::value::js_dynamic_add),
        (
            "dynamic string-or-number add",
            crate::value::js_dynamic_string_or_number_add,
        ),
    ];

    for (name, op) in cases {
        let left = crate::bigint::js_bigint_from_i64(41);
        let left_value = crate::value::js_nanbox_bigint(left as i64);
        assert!(crate::arena::pointer_in_nursery(left as usize));
        let left_scope = RuntimeHandleScope::new();
        let left_root = left_scope.root_bigint_ptr(left as *const crate::bigint::BigIntHeader);

        let right = crate::bigint::js_bigint_from_i64(1);
        let right_value = crate::value::js_nanbox_bigint(right as i64);

        force_next_general_arena_alloc_slow();
        trigger_guard.make_arena_trigger_due();
        let before = gc_collection_count();
        let result = unsafe { op(left_value, right_value) };
        let result_root = left_scope.root_nanbox_f64(result);
        drain_scheduled_minor_gc(before, &format!("{name} both BigInt across GC"));

        let rooted_left = left_root.get_raw_const_ptr::<crate::bigint::BigIntHeader>();
        assert_eq!(crate::bigint::js_bigint_to_f64(rooted_left), 41.0);

        let result_value = crate::value::JSValue::from_bits(result_root.get_nanbox_u64());
        assert!(result_value.is_bigint(), "{name} should return a BigInt");
        assert_eq!(
            crate::bigint::js_bigint_to_f64(result_value.as_bigint_ptr()),
            42.0
        );
    }
}

#[test]
fn test_bigint_method_add_roots_receiver_across_rhs_number_coercion_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let receiver = crate::bigint::js_bigint_from_i64(41);
    let receiver_value = crate::value::js_nanbox_bigint(receiver as i64);
    assert!(crate::arena::pointer_in_nursery(receiver as usize));
    let receiver_scope = RuntimeHandleScope::new();
    let receiver_root =
        receiver_scope.root_bigint_ptr(receiver as *const crate::bigint::BigIntHeader);
    let args = [1.0_f64];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver_value,
            b"add".as_ptr() as *const i8,
            3,
            args.as_ptr(),
            args.len(),
        )
    };

    let result_root = receiver_scope.root_nanbox_f64(result);
    drain_scheduled_minor_gc(before, "BigInt method RHS number coercion");
    let rooted_receiver = receiver_root.get_raw_const_ptr::<crate::bigint::BigIntHeader>();
    assert_eq!(crate::bigint::js_bigint_to_f64(rooted_receiver), 41.0);

    let result_value = crate::value::JSValue::from_bits(result_root.get_nanbox_u64());
    assert!(
        result_value.is_bigint(),
        "BigInt add method should return BigInt"
    );
    assert_eq!(
        crate::bigint::js_bigint_to_f64(result_value.as_bigint_ptr()),
        42.0
    );
}

#[test]
fn test_string_method_split_roots_receiver_across_separator_materialization_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    register_runtime_handle_root_scanner_for_tests();

    let receiver = crate::string::js_string_from_bytes(b"a,b,c".as_ptr(), 5);
    let receiver_value = f64::from_bits(string_bits(receiver as usize));
    assert!(crate::arena::pointer_in_nursery(receiver as usize));
    let receiver_scope = RuntimeHandleScope::new();
    let receiver_root = receiver_scope.root_string_ptr(receiver);
    let sep = crate::value::JSValue::try_short_string(b",").unwrap();
    let args = [f64::from_bits(sep.bits())];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver_value,
            b"split".as_ptr() as *const i8,
            5,
            args.as_ptr(),
            args.len(),
        )
    };

    let result_root = receiver_scope.root_nanbox_f64(result);
    drain_scheduled_minor_gc(before, "split separator materialization");
    let rooted_receiver = receiver_root.get_raw_const_ptr::<crate::StringHeader>();
    unsafe {
        assert_string_bytes(rooted_receiver, b"a,b,c");
        let arr = (result_root.get_nanbox_u64() & POINTER_MASK) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(arr), 3);
        let expected: [&[u8]; 3] = [b"a", b"b", b"c"];
        for (i, expected) in expected.iter().enumerate() {
            let value = crate::array::js_array_get(arr, i as u32);
            assert!(value.is_string(), "split element {i} should be a string");
            assert_string_bytes(value.as_string_ptr(), expected);
        }
    }
}

#[test]
fn test_string_method_replace_roots_receiver_across_pattern_materialization_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    register_runtime_handle_root_scanner_for_tests();

    let receiver = crate::string::js_string_from_bytes(b"a-a".as_ptr(), 3);
    let receiver_value = f64::from_bits(string_bits(receiver as usize));
    assert!(crate::arena::pointer_in_nursery(receiver as usize));
    let receiver_scope = RuntimeHandleScope::new();
    let receiver_root = receiver_scope.root_string_ptr(receiver);
    let pattern = crate::value::JSValue::try_short_string(b"-").unwrap();
    let replacement = crate::value::JSValue::try_short_string(b":").unwrap();
    let args = [
        f64::from_bits(pattern.bits()),
        f64::from_bits(replacement.bits()),
    ];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe {
        crate::object::js_native_call_method(
            receiver_value,
            b"replace".as_ptr() as *const i8,
            7,
            args.as_ptr(),
            args.len(),
        )
    };

    let result_root = receiver_scope.root_nanbox_f64(result);
    drain_scheduled_minor_gc(before, "replace pattern materialization");
    let rooted_receiver = receiver_root.get_raw_const_ptr::<crate::StringHeader>();
    unsafe {
        assert_string_bytes(rooted_receiver, b"a-a");
        let result_value = crate::value::JSValue::from_bits(result_root.get_nanbox_u64());
        assert!(result_value.is_string(), "replace should return a string");
        assert_string_bytes(result_value.as_string_ptr(), b"a:a");
    }
}

#[test]
fn test_transient_runtime_handle_array_push_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let arr = crate::array::js_array_alloc_with_length(200_000);
    let value = crate::string::js_string_from_bytes(b"array-payload".as_ptr(), 13);
    let value_bits = string_bits(value as usize);

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let grown = crate::array::js_array_push_f64(arr, f64::from_bits(value_bits));

    let grown_scope = RuntimeHandleScope::new();
    let grown_root = grown_scope.root_raw_mut_ptr(grown);
    drain_scheduled_minor_gc(before, "array grow");
    let grown = grown_root.get_raw_const_ptr::<crate::array::ArrayHeader>();
    unsafe {
        assert_eq!((*grown).length, 200_001);
        let elements =
            (grown as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const u64;
        let stored = *elements.add(200_000);
        assert_eq!(stored & TAG_MASK, STRING_TAG);
        let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
        assert_string_bytes(stored_ptr, b"array-payload");
    }
}

#[test]
fn test_transient_runtime_handle_object_set_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let obj = crate::object::js_object_alloc(0, 1);
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let value = crate::string::js_string_from_bytes(b"object-payload".as_ptr(), 14);
    force_next_general_arena_alloc_slow();

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    crate::object::js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(string_bits(value as usize)),
    );

    drain_scheduled_minor_gc(before, "keys-array allocation");
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    unsafe {
        assert!(!(*obj_after).keys_array.is_null());
        let stored_value = crate::object::js_object_get_field(obj_after, 0).bits();
        assert_eq!(stored_value & TAG_MASK, STRING_TAG);
        let stored_value_ptr = (stored_value & POINTER_MASK) as *const crate::StringHeader;
        assert_string_bytes(stored_value_ptr, b"object-payload");

        let key_value = crate::array::js_array_get((*obj_after).keys_array, 0).bits();
        assert_eq!(key_value & TAG_MASK, STRING_TAG);
        let stored_key_ptr = (key_value & POINTER_MASK) as *const crate::StringHeader;
        assert_string_bytes(stored_key_ptr, b"name");
    }
}

#[test]
fn test_transient_runtime_handle_closure_captures_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    extern "C" fn captured_func(_closure: *const crate::closure::ClosureHeader) -> f64 {
        0.0
    }

    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    crate::closure::test_clear_singleton_closure_caches();

    let captured = crate::string::js_string_from_bytes(b"closure-payload".as_ptr(), 15);
    let captures = [string_bits(captured as usize)];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let closure = crate::closure::js_closure_alloc_with_captures_singleton(
        captured_func as *const u8,
        1,
        captures.as_ptr(),
    );

    let closure_scope = RuntimeHandleScope::new();
    let closure_root = closure_scope.root_raw_mut_ptr(closure);
    drain_scheduled_minor_gc(before, "closure arena allocation");
    let closure = closure_root.get_raw_const_ptr::<crate::closure::ClosureHeader>();
    unsafe {
        let capture_slot = (closure as *const u8)
            .add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        let stored = *capture_slot;
        assert_eq!(stored & TAG_MASK, STRING_TAG);
        let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
        assert_string_bytes(stored_ptr, b"closure-payload");
    }

    let entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func as *const u8);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0.len(), 1);
    assert_eq!(entries[0].0[0] & TAG_MASK, STRING_TAG);
    let cached_capture = (entries[0].0[0] & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(cached_capture, b"closure-payload");
    }
    crate::closure::test_clear_singleton_closure_caches();
}
