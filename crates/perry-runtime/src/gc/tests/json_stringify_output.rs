use super::super::*;
use super::support::*;

#[test]
fn large_native_json_output_is_an_individually_tracked_leaf() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let text = format!("[\"{}\"]", "abcdefgh".repeat(128 * 1024));
    let result = crate::json::json_string_from_native_output_bytes(text.as_bytes());

    assert!(malloc_user_ptr_tracked(result.cast()));
    unsafe {
        assert_eq!((*result).utf16_len, text.len() as u32);
        assert_eq!((*result).byte_len, text.len() as u32);
        assert_eq!((*result).capacity, text.len() as u32);
        assert_eq!((*result).flags, 0);
        assert_eq!(crate::json::str_from_header(result), Some(text.as_str()));
    }

    let unicode = format!("[\"{}\"]", "é😀".repeat(128 * 1024));
    let unicode_result = crate::json::json_string_from_native_output_bytes(unicode.as_bytes());
    assert!(malloc_user_ptr_tracked(unicode_result.cast()));
    unsafe {
        assert_eq!(
            (*unicode_result).utf16_len,
            unicode.encode_utf16().count() as u32
        );
        assert_eq!(
            crate::json::str_from_header(unicode_result),
            Some(unicode.as_str())
        );
    }
}

#[test]
fn large_json_output_is_a_complete_malloc_leaf_and_obeys_runtime_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let text = format!(
        "{{\"id\":1,\"text\":\"{}\"}}",
        "abcdefgh".repeat(128 * 1024)
    );
    let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let parsed = unsafe { crate::json::test_json_parse_direct(source) };
    let result_bits = unsafe {
        crate::json::js_json_stringify_full(
            f64::from_bits(parsed.bits()),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_NULL),
        ) as u64
    };
    let result = crate::JSValue::from_bits(result_bits).as_string_ptr();
    assert!(malloc_user_ptr_tracked(result.cast_mut().cast()));
    assert!(
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()) > malloc_object_count(),
        "one 1 MiB result must remain below the 32 MiB sweep budget"
    );
    unsafe {
        assert_eq!((*result).utf16_len, text.len() as u32);
        assert_eq!((*result).byte_len, text.len() as u32);
        assert_eq!((*result).capacity, text.len() as u32);
        assert_eq!((*result).flags, 0);
        assert_eq!(crate::json::str_from_header(result), Some(text.as_str()));
    }

    let scope = RuntimeHandleScope::new();
    let rooted = scope.root_string_ptr(result);
    gc_collect_minor();
    let result_addr = rooted.with_const_ptr(|result: *const crate::StringHeader| {
        assert!(malloc_user_ptr_tracked(result.cast_mut().cast()));
        unsafe {
            assert_eq!(crate::json::str_from_header(result), Some(text.as_str()));
        }
        result as usize
    });

    drop(scope);
    // This test isolates root/lifetime behavior. Automatic scheduling is
    // covered by the repeated-output budget test below.
    gc_schedule_malloc_sweep_after_json_output();
    gc_collect_minor();
    assert!(
        !malloc_user_ptr_tracked(result_addr as *mut u8),
        "discarded JSON result must be reclaimed by the next malloc sweep"
    );
}

#[test]
fn repeated_large_exact_outputs_arm_sweep_only_after_a_completed_budget() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let text = format!(
        "{{\"id\":1,\"text\":\"{}\"}}",
        "abcdefgh".repeat(128 * 1024)
    );
    let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let parsed = unsafe { crate::json::test_json_parse_direct(source) };
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_u64(parsed.bits());
    let before = gc_collection_count();

    for iteration in 0..34 {
        let result_bits = unsafe {
            crate::json::js_json_stringify_full(
                input.get_nanbox_f64(),
                f64::from_bits(crate::value::TAG_NULL),
                f64::from_bits(crate::value::TAG_NULL),
            ) as u64
        };
        let result = crate::JSValue::from_bits(result_bits).as_string_ptr();
        unsafe {
            assert_eq!(crate::json::str_from_header(result), Some(text.as_str()));
        }
        if iteration == 0 {
            assert_eq!(
                gc_collection_count(),
                before,
                "one output must finish without synchronous collection"
            );
        }
    }

    assert!(
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()) <= malloc_object_count(),
        "crossing 32 MiB must leave malloc-leaf reclamation armed"
    );
    gc_collect_minor();
    assert!(gc_collection_count() > before);
}

