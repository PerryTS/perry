use super::*;

unsafe fn probe(hdr: *mut LazyArrayHeader, index: u32, key: &[u8]) -> u64 {
    js_array_index_own_number(
        f64::from_bits(JSValue::object_ptr(hdr.cast()).bits()),
        index,
        key.as_ptr(),
        key.len() as u64,
    )
    .to_bits()
}

#[test]
fn json_invariant_field_reads_pristine_exposed_and_materialized_without_allocating() {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(br#"[{"id":7},{"id":-0},{"id":3.25}]"#));
        for stage in 0..3 {
            if stage == 1 {
                root.with_mut_ptr(|hdr| lazy_get(hdr, 0));
            }
            if stage == 2 {
                root.with_mut_ptr(|hdr| force_materialize_lazy(hdr));
            }
            let allocated = crate::arena::arena_live_allocated_bytes();
            for (index, expected) in [7.0f64, -0.0, 3.25].into_iter().enumerate() {
                assert_eq!(
                    root.with_mut_ptr(|hdr| probe(hdr, index as u32, b"id")),
                    expected.to_bits(),
                    "stage {stage}"
                );
            }
            assert_eq!(crate::arena::arena_live_allocated_bytes(), allocated);
        }
    }
}

#[test]
fn json_invariant_field_declines_non_numbers_missing_and_handles() {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(
            br#"[{"id":"7"},{"id":null},{"id":true},{"x":1},{"id":{}},{"id":[]}]"#,
        ));
        for stage in 0..2 {
            if stage == 1 {
                root.with_mut_ptr(|hdr| force_materialize_lazy(hdr));
            }
            for index in 0..7 {
                assert_eq!(
                    root.with_mut_ptr(|hdr| probe(hdr, index, b"id")),
                    crate::value::TAG_HOLE
                );
            }
        }
        for bits in [
            0,
            crate::value::TAG_NULL,
            crate::value::TAG_UNDEFINED,
            crate::value::POINTER_TAG,
            crate::value::POINTER_TAG | crate::value::addr_class::FETCH_HANDLE_BAND_START as u64,
        ] {
            assert_eq!(
                js_array_index_own_number(f64::from_bits(bits), 0, b"id".as_ptr(), 2).to_bits(),
                crate::value::TAG_HOLE
            );
        }
    }
}

#[test]
fn json_invariant_field_admission_checks_descriptors_forwarding_and_class() {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let root = scope.root_raw_mut_ptr(array(br#"[{"id":7}]"#));
        root.with_mut_ptr(|hdr| force_materialize_lazy(hdr));
        root.with_mut_ptr(|hdr: *mut LazyArrayHeader| {
            assert_eq!(probe(hdr, 0, b"id"), 7.0f64.to_bits());
            let arr = (*hdr).materialized;
            let elem = dense_element(arr, 0).unwrap();
            let obj = elem.as_pointer::<crate::object::ObjectHeader>().cast_mut();
            for addr in [hdr as usize, arr as usize, obj as usize] {
                let header = crate::gc::header_from_trusted_user_ptr(addr as *const u8).cast_mut();
                let old = (*header)._reserved;
                for flag in [
                    crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
                    crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
                ] {
                    (*header)._reserved = old | flag;
                    assert_eq!(probe(hdr, 0, b"id"), crate::value::TAG_HOLE);
                }
                (*header)._reserved = old;
                let old = (*header).gc_flags;
                (*header).gc_flags = old | crate::gc::GC_FLAG_FORWARDED;
                assert_eq!(probe(hdr, 0, b"id"), crate::value::TAG_HOLE);
                (*header).gc_flags = old;
            }
            let class = (*obj).class_id;
            (*obj).class_id = 123;
            assert_eq!(probe(hdr, 0, b"id"), crate::value::TAG_HOLE);
            (*obj).class_id = class;
            assert_eq!(probe(hdr, 0, b"id"), 7.0f64.to_bits());
        });
    }
}

#[test]
fn json_invariant_field_normalizes_tagged_ints_and_preserves_ieee_numbers() {
    for n in [i32::MIN, -1, 0, i32::MAX] {
        assert_eq!(own_number(JSValue::int32(n)), n as f64);
    }
    for n in [0.0f64, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(own_number(JSValue::number(n)).to_bits(), n.to_bits());
    }
}
