use super::*;

unsafe fn owned_small(input: &[u8]) -> *mut crate::json_tape::LazyArrayHeader {
    let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let mut entries = crate::json_tape::build_tape(input).unwrap().entries;
    // Exercise the ownership branch with a movable, nursery-sized blob.
    entries.reserve(100_000);
    let length = crate::json_tape::count_array_length(&entries, 0);
    let header = crate::json_tape::alloc_lazy_array_from_scratch(&mut entries, 0, length, text);
    assert_eq!(entries.capacity(), 0, "the source must relinquish storage");
    header
}

#[test]
fn json_lazy_growth_resolves_each_cached_array_consumer() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for width in [0, 1, 2, 8, 17] {
        for consumer in 0..4 {
            let source = format!("[{}]", (0..width).map(|n| n.to_string()).collect::<Vec<_>>().join(","));
            unsafe {
                let lazy = owned_small(source.as_bytes());
                let original = crate::json_tape::force_materialize_lazy(lazy);
                let mut grown = original;
                for index in width..70 {
                    grown = crate::array::js_array_push(grown, crate::JSValue::number(index as f64));
                }
                assert_ne!(grown, original, "exercise a real growth forwarding chain");
                assert_eq!((*lazy).materialized, original, "keep the stale owner edge until the consumer reads it");
                match consumer {
                    0 => assert_eq!(crate::array::js_array_length(lazy.cast()), 70),
                    1 => assert_eq!(crate::json_tape::lazy_get(lazy, 69).as_number(), 69.0),
                    2 => assert_eq!(crate::json_tape::force_materialize_lazy(lazy), grown),
                    _ => {
                        let output = crate::json::js_json_stringify(f64::from_bits(ptr_bits(lazy as usize)), 0);
                        let bytes = std::slice::from_raw_parts(crate::string::string_data(output), (*output).byte_len as usize);
                        assert_eq!(serde_json::from_slice::<Vec<u32>>(bytes).unwrap(), (0..70).collect::<Vec<_>>());
                    }
                }
                assert_eq!((*lazy).materialized, grown);
                assert_eq!((*lazy).cached_length, 70);
            }
        }
    }
}

#[test]
fn json_lazy_growth_preserves_holes_and_repeated_mutation_through_owner() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let lazy = owned_small(b"[0,1]");
        for index in 2..70 {
            // Always pass the original wrapper, ignoring the returned head.
            crate::array::js_array_push(lazy.cast(), crate::JSValue::number(index as f64));
            assert_eq!(crate::json_tape::lazy_get(lazy, index).as_number(), index as f64);
        }
        let length = crate::js_string_from_bytes(b"length".as_ptr(), 6);
        let receiver = f64::from_bits(ptr_bits(lazy as usize));
        let key = f64::from_bits(string_bits(length as usize));
        crate::value::js_dyn_index_set_strict(receiver, key, 2.0, 1);
        crate::value::js_dyn_index_set_strict(receiver, key, 6.0, 1);
        assert_eq!(crate::array::js_array_length(lazy.cast()), 6);
        for index in 2..6 {
            assert_eq!(crate::json_tape::lazy_get(lazy, index).bits(), crate::JSValue::undefined().bits());
        }
        crate::value::js_dyn_index_set_strict(receiver, 5.0, 42.0, 1);
        assert_eq!(crate::json_tape::lazy_get(lazy, 5).as_number(), 42.0);
        let output = crate::json::js_json_stringify(receiver, 0);
        let bytes = std::slice::from_raw_parts(crate::string::string_data(output), (*output).byte_len as usize);
        assert_eq!(bytes, b"[0,1,null,null,null,42]");
    }
}

#[test]
fn json_lazy_growth_owner_alone_retains_forwarded_array_and_children_across_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection = crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let owner = scope.root_raw_mut_ptr(owned_small(br#"["heap child survives growth"]"#));
        let lazy = owner.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>();
        let original = crate::json_tape::force_materialize_lazy(lazy);
        let mut grown = original;
        for index in 1..70 {
            grown = crate::array::js_array_push(grown, crate::JSValue::number(index as f64));
        }
        assert_ne!(original, grown);
        let old_child = crate::array::js_array_get(grown, 0).bits();
        assert_eq!((*lazy).materialized, original);
        let before = gc_collection_count();
        let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        assert!(gc_collection_count() > before);
        let lazy = owner.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>();
        let moved = crate::json_tape::force_materialize_lazy(lazy);
        assert_ne!(moved, grown, "the live array must actually move");
        let child = crate::json_tape::lazy_get(lazy, 0);
        assert_ne!(child.bits(), old_child, "the child must actually move");
        let text = child.as_string_ptr();
        assert_eq!(std::slice::from_raw_parts(crate::string::string_data(text), (*text).byte_len as usize), b"heap child survives growth");
        assert_eq!(crate::array::js_array_length(lazy.cast()), 70);
        assert_eq!(crate::json_tape::lazy_get(lazy, 69).as_number(), 69.0);
    }
}