fn exercise_large_parse_output_debt(use_result_entry: bool, block_boundary: bool) {
    use crate::gc::policy::unsafe_zone_test_override::set_unsafe_zone_blocked_for_test;
    struct BlockedBoundary(Option<bool>);
    impl Drop for BlockedBoundary {
        fn drop(&mut self) {
            set_unsafe_zone_blocked_for_test(self.0);
        }
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let scope = RuntimeHandleScope::new();
    let payload = "x".repeat(1024 * 1024);
    let sources: Vec<_> = (0..2)
        .map(|id| {
            let text = format!(r#"{{"id":{id},"text":"{payload}"}}"#);
            scope.root_string_ptr(crate::js_string_from_bytes(
                text.as_ptr(),
                text.len() as u32,
            ))
        })
        .collect();
    let malloc_before = malloc_object_count();
    let collections_before = gc_collection_count();
    let blocked =
        block_boundary.then(|| BlockedBoundary(set_unsafe_zone_blocked_for_test(Some(true))));
    let iterations = if block_boundary { 34 } else { 96 };

    for iteration in 0..iterations {
        let id = iteration & 1;
        let before_call = gc_collection_count();
        let value = sources[id].with_const_ptr(|source| unsafe {
            if use_result_entry {
                crate::json::js_json_parse_result(source).unwrap()
            } else {
                crate::json::js_json_parse(source)
            }
        });
        assert_eq!(
            crate::object::js_object_get_field(value.as_pointer(), 0).as_number(),
            id as f64,
            "alternating sources must not reuse the previous object's value"
        );
        let text = crate::object::js_object_get_field(value.as_pointer(), 1);
        unsafe {
            assert_eq!((*text.as_string_ptr()).byte_len as usize, payload.len());
        }
        if iteration == 31 {
            assert_eq!(
                gc_collection_count(),
                before_call,
                "construction must return before collecting its completed output"
            );
            assert!(
                GC_NEXT_MALLOC_TRIGGER.with(Cell::get) <= malloc_object_count(),
                "post-parse accounting must not cancel the completed 32 MiB output request"
            );
        }
        // Model the caller's back-edge after JSON.parse returns. Allocation
        // checks deliberately defer copying to this precise safepoint.
        let result_scope = RuntimeHandleScope::new();
        let result = result_scope.root_nanbox_u64(value.bits());
        js_gc_loop_safepoint();
        assert_eq!(
            crate::object::js_object_get_field(
                crate::JSValue::from_bits(result.get_nanbox_u64()).as_pointer(),
                0,
            )
            .as_number(),
            id as f64,
            "the completed result must survive the caller's collection"
        );
    }

    if block_boundary {
        assert_eq!(gc_collection_count(), collections_before);
        assert!(
            GC_NEXT_MALLOC_TRIGGER.with(Cell::get) <= malloc_object_count(),
            "a blocked boundary must preserve the completed-byte request"
        );
        drop(blocked);
        gc_check_trigger();
        js_gc_loop_safepoint();
    }
    assert!(
        gc_collection_count() > collections_before,
        "completed-output debt must cause an actual collection"
    );
    assert!(
        malloc_object_count() <= malloc_before + 40,
        "discarded malloc leaves must be reclaimed while the two inputs stay rooted"
    );
}

#[test]
fn repeated_large_parse_leaves_keep_sweep_armed_after_accounting() {
    exercise_large_parse_output_debt(false, false);
}

#[test]
fn repeated_large_parse_result_leaves_keep_sweep_armed_after_accounting() {
    exercise_large_parse_output_debt(true, false);
}

#[test]
fn large_parse_output_debt_survives_a_blocked_collection_boundary() {
    exercise_large_parse_output_debt(false, true);
}

#[test]
fn large_stringify_sweep_cadence_keeps_post_request_output_bytes() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let text = format!(r#"{{"text":"{}"}}"#, "x".repeat(1024 * 1024));
    let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let parsed = unsafe { crate::json::test_json_parse_direct(source) };
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_u64(parsed.bits());
    let expected_interval = (32 * 1024 * 1024usize).div_ceil(text.len());
    let mut collection_calls = Vec::new();
    for iteration in 0..120 {
        let before = gc_collection_count();
        let bits = unsafe {
            crate::json::js_json_stringify_full(
                input.get_nanbox_f64(),
                f64::from_bits(crate::value::TAG_NULL),
                f64::from_bits(crate::value::TAG_NULL),
            ) as u64
        };
        let result_scope = RuntimeHandleScope::new();
        let result = result_scope.root_nanbox_u64(bits);
        js_gc_loop_safepoint();
        let value = crate::JSValue::from_bits(result.get_nanbox_u64());
        unsafe {
            assert_eq!(
                crate::json::str_from_header(value.as_string_ptr()),
                Some(text.as_str())
            );
        }
        if gc_collection_count() > before {
            collection_calls.push(iteration);
        }
    }
    assert!(
        collection_calls.len() >= 3,
        "must exercise repeated output sweeps"
    );
    for pair in collection_calls.windows(2) {
        assert!(
            pair[1] - pair[0] <= expected_interval,
            "acknowledging a deferred sweep must not discard bytes allocated after its request: {collection_calls:?}"
        );
    }
}
