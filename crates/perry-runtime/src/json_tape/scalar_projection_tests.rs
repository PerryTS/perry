use super::*;

unsafe fn array(text: &[u8]) -> *mut LazyArrayHeader {
    let blob = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    with_built_tape(text, |tape| {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), blob)
    })
    .expect("valid array must produce a lazy header")
}

unsafe fn read(hdr: *mut LazyArrayHeader, index: f64, name: &[u8]) -> u64 {
    js_json_lazy_index_scalar(
        f64::from_bits(JSValue::object_ptr(hdr.cast()).bits()),
        index,
        name.as_ptr(),
        name.len(),
    )
    .to_bits()
}

#[test]
fn json_scalar_projection_scans_without_construction_or_reparse() {
    unsafe {
        let text = format!(
            "[{}]",
            (0..200)
                .map(|i| { format!(r#"{{"id":{i},"name":"record","nested":[{{"id":999}}]}}"#) })
                .collect::<Vec<_>>()
                .join(",")
        );
        let hdr = array(text.as_bytes());
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(hdr);
        let allocated = crate::arena::arena_live_allocated_bytes();
        let reparses = reparse_materializations();
        for i in 0..200 {
            assert_eq!(
                root.with_mut_ptr(|hdr| read(hdr, i as f64, b"id")),
                (i as f64).to_bits()
            );
        }
        assert_eq!(crate::arena::arena_live_allocated_bytes(), allocated);
        assert_eq!(reparse_materializations(), reparses);
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            assert!((*hdr).materialized.is_null());
            assert_eq!((*hdr).sequential_streak, 0);
            assert_eq!((*hdr).cumulative_walk_steps, 0);
            for word in 0..4 {
                assert_eq!(*(*hdr).materialized_bitmap.add(word), 0);
            }
            // Misses preserve the cursor; short backward/forward walks work.
            for i in [199, 150] {
                assert_eq!(read(hdr, i as f64, b"id"), crate::value::TAG_HOLE);
                assert_eq!((*hdr).walk_idx, 199);
            }
            for i in [0, 5, 6] {
                assert_eq!(read(hdr, i as f64, b"id"), (i as f64).to_bits());
            }
            assert_eq!((*hdr).walk_idx, 6);
        });
        root.with_mut_ptr(|hdr| lazy_get(hdr, 7));
        root.with_const_ptr(|hdr: *const LazyArrayHeader| {
            assert_eq!((*hdr).sequential_streak, 1);
        });
    }
}

#[test]
fn json_scalar_projection_uses_last_own_scalar_and_exact_number_semantics() {
    for (record, key, expected) in [
        (r#"{"id":1,"id":2}"#, "id", 2.0f64.to_bits()),
        (r#"{"id":{"x":0},"id":3}"#, "id", 3.0f64.to_bits()),
        (r#"{"":true,"x":[false]}"#, "", JSValue::bool(true).bits()),
        (
            r#"{"id":false,"nested":{"id":true}}"#,
            "id",
            JSValue::bool(false).bits(),
        ),
        (r#"{"id":null}"#, "id", JSValue::null().bits()),
        (r#"{"id":-0}"#, "id", (-0.0f64).to_bits()),
        (
            r#"{"id":9007199254740993}"#,
            "id",
            9007199254740992.0f64.to_bits(),
        ),
        (r#"{"id":1e400}"#, "id", f64::INFINITY.to_bits()),
        (r#"{"id":-1e-400}"#, "id", (-0.0f64).to_bits()),
        (
            r#"{"id":0.01234567890123456789}"#,
            "id",
            0.012345678901234568f64.to_bits(),
        ),
        (
            r#"{"id":8,"x\u0069d":7,"identity":6}"#,
            "id",
            8.0f64.to_bits(),
        ),
    ] {
        unsafe {
            let hdr = array(format!("[{record}]").as_bytes());
            assert_eq!(read(hdr, 0.0, key.as_bytes()), expected, "{record}");
        }
    }
}

#[test]
fn json_scalar_projection_declines_ambiguous_keys_and_observable_records() {
    for record in [
        r#"{"id":1,"\u0069d":2}"#,
        r#"{"\u0069d":2,"id":3}"#,
        r#"{"id":"text"}"#,
        r#"{"id":1,"id":{"x":2}}"#,
        r#"{"id":[1,2]}"#,
        r#"{"identity":2}"#,
        "null",
    ] {
        unsafe {
            let hdr = array(format!("[{record}]").as_bytes());
            assert_eq!(read(hdr, 0.0, b"id"), crate::value::TAG_HOLE, "{record}");
            assert_eq!(
                (*hdr).walk_idx,
                u32::MAX,
                "a miss must not change the cursor"
            );
        }
    }
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(br#"[{"id":1},{"id":2}]"#));
        for index in [-1.0, 0.5, f64::NAN, f64::INFINITY, 2.0, u32::MAX as f64] {
            assert_eq!(
                root.with_mut_ptr(|hdr| read(hdr, index, b"id")),
                crate::value::TAG_HOLE
            );
        }
        assert_eq!(
            root.with_mut_ptr(|hdr| read(hdr, -0.0, b"id")),
            1.0f64.to_bits()
        );
        let record = root.with_mut_ptr(|hdr| lazy_get(hdr, 0));
        let record = scope.root_nanbox_u64(record.bits());
        let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
        crate::object::js_object_set_field_by_name(
            JSValue::from_bits(record.get_nanbox_u64())
                .as_pointer::<crate::object::ObjectHeader>()
                .cast_mut(),
            key,
            99.0,
        );
        assert_eq!(
            root.with_mut_ptr(|hdr| read(hdr, 0.0, b"id")),
            crate::value::TAG_HOLE
        );
        assert_eq!(
            root.with_mut_ptr(|hdr| read(hdr, 1.0, b"id")),
            2.0f64.to_bits()
        );
        root.with_mut_ptr(|hdr| force_materialize_lazy(hdr));
        assert_eq!(
            root.with_mut_ptr(|hdr| read(hdr, 1.0, b"id")),
            crate::value::TAG_HOLE
        );
    }
}

#[test]
fn json_scalar_projection_key_probe_agrees_with_plain_ascii_equality() {
    for left in ["", "i", "id", "id2", "identity", "name", "ID", "0123456789"] {
        for right in ["", "i", "id", "id2", "identity", "name", "ID", "0123456789"] {
            assert_eq!(
                key_matches(format!("\"{left}\":0").as_bytes(), right.as_bytes()),
                Some(left == right)
            );
        }
    }
    assert_eq!(key_matches(br#""\u0069d":0"#, b"id"), None);
    assert_eq!(key_matches(br#""i\u0064":0"#, b"id"), None);
    assert_eq!(key_matches(br#""x\u0064":0"#, b"id"), Some(false));
}

#[test]
fn json_scalar_projection_definitions_and_deletions_materialize_before_mutation() {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_raw_mut_ptr(array(br#"[{"id":1},{"id":2}]"#));
        let descriptor_text = br#"{"value":{"id":9}}"#;
        let text = crate::string::js_string_from_bytes(
            descriptor_text.as_ptr(),
            descriptor_text.len() as u32,
        );
        let descriptor = crate::json::js_json_parse(text);
        let descriptor = scope.root_nanbox_u64(descriptor.bits());
        let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
        let key = scope.root_string_ptr(key);
        let returned = receiver.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            key.with_mut_ptr(|key| {
                crate::object::js_object_define_property(
                    f64::from_bits(JSValue::object_ptr(hdr.cast()).bits()),
                    f64::from_bits(JSValue::string_ptr(key).bits()),
                    f64::from_bits(descriptor.get_nanbox_u64()),
                )
            })
        });
        assert_eq!(
            returned.to_bits(),
            receiver.with_mut_ptr(|hdr: *mut u8| JSValue::object_ptr(hdr).bits())
        );
        receiver.with_const_ptr(|hdr: *const LazyArrayHeader| {
            assert!(!(*hdr).materialized.is_null());
        });
        assert_eq!(
            receiver.with_mut_ptr(|hdr| read(hdr, 0.0, b"id")),
            crate::value::TAG_HOLE
        );
        let current = receiver.with_mut_ptr(|hdr| lazy_get(hdr, 0));
        let current = scope.root_nanbox_u64(current.bits());
        let id = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
        assert_eq!(
            crate::object::js_object_get_field_by_name(
                JSValue::from_bits(current.get_nanbox_u64()).as_pointer(),
                id,
            )
            .bits(),
            9.0f64.to_bits()
        );
        assert_eq!(
            receiver
                .with_mut_ptr(|hdr| key
                    .with_const_ptr(|key| { crate::object::js_object_delete_field(hdr, key) })),
            1
        );
        assert!(receiver.with_mut_ptr(|hdr| lazy_get(hdr, 0)).is_undefined());
    }
}

#[test]
fn json_scalar_projection_bounds_walks_and_caches_repeated_reads() {
    unsafe {
        let text = format!(
            "[{}]",
            (0..200)
                .map(|i| format!(r#"{{"id":{i}}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(text.as_bytes()));
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            assert_eq!(read(hdr, 33.0, b"id"), crate::value::TAG_HOLE);
            assert_eq!((*hdr).walk_idx, u32::MAX);
            assert_eq!(read(hdr, 32.0, b"id"), 32.0f64.to_bits());
            assert_eq!(read(hdr, 65.0, b"id"), crate::value::TAG_HOLE);
            assert_eq!((*hdr).walk_idx, 32);
            assert_eq!(read(hdr, 32.0, b"id"), crate::value::TAG_HOLE);
        });
        root.with_mut_ptr(|hdr| lazy_get(hdr, 32));
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            assert_ne!(*(*hdr).materialized_bitmap & (1u64 << 32), 0);
            assert_eq!(read(hdr, 31.0, b"id"), 31.0f64.to_bits());
            assert_eq!(read(hdr, 32.0, b"id"), crate::value::TAG_HOLE);
        });
        // A duplicate after the field budget cannot leave a partial hit.
        let fields = (0..31)
            .map(|i| format!(r#""x{i}":0"#))
            .collect::<Vec<_>>()
            .join(",");
        let hdr = array(format!(r#"[{{"id":1,{fields},"id":2}}]"#).as_bytes());
        assert_eq!(read(hdr, 0.0, b"id"), crate::value::TAG_HOLE);
        assert_eq!((*hdr).walk_idx, u32::MAX);
    }
}

#[test]
fn json_scalar_projection_mixed_reads_preserve_batch_construction() {
    unsafe {
        let text = format!(
            "[{}]",
            (0..1000)
                .map(|i| format!(r#"{{"id":{i},"name":"record"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        for project_first in [false, true] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let root = scope.root_raw_mut_ptr(array(text.as_bytes()));
            let reparses = reparse_materializations();
            let mut hits = 0;
            for i in 0..1000 {
                if project_first {
                    let result = root.with_mut_ptr(|hdr| read(hdr, i as f64, b"id"));
                    if result != crate::value::TAG_HOLE {
                        assert_eq!(result, (i as f64).to_bits());
                        hits += 1;
                    }
                }
                // The subsequent name/child read exposes the actual record.
                root.with_mut_ptr(|hdr| lazy_get(hdr, i));
            }
            assert_eq!(reparse_materializations() - reparses, 1);
            root.with_const_ptr(|hdr: *const LazyArrayHeader| {
                assert!(!(*hdr).materialized.is_null());
            });
            if project_first {
                assert!(hits > 0 && hits < 1000);
            }
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(text.as_bytes()));
        root.with_mut_ptr(|hdr| lazy_get(hdr, 0));
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            assert_eq!((*hdr).sequential_streak, 1);
            assert_eq!(read(hdr, 1.0, b"id"), 1.0f64.to_bits());
            assert_eq!((*hdr).sequential_streak, 1);
            assert_eq!(read(hdr, 2.0, b"id"), 2.0f64.to_bits());
            assert_eq!((*hdr).sequential_streak, 0);
        });
        root.with_mut_ptr(|hdr| lazy_get(hdr, 3));
        root.with_const_ptr(|hdr: *const LazyArrayHeader| {
            assert_eq!(
                (*hdr).sequential_streak,
                1,
                "unexposed records break a cold run"
            );
        });
    }
}
