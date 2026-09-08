use super::*;

unsafe fn parse(source: &str) -> crate::JSValue {
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    crate::json::test_json_parse_direct(text)
}

unsafe fn output(value: crate::JSValue) -> serde_json::Value {
    let text = crate::json::js_json_stringify(f64::from_bits(value.bits()), 0);
    serde_json::from_slice(std::slice::from_raw_parts(
        crate::string::string_data(text),
        (*text).byte_len as usize,
    ))
    .unwrap()
}

#[test]
fn json_construction_during_incremental_marking_preserves_black_births() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = r#"{"child":{"text":"black birth child"},"array":[1,"heap string",{}]}"#;
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    let scope = RuntimeHandleScope::new();
    let text = scope.root_raw_mut_ptr(text);
    let mut state = GcCycleState::new_full(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    state.step(GcWorkBudget::bounded(1));
    assert_eq!(state.phase(), GcCyclePhase::BuildValidPointerSet);
    unsafe {
        {
            let _suppressed = crate::gc::GcSuppressScope::new();
            assert!(crate::arena::ConstructionBatch::new().is_none());
        }
        let value = text.with_mut_ptr(|text| crate::json::test_json_parse_direct(text));
        let root = scope.root_nanbox_u64(value.bits());
        assert_ne!(
            (*header_from_user_ptr(value.as_pointer::<u8>())).gc_flags & GC_FLAG_MARKED,
            0
        );
        for _ in 0..100_000 {
            if state.phase() == GcCyclePhase::Complete {
                break;
            }
            state.step(GcWorkBudget::bounded(1));
        }
        assert_eq!(state.phase(), GcCyclePhase::Complete);
        let _ = state.take_outcome().unwrap();
        assert_eq!(
            output(crate::JSValue::from_bits(root.get_nanbox_u64())),
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );
    }
}

#[test]
fn json_construction_wide_old_record_remembers_sparse_pointer_pages() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = format!(
        "{{{}}}",
        (0..20_000)
            .map(|i| {
                let value = if i % 509 == 0 || i == 19_999 {
                    format!(r#"{{"child":{i}}}"#)
                } else {
                    i.to_string()
                };
                format!(r#""key{i}":{value}"#)
            })
            .collect::<Vec<_>>()
            .join(",")
    );
    let scope = RuntimeHandleScope::new();
    unsafe {
        let value = parse(&source);
        assert!(crate::arena::pointer_in_old_gen(
            value.as_pointer::<u8>() as usize
        ));
        let root = scope.root_nanbox_u64(value.bits());
        // The generic indexed getter has a pre-existing 10,000-slot guard.
        // Verify the final physical slot directly after proving its bounds.
        let last_child = |value: crate::JSValue| {
            assert_eq!(
                crate::object::object_live_slot_count(value.as_pointer()),
                20_000
            );
            let slots = value
                .as_pointer::<u8>()
                .add(std::mem::size_of::<crate::ObjectHeader>())
                .cast::<crate::JSValue>();
            (*slots.add(19_999)).bits()
        };
        let last_before = last_child(value);
        assert!(crate::JSValue::from_bits(last_before).is_pointer());
        gc_collect_minor();
        let live = crate::JSValue::from_bits(root.get_nanbox_u64());
        let last_after = last_child(live);
        assert_ne!(last_before, last_after);
        assert_eq!(
            output(live),
            serde_json::from_str::<serde_json::Value>(&source).unwrap()
        );
    }
}

#[test]
fn json_construction_fifty_thousand_records_keep_old_to_young_edges() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = format!(
        "[{}]",
        (0..50_000)
            .map(|i| format!(r#"{{"id":{i},"text":"child value {i}","values":[{i},true,null]}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let scope = RuntimeHandleScope::new();
    unsafe {
        let before = gc_collection_count();
        let value = parse(&source);
        assert_eq!(
            gc_collection_count(),
            before,
            "construction must not collect"
        );
        let root = scope.root_nanbox_u64(value.bits());
        let child_before = crate::array::js_array_get(value.as_pointer(), 23_456).bits();
        assert!(crate::arena::pointer_in_old_gen(
            value.as_pointer::<u8>() as usize
        ));
        gc_collect_minor();
        assert!(gc_collection_count() > before);
        let live = crate::JSValue::from_bits(root.get_nanbox_u64());
        let child_after = crate::array::js_array_get(live.as_pointer(), 23_456).bits();
        assert_ne!(
            child_before, child_after,
            "the subject child must actually move"
        );
        assert_eq!(
            output(live),
            serde_json::from_str::<serde_json::Value>(&source).unwrap()
        );
    }
}

#[test]
fn json_construction_retaining_one_child_does_not_retain_its_batch() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let scope = RuntimeHandleScope::new();
    unsafe {
        let source = format!(
            "[{}]",
            (0..1_000)
                .map(|i| format!(r#"{{"id":{i},"name":"retained child {i}"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let value = parse(&source);
        let child = crate::array::js_array_get(value.as_pointer(), 417);
        let root = scope.root_nanbox_u64(child.bits());
        // No root for the parent array or any sibling.
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_ne!(root.get_nanbox_u64(), child.bits());
        assert!(
            trace.copying_nursery.copied_objects < 100,
            "retaining one record must not copy its 999 siblings: {}",
            trace.copying_nursery.copied_objects
        );
        assert_eq!(
            output(crate::JSValue::from_bits(root.get_nanbox_u64())),
            serde_json::json!({"id":417,"name":"retained child 417"})
        );
    }
}

#[test]
fn json_construction_growth_mixed_layouts_and_errors_leave_a_walkable_heap() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    for item in [
        "17",
        "1e-310",
        "true",
        "null",
        r#""short""#,
        r#""heap Unicode 日本語 😀""#,
        r#"{"x":1,"x":2}"#,
        r#"[true,"heap string",null,3]"#,
    ] {
        for width in [0, 1, 8, 9, 17, 257] {
            let source = format!("[{}]", vec![item; width].join(","));
            let scope = RuntimeHandleScope::new();
            unsafe {
                let value = parse(&source);
                let root = scope.root_nanbox_u64(value.bits());
                gc_collect_minor();
                assert_eq!(
                    output(crate::JSValue::from_bits(root.get_nanbox_u64())),
                    serde_json::from_str::<serde_json::Value>(&source).unwrap(),
                    "{item}/{width}"
                );
            }
        }
    }
    for malformed in [
        r#"{"a":[1,2,{"b":"heap string"}],"c":[{"x":]}"#,
        r#"["heap string",{"x":1},[true,null],]"#,
    ] {
        unsafe {
            let text = crate::js_string_from_bytes(malformed.as_ptr(), malformed.len() as u32);
            assert!(crate::json::js_json_parse_result(text).is_err());
        }
        assert!(!crate::gc::gc_is_suppressed());
        gc_collect_minor();
        unsafe {
            assert_eq!(
                output(parse(r#"{"after":["still works",17]}"#)),
                serde_json::json!({"after":["still works",17]})
            );
        }
    }
}
