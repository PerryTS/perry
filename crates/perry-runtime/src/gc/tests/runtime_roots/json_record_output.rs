use super::*;

thread_local! { static GETTER_CALLS: Cell<u32> = const { Cell::new(0) }; }

extern "C" fn array_getter_collects(_closure: *const crate::ClosureHeader) -> f64 {
    GETTER_CALLS.with(|c| c.set(c.get() + 1));
    crate::gc::gc_collect_minor();
    17.0
}

#[test]
fn json_array_getter_runs_once_and_rederives_later_elements_after_movement() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = br#"[1,"tail after getter"]"#;
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    let value = unsafe { crate::json::test_json_parse_direct(text) };
    let scope = RuntimeHandleScope::new();
    let array = scope.root_nanbox_u64(value.bits());
    let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        array_getter_collects as *const u8,
        0,
    ));
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let get_key = crate::js_string_from_bytes(b"get".as_ptr(), 3);
    crate::object::js_object_set_field_by_name(
        descriptor.get_raw_mut_ptr(),
        get_key,
        f64::from_bits(ptr_bits(getter.get_raw_mut_ptr::<u8>() as usize)),
    );
    let index_key = crate::js_string_from_bytes(b"0".as_ptr(), 1);
    crate::object::js_object_define_property(
        array.get_nanbox_f64(),
        f64::from_bits(string_bits(index_key as usize)),
        f64::from_bits(ptr_bits(descriptor.get_raw_mut_ptr::<u8>() as usize)),
    );
    GETTER_CALLS.with(|c| c.set(0));
    let before_ptr = array.get_nanbox_u64();
    let before = gc_collection_count();
    let output = unsafe { crate::json::js_json_stringify(array.get_nanbox_f64(), 2) };
    assert_eq!(GETTER_CALLS.with(|c| c.get()), 1);
    assert!(gc_collection_count() > before);
    assert_ne!(
        array.get_nanbox_u64(),
        before_ptr,
        "getter must move its array"
    );
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(
                crate::string::string_data(output),
                (*output).byte_len as usize,
            )
        },
        br#"[17,"tail after getter"]"#
    );
}

#[test]
fn json_record_output_rederives_children_after_cold_prototype_collection() {
    assert_record_children_move(true, false);
}

#[test]
fn json_record_output_rederives_children_after_final_string_allocation() {
    assert_record_children_move(false, false);
}

#[test]
fn json_escaped_record_output_rederives_children_after_cold_prototype_collection() {
    assert_record_children_move(true, true);
}

#[test]
fn json_escaped_record_output_rederives_children_after_final_string_allocation() {
    assert_record_children_move(false, true);
}

#[test]
fn json_escaped_heap_string_rederives_source_after_final_allocation() {
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    let text = "quoted\"\n\\東京🙂".repeat(32);
    let expected = serde_json::to_string(&text).unwrap();
    let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let scope = RuntimeHandleScope::new();
    let input = scope.root_string_ptr(source);
    let before_ptr = input.get_raw_const_ptr::<crate::StringHeader>();
    force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();
    let before = gc_collection_count();
    let output = unsafe { crate::json::js_json_stringify_string(before_ptr) };
    assert!(gc_collection_count() > before);
    assert_ne!(input.get_raw_const_ptr::<crate::StringHeader>(), before_ptr);
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(
                crate::string::string_data(output),
                (*output).byte_len as usize,
            )
        },
        expected.as_bytes()
    );
    assert_eq!(
        unsafe { (*output).utf16_len as usize },
        expected.encode_utf16().count()
    );
}

fn assert_record_children_move(cold: bool, escaped: bool) {
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    crate::object::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let text: &[u8] = if escaped {
        br#"{"i\nd":42,"name":"user_\"42","email":"user_42\u0000@example.com","active":false,"score":63,"tags":["tag_\nalpha","tag_\\bravo"]}"#
    } else {
        br#"{"id":42,"name":"user_42","email":"user_42@example.com","active":false,"score":63,"tags":["tag_alpha","tag_bravo"]}"#
    };
    let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let value = unsafe { crate::json::test_json_parse_direct(source) };
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_u64(value.bits());
    let children = || {
        let obj =
            crate::JSValue::from_bits(input.get_nanbox_u64()).as_pointer::<crate::ObjectHeader>();
        let arr = crate::object::js_object_get_field(obj, 5).as_pointer::<crate::ArrayHeader>();
        let string = crate::array::js_array_get(arr, 0).as_string_ptr();
        (obj as usize, arr as usize, string as usize)
    };
    if cold {
        crate::json::CACHED_OBJECT_PROTO_BITS.with(|c| c.set(0));
        crate::json::OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(0));
    } else {
        unsafe {
            assert!(crate::json::to_json_definitely_absent(
                children().0 as *const u8
            ));
        }
    }
    let before_ptrs = children();
    force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = unsafe { crate::json::test_json_stringify_record_output(input.get_nanbox_u64()) }
        .expect("the direct record output path must execute");
    let output = scope.root_nanbox_u64(result.bits());
    assert!(
        gc_collection_count() > before,
        "the allocation must collect inside the emitter"
    );
    let after_ptrs = children();
    assert_ne!(after_ptrs.0, before_ptrs.0, "parent must move");
    assert_ne!(
        after_ptrs.1, before_ptrs.1,
        "array child must move through parent tracing"
    );
    assert_ne!(
        after_ptrs.2, before_ptrs.2,
        "string child must move through array tracing"
    );
    let header = crate::JSValue::from_bits(output.get_nanbox_u64()).as_string_ptr();
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(
                crate::string::string_data(header),
                (*header).byte_len as usize,
            )
        },
        text
    );
    crate::json::CACHED_OBJECT_PROTO_BITS.with(|c| c.set(0));
    crate::json::OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(0));
}