#[test]
fn json_owned_tape_transfers_exact_storage_and_releases_after_materialization() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let source = format!("[1,{}7]", "0,".repeat(99_998));
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    let tape = crate::json_tape::build_tape(source.as_bytes()).unwrap();
    let mut entries = tape.entries.into_boxed_slice().into_vec();
    let original = entries.as_ptr();
    let bytes = entries.len() * std::mem::size_of::<crate::json_tape::TapeEntry>();
    assert!(bytes > 1024 * 1024);
    assert_eq!(entries.len(), entries.capacity());
    let before = crate::json_tape_store::registered_bytes();
    let length = crate::json_tape::count_array_length(&entries, 0);
    let lazy =
        unsafe { crate::json_tape::alloc_lazy_array_from_scratch(&mut entries, 0, length, text) };
    assert_eq!(entries.capacity(), 0);
    assert_eq!(unsafe { (*lazy).tape.cast_const() }, original);
    assert_eq!(crate::json_tape_store::registered_bytes(), before + bytes);
    assert!(crate::arena::pointer_in_old_gen(lazy as usize));
    let array = unsafe { crate::json_tape::force_materialize_lazy(lazy) };
    assert_eq!(unsafe { (*array).length }, 100_000);
    assert_eq!(
        crate::array::js_array_get(array, 0).bits(),
        crate::JSValue::number(1.0).bits()
    );
    assert_eq!(
        crate::array::js_array_get(array, 99_999).bits(),
        crate::JSValue::number(7.0).bits()
    );
    assert!(unsafe { (*lazy).tape.is_null() });
    assert_eq!(crate::json_tape_store::registered_bytes(), before);
    unsafe { crate::json_tape::release_tape_after_materialize(lazy) };
    assert_eq!(crate::json_tape_store::registered_bytes(), before);
}

#[test]
fn json_small_tape_keeps_reusable_storage_and_copies_its_result() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let source = b"[1,2,3]";
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    let mut entries = crate::json_tape::build_tape(source).unwrap().entries;
    let original = entries.as_ptr();
    let capacity = entries.capacity();
    let before = crate::json_tape_store::registered_bytes();
    let lazy = unsafe { crate::json_tape::alloc_lazy_array_from_scratch(&mut entries, 0, 3, text) };
    assert_eq!(entries.capacity(), capacity);
    assert_eq!(entries.as_ptr(), original);
    assert_ne!(unsafe { (*lazy).tape.cast_const() }, original);
    let array = unsafe { crate::json_tape::force_materialize_lazy(lazy) };
    assert_eq!(unsafe { (*array).length }, 3);
    assert_eq!(crate::json_tape_store::registered_bytes(), before);
}

#[test]
fn json_owned_tape_roots_its_blob_through_copied_minor_during_construction() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    let source = br#"["owned-alpha-long","owned-beta-long"]"#;
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    let mut entries = crate::json_tape::build_tape(source).unwrap().entries;
    entries.reserve(100_000);
    let bytes = entries.len() * std::mem::size_of::<crate::json_tape::TapeEntry>();
    let before_bytes = crate::json_tape_store::registered_bytes();
    let before_gc = gc_collection_count();
    let hook =
        JsonTapeSafepointHookGuard::new(crate::json_tape::JsonTapeSafepoint::LazyArrayRooted);
    let lazy = unsafe { crate::json_tape::alloc_lazy_array_from_scratch(&mut entries, 0, 2, text) };
    assert_eq!(entries.capacity(), 0);
    assert_eq!(hook.fired_ptr(), lazy as usize);
    assert!(gc_collection_count() > before_gc);
    assert_ne!(
        unsafe { (*lazy).blob_str },
        text,
        "the blob must actually move"
    );
    assert_eq!(
        unsafe { crate::json_tape::LazyArrayHeader::blob_bytes(lazy) },
        source
    );
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        before_bytes + bytes
    );
    let array = unsafe { crate::json_tape::force_materialize_lazy(lazy) };
    assert_eq!(unsafe { (*array).length }, 2);
    assert_eq!(crate::json_tape_store::registered_bytes(), before_bytes);
}

