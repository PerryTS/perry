//! Cache eviction must release ownership, including the backing key storage.
use super::*;

fn wide_source() -> String {
    format!(
        "{{{}}}",
        (0..50_000)
            .map(|i| format!("\"field_{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

unsafe fn parse(source: &str) -> crate::JSValue {
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    crate::json::js_json_parse_result(text).unwrap()
}

unsafe fn assert_output(value: crate::JSValue, source: &str) {
    let output = crate::json::js_json_stringify(f64::from_bits(value.bits()), 0);
    let bytes = std::slice::from_raw_parts(
        crate::string::string_data(output),
        (*output).byte_len as usize,
    );
    assert_eq!(bytes, source.as_bytes());
}

fn collect_full() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

#[test]
fn json_discarded_wide_keys_release_storage_after_cache_eviction() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = wide_source();
    collect_full();
    let baseline = crate::arena::arena_live_allocated_bytes();
    let permanent = crate::arena::longlived_in_use_bytes();
    for _ in 0..3 {
        unsafe {
            let value = parse(&source);
            assert_eq!(
                crate::object::object_live_slot_count(value.as_pointer()),
                50_000
            );
        }
        assert_eq!(
            crate::json::PARSE_KEY_CACHE.with(|cache| cache.borrow().len()),
            0,
            "wide-only keys belong to the object-local duplicate index"
        );
        assert_eq!(
            crate::arena::longlived_in_use_bytes(),
            permanent,
            "discardable keys and their ordered array must not enter permanent storage"
        );
        let allocated = crate::arena::arena_live_allocated_bytes();
        assert!(
            allocated > baseline + 2_000_000,
            "exercise a substantial graph"
        );
        collect_full();
        let remaining = crate::arena::arena_live_allocated_bytes();
        assert!(
            remaining <= baseline + 65_536,
            "wide-only keys must be collectible with their discarded object: baseline={baseline}, remaining={remaining}"
        );
    }
}

#[test]
fn json_cached_keys_ring_and_shape_follow_actual_movement() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = "{\"moving_property\":17,\"日本語\":42}";
    let scope = RuntimeHandleScope::new();
    unsafe {
        let value = parse(source);
        let root = scope.root_nanbox_u64(value.bits());
        let keys_before = crate::object::object_keys_array(value.as_pointer());
        let key_before = crate::json::cached_parse_key_ptr(b"moving_property");
        gc_collect_minor();
        let live = crate::JSValue::from_bits(root.get_nanbox_u64());
        let keys_after = crate::object::object_keys_array(live.as_pointer());
        let key_after = crate::json::cached_parse_key_ptr(b"moving_property");
        assert_ne!(
            keys_before, keys_after,
            "the cached key array must actually move"
        );
        assert_ne!(
            key_before, key_after,
            "the cached key string must actually move"
        );
        assert_eq!(
            crate::array::js_array_get(keys_after, 0).as_string_ptr(),
            key_after
        );
        assert_output(live, source);
        let again = parse(source);
        assert_eq!(
            crate::object::object_keys_array(again.as_pointer()),
            keys_after
        );
        assert_output(again, source);
    }
}

#[test]
fn json_retained_wide_object_keeps_evicted_keys_through_minor_and_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let source = wide_source();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let value = parse(&source);
        let root = scope.root_nanbox_u64(value.bits());
        let keys = crate::object::object_keys_array(value.as_pointer());
        assert!(crate::arena::pointer_in_old_gen(keys as usize));
        let last_before = crate::array::js_array_get(keys, 49_999).bits();
        gc_collect_minor();
        let live = crate::JSValue::from_bits(root.get_nanbox_u64());
        let keys = crate::object::object_keys_array(live.as_pointer());
        assert_ne!(last_before, crate::array::js_array_get(keys, 49_999).bits());
        assert_output(live, &source);
        collect_full();
        assert_output(crate::JSValue::from_bits(root.get_nanbox_u64()), &source);
    }
}

#[test]
fn json_typed_hint_reads_cached_keys_after_the_entry_collection() {
    for field_count in [1, 0, 2] {
        let _pressure = crate::gc::policy::force_tiny_parse_pressure_due_for_test();
        let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuation = ForcedEvacuationTestGuard::on();
        let _protection =
            crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::ProtectPages);
        register_runtime_handle_root_scanner_for_tests();
        gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
        let scope = RuntimeHandleScope::new();
        let source = "[{\"moving_property\":17}]";
        let input = scope.root_string_ptr(crate::js_string_from_bytes(
            source.as_ptr(),
            source.len() as u32,
        ));
        let key_before = crate::json::cached_parse_key_ptr(b"moving_property");
        crate::gc::policy::GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(true));
        let before = gc_collection_count();
        unsafe {
            let value = input.with_const_ptr(|input| {
                crate::json::js_json_parse_typed_array(
                    input,
                    b"moving_property\0".as_ptr(),
                    16,
                    field_count,
                )
            });
            assert!(gc_collection_count() > before);
            assert_ne!(
                key_before,
                crate::json::cached_parse_key_ptr(b"moving_property")
            );
            assert_output(value, source);
        }
    }
}
