use super::*;

unsafe fn with_array(text: &str, f: impl FnOnce(*const crate::ArrayHeader)) {
    let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let value = crate::json::test_json_parse_direct(source);
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(value.as_pointer::<crate::ArrayHeader>());
    let first = crate::array::js_array_get(input.get_raw_const_ptr(), 0);
    let first = scope.root_nanbox_u64(first.bits());
    invalidate_object_proto_tojson_state();
    assert!(
        super::super::stringify_tojson_probe::to_json_definitely_absent(
            (first.get_nanbox_u64() & POINTER_MASK) as *const u8
        )
    );
    f(input.get_raw_const_ptr());
}

fn collections() -> u64 {
    let mut value = 0;
    crate::gc::js_gc_stats(&mut value, std::ptr::null_mut(), std::ptr::null_mut());
    value
}

#[test]
fn json_nested_records_reuses_multiple_shapes_without_managed_intermediates() {
    unsafe {
        let text = format!("[{}]", (0..160).map(|i| {
            format!("{{\"id\":{i},\"field_{}\":{},\"value\":\"東京🙂\\n\",\"nested\":{{\"yes\":true,\"escaped\\\"\":null}}}}", i % 32, i * 2)
        }).collect::<Vec<_>>().join(","));
        with_array(&text, |arr| {
            let bytes = crate::arena::arena_total_bytes();
            let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
            let count = collections();
            let stack = STRINGIFY_STACK.with(|s| s.borrow().len());
            set_to_json_key_str("unchanged key");
            let mut output = String::with_capacity(text.len());
            for _ in 0..20 {
                output.clear();
                assert!(try_emit(arr, &mut output, 0));
                assert_eq!(output, text);
            }
            assert_eq!(crate::arena::arena_total_bytes(), bytes);
            assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
            assert_eq!(collections(), count);
            assert_eq!(STRINGIFY_STACK.with(|s| s.borrow().len()), stack);
            assert_eq!(TO_JSON_KEY.with(|s| s.borrow().clone()), "unchanged key");
        });
    }
}

#[test]
fn json_nested_records_declines_unsupported_late_values_and_restores_prefix() {
    unsafe {
        for last in [
            r#"{"id":1,"nested":{"deeper":{"yes":true}}}"#,
            r#"{"id":1,"nested":[true]}"#,
            r#"{"id":1,"nested":{"2":true,"1":false}}"#,
            "null",
        ] {
            let text = format!(r#"[{{"id":0,"nested":{{"yes":true}}}},{last}]"#);
            with_array(&text, |arr| {
                let before = crate::arena::arena_total_bytes();
                let mut output = String::from("already emitted:");
                assert!(!try_emit(arr, &mut output, 0), "{text}");
                assert_eq!(output, "already emitted:");
                assert_eq!(crate::arena::arena_total_bytes(), before);
            });
        }
    }
}

#[test]
fn json_nested_records_observes_mutation_between_attempts() {
    unsafe {
        with_array(
            r#"[{"id":0,"nested":{"yes":true}},{"id":1,"nested":{"yes":true}}]"#,
            |arr| {
                let mut output = String::new();
                assert!(try_emit(arr, &mut output, 0));
                let second = crate::array::js_array_get(arr, 1).as_pointer::<crate::ObjectHeader>();
                crate::object::js_object_set_field(
                    second.cast_mut(),
                    0,
                    JSValue::from_bits(TAG_UNDEFINED),
                );
                output.clear();
                assert!(!try_emit(arr, &mut output, 0));
                assert!(output.is_empty());
                crate::object::js_object_set_field(second.cast_mut(), 0, JSValue::number(19.0));
                assert!(try_emit(arr, &mut output, 0));
                assert!(output.contains("\"id\":19"));
            },
        );
    }
}

#[test]
fn json_nested_records_declines_cold_prototype_lookup_without_collection() {
    unsafe {
        with_array(r#"[{"nested":{"x":1}},{"nested":{"x":2}}]"#, |arr| {
            let cached = CACHED_OBJECT_PROTO_BITS.with(|c| c.replace(0));
            let state = OBJECT_PROTO_TOJSON_STATE
                .with(|c| c.replace(super::super::stringify_tojson_probe::PROTO_TOJSON_DIRTY));
            let bytes = crate::arena::arena_total_bytes();
            let count = collections();
            let mut output = String::from("prefix");
            let result = try_emit(arr, &mut output, 0);
            CACHED_OBJECT_PROTO_BITS.with(|c| c.set(cached));
            OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(state));
            assert!(!result);
            assert_eq!(output, "prefix");
            assert_eq!(crate::arena::arena_total_bytes(), bytes);
            assert_eq!(collections(), count);
        });
    }
}