#[test]
fn json_owned_tapes_remain_independent_and_release_only_dead_owners() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let before = crate::json_tape_store::registered_bytes();
    let first = unsafe { owned_small(b"[1,2,3]") };
    js_shadow_slot_set(0, ptr_bits(first as usize));
    let first_bytes = crate::json_tape_store::registered_bytes() - before;
    let second = unsafe { owned_small(b"[4,5,6,7]") };
    js_shadow_slot_set(1, ptr_bits(second as usize));
    let both = crate::json_tape_store::registered_bytes();
    assert_ne!(unsafe { (*first).tape }, unsafe { (*second).tape });
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(crate::json_tape_store::registered_bytes(), both);
    assert_eq!(
        unsafe { crate::json_tape::LazyArrayHeader::blob_bytes(first) },
        b"[1,2,3]"
    );
    assert_eq!(
        unsafe { crate::json_tape::LazyArrayHeader::blob_bytes(second) },
        b"[4,5,6,7]"
    );
    js_shadow_slot_set(0, crate::JSValue::undefined().bits());
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        both - first_bytes
    );
    let array = unsafe { crate::json_tape::force_materialize_lazy(second) };
    assert_eq!(unsafe { (*array).length }, 4);
    assert_eq!(crate::json_tape_store::registered_bytes(), before);
}

#[test]
fn json_owned_tapes_release_through_thread_teardown_once() {
    std::thread::spawn(|| {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        assert_eq!(crate::json_tape_store::registered_bytes(), 0);
        let _first = unsafe { owned_small(b"[1,2]") };
        let _second = unsafe { owned_small(b"[3,4,5]") };
        assert!(crate::json_tape_store::registered_bytes() > 0);
        crate::gc::js_gc_release_current_thread_collection_side_allocations();
        assert_eq!(crate::json_tape_store::registered_bytes(), 0);
        crate::gc::js_gc_release_current_thread_collection_side_allocations();
        assert_eq!(crate::json_tape_store::registered_bytes(), 0);
    })
    .join()
    .unwrap();
}

#[test]
fn json_lazy_numeric_assignment_materializes_and_stores() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for strict in [0, 1] {
        let lazy = unsafe { owned_small(b"[1,2,3]") };
        let result = crate::value::js_dyn_index_set_strict(
            f64::from_bits(ptr_bits(lazy as usize)),
            1.0,
            42.0,
            strict,
        );
        assert_eq!(result, 42.0);
        let array = unsafe { (*lazy).materialized };
        assert!(!array.is_null());
        assert_eq!(
            crate::array::js_array_get(array, 1).bits(),
            crate::JSValue::number(42.0).bits()
        );
        assert!(unsafe { (*lazy).tape.is_null() });
    }
}

#[test]
fn json_lazy_assignment_updates_length_through_the_original_alias() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let lazy = unsafe { owned_small(b"[1,2,3]") };
    let receiver = f64::from_bits(ptr_bits(lazy as usize));
    crate::value::js_dyn_index_set_strict(receiver, 5.0, 42.0, 1);
    assert_eq!(crate::array::js_array_length(lazy.cast()), 6);
    assert_eq!(unsafe { (*lazy).cached_length }, 6);
    let length = crate::js_string_from_bytes(b"length".as_ptr(), 6);
    crate::value::js_dyn_index_set_strict(
        receiver,
        f64::from_bits(string_bits(length as usize)),
        2.0,
        1,
    );
    assert_eq!(crate::array::js_array_length(lazy.cast()), 2);
    assert_eq!(unsafe { (*(*lazy).materialized).length }, 2);
}

#[test]
fn json_lazy_assignment_roots_heap_key_and_value_during_materialization() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    let lazy = unsafe { owned_small(b"[1,2,3]") };
    let key = crate::js_string_from_bytes(b"1".as_ptr(), 1);
    let bytes = b"owned mutation value survives movement";
    let stored = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let before = gc_collection_count();
    let hook =
        JsonTapeSafepointHookGuard::new(crate::json_tape::JsonTapeSafepoint::ForceLazyArrayRooted);
    let result = crate::value::js_dyn_index_set_strict(
        f64::from_bits(ptr_bits(lazy as usize)),
        f64::from_bits(string_bits(key as usize)),
        f64::from_bits(string_bits(stored as usize)),
        1,
    );
    hook.fired_ptr();
    assert!(gc_collection_count() > before);
    assert_ne!(
        result.to_bits(),
        string_bits(stored as usize),
        "stored string must move"
    );
    let array = unsafe { (*lazy).materialized };
    assert!(!array.is_null());
    let actual = crate::array::js_array_get(array, 1);
    assert_eq!(actual.bits(), result.to_bits());
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(
                crate::string::string_data(actual.as_string_ptr()),
                bytes.len(),
            )
        },
        bytes
    );
}
