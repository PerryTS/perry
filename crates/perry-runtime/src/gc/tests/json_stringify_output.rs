use super::super::*;
use super::support::*;

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