#[test]
fn json_nested_records_caps_native_shape_and_prefix_storage() {
    unsafe {
        let text = format!(
            "[{}]",
            (0..MAX_SHAPES + 1)
                .map(|i| format!("{{\"field_{i}\":1,\"nested\":{{\"yes\":true}}}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        with_array(&text, |arr| {
            let mut output = String::from("prefix");
            assert!(!try_emit(arr, &mut output, 0));
            assert_eq!(output, "prefix");
        });
        let huge_key = "x".repeat(MAX_PREFIX_BYTES);
        let text = format!("[{{\"nested\":{{\"yes\":true}}}},{{\"{huge_key}\":1}}]");
        with_array(&text, |arr| {
            let mut output = String::from("prefix");
            assert!(!try_emit(arr, &mut output, 0));
            assert_eq!(output, "prefix");
        });
    }
}

#[test]
fn json_nested_records_preserves_empty_leaves_and_depth_fallback() {
    unsafe {
        let text = r#"[{"a":{}},{"b":{"s":"a","n":0,"b":false,"z":null}}]"#;
        with_array(text, |arr| {
            let mut output = String::new();
            assert!(try_emit(arr, &mut output, 0));
            assert_eq!(output, text);
            output.clear();
            assert!(!try_emit(
                arr,
                &mut output,
                super::super::stringify::MAX_STRINGIFY_NESTING_DEPTH as u32 - 2
            ));
            assert!(output.is_empty());
        });
    }
}

#[test]
fn json_nested_records_shape_table_probes_collisions_and_reuses_full_capacity() {
    unsafe {
        // 63 parent shapes plus one shared leaf reach the exact 64-plan cap.
        // Repeating them tests occupied-bucket lookup after the table is full.
        let text = format!(
            "[{}]",
            (0..(MAX_SHAPES - 1) * 3)
                .map(|i| format!(
                    "{{\"field_{}\":{i},\"nested\":{{\"yes\":true}}}}",
                    i % (MAX_SHAPES - 1)
                ))
                .collect::<Vec<_>>()
                .join(",")
        );
        with_array(&text, |arr| {
            let mut emitter = Emitter::new();
            let mut output = String::new();
            for i in 0..(*arr).length {
                let value = crate::array::js_array_get(arr, i).bits();
                assert!(emitter.emit_record(value, true, &mut output).is_some());
            }
            assert_eq!(emitter.count, MAX_SHAPES);
            output.clear();
            assert!(try_emit(arr, &mut output, 0));
            assert_eq!(output, text);
        });
        // Select a collision from real distinct shapes. Consecutive IDs can
        // distribute perfectly for 64 shapes, so use 129 for a guaranteed
        // collision in the 128 buckets; only the selected pair is emitted.
        let rows: Vec<_> = (0..PLAN_BUCKETS + 1)
            .map(|i| format!("{{\"field_{i}\":{i},\"nested\":{{\"yes\":true}}}}"))
            .collect();
        with_array(&format!("[{}]", rows.join(",")), |arr| {
            let mut homes = [None; PLAN_BUCKETS];
            let mut pair = None;
            for i in 0..(*arr).length {
                let bits = crate::array::js_array_get(arr, i).bits();
                let object = (bits & POINTER_MASK) as *const crate::ObjectHeader;
                let shape = crate::object::shapes::object_shape_stamp(object);
                let bucket = (shape.wrapping_mul(0x9e37_79b9) >> 25) as usize;
                if let Some(previous) = homes[bucket] {
                    pair = Some([previous, (bits, i as usize)]);
                    break;
                }
                homes[bucket] = Some((bits, i as usize));
            }
            let pair = pair.expect("129 distinct shapes must share a home bucket");
            let mut emitter = Emitter::new();
            let mut output = String::new();
            for _ in 0..20 {
                for (bits, i) in pair {
                    output.clear();
                    assert!(emitter.emit_record(bits, true, &mut output).is_some());
                    assert_eq!(output, rows[i]);
                }
            }
            assert_eq!(emitter.count, 3);
        });
    }
}

#[test]
fn json_nested_records_cached_shape_still_checks_each_instance() {
    unsafe {
        with_array(r#"[{"nested":{"x":1}},{"nested":{"x":2}}]"#, |arr| {
            let first = crate::array::js_array_get(arr, 0).bits();
            let second = crate::array::js_array_get(arr, 1).bits();
            let object = (second & POINTER_MASK) as *mut crate::ObjectHeader;
            let header = crate::value::addr_class::try_read_tracked_gc_header(object as usize)
                .unwrap()
                .as_ptr();
            let mut emitter = Emitter::new();
            let mut output = String::new();
            assert!(emitter.emit_record(first, true, &mut output).is_some());
            let count = emitter.count;
            assert!(emitter.emit_record(second, true, &mut output).is_some());
            assert_eq!(
                emitter.count, count,
                "second instance must hit the cached shape"
            );

            // Deliberately narrow the advertised allocation before the cache
            // hit. Restore it before asserting or performing any collection.
            let size = (*header).size;
            (*header).size =
                (crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>()) as u32;
            let bounded = emitter.emit_record(second, true, &mut output);
            (*header).size = size;
            assert!(bounded.is_none());

            let flags = (*header)._reserved;
            (*header)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
            let described = emitter.emit_record(second, true, &mut output);
            (*header)._reserved = flags;
            assert!(described.is_none());

            (*header)._reserved &= !crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
            let exotic = emitter.emit_record(second, true, &mut output);
            (*header)._reserved = flags;
            assert!(exotic.is_none());

            let class = (*object).class_id;
            (*object).class_id = 1;
            let class_instance = emitter.emit_record(second, true, &mut output);
            (*object).class_id = class;
            assert!(class_instance.is_none());

            output.clear();
            assert!(try_emit(arr, &mut output, 0));
            assert_eq!(output, r#"[{"nested":{"x":1}},{"nested":{"x":2}}]"#);
        });
    }
}
