use super::*;

#[test]
fn json_scalar_memo_reuses_slots_without_adding_gc_edges() {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(br#"[{"id":0},{"id":-0},{"id":null}]"#));
        let allocated = crate::arena::arena_live_allocated_bytes();
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            let slots_before = crate::gc::test_gc_rewrite_slot_addresses(hdr as usize).unwrap();
            for _ in 0..3 {
                for (i, bits) in [
                    0.0f64.to_bits(),
                    (-0.0f64).to_bits(),
                    JSValue::null().bits(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(read(hdr, i as f64, b"id"), bits);
                    let slot = (*hdr).materialized_elements.add(i);
                    assert_ne!((*slot).bits(), 0, "even +0 must be memoized");
                    assert!(!slots_before.contains(&(slot as usize)));
                }
            }
            assert_eq!(*(*hdr).materialized_bitmap, 0);
            assert_eq!(
                crate::gc::test_gc_rewrite_slot_addresses(hdr as usize).unwrap(),
                slots_before
            );
        });
        assert_eq!(crate::arena::arena_live_allocated_bytes(), allocated);
        root.with_mut_ptr(|hdr| lazy_get(hdr, 1));
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            let slots = crate::gc::test_gc_rewrite_slot_addresses(hdr as usize).unwrap();
            assert!(slots.contains(&((*hdr).materialized_elements.add(1) as usize)));
            assert_eq!(read(hdr, 1.0, b"id"), crate::value::TAG_HOLE);
            assert_eq!(read(hdr, 0.0, b"id"), 0.0f64.to_bits());
        });
    }
}

#[test]
fn json_scalar_memo_keeps_one_exact_key_and_declines_other_fields() {
    assert_ne!(property_id(b""), property_id(b"\0"));
    assert_ne!(property_id(b"a"), property_id(b"a\0"));
    assert!(property_id(b"1234567").is_some());
    assert!(property_id(b"12345678").is_none());
    unsafe {
        let hdr = array(br#"[{"id":7,"active":false},{"id":8,"active":true}]"#);
        assert_eq!(read(hdr, 0.0, b"id"), 7.0f64.to_bits());
        for index in [0.0, 1.0] {
            assert_eq!(read(hdr, index, b"active"), crate::value::TAG_HOLE);
        }
        assert_eq!(read(hdr, 1.0, b"id"), 8.0f64.to_bits());
        assert_eq!((*hdr).scalar_property, property_id(b"id").unwrap());
        assert_eq!(*(*hdr).materialized_bitmap, 0);
    }
}

#[test]
fn json_scalar_memo_preserves_stringify_and_materialized_records() {
    unsafe {
        let text = br#"[{"id":0,"active":true},{"id":7,"active":false}]"#;
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(text));
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            assert_eq!(read(hdr, 0.0, b"id"), 0.0f64.to_bits());
            assert_eq!(read(hdr, 1.0, b"id"), 7.0f64.to_bits());
        });
        let string = root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            crate::json::js_json_stringify(
                f64::from_bits(JSValue::object_ptr(hdr.cast()).bits()),
                0,
            )
        });
        let bytes = std::slice::from_raw_parts(
            crate::string::string_data(string),
            (*string).byte_len as usize,
        );
        assert_eq!(bytes, text);
        root.with_const_ptr(|hdr: *const LazyArrayHeader| assert!((*hdr).materialized.is_null()));
        root.with_mut_ptr(|hdr| force_materialize_lazy(hdr));
        let id = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
        let id = scope.root_string_ptr(id);
        for i in 0..2 {
            let record = root.with_mut_ptr(|hdr| lazy_get(hdr, i));
            let record = scope.root_nanbox_u64(record.bits());
            let number = id.with_const_ptr(|id| {
                crate::object::js_object_get_field_by_name(
                    JSValue::from_bits(record.get_nanbox_u64()).as_pointer(),
                    id,
                )
            });
            assert_eq!(
                number.bits(),
                if i == 0 { 0.0f64 } else { 7.0f64 }.to_bits()
            );
            assert_eq!(
                root.with_mut_ptr(|hdr| read(hdr, i as f64, b"id")),
                crate::value::TAG_HOLE
            );
        }
    }
}
